//! Deterministic ABNT citation and reference gate.
//!
//! This module never invents bibliographic metadata. Free-text inspection is
//! deliberately conservative; complete formatting is available only when the
//! caller supplies a structured `citation_manifest.v1`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::sync::OnceLock;

use chrono::Utc;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sanitize::{sanitize_short, sanitize_text};
use crate::session_evidence::{read_attachment_bytes, AttachmentManifestEntry};

const RESULT_SCHEMA: &str = "maestro_peer.v1";
const CITATION_SCHEMA: &str = "citation.v1";
const MANIFEST_SCHEMA: &str = "citation_manifest.v1";
const MAX_TEXT_CHARS: usize = 2_000_000;
const MAX_GROUP_BYTES: usize = 64 * 1024;
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
    #[serde(skip)]
    raw_direct_context: bool,
    #[serde(skip)]
    raw_start: Option<usize>,
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
    Sha256::digest(value.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct DigestWriter<'a>(&'a mut Sha256);

impl Write for DigestWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn manifest_digest(manifest: &CitationManifest) -> Result<Vec<u8>, String> {
    let mut hash = Sha256::new();
    serde_json::to_writer(DigestWriter(&mut hash), manifest)
        .map_err(|error| format!("failed to hash citation manifest: {error}"))?;
    Ok(hash.finalize().to_vec())
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

fn faithful_fold(value: &str) -> Option<String> {
    let folded = ascii_fold(value);
    if folded.is_empty()
        || value.chars().any(|character| {
            character.is_alphanumeric() && ascii_fold(&character.to_string()).is_empty()
        })
    {
        None
    } else {
        Some(folded)
    }
}

fn equivalent_value(left: &str, right: &str) -> bool {
    if left.trim().is_empty()
        || right.trim().is_empty()
        || !left.chars().any(char::is_alphanumeric)
        || !right.chars().any(char::is_alphanumeric)
    {
        return false;
    }
    match (faithful_fold(left), faithful_fold(right)) {
        (Some(left), Some(right)) => left == right,
        _ => left.to_lowercase() == right.to_lowercase(),
    }
}

fn contains_value(haystack: &str, needle: &str) -> bool {
    if needle.trim().is_empty() || !needle.chars().any(char::is_alphanumeric) {
        return false;
    }
    if let Some(folded_needle) = faithful_fold(needle) {
        ascii_fold(haystack).contains(&folded_needle)
    } else {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    }
}

fn canonical_author_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

fn displayed_surname(author: &CitationAuditCitation) -> String {
    let display = if author.author_key.contains(" E ") {
        author.author_display.as_str()
    } else {
        author.author_display.split(',').next().unwrap_or_default()
    };
    let display = display.trim();
    if display.is_empty() {
        sanitize_text(author.author_key.trim(), 160)
    } else {
        sanitize_text(display, 160)
    }
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

fn citation_separator_only(text: &str) -> bool {
    static SEPARATOR: OnceLock<Regex> = OnceLock::new();
    SEPARATOR
        .get_or_init(|| Regex::new(r"^[\p{P}\s]*$").expect("static citation separator pattern"))
        .is_match(text)
}

fn has_direct_quote_context(text: &str, start: usize) -> bool {
    let begin = text[..start]
        .char_indices()
        .rev()
        .nth(220)
        .map(|(at, _)| at)
        .unwrap_or(0);
    let context = &text[begin..start];
    if let Some((index, quote)) = context
        .char_indices()
        .rev()
        .find(|(_, character)| matches!(character, '”' | '"'))
    {
        if citation_separator_only(&context[index + quote.len_utf8()..]) {
            return true;
        }
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
    let apud_pattern =
        Regex::new(r"(?i)^apud\s+[\p{L}][\p{L}\s.'’\-]{0,80},\s*\d{4}[a-z]?(?:,\s*(.+))?$")
            .expect("static apud citation pattern must compile");
    let patterns = [
        r"(?i)\(\s*((?:[\p{L}][\p{L}\s.'’\-]{1,80}|\p{Lo})),\s*(\d{4}[a-z]?)(?:,\s*([^)]+))?\s*\)",
        r"\b((?:[\p{Lu}\p{Lt}]\p{L}[\p{L}'’\-]*|\p{Lo}[\p{L}'’\-]*)(?:,\s*(?:[\p{Lu}\p{Lt}]\p{L}[\p{L}'’\-]*|\p{Lo}[\p{L}'’\-]*)){1,50}\s+e\s+(?:[\p{Lu}\p{Lt}]\p{L}[\p{L}'’\-]*|\p{Lo}[\p{L}'’\-]*))\s+\(\s*(\d{4}[a-zA-Z]?)(?:,\s*([^)]+))?\s*\)",
        r"\b((?:[\p{Lu}\p{Lt}]\p{L}[\p{L}'’\-]*|\p{Lo}[\p{L}'’\-]*)(?:\s+(?:e|da|de|do|dos|das|(?:[\p{Lu}\p{Lt}]\p{L}[\p{L}'’\-]*|\p{Lo}[\p{L}'’\-]*))){0,3}(?:\s+et\s+al\.)?)\s+\(\s*(\d{4}[a-zA-Z]?)(?:,\s*([^)]+))?\s*\)",
    ];
    for (pattern_index, raw_pattern) in patterns.into_iter().enumerate() {
        let pattern = Regex::new(raw_pattern).expect("static citation pattern must compile");
        for capture in pattern.captures_iter(text) {
            let Some(whole) = capture.get(0) else {
                continue;
            };
            // A semicolon belongs to the grouped parser below, including when
            // the first source has a locator before the next source.
            if pattern_index == 0 && whole.as_str().contains(';') {
                continue;
            }
            if rows.len() > MAX_CITATIONS {
                break;
            }
            if seen
                .iter()
                .any(|(start, end)| whole.start() < *end && whole.end() > *start)
                || !seen.insert((whole.start(), whole.end()))
            {
                continue;
            }
            let author = capture
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let year = capture
                .get(2)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let mut locator = capture
                .get(3)
                .map(|value| sanitize_text(value.as_str().trim(), 80))
                .filter(|value| !value.is_empty());
            let mut apud = false;
            if pattern_index == 0 {
                if let Some(value) = locator.as_deref().filter(|value| {
                    value
                        .get(..4)
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("apud"))
                }) {
                    if let Some(found) = apud_pattern.captures(value) {
                        apud = true;
                        locator = found.get(1).map(|part| part.as_str().trim().to_string());
                    }
                }
            }
            let author_display = sanitize_text(author.trim(), 160);
            let author_without_et_al = author_display
                .strip_suffix(" et al.")
                .unwrap_or(&author_display);
            let author_key = canonical_author_key(author_without_et_al);
            let source_id = format!("source-{}-{year}", ascii_fold(&author_key));
            let direct = has_direct_quote_context(text, whole.start());
            let normalized = if apud {
                sanitize_text(whole.as_str(), 240)
            } else if let Some(locator) = locator.as_deref() {
                format!("({author_display}, {year}, {locator})")
            } else {
                format!("({author_display}, {year})")
            };
            rows.push(CitationAuditCitation {
                schema_version: CITATION_SCHEMA.to_string(),
                claim_id: sha256(format!(
                    "{}|{}|{}",
                    whole.start(),
                    whole.end(),
                    whole.as_str()
                )),
                citation_type: if apud {
                    CitationType::Apud
                } else if direct {
                    CitationType::DirectQuote
                } else if pattern_index != 0 {
                    CitationType::GenericMention
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
                raw_direct_context: direct,
                raw_start: Some(whole.start()),
            });
        }
    }
    let groups = Regex::new(r"\(([^()\r\n]*;[^()\r\n]*)\)")
        .expect("static grouped citation pattern must compile");
    let year = Regex::new(r"(?i),\s*(\d{4}[a-z]?)(?:,\s*[^()]*)?$")
        .expect("static grouped citation year pattern must compile");
    for group in groups.captures_iter(text) {
        if rows.len() > MAX_CITATIONS {
            break;
        }
        let Some(whole) = group.get(0) else { continue };
        let Some(inner) = group.get(1) else { continue };
        if whole.len() > MAX_GROUP_BYTES {
            continue;
        }
        if !year.is_match(inner.as_str()) {
            continue;
        }
        let parts = inner
            .as_str()
            .split(';')
            .take(MAX_CITATIONS + 2)
            .map(str::trim)
            .collect::<Vec<_>>();
        if parts.len() > MAX_CITATIONS + 1 {
            continue;
        }
        let shared_year = year
            .captures(parts.last().copied().unwrap_or_default())
            .and_then(|capture| capture.get(1))
            .map(|value| value.as_str());
        let dated_parts = parts.iter().filter(|part| year.is_match(part)).count();
        if dated_parts == 1 && shared_year.is_some() && parts.len() > 1 {
            let first_author = parts[0];
            let candidate = format!("({first_author}, {})", shared_year.unwrap_or_default());
            if let Some(mut citation) = raw_citations(&candidate).into_iter().next() {
                citation.claim_id = sha256(format!("{}|{}|coauthors", whole.start(), whole.end()));
                citation.original_text = Some(sanitize_text(whole.as_str(), 320));
                citation.raw_start = Some(whole.start());
                if has_direct_quote_context(text, whole.start()) {
                    citation.citation_type = CitationType::DirectQuote;
                    citation.raw_direct_context = true;
                }
                rows.push(citation);
            }
            continue;
        }
        if dated_parts != parts.len() {
            continue;
        }
        for (part_index, part) in parts.iter().enumerate() {
            if rows.len() > MAX_CITATIONS {
                break;
            }
            let candidate = if year.is_match(part) {
                format!("({part})")
            } else if let Some(shared_year) = shared_year {
                format!("({part}, {shared_year})")
            } else {
                continue;
            };
            let Some(mut citation) = raw_citations(&candidate).into_iter().next() else {
                continue;
            };
            citation.claim_id = sha256(format!(
                "{}|{}|{}|{}|{part_index}",
                whole.start(),
                whole.end(),
                citation.author_key,
                citation.year
            ));
            citation.original_text = Some(sanitize_text(whole.as_str(), 320));
            citation.raw_start = Some(whole.start());
            if has_direct_quote_context(text, whole.start()) {
                citation.citation_type = CitationType::DirectQuote;
                citation.raw_direct_context = true;
            }
            rows.push(citation);
        }
    }
    rows.sort_by(|left, right| left.claim_id.cmp(&right.claim_id));
    rows
}

fn reference_heading_bounds(text: &str) -> Option<(usize, usize, usize)> {
    let title_pattern =
        Regex::new(r"(?i)^(?:refer[eê]ncias(?:\s+bibliogr[aá]ficas)?|bibliografia)$")
            .expect("static reference title pattern must compile");
    let mut heading_start = None;
    let mut heading_title = String::new();
    let mut reference = None;
    let mut depth = 0usize;
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if matches!(tag, Tag::Heading { .. }) && depth == 0 {
                    if let Some((start, end)) = reference {
                        return Some((start, end, range.start));
                    }
                    heading_start = Some(range.start);
                    heading_title.clear();
                }
                depth += 1;
            }
            Event::Text(value) | Event::Code(value) if heading_start.is_some() => {
                heading_title.push_str(&value);
            }
            Event::End(tag) => {
                if matches!(tag, TagEnd::Heading(_)) && depth == 1 {
                    if let Some(start) = heading_start.take() {
                        if title_pattern.is_match(heading_title.trim()) {
                            reference = Some((start, range.end));
                        }
                    }
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    reference.map(|(start, end)| (start, end, text.len()))
}

fn reference_section(source: &str, visible: &str) -> Vec<RawReference> {
    debug_assert_eq!(source.len(), visible.len());
    let Some((_, heading_end, section_end)) = reference_heading_bounds(source) else {
        return Vec::new();
    };
    let body = &visible[heading_end..section_end];
    let year_pattern = Regex::new(r"(?i)\b((?:18|19|20)\d{2}[a-z]?)\b").ok();
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.trim_start_matches(['-', '*']).trim())
        .filter(|line| !line.is_empty())
        .take(MAX_SOURCES + 1)
        .map(|line| {
            let author = line.split('.').next().unwrap_or_default();
            let key = author.split(',').next().unwrap_or(author);
            let year = year_pattern
                .as_ref()
                .and_then(|pattern| pattern.captures(line))
                .and_then(|capture| capture.get(1))
                .map(|value| value.as_str().to_string());
            RawReference {
                key: key.to_string(),
                year,
                text: line.to_string(),
            }
        })
        .collect()
}

fn citation_body_text(source: &str, visible: &str) -> String {
    debug_assert_eq!(source.len(), visible.len());
    let mut body = visible.as_bytes().to_vec();
    if let Some((start, _, end)) = reference_heading_bounds(source) {
        mask_code_bytes(&mut body, start..end);
    }
    String::from_utf8(body).expect("masking complete reference ranges preserves UTF-8")
}

fn quote_blockers(text: &str, citations: &[CitationAuditCitation]) -> Vec<CitationAuditBlocker> {
    let mut blockers = Vec::new();
    let mut opened = None;
    let mut count = 0;
    for (index, character) in text.char_indices() {
        if opened.is_none() {
            if matches!(character, '“' | '"') {
                opened = Some((index, character));
            }
            continue;
        }
        let (start, opener) = opened.unwrap();
        if character != if opener == '“' { '”' } else { '"' } {
            continue;
        }
        opened = None;
        let end = index + character.len_utf8();
        let quote = &text[start..end];
        if !quote.chars().any(char::is_alphabetic) {
            continue;
        }
        count += 1;
        if count > MAX_CITATIONS {
            blockers.push(capacity_blocker("direct quotes"));
            break;
        }
        if quote.len() > 400 {
            blockers.push(blocker(
                "direct_quote_too_long",
                "Trecho entre aspas excede o limite de analise segura; divida e vincule cada trecho a fonte.",
                "error",
                None,
                None,
                Some(quote),
                true,
            ));
            continue;
        }
        if quote[opener.len_utf8()..quote.len() - character.len_utf8()]
            .split_whitespace()
            .count()
            < 2
        {
            continue;
        }
        let after_end = text[end..]
            .char_indices()
            .nth(220)
            .map(|(offset, _)| end + offset)
            .unwrap_or(text.len());
        let cited = citations.iter().any(|citation| {
            citation.raw_direct_context
                && citation.raw_start.is_some_and(|start| {
                    start >= end && start <= after_end && citation_separator_only(&text[end..start])
                })
        });
        if !cited {
            blockers.push(blocker(
                "direct_quote_without_citation",
                "Trecho entre aspas nao esta ligado a uma citacao estruturada proxima.",
                "error",
                None,
                None,
                Some(quote),
                true,
            ));
        }
    }
    if opened.is_some() {
        blockers.push(blocker(
            "direct_quote_unclosed",
            "Aspas de citacao nao foram fechadas.",
            "error",
            None,
            None,
            None,
            true,
        ));
    }
    blockers
}

fn unstructured_citation_signals(text: &str) -> (Vec<(usize, String)>, bool) {
    let patterns = [
        r"(?i)<(?:cite|blockquote|q)\b[^>]*>",
        r"(?m)\[\^[^\]\r\n]{1,80}\]",
        r"(?i)\b(?:apud|ibidem|idem)\b|\b(?:ibid\.|op\.\s*cit\.)",
    ];
    let mut signals = Vec::new();
    let mut count = 0;
    for raw_pattern in patterns {
        let Ok(pattern) = Regex::new(raw_pattern) else {
            continue;
        };
        for found in pattern.find_iter(text) {
            count += 1;
            if count > MAX_CITATIONS {
                return (signals, true);
            }
            signals.push((found.start(), sanitize_text(found.as_str(), 240)));
        }
    }
    signals.sort_by_key(|(start, _)| *start);
    (signals, false)
}

fn capacity_blocker(reader: &str) -> CitationAuditBlocker {
    blocker(
        "citation_capacity_exceeded",
        format!("O leitor de {reader} excedeu o limite seguro de 500 ocorrencias."),
        "error",
        None,
        None,
        None,
        false,
    )
}

fn mask_code_bytes(visible: &mut [u8], range: std::ops::Range<usize>) {
    for byte in &mut visible[range] {
        if !matches!(*byte, b'\r' | b'\n') {
            *byte = b' ';
        }
    }
}

fn write_rendered_text(visible: &mut [u8], range: std::ops::Range<usize>, rendered: &str) {
    if rendered.as_bytes() == &visible[range.clone()] {
        return;
    }
    mask_code_bytes(visible, range.clone());
    if rendered.len() <= range.len() {
        visible[range.start..range.start + rendered.len()].copy_from_slice(rendered.as_bytes());
    }
}

fn rendered_visible_text(text: &str) -> String {
    let mut visible = text.as_bytes().to_vec();
    mask_code_bytes(&mut visible, 0..text.len());
    let parser = Parser::new(text);
    let mut definition_bytes = vec![false; text.len()];
    for (_, definition) in parser.reference_definitions().iter() {
        definition_bytes[definition.span.clone()].fill(true);
    }
    let mut in_code_block = false;
    let mut in_image = false;
    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Start(Tag::Image { .. }) => in_image = true,
            Event::End(TagEnd::Image) => in_image = false,
            Event::Text(rendered)
                if !in_code_block
                    && !in_image
                    && !definition_bytes[range.clone()].contains(&true) =>
            {
                write_rendered_text(&mut visible, range, &rendered);
            }
            _ => {}
        }
    }
    String::from_utf8(visible).expect("masking complete parser ranges preserves UTF-8")
}

fn document_policy_blockers(
    text: &str,
    citations: &[CitationAuditCitation],
) -> Vec<CitationAuditBlocker> {
    let visible = rendered_visible_text(text);
    let body = citation_body_text(text, &visible);
    let mut blockers = quote_blockers(&body, citations);
    // The maintained CommonMark parser distinguishes rendered HTML from
    // examples inside code spans and blocks, and from prose such as `2 < 3`.
    if let Some((_, range)) = Parser::new(text)
        .into_offset_iter()
        .find(|(event, _)| matches!(event, Event::Html(_) | Event::InlineHtml(_)))
    {
        blockers.push(blocker(
            "raw_html_in_final_text",
            "O texto final contem HTML cru; substitua por texto ou Markdown.",
            "error",
            None,
            None,
            text.get(range),
            false,
        ));
    }
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
        if (citation.citation_type == CitationType::DirectQuote || citation.raw_direct_context)
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
        let author_key = &citation.author_key;
        let first_author_token = citation
            .author_key
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let matched = references.iter().enumerate().find(|(_, reference)| {
            (contains_value(&reference.key, author_key)
                || (first_author_token
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .count()
                    >= 4
                    && contains_value(&reference.key, first_author_token)))
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
            if source
                .place
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                missing.push("place");
            }
            if source
                .publisher
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
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
            if source
                .pages
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                missing.push("pages");
            }
            if source
                .place
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                missing.push("place");
            }
            if source
                .publisher
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
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

fn narrative_authors(authors: &[String]) -> String {
    if authors.len() >= 3 {
        format!(
            "{} e {}",
            authors[..authors.len() - 1].join(", "),
            authors[authors.len() - 1]
        )
    } else {
        authors.join(" e ")
    }
}

fn format_in_text_citation(citation: &CitationAuditCitation, source: &CitationSource) -> String {
    let author = displayed_surname(citation);
    let author = if source.authors.len() >= 4 {
        format!("{author} et al.")
    } else {
        author
    };
    let coauthors = (2..=3).contains(&source.authors.len()).then(|| {
        source
            .authors
            .iter()
            .map(|author| {
                author
                    .author_display
                    .split(',')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
            .collect::<Vec<_>>()
    });
    let locator = citation
        .locator
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| sanitize_text(value, 100));
    if source.authors.len() >= 4 && citation.citation_type != CitationType::Apud {
        let authors = source
            .authors
            .iter()
            .map(|item| {
                item.author_display
                    .split(',')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
            .collect::<Vec<_>>();
        let suffix = locator
            .as_deref()
            .map(|value| format!(", {value}"))
            .unwrap_or_default();
        let narrative = format!(
            "{} ({}{suffix})",
            narrative_authors(&authors),
            citation.year.trim()
        );
        let parenthetical = format!("({}, {}{suffix})", authors.join("; "), citation.year.trim());
        if citation
            .original_text
            .as_deref()
            .is_some_and(|original| original.trim().to_lowercase() == narrative.to_lowercase())
        {
            return narrative;
        }
        if citation
            .original_text
            .as_deref()
            .is_some_and(|original| original.trim().to_lowercase() == parenthetical.to_lowercase())
        {
            return parenthetical;
        }
    }
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
        CitationType::GenericMention => {
            let narrative = coauthors
                .as_ref()
                .map(|authors| narrative_authors(authors))
                .unwrap_or(author);
            let locator_suffix = locator
                .as_deref()
                .map(|value| format!(", {value}"))
                .unwrap_or_default();
            format!("{narrative} ({}{locator_suffix})", citation.year.trim())
        }
        CitationType::DirectQuote | CitationType::IndirectQuote | CitationType::Paraphrase => {
            let locator_suffix = locator
                .as_deref()
                .map(|value| format!(", {value}"))
                .unwrap_or_default();
            let narrative_author = coauthors
                .as_ref()
                .map(|authors| narrative_authors(authors))
                .unwrap_or_else(|| author.clone());
            let narrative = format!(
                "{narrative_author} ({}{locator_suffix})",
                citation.year.trim()
            );
            if citation
                .original_text
                .as_deref()
                .is_some_and(|original| original.trim().to_lowercase() == narrative.to_lowercase())
            {
                return narrative;
            }
            let author = coauthors
                .as_ref()
                .map(|authors| authors.join("; "))
                .unwrap_or(author);
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
    if let Some(subtitle) = source
        .subtitle
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        title.push_str(": ");
        title.push_str(&sanitize_text(subtitle, 300));
    }
    if !title.is_empty() {
        parts.push(format!("{title}."));
    }
    if let Some(edition) = source
        .edition
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
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
        if let Some(volume) = source
            .volume
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            publication.push(format!("v. {}", sanitize_text(volume, 80)));
        }
        if let Some(issue) = source
            .issue
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            publication.push(format!("n. {}", sanitize_text(issue, 80)));
        }
        if let Some(pages) = source
            .pages
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
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
            if !place.is_empty() && !publisher.is_empty() {
                ": "
            } else {
                ""
            },
            sanitize_text(publisher, 240)
        );
        if !publication.is_empty() && valid_year(source.year.trim()) {
            parts.push(format!("{publication}, {}.", source.year.trim()));
        } else if !publication.is_empty() {
            parts.push(format!("{publication}."));
        } else if valid_year(source.year.trim()) {
            parts.push(format!("{}.", source.year.trim()));
        }
        if let Some(volume) = source
            .volume
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            parts.push(format!("v. {}.", sanitize_text(volume, 80)));
        }
        if let Some(issue) = source
            .issue
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            parts.push(format!("n. {}.", sanitize_text(issue, 80)));
        }
        if let Some(pages) = source
            .pages
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            parts.push(format!("p. {}.", sanitize_text(pages, 100)));
        }
    }
    if let Some(doi) = source
        .doi
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        parts.push(format!("DOI: {}.", sanitize_text(doi, 240)));
    }
    if let Some(url) = source
        .url
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
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
    if !manifest.citations.is_empty() && raw_references.is_empty() {
        blockers.push(blocker(
            "reference_section_missing",
            "O texto contem citacoes, mas nao possui secao final de referencias.",
            "error",
            None,
            None,
            None,
            true,
        ));
    }
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
        if !equivalent_value(&displayed_surname(citation), citation.author_key.trim()) {
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
        let coauthor_key = source
            .authors
            .iter()
            .map(|author| canonical_author_key(&author.author_key))
            .collect::<Vec<_>>();
        let coauthor_key = narrative_authors(&coauthor_key);
        if citation.citation_type != CitationType::Apud
            && !source_keys.contains(&canonical_author_key(&citation.author_key))
            && (source.authors.len() < 2 || !equivalent_value(&coauthor_key, &citation.author_key))
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
            .map(|value| contains_value(text, value))
            .unwrap_or(false);
        let normalized_present = contains_value(text, &normalized_text);
        if citation.citation_type == CitationType::Apud && !normalized_present {
            blockers.push(blocker(
                "apud_citation_not_normalized",
                "A citacao apud deve identificar no corpo a fonte consultada do manifesto.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                Some(&normalized_text),
                false,
            ));
        }
        if source.authors.len() >= 2 && !normalized_present {
            blockers.push(blocker(
                "coauthor_citation_not_normalized",
                "A citacao de coautores deve apresentar todos os autores no formato ABNT.",
                "error",
                Some(&claim_id),
                Some(&source_id),
                Some(&normalized_text),
                false,
            ));
        }
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
        normalized.normalized_footnote = Some(format_footnote(source, citation.locator.as_deref()));
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
            if display_key.is_empty() || !equivalent_value(display_key, author.author_key.trim()) {
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
        references.push(formatted);
    }
    let mut matched_references = BTreeSet::new();
    let reference_lookup = references
        .iter()
        .enumerate()
        .filter(|(_, value)| value.chars().any(char::is_alphanumeric))
        .map(|(index, value)| {
            (
                faithful_fold(value).unwrap_or_else(|| value.to_lowercase()),
                index,
            )
        })
        .collect::<BTreeMap<_, _>>();
    for reference in raw_references {
        let matched = reference.text.chars().any(char::is_alphanumeric).then(|| {
            faithful_fold(&reference.text).unwrap_or_else(|| reference.text.to_lowercase())
        });
        let matched = matched.and_then(|key| reference_lookup.get(&key).copied());
        if matched.is_none() {
            blockers.push(blocker(
                "reference_not_in_manifest",
                "A secao final contem referencia que nao corresponde a uma fonte normalizada do manifesto.",
                "error",
                None,
                None,
                Some(&reference.text),
                false,
            ));
        } else if !matched_references.insert(matched.unwrap_or_default()) {
            blockers.push(blocker(
                "reference_duplicate",
                "A mesma referencia aparece mais de uma vez na secao final.",
                "error",
                None,
                None,
                Some(&reference.text),
                false,
            ));
        }
    }
    for (index, reference) in references.iter().enumerate() {
        if !matched_references.contains(&index) {
            blockers.push(blocker(
                "reference_not_normalized",
                "A fonte nao possui linha normalizada na secao final de referencias.",
                "error",
                None,
                manifest
                    .sources
                    .get(index)
                    .map(|source| source.source_id.as_str()),
                Some(reference),
                false,
            ));
        }
    }
    let mut available = (0..citations.len()).collect::<BTreeSet<_>>();
    let same_author_year_locator =
        |structured: &CitationAuditCitation, raw: &CitationAuditCitation| {
            let fields_match = equivalent_value(&structured.author_key, &raw.author_key)
                && structured.year.trim() == raw.year.trim()
                && match (structured.locator.as_deref(), raw.locator.as_deref()) {
                    (None, None) => true,
                    (Some(left), Some(right)) => equivalent_value(left, right),
                    _ => false,
                };
            let apud_matches = if structured.citation_type == CitationType::Apud
                || raw.citation_type == CitationType::Apud
            {
                structured.citation_type == CitationType::Apud
                    && raw.citation_type == CitationType::Apud
                    && manifest
                        .sources
                        .iter()
                        .find(|source| source.source_id == structured.source_id)
                        .zip(raw.original_text.as_deref())
                        .is_some_and(|(source, original)| {
                            equivalent_value(original, &format_in_text_citation(structured, source))
                        })
            } else {
                true
            };
            fields_match && apud_matches
        };
    let mut matched_signal_ranges = Vec::new();
    let mut ordered_raw = raw_citations.iter().collect::<Vec<_>>();
    ordered_raw.sort_by_key(|raw| {
        let priority = if raw.citation_type == CitationType::GenericMention {
            1
        } else {
            0
        };
        (priority, raw.raw_start.unwrap_or(usize::MAX))
    });
    for raw in ordered_raw {
        let compatible_type = |citation: &CitationAuditCitation| match raw.citation_type {
            CitationType::DirectQuote => citation.citation_type == CitationType::DirectQuote,
            CitationType::GenericMention => matches!(
                citation.citation_type,
                CitationType::GenericMention
                    | CitationType::IndirectQuote
                    | CitationType::Paraphrase
            ),
            CitationType::IndirectQuote => matches!(
                citation.citation_type,
                CitationType::IndirectQuote | CitationType::Paraphrase
            ),
            CitationType::Apud => citation.citation_type == CitationType::Apud,
            CitationType::Paraphrase => false,
        };
        let represented = available
            .iter()
            .copied()
            .filter(|index| {
                same_author_year_locator(&citations[*index], raw)
                    && compatible_type(&citations[*index])
            })
            .min_by_key(|index| {
                let citation = &citations[*index];
                let exact_type = citation.citation_type == raw.citation_type;
                let exact_text = citation
                    .original_text
                    .as_deref()
                    .zip(raw.original_text.as_deref())
                    .is_some_and(|(left, right)| equivalent_value(left, right));
                (!exact_text, !exact_type, *index)
            });
        if let Some(index) = represented {
            if let (Some(start), Some(original)) = (raw.raw_start, raw.original_text.as_deref()) {
                matched_signal_ranges.push((start, start.saturating_add(original.len())));
            }
            if let Some(original) = raw.original_text.as_deref() {
                let parts = original
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .split(';')
                    .map(str::trim)
                    .collect::<Vec<_>>();
                if parts.len() > 1
                    && parts[..parts.len() - 1]
                        .iter()
                        .all(|part| !part.chars().any(char::is_numeric))
                {
                    let actual_authors = parts
                        .iter()
                        .enumerate()
                        .map(|(position, part)| {
                            if position + 1 == parts.len() {
                                part.split(',').next().unwrap_or_default().trim()
                            } else {
                                part
                            }
                        })
                        .collect::<Vec<_>>();
                    let source_authors = manifest
                        .sources
                        .iter()
                        .find(|source| source.source_id == citations[index].source_id)
                        .map(|source| &source.authors);
                    if !source_authors.is_some_and(|authors| {
                        authors.len() == actual_authors.len()
                            && authors.iter().zip(&actual_authors).all(|(author, actual)| {
                                equivalent_value(&author.author_key, actual)
                            })
                    }) {
                        blockers.push(blocker(
                            "grouped_coauthors_mismatch",
                            "A citacao agrupada nao corresponde aos coautores da fonte.",
                            "error",
                            Some(&citations[index].claim_id),
                            Some(&citations[index].source_id),
                            Some(original),
                            false,
                        ));
                    }
                }
            }
            let type_matches = match raw.citation_type {
                CitationType::DirectQuote => {
                    citations[index].citation_type == CitationType::DirectQuote
                }
                CitationType::GenericMention => matches!(
                    citations[index].citation_type,
                    CitationType::GenericMention
                        | CitationType::IndirectQuote
                        | CitationType::Paraphrase
                ),
                CitationType::IndirectQuote => matches!(
                    citations[index].citation_type,
                    CitationType::IndirectQuote | CitationType::Paraphrase
                ),
                CitationType::Apud => citations[index].citation_type == CitationType::Apud,
                CitationType::Paraphrase => false,
            };
            if !type_matches {
                blockers.push(blocker(
                    "citation_type_mismatch",
                    "O tipo declarado no manifesto nao corresponde a forma da citacao no corpo.",
                    "error",
                    Some(&citations[index].claim_id),
                    Some(&citations[index].source_id),
                    raw.original_text.as_deref(),
                    false,
                ));
            }
            if raw.raw_direct_context && citations[index].citation_type == CitationType::Apud {
                if !locator_is_valid(citations[index].locator.as_deref()) {
                    blockers.push(blocker(
                        "direct_quote_locator_missing",
                        "Citacao direta apud requer localizador verificavel.",
                        "error",
                        Some(&citations[index].claim_id),
                        Some(&citations[index].source_id),
                        raw.original_text.as_deref(),
                        true,
                    ));
                }
                if !matches!(
                    citations[index].source_access,
                    CitationSourceAccess::FullDocumentOpened
                        | CitationSourceAccess::ExcerptConsulted
                ) {
                    blockers.push(blocker(
                        "direct_quote_source_access_insufficient",
                        "Citacao direta apud exige documento ou excerto consultado.",
                        "error",
                        Some(&citations[index].claim_id),
                        Some(&citations[index].source_id),
                        raw.original_text.as_deref(),
                        true,
                    ));
                }
            }
            available.remove(&index);
        }
        if represented.is_none() {
            if let Some(index) = available
                .iter()
                .copied()
                .find(|index| same_author_year_locator(&citations[*index], raw))
            {
                blockers.push(blocker(
                    "citation_type_mismatch",
                    "O tipo declarado no manifesto nao corresponde a forma da citacao no corpo.",
                    "error",
                    Some(&citations[index].claim_id),
                    Some(&citations[index].source_id),
                    raw.original_text.as_deref(),
                    false,
                ));
            }
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
    for index in available {
        let structured = &citations[index];
        blockers.push(blocker(
            "manifest_citation_without_body_occurrence",
            "Uma entrada do manifesto nao possui ocorrencia distinta no corpo do texto.",
            "error",
            Some(&structured.claim_id),
            Some(&structured.source_id),
            structured.original_text.as_deref(),
            false,
        ));
    }
    let (signals, overflow) = unstructured_citation_signals(text);
    if overflow {
        blockers.push(capacity_blocker("citation signals"));
    }
    for (start, signal) in signals {
        let represented = matched_signal_ranges
            .iter()
            .any(|(begin, end)| start >= *begin && start < *end);
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

fn semantic_diff(
    current: Option<&CitationManifest>,
    previous: Option<&CitationManifest>,
) -> String {
    let Some(current) = current else {
        return "Manifesto estruturado ausente; diff semantico indisponivel.".to_string();
    };
    let Some(previous) = previous else {
        return "Primeiro manifesto estruturado; nenhuma versao anterior para comparar."
            .to_string();
    };
    let current_claims = current
        .citations
        .iter()
        .map(|citation| {
            (
                citation.claim_id.clone(),
                sha256(serde_json::to_vec(citation).unwrap_or_default()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let previous_claims = previous
        .citations
        .iter()
        .map(|citation| {
            (
                citation.claim_id.clone(),
                sha256(serde_json::to_vec(citation).unwrap_or_default()),
            )
        })
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
    let visible_text = rendered_visible_text(&request.text);
    let raw_references = reference_section(&request.text, &visible_text);
    let body = citation_body_text(&request.text, &visible_text);
    let raw_citation_rows = raw_citations(&body);
    let mut blockers = Vec::new();
    if request.previous_manifest.as_ref().is_some_and(|manifest| {
        manifest.citations.len() > MAX_CITATIONS || manifest.sources.len() > MAX_SOURCES
    }) {
        blockers.push(blocker(
            "manifest_history_capacity_exceeded",
            "O manifesto anterior excede o limite seguro de comparacao.",
            "error",
            None,
            None,
            None,
            false,
        ));
    }
    let grouped = Regex::new(r"\([^()\r\n]*;[^()\r\n]*\)")
        .expect("static grouped citation pattern must compile");
    let year_signal =
        Regex::new(r"(?i),\s*\d{4}[a-z]?").expect("static citation year signal must compile");
    let group_start = Regex::new(r"^\([\p{Lu}\p{Lo}][\p{L}\s.'’\-]*(?:,|;)")
        .expect("static grouped citation start must compile");
    for (group_index, found) in grouped.find_iter(&body).enumerate() {
        if group_index >= MAX_CITATIONS
            || found.len() > MAX_GROUP_BYTES
            || found.as_str().matches(';').take(MAX_CITATIONS + 1).count() >= MAX_CITATIONS
        {
            blockers.push(capacity_blocker("grouped citations"));
            break;
        }
        let dated_parts = found
            .as_str()
            .split(';')
            .filter(|part| year_signal.is_match(part))
            .count();
        let expected_citations = if dated_parts == 1 {
            1
        } else {
            found.as_str().matches(';').count() + 1
        };
        if group_start.is_match(found.as_str())
            && year_signal.is_match(found.as_str())
            && raw_citation_rows
                .iter()
                .filter(|citation| citation.raw_start == Some(found.start()))
                .count()
                < expected_citations
        {
            blockers.push(blocker(
                "grouped_citation_unparsed",
                "Citacao agrupada nao foi decomposta em todas as fontes; revise sua estrutura.",
                "error",
                None,
                None,
                Some(found.as_str()),
                true,
            ));
        }
    }
    if raw_citation_rows.len() > MAX_CITATIONS {
        blockers.push(capacity_blocker("body citations"));
    }
    if raw_references.len() > MAX_SOURCES {
        blockers.push(capacity_blocker("references"));
    }
    let (citations, normalized_references) = if let Some(manifest) = request.manifest.as_ref() {
        let (citations, references) = validate_manifest(
            &body,
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
        let (signals, overflow) = unstructured_citation_signals(&body);
        if overflow {
            blockers.push(capacity_blocker("citation signals"));
        }
        for (_, signal) in signals {
            blockers.push(blocker(
                "unstructured_citation_signal",
                "Foi detectada citacao em nota ou HTML sem manifesto estruturado.",
                "error",
                None,
                None,
                Some(&signal),
                true,
            ));
        }
        blockers.extend(raw_text_blockers(
            &request.text,
            &citations,
            &raw_references,
        ));
        if !citations.is_empty() || reference_heading_bounds(&request.text).is_some() {
            blockers.push(blocker(
                "structured_manifest_missing",
                "Citacoes ou referencias foram detectadas em texto livre; forneca citation_manifest.v1 para provar metadados, acesso e verificacao.",
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
    let manifests_within_budget = [
        request.manifest.as_ref(),
        request.previous_manifest.as_ref(),
    ]
    .into_iter()
    .flatten()
    .all(|manifest| {
        manifest.citations.len() <= MAX_CITATIONS && manifest.sources.len() <= MAX_SOURCES
    });
    let semantic_diff = if manifests_within_budget {
        semantic_diff(
            request.manifest.as_ref(),
            request.previous_manifest.as_ref(),
        )
    } else {
        "Diff semantico indisponivel: manifesto excede o limite seguro.".to_string()
    };
    let mut audit_hash = Sha256::new();
    audit_hash.update(request.text.as_bytes());
    audit_hash.update(protocol_hash.as_deref().unwrap_or_default().as_bytes());
    for manifest in [
        request.manifest.as_ref(),
        request.previous_manifest.as_ref(),
    ] {
        if let Some(manifest) = manifest {
            audit_hash.update([1]);
            audit_hash.update(manifest_digest(manifest)?);
        } else {
            audit_hash.update([0]);
        }
    }
    let audit_id = audit_hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
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
                raw_direct_context: false,
                raw_start: None,
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
    fn references_and_markdown_metadata_cannot_supply_body_citations() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("(Silva, 2026)".to_string());
        manifest.sources[0].title = "Obra (Silva, 2026)".to_string();
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Texto autoral.\n\n## Referencias\n{reference}"),
            protocol_hash: Some(manifest.protocol_hash.clone()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|blocker| { blocker.code == "manifest_citation_without_body_occurrence" }));
        let text = "[Manual](https://example.org/a \"Silva (2020)\")";
        let result = audit_abnt_citations_inner(request(text)).unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::Ready);
        let linked_image = "[![Silva (2020)](x.png)](https://example.org/a)";
        let result = audit_abnt_citations_inner(request(linked_image)).unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::Ready);
        let hidden_definition =
            "Veja [a fonte][manual].\n\n[manual]: https://example.org/a \"Silva (2020)\"";
        let result = audit_abnt_citations_inner(request(hidden_definition)).unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::Ready);

        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("(Silva, 2026)".to_string());
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Texto (Silva, 2026).\n{reference}\n\n## Referencias\nOutra obra."),
            protocol_hash: Some(manifest.protocol_hash.clone()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "reference_not_normalized"));
    }

    #[test]
    fn non_heading_reference_label_cannot_hide_visible_citations_or_quotes() {
        for label in ["#Referencias", "\\## Referencias", "`x`## Referencias"] {
            let text = format!(
                "Texto.\n{label}\n“Frase longa entre aspas sem fonte” (Silva, 2020) e (Souza, 2021)."
            );
            let result = audit_abnt_citations_inner(request(&text)).unwrap();
            assert_ne!(
                result.maestro_peer_status,
                MaestroPeerStatus::Ready,
                "{label}"
            );
            assert!(
                result
                    .blockers
                    .iter()
                    .any(|item| item.code == "structured_manifest_missing"),
                "{label}"
            );
            assert!(
                result
                    .blockers
                    .iter()
                    .any(|item| item.code.starts_with("direct_quote_")),
                "{label}"
            );
        }
    }

    #[test]
    fn nested_reference_headings_cannot_hide_body_citations() {
        for heading in ["> ## Referencias", "- ## Referencias"] {
            let text = format!(
                "{heading}\n\n“Frase longa entre aspas sem fonte” (Silva, 2020) e (Souza, 2021)."
            );
            let result = audit_abnt_citations_inner(request(&text)).unwrap();
            assert!(
                result
                    .blockers
                    .iter()
                    .any(|item| item.code == "structured_manifest_missing"),
                "{heading}"
            );
            assert!(
                result
                    .blockers
                    .iter()
                    .any(|item| item.code.starts_with("direct_quote_")),
                "{heading}"
            );
        }
    }

    #[test]
    fn rendered_entity_and_escaped_backticks_cannot_hide_apud_signal() {
        for text in [
            "Texto (Silva, 2020 &#96;apud&#96; Souza, 2021).",
            "Texto (Silva, 2020 \\`apud\\` Souza, 2021).",
        ] {
            for with_manifest in [false, true] {
                let result = audit_abnt_citations_inner(AbntAuditRequest {
                    text: text.to_string(),
                    protocol_hash: Some("protocol-sha256".to_string()),
                    manifest: with_manifest.then(|| empty_citation_manifest("protocol-sha256")),
                    previous_manifest: None,
                })
                .unwrap();
                assert!(
                    result
                        .blockers
                        .iter()
                        .any(|item| item.code == "unstructured_citation_signal"),
                    "{text}; manifest={with_manifest}"
                );
            }
        }
    }

    #[test]
    fn markdown_emphasis_delimiters_cannot_hide_rendered_citations() {
        for text in [
            "Texto (*Silva*, 2020).",
            "Texto (**SILVA**, 2020).",
            "Texto (***Silva***, 2020).",
            "Texto (__Silva__, 2020).",
            "Silva *et al.* (2020) descrevem o resultado.",
            "(Silva *et al.*, 2020).",
            "Silva _et al._ (2020) descrevem o resultado.",
            "*Silva* (2020) escreveu.",
            "Silva (*2020*) escreveu.",
        ] {
            let result = audit_abnt_citations_inner(request(text)).unwrap();
            assert!(
                result
                    .blockers
                    .iter()
                    .any(|item| item.code == "structured_manifest_missing"),
                "{text}"
            );
        }
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("(Silva, 2026)".to_string());
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Texto (*Silva*, 2026).\n\n## Referencias\n{reference}"),
            protocol_hash: Some(manifest.protocol_hash.clone()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert!(!result
            .blockers
            .iter()
            .any(|item| item.code == "manifest_citation_without_body_occurrence"));

        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("Silva et al. (2026)".to_string());
        for name in ["Souza", "Pereira", "Costa"] {
            manifest.sources[0].authors.push(CitationAuthor {
                author_display: format!("{name}, Ana"),
                author_key: name.to_uppercase(),
            });
        }
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!(
                "Silva *et al.* (2026) descrevem o resultado.\n\n## Referencias\n{reference}"
            ),
            protocol_hash: Some(manifest.protocol_hash.clone()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert!(!result
            .blockers
            .iter()
            .any(|item| item.code == "manifest_citation_without_body_occurrence"));
    }

    #[test]
    fn reference_heading_without_manifest_does_not_pass_ready() {
        let result = audit_abnt_citations_inner(request("## Referencias")).unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "structured_manifest_missing"));
    }

    #[test]
    fn html_comment_cannot_satisfy_manifest_body_occurrence() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("(Silva, 2026)".to_string());
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!(
                "Texto autoral.\n\n<!-- (Silva, 2026) -->\n\n## Referencias\n{reference}"
            ),
            protocol_hash: Some(manifest.protocol_hash.clone()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "manifest_citation_without_body_occurrence"));
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "raw_html_in_final_text"));
    }

    #[test]
    fn matching_uses_citation_type_before_consuming_same_author_occurrence() {
        let mut manifest = verified_manifest();
        let mut indirect = manifest.citations[0].clone();
        indirect.claim_id = "claim-indirect".to_string();
        indirect.citation_type = CitationType::IndirectQuote;
        manifest.citations.push(indirect);
        let reference = format_reference(&manifest.sources[0]);
        let text = format!(
            "“Trecho direto com fonte identificada” (Silva, 2026, p. 12). Outra ideia (Silva, 2026, p. 12).\n\n## Referencias\n{reference}"
        );
        for reverse in [false, true] {
            let mut ordered = manifest.clone();
            if reverse {
                ordered.citations.reverse();
            }
            let result = audit_abnt_citations_inner(AbntAuditRequest {
                text: text.clone(),
                protocol_hash: Some(ordered.protocol_hash.clone()),
                manifest: Some(ordered),
                previous_manifest: None,
            })
            .unwrap();
            assert_eq!(
                result.maestro_peer_status,
                MaestroPeerStatus::Ready,
                "{:?}",
                result.blockers
            );
        }
    }

    #[test]
    fn citations_and_quotes_outside_old_reader_window_block_release() {
        for text in [
            "Newton (1687) escreveu.",
            "Silva (2026B) escreveu.",
            r"Silva \(2020\) escreveu.",
            "Silva &#40;2020&#41; escreveu.",
            "Texto (Silva, 2020).\n\n## Referencias\nSILVA. Obra. 2020.\n\n## Conclusao\nTexto (Souza, 2020).",
        ] {
            let result = audit_abnt_citations_inner(request(text)).unwrap();
            assert_ne!(result.maestro_peer_status, MaestroPeerStatus::Ready, "{text}");
            assert!(result
                .blockers
                .iter()
                .any(|item| item.code == "structured_manifest_missing"));
        }
        for quote in ["terra plana".to_string(), "trecho longo ".repeat(60)] {
            let result = audit_abnt_citations_inner(request(&format!("\"{quote}\""))).unwrap();
            assert_ne!(result.maestro_peer_status, MaestroPeerStatus::Ready);
            assert!(result
                .blockers
                .iter()
                .any(|item| item.code.starts_with("direct_quote_")));
        }
    }

    #[test]
    fn one_word_interface_labels_are_not_treated_as_quotations() {
        let result =
            audit_abnt_citations_inner(request("O botao \"Salvar\" esta disponivel.")).unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::Ready);
    }

    #[test]
    fn grouped_reader_has_a_finite_work_budget() {
        let text = format!("({})", "Silva, 2020; ".repeat(MAX_CITATIONS + 1));
        let result = audit_abnt_citations_inner(request(&text)).unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "citation_capacity_exceeded"));
    }

    #[test]
    fn over_limit_manifest_audit_id_still_identifies_exact_contents() {
        let mut manifest = verified_manifest();
        manifest.citations = vec![manifest.citations[0].clone(); MAX_CITATIONS + 1];
        let make_request = |manifest: CitationManifest| AbntAuditRequest {
            text: "Texto sem citacao.".to_string(),
            protocol_hash: Some(manifest.protocol_hash.clone()),
            manifest: Some(manifest),
            previous_manifest: None,
        };
        let first = audit_abnt_citations_inner(make_request(manifest.clone())).unwrap();
        manifest.citations[0].claim_id = "different-claim".to_string();
        let second = audit_abnt_citations_inner(make_request(manifest)).unwrap();
        assert_ne!(first.audit_id, second.audit_id);
        assert!(first
            .blockers
            .iter()
            .any(|item| item.code == "manifest_capacity_exceeded"));
    }

    #[test]
    fn common_multiauthor_narratives_bind_to_verified_sources() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].author_display = "Silva, Souza e Pereira".to_string();
        manifest.citations[0].author_key = "SILVA, SOUZA E PEREIRA".to_string();
        manifest.citations[0].original_text = Some("Silva, Souza e Pereira (2026)".to_string());
        for surname in ["Souza", "Pereira"] {
            manifest.sources[0].authors.push(CitationAuthor {
                author_display: format!("{surname}, Ana"),
                author_key: surname.to_uppercase(),
            });
        }
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!(
                "Silva, Souza e Pereira (2026) descrevem o tema.\n\n## Referencias\n{reference}"
            ),
            protocol_hash: Some(manifest.protocol_hash.clone()),
            manifest: Some(manifest.clone()),
            previous_manifest: None,
        })
        .unwrap();
        assert_eq!(
            result.maestro_peer_status,
            MaestroPeerStatus::Ready,
            "{:?}",
            result.blockers
        );

        manifest.sources[0].authors.push(CitationAuthor {
            author_display: "Costa, Ana".to_string(),
            author_key: "COSTA".to_string(),
        });
        manifest.citations[0].author_display = "Silva, Maria".to_string();
        manifest.citations[0].author_key = "SILVA".to_string();
        manifest.citations[0].original_text = Some("Silva et al. (2026)".to_string());
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Silva et al. (2026) descrevem o tema.\n\n## Referencias\n{reference}"),
            protocol_hash: Some(manifest.protocol_hash.clone()),
            manifest: Some(manifest.clone()),
            previous_manifest: None,
        })
        .unwrap();
        assert_eq!(
            result.maestro_peer_status,
            MaestroPeerStatus::Ready,
            "{:?}",
            result.blockers
        );

        manifest.citations[0].author_display = "Silva, Souza, Pereira e Costa".to_string();
        manifest.citations[0].author_key = "SILVA, SOUZA, PEREIRA E COSTA".to_string();
        manifest.citations[0].original_text =
            Some("Silva, Souza, Pereira e Costa (2026)".to_string());
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Silva, Souza, Pereira e Costa (2026) descrevem o tema.\n\n## Referencias\n{reference}"),
            protocol_hash: Some(manifest.protocol_hash.clone()),
            manifest: Some(manifest),
            previous_manifest: None,
        }).unwrap();
        assert_eq!(
            result.maestro_peer_status,
            MaestroPeerStatus::Ready,
            "{:?}",
            result.blockers
        );
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

    #[test]
    fn manifest_year_digits_do_not_disguise_an_unlisted_footnote() {
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: "“Trecho direto com mais de quatro palavras” (Silva, 2026, p. 12). Nota[^1].\n\n[^1]: Fonte adicional.\n\n## Referencias\nSILVA, Maria. Obra. Sao Paulo: Editora, 2026.".to_string(),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(verified_manifest()),
            previous_manifest: None,
        })
        .unwrap();
        assert!(result.blockers.iter().any(|item| {
            item.code == "unstructured_citation_signal" && item.excerpt.as_deref() == Some("[^1]")
        }));
    }

    #[test]
    fn manifest_overflow_blocks_before_truncated_entries_can_pass() {
        let mut manifest = verified_manifest();
        manifest.citations = vec![manifest.citations[0].clone(); MAX_CITATIONS + 1];
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: "“Trecho direto com mais de quatro palavras” (Silva, 2026, p. 12).\n\n## Referencias\nSILVA, Maria. Obra. Sao Paulo: Editora, 2026.".to_string(),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "manifest_capacity_exceeded"));
    }

    #[test]
    fn unlisted_narrative_citations_in_other_scripts_block_release() {
        for citation in ["Иванов (2020)", "王 (2020)", "ǅa (2020)"] {
            let raw = raw_citations(&format!("Texto cita {citation} sem entrada no manifesto."));
            assert_eq!(raw.len(), 1, "{citation}");
            assert_eq!(raw[0].author_display, citation.split(' ').next().unwrap());
            let result = audit_abnt_citations_inner(AbntAuditRequest {
                text: format!("Texto cita {citation} sem entrada no manifesto."),
                protocol_hash: Some("protocol-sha256".to_string()),
                manifest: Some(empty_citation_manifest("protocol-sha256")),
                previous_manifest: None,
            })
            .unwrap();
            assert_ne!(
                result.maestro_peer_status,
                MaestroPeerStatus::Ready,
                "{citation}"
            );
            assert!(
                result
                    .blockers
                    .iter()
                    .any(|item| item.code == "body_citation_not_in_manifest"),
                "{citation}"
            );
        }
    }

    #[test]
    fn single_cased_letter_is_not_a_narrative_author() {
        for text in ["X (2020)", "ǅ (2020)"] {
            assert!(raw_citations(text).is_empty(), "{text}");
        }
        assert_eq!(raw_citations("王 (2020)").len(), 1);
    }

    #[test]
    fn markdown_code_examples_do_not_create_body_citations() {
        for text in [
            "Exemplo: `Иванов (2020)`.",
            "Exemplo:\n\n```md\n(Silva, 2020)\n```",
        ] {
            let result = audit_abnt_citations_inner(request(text)).unwrap();
            assert!(result.citations.is_empty(), "{text}");
            assert_eq!(
                result.maestro_peer_status,
                MaestroPeerStatus::Ready,
                "{text}"
            );
        }
    }

    #[test]
    fn grouped_citations_cannot_escape_the_manifest_gate() {
        for text in ["(Silva, 2020; Souza, 2021)", "(Silva; Souza, 2020)"] {
            assert_eq!(
                raw_citations(text).len(),
                if text.contains("Silva;") { 1 } else { 2 },
                "{text}"
            );
            let result = audit_abnt_citations_inner(AbntAuditRequest {
                text: text.to_string(),
                protocol_hash: Some("protocol-sha256".to_string()),
                manifest: Some(empty_citation_manifest("protocol-sha256")),
                previous_manifest: None,
            })
            .unwrap();
            assert_eq!(result.citations.len(), 0);
            assert!(
                result
                    .blockers
                    .iter()
                    .any(|item| item.code == "body_citation_not_in_manifest"),
                "{text}"
            );
        }
    }

    #[test]
    fn first_grouped_locator_does_not_create_a_third_citation() {
        let text = "(Silva, 2020, p. 1; Souza, 2021)";
        let raw = raw_citations(text);
        assert_eq!(raw.len(), 2);
        assert!(raw
            .iter()
            .any(|row| row.author_key == "SILVA" && row.locator.as_deref() == Some("p. 1")));
        assert!(raw
            .iter()
            .any(|row| row.author_key == "SOUZA" && row.locator.is_none()));
        assert!(!raw.iter().any(|row| row
            .locator
            .as_deref()
            .is_some_and(|locator| locator.contains(';'))));

        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].year = "2020".to_string();
        manifest.citations[0].locator = Some("p. 1".to_string());
        manifest.citations[0].original_text = Some(text.to_string());
        manifest.sources[0].year = "2020".to_string();
        let mut second_source = manifest.sources[0].clone();
        second_source.source_id = "source-2".to_string();
        second_source.authors[0].author_display = "Souza, Ana".to_string();
        second_source.authors[0].author_key = "SOUZA".to_string();
        second_source.year = "2021".to_string();
        let mut second_citation = manifest.citations[0].clone();
        second_citation.claim_id = "claim-2".to_string();
        second_citation.author_display = "Souza, Ana".to_string();
        second_citation.author_key = "SOUZA".to_string();
        second_citation.year = "2021".to_string();
        second_citation.locator = None;
        second_citation.source_id = "source-2".to_string();
        manifest.citations.push(second_citation);
        let reference_1 = format_reference(&manifest.sources[0]);
        let reference_2 = format_reference(&second_source);
        manifest.sources.push(second_source);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Texto {text}.\n\n## Referencias\n{reference_1}\n{reference_2}"),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert_eq!(
            result.maestro_peer_status,
            MaestroPeerStatus::Ready,
            "{:?}",
            result.blockers
        );
    }

    #[test]
    fn manifest_claim_without_any_body_citation_blocks_release() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("Obra".to_string());
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Texto autoral.\n\n## Referencias\n{reference}"),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert_ne!(result.maestro_peer_status, MaestroPeerStatus::Ready);
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "manifest_citation_without_body_occurrence"));
    }

    #[test]
    fn narrative_semantics_and_parenthetical_form_are_validated_separately() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("Silva (2026)".to_string());
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Silva (2026) descreve a obra.\n\n## Referencias\n{reference}"),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest.clone()),
            previous_manifest: None,
        })
        .unwrap();
        assert_eq!(
            result.maestro_peer_status,
            MaestroPeerStatus::Ready,
            "{:?}",
            result.blockers
        );
        assert_eq!(
            result.citations[0].normalized_text.as_deref(),
            Some("Silva (2026)")
        );

        manifest.citations[0].citation_type = CitationType::GenericMention;
        manifest.citations[0].original_text = Some("(Silva, 2026)".to_string());
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Texto (Silva, 2026).\n\n## Referencias\n{reference}"),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "citation_type_mismatch"));
    }

    #[test]
    fn apud_matches_only_its_verified_consulted_source() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::Apud;
        manifest.citations[0].author_display = "Silva, Maria".to_string();
        manifest.citations[0].year = "2020".to_string();
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("(Silva, 2020, apud Souza, 2026)".to_string());
        manifest.sources[0].authors[0].author_display = "Souza, Ana".to_string();
        manifest.sources[0].authors[0].author_key = "SOUZA".to_string();
        let reference = format_reference(&manifest.sources[0]);
        let audit = |body: &str, manifest: CitationManifest| {
            audit_abnt_citations_inner(AbntAuditRequest {
                text: format!("Texto {body}.\n\n## Referencias\n{reference}"),
                protocol_hash: Some("protocol-sha256".to_string()),
                manifest: Some(manifest),
                previous_manifest: None,
            })
            .unwrap()
        };
        let valid = audit("(Silva, 2020, apud Souza, 2026)", manifest.clone());
        assert_eq!(
            valid.maestro_peer_status,
            MaestroPeerStatus::Ready,
            "{:?}",
            valid.blockers
        );
        let wrong_source = audit("(Silva, 2020, apud Mendes, 2026)", manifest.clone());
        assert_ne!(wrong_source.maestro_peer_status, MaestroPeerStatus::Ready);
        assert!(wrong_source
            .blockers
            .iter()
            .any(|item| item.code == "body_citation_not_in_manifest"));

        let mut duplicate = manifest.citations[0].clone();
        duplicate.claim_id = "claim-2".to_string();
        duplicate.original_text = Some("(Silva, 2020, apud Mendes, 2026)".to_string());
        manifest.citations.push(duplicate);
        let mixed = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Texto (Silva, 2020, apud Souza, 2026) e (Silva, 2020, apud Mendes, 2026).\n\n## Referencias\n{reference}"),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest.clone()),
            previous_manifest: None,
        }).unwrap();
        assert_ne!(mixed.maestro_peer_status, MaestroPeerStatus::Ready);
        assert!(mixed
            .blockers
            .iter()
            .any(|item| item.code == "body_citation_not_in_manifest"));

        manifest.citations.pop();
        for separator in [" ", " — ", ": ", "! ", "? ", "… "] {
            let direct = audit(
                &format!("“Trecho direto com mais de quatro palavras”{separator}(Silva, 2020, apud Souza, 2026)"),
                manifest.clone(),
            );
            assert!(
                direct
                    .blockers
                    .iter()
                    .any(|item| item.code == "direct_quote_locator_missing"),
                "{separator}"
            );
        }

        let mut with_locator = verified_manifest();
        with_locator.citations[0].citation_type = CitationType::Apud;
        with_locator.citations[0].year = "2020".to_string();
        with_locator.citations[0].original_text =
            Some("(Silva, 2020, apud Souza, 2026, p. 12)".to_string());
        with_locator.sources[0].authors[0].author_display = "Souza, Ana".to_string();
        with_locator.sources[0].authors[0].author_key = "SOUZA".to_string();
        let valid_direct = audit(
            "“Trecho direto com mais de quatro palavras” (Silva, 2020, apud Souza, 2026, p. 12)",
            with_locator,
        );
        assert_eq!(
            valid_direct.maestro_peer_status,
            MaestroPeerStatus::Ready,
            "{:?}",
            valid_direct.blockers
        );
    }

    #[test]
    fn coauthor_group_maps_to_one_verified_source() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("(Silva; Souza, 2026)".to_string());
        manifest.sources[0].authors.push(CitationAuthor {
            author_display: "Souza, Ana".to_string(),
            author_key: "SOUZA".to_string(),
        });
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Texto (Silva; Souza, 2026).\n\n## Referencias\n{reference}"),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::Ready);
        assert!(result.blockers.is_empty());
        assert_eq!(
            result.citations[0].normalized_text.as_deref(),
            Some("(Silva; Souza, 2026)")
        );
    }

    #[test]
    fn narrative_coauthors_match_the_verified_source() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::GenericMention;
        manifest.citations[0].locator = None;
        manifest.citations[0].author_display = "Silva e Souza".to_string();
        manifest.citations[0].author_key = "SILVA E SOUZA".to_string();
        manifest.citations[0].original_text = Some("Silva e Souza (2026)".to_string());
        manifest.sources[0].authors.push(CitationAuthor {
            author_display: "Souza, Ana".to_string(),
            author_key: "SOUZA".to_string(),
        });
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Silva e Souza (2026) explicam o tema.\n\n## Referencias\n{reference}"),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::Ready);
        assert_eq!(
            result.citations[0].normalized_text.as_deref(),
            Some("Silva e Souza (2026)")
        );
    }

    #[test]
    fn verified_coauthor_source_rejects_single_author_body_form() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("(Silva, 2026)".to_string());
        manifest.sources[0].authors.push(CitationAuthor {
            author_display: "Souza, Ana".to_string(),
            author_key: "SOUZA".to_string(),
        });
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("Texto (Silva, 2026).\n\n## Referencias\n{reference}"),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "coauthor_citation_not_normalized"));
    }

    #[test]
    fn semicolon_prose_is_not_a_grouped_citation() {
        let result =
            audit_abnt_citations_inner(request("Texto (versão 2020; atualizado em 2021)."))
                .unwrap();
        assert_eq!(result.maestro_peer_status, MaestroPeerStatus::Ready);
    }

    #[test]
    fn manifest_cannot_relabel_direct_quote_as_indirect() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("(Silva, 2026)".to_string());
        let reference = format_reference(&manifest.sources[0]);
        for separator in [" ", " — ", ": ", "! ", "? ", "… "] {
            let result = audit_abnt_citations_inner(AbntAuditRequest {
                text: format!("“Trecho direto com mais de quatro palavras”{separator}(Silva, 2026).\n\n## Referencias\n{reference}"),
                protocol_hash: Some("protocol-sha256".to_string()),
                manifest: Some(manifest.clone()),
                previous_manifest: None,
            }).unwrap();
            assert!(
                result
                    .blockers
                    .iter()
                    .any(|item| item.code == "citation_type_mismatch"),
                "{separator}"
            );
        }
    }

    #[test]
    fn unrelated_later_citation_does_not_support_a_direct_quote() {
        let result = audit_abnt_citations_inner(request(
            "“Trecho direto com mais de quatro palavras” vem de outra fonte. Depois (Silva, 2026, p. 12).",
        ))
        .unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "direct_quote_without_citation"));
    }

    #[test]
    fn manifesto_references_require_section_and_unique_lines() {
        let mut manifest = verified_manifest();
        manifest.citations[0].citation_type = CitationType::IndirectQuote;
        manifest.citations[0].locator = None;
        manifest.citations[0].original_text = Some("(Silva, 2026)".to_string());
        let reference = format_reference(&manifest.sources[0]);
        let audit = |text: String| {
            audit_abnt_citations_inner(AbntAuditRequest {
                text,
                protocol_hash: Some("protocol-sha256".to_string()),
                manifest: Some(manifest.clone()),
                previous_manifest: None,
            })
            .unwrap()
        };
        let missing = audit(format!("Texto (Silva, 2026). {reference}"));
        assert!(missing
            .blockers
            .iter()
            .any(|item| item.code == "reference_section_missing"));
        let duplicate = audit(format!(
            "Texto (Silva, 2026).\n\n## Referencias\n{reference}\n{reference}"
        ));
        assert!(duplicate
            .blockers
            .iter()
            .any(|item| item.code == "reference_duplicate"));
        let variant = audit(format!(
            "Texto (Silva, 2026).\n\n## Referencias\n{reference}\n{}",
            reference.replace(',', "")
        ));
        assert!(variant
            .blockers
            .iter()
            .any(|item| item.code == "reference_duplicate"));
    }

    #[test]
    fn surplus_manifest_entry_cannot_reuse_one_body_occurrence() {
        let mut manifest = verified_manifest();
        let mut duplicate = manifest.citations[0].clone();
        duplicate.claim_id = "claim-2".to_string();
        manifest.citations.push(duplicate);
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!("“Trecho direto com mais de quatro palavras” (Silva, 2026, p. 12).\n\n## Referencias\n{reference}"),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert_ne!(result.maestro_peer_status, MaestroPeerStatus::Ready);
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "manifest_citation_without_body_occurrence"));
    }

    #[test]
    fn unstructured_citation_signals_block_without_manifest() {
        for signal in [
            "[^1]",
            "<cite>Fonte</cite>",
            "<blockquote>Fonte</blockquote>",
            "<q>Fonte</q>",
            "apud",
            "ibid.",
            "op. cit.",
        ] {
            let result =
                audit_abnt_citations_inner(request(&format!("Texto com {signal} sem autor-data.")))
                    .unwrap();
            assert!(
                result.blockers.iter().any(|item| {
                    item.code
                        == if signal.starts_with('<') {
                            "raw_html_in_final_text"
                        } else {
                            "unstructured_citation_signal"
                        }
                }),
                "{signal}"
            );
        }
    }

    #[test]
    fn every_reader_blocks_at_the_501st_occurrence() {
        let citations = "(Silva, 2020) ".repeat(MAX_CITATIONS + 1);
        assert!(audit_abnt_citations_inner(request(&citations))
            .unwrap()
            .blockers
            .iter()
            .any(|item| item.code == "citation_capacity_exceeded"));

        let references = format!(
            "## Referencias\n{}",
            "SILVA, Ana. Obra. Editora, 2020.\n".repeat(MAX_SOURCES + 1)
        );
        assert!(audit_abnt_citations_inner(request(&references))
            .unwrap()
            .blockers
            .iter()
            .any(|item| item.code == "citation_capacity_exceeded"));

        let quotes = "\"Trecho direto suficientemente longo sem fonte\" ".repeat(MAX_CITATIONS + 1);
        assert!(audit_abnt_citations_inner(request(&quotes))
            .unwrap()
            .blockers
            .iter()
            .any(|item| item.code == "citation_capacity_exceeded"));

        let signals = "[^1] ".repeat(MAX_CITATIONS + 1);
        assert!(audit_abnt_citations_inner(request(&signals))
            .unwrap()
            .blockers
            .iter()
            .any(|item| item.code == "citation_capacity_exceeded"));
    }

    #[test]
    fn values_that_fold_to_empty_do_not_count_as_present() {
        assert!(!contains_value("O texto termina.", "."));
        assert!(!equivalent_value(".", "*"));
        let mut manifest = verified_manifest();
        manifest.citations[0].original_text = Some(".".to_string());
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: "Texto sem citacao.\n\n## Referencias\nSILVA, Maria. Obra. Sao Paulo: Editora, 2026.".to_string(),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        }).unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "manifest_citation_absent_from_text"));
    }

    #[test]
    fn unicode_letters_are_never_discarded_during_matching() {
        assert!(!contains_value("Referencia de 2020.", "(Ωμέγα, 2020)"));
        assert!(!equivalent_value("Ωμέγα, 2020", "2020"));
        let mut manifest = verified_manifest();
        manifest.citations[0].original_text = Some("(Ωμέγα, 2020)".to_string());
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: "Texto de 2020.\n\n## Referencias\nSILVA, Maria. Obra. Sao Paulo: Editora, 2026."
                .to_string(),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "manifest_citation_absent_from_text"));
    }

    #[test]
    fn each_body_occurrence_consumes_one_manifest_entry() {
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: "Primeira frase (Silva, 2026, p. 12). Segunda frase (Silva, 2026, p. 12).\n\n## Referencias\nSILVA, Maria. Obra. Sao Paulo: Editora, 2026.".to_string(),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(verified_manifest()),
            previous_manifest: None,
        }).unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "body_citation_not_in_manifest"));
    }

    #[test]
    fn prose_quotes_and_raw_html_are_independently_checked() {
        for text in [
            "2 < 3 e \"um trecho direto com varias palavras\".",
            "x = \"um trecho direto com varias palavras\".",
        ] {
            let result = audit_abnt_citations_inner(request(text)).unwrap();
            assert!(result
                .blockers
                .iter()
                .any(|item| item.code == "direct_quote_without_citation"));
            assert!(!result
                .blockers
                .iter()
                .any(|item| item.code == "raw_html_in_final_text"));
        }
        let html = audit_abnt_citations_inner(request("Texto <cite>fonte</cite> e <!-- nota -->."))
            .unwrap();
        assert!(html
            .blockers
            .iter()
            .any(|item| item.code == "raw_html_in_final_text"));
        let html_quote = audit_abnt_citations_inner(request(
            "<p>“trecho literal com varias palavras” (Silva, 2026, p. 12)</p>",
        ))
        .unwrap();
        assert_eq!(html_quote.maestro_peer_status, MaestroPeerStatus::NotReady);
        assert!(html_quote
            .blockers
            .iter()
            .any(|item| item.code == "raw_html_in_final_text"));
    }

    #[test]
    fn multi_codepoint_entity_keeps_source_offsets_valid() {
        let result = audit_abnt_citations_inner(request(
            "Texto &nGt; citado (Silva, 2026).\n\n## Referencias\nSILVA, Maria. Obra. Sao Paulo: Editora, 2026.",
        ))
        .unwrap();
        assert!(result
            .blockers
            .iter()
            .any(|item| item.code == "structured_manifest_missing"));
    }

    #[test]
    fn unicode_authors_match_without_erasing_their_letters() {
        assert!(equivalent_value("Иванов", "ИВАНОВ"));
        assert!(equivalent_value("王", "王"));
        assert!(contains_value("Fonte: ИВАНОВ, 2026.", "Иванов"));
        assert!(contains_value("Fonte: 王, 2026.", "王"));
        assert!(!equivalent_value("Ωμέγα, 2020", "2020"));
        assert!(!contains_value("Fonte de 2020.", "Ωμέγα, 2020"));
        assert!(!equivalent_value(".", "."));

        let mut manifest = verified_manifest();
        manifest.citations[0].author_display = "王".to_string();
        manifest.citations[0].author_key = "王".to_string();
        manifest.citations[0].original_text = Some("(王, 2026, p. 12)".to_string());
        manifest.sources[0].authors[0].author_display = "王".to_string();
        manifest.sources[0].authors[0].author_key = "王".to_string();
        let reference = format_reference(&manifest.sources[0]);
        let result = audit_abnt_citations_inner(AbntAuditRequest {
            text: format!(
                "“Trecho direto com mais de quatro palavras” (王, 2026, p. 12).\n\n## Referencias\n{reference}"
            ),
            protocol_hash: Some("protocol-sha256".to_string()),
            manifest: Some(manifest),
            previous_manifest: None,
        })
        .unwrap();
        assert!(!result.blockers.iter().any(|item| {
            matches!(
                item.code.as_str(),
                "citation_canonical_author_mismatch"
                    | "canonical_author_display_mismatch"
                    | "reference_not_normalized"
            )
        }));
        assert_eq!(
            result.maestro_peer_status,
            MaestroPeerStatus::Ready,
            "{:?}",
            result.blockers
        );
    }

    #[test]
    fn markdown_code_examples_do_not_count_as_raw_html() {
        for text in [
            "Exemplo `<div class=\"example\">` em prosa.",
            "Exemplo `` `<cite>fonte</cite>` `` em prosa.",
            "Exemplo:\n```html\n<div class=\"example\">\n```\nFim.",
            "Exemplo:\n~~~html\n<cite>fonte</cite>\n~~~\nFim.",
            "Exemplo:\n\n    <div>codigo</div>\nFim.",
        ] {
            let result = audit_abnt_citations_inner(request(text)).unwrap();
            assert!(
                !result
                    .blockers
                    .iter()
                    .any(|item| item.code == "raw_html_in_final_text"),
                "{text}"
            );
            assert!(
                !result
                    .blockers
                    .iter()
                    .any(|item| item.code == "unstructured_citation_signal"),
                "{text}"
            );
        }
        let real = audit_abnt_citations_inner(request("Use `<div>` e depois <cite>fonte</cite>."))
            .unwrap();
        assert!(real
            .blockers
            .iter()
            .any(|item| item.code == "raw_html_in_final_text"));
        let continued =
            audit_abnt_citations_inner(request("Prosa\n    <cite>fonte</cite>.")).unwrap();
        assert!(continued
            .blockers
            .iter()
            .any(|item| item.code == "raw_html_in_final_text"));
    }
}
