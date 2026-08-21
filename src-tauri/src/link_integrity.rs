//! Persistent Link Integrity Engine.
//!
//! Mechanical reachability is deliberately separated from editorial claim
//! support. A successful HTTP response remains `verified_but_weak` and
//! `pending` until an explicit review is recorded against the exact source
//! fingerprint, claim context, normalized URL, and content hash.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use regex::Regex;
use reqwest::Url;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::app_paths::{checked_data_child_path, data_dir};
use crate::editorial_io::write_text_file;
use crate::sanitize::{sanitize_short, sanitize_text};
use crate::web_evidence::{
    fetch_web_evidence_inner, search_web_evidence_inner, WebEvidenceFetchRequest,
    WebEvidenceInteractionState, WebEvidenceMethod, WebEvidenceRecord, WebEvidenceSearchRequest,
    WebEvidenceState,
};
use crate::{
    LinkAuditResult, LinkAuditRow, LinkClassification, LinkCorrectionAction,
    LinkCorrectionCandidate, LinkCorrectionProposalRequest, LinkCrossReviewStatus,
    LinkEvidenceRedirect, LinkIntegrityListRequest, LinkIntegrityListResult,
    LinkIntegrityReviewRequest, LinkReviewDecision,
};

const SCHEMA_VERSION: &str = "link_evidence.v1";
const SOURCE_ARTIFACT: &str = "operator/current-editor";
pub(crate) const LINK_INTEGRITY_MAX_OCCURRENCES: usize = 30;
const MAX_CONTEXT_CHARS: usize = 360;
const MAX_REVIEW_NOTE_CHARS: usize = 1200;
const MAX_LIST_RESULTS: usize = 100;

static LINK_INTEGRITY_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug)]
struct ExtractedLink {
    start: usize,
    original_url: String,
    anchor_text: Option<String>,
    surrounding_text: String,
}

fn io_lock() -> &'static Mutex<()> {
    LINK_INTEGRITY_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn integrity_dir() -> Result<PathBuf, String> {
    let path = checked_data_child_path(&data_dir().join("evidence").join("link-integrity"))?;
    fs::create_dir_all(&path)
        .map_err(|error| format!("failed to create link-integrity directory: {error}"))?;
    Ok(path)
}

fn records_dir() -> Result<PathBuf, String> {
    let path = checked_data_child_path(&integrity_dir()?.join("records"))?;
    fs::create_dir_all(&path)
        .map_err(|error| format!("failed to create link-integrity records directory: {error}"))?;
    Ok(path)
}

fn is_valid_link_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn record_path(link_id: &str) -> Result<PathBuf, String> {
    if !is_valid_link_id(link_id) {
        return Err("invalid link-integrity id".to_string());
    }
    checked_data_child_path(&records_dir()?.join(format!("{link_id}.json")))
}

fn sha256(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}

fn load_record(link_id: &str) -> Result<LinkAuditRow, String> {
    let path = record_path(link_id)?;
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read link-integrity record: {error}"))?;
    let record = serde_json::from_str::<LinkAuditRow>(&raw)
        .map_err(|error| format!("failed to parse link-integrity record: {error}"))?;
    if record.schema_version != SCHEMA_VERSION || record.link_id != link_id {
        return Err("link-integrity record schema or identity mismatch".to_string());
    }
    Ok(record)
}

fn save_record_unlocked(record: &LinkAuditRow) -> Result<(), String> {
    if record.schema_version != SCHEMA_VERSION || !is_valid_link_id(&record.link_id) {
        return Err("refusing to persist an invalid link-integrity record".to_string());
    }
    let encoded = serde_json::to_string_pretty(record)
        .map_err(|error| format!("failed to serialize link-integrity record: {error}"))?;
    write_text_file(&record_path(&record.link_id)?, &encoded)
}

fn update_record<F>(link_id: &str, update: F) -> Result<LinkAuditRow, String>
where
    F: FnOnce(&mut LinkAuditRow) -> Result<(), String>,
{
    let _guard = io_lock()
        .lock()
        .map_err(|_| "link-integrity I/O lock poisoned".to_string())?;
    let mut record = load_record(link_id)?;
    update(&mut record)?;
    save_record_unlocked(&record)?;
    Ok(record)
}

fn save_audit_record(mut record: LinkAuditRow) -> Result<LinkAuditRow, String> {
    let _guard = io_lock()
        .lock()
        .map_err(|_| "link-integrity I/O lock poisoned".to_string())?;
    if let Ok(previous) = load_record(&record.link_id) {
        apply_preserved_review(&mut record, &previous);
    }
    save_record_unlocked(&record)?;
    Ok(record)
}

