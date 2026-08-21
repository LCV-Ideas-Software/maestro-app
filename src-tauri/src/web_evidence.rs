//! Persistent, fail-closed Web Evidence Engine.
//!
//! Every automatic network request is restricted to public HTTP(S) targets,
//! uses the same DNS guard as `link_audit`, follows redirects manually so each
//! hop is revalidated, and stores only allowlisted response metadata. Browser
//! rendering runs in an isolated app-owned WebView2 window and still requires
//! explicit operator import before rendered content becomes evidence. No
//! browser profile, cookie jar, password store, or ambient authentication
//! material is imported here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use regex::Regex;
use reqwest::blocking::{Client, Response};
use reqwest::header::{
    HeaderName, HeaderValue, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE,
    ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, LOCATION,
};
use reqwest::redirect::Policy;
use reqwest::{Method, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::app_init::hidden_command;
use crate::app_paths::{checked_data_child_path, data_dir, sanitize_path_segment};
use crate::editorial_io::{read_text_file, write_binary_file, write_text_file};
use crate::link_audit::{public_http_url_rejection_reason, PublicOnlyResolver};
use crate::sanitize::{redact_secrets, sanitize_short, sanitize_text};

const SCHEMA_VERSION: &str = "web_evidence.v1";
const PROGRESS_EVENT: &str = "web-evidence-progress";
const DEFAULT_CACHE_TTL: &str = "P30D";
const DEFAULT_CACHE_TTL_DAYS: i64 = 30;
const HTTP_TIMEOUT_SECS: u64 = 30;
const MAX_REDIRECTS: usize = 5;
const MAX_HTTP_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_ROBOTS_BYTES: usize = 512 * 1024;
const MAX_OPERATOR_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 20;
const MAX_LIST_RESULTS: usize = 100;
const SHARED_CHAT_SCHEMA_VERSION: &str = "shared_chat_import.v1";
const MAX_SHARED_CHAT_ARTIFACT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SHARED_CHAT_TURNS: usize = 500;
const MAX_SHARED_CHAT_TURN_BYTES: usize = 512 * 1024;
const MAX_SHARED_CHAT_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

static EVIDENCE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub(crate) enum WebEvidenceMethod {
    Get,
    Head,
}

impl WebEvidenceMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
        }
    }

    fn reqwest(self) -> Method {
        match self {
            Self::Get => Method::GET,
            Self::Head => Method::HEAD,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebEvidenceAccessMode {
    HttpFetch,
    RenderedFetch,
    OfficialApi,
    OperatorAssistedBrowserCapture,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebEvidenceState {
    Queued,
    Collecting,
    Ready,
    Stale,
    OperatorActionRequired,
    Blocked,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebEvidenceCacheState {
    Fresh,
    Stale,
    Revalidating,
    Missing,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebEvidenceRobotsState {
    Allowed,
    Disallowed,
    Unavailable,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebEvidenceCopyrightState {
    Public,
    Licensed,
    OperatorProvided,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebEvidenceInteractionState {
    None,
    CaptchaRequired,
    LoginRequired,
    ConsentRequired,
    DownloadConfirmation,
    Paywall,
    HumanResolved,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct WebEvidenceRedirect {
    pub(crate) url: String,
    pub(crate) status: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WebEvidenceRecord {
    pub(crate) id: String,
    pub(crate) schema_version: String,
    pub(crate) state: WebEvidenceState,
    pub(crate) url: String,
    pub(crate) method: WebEvidenceMethod,
    pub(crate) access_mode: WebEvidenceAccessMode,
    pub(crate) status: Option<u16>,
    pub(crate) final_url: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) content_type: Option<String>,
    pub(crate) sha256: Option<String>,
    pub(crate) retrieved_at: Option<String>,
    pub(crate) expires_at: Option<String>,
    pub(crate) cache_ttl: String,
    pub(crate) cache_state: WebEvidenceCacheState,
    pub(crate) robots_state: WebEvidenceRobotsState,
    pub(crate) copyright_state: WebEvidenceCopyrightState,
    pub(crate) interaction_state: WebEvidenceInteractionState,
    pub(crate) human_resolved: bool,
    pub(crate) byte_count: Option<u64>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) redirect_chain: Vec<WebEvidenceRedirect>,
    pub(crate) curl_command: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) query: Option<String>,
    pub(crate) artifact_name: Option<String>,
    pub(crate) notes: Vec<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WebEvidenceListRequest {
    #[serde(default)]
    pub(crate) query: Option<String>,
    #[serde(default)]
    pub(crate) states: Vec<WebEvidenceState>,
    #[serde(default)]
    pub(crate) access_modes: Vec<WebEvidenceAccessMode>,
    #[serde(default)]
    pub(crate) stale_only: bool,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WebEvidenceListResult {
    pub(crate) items: Vec<WebEvidenceRecord>,
    pub(crate) next_cursor: Option<String>,
    pub(crate) total: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WebEvidenceFetchRequest {
    pub(crate) url: String,
    pub(crate) method: WebEvidenceMethod,
    #[serde(default)]
    pub(crate) force_revalidate: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WebEvidenceReplayRequest {
    pub(crate) evidence_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WebEvidenceSearchRequest {
    pub(crate) query: String,
    pub(crate) provider: String,
    pub(crate) limit: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WebEvidenceSearchResult {
    pub(crate) query: String,
    pub(crate) provider: String,
    pub(crate) items: Vec<WebEvidenceRecord>,
    pub(crate) total: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WebEvidenceUrlRequest {
    pub(crate) url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WebEvidenceImportRequest {
    #[serde(default)]
    pub(crate) url: Option<String>,
    pub(crate) name: String,
    pub(crate) media_type: String,
    pub(crate) data_base64: String,
    #[serde(default)]
    pub(crate) notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WebEvidenceInteractionRequest {
    pub(crate) evidence_id: String,
    pub(crate) confirmed: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SharedChatProvider {
    #[serde(rename = "chatgpt")]
    ChatGpt,
    Gemini,
    Claude,
}

impl SharedChatProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::ChatGpt => "chatgpt",
            Self::Gemini => "gemini",
            Self::Claude => "claude",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SharedChatImportRequest {
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) evidence_id: Option<String>,
    #[serde(default)]
    pub(crate) force_revalidate: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SharedChatEvidenceProjection {
    pub(crate) id: String,
    pub(crate) source_url: String,
    pub(crate) final_url: Option<String>,
    pub(crate) sha256: Option<String>,
    pub(crate) retrieved_at: Option<String>,
    pub(crate) access_mode: WebEvidenceAccessMode,
    pub(crate) notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SharedChatActionRequired {
    pub(crate) kind: String,
    pub(crate) reason: String,
    pub(crate) next_step: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum SharedChatImportResult {
    Ready {
        title: Option<String>,
        html: String,
        provider: SharedChatProvider,
        evidence: SharedChatEvidenceProjection,
        provenance_id: String,
        markdown_path: String,
        provenance_path: String,
    },
    OperatorActionRequired {
        provider: SharedChatProvider,
        evidence: SharedChatEvidenceProjection,
        action: SharedChatActionRequired,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SharedChatRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct SharedChatArtifact {
    kind: String,
    name: Option<String>,
    content: Option<String>,
    url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct SharedChatTurn {
    ordinal: usize,
    role: SharedChatRole,
    content_markdown: String,
    artifacts: Vec<SharedChatArtifact>,
    timestamp_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SharedChatCandidate {
    title: Option<String>,
    turns: Vec<SharedChatTurn>,
}

#[derive(Clone, Debug)]
struct ClassifiedSharedChatUrl {
    provider: SharedChatProvider,
    normalized_url: String,
    requires_gemini_redirect_validation: bool,
}

#[derive(Clone, Debug)]
enum SharedChatExtractionError {
    Insufficient(String),
    Ambiguous(String),
    Invalid(String),
}

impl SharedChatExtractionError {
    fn message(&self) -> &str {
        match self {
            Self::Insufficient(message) | Self::Ambiguous(message) | Self::Invalid(message) => {
                message
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct SharedChatProvenance {
    schema_version: String,
    provenance_id: String,
    provider: SharedChatProvider,
    source_url: String,
    canonical_url: String,
    evidence: SharedChatEvidenceProjection,
    title: Option<String>,
    turns: Vec<SharedChatTurn>,
    conversation_sha256: String,
    markdown_sha256: String,
    html_sha256: String,
    trust_classification: String,
    created_at: String,
}

#[derive(Clone, Debug, Serialize)]
struct WebEvidenceProgressEvent {
    operation: String,
    evidence_id: Option<String>,
    phase: String,
    message: String,
    at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ReplayRecipe {
    HttpFetch {
        url: String,
        method: WebEvidenceMethod,
    },
    Search {
        provider: String,
        query: String,
        limit: usize,
        result_url: String,
    },
    OperatorImport,
    BrowserHandoff,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredWebEvidence {
    record: WebEvidenceRecord,
    #[serde(default)]
    content_path: Option<String>,
    #[serde(default)]
    response_headers: BTreeMap<String, String>,
    replay: ReplayRecipe,
}

#[derive(Clone, Debug)]
struct RawHttpResponse {
    status: u16,
    final_url: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    duration_ms: u64,
    redirects: Vec<WebEvidenceRedirect>,
}

#[derive(Clone, Debug, Deserialize)]
struct SearchConnectorFile {
    #[allow(dead_code)]
    schema_version: String,
    connectors: Vec<SearchConnector>,
}

#[derive(Clone, Debug, Deserialize)]
struct SearchConnector {
    id: String,
    label: String,
    endpoint: String,
    query_parameter: String,
    limit_parameter: String,
    results_path: String,
    title_field: String,
    url_field: String,
    #[serde(default)]
    snippet_field: Option<String>,
    #[serde(default)]
    api_key_env_var: Option<String>,
    #[serde(default)]
    api_key_header: Option<String>,
}

fn io_lock() -> &'static Mutex<()> {
    EVIDENCE_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn evidence_dir() -> Result<PathBuf, String> {
    let path = checked_data_child_path(&data_dir().join("evidence"))?;
    fs::create_dir_all(&path)
        .map_err(|error| format!("failed to create web evidence directory: {error}"))?;
    Ok(path)
}

fn records_dir() -> Result<PathBuf, String> {
    let path = checked_data_child_path(&evidence_dir()?.join("records"))?;
    fs::create_dir_all(&path)
        .map_err(|error| format!("failed to create web evidence records directory: {error}"))?;
    Ok(path)
}

fn content_dir() -> Result<PathBuf, String> {
    let path = checked_data_child_path(&evidence_dir()?.join("content"))?;
    fs::create_dir_all(&path)
        .map_err(|error| format!("failed to create web evidence content directory: {error}"))?;
    Ok(path)
}

fn searches_dir() -> Result<PathBuf, String> {
    let path = checked_data_child_path(&evidence_dir()?.join("searches"))?;
    fs::create_dir_all(&path)
        .map_err(|error| format!("failed to create web evidence search directory: {error}"))?;
    Ok(path)
}

fn evidence_id(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sha256_bytes(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_valid_evidence_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn record_path(id: &str) -> Result<PathBuf, String> {
    if !is_valid_evidence_id(id) {
        return Err("invalid web evidence id".to_string());
    }
    checked_data_child_path(&records_dir()?.join(format!("{id}.json")))
}

fn content_path(id: &str, extension: &str) -> Result<PathBuf, String> {
    if !is_valid_evidence_id(id) {
        return Err("invalid web evidence id".to_string());
    }
    let extension = sanitize_path_segment(extension, 12);
    if extension.is_empty() {
        return Err("invalid web evidence content extension".to_string());
    }
    checked_data_child_path(&content_dir()?.join(format!("{id}.{extension}")))
}

fn save_stored(stored: &StoredWebEvidence) -> Result<(), String> {
    let encoded = serde_json::to_string_pretty(stored)
        .map_err(|error| format!("failed to serialize web evidence record: {error}"))?;
    let _guard = io_lock()
        .lock()
        .map_err(|_| "web evidence I/O lock poisoned".to_string())?;
    write_text_file(&record_path(&stored.record.id)?, &encoded)
}

fn load_stored(id: &str) -> Result<StoredWebEvidence, String> {
    let encoded = read_text_file(&record_path(id)?)
        .map_err(|_| format!("web evidence record '{id}' was not found"))?;
    serde_json::from_str(&encoded)
        .map_err(|error| format!("failed to decode web evidence record '{id}': {error}"))
}

fn cache_state(record: &WebEvidenceRecord, now: DateTime<Utc>) -> WebEvidenceCacheState {
    if record.state != WebEvidenceState::Ready && record.state != WebEvidenceState::Stale {
        return WebEvidenceCacheState::Missing;
    }
    let Some(expires_at) = record.expires_at.as_deref() else {
        return WebEvidenceCacheState::Stale;
    };
    match DateTime::parse_from_rfc3339(expires_at) {
        Ok(expires_at) if expires_at.with_timezone(&Utc) > now => WebEvidenceCacheState::Fresh,
        _ => WebEvidenceCacheState::Stale,
    }
}

fn project_record(mut record: WebEvidenceRecord, now: DateTime<Utc>) -> WebEvidenceRecord {
    record.cache_state = cache_state(&record, now);
    if record.state == WebEvidenceState::Ready
        && record.cache_state == WebEvidenceCacheState::Stale
    {
        record.state = WebEvidenceState::Stale;
    }
    record
}

fn base_record(
    id: String,
    url: &str,
    method: WebEvidenceMethod,
    access_mode: WebEvidenceAccessMode,
    now: DateTime<Utc>,
) -> WebEvidenceRecord {
    let now_text = now.to_rfc3339();
    WebEvidenceRecord {
        id,
        schema_version: SCHEMA_VERSION.to_string(),
        state: WebEvidenceState::Collecting,
        url: sanitize_text(url, 2_048),
        method,
        access_mode,
        status: None,
        final_url: None,
        title: None,
        content_type: None,
        sha256: None,
        retrieved_at: None,
        expires_at: None,
        cache_ttl: DEFAULT_CACHE_TTL.to_string(),
        cache_state: WebEvidenceCacheState::Missing,
        robots_state: WebEvidenceRobotsState::NotApplicable,
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
        created_at: now_text.clone(),
        updated_at: now_text,
    }
}

fn emit_progress(
    app: Option<&tauri::AppHandle>,
    operation: &str,
    evidence_id: Option<&str>,
    phase: &str,
    message: &str,
) {
    let Some(app) = app else {
        return;
    };
    let _ = app.emit(
        PROGRESS_EVENT,
        WebEvidenceProgressEvent {
            operation: sanitize_short(operation, 40),
            evidence_id: evidence_id.map(|value| sanitize_short(value, 64)),
            phase: sanitize_short(phase, 40),
            message: sanitize_text(message, 240),
            at: Utc::now().to_rfc3339(),
        },
    );
}

fn append_event(operation: &str, record: &WebEvidenceRecord) -> Result<(), String> {
    let path = checked_data_child_path(&evidence_dir()?.join("events.ndjson"))?;
    let line = serde_json::to_string(&serde_json::json!({
        "schema_version": 1,
        "operation": sanitize_short(operation, 40),
        "evidence_id": record.id,
        "state": record.state,
        "access_mode": record.access_mode,
        "at": Utc::now().to_rfc3339()
    }))
    .map_err(|error| format!("failed to serialize web evidence event: {error}"))?;
    let _guard = io_lock()
        .lock()
        .map_err(|_| "web evidence I/O lock poisoned".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create web evidence event directory: {error}"))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("failed to open web evidence event log: {error}"))?;
    writeln!(file, "{line}")
        .map_err(|error| format!("failed to append web evidence event: {error}"))
}

fn validate_public_url(value: &str) -> Result<Url, String> {
    if value.len() > 4_096 {
        return Err("URL exceeds the 4096-character evidence limit".to_string());
    }
    if let Some(reason) = public_http_url_rejection_reason(value) {
        return Err(reason);
    }
    let mut url = Url::parse(value).map_err(|_| "URL invalida ou incompleta".to_string())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URLs with embedded credentials are blocked".to_string());
    }
    if url
        .query_pairs()
        .any(|(key, _)| sensitive_query_key(key.as_ref()))
    {
        return Err(
            "URLs with credential-like query parameters are blocked; use an environment-backed connector or operator capture"
                .to_string(),
        );
    }
    url.set_fragment(None);
    Ok(url)
}

fn sensitive_query_key(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    [
        "access_token",
        "api_key",
        "apikey",
        "authorization",
        "credential",
        "key",
        "password",
        "secret",
        "signature",
        "sig",
        "token",
        "x-amz-credential",
        "x-amz-signature",
    ]
    .iter()
    .any(|candidate| normalized == *candidate || normalized.ends_with(candidate))
}

fn build_public_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .redirect(Policy::none())
        .dns_resolver(Arc::new(PublicOnlyResolver))
        // A system/environment proxy would resolve and connect on the proxy's
        // side, bypassing the resolver-to-socket binding above. Evidence fetch
        // therefore deliberately fails closed to direct public connections.
        .no_proxy()
        .user_agent(format!(
            "MaestroEditorialAI/{} (+https://github.com/LCV-Ideas-Software/maestro-app)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|error| format!("failed to create guarded HTTP client: {}", error.without_url()))
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn safe_response_headers(response: &Response) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for name in [
        CACHE_CONTROL,
        CONTENT_DISPOSITION,
        CONTENT_LENGTH,
        CONTENT_TYPE,
        ETAG,
        LAST_MODIFIED,
        LOCATION,
    ] {
        if let Some(value) = response.headers().get(&name).and_then(|value| value.to_str().ok()) {
            result.insert(
                name.as_str().to_string(),
                sanitize_text(value, 1_024),
            );
        }
    }
    result
}

fn execute_public_request(
    client: &Client,
    method: WebEvidenceMethod,
    initial_url: &Url,
    initial_headers: &[(HeaderName, HeaderValue)],
    max_body_bytes: usize,
) -> Result<RawHttpResponse, String> {
    let started = Instant::now();
    let original_origin = initial_url.clone();
    let mut current = initial_url.clone();
    let mut seen = BTreeSet::new();
    let mut redirects = Vec::new();

    loop {
        current = validate_public_url(current.as_str())?;
        if !seen.insert(current.as_str().to_string()) {
            return Err("redirect loop detected".to_string());
        }

        let mut request = client.request(method.reqwest(), current.clone());
        // Conditional/auth headers are origin-bound. This prevents API keys,
        // validators, or tracking values from crossing a redirect boundary.
        if same_origin(&original_origin, &current) {
            for (name, value) in initial_headers {
                request = request.header(name, value);
            }
        }
        let response = request
            .send()
            .map_err(|error| format!("HTTP request failed: {}", error.without_url()))?;
        let status = response.status();

        if status.is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| format!("HTTP {} redirect omitted Location", status.as_u16()))?;
            if redirects.len() >= MAX_REDIRECTS {
                return Err(format!("redirect chain exceeded {MAX_REDIRECTS} hops"));
            }
            let next = current
                .join(location)
                .map_err(|_| "redirect Location was invalid".to_string())?;
            let next = validate_public_url(next.as_str())?;
            if !same_origin(&original_origin, &next) && !initial_headers.is_empty() {
                return Err(
                    "cross-origin redirect blocked because the request uses origin-bound headers"
                        .to_string(),
                );
            }
            redirects.push(WebEvidenceRedirect {
                url: sanitize_text(next.as_str(), 2_048),
                status: status.as_u16(),
            });
            current = next;
            continue;
        }

        let headers = safe_response_headers(&response);
        if method == WebEvidenceMethod::Head || status == StatusCode::NOT_MODIFIED {
            return Ok(RawHttpResponse {
                status: status.as_u16(),
                final_url: sanitize_text(current.as_str(), 2_048),
                headers,
                body: Vec::new(),
                duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                redirects,
            });
        }

        if response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|length| length > max_body_bytes as u64)
        {
            return Err(format!(
                "HTTP body exceeds the {} byte evidence limit",
                max_body_bytes
            ));
        }

        let mut body = Vec::with_capacity(max_body_bytes.min(64 * 1024));
        response
            .take((max_body_bytes + 1) as u64)
            .read_to_end(&mut body)
            .map_err(|error| format!("failed to read HTTP response body: {error}"))?;
        if body.len() > max_body_bytes {
            return Err(format!(
                "HTTP body exceeds the {} byte evidence limit",
                max_body_bytes
            ));
        }

        return Ok(RawHttpResponse {
            status: status.as_u16(),
            final_url: sanitize_text(current.as_str(), 2_048),
            headers,
            body,
            duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            redirects,
        });
    }
}

fn robots_url_for(url: &Url) -> Result<Url, String> {
    let mut robots = url.clone();
    robots.set_path("/robots.txt");
    robots.set_query(None);
    robots.set_fragment(None);
    validate_public_url(robots.as_str())
}

fn robots_state_for(client: &Client, url: &Url) -> WebEvidenceRobotsState {
    let Ok(robots_url) = robots_url_for(url) else {
        return WebEvidenceRobotsState::Unavailable;
    };
    let Ok(response) = execute_public_request(
        client,
        WebEvidenceMethod::Get,
        &robots_url,
        &[],
        MAX_ROBOTS_BYTES,
    ) else {
        return WebEvidenceRobotsState::Unavailable;
    };
    match response.status {
        401 | 403 => return WebEvidenceRobotsState::Disallowed,
        404 | 410 => return WebEvidenceRobotsState::Allowed,
        200..=299 => {}
        _ => return WebEvidenceRobotsState::Unavailable,
    }
    let text = String::from_utf8_lossy(&response.body);
    if robots_disallows_path(&text, url.path()) {
        WebEvidenceRobotsState::Disallowed
    } else {
        WebEvidenceRobotsState::Allowed
    }
}

fn robots_disallows_path(robots: &str, target_path: &str) -> bool {
    let mut group_applies = false;
    let mut group_has_rules = false;
    let mut best_match: Option<(usize, bool)> = None;
    for raw_line in robots.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        let field = field.trim().to_ascii_lowercase();
        let value = value.trim();
        if field == "user-agent" {
            if group_has_rules {
                group_applies = false;
                group_has_rules = false;
            }
            let agent = value.to_ascii_lowercase();
            group_applies |= agent == "*"
                || agent == "maestroeditorialai"
                || agent.starts_with("maestroeditorialai/");
            continue;
        }
        if field == "allow" || field == "disallow" {
            group_has_rules = true;
        }
        if !group_applies || (field != "allow" && field != "disallow") || value.is_empty() {
            continue;
        }
        let pattern = value.split(['*', '$']).next().unwrap_or_default();
        if pattern.is_empty() || !target_path.starts_with(pattern) {
            continue;
        }
        let candidate = (pattern.len(), field == "allow");
        if best_match
            .as_ref()
            .map(|current| candidate.0 >= current.0)
            .unwrap_or(true)
        {
            best_match = Some(candidate);
        }
    }
    matches!(best_match, Some((_, false)))
}

fn curl_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn direct_curl_command(method: WebEvidenceMethod, url: &str) -> String {
    format!(
        "curl.exe --fail-with-body --noproxy '*' --proto '=http,https' --max-redirs 0 --request {} --user-agent {} --url {}",
        method.as_str(),
        curl_quote(&format!("MaestroEditorialAI/{}", env!("CARGO_PKG_VERSION"))),
        curl_quote(url)
    )
}

fn content_extension(content_type: Option<&str>, body: &[u8]) -> &'static str {
    let content_type = content_type
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if content_type == "application/pdf" || body.starts_with(b"%PDF-") {
        "pdf"
    } else if content_type.contains("html") {
        "html"
    } else if content_type.contains("markdown") {
        "md"
    } else if content_type.contains("json") {
        "json"
    } else if content_type == "image/png" {
        "png"
    } else if content_type == "image/jpeg" {
        "jpg"
    } else if content_type == "image/webp" {
        "webp"
    } else if content_type.starts_with("text/") {
        "txt"
    } else {
        "bin"
    }
}

fn extract_html_title(body: &[u8], content_type: Option<&str>) -> Option<String> {
    if !content_type
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("html")
    {
        return None;
    }
    let sample = String::from_utf8_lossy(&body[..body.len().min(256 * 1024)]);
    let regex = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").ok()?;
    let raw = regex.captures(&sample)?.get(1)?.as_str();
    let tags = Regex::new(r"(?is)<[^>]+>").ok()?;
    let title = sanitize_text(tags.replace_all(raw, " ").trim(), 240);
    (!title.is_empty()).then_some(title)
}

fn classify_interaction(status: u16, content_type: Option<&str>, body: &[u8]) -> WebEvidenceInteractionState {
    if status == 401 || status == 403 {
        return WebEvidenceInteractionState::LoginRequired;
    }
    if !content_type
        .unwrap_or_default()
        .to_ascii_lowercase()
        .starts_with("text/")
        && !content_type
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains("html")
    {
        return WebEvidenceInteractionState::None;
    }
    let sample = String::from_utf8_lossy(&body[..body.len().min(128 * 1024)]).to_ascii_lowercase();
    if sample.contains("captcha") || sample.contains("cf-chl-") || sample.contains("hcaptcha") {
        WebEvidenceInteractionState::CaptchaRequired
    } else if sample.contains("sign in")
        || sample.contains("log in")
        || sample.contains("login required")
    {
        WebEvidenceInteractionState::LoginRequired
    } else if sample.contains("cookie consent")
        || sample.contains("consent required")
        || sample.contains("manage cookies")
    {
        WebEvidenceInteractionState::ConsentRequired
    } else if sample.contains("paywall")
        || sample.contains("subscribe to continue")
        || sample.contains("subscriber-only")
    {
        WebEvidenceInteractionState::Paywall
    } else if sample.contains("confirm download") || sample.contains("download confirmation") {
        WebEvidenceInteractionState::DownloadConfirmation
    } else {
        WebEvidenceInteractionState::None
    }
}

fn interaction_requires_operator(state: WebEvidenceInteractionState) -> bool {
    !matches!(
        state,
        WebEvidenceInteractionState::None | WebEvidenceInteractionState::HumanResolved
    )
}

fn conditional_headers(stored: Option<&StoredWebEvidence>) -> Vec<(HeaderName, HeaderValue)> {
    let mut result = Vec::new();
    let Some(stored) = stored else {
        return result;
    };
    for (source, target) in [("etag", IF_NONE_MATCH), ("last-modified", IF_MODIFIED_SINCE)] {
        if let Some(value) = stored.response_headers.get(source) {
            if let Ok(value) = HeaderValue::from_str(value) {
                result.push((target, value));
            }
        }
    }
    result
}

fn persist_content(id: &str, content_type: Option<&str>, bytes: &[u8]) -> Result<Option<String>, String> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let extension = content_extension(content_type, bytes);
    let path = content_path(id, extension)?;
    write_binary_file(&path, bytes)?;
    let relative = path
        .strip_prefix(evidence_dir()?)
        .map_err(|_| "web evidence content path escaped evidence directory".to_string())?;
    Ok(Some(relative.to_string_lossy().replace('\\', "/")))
}

fn failed_fetch_record(
    existing: Option<&StoredWebEvidence>,
    id: &str,
    url: &str,
    method: WebEvidenceMethod,
    state: WebEvidenceState,
    note: &str,
    now: DateTime<Utc>,
) -> StoredWebEvidence {
    let mut record = base_record(
        id.to_string(),
        url,
        method,
        WebEvidenceAccessMode::HttpFetch,
        now,
    );
    if let Some(existing) = existing {
        record.created_at = existing.record.created_at.clone();
    }
    record.state = state;
    record.updated_at = now.to_rfc3339();
    record.notes.push(sanitize_text(note, 500));
    record.curl_command = Some(direct_curl_command(method, url));
    StoredWebEvidence {
        record,
        content_path: None,
        response_headers: BTreeMap::new(),
        replay: ReplayRecipe::HttpFetch {
            url: url.to_string(),
            method,
        },
    }
}

pub(crate) fn fetch_web_evidence_inner(
    app: Option<&tauri::AppHandle>,
    request: WebEvidenceFetchRequest,
) -> Result<WebEvidenceRecord, String> {
    let now = Utc::now();
    let preliminary_id = evidence_id(&format!(
        "http_fetch|{}|{}",
        request.method.as_str(),
        request.url
    ));
    emit_progress(app, "fetch", Some(&preliminary_id), "validating", "Validando destino publico");

    let validated = match validate_public_url(&request.url) {
        Ok(url) => url,
        Err(error) => {
            let stored = failed_fetch_record(
                None,
                &preliminary_id,
                &request.url,
                request.method,
                WebEvidenceState::Blocked,
                &error,
                now,
            );
            save_stored(&stored)?;
            append_event("fetch_blocked", &stored.record)?;
            emit_progress(app, "fetch", Some(&stored.record.id), "blocked", &error);
            return Ok(stored.record);
        }
    };
    let canonical_url = validated.as_str().to_string();
    let id = evidence_id(&format!(
        "http_fetch|{}|{}",
        request.method.as_str(),
        canonical_url
    ));
    let existing = if record_path(&id)?.exists() {
        Some(load_stored(&id)?)
    } else {
        None
    };
    if !request.force_revalidate {
        if let Some(stored) = existing.as_ref() {
            let record = project_record(stored.record.clone(), now);
            if record.state == WebEvidenceState::Ready
                && record.cache_state == WebEvidenceCacheState::Fresh
            {
                emit_progress(app, "fetch", Some(&id), "cache_hit", "Evidencia fresca reutilizada");
                return Ok(record);
            }
        }
    }

    let client = match build_public_http_client() {
        Ok(client) => client,
        Err(error) => {
            let stored = failed_fetch_record(
                existing.as_ref(),
                &id,
                &canonical_url,
                request.method,
                WebEvidenceState::Failed,
                &error,
                now,
            );
            save_stored(&stored)?;
            append_event("fetch_failed", &stored.record)?;
            return Ok(stored.record);
        }
    };

    emit_progress(app, "fetch", Some(&id), "robots", "Verificando politica robots.txt");
    let robots_state = robots_state_for(&client, &validated);
    if robots_state == WebEvidenceRobotsState::Disallowed {
        let mut stored = failed_fetch_record(
            existing.as_ref(),
            &id,
            &canonical_url,
            request.method,
            WebEvidenceState::Blocked,
            "robots.txt disallows automatic collection for this path",
            now,
        );
        stored.record.robots_state = robots_state;
        save_stored(&stored)?;
        append_event("fetch_robots_disallowed", &stored.record)?;
        emit_progress(app, "fetch", Some(&id), "blocked", "Coleta automatica bloqueada por robots.txt");
        return Ok(stored.record);
    }

    emit_progress(app, "fetch", Some(&id), "requesting", "Coletando evidencia HTTP");
    let headers = if request.force_revalidate {
        conditional_headers(existing.as_ref())
    } else {
        Vec::new()
    };
    let raw = match execute_public_request(
        &client,
        request.method,
        &validated,
        &headers,
        MAX_HTTP_BODY_BYTES,
    ) {
        Ok(raw) => raw,
        Err(error) => {
            let mut stored = failed_fetch_record(
                existing.as_ref(),
                &id,
                &canonical_url,
                request.method,
                WebEvidenceState::Failed,
                &error,
                now,
            );
            stored.record.robots_state = robots_state;
            save_stored(&stored)?;
            append_event("fetch_failed", &stored.record)?;
            emit_progress(app, "fetch", Some(&id), "failed", &error);
            return Ok(stored.record);
        }
    };

    if raw.status == StatusCode::NOT_MODIFIED.as_u16() {
        let Some(mut stored) = existing else {
            return Err("received HTTP 304 without a cached evidence record".to_string());
        };
        let retrieved_at = Utc::now();
        stored.record.state = WebEvidenceState::Ready;
        stored.record.cache_state = WebEvidenceCacheState::Fresh;
        stored.record.retrieved_at = Some(retrieved_at.to_rfc3339());
        stored.record.expires_at = Some(
            (retrieved_at.clone() + ChronoDuration::days(DEFAULT_CACHE_TTL_DAYS)).to_rfc3339(),
        );
        stored.record.updated_at = retrieved_at.to_rfc3339();
        stored.record.duration_ms = Some(raw.duration_ms);
        stored.record.robots_state = robots_state;
        stored.record.redirect_chain = raw.redirects;
        stored
            .record
            .notes
            .push("Cache revalidated by HTTP 304; content hash preserved".to_string());
        for (key, value) in raw.headers {
            stored.response_headers.insert(key, value);
        }
        save_stored(&stored)?;
        append_event("fetch_revalidated", &stored.record)?;
        emit_progress(app, "fetch", Some(&id), "ready", "Cache revalidado sem nova transferencia");
        return Ok(stored.record);
    }

    let retrieved_at = Utc::now();
    let content_type = raw.headers.get("content-type").cloned();
    let interaction = classify_interaction(raw.status, content_type.as_deref(), &raw.body);
    let state = if interaction_requires_operator(interaction) {
        WebEvidenceState::OperatorActionRequired
    } else if (200..=299).contains(&raw.status) {
        WebEvidenceState::Ready
    } else {
        WebEvidenceState::Failed
    };
    let content_sha = (!raw.body.is_empty()).then(|| sha256_bytes(&raw.body));
    // A failed/challenged refresh must never overwrite the bytes belonging to
    // the last ready record. Interactive pages only become evidence after an
    // explicit operator import.
    let stored_content_path = if state == WebEvidenceState::Ready {
        persist_content(&id, content_type.as_deref(), &raw.body)?
    } else {
        None
    };
    let mut record = base_record(
        id.clone(),
        &canonical_url,
        request.method,
        WebEvidenceAccessMode::HttpFetch,
        now,
    );
    if let Some(existing) = existing.as_ref() {
        record.created_at = existing.record.created_at.clone();
    }
    record.state = state;
    record.status = Some(raw.status);
    record.final_url = Some(raw.final_url.clone());
    record.title = extract_html_title(&raw.body, content_type.as_deref());
    record.content_type = content_type.clone();
    record.sha256 = content_sha;
    record.retrieved_at = Some(retrieved_at.to_rfc3339());
    record.expires_at = Some(
        (retrieved_at.clone() + ChronoDuration::days(DEFAULT_CACHE_TTL_DAYS)).to_rfc3339(),
    );
    record.cache_state = if state == WebEvidenceState::Ready {
        WebEvidenceCacheState::Fresh
    } else {
        WebEvidenceCacheState::Missing
    };
    record.robots_state = robots_state;
    record.interaction_state = interaction;
    record.byte_count = Some(raw.body.len() as u64);
    record.duration_ms = Some(raw.duration_ms);
    record.redirect_chain = raw.redirects;
    record.curl_command = Some(direct_curl_command(request.method, &canonical_url));
    record.updated_at = retrieved_at.to_rfc3339();
    if interaction_requires_operator(interaction) {
        record.notes.push(
            "Automatic collection reached an interaction boundary; use isolated rendering or operator capture"
                .to_string(),
        );
    }
    if content_extension(content_type.as_deref(), &raw.body) == "pdf" {
        record.notes.push(
            "PDF detected and hashed; text extraction was not attempted because no trusted extractor is configured"
                .to_string(),
        );
    }
    let stored = StoredWebEvidence {
        record,
        content_path: stored_content_path,
        response_headers: raw.headers,
        replay: ReplayRecipe::HttpFetch {
            url: canonical_url,
            method: request.method,
        },
    };
    save_stored(&stored)?;
    append_event("fetch_completed", &stored.record)?;
    emit_progress(
        app,
        "fetch",
        Some(&id),
        if state == WebEvidenceState::Ready { "ready" } else { "attention" },
        "Coleta HTTP persistida com proveniencia e hash",
    );
    Ok(stored.record)
}

#[tauri::command]
pub(crate) async fn fetch_web_evidence(
    app: tauri::AppHandle,
    request: WebEvidenceFetchRequest,
) -> Result<WebEvidenceRecord, String> {
    tauri::async_runtime::spawn_blocking(move || fetch_web_evidence_inner(Some(&app), request))
        .await
        .map_err(|error| format!("web evidence fetch worker failed: {error}"))?
}

#[tauri::command]
pub(crate) fn get_web_evidence(evidence_id: String) -> Result<WebEvidenceRecord, String> {
    let stored = load_stored(&evidence_id)?;
    Ok(project_record(stored.record, Utc::now()))
}

#[tauri::command]
pub(crate) fn list_web_evidence(
    request: WebEvidenceListRequest,
) -> Result<WebEvidenceListResult, String> {
    let mut items = Vec::new();
    let now = Utc::now();
    for entry in fs::read_dir(records_dir()?)
        .map_err(|error| format!("failed to list web evidence records: {error}"))?
    {
        let entry = entry.map_err(|error| format!("failed to inspect web evidence entry: {error}"))?;
        if !entry.file_type().map(|kind| kind.is_file()).unwrap_or(false) {
            continue;
        }
        let Some(stem) = entry.path().file_stem().and_then(|value| value.to_str()).map(str::to_string) else {
            continue;
        };
        if !is_valid_evidence_id(&stem) {
            continue;
        }
        let stored = load_stored(&stem)?;
        let record = project_record(stored.record, now.clone());
        if !request.states.is_empty() && !request.states.contains(&record.state) {
            continue;
        }
        if !request.access_modes.is_empty() && !request.access_modes.contains(&record.access_mode) {
            continue;
        }
        if request.stale_only && record.cache_state != WebEvidenceCacheState::Stale {
            continue;
        }
        if let Some(query) = request.query.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            let query = query.to_ascii_lowercase();
            let haystack = format!(
                "{} {} {} {} {}",
                record.url,
                record.title.as_deref().unwrap_or_default(),
                record.provider.as_deref().unwrap_or_default(),
                record.query.as_deref().unwrap_or_default(),
                record.notes.join(" ")
            )
            .to_ascii_lowercase();
            if !haystack.contains(&query) {
                continue;
            }
        }
        items.push(record);
    }
    items.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let total = items.len();
    let offset = request
        .cursor
        .as_deref()
        .unwrap_or("0")
        .parse::<usize>()
        .map_err(|_| "invalid web evidence cursor".to_string())?;
    if offset > total {
        return Err("web evidence cursor is outside the result set".to_string());
    }
    let limit = request.limit.unwrap_or(30).clamp(1, MAX_LIST_RESULTS);
    let end = offset.saturating_add(limit).min(total);
    let next_cursor = (end < total).then(|| end.to_string());
    Ok(WebEvidenceListResult {
        items: items[offset..end].to_vec(),
        next_cursor,
        total,
    })
}

fn built_in_search_connectors() -> Vec<SearchConnector> {
    vec![
        SearchConnector {
            id: "crossref".to_string(),
            label: "Crossref".to_string(),
            endpoint: "https://api.crossref.org/works".to_string(),
            query_parameter: "query.bibliographic".to_string(),
            limit_parameter: "rows".to_string(),
            results_path: "message.items".to_string(),
            title_field: "title".to_string(),
            url_field: "URL".to_string(),
            snippet_field: Some("publisher".to_string()),
            api_key_env_var: None,
            api_key_header: None,
        },
        SearchConnector {
            id: "openalex".to_string(),
            label: "OpenAlex".to_string(),
            endpoint: "https://api.openalex.org/works".to_string(),
            query_parameter: "search".to_string(),
            limit_parameter: "per-page".to_string(),
            results_path: "results".to_string(),
            title_field: "display_name".to_string(),
            url_field: "id".to_string(),
            snippet_field: Some("doi".to_string()),
            api_key_env_var: None,
            api_key_header: None,
        },
    ]
}

fn valid_connector_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn valid_parameter_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '[' | ']')
        })
}

fn validate_search_connector(connector: &SearchConnector) -> Result<(), String> {
    if !valid_connector_identifier(&connector.id) {
        return Err("search connector id contains unsupported characters".to_string());
    }
    if connector.label.trim().is_empty() || connector.label.len() > 120 {
        return Err(format!("search connector '{}' has an invalid label", connector.id));
    }
    let endpoint = validate_public_url(&connector.endpoint)?;
    if endpoint.query().is_some() || endpoint.fragment().is_some() {
        return Err(format!(
            "search connector '{}' endpoint must not embed query parameters or fragments",
            connector.id
        ));
    }
    if !valid_parameter_name(&connector.query_parameter)
        || !valid_parameter_name(&connector.limit_parameter)
    {
        return Err(format!(
            "search connector '{}' has an invalid query/limit parameter",
            connector.id
        ));
    }
    for field in [
        connector.results_path.as_str(),
        connector.title_field.as_str(),
        connector.url_field.as_str(),
    ] {
        if field.is_empty()
            || field.len() > 160
            || !field
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
        {
            return Err(format!(
                "search connector '{}' contains an invalid response field path",
                connector.id
            ));
        }
    }
    match (
        connector.api_key_env_var.as_deref(),
        connector.api_key_header.as_deref(),
    ) {
        (None, None) => {}
        (Some(env_var), Some(header)) => {
            if env_var.is_empty()
                || env_var.len() > 128
                || !env_var
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
            {
                return Err(format!(
                    "search connector '{}' has an invalid credential environment variable name",
                    connector.id
                ));
            }
            HeaderName::from_bytes(header.as_bytes()).map_err(|_| {
                format!("search connector '{}' has an invalid API header name", connector.id)
            })?;
        }
        _ => {
            return Err(format!(
                "search connector '{}' must configure both api_key_env_var and api_key_header, or neither",
                connector.id
            ));
        }
    }
    Ok(())
}

fn load_search_connectors() -> Result<Vec<SearchConnector>, String> {
    let mut connectors = built_in_search_connectors();
    let config_path = checked_data_child_path(
        &data_dir()
            .join("config")
            .join("web-evidence-search.json"),
    )?;
    if config_path.exists() {
        let encoded = read_text_file(&config_path)?;
        let configured: SearchConnectorFile = serde_json::from_str(&encoded)
            .map_err(|error| format!("invalid web evidence search connector config: {error}"))?;
        if configured.schema_version != "web_evidence_search_connectors.v1" {
            return Err("unsupported web evidence search connector schema_version".to_string());
        }
        for connector in configured.connectors {
            validate_search_connector(&connector)?;
            if let Some(index) = connectors.iter().position(|value| value.id == connector.id) {
                connectors[index] = connector;
            } else {
                connectors.push(connector);
            }
        }
    }
    for connector in &connectors {
        validate_search_connector(connector)?;
    }
    Ok(connectors)
}

fn json_value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.').filter(|segment| !segment.is_empty()) {
        current = current.get(segment)?;
    }
    Some(current)
}

fn json_field_text(value: &Value, field: &str) -> Option<String> {
    let value = json_value_at_path(value, field)?;
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(values) => values.iter().find_map(|value| value.as_str().map(str::to_string)),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn search_curl_command(connector: &SearchConnector, url: &Url) -> String {
    let mut command = format!(
        "curl.exe --fail-with-body --noproxy '*' --proto '=https' --max-redirs 0 --request GET --user-agent {} --url {}",
        curl_quote(&format!("MaestroEditorialAI/{}", env!("CARGO_PKG_VERSION"))),
        curl_quote(url.as_str())
    );
    if let (Some(env_var), Some(header)) = (
        connector.api_key_env_var.as_deref(),
        connector.api_key_header.as_deref(),
    ) {
        command.push_str(&format!(
            " --header {}",
            curl_quote(&format!("{header}: <redacted:{env_var}>"))
        ));
    }
    command
}

fn persist_search_response(id: &str, bytes: &[u8]) -> Result<(), String> {
    if !is_valid_evidence_id(id) {
        return Err("invalid search response id".to_string());
    }
    let path = checked_data_child_path(&searches_dir()?.join(format!("{id}.json")))?;
    write_binary_file(&path, bytes)
}

pub(crate) fn search_web_evidence_inner(
    app: Option<&tauri::AppHandle>,
    request: WebEvidenceSearchRequest,
) -> Result<WebEvidenceSearchResult, String> {
    let query = sanitize_text(request.query.trim(), 500);
    if query.is_empty() {
        return Err("web evidence search query cannot be empty".to_string());
    }
    let provider = sanitize_short(request.provider.trim(), 80);
    if !valid_connector_identifier(&provider) {
        return Err("invalid web evidence search provider".to_string());
    }
    let limit = request.limit.clamp(1, MAX_SEARCH_RESULTS);
    let connector = load_search_connectors()?
        .into_iter()
        .find(|connector| connector.id == provider)
        .ok_or_else(|| {
            format!(
                "unknown web evidence search provider '{provider}'; available built-ins are crossref and openalex"
            )
        })?;
    let mut url = validate_public_url(&connector.endpoint)?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair(&connector.query_parameter, &query);
        pairs.append_pair(&connector.limit_parameter, &limit.to_string());
    }
    let mut headers = Vec::new();
    if let (Some(env_var), Some(header)) = (
        connector.api_key_env_var.as_deref(),
        connector.api_key_header.as_deref(),
    ) {
        let secret = std::env::var(env_var).map_err(|_| {
            format!(
                "search provider '{}' requires environment variable '{}'",
                connector.id, env_var
            )
        })?;
        let name = HeaderName::from_bytes(header.as_bytes())
            .map_err(|_| format!("search provider '{}' API header is invalid", connector.id))?;
        let value = HeaderValue::from_str(&secret)
            .map_err(|_| format!("search provider '{}' API credential is invalid", connector.id))?;
        headers.push((name, value));
    }
    let client = build_public_http_client()?;
    emit_progress(app, "search", None, "requesting", &format!("Consultando {}", connector.label));
    let raw = execute_public_request(
        &client,
        WebEvidenceMethod::Get,
        &url,
        &headers,
        MAX_HTTP_BODY_BYTES,
    )?;
    if !(200..=299).contains(&raw.status) {
        return Err(format!(
            "search provider '{}' returned HTTP {}",
            connector.id, raw.status
        ));
    }
    let parsed: Value = serde_json::from_slice(&raw.body).map_err(|error| {
        format!(
            "search provider '{}' returned invalid JSON: {error}",
            connector.id
        )
    })?;
    let values = json_value_at_path(&parsed, &connector.results_path)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!(
                "search provider '{}' response did not contain array '{}'",
                connector.id, connector.results_path
            )
        })?;
    let search_id = evidence_id(&format!("search|{}|{}|{}", connector.id, query, limit));
    persist_search_response(&search_id, &raw.body)?;
    let now = Utc::now();
    let curl = search_curl_command(&connector, &url);
    let mut items = Vec::new();
    for value in values.iter().take(limit) {
        let Some(result_url) = json_field_text(value, &connector.url_field) else {
            continue;
        };
        let Ok(result_url) = validate_public_url(&result_url) else {
            continue;
        };
        let result_url = result_url.as_str().to_string();
        let id = evidence_id(&format!(
            "official_api|{}|{}|{}",
            connector.id, query, result_url
        ));
        let hit_bytes = serde_json::to_vec(value)
            .map_err(|error| format!("failed to serialize search result evidence: {error}"))?;
        let relative_path = persist_content(&id, Some("application/json"), &hit_bytes)?;
        let mut record = base_record(
            id.clone(),
            &result_url,
            WebEvidenceMethod::Get,
            WebEvidenceAccessMode::OfficialApi,
            now.clone(),
        );
        if record_path(&id)?.exists() {
            record.created_at = load_stored(&id)?.record.created_at;
        }
        record.state = WebEvidenceState::Ready;
        record.status = Some(raw.status);
        record.final_url = Some(result_url.clone());
        record.title = json_field_text(value, &connector.title_field)
            .map(|value| sanitize_text(&value, 240));
        record.content_type = Some("application/json".to_string());
        record.sha256 = Some(sha256_bytes(&hit_bytes));
        record.retrieved_at = Some(now.to_rfc3339());
        record.expires_at = Some(
            (now.clone() + ChronoDuration::days(DEFAULT_CACHE_TTL_DAYS)).to_rfc3339(),
        );
        record.cache_state = WebEvidenceCacheState::Fresh;
        record.byte_count = Some(hit_bytes.len() as u64);
        record.duration_ms = Some(raw.duration_ms);
        record.redirect_chain = raw.redirects.clone();
        record.curl_command = Some(curl.clone());
        record.provider = Some(connector.id.clone());
        record.query = Some(query.clone());
        record.updated_at = now.to_rfc3339();
        record.notes.push(format!(
            "Metadata returned by the {} official/configured API; target page was not fetched",
            sanitize_text(&connector.label, 120)
        ));
        if let Some(snippet_field) = connector.snippet_field.as_deref() {
            if let Some(snippet) = json_field_text(value, snippet_field) {
                record.notes.push(sanitize_text(&snippet, 500));
            }
        }
        let stored = StoredWebEvidence {
            record: record.clone(),
            content_path: relative_path,
            response_headers: raw.headers.clone(),
            replay: ReplayRecipe::Search {
                provider: connector.id.clone(),
                query: query.clone(),
                limit,
                result_url: result_url.clone(),
            },
        };
        save_stored(&stored)?;
        append_event("search_result_persisted", &record)?;
        items.push(record);
    }
    emit_progress(
        app,
        "search",
        None,
        "ready",
        &format!("{} resultado(s) de busca persistido(s)", items.len()),
    );
    let total = items.len();
    Ok(WebEvidenceSearchResult {
        query,
        provider: connector.id,
        items,
        total,
    })
}

#[tauri::command]
pub(crate) async fn search_web_evidence(
    app: tauri::AppHandle,
    request: WebEvidenceSearchRequest,
) -> Result<WebEvidenceSearchResult, String> {
    tauri::async_runtime::spawn_blocking(move || search_web_evidence_inner(Some(&app), request))
        .await
        .map_err(|error| format!("web evidence search worker failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn replay_web_evidence(
    app: tauri::AppHandle,
    request: WebEvidenceReplayRequest,
) -> Result<WebEvidenceRecord, String> {
    let stored = load_stored(&request.evidence_id)?;
    let previous = stored.clone();
    let refreshed = match stored.replay {
        ReplayRecipe::HttpFetch { url, method } => {
            tauri::async_runtime::spawn_blocking(move || {
                fetch_web_evidence_inner(
                    Some(&app),
                    WebEvidenceFetchRequest {
                        url,
                        method,
                        force_revalidate: true,
                    },
                )
            })
            .await
            .map_err(|error| format!("web evidence replay worker failed: {error}"))?
        }
        ReplayRecipe::Search {
            provider,
            query,
            limit,
            result_url,
        } => {
            let result = tauri::async_runtime::spawn_blocking(move || {
                search_web_evidence_inner(
                    Some(&app),
                    WebEvidenceSearchRequest {
                        query,
                        provider,
                        limit,
                    },
                )
            })
            .await
            .map_err(|error| format!("web evidence search replay worker failed: {error}"))??;
            result
                .items
                .into_iter()
                .find(|record| record.url == result_url)
                .ok_or_else(|| "replayed search no longer returned the selected result".to_string())
        }
        ReplayRecipe::OperatorImport => Err(
            "operator-provided evidence cannot be replayed automatically; import a new explicit capture"
                .to_string(),
        ),
        ReplayRecipe::BrowserHandoff => Err(
            "browser handoff has no automatic replay; reopen the isolated/default browser and import the capture"
                .to_string(),
        ),
    }?;

    if refreshed.state != WebEvidenceState::Ready {
        // The attempted refresh may have persisted diagnostics under the same
        // deterministic ID. Restore the complete last-known-good record; the
        // failed attempt remains available in the append-only event ledger.
        save_stored(&previous)?;
        return Err(format!(
            "replay did not produce ready evidence (state {:?}); previous evidence was preserved",
            refreshed.state
        ));
    }

    let mut current = load_stored(&refreshed.id)?;
    let comparison = match (
        previous.record.sha256.as_deref(),
        current.record.sha256.as_deref(),
    ) {
        (Some(before), Some(after)) if before == after => {
            "Replay hash comparison: content is unchanged".to_string()
        }
        (Some(before), Some(after)) => format!(
            "Replay hash comparison: content changed from {} to {}",
            before.chars().take(12).collect::<String>(),
            after.chars().take(12).collect::<String>()
        ),
        _ => "Replay completed; a content hash comparison was not available".to_string(),
    };
    current.record.notes.push(comparison);
    current.record.updated_at = Utc::now().to_rfc3339();
    save_stored(&current)?;
    append_event("replay_completed", &current.record)?;
    Ok(current.record)
}

fn handoff_record(
    url: &str,
    access_mode: WebEvidenceAccessMode,
    note: &str,
) -> Result<StoredWebEvidence, String> {
    let validated = validate_public_url(url)?;
    let url = validated.as_str().to_string();
    let now = Utc::now();
    let id = evidence_id(&format!("browser_handoff|{:?}|{}", access_mode, url));
    let existing = if record_path(&id)?.exists() {
        Some(load_stored(&id)?)
    } else {
        None
    };
    let mut record = base_record(id, &url, WebEvidenceMethod::Get, access_mode, now.clone());
    if let Some(existing) = existing.as_ref() {
        record.created_at = existing.record.created_at.clone();
    }
    record.state = WebEvidenceState::OperatorActionRequired;
    record.final_url = Some(url.clone());
    record.interaction_state = WebEvidenceInteractionState::None;
    record.notes.push(note.to_string());
    record.notes.push(
        "No browser cookies, active profile, password store, or captured page content is imported automatically"
            .to_string(),
    );
    record.updated_at = now.to_rfc3339();
    Ok(StoredWebEvidence {
        record,
        content_path: None,
        response_headers: BTreeMap::new(),
        replay: ReplayRecipe::BrowserHandoff,
    })
}

fn webview_data_dir() -> Result<PathBuf, String> {
    let path = checked_data_child_path(&data_dir().join("webview"))?;
    fs::create_dir_all(&path)
        .map_err(|error| format!("failed to create app-owned WebView2 data directory: {error}"))?;
    Ok(path)
}

#[tauri::command]
pub(crate) async fn start_rendered_web_evidence(
    app: tauri::AppHandle,
    request: WebEvidenceUrlRequest,
) -> Result<WebEvidenceRecord, String> {
    let request_url = request.url;
    let mut stored = tauri::async_runtime::spawn_blocking(move || {
        handoff_record(
            &request_url,
            WebEvidenceAccessMode::RenderedFetch,
            "Opened in an isolated, app-owned WebView2 session; operator import remains required to capture the rendered artifact",
        )
    })
    .await
    .map_err(|error| format!("rendered evidence validation worker failed: {error}"))??;
    let external_url = Url::parse(&stored.record.url)
        .map_err(|_| "validated rendered evidence URL could not be parsed".to_string())?;
    let label = format!("web-evidence-{}", &stored.record.id[..12]);
    if let Some(existing) = app.get_webview_window(&label) {
        existing
            .close()
            .map_err(|error| format!("failed to close previous evidence window: {error}"))?;
    }
    let navigation_guard = |url: &Url| validate_public_url(url.as_str()).is_ok();
    let build_result = WebviewWindowBuilder::new(
        &app,
        label,
        WebviewUrl::External(external_url),
    )
    .title("Maestro - coleta web isolada")
    .inner_size(1180.0, 820.0)
    .data_directory(webview_data_dir()?)
    .incognito(true)
    .browser_extensions_enabled(false)
    .general_autofill_enabled(false)
    .devtools(false)
    .on_navigation(navigation_guard)
    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
    .on_download(|_, _| false)
    .build();
    match build_result {
        Ok(_) => {
            stored.record.notes.push(
                "Top-level navigations are revalidated against the public-network policy; new windows and downloads are blocked"
                    .to_string(),
            );
            emit_progress(
                Some(&app),
                "rendered_fetch",
                Some(&stored.record.id),
                "operator_action_required",
                "Janela WebView2 isolada aberta; importe explicitamente o artefato capturado",
            );
        }
        Err(error) => {
            let safe_error = sanitize_text(
                &format!("isolated WebView2 could not be opened: {error}"),
                500,
            );
            stored.record.state = WebEvidenceState::Failed;
            stored.record.cache_state = WebEvidenceCacheState::Missing;
            stored.record.notes.push(safe_error.clone());
            stored.record.updated_at = Utc::now().to_rfc3339();
            save_stored(&stored)?;
            append_event("rendered_handoff_failed", &stored.record)?;
            emit_progress(
                Some(&app),
                "rendered_fetch",
                Some(&stored.record.id),
                "failed",
                "A janela WebView2 isolada nao pode ser aberta",
            );
            return Err(safe_error);
        }
    }
    stored.record.updated_at = Utc::now().to_rfc3339();
    save_stored(&stored)?;
    append_event("rendered_handoff", &stored.record)?;
    Ok(stored.record)
}

fn open_default_browser(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = {
        let mut command = hidden_command("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(url);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = hidden_command("open");
        command.arg(url);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = hidden_command("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to open system default browser: {error}"))
}

#[tauri::command]
pub(crate) async fn open_web_evidence_in_default_browser(
    app: tauri::AppHandle,
    request: WebEvidenceUrlRequest,
) -> Result<WebEvidenceRecord, String> {
    let request_url = request.url;
    let mut stored = tauri::async_runtime::spawn_blocking(move || {
        let mut stored = handoff_record(
            &request_url,
            WebEvidenceAccessMode::OperatorAssistedBrowserCapture,
            "Explicit operator-assisted handoff to the system default browser; Maestro does not read that browser profile",
        )?;
        match open_default_browser(&stored.record.url) {
            Ok(()) => stored.record.notes.push("Default-browser handoff launched".to_string()),
            Err(error) => stored.record.notes.push(sanitize_text(&error, 500)),
        }
        stored.record.updated_at = Utc::now().to_rfc3339();
        save_stored(&stored)?;
        append_event("default_browser_handoff", &stored.record)?;
        Ok::<StoredWebEvidence, String>(stored)
    })
    .await
    .map_err(|error| format!("default browser handoff worker failed: {error}"))??;
    emit_progress(
        Some(&app),
        "operator_capture",
        Some(&stored.record.id),
        "operator_action_required",
        "Exporte a pagina no navegador e importe o artefato no Maestro",
    );
    stored.record.cache_state = WebEvidenceCacheState::Missing;
    Ok(stored.record)
}

fn normalized_import_media_type(value: &str) -> Option<(&'static str, &'static str)> {
    let media_type = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match media_type.as_str() {
        "text/plain" => Some(("text/plain", "txt")),
        "text/html" => Some(("text/html", "html")),
        "text/markdown" | "text/x-markdown" => Some(("text/markdown", "md")),
        "application/pdf" => Some(("application/pdf", "pdf")),
        "application/json" => Some(("application/json", "json")),
        "image/png" => Some(("image/png", "png")),
        "image/jpeg" => Some(("image/jpeg", "jpg")),
        "image/webp" => Some(("image/webp", "webp")),
        _ => None,
    }
}

fn valid_artifact_name(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 180
        && trimmed != "."
        && trimmed != ".."
        && !trimmed
            .chars()
            .any(|character| matches!(character, '/' | '\\' | ':'))
        && trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(character, '_' | '-' | '.' | '(' | ')')
        })
}

fn validate_import_magic(media_type: &str, bytes: &[u8]) -> Result<(), String> {
    let valid = match media_type {
        "application/pdf" => bytes.starts_with(b"%PDF-"),
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        "application/json" => serde_json::from_slice::<Value>(bytes).is_ok(),
        _ => true,
    };
    valid
        .then_some(())
        .ok_or_else(|| "operator artifact bytes do not match the declared media type".to_string())
}

#[tauri::command]
pub(crate) async fn import_operator_evidence(
    app: tauri::AppHandle,
    request: WebEvidenceImportRequest,
) -> Result<WebEvidenceRecord, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !valid_artifact_name(&request.name) {
            return Err("operator artifact name is invalid or contains a path".to_string());
        }
        let (media_type, extension) = normalized_import_media_type(&request.media_type)
            .ok_or_else(|| "operator artifact media type is not allowlisted".to_string())?;
        let encoded = request.data_base64.trim();
        if encoded.len() > ((MAX_OPERATOR_ARTIFACT_BYTES + 2) / 3) * 4 + 8 {
            return Err(format!(
                "operator artifact exceeds the {} byte decoded limit",
                MAX_OPERATOR_ARTIFACT_BYTES
            ));
        }
        let bytes = BASE64_STANDARD
            .decode(encoded)
            .map_err(|_| "operator artifact is not valid base64".to_string())?;
        if bytes.is_empty() || bytes.len() > MAX_OPERATOR_ARTIFACT_BYTES {
            return Err(format!(
                "operator artifact must contain 1..={} decoded bytes",
                MAX_OPERATOR_ARTIFACT_BYTES
            ));
        }
        validate_import_magic(media_type, &bytes)?;
        let source_url = match request.url.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            Some(url) => Some(validate_public_url(url)?.as_str().to_string()),
            None => None,
        };
        let digest = sha256_bytes(&bytes);
        let id = evidence_id(&format!(
            "operator_import|{}|{}|{}",
            source_url.as_deref().unwrap_or_default(),
            request.name,
            digest
        ));
        let path = content_path(&id, extension)?;
        write_binary_file(&path, &bytes)?;
        let now = Utc::now();
        let display_url = source_url
            .clone()
            .unwrap_or_else(|| format!("maestro://operator-evidence/{id}"));
        let mut record = base_record(
            id.clone(),
            &display_url,
            WebEvidenceMethod::Get,
            WebEvidenceAccessMode::OperatorAssistedBrowserCapture,
            now.clone(),
        );
        if record_path(&id)?.exists() {
            record.created_at = load_stored(&id)?.record.created_at;
        }
        record.state = WebEvidenceState::Ready;
        record.final_url = source_url;
        record.content_type = Some(media_type.to_string());
        record.sha256 = Some(digest);
        record.retrieved_at = Some(now.to_rfc3339());
        record.expires_at = Some(
            (now.clone() + ChronoDuration::days(DEFAULT_CACHE_TTL_DAYS)).to_rfc3339(),
        );
        record.cache_state = WebEvidenceCacheState::Fresh;
        record.copyright_state = WebEvidenceCopyrightState::OperatorProvided;
        record.interaction_state = WebEvidenceInteractionState::HumanResolved;
        record.human_resolved = true;
        record.byte_count = Some(bytes.len() as u64);
        record.artifact_name = Some(sanitize_text(request.name.trim(), 180));
        record.notes = request
            .notes
            .iter()
            .take(20)
            .map(|note| sanitize_text(note, 500))
            .filter(|note| !note.is_empty())
            .collect();
        record
            .notes
            .push("Artifact explicitly supplied by the operator; no browser profile was read".to_string());
        record.updated_at = now.to_rfc3339();
        let relative = path
            .strip_prefix(evidence_dir()?)
            .map_err(|_| "operator evidence path escaped evidence directory".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let stored = StoredWebEvidence {
            record: record.clone(),
            content_path: Some(relative),
            response_headers: BTreeMap::new(),
            replay: ReplayRecipe::OperatorImport,
        };
        save_stored(&stored)?;
        append_event("operator_artifact_imported", &record)?;
        emit_progress(
            Some(&app),
            "operator_import",
            Some(&record.id),
            "ready",
            "Artefato do operador persistido e verificado por hash",
        );
        Ok(record)
    })
    .await
    .map_err(|error| format!("operator evidence import worker failed: {error}"))?
}

#[tauri::command]
pub(crate) fn resume_web_evidence_interaction(
    request: WebEvidenceInteractionRequest,
) -> Result<WebEvidenceRecord, String> {
    let mut stored = load_stored(&request.evidence_id)?;
    if !request.confirmed {
        return Ok(project_record(stored.record, Utc::now()));
    }
    if stored.record.state != WebEvidenceState::OperatorActionRequired {
        return Err("web evidence record is not waiting for operator interaction".to_string());
    }
    stored.record.human_resolved = true;
    stored.record.interaction_state = WebEvidenceInteractionState::HumanResolved;
    stored.record.notes.push(
        "Operator confirmed the interaction; explicit artifact import is still required before evidence becomes ready"
            .to_string(),
    );
    stored.record.updated_at = Utc::now().to_rfc3339();
    save_stored(&stored)?;
    append_event("operator_interaction_confirmed", &stored.record)?;
    Ok(stored.record)
}

fn valid_shared_chat_token(value: &str) -> bool {
    (1..=256).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn classify_shared_chat_url(value: &str) -> Result<ClassifiedSharedChatUrl, String> {
    let mut url = validate_public_url(value)?;
    if url.scheme() != "https" {
        return Err("shared-chat URLs must use HTTPS".to_string());
    }
    if url.port().is_some() {
        return Err("shared-chat URLs with explicit ports are not supported".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "shared-chat URL omitted its host".to_string())?
        .to_ascii_lowercase();
    let mut segments = url
        .path_segments()
        .ok_or_else(|| "shared-chat URL path could not be inspected".to_string())?
        .collect::<Vec<_>>();
    while segments.last().is_some_and(|segment| segment.is_empty()) {
        segments.pop();
    }

    let (provider, token, requires_gemini_redirect_validation) = match host.as_str() {
        "chatgpt.com" if segments.len() == 2 && segments[0] == "share" => {
            (SharedChatProvider::ChatGpt, segments[1], false)
        }
        "gemini.google.com" if segments.len() == 2 && segments[0] == "share" => {
            (SharedChatProvider::Gemini, segments[1], false)
        }
        "g.co"
            if segments.len() == 3
                && segments[0] == "gemini"
                && segments[1] == "share" =>
        {
            (SharedChatProvider::Gemini, segments[2], true)
        }
        "claude.ai" if segments.len() == 2 && segments[0] == "share" => {
            (SharedChatProvider::Claude, segments[1], false)
        }
        _ => {
            return Err(
                "URL is not a recognized public ChatGPT, Gemini, or Claude share URL"
                    .to_string(),
            )
        }
    };
    if !valid_shared_chat_token(token) {
        return Err("shared-chat URL contains an invalid or truncated share identifier".to_string());
    }
    let token = token.to_string();

    url.set_query(None);
    url.set_fragment(None);
    let normalized_path = match (provider, requires_gemini_redirect_validation) {
        (SharedChatProvider::Gemini, true) => format!("/gemini/share/{token}"),
        _ => format!("/share/{token}"),
    };
    url.set_path(&normalized_path);
    Ok(ClassifiedSharedChatUrl {
        provider,
        normalized_url: url.as_str().to_string(),
        requires_gemini_redirect_validation,
    })
}

fn canonical_url_from_evidence(
    classified: &ClassifiedSharedChatUrl,
    record: &WebEvidenceRecord,
) -> Result<String, SharedChatExtractionError> {
    let candidates = record
        .redirect_chain
        .iter()
        .map(|redirect| redirect.url.as_str())
        .chain(record.final_url.as_deref())
        .chain(std::iter::once(record.url.as_str()));

    for candidate in candidates {
        let Ok(destination) = classify_shared_chat_url(candidate) else {
            continue;
        };
        let same_share = classified.requires_gemini_redirect_validation
            || destination.normalized_url == classified.normalized_url;
        if destination.provider == classified.provider
            && !destination.requires_gemini_redirect_validation
            && same_share
        {
            return Ok(destination.normalized_url);
        }
    }

    if classified.requires_gemini_redirect_validation {
        return Err(SharedChatExtractionError::Invalid(
            "g.co Gemini share redirect did not resolve through a canonical gemini.google.com/share URL"
                .to_string(),
        ));
    }
    Err(SharedChatExtractionError::Insufficient(
        "the collected page did not remain on the recognized provider share surface".to_string(),
    ))
}

fn shared_chat_evidence_projection(record: &WebEvidenceRecord) -> SharedChatEvidenceProjection {
    let source_url = classify_shared_chat_url(&record.url)
        .map(|classified| classified.normalized_url)
        .unwrap_or_default();
    let final_url = record.final_url.as_deref().and_then(|value| {
        classify_shared_chat_url(value)
            .ok()
            .map(|classified| classified.normalized_url)
    });
    SharedChatEvidenceProjection {
        id: record.id.clone(),
        source_url,
        final_url,
        sha256: record.sha256.clone(),
        retrieved_at: record.retrieved_at.clone(),
        access_mode: record.access_mode,
        // Evidence records can contain diagnostic details intended only for the
        // native log. Do not project those details (including possible local
        // paths) across the IPC boundary.
        notes: Vec::new(),
    }
}

fn load_shared_chat_artifact(stored: &StoredWebEvidence) -> Result<(String, String), String> {
    if stored.record.state != WebEvidenceState::Ready {
        return Err("web evidence is not ready for shared-chat extraction".to_string());
    }
    let relative = stored
        .content_path
        .as_deref()
        .ok_or_else(|| "web evidence has no persisted content artifact".to_string())?;
    let path = checked_data_child_path(&evidence_dir()?.join(relative))?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let media_type = stored
        .record
        .content_type
        .as_deref()
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let kind = if media_type.contains("html") || extension == "html" {
        "html"
    } else if media_type.contains("markdown") || extension == "md" {
        "markdown"
    } else {
        return Err(
            "shared-chat extraction accepts only persisted HTML or Markdown evidence artifacts"
                .to_string(),
        );
    };
    let content = read_text_file(&path)?;
    if content.is_empty() || content.len() > MAX_SHARED_CHAT_ARTIFACT_BYTES {
        return Err(format!(
            "shared-chat artifact must contain 1..={} UTF-8 bytes",
            MAX_SHARED_CHAT_ARTIFACT_BYTES
        ));
    }
    Ok((kind.to_string(), content))
}

fn decode_html_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'&' {
            let character = value[index..].chars().next().unwrap_or_default();
            output.push(character);
            index += character.len_utf8();
            continue;
        }
        let Some(relative_end) = value[index..].find(';') else {
            output.push('&');
            index += 1;
            continue;
        };
        if relative_end > 14 {
            output.push('&');
            index += 1;
            continue;
        }
        let entity = &value[index + 1..index + relative_end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            value if value.starts_with("#x") || value.starts_with("#X") => {
                u32::from_str_radix(&value[2..], 16).ok().and_then(char::from_u32)
            }
            value if value.starts_with('#') => value[1..]
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32),
            _ => None,
        };
        if let Some(character) = decoded {
            output.push(character);
            index += relative_end + 1;
        } else {
            output.push('&');
            index += 1;
        }
    }
    output
}

fn normalize_visible_text(value: &str) -> String {
    let value = value.replace("\r\n", "\n").replace('\r', "\n");
    let lines = value
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>();
    let mut normalized = lines.join("\n");
    while normalized.contains("\n\n\n") {
        normalized = normalized.replace("\n\n\n", "\n\n");
    }
    redact_secrets(normalized.trim()).chars().take(MAX_SHARED_CHAT_TURN_BYTES).collect()
}

fn html_fragment_to_markdown(value: &str) -> String {
    let without_active = Regex::new(r"(?is)<(?:script|style|noscript)\b[^>]*>.*?</(?:script|style|noscript)\s*>")
        .map(|regex| regex.replace_all(value, " ").to_string())
        .unwrap_or_else(|_| value.to_string());
    let anchors = Regex::new(
        r#"(?is)<a\b[^>]*\bhref\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a\s*>"#,
    )
    .ok();
    let with_links = anchors
        .as_ref()
        .map(|regex| {
            regex
                .replace_all(&without_active, |captures: &regex::Captures<'_>| {
                    let label = Regex::new(r"(?is)<[^>]+>")
                        .map(|tags| tags.replace_all(&captures[2], " ").to_string())
                        .unwrap_or_else(|_| captures[2].to_string());
                    let label = normalize_visible_text(&decode_html_entities(&label));
                    let href = captures.get(1).map(|value| value.as_str()).unwrap_or_default();
                    let safe_href = validate_public_url(href)
                        .ok()
                        .filter(|url| url.scheme() == "https")
                        .map(|url| url.as_str().to_string());
                    match (label.is_empty(), safe_href) {
                        (false, Some(url)) => format!("[{label}]({url})"),
                        (false, None) => label,
                        (true, Some(url)) => url,
                        (true, None) => String::new(),
                    }
                })
                .to_string()
        })
        .unwrap_or(without_active);
    let with_breaks = Regex::new(
        r"(?is)<br\s*/?>|</(?:p|div|section|article|li|h[1-6]|blockquote|pre|tr)\s*>",
    )
    .map(|regex| regex.replace_all(&with_links, "\n").to_string())
    .unwrap_or(with_links);
    let with_lists = Regex::new(r"(?is)<li\b[^>]*>")
        .map(|regex| regex.replace_all(&with_breaks, "- ").to_string())
        .unwrap_or(with_breaks);
    let without_tags = Regex::new(r"(?is)<[^>]+>")
        .map(|regex| regex.replace_all(&with_lists, " ").to_string())
        .unwrap_or(with_lists);
    normalize_visible_text(&decode_html_entities(&without_tags))
}

fn json_title(object: &serde_json::Map<String, Value>) -> Option<String> {
    ["title", "conversation_title", "name"]
        .iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_str))
        .map(|title| sanitize_text(title.trim(), 300))
        .find(|title| !title.is_empty())
}

fn role_from_label(label: &str, provider: SharedChatProvider) -> Option<SharedChatRole> {
    let role = label
        .trim()
        .to_ascii_lowercase()
        .replace('-', " ")
        .replace('_', " ");
    match role.as_str() {
        "user" | "human" | "prompt" => Some(SharedChatRole::User),
        "assistant" => Some(SharedChatRole::Assistant),
        "model" if provider == SharedChatProvider::Gemini => Some(SharedChatRole::Assistant),
        "gemini" if provider == SharedChatProvider::Gemini => Some(SharedChatRole::Assistant),
        "chatgpt" if provider == SharedChatProvider::ChatGpt => Some(SharedChatRole::Assistant),
        "claude" if provider == SharedChatProvider::Claude => Some(SharedChatRole::Assistant),
        _ => None,
    }
}

fn role_from_json(
    object: &serde_json::Map<String, Value>,
    provider: SharedChatProvider,
) -> Option<SharedChatRole> {
    object
        .get("role")
        .and_then(Value::as_str)
        .or_else(|| object.get("speaker").and_then(Value::as_str))
        .or_else(|| {
            object
                .get("author")
                .and_then(Value::as_object)
                .and_then(|author| author.get("role"))
                .and_then(Value::as_str)
        })
        .and_then(|role| role_from_label(role, provider))
}

fn collect_content_strings(value: &Value, depth: usize, output: &mut Vec<String>) {
    if depth == 0 || output.len() >= 100 {
        return;
    }
    match value {
        Value::String(text) => output.push(text.clone()),
        Value::Array(items) => {
            for item in items.iter().take(100) {
                collect_content_strings(item, depth - 1, output);
            }
        }
        Value::Object(object) => {
            for key in ["text", "content", "parts"] {
                if let Some(item) = object.get(key) {
                    collect_content_strings(item, depth - 1, output);
                }
            }
        }
        _ => {}
    }
}

fn message_content_from_json(object: &serde_json::Map<String, Value>) -> Option<String> {
    let mut fragments = Vec::new();
    for key in ["content", "parts", "text"] {
        if let Some(value) = object.get(key) {
            collect_content_strings(value, 5, &mut fragments);
        }
    }
    let joined = fragments.join("\n\n");
    let content = if joined.contains('<') && joined.contains('>') {
        html_fragment_to_markdown(&joined)
    } else {
        normalize_visible_text(&joined)
    };
    (!content.is_empty()).then_some(content)
}

fn timestamp_hint_from_json(object: &serde_json::Map<String, Value>) -> Option<String> {
    for key in ["timestamp", "timestamp_hint", "created_at", "create_time", "updated_at", "time"] {
        let Some(value) = object.get(key) else {
            continue;
        };
        let rendered = match value {
            Value::String(text) => sanitize_text(text, 120),
            Value::Number(number) => sanitize_text(&number.to_string(), 120),
            _ => continue,
        };
        if !rendered.is_empty() {
            return Some(rendered);
        }
    }
    None
}

fn artifact_from_json(value: &Value) -> Option<SharedChatArtifact> {
    let object = value.as_object()?;
    if object.get("hidden").and_then(Value::as_bool) == Some(true)
        || object.get("visible").and_then(Value::as_bool) == Some(false)
    {
        return None;
    }
    let kind = ["kind", "type", "media_type"]
        .iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_str))
        .map(|value| sanitize_short(value, 60))
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| "artifact".to_string());
    let name = ["name", "title", "filename"]
        .iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_str))
        .map(|value| sanitize_text(value.trim(), 240))
        .find(|value| !value.is_empty());
    let content = ["text", "content", "summary"]
        .iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_str))
        .map(|value| normalize_visible_text(value))
        .find(|value| !value.is_empty());
    let url = ["url", "href", "download_url"]
        .iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_str))
        .filter_map(|value| validate_public_url(value).ok())
        .filter(|url| url.scheme() == "https")
        .map(|url| url.as_str().to_string())
        .next();
    if name.is_none() && content.is_none() && url.is_none() {
        return None;
    }
    Some(SharedChatArtifact {
        kind,
        name,
        content,
        url,
    })
}

fn visible_artifacts_from_json(object: &serde_json::Map<String, Value>) -> Vec<SharedChatArtifact> {
    let mut artifacts = Vec::new();
    for key in ["artifacts", "attachments", "files"] {
        let Some(value) = object.get(key) else {
            continue;
        };
        match value {
            Value::Array(items) => {
                artifacts.extend(items.iter().take(50).filter_map(artifact_from_json));
            }
            Value::Object(_) => {
                if let Some(artifact) = artifact_from_json(value) {
                    artifacts.push(artifact);
                }
            }
            _ => {}
        }
    }
    artifacts.sort_by(|left, right| {
        (&left.kind, &left.name, &left.url, &left.content)
            .cmp(&(&right.kind, &right.name, &right.url, &right.content))
    });
    artifacts.dedup();
    artifacts.truncate(50);
    artifacts
}

fn turn_from_json(value: &Value, provider: SharedChatProvider) -> Option<SharedChatTurn> {
    let outer = value.as_object()?;
    let message = outer
        .get("message")
        .and_then(Value::as_object)
        .unwrap_or(outer);
    let role = role_from_json(message, provider).or_else(|| role_from_json(outer, provider))?;
    let content_markdown = message_content_from_json(message).or_else(|| {
        if std::ptr::eq(message, outer) {
            None
        } else {
            message_content_from_json(outer)
        }
    })?;
    let mut artifacts = visible_artifacts_from_json(message);
    if !std::ptr::eq(message, outer) {
        artifacts.extend(visible_artifacts_from_json(outer));
        artifacts.sort_by(|left, right| {
            (&left.kind, &left.name, &left.url, &left.content)
                .cmp(&(&right.kind, &right.name, &right.url, &right.content))
        });
        artifacts.dedup();
    }
    Some(SharedChatTurn {
        ordinal: 0,
        role,
        content_markdown,
        artifacts,
        timestamp_hint: timestamp_hint_from_json(message)
            .or_else(|| timestamp_hint_from_json(outer)),
    })
}

fn candidate_from_message_array(
    messages: &[Value],
    provider: SharedChatProvider,
    title: Option<String>,
) -> Option<SharedChatCandidate> {
    let turns = messages
        .iter()
        .take(MAX_SHARED_CHAT_TURNS * 2)
        .filter_map(|message| turn_from_json(message, provider))
        .collect::<Vec<_>>();
    (!turns.is_empty()).then_some(SharedChatCandidate { title, turns })
}

fn chatgpt_mapping_candidate(
    object: &serde_json::Map<String, Value>,
) -> Result<Option<SharedChatCandidate>, SharedChatExtractionError> {
    let Some(mapping) = object.get("mapping").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(mut current) = object
        .get("current_node")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Ok(None);
    };
    let mut seen = BTreeSet::new();
    let mut turns = Vec::new();
    let mut reached_root = false;
    for _ in 0..MAX_SHARED_CHAT_TURNS * 2 {
        if !seen.insert(current.clone()) {
            return Err(SharedChatExtractionError::Ambiguous(
                "ChatGPT mapping contains a parent cycle".to_string(),
            ));
        }
        let Some(node) = mapping.get(&current).and_then(Value::as_object) else {
            return Err(SharedChatExtractionError::Ambiguous(
                "ChatGPT mapping current_node chain is incomplete".to_string(),
            ));
        };
        if let Some(turn) = turn_from_json(&Value::Object(node.clone()), SharedChatProvider::ChatGpt) {
            turns.push(turn);
        }
        let Some(parent) = node.get("parent").and_then(Value::as_str) else {
            reached_root = true;
            break;
        };
        current = parent.to_string();
    }
    if !reached_root {
        return Err(SharedChatExtractionError::Ambiguous(
            "ChatGPT mapping exceeded the bounded parent-chain limit".to_string(),
        ));
    }
    turns.reverse();
    Ok((!turns.is_empty()).then_some(SharedChatCandidate {
        title: json_title(object),
        turns,
    }))
}

