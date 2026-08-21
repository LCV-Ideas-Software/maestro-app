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
use crate::sanitize::{sanitize_short, sanitize_text};

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
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn sha256_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
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
        let current = validate_public_url(current.as_str())?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