fn append_event(kind: &str, record: &LinkAuditRow) -> Result<(), String> {
    let line = serde_json::to_string(&json!({
        "schema_version": SCHEMA_VERSION,
        "kind": sanitize_short(kind, 64),
        "at": Utc::now().to_rfc3339(),
        "link_id": record.link_id,
        "source_fingerprint": record.source_fingerprint,
        "normalized_url": record.normalized_url,
        "sha256": record.sha256,
        "classification": record.classification,
        "cross_review_status": record.cross_review_status,
        "reviewed_by": record.reviewed_by,
    }))
    .map_err(|error| format!("failed to serialize link-integrity event: {error}"))?;
    let path = checked_data_child_path(&integrity_dir()?.join("events.ndjson"))?;
    let _guard = io_lock()
        .lock()
        .map_err(|_| "link-integrity I/O lock poisoned".to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("failed to open link-integrity ledger: {error}"))?;
    writeln!(file, "{line}")
        .map_err(|error| format!("failed to append link-integrity event: {error}"))?;
    file.flush()
        .map_err(|error| format!("failed to flush link-integrity event: {error}"))
}

fn char_boundary_before(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn char_boundary_after(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while index < value.len() && !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn surrounding_text(value: &str, start: usize, end: usize) -> String {
    let left = char_boundary_before(value, start.saturating_sub(MAX_CONTEXT_CHARS / 2));
    let right = char_boundary_after(value, (end + MAX_CONTEXT_CHARS / 2).min(value.len()));
    sanitize_text(value[left..right].split_whitespace().collect::<Vec<_>>().join(" "), 500)
}

fn strip_html(value: &str) -> String {
    let without_tags = Regex::new(r"(?is)<[^>]+>")
        .ok()
        .map(|regex| regex.replace_all(value, " ").into_owned())
        .unwrap_or_else(|| value.to_string());
    sanitize_text(
        &without_tags
            .replace("&nbsp;", " ")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        240,
    )
}

fn clean_url_tail(value: &str) -> String {
    value
        .trim()
        .trim_matches(['\"', '\'', '<', '>'])
        .trim_end_matches(['.', ',', ';', ':'])
        .to_string()
}

fn overlaps(start: usize, end: usize, spans: &[(usize, usize)]) -> bool {
    spans
        .iter()
        .any(|(covered_start, covered_end)| start < *covered_end && end > *covered_start)
}

fn extract_links(text: &str) -> Vec<ExtractedLink> {
    let mut links = Vec::new();
    let mut covered = Vec::new();

    if let Ok(markdown) = Regex::new(
        r#"(?s)\[([^\]\n]{0,240})\]\(\s*((?:[a-zA-Z][a-zA-Z0-9+.-]*:)[^)\s]+)(?:\s+[\"'][^\"']*[\"'])?\s*\)"#,
    ) {
        for capture in markdown.captures_iter(text) {
            let Some(whole) = capture.get(0) else { continue };
            let Some(url) = capture.get(2) else { continue };
            let anchor = capture
                .get(1)
                .map(|value| sanitize_text(value.as_str().trim(), 240))
                .filter(|value| !value.is_empty());
            links.push(ExtractedLink {
                start: whole.start(),
                original_url: clean_url_tail(url.as_str()),
                anchor_text: anchor,
                surrounding_text: surrounding_text(text, whole.start(), whole.end()),
            });
            covered.push((whole.start(), whole.end()));
        }
    }

    if let Ok(html) = Regex::new(
        r#"(?is)<a\b[^>]*\bhref\s*=\s*[\"']((?:[a-z][a-z0-9+.-]*:)[^\"']+)[\"'][^>]*>(.*?)</a>"#,
    ) {
        for capture in html.captures_iter(text) {
            let Some(whole) = capture.get(0) else { continue };
            if overlaps(whole.start(), whole.end(), &covered) {
                continue;
            }
            let Some(url) = capture.get(1) else { continue };
            let anchor = capture
                .get(2)
                .map(|value| strip_html(value.as_str()))
                .filter(|value| !value.is_empty());
            links.push(ExtractedLink {
                start: whole.start(),
                original_url: clean_url_tail(url.as_str()),
                anchor_text: anchor,
                surrounding_text: surrounding_text(text, whole.start(), whole.end()),
            });
            covered.push((whole.start(), whole.end()));
        }
    }

    if let Ok(bare) = Regex::new(
        r#"(?i)(?:https?://|mailto:|ftps?://|tel:|javascript:|data:|file:|blob:)[^\s<>\"')\]]+"#,
    ) {
        for matched in bare.find_iter(text) {
            if overlaps(matched.start(), matched.end(), &covered) {
                continue;
            }
            links.push(ExtractedLink {
                start: matched.start(),
                original_url: clean_url_tail(matched.as_str()),
                anchor_text: None,
                surrounding_text: surrounding_text(text, matched.start(), matched.end()),
            });
        }
    }

    links.sort_by_key(|link| link.start);
    links
}

pub(crate) fn count_link_occurrences(text: &str) -> usize {
    extract_links(text).len()
}

fn normalize_url(value: &str) -> Result<(String, Vec<String>), String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed
            .chars()
            .any(|character| character.is_control() || character == '\u{202e}')
    {
        return Err("URL vazia ou com caracteres de controle".to_string());
    }
    let parsed = Url::parse(trimmed).map_err(|_| "URL malformada ou incompleta".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err("somente http, https e mailto sao permitidos".to_string());
    }
    if matches!(parsed.scheme(), "http" | "https") {
        if parsed.host_str().is_none() {
            return Err("URL http/https sem host".to_string());
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err("credenciais embutidas na URL sao proibidas".to_string());
        }
    }
    let normalized = parsed.to_string();
    let mut changes = Vec::new();
    if trimmed != value {
        changes.push("whitespace_removed".to_string());
    }
    if normalized != trimmed {
        changes.push("url_parser_normalization".to_string());
    }
    Ok((normalized, changes))
}

fn link_id(
    source_fingerprint: &str,
    normalized_url: &str,
    anchor_text: Option<&str>,
    surrounding_text: &str,
    occurrence: usize,
) -> String {
    sha256(format!(
        "{SOURCE_ARTIFACT}|{source_fingerprint}|{normalized_url}|{}|{surrounding_text}|{occurrence}",
        anchor_text.unwrap_or_default(),
    ))
}

fn base_row(
    extracted: &ExtractedLink,
    source_fingerprint: &str,
    normalized_url: String,
    normalization_changes: Vec<String>,
    occurrence: usize,
) -> LinkAuditRow {
    let id = link_id(
        source_fingerprint,
        &normalized_url,
        extracted.anchor_text.as_deref(),
        &extracted.surrounding_text,
        occurrence,
    );
    LinkAuditRow {
        schema_version: SCHEMA_VERSION.to_string(),
        link_id: id,
        source_artifact: SOURCE_ARTIFACT.to_string(),
        source_fingerprint: source_fingerprint.to_string(),
        anchor_text: extracted.anchor_text.clone(),
        surrounding_text: extracted.surrounding_text.clone(),
        original_url: sanitize_text(&extracted.original_url, 1000),
        normalized_url: sanitize_text(&normalized_url, 1000),
        normalization_changes,
        final_url: None,
        redirect_chain: Vec::new(),
        http_status: None,
        content_type: None,
        sha256: None,
        checked_at: Utc::now().to_rfc3339(),
        claim_supported: None,
        classification: LinkClassification::VerifiedButWeak,
        correction_candidates: Vec::new(),
        cross_review_status: LinkCrossReviewStatus::Pending,
        review_decision: None,
        reviewed_by: None,
        review_note: None,
        reviewed_at: None,
        web_evidence_id: None,
        url: sanitize_text(&extracted.original_url, 240),
        status: "revisao editorial pendente".to_string(),
        invalidity: "acessibilidade mecanica ainda nao comprova suporte a afirmacao".to_string(),
        tone: "warn".to_string(),
    }
}

fn content_type_mismatch(url: &str, content_type: Option<&str>) -> bool {
    let path_is_pdf = Url::parse(url)
        .ok()
        .map(|url| url.path().to_ascii_lowercase().ends_with(".pdf"))
        .unwrap_or(false);
    let response_is_pdf = content_type
        .map(|value| value.to_ascii_lowercase().starts_with("application/pdf"))
        .unwrap_or(false);
    path_is_pdf != response_is_pdf && (path_is_pdf || response_is_pdf)
}

fn mechanical_failure_class(record: &WebEvidenceRecord) -> Option<LinkClassification> {
    if record.interaction_state == WebEvidenceInteractionState::CaptchaRequired {
        return Some(LinkClassification::CaptchaRequired);
    }
    if record.interaction_state == WebEvidenceInteractionState::LoginRequired {
        return Some(LinkClassification::AuthRequired);
    }
    if record.interaction_state == WebEvidenceInteractionState::Paywall {
        return Some(LinkClassification::Paywall);
    }
    match record.status {
        Some(401) => Some(LinkClassification::AuthRequired),
        Some(403) => Some(LinkClassification::Forbidden),
        Some(404 | 410) => Some(LinkClassification::NotFound),
        Some(status) if !(200..=299).contains(&status) => {
            Some(LinkClassification::SuspectedHallucination)
        }
        _ if record.state == WebEvidenceState::Blocked => Some(LinkClassification::Quarantined),
        _ if record.state == WebEvidenceState::Failed => {
            let notes = record.notes.join(" ").to_ascii_lowercase();
            if notes.contains("timed out") || notes.contains("timeout") {
                Some(LinkClassification::Timeout)
            } else if notes.contains("dns") || notes.contains("resolve") {
                Some(LinkClassification::DnsError)
            } else if notes.contains("tls") || notes.contains("certificate") {
                Some(LinkClassification::TlsError)
            } else {
                Some(LinkClassification::SuspectedHallucination)
            }
        }
        _ => None,
    }
}

fn apply_web_evidence(row: &mut LinkAuditRow, evidence: WebEvidenceRecord) {
    row.web_evidence_id = Some(evidence.id.clone());
    row.final_url = evidence.final_url.clone();
    row.redirect_chain = evidence
        .redirect_chain
        .iter()
        .map(|redirect| LinkEvidenceRedirect {
            url: sanitize_text(&redirect.url, 1000),
            status: redirect.status,
        })
        .collect();
    row.http_status = evidence.status;
    row.content_type = evidence.content_type.clone();
    row.sha256 = evidence.sha256.clone();
    row.checked_at = evidence
        .retrieved_at
        .clone()
        .unwrap_or_else(|| evidence.updated_at.clone());

    if let Some(classification) = mechanical_failure_class(&evidence) {
        row.classification = classification;
        row.cross_review_status = LinkCrossReviewStatus::Pending;
        row.status = evidence
            .status
            .map(|status| format!("HTTP {status}"))
            .unwrap_or_else(|| "falha mecanica".to_string());
        row.invalidity = evidence
            .notes
            .last()
            .map(|note| sanitize_text(note, 180))
            .unwrap_or_else(|| "o link nao passou pela verificacao mecanica".to_string());
        row.tone = if evidence.state == WebEvidenceState::Blocked {
            "blocked".to_string()
        } else {
            "error".to_string()
        };
        return;
    }

    if content_type_mismatch(&row.normalized_url, row.content_type.as_deref()) {
        row.classification = LinkClassification::ContentTypeMismatch;
        row.status = row
            .http_status
            .map(|status| format!("HTTP {status}"))
            .unwrap_or_else(|| "tipo divergente".to_string());
        row.invalidity = "o tipo de conteudo nao corresponde ao destino declarado".to_string();
        row.tone = "error".to_string();
        return;
    }

    let redirected = row
        .final_url
        .as_deref()
        .map(|final_url| final_url != row.normalized_url)
        .unwrap_or(false);
    row.classification = if redirected {
        LinkClassification::RedirectedVerified
    } else {
        LinkClassification::VerifiedButWeak
    };
    row.cross_review_status = LinkCrossReviewStatus::Pending;
    row.status = row
        .http_status
        .map(|status| format!("HTTP {status}"))
        .unwrap_or_else(|| "acessivel".to_string());
    row.invalidity = if redirected {
        "redirecionamento verificado; aceite editorial explicito ainda necessario".to_string()
    } else {
        "acessivel, mas suporte a afirmacao ainda nao foi julgado".to_string()
    };
    row.tone = "warn".to_string();
}

fn apply_preserved_review(row: &mut LinkAuditRow, previous: &LinkAuditRow) {
    if previous.source_fingerprint != row.source_fingerprint
        || previous.surrounding_text != row.surrounding_text
        || previous.anchor_text != row.anchor_text
        || previous.normalized_url != row.normalized_url
        || previous.sha256 != row.sha256
        || previous.review_decision.is_none()
    {
        return;
    }
    row.review_decision = previous.review_decision;
    row.reviewed_by = previous.reviewed_by.clone();
    row.review_note = previous.review_note.clone();
    row.reviewed_at = previous.reviewed_at.clone();
    row.correction_candidates = previous.correction_candidates.clone();
    match previous.review_decision {
        Some(LinkReviewDecision::Accept) => {
            row.claim_supported = Some(true);
            row.cross_review_status = LinkCrossReviewStatus::Accepted;
            row.classification = if row
                .final_url
                .as_deref()
                .map(|value| value != row.normalized_url)
                .unwrap_or(false)
            {
                LinkClassification::RedirectedVerified
            } else {
                LinkClassification::VerifiedSupportsClaim
            };
            row.invalidity =
                "suporte aceito explicitamente para esta afirmacao, URL e hash".to_string();
            row.tone = "ok".to_string();
        }
        Some(LinkReviewDecision::Reject) => {
            row.claim_supported = Some(false);
            row.cross_review_status = LinkCrossReviewStatus::Rejected;
            row.classification = LinkClassification::SuspectedHallucination;
            row.invalidity = "link rejeitado pela revisao editorial".to_string();
            row.tone = "error".to_string();
        }
        Some(LinkReviewDecision::Quarantine) => {
            row.claim_supported = Some(false);
            row.cross_review_status = LinkCrossReviewStatus::Rejected;
            row.classification = LinkClassification::Quarantined;
            row.invalidity = "link mantido em quarentena editorial".to_string();
            row.tone = "blocked".to_string();
        }
        None => {}
    }
}

fn malformed_row(
    extracted: &ExtractedLink,
    source_fingerprint: &str,
    occurrence: usize,
    error: &str,
) -> LinkAuditRow {
    let mut row = base_row(
        extracted,
        source_fingerprint,
        extracted.original_url.clone(),
        Vec::new(),
        occurrence,
    );
    row.classification = LinkClassification::Malformed;
    row.status = "URL invalida".to_string();
    row.invalidity = sanitize_text(error, 180);
    row.tone = "blocked".to_string();
    row
}

pub(crate) fn run_link_integrity_audit(text: &str) -> Result<LinkAuditResult, String> {
    let source_fingerprint = sha256(text.as_bytes());
    let checked_at = Utc::now().to_rfc3339();
    let mut occurrences = std::collections::BTreeMap::<String, usize>::new();
    let mut rows = Vec::new();
    let extracted_links = extract_links(text);
    if extracted_links.len() > LINK_INTEGRITY_MAX_OCCURRENCES {
        return Err(format!(
            "link-integrity capacity exceeded: found {} link occurrences; maximum is {}",
            extracted_links.len(),
            LINK_INTEGRITY_MAX_OCCURRENCES
        ));
    }

    for extracted in extracted_links {
        let normalized = normalize_url(&extracted.original_url);
        let occurrence_key = normalized
            .as_ref()
            .map(|(value, _)| value.clone())
            .unwrap_or_else(|_| extracted.original_url.clone());
        let occurrence = occurrences.entry(occurrence_key).or_insert(0);
        *occurrence += 1;
        let mut row = match normalized {
            Ok((normalized_url, changes)) => base_row(
                &extracted,
                &source_fingerprint,
                normalized_url,
                changes,
                *occurrence,
            ),
            Err(error) => {
                let row = save_audit_record(malformed_row(
                    &extracted,
                    &source_fingerprint,
                    *occurrence,
                    &error,
                ))?;
                append_event("audit", &row)?;
                rows.push(row);
                continue;
            }
        };

        if row.normalized_url.starts_with("mailto:") {
            row.status = "mailto sintaticamente valido".to_string();
            row.invalidity = "destino mailto requer julgamento editorial explicito".to_string();
            row.classification = LinkClassification::VerifiedButWeak;
            row.cross_review_status = LinkCrossReviewStatus::Pending;
            row.tone = "warn".to_string();
        } else {
            match fetch_web_evidence_inner(
                None,
                WebEvidenceFetchRequest {
                    url: row.normalized_url.clone(),
                    method: WebEvidenceMethod::Get,
                    force_revalidate: false,
                },
            ) {
                Ok(evidence) => apply_web_evidence(&mut row, evidence),
                Err(error) => {
                    let lower = error.to_ascii_lowercase();
                    row.classification = if lower.contains("timeout") {
                        LinkClassification::Timeout
                    } else if lower.contains("dns") || lower.contains("resolve") {
                        LinkClassification::DnsError
                    } else if lower.contains("tls") || lower.contains("certificate") {
                        LinkClassification::TlsError
                    } else {
                        LinkClassification::SuspectedHallucination
                    };
                    row.status = "falha mecanica".to_string();
                    row.invalidity = sanitize_text(&error, 180);
                    row.tone = "error".to_string();
                }
            }
        }
        let row = save_audit_record(row)?;
        append_event("audit", &row)?;
        rows.push(row);
    }

    let pending_review = rows
        .iter()
        .filter(|row| row.cross_review_status == LinkCrossReviewStatus::Pending)
        .count();
    let blocked = rows
        .iter()
        .filter(|row| row.tone == "blocked")
        .count();
    let failed = rows
        .iter()
        .filter(|row| matches!(row.tone.as_str(), "error" | "blocked"))
        .count();
    let ok = rows.iter().filter(|row| row.tone == "ok").count();
    let checked = rows
        .iter()
        .filter(|row| row.normalized_url.starts_with("http"))
        .count();
    Ok(LinkAuditResult {
        schema_version: "link_integrity_audit.v1".to_string(),
        audit_id: sha256(format!("{source_fingerprint}|{checked_at}")),
        source_artifact: SOURCE_ARTIFACT.to_string(),
        checked_at,
        urls_found: rows.len(),
        checked,
        ok,
        failed,
        pending_review,
        blocked,
        rows,
    })
}

pub(crate) fn list_link_integrity_records(
    request: LinkIntegrityListRequest,
) -> Result<LinkIntegrityListResult, String> {
    let query = request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let mut items = Vec::new();
    for entry in fs::read_dir(records_dir()?)
        .map_err(|error| format!("failed to list link-integrity records: {error}"))?
    {
        let entry = entry.map_err(|error| format!("failed to read link-integrity entry: {error}"))?;
        let Some(link_id) = entry
            .path()
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Ok(record) = load_record(&link_id) else {
            continue;
        };
        if !request.classifications.is_empty()
            && !request.classifications.contains(&record.classification)
        {
            continue;
        }
        if !request.cross_review_statuses.is_empty()
            && !request
                .cross_review_statuses
                .contains(&record.cross_review_status)
        {
            continue;
        }
        if request.needs_review_only
            && record.cross_review_status != LinkCrossReviewStatus::Pending
        {
            continue;
        }
        if let Some(source_artifact) = request.source_artifact.as_deref() {
            if record.source_artifact != source_artifact {
                continue;
            }
        }
        if let Some(query) = query.as_deref() {
            let haystack = format!(
                "{} {} {} {} {}",
                record.original_url,
                record.normalized_url,
                record.anchor_text.as_deref().unwrap_or_default(),
                record.surrounding_text,
                record.review_note.as_deref().unwrap_or_default()
            )
            .to_ascii_lowercase();
            if !haystack.contains(query) {
                continue;
            }
        }
        items.push(record);
    }
    items.sort_by(|left, right| {
        right
            .checked_at
            .cmp(&left.checked_at)
            .then_with(|| left.link_id.cmp(&right.link_id))
    });
    let total = items.len();
    let start = request
        .cursor
        .as_deref()
        .unwrap_or("0")
        .parse::<usize>()
        .map_err(|_| "invalid link-integrity cursor".to_string())?
        .min(total);
    let limit = request.limit.unwrap_or(30).clamp(1, MAX_LIST_RESULTS);
    let end = (start + limit).min(total);
    let page = items[start..end].to_vec();
    let next_cursor = (end < total).then(|| end.to_string());
    Ok(LinkIntegrityListResult {
        items: page,
        next_cursor,
        total,
    })
}

pub(crate) fn review_link_integrity(
    request: LinkIntegrityReviewRequest,
) -> Result<LinkAuditRow, String> {
    let reviewer = sanitize_short(request.reviewer.trim(), 64);
    if !matches!(
        reviewer.as_str(),
        "operator" | "claude" | "codex" | "gemini" | "agy" | "deepseek" | "grok" | "perplexity"
    ) {
        return Err("reviewer identity is not allowlisted".to_string());
    }
    let note = sanitize_text(request.note.trim(), MAX_REVIEW_NOTE_CHARS);
    if note.chars().count() < 10 {
        return Err("review note must contain at least 10 characters".to_string());
    }
    let link_id = request.link_id;
    let expected_normalized_url = request.expected_normalized_url;
    let expected_sha256 = request.expected_sha256;
    let decision = request.decision;
    let record = update_record(&link_id, move |record| {
        if record.normalized_url != expected_normalized_url
            || record.sha256 != expected_sha256
        {
            return Err(
                "link URL or content hash changed since it was read; reload before reviewing"
                    .to_string(),
            );
        }
        if decision == LinkReviewDecision::Accept {
            let reachable_http = record
                .http_status
                .map(|status| (200..=299).contains(&status))
                .unwrap_or(false);
            if !reachable_http && !record.normalized_url.starts_with("mailto:") {
                return Err(
                    "cannot accept a link that did not pass mechanical validation".to_string(),
                );
            }
            if record.classification == LinkClassification::ContentTypeMismatch {
                return Err(
                    "content-type mismatch must be corrected before acceptance".to_string(),
                );
            }
        }
        record.review_decision = Some(decision);
        record.reviewed_by = Some(reviewer);
        record.review_note = Some(note);
        record.reviewed_at = Some(Utc::now().to_rfc3339());
        match decision {
            LinkReviewDecision::Accept => {
                record.claim_supported = Some(true);
                record.cross_review_status = LinkCrossReviewStatus::Accepted;
                record.classification = if record
                    .final_url
                    .as_deref()
                    .map(|value| value != record.normalized_url)
                    .unwrap_or(false)
                {
                    LinkClassification::RedirectedVerified
                } else {
                    LinkClassification::VerifiedSupportsClaim
                };
                record.invalidity =
                    "suporte aceito explicitamente para esta afirmacao, URL e hash".to_string();
                record.tone = "ok".to_string();
            }
            LinkReviewDecision::Reject => {
                record.claim_supported = Some(false);
                record.cross_review_status = LinkCrossReviewStatus::Rejected;
                record.classification = LinkClassification::SuspectedHallucination;
                record.invalidity = "link rejeitado pela revisao editorial".to_string();
                record.tone = "error".to_string();
            }
            LinkReviewDecision::Quarantine => {
                record.claim_supported = Some(false);
                record.cross_review_status = LinkCrossReviewStatus::Rejected;
                record.classification = LinkClassification::Quarantined;
                record.invalidity = "link mantido em quarentena editorial".to_string();
                record.tone = "blocked".to_string();
            }
        }
        Ok(())
    })?;
    append_event("review", &record)?;
    Ok(record)
}

fn default_correction_query(record: &LinkAuditRow) -> String {
    let seed = record
        .anchor_text
        .as_deref()
        .filter(|value| value.chars().count() >= 4)
        .unwrap_or(&record.surrounding_text);
    sanitize_text(seed, 300)
}

pub(crate) fn propose_link_corrections(
    request: LinkCorrectionProposalRequest,
) -> Result<LinkAuditRow, String> {
    let record = load_record(&request.link_id)?;
    let expected_source_fingerprint = record.source_fingerprint.clone();
    let expected_normalized_url = record.normalized_url.clone();
    let expected_sha256 = record.sha256.clone();
    let provider = sanitize_short(request.provider.trim(), 80);
    if provider.is_empty() {
        return Err("correction provider is required".to_string());
    }
    let query = request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| sanitize_text(value, 300))
        .unwrap_or_else(|| default_correction_query(&record));
    if query.is_empty() {
        return Err("correction search query is empty".to_string());
    }
    let result = search_web_evidence_inner(
        None,
        WebEvidenceSearchRequest {
            query: query.clone(),
            provider: provider.clone(),
            limit: request.limit.unwrap_or(8).clamp(1, 12),
        },
    )?;
    let proposed_at = Utc::now().to_rfc3339();
    let mut candidates = result
        .items
        .into_iter()
        .map(|item| {
            let target = item.final_url.clone().unwrap_or_else(|| item.url.clone());
            LinkCorrectionCandidate {
                candidate_id: sha256(format!(
                    "{}|replace|{}|{}",
                    record.link_id, provider, target
                )),
                action: LinkCorrectionAction::Replace,
                url: Some(target),
                title: item.title,
                provider: provider.clone(),
                query: Some(query.clone()),
                web_evidence_id: Some(item.id),
                rationale: "candidato retornado por API oficial/configurada; exige revisao e nova verificacao mecanica"
                    .to_string(),
                proposed_at: proposed_at.clone(),
            }
        })
        .collect::<Vec<_>>();
    candidates.push(LinkCorrectionCandidate {
        candidate_id: sha256(format!("{}|remove", record.link_id)),
        action: LinkCorrectionAction::Remove,
        url: None,
        title: None,
        provider: "maestro".to_string(),
        query: None,
        web_evidence_id: None,
        rationale: "remover o link ou a afirmacao quando nenhuma fonte confiavel sustentar o trecho"
            .to_string(),
        proposed_at: proposed_at.clone(),
    });
    candidates.push(LinkCorrectionCandidate {
        candidate_id: sha256(format!("{}|reword", record.link_id)),
        action: LinkCorrectionAction::Reword,
        url: None,
        title: None,
        provider: "maestro".to_string(),
        query: None,
        web_evidence_id: None,
        rationale: "reescrever ou estreitar a afirmacao sem apresentar evidencia nao verificada"
            .to_string(),
        proposed_at,
    });
    let mut seen = BTreeSet::new();
    candidates.retain(|candidate| seen.insert(candidate.candidate_id.clone()));
    let record = update_record(&request.link_id, move |latest| {
        if latest.source_fingerprint != expected_source_fingerprint
            || latest.normalized_url != expected_normalized_url
            || latest.sha256 != expected_sha256
        {
            return Err(
                "link source, URL, or content hash changed during correction search; reload and retry"
                    .to_string(),
            );
        }
        latest.correction_candidates = candidates;
        Ok(())
    })?;
    append_event("correction_candidates", &record)?;
    Ok(record)
}