fn collect_json_candidates(
    value: &Value,
    provider: SharedChatProvider,
    depth: usize,
    output: &mut Vec<SharedChatCandidate>,
) -> Result<(), SharedChatExtractionError> {
    if depth == 0 || output.len() >= 50 {
        return Ok(());
    }
    match value {
        Value::Object(object) => {
            if provider == SharedChatProvider::ChatGpt {
                if let Some(candidate) = chatgpt_mapping_candidate(object)? {
                    output.push(candidate);
                }
            }
            let title = json_title(object);
            for key in ["messages", "turns"] {
                if let Some(messages) = object.get(key).and_then(Value::as_array) {
                    if let Some(candidate) =
                        candidate_from_message_array(messages, provider, title.clone())
                    {
                        output.push(candidate);
                    }
                }
            }
            if let Some(messages) = object.get("conversation").and_then(Value::as_array) {
                if let Some(candidate) =
                    candidate_from_message_array(messages, provider, title.clone())
                {
                    output.push(candidate);
                }
            }
            for nested in object.values() {
                collect_json_candidates(nested, provider, depth - 1, output)?;
            }
        }
        Value::Array(items) => {
            for nested in items.iter().take(500) {
                collect_json_candidates(nested, provider, depth - 1, output)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn candidates_from_embedded_json(
    html: &str,
    provider: SharedChatProvider,
) -> Result<Vec<SharedChatCandidate>, SharedChatExtractionError> {
    let scripts = Regex::new(r#"(?is)<script\b([^>]*)>(.*?)</script\s*>"#)
        .map_err(|error| SharedChatExtractionError::Invalid(format!("invalid JSON script matcher: {error}")))?;
    let mut candidates = Vec::new();
    for captures in scripts.captures_iter(html).take(100) {
        let attributes = captures.get(1).map(|value| value.as_str()).unwrap_or_default();
        let attributes_lower = attributes.to_ascii_lowercase();
        let is_json = attributes_lower.contains("application/json")
            || attributes_lower.contains("__next_data__")
            || attributes_lower.contains("__nuxt_data__");
        if !is_json {
            continue;
        }
        let body = captures.get(2).map(|value| value.as_str()).unwrap_or_default().trim();
        if body.is_empty() || body.len() > MAX_SHARED_CHAT_ARTIFACT_BYTES {
            continue;
        }
        let parsed = serde_json::from_str::<Value>(body)
            .or_else(|_| serde_json::from_str::<Value>(&decode_html_entities(body)));
        if let Ok(value) = parsed {
            collect_json_candidates(&value, provider, 16, &mut candidates)?;
        }
    }
    Ok(candidates)
}

fn balanced_html_element<'a>(
    html: &'a str,
    opening_end: usize,
    tag: &str,
) -> Option<&'a str> {
    let token = Regex::new(&format!(r"(?is)</?{}\b[^>]*>", regex::escape(tag))).ok()?;
    let mut depth = 1usize;
    for item in token.find_iter(&html[opening_end..]) {
        let rendered = item.as_str().trim_start();
        if rendered.starts_with("</") {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(&html[opening_end..opening_end + item.start()]);
            }
        } else if !rendered.trim_end().ends_with("/>") {
            depth = depth.saturating_add(1);
        }
    }
    None
}

fn dom_marker_regex(provider: SharedChatProvider) -> Result<Regex, SharedChatExtractionError> {
    let pattern = match provider {
        SharedChatProvider::ChatGpt => {
            r#"(?is)<(article|section|div)\b[^>]*\bdata-message-author-role\s*=\s*["'](user|assistant)["'][^>]*>"#
        }
        SharedChatProvider::Gemini => {
            r#"(?is)<(article|section|div)\b[^>]*\bdata-message-role\s*=\s*["'](user|model)["'][^>]*>"#
        }
        SharedChatProvider::Claude => {
            r#"(?is)<(article|section|div)\b[^>]*\bdata-testid\s*=\s*["'](user-message|assistant-message)["'][^>]*>"#
        }
    };
    Regex::new(pattern)
        .map_err(|error| SharedChatExtractionError::Invalid(format!("invalid DOM marker matcher: {error}")))
}

fn candidate_from_dom(
    html: &str,
    provider: SharedChatProvider,
) -> Result<Option<SharedChatCandidate>, SharedChatExtractionError> {
    let marker = dom_marker_regex(provider)?;
    let mut turns = Vec::new();
    for captures in marker.captures_iter(html).take(MAX_SHARED_CHAT_TURNS * 2) {
        let Some(opening) = captures.get(0) else {
            continue;
        };
        let tag = captures.get(1).map(|value| value.as_str()).unwrap_or_default();
        let label = captures.get(2).map(|value| value.as_str()).unwrap_or_default();
        let normalized_label = label.strip_suffix("-message").unwrap_or(label);
        let Some(role) = role_from_label(normalized_label, provider) else {
            continue;
        };
        let Some(fragment) = balanced_html_element(html, opening.end(), tag) else {
            continue;
        };
        let content_markdown = html_fragment_to_markdown(fragment);
        if content_markdown.is_empty() {
            continue;
        }
        let artifacts = visible_artifacts_from_html(fragment);
        let timestamp_hint = timestamp_hint_from_html(fragment);
        turns.push(SharedChatTurn {
            ordinal: 0,
            role,
            content_markdown,
            artifacts,
            timestamp_hint,
        });
    }
    let title = Regex::new(r"(?is)<title\b[^>]*>(.*?)</title\s*>")
        .ok()
        .and_then(|regex| regex.captures(html))
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
        .map(|value| html_fragment_to_markdown(&value))
        .filter(|value| !value.is_empty());
    Ok((!turns.is_empty()).then_some(SharedChatCandidate { title, turns }))
}

fn visible_artifacts_from_html(fragment: &str) -> Vec<SharedChatArtifact> {
    let Ok(anchors) = Regex::new(
        r#"(?is)<a\b[^>]*\bhref\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a\s*>"#,
    ) else {
        return Vec::new();
    };
    let mut artifacts = anchors
        .captures_iter(fragment)
        .take(50)
        .filter_map(|captures| {
            let href = captures.get(1)?.as_str();
            let url = validate_public_url(href)
                .ok()
                .filter(|url| url.scheme() == "https")?
                .as_str()
                .to_string();
            let label = captures
                .get(2)
                .map(|value| html_fragment_to_markdown(value.as_str()))
                .filter(|value| !value.is_empty());
            Some(SharedChatArtifact {
                kind: "link".to_string(),
                name: label,
                content: None,
                url: Some(url),
            })
        })
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| (&left.name, &left.url).cmp(&(&right.name, &right.url)));
    artifacts.dedup();
    artifacts
}

fn timestamp_hint_from_html(fragment: &str) -> Option<String> {
    let datetime = Regex::new(
        r#"(?is)<time\b[^>]*\bdatetime\s*=\s*["']([^"']+)["'][^>]*>"#,
    )
    .ok()
    .and_then(|regex| regex.captures(fragment))
    .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()));
    let data_timestamp = Regex::new(
        r#"(?is)\bdata-(?:timestamp|created-at)\s*=\s*["']([^"']+)["']"#,
    )
    .ok()
    .and_then(|regex| regex.captures(fragment))
    .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()));
    datetime
        .or(data_timestamp)
        .map(|value| sanitize_text(&value, 120))
        .filter(|value| !value.is_empty())
}

