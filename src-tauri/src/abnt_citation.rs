//! Deterministic ABNT citation and reference gate.
//!
//! This module never invents bibliographic metadata. Free-text inspection is
//! deliberately conservative; complete formatting is available only when the
//! caller supplies a structured `citation_manifest.v1`.

use std::collections::{BTreeMap, BTreeSet};

use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sanitize::{sanitize_short, sanitize_text};
use crate::session_evidence::{read_attachment_bytes, AttachmentManifestEntry};

const RESULT_SCHEMA: &str = "maestro_peer.v1";
const CITATION_SCHEMA: &str = "citation.v1";
const MANIFEST_SCHEMA: &str = "citation_manifest.v1";
const MAX_TEXT_CHARS: usize = 2_000_000;
const MAX_CITATIONS: usize = 500;
const MAX_SOURCES: usize = 500;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CitationType {
    DirectQuote,
    IndirectQuote,
    Paraphrase,
    Apud,
    GenericMention,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CitationSourceAccess {
    FullDocumentOpened,
    ExcerptConsulted,
    ConsolidatedMemory,
    ContextualInference,
    UnverifiedHypothesis,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CitationVerificationStatus {
    Verified,
    NeedsEvidence,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CitationRisk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MaestroPeerStatus {
    Ready,
    NotReady,
    NeedsEvidence,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CitationSourceType {
    Book,
    Chapter,
    Article,
    Online,
    Other,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CitationAuditCitation {
    pub(crate) schema_version: String,
    pub(crate) claim_id: String,
    pub(crate) citation_type: CitationType,
    pub(crate) author_display: String,
    pub(crate) author_key: String,
    pub(crate) year: String,
    pub(crate) locator: Option<String>,
    pub(crate) source_id: String,
    pub(crate) source_access: CitationSourceAccess,
    pub(crate) verification_status: CitationVerificationStatus,
    pub(crate) risk_if_wrong: CitationRisk,
    #[serde(default)]
    pub(crate) original_text: Option<String>,
    #[serde(default)]
    pub(crate) normalized_text: Option<String>,
    #[serde(default)]
    pub(crate) normalized_footnote: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CitationAuthor {
    pub(crate) author_display: String,
    pub(crate) author_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CitationSource {
    pub(crate) source_id: String,
    pub(crate) source_type: CitationSourceType,
    #[serde(default)]
    pub(crate) authors: Vec<CitationAuthor>,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) subtitle: Option<String>,
    #[serde(default)]
    pub(crate) edition: Option<String>,
    #[serde(default)]
    pub(crate) place: Option<String>,
    #[serde(default)]
    pub(crate) publisher: Option<String>,
    pub(crate) year: String,
    #[serde(default)]
    pub(crate) container_title: Option<String>,
    #[serde(default)]
    pub(crate) volume: Option<String>,
    #[serde(default)]
    pub(crate) issue: Option<String>,
    #[serde(default)]
    pub(crate) pages: Option<String>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) doi: Option<String>,
    #[serde(default)]
    pub(crate) accessed_at: Option<String>,
    #[serde(default)]
    pub(crate) verification_sha256: Option<String>,
    pub(crate) verification_status: CitationVerificationStatus,
    #[serde(default)]
    pub(crate) prohibited: bool,
    #[serde(default)]
    pub(crate) quarantine_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CitationManifest {
    pub(crate) schema_version: String,
    pub(crate) protocol_hash: String,
    #[serde(default)]
    pub(crate) citations: Vec<CitationAuditCitation>,
    #[serde(default)]
    pub(crate) sources: Vec<CitationSource>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AbntAuditRequest {
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) protocol_hash: Option<String>,
    #[serde(default)]
    pub(crate) manifest: Option<CitationManifest>,
    #[serde(default)]
    pub(crate) previous_manifest: Option<CitationManifest>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CitationAuditBlocker {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) severity: String,
    pub(crate) claim_id: Option<String>,
    pub(crate) source_id: Option<String>,
    pub(crate) excerpt: Option<String>,
    pub(crate) needs_evidence: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CitationAuditResult {
    pub(crate) schema_version: String,
    pub(crate) audit_id: String,
    pub(crate) checked_at: String,
    pub(crate) protocol_hash: Option<String>,
    pub(crate) maestro_peer_status: MaestroPeerStatus,
    pub(crate) citations: Vec<CitationAuditCitation>,
    pub(crate) normalized_references: Vec<String>,
    pub(crate) markdown_references: Vec<String>,
    pub(crate) html_references: Vec<String>,
    pub(crate) blockers: Vec<CitationAuditBlocker>,
    pub(crate) audit_table_markdown: String,
    pub(crate) semantic_diff: String,
}

#[derive(Clone, Debug)]
struct RawReference {
    key: String,
    year: Option<String>,
    text: String,
}

pub(crate) struct CitationManifestAttachments {
    pub(crate) current: Option<CitationManifest>,
    pub(crate) previous: Option<CitationManifest>,
}

pub(crate) fn empty_citation_manifest(protocol_hash: &str) -> CitationManifest {
    CitationManifest {
        schema_version: MANIFEST_SCHEMA.to_string(),
        protocol_hash: sanitize_short(protocol_hash.trim(), 128),
        citations: Vec::new(),
        sources: Vec::new(),
    }
}

fn sha256(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}

fn blocker(
    code: &str,
    message: impl AsRef<str>,
    severity: &str,
    claim_id: Option<&str>,
    source_id: Option<&str>,
    excerpt: Option<&str>,
    needs_evidence: bool,
) -> CitationAuditBlocker {
    CitationAuditBlocker {
        code: sanitize_short(code, 80),
        message: sanitize_text(message.as_ref(), 500),
        severity: sanitize_short(severity, 20),
        claim_id: claim_id.map(|value| sanitize_short(value, 120)),
        source_id: source_id.map(|value| sanitize_short(value, 120)),
        excerpt: excerpt.map(|value| sanitize_text(value, 360)),
        needs_evidence,
    }
}

fn ascii_fold(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter_map(|character| match character {
            'a'..='z' | '0'..='9' => Some(character),
            'á' | 'à' | 'ã' | 'â' | 'ä' => Some('a'),
            'é' | 'è' | 'ê' | 'ë' => Some('e'),
            'í' | 'ì' | 'î' | 'ï' => Some('i'),
            'ó' | 'ò' | 'õ' | 'ô' | 'ö' => Some('o'),
            'ú' | 'ù' | 'û' | 'ü' => Some('u'),
            'ç' => Some('c'),
            _ => None,
        })
        .collect()
}

fn canonical_author_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

fn displayed_surname(author: &CitationAuditCitation) -> String {
    author
        .author_display
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| sanitize_text(value, 160))
        .unwrap_or_else(|| sanitize_text(author.author_key.trim(), 160))
}

fn source_surname(source: &CitationSource) -> String {
    source
        .authors
        .first()
        .map(|author| {
            author
                .author_display
                .split(',')
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| sanitize_text(value, 160))
                .unwrap_or_else(|| sanitize_text(author.author_key.trim(), 160))
        })
        .unwrap_or_default()
}

fn valid_year(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() == 4 && bytes.iter().all(|byte| byte.is_ascii_digit()))
        || (bytes.len() == 5
            && bytes[..4].iter().all(|byte| byte.is_ascii_digit())
            && bytes[4].is_ascii_alphabetic())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn locator_is_valid(locator: Option<&str>) -> bool {
    let Some(locator) = locator.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    Regex::new(r"(?i)\b(?:p{1,2}\.|par\.|cap\.|v\.|n\.|item|se[cç][aã]o)\s*[a-z0-9ivxlcdm]+")
        .map(|pattern| pattern.is_match(locator))
        .unwrap_or(false)
}

fn has_direct_quote_context(text: &str, start: usize) -> bool {
    let begin = text[..start].char_indices().rev().nth(220).map(|(at, _)| at).unwrap_or(0);
    let context = &text[begin..start];
    let trimmed = context.trim_end();
    if trimmed.ends_with('”') || trimmed.ends_with('"') {
        return true;
    }
    context
        .lines()
        .last()
        .map(|line| line.trim_start().starts_with('>'))
        .unwrap_or(false)
}

fn raw_citations(text: &str) -> Vec<CitationAuditCitation> {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    let patterns = [
        r"(?i)\(([\p{L}][\p{L}\s.'’\-]{1,80}),\s*((?:18|19|20)\d{2}[a-z]?)(?:,\s*([^)]+))?\)",
        r"\b([A-ZÁÀÃÂÉÊÍÓÔÕÚÇ][\p{L}'’\-]+(?:\s+(?:e|da|de|do|dos|das|[A-ZÁÀÃÂÉÊÍÓÔÕÚÇ][\p{L}'’\-]+)){0,3})\s+\(((?:18|19|20)\d{2}[a-z]?)(?:,\s*([^)]+))?\)",
    ];
    for raw_pattern in patterns {
        let Ok(pattern) = Regex::new(raw_pattern) else {
            continue;
        };
        for capture in pattern.captures_iter(text) {
            let Some(whole) = capture.get(0) else {
                continue;
            };
            if !seen.insert((whole.start(), whole.end())) || rows.len() >= MAX_CITATIONS {
                continue;
            }
            let author = capture.get(1).map(|value| value.as_str()).unwrap_or_default();
            let year = capture.get(2).map(|value| value.as_str()).unwrap_or_default();
            let locator = capture
                .get(3)
                .map(|value| sanitize_text(value.as_str().trim(), 80))
                .filter(|value| !value.is_empty());
            let author_display = sanitize_text(author.trim(), 160);
            let author_key = canonical_author_key(&author_display);
            let source_id = format!("source-{}-{year}", ascii_fold(&author_key));
            let direct = has_direct_quote_context(text, whole.start());
            let normalized = if let Some(locator) = locator.as_deref() {
                format!("({author_display}, {year}, {locator})")
            } else {
                format!("({author_display}, {year})")
            };
            rows.push(CitationAuditCitation {
                schema_version: CITATION_SCHEMA.to_string(),
                claim_id: sha256(format!("{}|{}|{}", whole.start(), whole.end(), whole.as_str())),
                citation_type: if direct {
                    CitationType::DirectQuote
                } else {
                    CitationType::IndirectQuote
                },
                author_display,
                author_key,
                year: year.to_string(),
                locator,
                source_id,
                source_access: CitationSourceAccess::UnverifiedHypothesis,
                verification_status: CitationVerificationStatus::NeedsEvidence,
                risk_if_wrong: CitationRisk::Medium,
                original_text: Some(sanitize_text(whole.as_str(), 240)),
                normalized_text: Some(normalized),
                normalized_footnote: None,
            });
        }
    }
    rows.sort_by(|left, right| left.claim_id.cmp(&right.claim_id));
    rows
}

fn reference_section(text: &str) -> Vec<RawReference> {
    let Ok(heading) = Regex::new(
        r"(?im)^#{1,6}\s*(?:refer[eê]ncias(?:\s+bibliogr[aá]ficas)?|bibliografia)\s*$",
    ) else {
        return Vec::new();
    };
    let Some(found) = heading.find(text) else {
        return Vec::new();
    };
    let body = &text[found.end()..];
    let year_pattern = Regex::new(r"(?i)\b((?:18|19|20)\d{2}[a-z]?)\b").ok();
    body.lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('#'))
        .filter(|line| !line.is_empty())
        .map(|line| line.trim_start_matches(['-', '*']).trim())
        .filter(|line| !line.is_empty())
        .take(MAX_SOURCES)
        .map(|line| {
            let author = line.split('.').next().unwrap_or_default();
            let key = author.split(',').next().unwrap_or(author);
            let year = year_pattern
                .as_ref()
                .and_then(|pattern| pattern.captures(line))
                .and_then(|capture| capture.get(1))
                .map(|value| value.as_str().to_string());
            RawReference {
                key: ascii_fold(key),
                year,
                text: sanitize_text(line, 1200),
            }
        })
        .collect()
}

fn quote_blockers(text: &str, citations: &[CitationAuditCitation]) -> Vec<CitationAuditBlocker> {
    let Ok(pattern) = Regex::new(r#"[“\"]([^“”\"\n]{12,400})[”\"]"#) else {
        return Vec::new();
    };
    let mut blockers = Vec::new();
    for found in pattern.find_iter(text).take(MAX_CITATIONS) {
        if found.as_str().starts_with('"') {
            let before = &text[..found.start()];
            let inside_html_tag = match (before.rfind('<'), before.rfind('>')) {
                (Some(_), None) => true,
                (Some(open), Some(close)) => open > close,
                _ => false,
            };
            if inside_html_tag || before.trim_end().ends_with('=') {
                continue;
            }
        }
        if found.as_str().split_whitespace().count() < 4 {
            continue;
        }
        let after_end = text[found.end()..]
            .char_indices()
            .nth(220)
            .map(|(offset, _)| found.end() + offset)
            .unwrap_or(text.len());
        let nearby = &text[found.end()..after_end];
        let cited = citations.iter().any(|citation| {
            [
                citation.original_text.as_deref(),
                citation.normalized_text.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|candidate| nearby.contains(candidate))
        });
        if !cited {
            blockers.push(blocker(
                "direct_quote_without_citation",
                "Trecho entre aspas nao esta ligado a uma citacao estruturada proxima.",
                "error",
                None,
                None,
                Some(found.as_str()),
                true,
            ));
        }
    }
    blockers
}

fn unstructured_citation_signals(text: &str) -> Vec<String> {
    let patterns = [
        r"(?i)<(?:cite|blockquote|q)\b[^>]*>",
        r"(?m)\[\^[^\]\r\n]{1,80}\]",
        r"(?i)\b(?:apud|ibidem|ibid\.|idem|op\.\s*cit\.)\b",
    ];
    let mut signals = BTreeSet::new();
    for raw_pattern in patterns {
        let Ok(pattern) = Regex::new(raw_pattern) else {
            continue;
        };
        for found in pattern.find_iter(text).take(MAX_CITATIONS) {
            signals.insert(sanitize_text(found.as_str(), 240));
        }
    }
    signals.into_iter().collect()
}

fn document_policy_blockers(
    text: &str,
    citations: &[CitationAuditCitation],
) -> Vec<CitationAuditBlocker> {
    let mut blockers = quote_blockers(text, citations);
    let folded = ascii_fold(text);
    if folded.contains("wikipediaorg") || folded.contains("ptwikipediaorg") {
        blockers.push(blocker(
            "prohibited_source",
            "Wikipedia foi detectada como suporte bibliografico e exige remocao ou substituicao por fonte permitida.",
            "error",
            None,
            None,
            None,
            false,
        ));
    }
    if folded.contains("protocoloeditorialv")
        || folded.contains("deacordocomoprotocoloeditorial")
        || folded.contains("nesteprotocolo")
        || folded.contains("esteprotocolo")
    {
        blockers.push(blocker(
            "public_protocol_self_reference",
            "O texto publico contem autorreferencia ao protocolo editorial.",
            "error",
            None,
            None,
            None,
            false,
        ));
    }
    let mut last_apparatus_rank = None;
    for line in text.lines() {
        let heading = ascii_fold(line.trim_start_matches('#').trim());
        let rank = if heading == "referencias" || heading == "referenciasbibliograficas" {
            Some(0u8)
        } else if heading == "fontesonline"
            || heading == "fontesconsultaveisonline"
            || heading == "fontesconsultadasonline"
        {
            Some(1u8)
        } else if heading == "leiturascomplementares" {
            Some(2u8)
        } else {
            None
        };
        if let Some(rank) = rank {
            if last_apparatus_rank.map(|last| rank < last).unwrap_or(false) {
                blockers.push(blocker(
                    "bibliographic_apparatus_order_invalid",
                    "A ordem do aparato deve ser referencias ABNT, fontes consultaveis online e leituras complementares.",
                    "error",
                    None,
                    None,
                    Some(line),
                    false,
                ));
                break;
            }
            last_apparatus_rank = Some(rank);
        }
    }
    blockers
}

fn raw_text_blockers(
    text: &str,
    citations: &[CitationAuditCitation],
    references: &[RawReference],
) -> Vec<CitationAuditBlocker> {
    let mut blockers = document_policy_blockers(text, citations);
    if !citations.is_empty() && references.is_empty() {
        blockers.push(blocker(
            "reference_section_missing",
            "O texto contem citacoes autor-data, mas nao possui secao final de referencias.",
            "error",
            None,
            None,
            None,
            true,
        ));
    }
    let mut used_reference_indexes = BTreeSet::new();
    for citation in citations {
        if citation.citation_type == CitationType::DirectQuote
            && !locator_is_valid(citation.locator.as_deref())
        {
            blockers.push(blocker(
                "direct_quote_locator_missing",
                "Citacao direta requer localizador verificavel.",
                "error",
                Some(&citation.claim_id),
                Some(&citation.source_id),
                citation.original_text.as_deref(),
                true,
            ));
        }
        let author_key = ascii_fold(&citation.author_key);
        let first_author_token = citation
            .author_key
            .split_whitespace()
            .next()
            .map(ascii_fold)
            .unwrap_or_default();
        let matched = references.iter().enumerate().find(|(_, reference)| {
            (reference.key.contains(&author_key)
                || (first_author_token.len() >= 4
                    && reference.key.contains(&first_author_token)))
                && reference.year.as_deref() == Some(citation.year.as_str())
        });
        if let Some((index, _)) = matched {
            used_reference_indexes.insert(index);
        } else if !references.is_empty() {
            blockers.push(blocker(
                "citation_without_reference",
                "Citacao no corpo nao possui referencia final inequivoca com autor e ano correspondentes.",
                "error",
                Some(&citation.claim_id),
                Some(&citation.source_id),
                citation.original_text.as_deref(),
                true,
            ));
        }
    }
    for (index, reference) in references.iter().enumerate() {
        if !used_reference_indexes.contains(&index) {
            blockers.push(blocker(
                "reference_without_body_use",
                "Referencia final nao possui citacao correspondente no corpo.",
                "error",
                None,
                None,
                Some(&reference.text),
                false,
            ));
        }
        if reference.year.is_none() || reference.text.matches('.').count() < 2 {
            blockers.push(blocker(
                "reference_required_fields_missing",
                "Referencia em texto livre nao apresenta campos mecanicamente suficientes; forneca manifesto estruturado.",
                "error",
                None,
                None,
                Some(&reference.text),
                true,
            ));
        }
    }
    blockers
}

fn required_source_fields(source: &CitationSource) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if source.authors.is_empty() {
        missing.push("authors");
    }
    if source.title.trim().is_empty() {
        missing.push("title");
    }
    if !valid_year(source.year.trim()) {
        missing.push("year");
    }
    if source.verification_status == CitationVerificationStatus::Verified
        && !source
            .verification_sha256
            .as_deref()
            .map(str::trim)
            .map(valid_sha256)
            .unwrap_or(false)
    {
        missing.push("verification_sha256");
    }
    match source.source_type {
        CitationSourceType::Book => {
            if source.place.as_deref().unwrap_or_default().trim().is_empty() {
                missing.push("place");
            }
            if source.publisher.as_deref().unwrap_or_default().trim().is_empty() {
                missing.push("publisher");
            }
        }
        CitationSourceType::Chapter => {
            if source
                .container_title
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                missing.push("container_title");
            }
            if source.pages.as_deref().unwrap_or_default().trim().is_empty() {
                missing.push("pages");
            }
            if source.place.as_deref().unwrap_or_default().trim().is_empty() {
                missing.push("place");
            }
            if source.publisher.as_deref().unwrap_or_default().trim().is_empty() {
                missing.push("publisher");
            }
        }
        CitationSourceType::Article => {
            if source
                .container_title
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                missing.push("container_title");
            }
        }
        CitationSourceType::Online => {
            if source.url.as_deref().unwrap_or_default().trim().is_empty() {
                missing.push("url");
            }
            if source
                .accessed_at
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                missing.push("accessed_at");
            }
        }
        CitationSourceType::Other => {}
    }
    missing
}

