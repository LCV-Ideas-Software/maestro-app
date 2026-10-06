//! Local, recoverable MainSite draft custody.
//!
//! This module deliberately stops at one portable JSON artifact. It does not
//! talk to D1 and it does not claim that a caller-provided HTML sanitizer
//! profile was executed here. The fixed profile is an input/output invariant;
//! the future remote bridge must independently sanitize and revalidate before
//! publishing.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app_paths::{checked_data_child_path, data_dir};
use crate::editorial_io::{read_text_file, write_text_file};
use crate::mainsite_citation::{require_mainsite_citations_ready, MainSiteCitationContext};
use crate::sanitize::sanitize_text;
use crate::web_evidence::SharedChatProvider;

pub(crate) const MAINSITE_DRAFT_SCHEMA_VERSION: &str = "mainsite_draft.v1";
pub(crate) const MAINSITE_SANITIZER_PROFILE: &str = "mainsite_post_html.v1";
const MAX_TITLE_CHARS: usize = 300;
const MAX_AUTHOR_CHARS: usize = 200;
const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

static DRAFT_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredSharedChatEvidence {
    pub(crate) provider: SharedChatProvider,
    pub(crate) id: String,
    pub(crate) source_url: String,
    #[serde(default)]
    pub(crate) final_url: Option<String>,
    #[serde(default)]
    pub(crate) sha256: Option<String>,
    #[serde(default)]
    pub(crate) retrieved_at: Option<String>,
    #[serde(default)]
    pub(crate) access_mode: Option<String>,
    #[serde(default)]
    pub(crate) notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SaveMainSiteDraftRequest {
    #[serde(default)]
    pub(crate) requested_post_id: Option<u64>,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) is_pinned: bool,
    #[serde(default)]
    pub(crate) display_order: i64,
    pub(crate) is_published: bool,
    pub(crate) is_about_site: bool,
    pub(crate) sanitizer_profile: String,
    #[serde(default)]
    pub(crate) citation_context: Option<MainSiteCitationContext>,
    #[serde(default)]
    pub(crate) shared_chat_evidence: Vec<StoredSharedChatEvidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct MainSiteDraft {
    pub(crate) schema_version: String,
    pub(crate) requested_post_id: Option<u64>,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) content: String,
    pub(crate) is_pinned: bool,
    pub(crate) display_order: i64,
    pub(crate) is_published: bool,
    pub(crate) is_about_site: bool,
    pub(crate) sanitizer_profile: String,
    #[serde(default)]
    pub(crate) citation_context: Option<MainSiteCitationContext>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) shared_chat_evidence: Vec<StoredSharedChatEvidence>,
    pub(crate) content_sha256: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

fn io_lock() -> &'static Mutex<()> {
    DRAFT_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn draft_path() -> Result<PathBuf, String> {
    checked_data_child_path(&data_dir().join("drafts").join("mainsite-draft.json"))
}

fn content_sha256(content: &str) -> String {
    Sha256::digest(content.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn char_count(value: &str) -> usize {
    value.chars().count()
}

fn has_meaningful_html_content(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("<p></p>") {
        return false;
    }

    // Media-only documents are valid MainSite content even though removing
    // tags leaves no text. Everything else must retain a non-whitespace text
    // node after common empty-editor entities are removed.
    let lowercase = trimmed.to_ascii_lowercase();
    if ["<img", "<video", "<audio", "<iframe", "<figure"]
        .iter()
        .any(|marker| lowercase.contains(marker))
    {
        return true;
    }
    let without_tags = Regex::new(r"(?is)<[^>]*>")
        .ok()
        .map(|regex| regex.replace_all(trimmed, " ").into_owned())
        .unwrap_or_else(|| trimmed.to_string());
    let without_empty_entities = without_tags
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&#xA0;", " ")
        .replace("&#xa0;", " ");
    !without_empty_entities.trim().is_empty()
}

fn validate_shared_chat_source(url: &str, provider: SharedChatProvider) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| "MainSite shared-chat source URL is invalid".to_string())?;
    let segments = parsed
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();
    let token = match (provider, parsed.host_str(), segments.as_slice()) {
        (SharedChatProvider::ChatGpt, Some("chatgpt.com"), ["share", token])
        | (SharedChatProvider::Claude, Some("claude.ai"), ["share", token])
        | (SharedChatProvider::Gemini, Some("gemini.google.com"), ["share", token])
        | (SharedChatProvider::Gemini, Some("g.co"), ["gemini", "share", token]) => *token,
        _ => return Err("MainSite shared-chat source does not match its provider".to_string()),
    };
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.as_str() != url
        || token.is_empty()
        || token.len() > 256
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || sanitize_text(url, 4_096) != url
    {
        return Err(
            "MainSite shared-chat source must be a canonical public HTTPS share URL".to_string(),
        );
    }
    Ok(())
}