fn candidate_from_markdown(
    markdown: &str,
    provider: SharedChatProvider,
) -> Result<Option<SharedChatCandidate>, SharedChatExtractionError> {
    let heading = Regex::new(
        r"(?i)^#{1,6}\s*(user|human|prompt|assistant|model|claude|chatgpt|gemini|usuário|usuario|assistente|pergunta|resposta)\s*:?[ \t]*$",
    )
    .map_err(|error| SharedChatExtractionError::Invalid(format!("invalid Markdown marker matcher: {error}")))?;
    let mut turns = Vec::new();
    let mut current_role = None;
    let mut current_lines = Vec::new();

    let flush = |role: Option<SharedChatRole>, lines: &mut Vec<&str>, turns: &mut Vec<SharedChatTurn>| {
        let Some(role) = role else {
            lines.clear();
            return;
        };
        let content_markdown = normalize_visible_text(&lines.join("\n"));
        lines.clear();
        if !content_markdown.is_empty() {
            turns.push(SharedChatTurn {
                ordinal: 0,
                role,
                content_markdown,
                artifacts: Vec::new(),
                timestamp_hint: None,
            });
        }
    };

    for line in markdown.lines() {
        if let Some(captures) = heading.captures(line.trim()) {
            flush(current_role, &mut current_lines, &mut turns);
            let label = captures.get(1).map(|value| value.as_str()).unwrap_or_default();
            let lowercase_label = label.to_lowercase();
            let translated = match lowercase_label.as_str() {
                "usuário" | "usuario" | "pergunta" => "user",
                "assistente" | "resposta" => "assistant",
                other => other,
            };
            current_role = role_from_label(translated, provider);
        } else if current_role.is_some() {
            current_lines.push(line);
        }
    }
    flush(current_role, &mut current_lines, &mut turns);
    Ok((!turns.is_empty()).then_some(SharedChatCandidate { title: None, turns }))
}