fn author_text(authors: &[CitationAuthor]) -> String {
    authors
        .iter()
        .map(|author| {
            let key = canonical_author_key(&author.author_key);
            let given_names = author
                .author_display
                .split_once(',')
                .map(|(_, names)| names.trim())
                .unwrap_or_default();
            if given_names.is_empty() {
                key
            } else {
                format!("{key}, {}", sanitize_text(given_names, 180))
            }
        })
        .filter(|author| !author.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

fn format_in_text_citation(
    citation: &CitationAuditCitation,
    source: &CitationSource,
) -> String {
    let author = displayed_surname(citation);
    let locator = citation
        .locator
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| sanitize_text(value, 100));
    match citation.citation_type {
        CitationType::Apud => {
            let consulted_author = source_surname(source);
            let locator_suffix = locator
                .as_deref()
                .map(|value| format!(", {value}"))
                .unwrap_or_default();
            format!(
                "({author}, {}, apud {consulted_author}, {}{locator_suffix})",
                citation.year.trim(),
                source.year.trim()
            )
        }
        CitationType::GenericMention => format!("{author} ({})", citation.year.trim()),
        CitationType::DirectQuote | CitationType::IndirectQuote | CitationType::Paraphrase => {
            let locator_suffix = locator
                .as_deref()
                .map(|value| format!(", {value}"))
                .unwrap_or_default();
            format!("({author}, {}{locator_suffix})", citation.year.trim())
        }
    }
}

fn format_footnote(source: &CitationSource, locator: Option<&str>) -> String {
    let mut note = format_reference(source);
    if let Some(locator) = locator.map(str::trim).filter(|value| !value.is_empty()) {
        note.push(' ');
        note.push_str(&sanitize_text(locator, 100));
        if !note.ends_with('.') {
            note.push('.');
        }
    }
    note
}

fn format_reference(source: &CitationSource) -> String {
    let mut parts = Vec::new();
    let authors = author_text(&source.authors);
    if !authors.is_empty() {
        parts.push(format!("{authors}."));
    }
    let mut title = sanitize_text(source.title.trim(), 500);
    if let Some(subtitle) = source.subtitle.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        title.push_str(": ");
        title.push_str(&sanitize_text(subtitle, 300));
    }
    if !title.is_empty() {
        parts.push(format!("{title}."));
    }
    if let Some(edition) = source.edition.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        parts.push(format!("{}.", sanitize_text(edition, 80)));
    }
    if let Some(container) = source
        .container_title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("In: {}.", sanitize_text(container, 400)));
    }
    let place = source.place.as_deref().unwrap_or_default().trim();
    let publisher = source.publisher.as_deref().unwrap_or_default().trim();
    if source.source_type == CitationSourceType::Article {
        let mut publication = Vec::new();
        if !place.is_empty() {
            publication.push(sanitize_text(place, 160));
        }
        if let Some(volume) = source.volume.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            publication.push(format!("v. {}", sanitize_text(volume, 80)));
        }
        if let Some(issue) = source.issue.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            publication.push(format!("n. {}", sanitize_text(issue, 80)));
        }
        if let Some(pages) = source.pages.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            publication.push(format!("p. {}", sanitize_text(pages, 100)));
        }
        if valid_year(source.year.trim()) {
            publication.push(source.year.trim().to_string());
        }
        if !publication.is_empty() {
            parts.push(format!("{}.", publication.join(", ")));
        }
    } else {
        let publication = format!(
            "{}{}{}",
            sanitize_text(place, 160),
            if !place.is_empty() && !publisher.is_empty() { ": " } else { "" },
            sanitize_text(publisher, 240)
        );
        if !publication.is_empty() && valid_year(source.year.trim()) {
            parts.push(format!("{publication}, {}.", source.year.trim()));
        } else if !publication.is_empty() {
            parts.push(format!("{publication}."));
        } else if valid_year(source.year.trim()) {
            parts.push(format!("{}.", source.year.trim()));
        }
        if let Some(volume) = source.volume.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            parts.push(format!("v. {}.", sanitize_text(volume, 80)));
        }
        if let Some(issue) = source.issue.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            parts.push(format!("n. {}.", sanitize_text(issue, 80)));
        }
        if let Some(pages) = source.pages.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            parts.push(format!("p. {}.", sanitize_text(pages, 100)));
        }
    }
    if let Some(doi) = source.doi.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        parts.push(format!("DOI: {}.", sanitize_text(doi, 240)));
    }
    if let Some(url) = source.url.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        parts.push(format!("Disponivel em: {}.", sanitize_text(url, 1000)));
    }
    if let Some(accessed) = source
        .accessed_at
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("Acesso em: {}.", sanitize_text(accessed, 120)));
    }
    parts.join(" ")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn validate_manifest(
    text: &str,
    raw_citations: &[CitationAuditCitation],
    raw_references: &[RawReference],
    request_protocol_hash: Option<&str>,
    manifest: &CitationManifest,
    blockers: &mut Vec<CitationAuditBlocker>,
) -> (Vec<CitationAuditCitation>, Vec<String>) {
    if manifest.schema_version != MANIFEST_SCHEMA {
        blockers.push(blocker(
            "manifest_schema_invalid",
            "O manifesto de citacoes nao usa citation_manifest.v1.",
            "error",
            None,
            None,
            None,
            false,
        ));
    }
    if let Some(expected) = request_protocol_hash {
        if expected != manifest.protocol_hash {
            blockers.push(blocker(
                "protocol_hash_mismatch",
                "O manifesto nao esta vinculado ao hash do protocolo ativo.",
                "error",
                None,
                None,
                None,
                false,
            ));
        }
    }
    if manifest.citations.len() > MAX_CITATIONS || manifest.sources.len() > MAX_SOURCES {
        blockers.push(blocker(
            "manifest_capacity_exceeded",
            "O manifesto excede o limite seguro de citacoes ou fontes.",
            "error",
            None,
            None,
            None,
            false,
        ));
    }
    if manifest.protocol_hash.trim().is_empty() {
        blockers.push(blocker(
            "manifest_protocol_hash_missing",
            "O manifesto nao registra o hash do protocolo editorial ativo.",
            "error",
            None,
            None,
            None,
            false,
        ));
    }
    let mut sources = BTreeMap::new();
    for source in manifest.sources.iter().take(MAX_SOURCES) {
        if source.source_id.trim().is_empty() {
            blockers.push(blocker(
                "source_id_missing",
                "Uma fonte estruturada nao possui source_id.",
                "error",
                None,
                None,
                None,
                false,
            ));
            continue;
        }
        if sources.insert(source.source_id.as_str(), source).is_some() {
            blockers.push(blocker(
                "source_id_duplicate",
                "O manifesto contem source_id duplicado.",
                "error",
                None,
                Some(&source.source_id),
                None,
                false,
            ));
        }
    }
    let mut used_sources = BTreeSet::new();
    let mut seen_claims = BTreeSet::new();
    let mut citations = Vec::new();
    let folded_text = ascii_fold(text);
    for citation in manifest.citations.iter().take(MAX_CITATIONS) {
        let claim_id = sanitize_short(&citation.claim_id, 120);
        let source_id = sanitize_short(&citation.source_id, 120);
        if claim_id.trim().is_empty() {
            blockers.push(blocker(
                "claim_id_missing",
                "Uma citacao estruturada nao possui claim_id.",
                "error",
                None,
                Some(&source_id),
                citation.original_text.as_deref(),
                false,
            ));
        } else if !seen_claims.insert(claim_id.clone()) {
            blockers.push(blocker(
                "claim_id_duplicate",
                "O manifesto contem claim_id duplicado.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                citation.original_text.as_deref(),
                false,
            ));
        }
        if citation.schema_version != CITATION_SCHEMA {
            blockers.push(blocker(
                "citation_schema_invalid",
                "A citacao nao usa citation.v1.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                citation.original_text.as_deref(),
                false,
            ));
        }
        if citation.citation_type == CitationType::DirectQuote
            && !locator_is_valid(citation.locator.as_deref())
        {
            blockers.push(blocker(
                "direct_quote_locator_missing",
                "Citacao direta estruturada requer localizador verificavel.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                citation.original_text.as_deref(),
                true,
            ));
        }
        if citation.author_display.trim().is_empty()
            || citation.author_key.trim().is_empty()
            || !valid_year(citation.year.trim())
        {
            blockers.push(blocker(
                "citation_required_fields_missing",
                "A citacao requer author_display, author_key e ano valido.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                citation.original_text.as_deref(),
                true,
            ));
        }
        if ascii_fold(&displayed_surname(citation)) != ascii_fold(citation.author_key.trim()) {
            blockers.push(blocker(
                "citation_canonical_author_mismatch",
                "author_display da citacao deve preservar integralmente a chave canonica author_key.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                Some(&citation.author_display),
                false,
            ));
        }
        let Some(source) = sources.get(citation.source_id.as_str()) else {
            blockers.push(blocker(
                "citation_source_missing",
                "source_id da citacao nao existe no manifesto.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                citation.original_text.as_deref(),
                true,
            ));
            citations.push(citation.clone());
            continue;
        };
        used_sources.insert(citation.source_id.clone());
        if citation.verification_status != CitationVerificationStatus::Verified
            || source.verification_status != CitationVerificationStatus::Verified
            || citation.source_access == CitationSourceAccess::UnverifiedHypothesis
            || citation.source_access == CitationSourceAccess::ContextualInference
        {
            blockers.push(blocker(
                "source_not_verified",
                "A citacao depende de fonte sem verificacao suficiente.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                citation.original_text.as_deref(),
                true,
            ));
        }
        if citation.citation_type == CitationType::DirectQuote
            && !matches!(
                citation.source_access,
                CitationSourceAccess::FullDocumentOpened | CitationSourceAccess::ExcerptConsulted
            )
        {
            blockers.push(blocker(
                "direct_quote_source_access_insufficient",
                "Citacao direta exige documento integral aberto ou excerto efetivamente consultado.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                citation.original_text.as_deref(),
                true,
            ));
        }
        if source.prohibited {
            blockers.push(blocker(
                "prohibited_source",
                "A fonte foi marcada como proibida pelo protocolo ativo.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                None,
                false,
            ));
        }
        if source.verification_status == CitationVerificationStatus::Quarantined
            || source.quarantine_reason.is_some()
        {
            blockers.push(blocker(
                "source_quarantined",
                "A fonte permanece em quarentena bibliografica.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                source.quarantine_reason.as_deref(),
                true,
            ));
        }
        let source_keys = source
            .authors
            .iter()
            .map(|author| canonical_author_key(&author.author_key))
            .collect::<BTreeSet<_>>();
        if citation.citation_type != CitationType::Apud
            && !source_keys.contains(&canonical_author_key(&citation.author_key))
        {
            blockers.push(blocker(
                "canonical_author_mismatch",
                "author_key da citacao nao corresponde a autoria canonica fornecida pela fonte.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                citation.original_text.as_deref(),
                false,
            ));
        }
        let normalized_text = format_in_text_citation(citation, source);
        let original_present = citation
            .original_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| folded_text.contains(&ascii_fold(value)))
            .unwrap_or(false);
        let normalized_present = folded_text.contains(&ascii_fold(&normalized_text));
        if !original_present && !normalized_present {
            blockers.push(blocker(
                "manifest_citation_absent_from_text",
                "A citacao do manifesto nao foi localizada no texto final.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                citation.original_text.as_deref(),
                false,
            ));
        }
        let mut normalized = citation.clone();
        normalized.normalized_text = Some(normalized_text);
        normalized.normalized_footnote = Some(format_footnote(
            source,
            citation.locator.as_deref(),
        ));
        citations.push(normalized);
    }
    let mut references = Vec::new();
    for source in manifest.sources.iter().take(MAX_SOURCES) {
        let missing = required_source_fields(source);
        if !missing.is_empty() {
            blockers.push(blocker(
                "reference_required_fields_missing",
                format!("Campos obrigatorios ausentes: {}.", missing.join(", ")),
                "error",
                None,
                Some(&source.source_id),
                None,
                true,
            ));
        }
        for author in &source.authors {
            if author.author_key.trim() != canonical_author_key(&author.author_key) {
                blockers.push(blocker(
                    "canonical_author_key_malformed",
                    "author_key deve preservar a chave canonica completa em maiusculas.",
                    "error",
                    None,
                    Some(&source.source_id),
                    Some(&author.author_key),
                    false,
                ));
            }
            let display_key = author
                .author_display
                .split(',')
                .next()
                .map(str::trim)
                .unwrap_or_default();
            if display_key.is_empty()
                || ascii_fold(display_key) != ascii_fold(author.author_key.trim())
            {
                blockers.push(blocker(
                    "canonical_author_display_mismatch",
                    "author_display deve iniciar pela mesma chave canonica completa de author_key.",
                    "error",
                    None,
                    Some(&source.source_id),
                    Some(&author.author_display),
                    false,
                ));
            }
        }
        if !used_sources.contains(&source.source_id) {
            blockers.push(blocker(
                "reference_without_body_use",
                "Fonte estruturada nao e usada por nenhuma citacao do manifesto.",
                "error",
                None,
                Some(&source.source_id),
                None,
                false,
            ));
        }
        let formatted = format_reference(source);
        if !formatted.is_empty() && !folded_text.contains(&ascii_fold(&formatted)) {
            blockers.push(blocker(
                "reference_not_normalized",
                "A referencia estruturada ainda nao aparece no texto com a forma normalizada gerada pelo motor.",
                "error",
                None,
                Some(&source.source_id),
                Some(&formatted),
                false,
            ));
        }
        references.push(formatted);
    }
    for reference in raw_references {
        let raw_key = ascii_fold(&reference.text);
        if !references
            .iter()
            .any(|normalized| ascii_fold(normalized) == raw_key)
        {
            blockers.push(blocker(
                "reference_not_in_manifest",
                "A secao final contem referencia que nao corresponde a uma fonte normalizada do manifesto.",
                "error",
                None,
                None,
                Some(&reference.text),
                false,
            ));
        }
    }
    for raw in raw_citations {
        let represented = citations.iter().any(|structured| {
            ascii_fold(&structured.author_key) == ascii_fold(&raw.author_key)
                && structured.year.trim() == raw.year.trim()
                && structured
                    .locator
                    .as_deref()
                    .map(ascii_fold)
                    .unwrap_or_default()
                    == raw
                        .locator
                        .as_deref()
                        .map(ascii_fold)
                        .unwrap_or_default()
        });
        if !represented {
            blockers.push(blocker(
                "body_citation_not_in_manifest",
                "O texto contem citacao autor-data sem entrada inequivoca no manifesto estruturado.",
                "error",
                Some(&raw.claim_id),
                Some(&raw.source_id),
                raw.original_text.as_deref(),
                true,
            ));
        }
    }
    for signal in unstructured_citation_signals(text) {
        let represented = citations.iter().any(|citation| {
            [
                citation.original_text.as_deref(),
                citation.normalized_text.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|candidate| ascii_fold(candidate).contains(&ascii_fold(&signal)))
        });
        if !represented {
            blockers.push(blocker(
                "unstructured_citation_signal",
                "Foi detectada citacao em nota ou HTML sem entrada inequivoca no manifesto.",
                "error",
                None,
                None,
                Some(&signal),
                true,
            ));
        }
    }
    (citations, references)
}