fn validate_shared_chat_evidence(evidence: &[StoredSharedChatEvidence]) -> Result<(), String> {
    let sensitive_note = Regex::new(
        r"(?i)(authorization|bearer|api[\s_-]?key|api[\s_-]?token|access[\s_-]?token|secret|password|cookie)",
    )
    .expect("valid shared-chat note redaction pattern");
    let plain_text = |value: &str, max_chars: usize| {
        !value.is_empty()
            && value.trim() == value
            && char_count(value) <= max_chars
            && !value.contains(['<', '>'])
            && sanitize_text(value, max_chars) == value
    };
    for item in evidence {
        if !plain_text(&item.id, 256) {
            return Err("MainSite shared-chat evidence identifier is invalid".to_string());
        }
        validate_shared_chat_source(&item.source_url, item.provider)?;
        if let Some(url) = item.final_url.as_deref() {
            validate_shared_chat_source(url, item.provider)?;
        }
        if item.sha256.as_deref().is_some_and(|hash| {
            hash.len() != 64
                || !hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }) {
            return Err("MainSite shared-chat evidence SHA-256 is invalid".to_string());
        }
        if item
            .retrieved_at
            .as_deref()
            .is_some_and(|date| DateTime::parse_from_rfc3339(date).is_err())
        {
            return Err("MainSite shared-chat evidence retrieval date is invalid".to_string());
        }
        if item
            .access_mode
            .as_deref()
            .is_some_and(|mode| !plain_text(mode, 80))
        {
            return Err("MainSite shared-chat evidence access mode is invalid".to_string());
        }
        if item.notes.len() > 20
            || item
                .notes
                .iter()
                .any(|note| !plain_text(note, 500) || sensitive_note.is_match(note))
        {
            return Err(
                "MainSite shared-chat evidence notes are invalid or contain sensitive fields"
                    .to_string(),
            );
        }
    }
    Ok(())
}

fn validate_editable_fields(request: &SaveMainSiteDraftRequest) -> Result<(), String> {
    validate_shared_chat_evidence(&request.shared_chat_evidence)?;
    if request.requested_post_id == Some(0) {
        return Err("requested_post_id must be null or a positive integer".to_string());
    }
    if request
        .requested_post_id
        .is_some_and(|value| value > MAX_JAVASCRIPT_SAFE_INTEGER)
    {
        return Err("requested_post_id exceeds the JavaScript safe-integer limit".to_string());
    }
    if request.title.trim().is_empty() {
        return Err("MainSite draft title cannot be empty".to_string());
    }
    if char_count(request.title.trim()) > MAX_TITLE_CHARS {
        return Err(format!(
            "MainSite draft title exceeds the {MAX_TITLE_CHARS}-character limit"
        ));
    }
    if char_count(request.author.trim()) > MAX_AUTHOR_CHARS {
        return Err(format!(
            "MainSite draft author exceeds the {MAX_AUTHOR_CHARS}-character limit"
        ));
    }
    if request.content.len() > MAX_CONTENT_BYTES {
        return Err(format!(
            "MainSite draft content exceeds the {MAX_CONTENT_BYTES}-byte limit"
        ));
    }
    if !has_meaningful_html_content(&request.content) {
        return Err("MainSite draft content cannot be empty or <p></p>".to_string());
    }
    if request.is_pinned {
        return Err("local MainSite drafts require is_pinned=false".to_string());
    }
    if request.display_order != 0 {
        return Err("local MainSite drafts require display_order=0".to_string());
    }
    if request.sanitizer_profile != MAINSITE_SANITIZER_PROFILE {
        return Err(format!(
            "unsupported MainSite sanitizer_profile; expected {MAINSITE_SANITIZER_PROFILE}"
        ));
    }
    Ok(())
}