fn normalize_shared_chat_candidate(
    mut candidate: SharedChatCandidate,
) -> Result<SharedChatCandidate, SharedChatExtractionError> {
    if candidate.turns.len() > MAX_SHARED_CHAT_TURNS {
        return Err(SharedChatExtractionError::Invalid(format!(
            "shared conversation exceeds the {MAX_SHARED_CHAT_TURNS} turn limit"
        )));
    }
    candidate.title = candidate
        .title
        .map(|title| sanitize_text(&redact_secrets(&title), 300))
        .filter(|title| !title.is_empty());
    let mut normalized = Vec::new();
    let mut total_bytes = 0usize;
    for mut turn in candidate.turns {
        turn.content_markdown = normalize_visible_text(&turn.content_markdown);
        if turn.content_markdown.is_empty() {
            continue;
        }
        total_bytes = total_bytes
            .saturating_add(turn.content_markdown.len())
            .saturating_add(
                turn.artifacts
                    .iter()
                    .map(|artifact| {
                        artifact.kind.len()
                            + artifact.name.as_ref().map(String::len).unwrap_or(0)
                            + artifact.content.as_ref().map(String::len).unwrap_or(0)
                            + artifact.url.as_ref().map(String::len).unwrap_or(0)
                    })
                    .sum::<usize>(),
            );
        turn.ordinal = normalized.len() + 1;
        let duplicate = normalized.last().is_some_and(|previous: &SharedChatTurn| {
            previous.role == turn.role
                && previous.content_markdown == turn.content_markdown
                && previous.artifacts == turn.artifacts
        });
        if !duplicate {
            normalized.push(turn);
        }
    }
    if total_bytes > MAX_SHARED_CHAT_OUTPUT_BYTES {
        return Err(SharedChatExtractionError::Invalid(format!(
            "shared conversation exceeds the {MAX_SHARED_CHAT_OUTPUT_BYTES} byte output limit"
        )));
    }
    for (index, turn) in normalized.iter_mut().enumerate() {
        turn.ordinal = index + 1;
    }
    let has_user = normalized.iter().any(|turn| turn.role == SharedChatRole::User);
    let has_assistant = normalized
        .iter()
        .any(|turn| turn.role == SharedChatRole::Assistant);
    let has_exchange = normalized.windows(2).any(|pair| {
        pair[0].role == SharedChatRole::User && pair[1].role == SharedChatRole::Assistant
    });
    if normalized.len() < 2 || !has_user || !has_assistant || !has_exchange {
        return Err(SharedChatExtractionError::Insufficient(
            "no unequivocal user-to-assistant conversation was found in recognized provider markers"
                .to_string(),
        ));
    }
    candidate.turns = normalized;
    Ok(candidate)
}

