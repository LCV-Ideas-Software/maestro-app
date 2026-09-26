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
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use regex::Regex;
use reqwest::Url;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::app_paths::{checked_data_child_path, data_dir};
use crate::editorial_io::write_text_file;
use crate::sanitize::{sanitize_short, sanitize_text};
use crate::web_evidence::{
    fetch_web_evidence_inner, rejected_url_for_record, search_web_evidence_inner,
    url_has_sensitive_parameters, WebEvidenceCacheState, WebEvidenceFetchRequest,
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
    url_start: usize,
    url_end: usize,
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
    Sha256::digest(value.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
    sanitize_text(
        &value[left..right]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        500,
    )
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

fn clean_bare_url_tail(value: &str) -> &str {
    let mut end = value.len();
    let opens = value.bytes().filter(|byte| *byte == b'(').count();
    let mut closes = value.bytes().filter(|byte| *byte == b')').count();
    loop {
        let candidate = &value[..end];
        let last = candidate.chars().next_back();
        let unmatched_close = last == Some(')') && closes > opens;
        if unmatched_close || matches!(last, Some('.' | ',' | ';' | ':' | ']' | '}')) {
            if last == Some(')') {
                closes -= 1;
            }
            end -= last.expect("trimmed tail has a character").len_utf8();
        } else {
            return candidate;
        }
    }
}

fn overlaps(start: usize, end: usize, spans: &[(usize, usize)]) -> bool {
    spans
        .iter()
        .any(|(covered_start, covered_end)| start < *covered_end && end > *covered_start)
}

fn extract_links(text: &str) -> Vec<ExtractedLink> {
    let mut links = Vec::new();
    let mut covered = Vec::new();
    let mut markdown_link: Option<(usize, String, String)> = None;
    let mut code_block_start = None;
    let parser = Parser::new(text);
    covered.extend(
        parser
            .reference_definitions()
            .iter()
            .map(|(_, definition)| (definition.span.start, definition.span.end)),
    );
    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code_block_start = Some(range.start),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = code_block_start.take() {
                    covered.push((start, range.end));
                }
            }
            Event::Code(label) => {
                covered.push((range.start, range.end));
                if let Some((_, _, anchor)) = markdown_link.as_mut() {
                    anchor.push_str(&label);
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                let html = &text[range.clone()];
                let mut cursor = 0;
                while let Some(relative_start) = html[cursor..].find("<!--") {
                    let start = cursor + relative_start;
                    let end = html[start + 4..]
                        .find("-->")
                        .map(|close| start + 4 + close + 3)
                        .unwrap_or(html.len());
                    covered.push((range.start + start, range.start + end));
                    cursor = end;
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                markdown_link = Some((range.start, dest_url.into_string(), String::new()));
            }
            Event::Text(label) => {
                if let Some((_, _, anchor)) = markdown_link.as_mut() {
                    anchor.push_str(&label);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((start, url, anchor)) = markdown_link.take() {
                    let end = range.end;
                    let source = &text[start..end];
                    let raw_range = source
                        .find(&url)
                        .map(|offset| (start + offset, start + offset + url.len()));
                    let (url_start, url_end) = raw_range.unwrap_or((start, end));
                    if !url.starts_with('#') {
                        links.push(ExtractedLink {
                            start,
                            url_start,
                            url_end,
                            original_url: url,
                            anchor_text: (!anchor.trim().is_empty())
                                .then(|| sanitize_text(anchor.trim(), 240)),
                            surrounding_text: surrounding_text(text, start, end),
                        });
                    }
                    covered.push((start, end));
                }
            }
            _ => {}
        }
    }

    if let Ok(html) = Regex::new(
        r#"(?is)<a\b[^>]*\bhref\s*=\s*[\"']((?:[a-z][a-z0-9+.-]*:)[^\"']+)[\"'][^>]*>(.*?)</a>"#,
    ) {
        for capture in html.captures_iter(text) {
            let Some(whole) = capture.get(0) else {
                continue;
            };
            // A comment inside an otherwise live anchor does not hide its href.
            // Anchors that begin inside a comment, code example, or Markdown
            // link are already represented by that enclosing span.
            if covered
                .iter()
                .any(|(start, end)| whole.start() >= *start && whole.start() < *end)
            {
                continue;
            }
            let Some(url) = capture.get(1) else { continue };
            let anchor = capture
                .get(2)
                .map(|value| strip_html(value.as_str()))
                .filter(|value| !value.is_empty());
            links.push(ExtractedLink {
                start: whole.start(),
                url_start: url.start(),
                url_end: url.end(),
                original_url: clean_url_tail(url.as_str()),
                anchor_text: anchor,
                surrounding_text: surrounding_text(text, whole.start(), whole.end()),
            });
            covered.push((whole.start(), whole.end()));
        }
    }

    if let Ok(bare) = Regex::new(
        r#"(?i)(?:https?://|mailto:|ftps?://|tel:|javascript:|data:|file:|blob:)[^\s<>\"']+"#,
    ) {
        for matched in bare.find_iter(text) {
            if overlaps(matched.start(), matched.end(), &covered) {
                continue;
            }
            let url = clean_bare_url_tail(matched.as_str());
            links.push(ExtractedLink {
                start: matched.start(),
                url_start: matched.start(),
                url_end: matched.start() + url.len(),
                original_url: clean_url_tail(url),
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
    if url_has_sensitive_parameters(&parsed) {
        return Err("parametro de credencial na URL e proibido".to_string());
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

fn same_network_url(left: &str, right: &str) -> bool {
    let (Ok(mut left), Ok(mut right)) = (Url::parse(left), Url::parse(right)) else {
        return false;
    };
    left.set_fragment(None);
    right.set_fragment(None);
    left == right
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
        mechanical_classification: None,
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
    if record.status == Some(403)
        && record.interaction_state == WebEvidenceInteractionState::LoginRequired
    {
        return Some(LinkClassification::Forbidden);
    }
    if record.interaction_state == WebEvidenceInteractionState::CaptchaRequired {
        return Some(LinkClassification::CaptchaRequired);
    }
    if record.interaction_state == WebEvidenceInteractionState::LoginRequired {
        return Some(LinkClassification::AuthRequired);
    }
    if record.interaction_state == WebEvidenceInteractionState::Paywall {
        return Some(LinkClassification::Paywall);
    }
    if !matches!(record.interaction_state, WebEvidenceInteractionState::None)
        && !(record.interaction_state == WebEvidenceInteractionState::HumanResolved
            && record.human_resolved)
    {
        return Some(LinkClassification::Quarantined);
    }
    if record.state == WebEvidenceState::Blocked {
        return Some(LinkClassification::Quarantined);
    }
    if record.state != WebEvidenceState::Ready && record.state != WebEvidenceState::Failed {
        return Some(LinkClassification::Quarantined);
    }
    if record.state == WebEvidenceState::Ready && record.cache_state != WebEvidenceCacheState::Fresh
    {
        return Some(LinkClassification::Quarantined);
    }
    match record.status {
        Some(401) => Some(LinkClassification::AuthRequired),
        Some(403) => Some(LinkClassification::Forbidden),
        Some(404 | 410) => Some(LinkClassification::NotFound),
        Some(status) if !(200..=299).contains(&status) => {
            Some(LinkClassification::SuspectedHallucination)
        }
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
        _ if record.status.is_none() => Some(LinkClassification::Quarantined),
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
        row.mechanical_classification = Some(classification);
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
        row.mechanical_classification = Some(LinkClassification::ContentTypeMismatch);
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
        .map(|final_url| !same_network_url(final_url, &row.normalized_url))
        .unwrap_or(false);
    row.classification = if redirected {
        LinkClassification::RedirectedVerified
    } else {
        LinkClassification::VerifiedButWeak
    };
    row.mechanical_classification = Some(row.classification);
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
        || previous.final_url != row.final_url
        || previous.redirect_chain != row.redirect_chain
        || previous.sha256 != row.sha256
        || previous.review_decision.is_none()
        || (previous.review_decision == Some(LinkReviewDecision::Accept)
            && !mechanically_acceptable(row))
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
                .map(|value| !same_network_url(value, &row.normalized_url))
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

fn valid_content_hash(hash: Option<&str>) -> bool {
    hash.is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn mechanically_acceptable(row: &LinkAuditRow) -> bool {
    if row.normalized_url.starts_with("mailto:") {
        return row.mechanical_classification == Some(LinkClassification::VerifiedButWeak);
    }
    row.http_status
        .is_some_and(|status| (200..=299).contains(&status))
        && valid_content_hash(row.sha256.as_deref())
        && matches!(
            row.mechanical_classification,
            Some(LinkClassification::VerifiedButWeak | LinkClassification::RedirectedVerified)
        )
}

fn normalized_url_is_safe_to_collect(url: &str) -> bool {
    sanitize_text(url, 1000) == url
}

fn redacted_extracted_link(extracted: &ExtractedLink) -> ExtractedLink {
    ExtractedLink {
        start: extracted.start,
        url_start: extracted.url_start,
        url_end: extracted.url_end,
        original_url: rejected_url_for_record(&extracted.original_url),
        anchor_text: None,
        surrounding_text: "<redacted context>".to_string(),
    }
}

fn source_with_rejected_urls_masked(text: &str, links: &[ExtractedLink]) -> String {
    let mut bytes = text.as_bytes().to_vec();
    for link in links {
        let rejected = normalize_url(&link.original_url)
            .map(|(normalized, _)| !normalized_url_is_safe_to_collect(&normalized))
            .unwrap_or(true);
        if rejected {
            for byte in &mut bytes[link.url_start..link.url_end] {
                if !matches!(*byte, b'\r' | b'\n') {
                    *byte = b' ';
                }
            }
        }
    }
    let literals = Regex::new(
        r#"(?i)(?:https?://|mailto:|ftps?://|tel:|javascript:|data:|file:|blob:)[^\s<>\"']+"#,
    )
    .expect("static URL literal pattern must compile");
    for found in literals.find_iter(text) {
        let literal = clean_bare_url_tail(found.as_str());
        let rejected = normalize_url(literal)
            .map(|(normalized, _)| !normalized_url_is_safe_to_collect(&normalized))
            .unwrap_or(true);
        if rejected {
            for byte in &mut bytes[found.start()..found.start() + literal.len()] {
                if !matches!(*byte, b'\r' | b'\n') {
                    *byte = b' ';
                }
            }
        }
    }
    String::from_utf8(bytes).expect("complete URL ranges preserve UTF-8 when masked")
}

fn safe_context_link(extracted: &ExtractedLink, masked_links: &[ExtractedLink]) -> ExtractedLink {
    masked_links
        .iter()
        .find(|candidate| {
            candidate.start == extracted.start && candidate.original_url == extracted.original_url
        })
        .cloned()
        .unwrap_or_else(|| {
            let mut safe = extracted.clone();
            safe.anchor_text = None;
            safe.surrounding_text = "<redacted context>".to_string();
            safe
        })
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
    let masked_text = source_with_rejected_urls_masked(text, &extracted_links);
    let masked_links = extract_links(&masked_text);
    if extracted_links.len() > LINK_INTEGRITY_MAX_OCCURRENCES {
        return Err(format!(
            "link-integrity capacity exceeded: found {} link occurrences; maximum is {}",
            extracted_links.len(),
            LINK_INTEGRITY_MAX_OCCURRENCES
        ));
    }

    for extracted in extracted_links {
        let normalized = normalize_url(&extracted.original_url);
        let rejected = normalized
            .as_ref()
            .map(|(url, _)| !normalized_url_is_safe_to_collect(url))
            .unwrap_or(true);
        let safe_extracted = if rejected {
            redacted_extracted_link(&extracted)
        } else {
            safe_context_link(&extracted, &masked_links)
        };
        let occurrence_key = if rejected {
            safe_extracted.original_url.clone()
        } else {
            normalized
                .as_ref()
                .map(|(value, _)| value.clone())
                .unwrap_or_default()
        };
        let occurrence = occurrences.entry(occurrence_key).or_insert(0);
        *occurrence += 1;
        let mut row = match normalized {
            Ok((normalized_url, changes)) => {
                if !normalized_url_is_safe_to_collect(&normalized_url) {
                    let rejected = redacted_extracted_link(&extracted);
                    let row = save_audit_record(malformed_row(
                        &rejected,
                        &source_fingerprint,
                        *occurrence,
                        "normalized URL would change during sanitization",
                    ))?;
                    append_event("audit", &row)?;
                    rows.push(row);
                    continue;
                }
                base_row(
                    &safe_extracted,
                    &source_fingerprint,
                    normalized_url,
                    changes,
                    *occurrence,
                )
            }
            Err(error) => {
                let row = save_audit_record(malformed_row(
                    &safe_extracted,
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
            row.mechanical_classification = Some(LinkClassification::VerifiedButWeak);
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
                    row.mechanical_classification = Some(row.classification);
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
    let blocked = rows.iter().filter(|row| row.tone == "blocked").count();
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
        let entry =
            entry.map_err(|error| format!("failed to read link-integrity entry: {error}"))?;
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
        if request.needs_review_only && record.cross_review_status != LinkCrossReviewStatus::Pending
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
        if record.normalized_url != expected_normalized_url || record.sha256 != expected_sha256 {
            return Err(
                "link URL or content hash changed since it was read; reload before reviewing"
                    .to_string(),
            );
        }
        if decision == LinkReviewDecision::Accept {
            if !mechanically_acceptable(record) {
                return Err(
                    "cannot accept a link that did not pass mechanical validation".to_string(),
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
                    .map(|value| !same_network_url(value, &record.normalized_url))
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
        rationale:
            "remover o link ou a afirmacao quando nenhuma fonte confiavel sustentar o trecho"
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
    use crate::web_evidence::{
        WebEvidenceAccessMode, WebEvidenceCacheState, WebEvidenceCopyrightState,
        WebEvidenceRobotsState,
    };

    fn test_row() -> LinkAuditRow {
        let extracted = ExtractedLink {
            start: 0,
            url_start: 0,
            url_end: 26,
            original_url: "https://example.com/source".to_string(),
            anchor_text: Some("fonte".to_string()),
            surrounding_text: "afirmacao com fonte".to_string(),
        };
        base_row(
            &extracted,
            &sha256("source"),
            extracted.original_url.clone(),
            Vec::new(),
            1,
        )
    }

    fn test_evidence() -> WebEvidenceRecord {
        WebEvidenceRecord {
            id: "evidence".to_string(),
            schema_version: "web_evidence.v1".to_string(),
            state: WebEvidenceState::Ready,
            url: "https://example.com/source".to_string(),
            method: WebEvidenceMethod::Get,
            access_mode: WebEvidenceAccessMode::HttpFetch,
            status: Some(200),
            final_url: Some("https://example.com/source".to_string()),
            title: None,
            content_type: Some("text/html".to_string()),
            sha256: Some("a".repeat(64)),
            retrieved_at: None,
            expires_at: None,
            cache_ttl: "test".to_string(),
            cache_state: WebEvidenceCacheState::Fresh,
            robots_state: WebEvidenceRobotsState::Allowed,
            copyright_state: WebEvidenceCopyrightState::Unknown,
            interaction_state: WebEvidenceInteractionState::None,
            human_resolved: false,
            byte_count: None,
            duration_ms: None,
            redirect_chain: Vec::new(),
            curl_command: None,
            provider: None,
            query: None,
            artifact_name: None,
            notes: Vec::new(),
            created_at: "2026-09-25T00:00:00Z".to_string(),
            updated_at: "2026-09-25T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn extraction_preserves_markdown_anchor_and_context() {
        let links =
            extract_links("A fonte [documento oficial](https://example.com/a) sustenta a frase.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].anchor_text.as_deref(), Some("documento oficial"));
        assert!(links[0].surrounding_text.contains("sustenta a frase"));
    }

    #[test]
    fn markdown_code_examples_are_not_live_links() {
        let text = "Example: `https://example.org/token`\n\n```text\nhttps://example.org/inside\n```\n\n<!-- https://example.org/hidden -->\n\n[real](https://example.org/live)";
        let links = extract_links(text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].original_url, "https://example.org/live");
    }

    #[test]
    fn comment_before_same_line_html_anchor_does_not_hide_anchor() {
        let text =
            "<!-- https://example.org/hidden --> <a href=\"https://example.org/live\">fonte</a>";
        let links = extract_links(text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].original_url, "https://example.org/live");
        let inside = "<a href=\"https://example.org/live\"><!-- nota -->fonte</a>";
        let links = extract_links(inside);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].original_url, "https://example.org/live");
    }

    #[test]
    fn extraction_preserves_balanced_parentheses_in_destinations() {
        for text in [
            "[fonte](https://example.org/article(v2))",
            "Veja https://example.org/article(v2).",
        ] {
            let links = extract_links(text);
            assert_eq!(links.len(), 1, "{text}");
            assert_eq!(links[0].original_url, "https://example.org/article(v2)");
        }
    }

    #[test]
    fn long_unmatched_punctuation_tail_is_removed_without_repeated_scans() {
        let text = format!("Veja https://example.org/article{}.", ")".repeat(20_000));
        let links = extract_links(&text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].original_url, "https://example.org/article");
    }

    #[test]
    fn malformed_markdown_destination_remains_a_blocked_link() {
        let result = run_link_integrity_audit("[fonte](http://)").unwrap();
        assert_eq!(result.urls_found, 1);
        assert_eq!(result.failed, 1);
    }

    #[test]
    fn reference_style_link_is_counted_once_at_its_use() {
        let text = "[fonte][manual]\n\n[manual]: https://example.org/article(v2)";
        let links = extract_links(text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].original_url, "https://example.org/article(v2)");
    }

    #[test]
    fn changed_redirect_destination_cannot_keep_editorial_acceptance() {
        let mut previous = test_row();
        previous.final_url = Some("https://example.com/old".to_string());
        previous.sha256 = Some("a".repeat(64));
        previous.review_decision = Some(LinkReviewDecision::Accept);
        let mut current = previous.clone();
        current.final_url = Some("https://example.com/new".to_string());
        current.review_decision = None;
        apply_preserved_review(&mut current, &previous);
        assert_eq!(current.review_decision, None);
        assert!(same_network_url(
            "https://example.com/source#monkey",
            "https://example.com/source"
        ));
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
        assert!(normalize_url("https://example.com/a?access_token=secret").is_err());
        assert!(normalize_url("https://example.com/a#access_token=secret").is_err());
        assert!(normalize_url("https://example.com/a?utm_source=x;access_token=secret").is_err());
        assert!(normalize_url("https://example.com/a").is_ok());
        assert!(normalize_url("mailto:editor@example.com").is_ok());
    }

    #[test]
    fn rejected_url_record_and_context_do_not_persist_credentials() {
        for url in [
            "https://user:secret@example.com/a?access_token=secret",
            "https://example.com/a?access_token=secret",
            "https://example.com/a#access_token=secret",
            "https://example.com/a?utm_source=x;access_token=secret",
        ] {
            assert!(normalize_url(url).is_err());
            let extracted = ExtractedLink {
                start: 0,
                url_start: 0,
                url_end: url.len(),
                original_url: url.to_string(),
                anchor_text: Some(url.to_string()),
                surrounding_text: format!("fonte {url}"),
            };
            let safe = redacted_extracted_link(&extracted);
            let row = malformed_row(&safe, &sha256(url), 1, "URL bloqueada");
            let serialized = serde_json::to_string(&row).unwrap();
            assert!(!serialized.contains("secret"));
            assert!(!serialized.contains("access_token"));
            assert!(!serialized.contains("user:"));
        }
    }

    #[test]
    fn accepted_link_context_redacts_neighboring_rejected_url() {
        let text = format!(
            "https://example.com/{}?access_token=secret https://example.com/public",
            "x".repeat(300)
        );
        let links = extract_links(&text);
        let public = links
            .iter()
            .find(|link| link.original_url == "https://example.com/public")
            .unwrap();
        let masked = source_with_rejected_urls_masked(&text, &links);
        let masked_links = extract_links(&masked);
        let safe = safe_context_link(public, &masked_links);
        let row = base_row(
            &safe,
            &sha256(&text),
            safe.original_url.clone(),
            Vec::new(),
            1,
        );
        let serialized = serde_json::to_string(&row).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("access_token"));
    }

    #[test]
    fn accepted_markdown_link_masks_rejected_url_in_anchor() {
        let text = "[https://example.com/private?access_token=secret](https://example.com/public)";
        let links = extract_links(text);
        assert_eq!(links.len(), 1);
        let masked = source_with_rejected_urls_masked(text, &links);
        let safe = safe_context_link(&links[0], &extract_links(&masked));
        let row = base_row(
            &safe,
            &sha256(text),
            safe.original_url.clone(),
            Vec::new(),
            1,
        );
        let serialized = serde_json::to_string(&row).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("access_token"));
    }

    #[test]
    fn rejected_links_from_one_origin_keep_distinct_occurrence_ids() {
        let first = ExtractedLink {
            start: 0,
            url_start: 0,
            url_end: "https://user:a@example.com/x".len(),
            original_url: "https://user:a@example.com/x".to_string(),
            anchor_text: None,
            surrounding_text: "first".to_string(),
        };
        let mut second = first.clone();
        second.original_url = "https://user:b@example.com/y".to_string();
        let first = redacted_extracted_link(&first);
        let second = redacted_extracted_link(&second);
        assert_eq!(first.original_url, second.original_url);
        let a = malformed_row(&first, &sha256("source"), 1, "blocked");
        let b = malformed_row(&second, &sha256("source"), 2, "blocked");
        assert_ne!(a.link_id, b.link_id);
        let long_a = format!("https://example.com/{}a", "x".repeat(1_001));
        let long_b = format!("https://example.com/{}b", "x".repeat(1_001));
        assert!(!normalized_url_is_safe_to_collect(&long_a));
        assert!(!normalized_url_is_safe_to_collect(&long_b));
        let redacted_a = rejected_url_for_record(&long_a);
        let redacted_b = rejected_url_for_record(&long_b);
        assert_eq!(redacted_a, redacted_b);
        let first = base_row(
            &redacted_extracted_link(&ExtractedLink {
                start: 0,
                url_start: 0,
                url_end: long_a.len(),
                original_url: long_a,
                anchor_text: None,
                surrounding_text: String::new(),
            }),
            &sha256("source"),
            redacted_a,
            Vec::new(),
            1,
        );
        let second = base_row(
            &redacted_extracted_link(&ExtractedLink {
                start: 0,
                url_start: 0,
                url_end: long_b.len(),
                original_url: long_b,
                anchor_text: None,
                surrounding_text: String::new(),
            }),
            &sha256("source"),
            redacted_b,
            Vec::new(),
            2,
        );
        assert_ne!(first.link_id, second.link_id);
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
            url_start: 0,
            url_end: 26,
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
            url_start: 0,
            url_end: 26,
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

    #[test]
    fn accept_and_preservation_require_current_mechanical_proof() {
        let mut row = test_row();
        apply_web_evidence(&mut row, test_evidence());
        assert!(mechanically_acceptable(&row));
        let mut previous = row.clone();
        previous.review_decision = Some(LinkReviewDecision::Accept);
        let mut refreshed = row.clone();
        refreshed.mechanical_classification = Some(LinkClassification::Quarantined);
        apply_preserved_review(&mut refreshed, &previous);
        assert_eq!(refreshed.review_decision, None);

        let mut blocked = test_evidence();
        blocked.state = WebEvidenceState::Blocked;
        let mut blocked_row = test_row();
        apply_web_evidence(&mut blocked_row, blocked);
        assert_eq!(
            blocked_row.mechanical_classification,
            Some(LinkClassification::Quarantined)
        );
        assert!(!mechanically_acceptable(&blocked_row));
    }

    #[test]
    fn collected_url_must_survive_sanitization_unchanged() {
        assert!(normalized_url_is_safe_to_collect(
            "https://example.com/source"
        ));
        assert!(!normalized_url_is_safe_to_collect(&format!(
            "https://example.com/{}",
            "a".repeat(1000)
        )));
    }

    #[test]
    fn http_accept_requires_valid_content_hash() {
        let mut row = test_row();
        apply_web_evidence(&mut row, test_evidence());
        row.sha256 = None;
        assert!(!mechanically_acceptable(&row));
        let mut previous = row.clone();
        previous.review_decision = Some(LinkReviewDecision::Accept);
        let mut refreshed = row.clone();
        apply_preserved_review(&mut refreshed, &previous);
        assert_eq!(refreshed.review_decision, None);
        row.sha256 = Some("short".to_string());
        assert!(!mechanically_acceptable(&row));
    }

    #[test]
    fn only_ready_resolved_evidence_is_mechanically_acceptable() {
        let mut evidence = test_evidence();
        for state in [
            WebEvidenceState::Queued,
            WebEvidenceState::Collecting,
            WebEvidenceState::Stale,
            WebEvidenceState::OperatorActionRequired,
        ] {
            evidence.state = state;
            assert!(mechanical_failure_class(&evidence).is_some());
        }
        evidence.state = WebEvidenceState::Ready;
        evidence.cache_state = WebEvidenceCacheState::Stale;
        assert!(mechanical_failure_class(&evidence).is_some());
        evidence.cache_state = WebEvidenceCacheState::Fresh;
        for interaction in [
            WebEvidenceInteractionState::ConsentRequired,
            WebEvidenceInteractionState::DownloadConfirmation,
            WebEvidenceInteractionState::HumanResolved,
        ] {
            evidence.interaction_state = interaction;
            assert!(mechanical_failure_class(&evidence).is_some());
        }
        evidence.interaction_state = WebEvidenceInteractionState::HumanResolved;
        evidence.human_resolved = true;
        assert_eq!(mechanical_failure_class(&evidence), None);
    }

    #[test]
    fn login_required_403_keeps_forbidden_classification() {
        let mut evidence = test_evidence();
        evidence.status = Some(403);
        evidence.interaction_state = WebEvidenceInteractionState::LoginRequired;
        assert_eq!(
            mechanical_failure_class(&evidence),
            Some(LinkClassification::Forbidden)
        );
        evidence.status = Some(401);
        assert_eq!(
            mechanical_failure_class(&evidence),
            Some(LinkClassification::AuthRequired)
        );
    }
}