fn semantic_diff(current: Option<&CitationManifest>, previous: Option<&CitationManifest>) -> String {
    let Some(current) = current else {
        return "Manifesto estruturado ausente; diff semantico indisponivel.".to_string();
    };
    let Some(previous) = previous else {
        return "Primeiro manifesto estruturado; nenhuma versao anterior para comparar.".to_string();
    };
    let current_claims = current
        .citations
        .iter()
        .map(|citation| (citation.claim_id.clone(), sha256(serde_json::to_vec(citation).unwrap_or_default())))
        .collect::<BTreeMap<_, _>>();
    let previous_claims = previous
        .citations
        .iter()
        .map(|citation| (citation.claim_id.clone(), sha256(serde_json::to_vec(citation).unwrap_or_default())))
        .collect::<BTreeMap<_, _>>();
    let added = current_claims
        .keys()
        .filter(|key| !previous_claims.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    let removed = previous_claims
        .keys()
        .filter(|key| !current_claims.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    let changed = current_claims
        .iter()
        .filter(|(key, value)| {
            previous_claims
                .get(*key)
                .map(|old| old != *value)
                .unwrap_or(false)
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let current_sources = current
        .sources
        .iter()
        .map(|source| {
            (
                source.source_id.clone(),
                sha256(serde_json::to_vec(source).unwrap_or_default()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let previous_sources = previous
        .sources
        .iter()
        .map(|source| {
            (
                source.source_id.clone(),
                sha256(serde_json::to_vec(source).unwrap_or_default()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let sources_added = current_sources
        .keys()
        .filter(|key| !previous_sources.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    let sources_removed = previous_sources
        .keys()
        .filter(|key| !current_sources.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    let sources_changed = current_sources
        .iter()
        .filter(|(key, value)| {
            previous_sources
                .get(*key)
                .map(|old| old != *value)
                .unwrap_or(false)
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    format!(
        "Citacoes adicionadas: {}\nCitacoes removidas: {}\nCitacoes alteradas: {}\nFontes adicionadas: {}\nFontes removidas: {}\nFontes alteradas: {}",
        if added.is_empty() { "nenhuma".to_string() } else { added.join(", ") },
        if removed.is_empty() { "nenhuma".to_string() } else { removed.join(", ") },
        if changed.is_empty() { "nenhuma".to_string() } else { changed.join(", ") },
        if sources_added.is_empty() { "nenhuma".to_string() } else { sources_added.join(", ") },
        if sources_removed.is_empty() { "nenhuma".to_string() } else { sources_removed.join(", ") },
        if sources_changed.is_empty() { "nenhuma".to_string() } else { sources_changed.join(", ") }
    )
}

pub(crate) fn citation_manifests_from_attachments(
    attachments: &[AttachmentManifestEntry],
) -> Result<CitationManifestAttachments, String> {
    let mut current = None;
    let mut previous = None;
    for attachment in attachments {
        let name = attachment.original_name.to_lowercase();
        let media_type = attachment.media_type.to_lowercase();
        let explicitly_named = name.contains("citation-manifest")
            || name.contains("citation_manifest")
            || name.contains("manifesto-citacoes")
            || name.contains("manifesto_citacoes");
        if !name.ends_with(".json") && media_type != "application/json" && !explicitly_named {
            continue;
        }
        let bytes = read_attachment_bytes(attachment)?;
        let value = match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(value) => value,
            Err(error) if explicitly_named => {
                return Err(format!(
                    "citation manifest attachment is not valid JSON: {error}"
                ));
            }
            Err(_) => continue,
        };
        if value
            .get("schema_version")
            .and_then(serde_json::Value::as_str)
            != Some(MANIFEST_SCHEMA)
        {
            if explicitly_named {
                return Err(format!(
                    "citation manifest attachment must use {MANIFEST_SCHEMA}"
                ));
            }
            continue;
        }
        let manifest = serde_json::from_value::<CitationManifest>(value)
            .map_err(|error| format!("citation manifest payload is invalid: {error}"))?;
        let is_previous = name.contains("previous") || name.contains("anterior");
        let slot = if is_previous {
            &mut previous
        } else {
            &mut current
        };
        if slot.is_some() {
            return Err(if is_previous {
                "multiple previous citation manifests were supplied".to_string()
            } else {
                "multiple current citation manifests were supplied".to_string()
            });
        }
        *slot = Some(manifest);
    }
    Ok(CitationManifestAttachments { current, previous })
}

fn audit_table(citations: &[CitationAuditCitation], blockers: &[CitationAuditBlocker]) -> String {
    let mut lines = vec![
        "| Claim | Fonte | Tipo | Verificacao | Blockers |".to_string(),
        "| --- | --- | --- | --- | ---: |".to_string(),
    ];
    for citation in citations {
        let count = blockers
            .iter()
            .filter(|item| item.claim_id.as_deref() == Some(citation.claim_id.as_str()))
            .count();
        lines.push(format!(
            "| {} | {} | {:?} | {:?} | {} |",
            sanitize_short(&citation.claim_id, 80),
            sanitize_short(&citation.source_id, 80),
            citation.citation_type,
            citation.verification_status,
            count
        ));
    }
    if citations.is_empty() {
        lines.push("| — | — | — | — | 0 |".to_string());
    }
    lines.join("\n")
}

pub(crate) fn audit_abnt_citations_inner(
    request: AbntAuditRequest,
) -> Result<CitationAuditResult, String> {
    if request.text.chars().count() > MAX_TEXT_CHARS {
        return Err("citation audit input exceeds the safe text limit".to_string());
    }
    let protocol_hash = request
        .protocol_hash
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| sanitize_short(value, 128));
    let raw_references = reference_section(&request.text);
    let raw_citation_rows = raw_citations(&request.text);
    let mut blockers = Vec::new();
    let (citations, normalized_references) = if let Some(manifest) = request.manifest.as_ref() {
        let (citations, references) = validate_manifest(
            &request.text,
            &raw_citation_rows,
            &raw_references,
            protocol_hash.as_deref(),
            manifest,
            &mut blockers,
        );
        let mut policy_citations = raw_citation_rows.clone();
        policy_citations.extend(citations.iter().cloned());
        blockers.extend(document_policy_blockers(&request.text, &policy_citations));
        (citations, references)
    } else {
        let citations = raw_citation_rows;
        blockers.extend(raw_text_blockers(
            &request.text,
            &citations,
            &raw_references,
        ));
        if !citations.is_empty() {
            blockers.push(blocker(
                "structured_manifest_missing",
                "Citacoes foram detectadas em texto livre; forneca citation_manifest.v1 para provar metadados, acesso e verificacao.",
                "error",
                None,
                None,
                None,
                true,
            ));
        }
        (citations, Vec::new())
    };
    if contains_legacy_lacuna(&request.text) {
        blockers.push(blocker(
            "bibliographic_lacuna",
            "O texto ainda contem marcador de evidencia ou lacuna bibliografica.",
            "error",
            None,
            None,
            None,
            true,
        ));
    }
    let maestro_peer_status = if blockers.is_empty() {
        MaestroPeerStatus::Ready
    } else if blockers.iter().any(|item| !item.needs_evidence) {
        MaestroPeerStatus::NotReady
    } else {
        MaestroPeerStatus::NeedsEvidence
    };
    let markdown_references = normalized_references
        .iter()
        .map(|reference| format!("- {reference}"))
        .collect::<Vec<_>>();
    let html_references = normalized_references
        .iter()
        .map(|reference| format!("<li>{}</li>", escape_html(reference)))
        .collect::<Vec<_>>();
    let semantic_diff = semantic_diff(request.manifest.as_ref(), request.previous_manifest.as_ref());
    let manifest_bytes = request
        .manifest
        .as_ref()
        .and_then(|manifest| serde_json::to_vec(manifest).ok())
        .unwrap_or_default();
    let audit_id = sha256([
        request.text.as_bytes(),
        protocol_hash.as_deref().unwrap_or_default().as_bytes(),
        manifest_bytes.as_slice(),
    ]
    .concat());
    let audit_table_markdown = audit_table(&citations, &blockers);
    Ok(CitationAuditResult {
        schema_version: RESULT_SCHEMA.to_string(),
        audit_id,
        checked_at: Utc::now().to_rfc3339(),
        protocol_hash,
        maestro_peer_status,
        citations,
        normalized_references,
        markdown_references,
        html_references,
        blockers,
        audit_table_markdown,
        semantic_diff,
    })
}

fn contains_legacy_lacuna(text: &str) -> bool {
    let folded = ascii_fold(text);
    [
        "evidenciapendente",
        "edicaoconsultadanaoidentificada",
        "sineloco",
        "sinenomine",
        "sinedata",
    ]
    .iter()
    .any(|marker| folded.contains(marker))
}

pub(crate) fn maestro_peer_blocks_release(result: &CitationAuditResult) -> bool {
    result.maestro_peer_status != MaestroPeerStatus::Ready
}

#[tauri::command]
pub(crate) async fn audit_abnt_citations(
    request: AbntAuditRequest,
) -> Result<CitationAuditResult, String> {
    tauri::async_runtime::spawn_blocking(move || audit_abnt_citations_inner(request))
        .await
        .map_err(|error| format!("ABNT citation audit worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(text: &str) -> AbntAuditRequest {
        AbntAuditRequest {
            text: text.to_string(),
            protocol_hash: None,
            manifest: None,
            previous_manifest: None,
        }
    }

    fn verified_manifest() -> CitationManifest {
        CitationManifest {
            schema_version: MANIFEST_SCHEMA.to_string(),
            protocol_hash: "protocol-sha256".to_string(),
            citations: vec![CitationAuditCitation {
                schema_version: CITATION_SCHEMA.to_string(),
                claim_id: "claim-1".to_string(),
                citation_type: CitationType::DirectQuote,
                author_display: "Silva, Maria".to_string(),
                author_key: "SILVA".to_string(),
                year: "2026".to_string(),
                locator: Some("p. 12".to_string()),
                source_id: "source-1".to_string(),
                source_access: CitationSourceAccess::FullDocumentOpened,
                verification_status: CitationVerificationStatus::Verified,
                risk_if_wrong: CitationRisk::Medium,
                original_text: Some("(Silva, 2026, p. 12)".to_string()),
                normalized_text: None,
                normalized_footnote: None,
            }],
            sources: vec![CitationSource {
                source_id: "source-1".to_string(),
                source_type: CitationSourceType::Book,
                authors: vec![CitationAuthor {
                    author_display: "Silva, Maria".to_string(),
                    author_key: "SILVA".to_string(),
                }],
                title: "Obra".to_string(),
                subtitle: None,
                edition: None,
                place: Some("Sao Paulo".to_string()),
                publisher: Some("Editora".to_string()),
                year: "2026".to_string(),
                container_title: None,
                volume: None,
                issue: None,
                pages: None,
                url: None,
                doi: None,
                accessed_at: None,
                verification_sha256: Some("a".repeat(64)),
                verification_status: CitationVerificationStatus::Verified,
                prohibited: false,
                quarantine_reason: None,
            }],
        }
    }

    #[test]
    fn text_without_bibliographic_apparatus_is_ready() {
        let result = audit_abnt_citations_inner(request("Texto autoral sem citacao.")).unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::Ready);
        assert!(result.blockers.is_empty());
    }

    #[test]
    fn direct_quote_without_locator_needs_evidence() {
        let result = audit_abnt_citations_inner(request(
            "“Esta e uma citacao direta suficientemente longa” (Silva, 2020).\n\n## Referencias\nSILVA, Ana. Obra completa. Sao Paulo: Editora, 2020.",
        ))
        .unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::NeedsEvidence);
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "direct_quote_locator_missing"));
    }

    #[test]
    fn citation_and_reference_pairing_is_bidirectional() {
        let result = audit_abnt_citations_inner(request(
            "Texto indireto (Silva, 2020).\n\n## Referencias\nSOUZA, Bia. Outra obra. Rio: Editora, 2021.",
        ))
        .unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "citation_without_reference"));
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "reference_without_body_use"));
    }

    #[test]
    fn prohibited_source_is_not_ready() {
        let result = audit_abnt_citations_inner(request(
            "Texto sem citacao formal. Fonte: https://pt.wikipedia.org/wiki/Teste",
        ))
        .unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::NotReady);
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "prohibited_source"));
    }

    #[test]
    fn verified_manifest_generates_normalized_outputs_and_ready_peer() {
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: "“Trecho direto com mais de quatro palavras” (Silva, 2026, p. 12).\n\n## Referencias\nSILVA, Maria. Obra. Sao Paulo: Editora, 2026.".to_string(),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(verified_manifest()),
            previous_manifest: None,
        })
        .unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::Ready);
        assert_eq!(
            result.citations[0].normalized_text.as_deref(),
            Some("(Silva, 2026, p. 12)")
        );
        assert!(result.citations[0]
            .normalized_footnote
            .as_deref()
            .unwrap_or_default()
            .contains("SILVA, Maria. Obra."));
        assert!(result.blockers.is_empty());
    }

    #[test]
    fn verified_source_requires_a_real_verification_fingerprint() {
        let mut manifest = verified_manifest();
        manifest.sources[0].verification_sha256 = None;
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: "“Trecho direto com mais de quatro palavras” (Silva, 2026, p. 12).\n\n## Referencias\nSILVA, Maria. Obra. Sao Paulo: Editora, 2026.".to_string(),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::NeedsEvidence);
        assert!(result.blockers.iter().any(|item| {
            item.code == "reference_required_fields_missing" && item.needs_evidence
        }));
    }

    #[test]
    fn year_suffixes_used_for_abnt_disambiguation_are_valid() {
        assert!(valid_year("2026a"));
        assert!(valid_year("2026B"));
        assert!(!valid_year("26a"));
    }

    #[test]
    fn empty_manifest_does_not_silently_accept_footnote_citation_signals() {
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: "Texto com nota bibliografica[^1].\n\n[^1]: Fonte consultada.".to_string(),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(empty_citation_manifest("protocol-sha256")),
            previous_manifest: None,
        })
        .unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::NeedsEvidence);
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "unstructured_citation_signal"));
    }
}