fn select_shared_chat_candidate(
    candidates: Vec<SharedChatCandidate>,
) -> Result<SharedChatCandidate, SharedChatExtractionError> {
    let mut valid = Vec::new();
    for candidate in candidates {
        match normalize_shared_chat_candidate(candidate) {
            Ok(candidate) if !valid.iter().any(|existing: &SharedChatCandidate| existing.turns == candidate.turns) => {
                valid.push(candidate)
            }
            Ok(_) | Err(SharedChatExtractionError::Insufficient(_)) => {}
            Err(error) => return Err(error),
        }
    }
    if valid.is_empty() {
        return Err(SharedChatExtractionError::Insufficient(
            "the artifact contains no complete conversation in recognized turn markers".to_string(),
        ));
    }
    valid.sort_by(|left, right| right.turns.len().cmp(&left.turns.len()));
    let selected = valid.remove(0);
    if valid
        .iter()
        .any(|candidate| !selected.turns.starts_with(&candidate.turns))
    {
        return Err(SharedChatExtractionError::Ambiguous(
            "the artifact contains multiple distinct conversations; import a single-conversation export"
                .to_string(),
        ));
    }
    Ok(selected)
}

fn extract_shared_chat_candidate(
    kind: &str,
    artifact: &str,
    provider: SharedChatProvider,
) -> Result<SharedChatCandidate, SharedChatExtractionError> {
    if kind == "markdown" {
        return select_shared_chat_candidate(
            candidate_from_markdown(artifact, provider)?.into_iter().collect(),
        );
    }

    let json_candidates = candidates_from_embedded_json(artifact, provider)?;
    if !json_candidates.is_empty() {
        match select_shared_chat_candidate(json_candidates) {
            Ok(candidate) => return Ok(candidate),
            Err(SharedChatExtractionError::Ambiguous(message)) => {
                return Err(SharedChatExtractionError::Ambiguous(message))
            }
            Err(SharedChatExtractionError::Invalid(message)) => {
                return Err(SharedChatExtractionError::Invalid(message))
            }
            Err(SharedChatExtractionError::Insufficient(_)) => {}
        }
    }
    select_shared_chat_candidate(candidate_from_dom(artifact, provider)?.into_iter().collect())
}