pub(crate) fn validate_stored_draft(draft: &MainSiteDraft) -> Result<(), String> {
    if draft.schema_version != MAINSITE_DRAFT_SCHEMA_VERSION {
        return Err("unsupported or tampered MainSite draft schema_version".to_string());
    }
    let editable = SaveMainSiteDraftRequest {
        requested_post_id: draft.requested_post_id,
        title: draft.title.clone(),
        author: draft.author.clone(),
        content: draft.content.clone(),
        is_pinned: draft.is_pinned,
        display_order: draft.display_order,
        is_published: draft.is_published,
        is_about_site: draft.is_about_site,
        sanitizer_profile: draft.sanitizer_profile.clone(),
        citation_context: draft.citation_context.clone(),
        shared_chat_evidence: draft.shared_chat_evidence.clone(),
    };
    validate_editable_fields(&editable)?;
    let expected_hash = content_sha256(&draft.content);
    if draft.content_sha256 != expected_hash {
        return Err(
            "MainSite draft content hash mismatch; file may be corrupted or tampered".to_string(),
        );
    }
    let created_at = DateTime::parse_from_rfc3339(&draft.created_at)
        .map_err(|_| "MainSite draft created_at is invalid".to_string())?;
    let updated_at = DateTime::parse_from_rfc3339(&draft.updated_at)
        .map_err(|_| "MainSite draft updated_at is invalid".to_string())?;
    if updated_at < created_at {
        return Err("MainSite draft updated_at predates created_at".to_string());
    }
    Ok(())
}

fn decode_draft(encoded: &str) -> Result<MainSiteDraft, String> {
    let draft: MainSiteDraft = serde_json::from_str(encoded)
        .map_err(|error| format!("failed to decode MainSite draft JSON: {error}"))?;
    validate_stored_draft(&draft)?;
    Ok(draft)
}

fn load_from_path(path: &Path) -> Result<Option<MainSiteDraft>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let encoded = read_text_file(path)?;
    decode_draft(&encoded).map(Some)
}

fn save_to_path(path: &Path, request: SaveMainSiteDraftRequest) -> Result<MainSiteDraft, String> {
    validate_editable_fields(&request)?;
    require_mainsite_citations_ready(&request.content, request.citation_context.as_ref())?;

    // A corrupt existing draft is never overwritten silently. The operator
    // must first recover or explicitly remove it outside this command.
    let existing = load_from_path(path)?;
    let now = Utc::now().to_rfc3339();
    let draft = MainSiteDraft {
        schema_version: MAINSITE_DRAFT_SCHEMA_VERSION.to_string(),
        requested_post_id: request.requested_post_id,
        title: request.title.trim().to_string(),
        author: request.author.trim().to_string(),
        content_sha256: content_sha256(&request.content),
        content: request.content,
        citation_context: request.citation_context,
        shared_chat_evidence: request.shared_chat_evidence,
        is_pinned: false,
        display_order: 0,
        is_published: request.is_published,
        is_about_site: request.is_about_site,
        sanitizer_profile: MAINSITE_SANITIZER_PROFILE.to_string(),
        created_at: existing
            .as_ref()
            .map(|value| value.created_at.clone())
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
    };
    validate_stored_draft(&draft)?;
    let encoded = serde_json::to_string_pretty(&draft)
        .map_err(|error| format!("failed to encode MainSite draft JSON: {error}"))?;
    write_text_file(path, &encoded)?;

    // Read back from disk, including schema and hash validation, so a
    // successful command means the recoverable artifact is actually usable.
    load_from_path(path)?.ok_or_else(|| "MainSite draft disappeared after save".to_string())
}

#[tauri::command(async)]
pub(crate) fn load_mainsite_draft() -> Result<Option<MainSiteDraft>, String> {
    let _guard = io_lock()
        .lock()
        .map_err(|_| "MainSite draft I/O lock poisoned".to_string())?;
    load_from_path(&draft_path()?)
}

