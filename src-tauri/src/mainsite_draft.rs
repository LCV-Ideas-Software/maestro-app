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

pub(crate) const MAINSITE_DRAFT_SCHEMA_VERSION: &str = "mainsite_draft.v1";
pub(crate) const MAINSITE_SANITIZER_PROFILE: &str = "mainsite_post_html.v1";
const MAX_TITLE_CHARS: usize = 300;
const MAX_AUTHOR_CHARS: usize = 200;
const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

static DRAFT_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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
    pub(crate) content_sha256: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

fn io_lock() -> &'static Mutex<()> {
    DRAFT_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn draft_path() -> Result<PathBuf, String> {
    checked_data_child_path(
        &data_dir()
            .join("drafts")
            .join("mainsite-draft.json"),
    )
}

fn content_sha256(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
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

fn validate_editable_fields(request: &SaveMainSiteDraftRequest) -> Result<(), String> {
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
    };
    validate_editable_fields(&editable)?;
    let expected_hash = content_sha256(&draft.content);
    if draft.content_sha256 != expected_hash {
        return Err("MainSite draft content hash mismatch; file may be corrupted or tampered".to_string());
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

#[tauri::command]
pub(crate) fn load_mainsite_draft() -> Result<Option<MainSiteDraft>, String> {
    let _guard = io_lock()
        .lock()
        .map_err(|_| "MainSite draft I/O lock poisoned".to_string())?;
    load_from_path(&draft_path()?)
}

#[tauri::command]
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
        let first = save_to_path(&path, valid_request("<p>Primeiro</p>"))
            .expect("first draft persists");
        let second = save_to_path(&path, valid_request("<p>Segundo</p>"))
            .expect("updated draft persists");

        assert_eq!(first.created_at, second.created_at);
        assert_ne!(first.content_sha256, second.content_sha256);
        assert_eq!(second.content_sha256, content_sha256("<p>Segundo</p>"));
    }
}