fn markdown_plain_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(
            character,
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '<' | '>' | '(' | ')' | '#'
                | '+' | '-' | '.' | '!' | '|' | '~'
        ) {
            output.push('\\');
        }
        output.push(character);
    }
    output
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_shared_chat(
    title: Option<&str>,
    turns: &[SharedChatTurn],
) -> Result<(String, String), SharedChatExtractionError> {
    let mut markdown = String::new();
    let mut html = String::from("<article data-maestro-shared-chat=\"v1\">");
    if let Some(title) = title {
        markdown.push_str("# ");
        markdown.push_str(&markdown_plain_text(title));
        markdown.push_str("\n\n");
        html.push_str("<h2>");
        html.push_str(&escape_html(title));
        html.push_str("</h2>");
    }
    for turn in turns {
        let (label, role) = match turn.role {
            SharedChatRole::User => ("Prompt", "user"),
            SharedChatRole::Assistant => ("Resposta", "assistant"),
        };
        markdown.push_str(&format!("## {label} {}\n\n", turn.ordinal));
        markdown.push_str(&markdown_plain_text(&turn.content_markdown));
        markdown.push_str("\n\n");
        if let Some(timestamp) = turn.timestamp_hint.as_deref() {
            markdown.push_str("_Horário informado pela fonte: ");
            markdown.push_str(&markdown_plain_text(timestamp));
            markdown.push_str("_\n\n");
        }
        html.push_str(&format!(
            "<section data-role=\"{role}\"><h3>{label} {}</h3><p>",
            turn.ordinal
        ));
        html.push_str(&escape_html(&turn.content_markdown).replace('\n', "<br>"));
        html.push_str("</p>");
        if let Some(timestamp) = turn.timestamp_hint.as_deref() {
            html.push_str("<p><small>Horário informado pela fonte: ");
            html.push_str(&escape_html(timestamp));
            html.push_str("</small></p>");
        }
        if !turn.artifacts.is_empty() {
            markdown.push_str("### Artefatos visíveis\n\n");
            html.push_str("<h4>Artefatos visíveis</h4><ul>");
            for artifact in &turn.artifacts {
                let label = artifact.name.as_deref().unwrap_or(&artifact.kind);
                markdown.push_str("- ");
                markdown.push_str(&markdown_plain_text(label));
                if let Some(url) = artifact.url.as_deref() {
                    markdown.push_str(": <");
                    markdown.push_str(url);
                    markdown.push('>');
                }
                if let Some(content) = artifact.content.as_deref() {
                    markdown.push_str(" — ");
                    markdown.push_str(&markdown_plain_text(content));
                }
                markdown.push('\n');
                html.push_str("<li>");
                html.push_str(&escape_html(label));
                if let Some(url) = artifact.url.as_deref() {
                    html.push_str(" — <a rel=\"noopener noreferrer\" href=\"");
                    html.push_str(&escape_html(url));
                    html.push_str("\">abrir artefato</a>");
                }
                if let Some(content) = artifact.content.as_deref() {
                    html.push_str(" — ");
                    html.push_str(&escape_html(content));
                }
                html.push_str("</li>");
            }
            markdown.push('\n');
            html.push_str("</ul>");
        }
        html.push_str("</section>");
    }
    html.push_str("</article>");
    if markdown.len() > MAX_SHARED_CHAT_OUTPUT_BYTES || html.len() > MAX_SHARED_CHAT_OUTPUT_BYTES {
        return Err(SharedChatExtractionError::Invalid(
            "safe shared-chat rendering exceeded the output limit".to_string(),
        ));
    }
    Ok((markdown, html))
}