pub(crate) fn audit_requires_editorial_resolution(result: &LinkAuditResult) -> bool {
    result.failed > 0 || result.pending_review > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_preserves_markdown_anchor_and_context() {
        let links = extract_links("A fonte [documento oficial](https://example.com/a) sustenta a frase.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].anchor_text.as_deref(), Some("documento oficial"));
        assert!(links[0].surrounding_text.contains("sustenta a frase"));
    }

    #[test]
    fn extraction_does_not_duplicate_urls_inside_markup() {
        let links = extract_links(
            "[fonte](https://example.com/a) e <a href=\"https://example.com/b\">outra</a>",
        );
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn normalization_rejects_script_and_credentials() {
        assert!(normalize_url("javascript:alert(1)").is_err());
        assert!(normalize_url("ftp://files.example.com/archive.zip").is_err());
        assert!(normalize_url("tel:+5511999999999").is_err());
        assert!(normalize_url("https://user:secret@example.com/").is_err());
        assert!(normalize_url("https://example.com/a").is_ok());
        assert!(normalize_url("mailto:editor@example.com").is_ok());
    }

    #[test]
    fn unsupported_protocols_are_extracted_and_fail_closed() {
        let links = extract_links(
            "[arquivo](ftp://files.example.com/a.zip), tel:+5511999999999 e javascript:alert(1)",
        );
        assert_eq!(links.len(), 3);
        assert!(links
            .iter()
            .all(|link| normalize_url(&link.original_url).is_err()));
    }

    #[test]
    fn extraction_counts_every_occurrence_before_capacity_gate() {
        let text = (0..=LINK_INTEGRITY_MAX_OCCURRENCES)
            .map(|index| format!("[fonte {index}](https://example.com/source)"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            count_link_occurrences(&text),
            LINK_INTEGRITY_MAX_OCCURRENCES + 1
        );
    }

    #[test]
    fn link_identity_is_bound_to_the_exact_source_claim() {
        let extracted = ExtractedLink {
            start: 0,
            original_url: "https://example.com/source".to_string(),
            anchor_text: Some("fonte".to_string()),
            surrounding_text: "afirmacao A com fonte".to_string(),
        };
        let first = base_row(
            &extracted,
            &sha256("source A"),
            extracted.original_url.clone(),
            Vec::new(),
            1,
        );
        let second = base_row(
            &extracted,
            &sha256("source B"),
            extracted.original_url.clone(),
            Vec::new(),
            1,
        );
        assert_ne!(first.link_id, second.link_id);
    }

    #[test]
    fn reachable_evidence_is_not_automatically_claim_support() {
        let extracted = ExtractedLink {
            start: 0,
            original_url: "https://example.com/source".to_string(),
            anchor_text: Some("fonte".to_string()),
            surrounding_text: "afirmacao com fonte".to_string(),
        };
        let row = base_row(
            &extracted,
            &sha256("source"),
            extracted.original_url.clone(),
            Vec::new(),
            1,
        );
        assert_eq!(row.claim_supported, None);
        assert_eq!(row.classification, LinkClassification::VerifiedButWeak);
        assert_eq!(row.cross_review_status, LinkCrossReviewStatus::Pending);
    }
}