#[tauri::command(async)]
pub(crate) fn save_mainsite_draft(
    request: SaveMainSiteDraftRequest,
) -> Result<MainSiteDraft, String> {
    let _guard = io_lock()
        .lock()
        .map_err(|_| "MainSite draft I/O lock poisoned".to_string())?;
    save_to_path(&draft_path()?, request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_request(content: &str) -> SaveMainSiteDraftRequest {
        SaveMainSiteDraftRequest {
            requested_post_id: None,
            title: "Titulo editorial".to_string(),
            author: "Autoria".to_string(),
            content: content.to_string(),
            is_pinned: false,
            display_order: 0,
            is_published: false,
            is_about_site: false,
            sanitizer_profile: MAINSITE_SANITIZER_PROFILE.to_string(),
            citation_context: Some(MainSiteCitationContext {
                protocol_hash: None,
                manifest: None,
                previous_manifest: None,
            }),
            shared_chat_evidence: Vec::new(),
        }
    }

    #[test]
    fn rejects_empty_editor_html_and_non_default_d1_fields() {
        assert!(validate_editable_fields(&valid_request("<p></p>")).is_err());

        let mut pinned = valid_request("<p>Conteudo</p>");
        pinned.is_pinned = true;
        assert!(validate_editable_fields(&pinned).is_err());

        let mut ordered = valid_request("<p>Conteudo</p>");
        ordered.display_order = 1;
        assert!(validate_editable_fields(&ordered).is_err());
    }

    #[test]
    fn rejects_profile_drift_and_non_positive_requested_id() {
        let mut profile = valid_request("<p>Conteudo</p>");
        profile.sanitizer_profile = "outro.v1".to_string();
        assert!(validate_editable_fields(&profile).is_err());

        let mut id = valid_request("<p>Conteudo</p>");
        id.requested_post_id = Some(0);
        assert!(validate_editable_fields(&id).is_err());
    }

    #[test]
    fn allows_empty_optional_author() {
        let mut request = valid_request("<p>Conteudo</p>");
        request.author.clear();
        assert!(validate_editable_fields(&request).is_ok());
    }

    fn shared_chat_evidence() -> StoredSharedChatEvidence {
        StoredSharedChatEvidence {
            provider: SharedChatProvider::ChatGpt,
            id: "shared-chat-evidence-test".to_string(),
            source_url: "https://chatgpt.com/share/public-fixture".to_string(),
            final_url: Some("https://chatgpt.com/share/public-fixture".to_string()),
            sha256: Some("a".repeat(64)),
            retrieved_at: Some("2026-10-05T00:00:00Z".to_string()),
            access_mode: Some("http_get".to_string()),
            notes: vec!["Imported from a public shared conversation".to_string()],
        }
    }

    #[test]
    fn shared_chat_provenance_survives_save_load_without_entering_public_html() {
        let path = checked_data_child_path(&data_dir().join("drafts").join(format!(
            "shared-chat-roundtrip-{}-{}.json",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        )))
        .unwrap();
        let mut request = valid_request("<p>Imported conversation.</p>");
        request.shared_chat_evidence = vec![shared_chat_evidence()];
        let saved = save_to_path(&path, request).unwrap();
        let restored = load_from_path(&path).unwrap().unwrap();
        assert_eq!(restored.shared_chat_evidence, saved.shared_chat_evidence);
        assert_eq!(restored.shared_chat_evidence, [shared_chat_evidence()]);
        assert_eq!(restored.content, "<p>Imported conversation.</p>");
        assert!(!restored.content.contains("chatgpt.com/share"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn old_v1_drafts_default_to_empty_shared_chat_evidence() {
        let mut request = valid_request("<p>Legacy draft.</p>");
        request.shared_chat_evidence = vec![shared_chat_evidence()];
        let now = Utc::now().to_rfc3339();
        let legacy = serde_json::json!({
            "schema_version": MAINSITE_DRAFT_SCHEMA_VERSION,
            "requested_post_id": null,
            "title": request.title,
            "author": request.author,
            "content": request.content,
            "is_pinned": false,
            "display_order": 0,
            "is_published": false,
            "is_about_site": false,
            "sanitizer_profile": MAINSITE_SANITIZER_PROFILE,
            "citation_context": request.citation_context,
            "content_sha256": content_sha256("<p>Legacy draft.</p>"),
            "created_at": now,
            "updated_at": now
        });
        let draft = decode_draft(&legacy.to_string()).unwrap();
        assert!(draft.shared_chat_evidence.is_empty());
        assert!(serde_json::to_value(draft)
            .unwrap()
            .get("shared_chat_evidence")
            .is_none());
    }

    #[test]
    fn shared_chat_provenance_rejects_url_provider_and_secret_metadata_drift() {
        for source in [
            "https://chatgpt.com/share/public-fixture?access_token=private-value",
            "https://chatgpt.com/share/public-fixture#private-value",
            "https://user:private-value@chatgpt.com/share/public-fixture",
            "https://chatgpt.com:443/share/public-fixture",
            "https://claude.ai/share/public-fixture",
            "https://chatgpt.com/account/profile",
            "https://attacker.example/share/public-fixture",
        ] {
            let mut evidence = shared_chat_evidence();
            evidence.source_url = source.to_string();
            let error = validate_shared_chat_evidence(&[evidence]).unwrap_err();
            assert!(!error.contains("private-value"));
        }
        let mut evidence = shared_chat_evidence();
        evidence.sha256 = Some("not-a-hash".to_string());
        assert!(validate_shared_chat_evidence(&[evidence]).is_err());
        let mut evidence = shared_chat_evidence();
        evidence.notes = vec!["Authorization: Bearer private-value".to_string()];
        assert!(validate_shared_chat_evidence(&[evidence]).is_err());
        let mut encoded = serde_json::to_value(shared_chat_evidence()).unwrap();
        encoded["cookie"] = serde_json::json!("private-value");
        assert!(serde_json::from_value::<StoredSharedChatEvidence>(encoded).is_err());
    }

    #[test]
    fn native_save_cannot_persist_without_a_ready_citation_audit() {
        let missing = valid_request("<p>Texto autoral.</p>");
        let mut missing = missing;
        missing.citation_context = None;
        assert_eq!(
            save_to_path(Path::new("not-created.json"), missing).unwrap_err(),
            "MAINSITE_CITATION_CONTEXT_REQUIRED"
        );
        let blocked = valid_request("<p>Silva (2026) descreve o resultado.</p>");
        assert!(save_to_path(Path::new("not-created.json"), blocked)
            .unwrap_err()
            .starts_with("MAINSITE_CITATION_GATE_BLOCKED"));
    }

    #[test]
    fn stored_hash_is_fail_closed() {
        let now = Utc::now().to_rfc3339();
        let draft = MainSiteDraft {
            schema_version: MAINSITE_DRAFT_SCHEMA_VERSION.to_string(),
            requested_post_id: Some(42),
            title: "Titulo".to_string(),
            author: "Autoria".to_string(),
            content: "<p>Conteudo</p>".to_string(),
            is_pinned: false,
            display_order: 0,
            is_published: false,
            is_about_site: false,
            sanitizer_profile: MAINSITE_SANITIZER_PROFILE.to_string(),
            citation_context: None,
            shared_chat_evidence: Vec::new(),
            content_sha256: "0".repeat(64),
            created_at: now.clone(),
            updated_at: now,
        };
        let encoded = serde_json::to_string(&draft).expect("fixture encodes");
        assert!(decode_draft(&encoded).is_err());
    }

    #[test]
    fn update_preserves_created_at_and_rehashes_content() {
        let path = draft_path().expect("test draft path");
        let first =
            save_to_path(&path, valid_request("<p>Primeiro</p>")).expect("first draft persists");
        let second =
            save_to_path(&path, valid_request("<p>Segundo</p>")).expect("updated draft persists");

        assert_eq!(first.created_at, second.created_at);
        assert_ne!(first.content_sha256, second.content_sha256);
        assert_eq!(second.content_sha256, content_sha256("<p>Segundo</p>"));
    }
}