fn persist_shared_chat(
    classified: &ClassifiedSharedChatUrl,
    canonical_url: &str,
    stored: &StoredWebEvidence,
    candidate: SharedChatCandidate,
) -> Result<SharedChatImportResult, SharedChatExtractionError> {
    let title = candidate
        .title
        .clone()
        .or_else(|| {
            stored
                .record
                .title
                .as_deref()
                .map(|title| sanitize_text(&redact_secrets(title), 300))
        })
        .filter(|title| !title.is_empty());
    let (markdown, html) = render_shared_chat(title.as_deref(), &candidate.turns)?;
    let conversation_bytes = serde_json::to_vec(&candidate.turns).map_err(|error| {
        SharedChatExtractionError::Invalid(format!("failed to hash extracted conversation: {error}"))
    })?;
    let conversation_sha256 = sha256_bytes(&conversation_bytes);
    let markdown_sha256 = sha256_bytes(markdown.as_bytes());
    let html_sha256 = sha256_bytes(html.as_bytes());
    let provenance_id = evidence_id(&format!(
        "{}|{}|{}|{}|{}|{}",
        SHARED_CHAT_SCHEMA_VERSION,
        classified.provider.as_str(),
        canonical_url,
        stored.record.id,
        stored.record.sha256.as_deref().unwrap_or_default(),
        conversation_sha256
    ));
    let relative_dir = PathBuf::from("shared-chat-imports").join(&provenance_id);
    let directory = checked_data_child_path(&data_dir().join(&relative_dir))
        .map_err(SharedChatExtractionError::Invalid)?;
    fs::create_dir_all(&directory).map_err(|error| {
        SharedChatExtractionError::Invalid(format!("failed to create shared-chat data directory: {error}"))
    })?;
    let markdown_path = checked_data_child_path(&directory.join("conversation.md"))
        .map_err(SharedChatExtractionError::Invalid)?;
    let provenance_path = checked_data_child_path(&directory.join("provenance.json"))
        .map_err(SharedChatExtractionError::Invalid)?;
    let evidence = shared_chat_evidence_projection(&stored.record);
    let provenance = SharedChatProvenance {
        schema_version: SHARED_CHAT_SCHEMA_VERSION.to_string(),
        provenance_id: provenance_id.clone(),
        provider: classified.provider,
        source_url: classified.normalized_url.clone(),
        canonical_url: canonical_url.to_string(),
        evidence: evidence.clone(),
        title: title.clone(),
        turns: candidate.turns,
        conversation_sha256,
        markdown_sha256,
        html_sha256,
        trust_classification: "operator-visible public shared conversation; untrusted content"
            .to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    let encoded = serde_json::to_string_pretty(&provenance).map_err(|error| {
        SharedChatExtractionError::Invalid(format!("failed to serialize shared-chat provenance: {error}"))
    })?;
    let _guard = io_lock()
        .lock()
        .map_err(|_| SharedChatExtractionError::Invalid("web evidence I/O lock poisoned".to_string()))?;
    // Markdown is written first; provenance is the final commit marker. Both
    // helpers use temp-file + fsync + rename semantics.
    write_text_file(&markdown_path, &markdown).map_err(SharedChatExtractionError::Invalid)?;
    write_text_file(&provenance_path, &encoded).map_err(SharedChatExtractionError::Invalid)?;
    if read_text_file(&markdown_path).map_err(SharedChatExtractionError::Invalid)? != markdown
        || read_text_file(&provenance_path).map_err(SharedChatExtractionError::Invalid)? != encoded
    {
        return Err(SharedChatExtractionError::Invalid(
            "shared-chat provenance readback did not match the atomic writes".to_string(),
        ));
    }
    let relative = relative_dir.to_string_lossy().replace('\\', "/");
    Ok(SharedChatImportResult::Ready {
        title,
        html,
        provider: classified.provider,
        evidence,
        provenance_id,
        markdown_path: format!("{relative}/conversation.md"),
        provenance_path: format!("{relative}/provenance.json"),
    })
}

fn process_shared_chat_evidence(
    classified: &ClassifiedSharedChatUrl,
    evidence_id_value: &str,
) -> Result<SharedChatImportResult, SharedChatExtractionError> {
    let stored = load_stored(evidence_id_value).map_err(SharedChatExtractionError::Invalid)?;
    let canonical_url = canonical_url_from_evidence(classified, &stored.record)?;
    let (kind, artifact) =
        load_shared_chat_artifact(&stored).map_err(SharedChatExtractionError::Insufficient)?;
    let candidate = extract_shared_chat_candidate(&kind, &artifact, classified.provider)?;
    persist_shared_chat(classified, &canonical_url, &stored, candidate)
}

fn shared_chat_action_result(
    provider: SharedChatProvider,
    record: &WebEvidenceRecord,
    reason: &str,
) -> SharedChatImportResult {
    let next_step = match record.access_mode {
        WebEvidenceAccessMode::RenderedFetch => "Na janela isolada, confirme que a conversa pública está visível; exporte somente essa página como HTML ou Markdown, importe-a em Web Evidence com a URL canônica exibida e invoque import_shared_chat novamente informando o evidence_id retornado.",
        _ => "No navegador aberto pelo operador, confirme que a conversa pública está visível; exporte somente essa página como HTML ou Markdown, importe-a em Web Evidence com a URL canônica exibida e invoque import_shared_chat novamente informando o evidence_id retornado.",
    };
    SharedChatImportResult::OperatorActionRequired {
        provider,
        evidence: shared_chat_evidence_projection(record),
        action: SharedChatActionRequired {
            kind: "rendered_or_operator_capture".to_string(),
            reason: sanitize_text(reason, 800),
            next_step: next_step.to_string(),
        },
    }
}

async fn begin_shared_chat_handoff(
    app: tauri::AppHandle,
    classified: &ClassifiedSharedChatUrl,
    reason: &str,
) -> Result<SharedChatImportResult, String> {
    let request = WebEvidenceUrlRequest {
        url: classified.normalized_url.clone(),
    };
    let record = match start_rendered_web_evidence(app.clone(), request).await {
        Ok(record) => record,
        Err(_rendered_error) => {
            let fallback = open_web_evidence_in_default_browser(
                app,
                WebEvidenceUrlRequest {
                    url: classified.normalized_url.clone(),
                },
            )
            .await?;
            let reason = format!(
                "{reason} The isolated renderer was unavailable; a system-browser handoff was started."
            );
            return Ok(shared_chat_action_result(
                classified.provider,
                &fallback,
                &reason,
            ));
        }
    };
    Ok(shared_chat_action_result(
        classified.provider,
        &record,
        reason,
    ))
}

/// Imports a public provider share into editor-safe HTML while preserving a
/// local, hash-addressed provenance record. Static pages that do not expose an
/// unequivocal conversation never become editor content.
#[tauri::command]
pub(crate) async fn import_shared_chat(
    app: tauri::AppHandle,
    request: SharedChatImportRequest,
) -> Result<SharedChatImportResult, String> {
    let classified = classify_shared_chat_url(&request.url)?;
    if let Some(evidence_id_value) = request.evidence_id.as_deref() {
        let stored = load_stored(evidence_id_value)?;
        if stored.record.state != WebEvidenceState::Ready {
            return Ok(shared_chat_action_result(
                classified.provider,
                &stored.record,
                "The selected Web Evidence record does not contain a ready HTML or Markdown artifact.",
            ));
        }
        return match process_shared_chat_evidence(&classified, evidence_id_value) {
            Ok(result) => Ok(result),
            Err(SharedChatExtractionError::Insufficient(message))
                if stored.record.access_mode != WebEvidenceAccessMode::OperatorAssistedBrowserCapture =>
            {
                begin_shared_chat_handoff(app, &classified, &message).await
            }
            Err(error) => Err(error.message().to_string()),
        };
    }

    let fetch_url = classified.normalized_url.clone();
    let force_revalidate = request.force_revalidate;
    let fetch_app = app.clone();
    let record = tauri::async_runtime::spawn_blocking(move || {
        fetch_web_evidence_inner(
            Some(&fetch_app),
            WebEvidenceFetchRequest {
                url: fetch_url,
                method: WebEvidenceMethod::Get,
                force_revalidate,
            },
        )
    })
    .await
    .map_err(|error| format!("shared-chat fetch worker failed: {error}"))??;

    if record.state == WebEvidenceState::Ready {
        match process_shared_chat_evidence(&classified, &record.id) {
            Ok(result) => return Ok(result),
            Err(SharedChatExtractionError::Insufficient(message)) => {
                return begin_shared_chat_handoff(app, &classified, &message).await
            }
            Err(error) => return Err(error.message().to_string()),
        }
    }
    let reason = if interaction_requires_operator(record.interaction_state) {
        "The provider returned a CAPTCHA, login, consent, or other interaction boundary; navigation text was not imported."
    } else {
        "Static Web Evidence did not produce a ready provider conversation artifact."
    };
    begin_shared_chat_handoff(app, &classified, reason).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_chat_url_gate_accepts_only_exact_public_share_surfaces() {
        let chatgpt = classify_shared_chat_url("https://chatgpt.com/share/abcdefgh?utm_source=test")
            .expect("ChatGPT share should classify");
        assert_eq!(chatgpt.provider, SharedChatProvider::ChatGpt);
        assert_eq!(chatgpt.normalized_url, "https://chatgpt.com/share/abcdefgh");

        let gemini = classify_shared_chat_url("https://g.co/gemini/share/abcdefgh")
            .expect("Gemini short share should classify");
        assert!(gemini.requires_gemini_redirect_validation);
        assert_eq!(gemini.provider, SharedChatProvider::Gemini);

        assert!(classify_shared_chat_url("http://chatgpt.com/share/abcdefgh").is_err());
        assert!(classify_shared_chat_url("https://chatgpt.example/share/abcdefgh").is_err());
        assert!(classify_shared_chat_url("https://claude.ai.evil.example/share/abcdefgh").is_err());
        assert!(classify_shared_chat_url("https://gemini.google.com/app/abcdefgh").is_err());
    }

    #[test]
    fn shared_chat_extracts_chatgpt_mapping_and_rejects_navigation_text() {
        let fixture = r#"<script id="__NEXT_DATA__" type="application/json">{
          "title":"Conversa de teste",
          "current_node":"assistant-1",
          "mapping":{
            "user-1":{"parent":null,"message":{"author":{"role":"user"},"content":{"parts":["Qual é o resultado?"]}}},
            "assistant-1":{"parent":"user-1","message":{"author":{"role":"assistant"},"content":{"parts":["O resultado é 42."]}}}
          }
        }</script>"#;
        let candidate = extract_shared_chat_candidate("html", fixture, SharedChatProvider::ChatGpt)
            .expect("recognized ChatGPT mapping should extract");
        assert_eq!(candidate.title.as_deref(), Some("Conversa de teste"));
        assert_eq!(candidate.turns.len(), 2);
        assert_eq!(candidate.turns[0].role, SharedChatRole::User);
        assert_eq!(candidate.turns[1].role, SharedChatRole::Assistant);

        let navigation = "<html><nav>User Assistant Login Consent</nav><main>Choose a chat</main></html>";
        assert!(matches!(
            extract_shared_chat_candidate("html", navigation, SharedChatProvider::ChatGpt),
            Err(SharedChatExtractionError::Insufficient(_))
        ));
    }

    #[test]
    fn shared_chat_extracts_gemini_json_and_claude_dom_markers() {
        let gemini = r#"<script type="application/json">{"turns":[
          {"role":"user","text":"Pergunta Gemini"},
          {"role":"model","text":"Resposta Gemini"}
        ]}</script>"#;
        let candidate = extract_shared_chat_candidate("html", gemini, SharedChatProvider::Gemini)
            .expect("recognized Gemini turn JSON should extract");
        assert_eq!(candidate.turns.len(), 2);

        let claude = r#"<main>
          <div data-testid="user-message"><p>Pergunta Claude</p></div>
          <div data-testid="assistant-message"><div><p>Resposta Claude</p></div></div>
        </main>"#;
        let candidate = extract_shared_chat_candidate("html", claude, SharedChatProvider::Claude)
            .expect("recognized Claude DOM markers should extract");
        assert_eq!(candidate.turns.len(), 2);
        assert_eq!(candidate.turns[1].content_markdown, "Resposta Claude");
    }

    #[test]
    fn shared_chat_markdown_requires_explicit_turn_headings() {
        let fixture = "# Export\n\n## Prompt\n\nPergunta\n\n## Resposta\n\nResposta\n";
        let candidate = extract_shared_chat_candidate(
            "markdown",
            fixture,
            SharedChatProvider::Claude,
        )
        .expect("explicit Markdown turn headings should extract");
        assert_eq!(candidate.turns.len(), 2);

        assert!(matches!(
            extract_shared_chat_candidate(
                "markdown",
                "Esta página menciona user e assistant na navegação.",
                SharedChatProvider::Claude,
            ),
            Err(SharedChatExtractionError::Insufficient(_))
        ));
    }

    #[test]
    fn shared_chat_safe_html_never_reuses_active_source_markup() {
        let candidate = normalize_shared_chat_candidate(SharedChatCandidate {
            title: Some("<img src=x onerror=alert(1)>".to_string()),
            turns: vec![
                SharedChatTurn {
                    ordinal: 0,
                    role: SharedChatRole::User,
                    content_markdown: "<script>alert(1)</script>".to_string(),
                    artifacts: Vec::new(),
                    timestamp_hint: None,
                },
                SharedChatTurn {
                    ordinal: 0,
                    role: SharedChatRole::Assistant,
                    content_markdown: "Resposta segura".to_string(),
                    artifacts: Vec::new(),
                    timestamp_hint: None,
                },
            ],
        })
        .expect("fixture should normalize");
        let (_, html) = render_shared_chat(candidate.title.as_deref(), &candidate.turns)
            .expect("safe render should succeed");
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }

    #[test]
    fn public_url_gate_blocks_private_metadata_and_credentials() {
        assert!(validate_public_url("http://127.0.0.1/admin").is_err());
        assert!(validate_public_url("http://169.254.169.254/latest/meta-data").is_err());
        assert!(validate_public_url("https://user:password@example.com/").is_err());
        assert!(validate_public_url("https://example.com/?access_token=secret").is_err());
    }

    #[test]
    fn curl_replay_is_direct_and_does_not_follow_redirects() {
        let command = direct_curl_command(WebEvidenceMethod::Get, "https://example.com/source");
        assert!(command.contains("--noproxy '*'"));
        assert!(command.contains("--max-redirs 0"));
        assert!(!command.contains("--location"));
    }

    #[test]
    fn robots_parser_uses_longest_matching_rule() {
        let robots = "User-agent: *\nDisallow: /private\nAllow: /private/public\n";
        assert!(robots_disallows_path(robots, "/private/file"));
        assert!(!robots_disallows_path(robots, "/private/public/file"));
        assert!(!robots_disallows_path(robots, "/open"));
    }

    #[test]
    fn import_magic_rejects_mime_mismatch() {
        assert!(validate_import_magic("application/pdf", b"not-a-pdf").is_err());
        assert!(validate_import_magic("application/pdf", b"%PDF-1.7\n").is_ok());
        assert!(validate_import_magic("image/png", b"\x89PNG\r\n\x1a\nrest").is_ok());
    }

    #[test]
    fn connector_requires_environment_backed_credentials() {
        let connector = SearchConnector {
            id: "example".to_string(),
            label: "Example".to_string(),
            endpoint: "https://example.com/search".to_string(),
            query_parameter: "q".to_string(),
            limit_parameter: "limit".to_string(),
            results_path: "results".to_string(),
            title_field: "title".to_string(),
            url_field: "url".to_string(),
            snippet_field: None,
            api_key_env_var: Some("EXAMPLE_API_KEY".to_string()),
            api_key_header: None,
        };
        assert!(validate_search_connector(&connector).is_err());
    }

    #[test]
    fn evidence_ids_are_path_safe_sha256_values() {
        let id = evidence_id("stable source key");
        assert_eq!(id.len(), 64);
        assert!(is_valid_evidence_id(&id));
        assert!(!is_valid_evidence_id("../escape"));
    }
}
