//! Guarded MainSite publication through the Cloudflare D1 HTTP API.
//!
//! The module deliberately keeps preview and publication separate. Preview is
//! read-only and binds the validated local draft, the observed remote row and
//! the current MainSite content-version into a confirmation token. Publication
//! repeats every read and rejects drift before sending one atomic, parameterized
//! D1 batch. An indeterminate network failure is never retried automatically.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use reqwest::blocking::Client;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::cloudflare::{
    cloudflare_client, cloudflare_get_paginated_results, cloudflare_post_json,
    cloudflare_result_id_for_name, cloudflare_token_from_provider_request,
};
use crate::mainsite_draft::{
    validate_stored_draft, MainSiteDraft, MAINSITE_SANITIZER_PROFILE,
};
use crate::CloudflareProviderStorageRequest;

const PROBE_SCHEMA_VERSION: &str = "mainsite_d1_probe.v1";
const PLAN_SCHEMA_VERSION: &str = "mainsite_d1_plan.v1";
const PUBLISH_SCHEMA_VERSION: &str = "mainsite_d1_publish.v1";
const CONTENT_VERSION_KEY: &str = "mainsite/content-version";
const SETTINGS_TABLE: &str = "mainsite_settings";
const MAX_IDENTIFIER_LENGTH: usize = 63;
const MAX_ACCOUNT_ID_LENGTH: usize = 32;

const POST_COLUMNS: [&str; 9] = [
    "id",
    "title",
    "content",
    "author",
    "is_pinned",
    "display_order",
    "is_published",
    "created_at",
    "updated_at",
];

const SETTINGS_COLUMNS: [&str; 3] = ["id", "payload", "updated_at"];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MainSiteD1Target {
    pub(crate) account_id: String,
    #[serde(default)]
    pub(crate) api_token: Option<String>,
    #[serde(default = "default_cloudflare_token_env")]
    pub(crate) api_token_env_var: String,
    pub(crate) database: String,
    pub(crate) table: String,
    #[serde(default)]
    pub(crate) allow_wrangler_fallback: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MainSiteD1ProbeRequest {
    pub(crate) target: MainSiteD1Target,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MainSiteD1ProbeResult {
    pub(crate) schema_version: String,
    pub(crate) transport: String,
    pub(crate) database: String,
    pub(crate) table: String,
    pub(crate) readable: bool,
    pub(crate) required_columns: Vec<String>,
    pub(crate) content_version_ready: bool,
    pub(crate) checked_at: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MainSiteD1PreviewRequest {
    pub(crate) target: MainSiteD1Target,
    pub(crate) draft: MainSiteDraft,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MainSiteD1PublishAction {
    Insert,
    Update,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MainSiteD1DiffChange {
    New,
    Changed,
    Unchanged,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct MainSiteD1DiffItem {
    pub(crate) field: String,
    pub(crate) change: MainSiteD1DiffChange,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct MainSiteD1PublishPlan {
    pub(crate) schema_version: String,
    pub(crate) plan_id: String,
    pub(crate) action: MainSiteD1PublishAction,
    pub(crate) database: String,
    pub(crate) table: String,
    pub(crate) requested_post_id: Option<u64>,
    pub(crate) sql_intent: String,
    pub(crate) diff_summary: Vec<MainSiteD1DiffItem>,
    pub(crate) draft_hash: String,
    pub(crate) remote_hash: Option<String>,
    pub(crate) content_version_hash: Option<String>,
    pub(crate) content_version_current: u64,
    pub(crate) content_version_next: u64,
    pub(crate) confirmation_token: String,
    pub(crate) checked_at: String,
    pub(crate) read_only: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MainSiteD1PublishRequest {
    pub(crate) target: MainSiteD1Target,
    pub(crate) draft: MainSiteDraft,
    pub(crate) preview: MainSiteD1PublishPlan,
    pub(crate) confirmed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MainSiteD1PublishResult {
    pub(crate) schema_version: String,
    pub(crate) plan_id: String,
    pub(crate) action: MainSiteD1PublishAction,
    pub(crate) post_id: u64,
    pub(crate) draft_hash: String,
    pub(crate) readback_hash: String,
    pub(crate) content_version: u64,
    pub(crate) fields_verified: Vec<String>,
    pub(crate) verified: bool,
    pub(crate) transport: String,
    pub(crate) published_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ValidatedTarget {
    account_id: String,
    database: String,
    table: String,
    allow_wrangler_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RemotePost {
    id: u64,
    title: String,
    content: String,
    author: String,
    is_pinned: i64,
    display_order: i64,
    is_published: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContentVersionRecord {
    version: u64,
    payload: Option<String>,
    updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentVersionPayload {
    version: u64,
    updated_at: String,
}

#[derive(Clone, Debug, Default)]
struct D1Meta {
    changes: Option<u64>,
    rows_written: Option<u64>,
    last_row_id: Option<u64>,
}

#[derive(Clone, Debug, Default)]
struct D1Result {
    rows: Vec<Value>,
    meta: D1Meta,
}

#[derive(Clone, Debug)]
struct ParsedTag {
    name: String,
    closing: bool,
    self_closing: bool,
    attributes: Vec<(String, Option<String>)>,
}

fn default_cloudflare_token_env() -> String {
    "MAESTRO_CLOUDFLARE_API_TOKEN".to_string()
}

fn sha256_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn hash_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.len().to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{hasher:x}")
}

fn draft_hash(draft: &MainSiteDraft) -> Result<String, String> {
    serde_json::to_vec(draft)
        .map(|encoded| sha256_bytes(&encoded))
        .map_err(|_| "DRAFT_HASH_FAILED: unable to bind the local draft".to_string())
}

fn remote_post_hash(post: &RemotePost) -> String {
    hash_parts(&[
        &post.id.to_string(),
        &post.title,
        &post.content,
        &post.author,
        &post.is_pinned.to_string(),
        &post.display_order.to_string(),
        &post.is_published.to_string(),
        &post.created_at,
        &post.updated_at,
    ])
}

fn content_version_hash(record: &ContentVersionRecord) -> Option<String> {
    record.payload.as_deref().map(|payload| {
        hash_parts(&[
            payload,
            record.updated_at.as_deref().unwrap_or_default(),
        ])
    })
}

pub(crate) fn validate_database_name(value: &str) -> Result<String, String> {
    validate_simple_identifier(value, true, "Cloudflare publication database")
}

pub(crate) fn validate_table_identifier(value: &str) -> Result<String, String> {
    validate_simple_identifier(value, false, "Cloudflare publication table")
}

fn validate_simple_identifier(
    value: &str,
    allow_hyphen: bool,
    label: &str,
) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_IDENTIFIER_LENGTH {
        return Err(format!(
            "{label} must contain 1 to {MAX_IDENTIFIER_LENGTH} characters"
        ));
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return Err(format!("{label} is required"));
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(format!(
            "{label} must start with an ASCII letter or underscore"
        ));
    }
    if !chars.all(|character| {
        character.is_ascii_alphanumeric()
            || character == '_'
            || (allow_hyphen && character == '-')
    }) {
        return Err(format!("{label} contains an unsupported character"));
    }
    Ok(trimmed.to_string())
}

fn validate_account_id(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_ACCOUNT_ID_LENGTH {
        return Err("Cloudflare account_id is missing or invalid".to_string());
    }
    if !trimmed
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
    {
        return Err("Cloudflare account_id contains an unsupported character".to_string());
    }
    Ok(trimmed.to_string())
}

fn validate_database_id(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > 100
        || !trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return Err("CLOUDFLARE_DATABASE_ID_INVALID".to_string());
    }
    Ok(trimmed.to_string())
}

fn validate_target(target: &MainSiteD1Target) -> Result<ValidatedTarget, String> {
    Ok(ValidatedTarget {
        account_id: validate_account_id(&target.account_id)?,
        database: validate_database_name(&target.database)?,
        table: validate_table_identifier(&target.table)?,
        allow_wrangler_fallback: target.allow_wrangler_fallback,
    })
}

fn token_for_target(target: &MainSiteD1Target) -> Result<String, String> {
    let env_var = target.api_token_env_var.trim();
    if !env_var.is_empty()
        && !matches!(
            env_var,
            "MAESTRO_CLOUDFLARE_API_TOKEN" | "CLOUDFLARE_API_TOKEN" | "CF_API_TOKEN"
        )
    {
        return Err("CLOUDFLARE_TOKEN_ENV_NOT_ALLOWED".to_string());
    }
    if target.api_token.as_deref().is_some_and(|token| {
        let trimmed = token.trim();
        trimmed.is_empty()
            || trimmed.len() > 4_096
            || token
                .chars()
                .any(|character| character.is_ascii_whitespace() || character.is_control())
    }) {
        return Err("CLOUDFLARE_TOKEN_INVALID".to_string());
    }
    let request = CloudflareProviderStorageRequest {
        account_id: target.account_id.clone(),
        api_token: target.api_token.clone(),
        api_token_env_var: env_var.to_string(),
        persistence_database: target.database.clone(),
        secret_store: String::new(),
    };
    cloudflare_token_from_provider_request(&request)
        .map_err(|_| "CLOUDFLARE_TOKEN_UNAVAILABLE".to_string())
}

fn read_error(
    target: &ValidatedTarget,
    operation: &'static str,
    raw_error: &str,
) -> String {
    let deterministic = raw_error.contains("HTTP 400")
        || raw_error.contains("HTTP 401")
        || raw_error.contains("HTTP 403")
        || raw_error.contains("HTTP 404");
    if target.allow_wrangler_fallback {
        return format!(
            "WRANGLER_FALLBACK_UNAVAILABLE: {operation} failed through the Cloudflare API and no audited stdin/temp-file transport is implemented"
        );
    }
    if deterministic {
        format!("CLOUDFLARE_API_READ_REJECTED: {operation}")
    } else {
        format!("CLOUDFLARE_API_READ_FAILED: {operation}")
    }
}

fn publish_error(raw_error: &str) -> String {
    if raw_error.contains("HTTP 400")
        || raw_error.contains("HTTP 401")
        || raw_error.contains("HTTP 403")
        || raw_error.contains("HTTP 404")
    {
        "CLOUDFLARE_PUBLISH_REJECTED_NO_RETRY".to_string()
    } else {
        "PUBLISH_OUTCOME_UNKNOWN_NO_RETRY: inspect the remote row and content-version before creating a new preview"
            .to_string()
    }
}

fn resolve_database_id(
    client: &Client,
    token: &str,
    target: &ValidatedTarget,
) -> Result<String, String> {
    let path = format!("/accounts/{}/d1/database", target.account_id);
    let listed = cloudflare_get_paginated_results(client, token, &path)
        .map_err(|error| read_error(target, "resolve_database", &error))?;
    let id = cloudflare_result_id_for_name(&listed, &target.database)
        .ok_or_else(|| "CLOUDFLARE_PUBLICATION_DATABASE_NOT_FOUND".to_string())?;
    validate_database_id(&id)
}

fn d1_query_path(target: &ValidatedTarget, database_id: &str) -> String {
    format!(
        "/accounts/{}/d1/database/{database_id}/query",
        target.account_id
    )
}

fn value_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
            .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
    })
}

fn d1_param(value: impl ToString) -> Value {
    Value::String(value.to_string())
}

fn parse_d1_results(value: &Value, expected_items: usize) -> Result<Vec<D1Result>, String> {
    if value.get("success").and_then(Value::as_bool) != Some(true) {
        return Err("D1_RESPONSE_TOP_LEVEL_FAILED".to_string());
    }
    let items = value
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| "D1_RESPONSE_RESULT_ARRAY_MISSING".to_string())?;
    if items.len() != expected_items {
        return Err("D1_RESPONSE_RESULT_COUNT_MISMATCH".to_string());
    }

    items
        .iter()
        .map(|item| {
            if item.get("success").and_then(Value::as_bool) != Some(true) {
                return Err("D1_RESPONSE_ITEM_FAILED".to_string());
            }
            let meta = item
                .get("meta")
                .and_then(Value::as_object)
                .ok_or_else(|| "D1_RESPONSE_ITEM_META_MISSING".to_string())?;
            let rows = item
                .get("results")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Ok(D1Result {
                rows,
                meta: D1Meta {
                    changes: value_u64(meta.get("changes")),
                    rows_written: value_u64(meta.get("rows_written")),
                    last_row_id: value_u64(meta.get("last_row_id")),
                },
            })
        })
        .collect()
}

fn execute_read_query(
    client: &Client,
    token: &str,
    target: &ValidatedTarget,
    database_id: &str,
    operation: &'static str,
    sql: &str,
    params: Vec<Value>,
) -> Result<D1Result, String> {
    let response = cloudflare_post_json(
        client,
        token,
        &d1_query_path(target, database_id),
        json!({ "sql": sql, "params": params }),
    )
    .map_err(|error| read_error(target, operation, &error))?;
    parse_d1_results(&response, 1)?
        .into_iter()
        .next()
        .ok_or_else(|| "D1_RESPONSE_RESULT_ARRAY_MISSING".to_string())
}

fn row_object(row: &Value) -> Result<&Map<String, Value>, String> {
    row.as_object()
        .ok_or_else(|| "D1_RESPONSE_ROW_INVALID".to_string())
}

fn parse_required_string(row: &Map<String, Value>, key: &str) -> Result<String, String> {
    row.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| format!("D1_RESPONSE_REQUIRED_FIELD_INVALID: {key}"))
}

fn parse_nullable_string(row: &Map<String, Value>, key: &str) -> Result<String, String> {
    match row.get(key) {
        Some(Value::Null) | None => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.clone()),
        _ => Err(format!("D1_RESPONSE_FIELD_INVALID: {key}")),
    }
}

fn parse_i64_field(row: &Map<String, Value>, key: &str) -> Result<i64, String> {
    row.get(key)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
        })
        .ok_or_else(|| format!("D1_RESPONSE_FIELD_INVALID: {key}"))
}

fn parse_remote_post(row: &Value) -> Result<RemotePost, String> {
    let row = row_object(row)?;
    let id = value_u64(row.get("id"))
        .filter(|id| *id > 0)
        .ok_or_else(|| "D1_RESPONSE_POST_ID_INVALID".to_string())?;
    let post = RemotePost {
        id,
        title: parse_required_string(row, "title")?,
        content: parse_required_string(row, "content")?,
        author: parse_nullable_string(row, "author")?,
        is_pinned: parse_i64_field(row, "is_pinned")?,
        display_order: parse_i64_field(row, "display_order")?,
        is_published: parse_i64_field(row, "is_published")?,
        created_at: parse_required_string(row, "created_at")?,
        updated_at: parse_nullable_string(row, "updated_at")?,
    };
    if !matches!(post.is_pinned, 0 | 1) || !matches!(post.is_published, 0 | 1) {
        return Err("D1_RESPONSE_POST_BOOLEAN_INVALID".to_string());
    }
    Ok(post)
}

fn validate_columns(
    rows: &[Value],
    expected: &[&str],
    schema_label: &'static str,
) -> Result<(), String> {
    let names = rows
        .iter()
        .filter_map(|row| row.get("name").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let missing = expected
        .iter()
        .copied()
        .filter(|column| !names.contains(column))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "D1_SCHEMA_MISMATCH: {schema_label} is missing {}",
            missing.join(",")
        ))
    }
}

fn schema_column<'a>(rows: &'a [Value], name: &str) -> Result<&'a Map<String, Value>, String> {
    rows.iter()
        .filter_map(Value::as_object)
        .find(|row| row.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| "D1_SCHEMA_COLUMN_MISSING".to_string())
}

fn validate_schema_column(
    rows: &[Value],
    name: &str,
    expected_type: &str,
    require_not_null: bool,
    require_primary_key: bool,
) -> Result<(), String> {
    let column = schema_column(rows, name)?;
    let column_type = column
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let not_null = value_u64(column.get("notnull")).unwrap_or_default() > 0;
    let primary_key = value_u64(column.get("pk")).unwrap_or_default() > 0;
    if !column_type.eq_ignore_ascii_case(expected_type)
        || (require_not_null && !not_null)
        || (require_primary_key && !primary_key)
    {
        return Err(format!("D1_SCHEMA_COLUMN_CONTRACT_MISMATCH: {name}"));
    }
    Ok(())
}

fn validate_remote_schema(
    client: &Client,
    token: &str,
    target: &ValidatedTarget,
    database_id: &str,
) -> Result<(), String> {
    let post_schema = execute_read_query(
        client,
        token,
        target,
        database_id,
        "validate_post_schema",
        &format!("PRAGMA table_info({})", target.table),
        Vec::new(),
    )?;
    validate_columns(&post_schema.rows, &POST_COLUMNS, "publication table")?;
    validate_schema_column(&post_schema.rows, "id", "INTEGER", false, true)?;
    validate_schema_column(&post_schema.rows, "title", "TEXT", true, false)?;
    validate_schema_column(&post_schema.rows, "content", "TEXT", true, false)?;
    validate_schema_column(&post_schema.rows, "author", "TEXT", false, false)?;
    validate_schema_column(&post_schema.rows, "is_pinned", "INTEGER", true, false)?;
    validate_schema_column(
        &post_schema.rows,
        "display_order",
        "INTEGER",
        true,
        false,
    )?;
    validate_schema_column(
        &post_schema.rows,
        "is_published",
        "INTEGER",
        true,
        false,
    )?;

    let settings_schema = execute_read_query(
        client,
        token,
        target,
        database_id,
        "validate_content_version_schema",
        &format!("PRAGMA table_info({SETTINGS_TABLE})"),
        Vec::new(),
    )?;
    validate_columns(
        &settings_schema.rows,
        &SETTINGS_COLUMNS,
        "mainsite_settings",
    )?;
    validate_schema_column(&settings_schema.rows, "id", "TEXT", false, true)?;
    // Publication uses this NOT NULL contract as the rollback guard: a stale
    // optimistic state deliberately selects NULL and makes the whole D1 batch
    // fail atomically instead of committing a partial post mutation.
    validate_schema_column(&settings_schema.rows, "payload", "TEXT", true, false)
}

fn read_post(
    client: &Client,
    token: &str,
    target: &ValidatedTarget,
    database_id: &str,
    id: Option<u64>,
) -> Result<Option<RemotePost>, String> {
    let Some(id) = id else {
        return Ok(None);
    };
    let result = execute_read_query(
        client,
        token,
        target,
        database_id,
        "read_post",
        &format!(
            "SELECT id,title,content,author,is_pinned,display_order,is_published,created_at,updated_at FROM {} WHERE id = ? LIMIT 1",
            target.table
        ),
        vec![d1_param(id)],
    )?;
    match result.rows.as_slice() {
        [] => Ok(None),
        [row] => parse_remote_post(row).map(Some),
        _ => Err("D1_RESPONSE_POST_CARDINALITY_INVALID".to_string()),
    }
}

fn read_content_version(
    client: &Client,
    token: &str,
    target: &ValidatedTarget,
    database_id: &str,
) -> Result<ContentVersionRecord, String> {
    let result = execute_read_query(
        client,
        token,
        target,
        database_id,
        "read_content_version",
        &format!(
            "SELECT payload,updated_at FROM {SETTINGS_TABLE} WHERE id = ? LIMIT 1"
        ),
        vec![d1_param(CONTENT_VERSION_KEY)],
    )?;
    match result.rows.as_slice() {
        [] => Ok(ContentVersionRecord {
            version: 0,
            payload: None,
            updated_at: None,
        }),
        [value] => {
            let row = row_object(value)?;
            let payload = parse_required_string(row, "payload")?;
            let updated_at = parse_nullable_string(row, "updated_at")?;
            if payload.len() > 4_096
                || updated_at.len() > 128
                || updated_at.chars().any(char::is_control)
            {
                return Err("CONTENT_VERSION_RECORD_INVALID".to_string());
            }
            let parsed: ContentVersionPayload = serde_json::from_str(&payload)
                .map_err(|_| "CONTENT_VERSION_PAYLOAD_INVALID".to_string())?;
            DateTime::parse_from_rfc3339(&parsed.updated_at)
                .map_err(|_| "CONTENT_VERSION_PAYLOAD_INVALID".to_string())?;
            // The legacy table may contain SQLite's `YYYY-MM-DD HH:MM:SS`
            // default in this column. Preserve the exact value for optimistic
            // concurrency; the canonical timestamp lives inside the JSON
            // payload and is the one required to be RFC 3339.
            Ok(ContentVersionRecord {
                version: parsed.version,
                payload: Some(payload),
                updated_at: Some(updated_at),
            })
        }
        _ => Err("CONTENT_VERSION_CARDINALITY_INVALID".to_string()),
    }
}

fn validate_publishable_draft(draft: &MainSiteDraft) -> Result<(), String> {
    validate_stored_draft(draft)?;
    if draft.is_about_site {
        return Err(
            "ABOUT_SITE_UNSUPPORTED: mainsite_about publication requires a separate reviewed contract"
                .to_string(),
        );
    }
    if draft.sanitizer_profile != MAINSITE_SANITIZER_PROFILE {
        return Err("SANITIZER_PROFILE_MISMATCH".to_string());
    }
    validate_mainsite_html(&draft.content)
}

fn diff_item(field: &str, change: MainSiteD1DiffChange) -> MainSiteD1DiffItem {
    MainSiteD1DiffItem {
        field: field.to_string(),
        change,
    }
}

fn field_change<T: PartialEq>(remote: &T, desired: &T) -> MainSiteD1DiffChange {
    if remote == desired {
        MainSiteD1DiffChange::Unchanged
    } else {
        MainSiteD1DiffChange::Changed
    }
}

fn build_diff(draft: &MainSiteDraft, remote: Option<&RemotePost>) -> Vec<MainSiteD1DiffItem> {
    let Some(remote) = remote else {
        return [
            "title",
            "content",
            "author",
            "is_pinned",
            "display_order",
            "is_published",
        ]
        .into_iter()
        .map(|field| diff_item(field, MainSiteD1DiffChange::New))
        .collect();
    };
    vec![
        diff_item("title", field_change(&remote.title, &draft.title)),
        diff_item("content", field_change(&remote.content, &draft.content)),
        diff_item("author", field_change(&remote.author, &draft.author)),
        diff_item("is_pinned", field_change(&remote.is_pinned, &0_i64)),
        diff_item(
            "display_order",
            field_change(&remote.display_order, &0_i64),
        ),
        diff_item(
            "is_published",
            field_change(&remote.is_published, &(draft.is_published as i64)),
        ),
    ]
}

fn plan_material(
    target: &ValidatedTarget,
    database_id: &str,
    action: MainSiteD1PublishAction,
    requested_post_id: Option<u64>,
    draft_hash: &str,
    remote_hash: Option<&str>,
    content_version_hash: Option<&str>,
    content_version_current: u64,
    content_version_next: u64,
) -> String {
    hash_parts(&[
        &target.account_id,
        &target.database,
        &target.table,
        &sha256_bytes(database_id.as_bytes()),
        match action {
            MainSiteD1PublishAction::Insert => "insert",
            MainSiteD1PublishAction::Update => "update",
        },
        &requested_post_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "auto".to_string()),
        draft_hash,
        remote_hash.unwrap_or("absent"),
        content_version_hash.unwrap_or("absent"),
        &content_version_current.to_string(),
        &content_version_next.to_string(),
    ])
}

fn build_plan(
    target: &ValidatedTarget,
    database_id: &str,
    draft: &MainSiteDraft,
    remote: Option<&RemotePost>,
    version: &ContentVersionRecord,
) -> Result<MainSiteD1PublishPlan, String> {
    let draft_hash = draft_hash(draft)?;
    let remote_hash = remote.map(remote_post_hash);
    let version_hash = content_version_hash(version);
    let next = version
        .version
        .checked_add(1)
        .ok_or_else(|| "CONTENT_VERSION_OVERFLOW".to_string())?;
    let action = if remote.is_some() {
        MainSiteD1PublishAction::Update
    } else {
        MainSiteD1PublishAction::Insert
    };
    let material = plan_material(
        target,
        database_id,
        action,
        draft.requested_post_id,
        &draft_hash,
        remote_hash.as_deref(),
        version_hash.as_deref(),
        version.version,
        next,
    );
    Ok(MainSiteD1PublishPlan {
        schema_version: PLAN_SCHEMA_VERSION.to_string(),
        plan_id: hash_parts(&["mainsite_d1_plan", &material]),
        action,
        database: target.database.clone(),
        table: target.table.clone(),
        requested_post_id: draft.requested_post_id,
        sql_intent: match action {
            MainSiteD1PublishAction::Insert => {
                "parameterized insert plus atomic mainsite/content-version increment".to_string()
            }
            MainSiteD1PublishAction::Update => {
                "optimistic parameterized update plus atomic mainsite/content-version increment"
                    .to_string()
            }
        },
        diff_summary: build_diff(draft, remote),
        draft_hash,
        remote_hash,
        content_version_hash: version_hash,
        content_version_current: version.version,
        content_version_next: next,
        confirmation_token: hash_parts(&["mainsite_d1_confirm", &material]),
        checked_at: Utc::now().to_rfc3339(),
        read_only: true,
    })
}

fn plans_match(preview: &MainSiteD1PublishPlan, current: &MainSiteD1PublishPlan) -> bool {
    preview.schema_version == current.schema_version
        && preview.plan_id == current.plan_id
        && preview.action == current.action
        && preview.database == current.database
        && preview.table == current.table
        && preview.requested_post_id == current.requested_post_id
        && preview.sql_intent == current.sql_intent
        && preview.diff_summary == current.diff_summary
        && preview.draft_hash == current.draft_hash
        && preview.remote_hash == current.remote_hash
        && preview.content_version_hash == current.content_version_hash
        && preview.content_version_current == current.content_version_current
        && preview.content_version_next == current.content_version_next
        && preview.confirmation_token == current.confirmation_token
        && preview.read_only
}

fn probe_sync(request: MainSiteD1ProbeRequest) -> Result<MainSiteD1ProbeResult, String> {
    let target = validate_target(&request.target)?;
    let token = token_for_target(&request.target)?;
    let client = cloudflare_client().map_err(|_| "CLOUDFLARE_CLIENT_UNAVAILABLE".to_string())?;
    let database_id = resolve_database_id(&client, &token, &target)?;
    validate_remote_schema(&client, &token, &target, &database_id)?;

    execute_read_query(
        &client,
        &token,
        &target,
        &database_id,
        "probe_post_read",
        &format!("SELECT id FROM {} LIMIT 1", target.table),
        Vec::new(),
    )?;
    read_content_version(&client, &token, &target, &database_id)?;

    Ok(MainSiteD1ProbeResult {
        schema_version: PROBE_SCHEMA_VERSION.to_string(),
        transport: "cloudflare_api".to_string(),
        database: target.database,
        table: target.table,
        readable: true,
        required_columns: POST_COLUMNS
            .iter()
            .chain(SETTINGS_COLUMNS.iter())
            .map(|value| (*value).to_string())
            .collect(),
        content_version_ready: true,
        checked_at: Utc::now().to_rfc3339(),
    })
}

fn preview_sync(
    request: MainSiteD1PreviewRequest,
) -> Result<MainSiteD1PublishPlan, String> {
    validate_publishable_draft(&request.draft)?;
    let target = validate_target(&request.target)?;
    let token = token_for_target(&request.target)?;
    let client = cloudflare_client().map_err(|_| "CLOUDFLARE_CLIENT_UNAVAILABLE".to_string())?;
    let database_id = resolve_database_id(&client, &token, &target)?;
    validate_remote_schema(&client, &token, &target, &database_id)?;
    let remote = read_post(
        &client,
        &token,
        &target,
        &database_id,
        request.draft.requested_post_id,
    )?;
    let version = read_content_version(&client, &token, &target, &database_id)?;
    build_plan(
        &target,
        &database_id,
        &request.draft,
        remote.as_ref(),
        &version,
    )
}

fn write_meta_reports_write(meta: &D1Meta) -> bool {
    meta.changes.is_some_and(|value| value >= 1)
        && meta.rows_written.is_some_and(|value| value >= 1)
}

fn content_version_payload(version: u64, updated_at: &str) -> String {
    json!({
        "version": version,
        "updated_at": updated_at,
    })
    .to_string()
}

fn build_post_statement(
    target: &ValidatedTarget,
    draft: &MainSiteDraft,
    remote: Option<&RemotePost>,
    version: &ContentVersionRecord,
    write_at: &str,
) -> (String, Vec<Value>) {
    let (version_guard, version_params) = if let Some(payload) = version.payload.as_deref() {
        (
            format!(
                "EXISTS(SELECT 1 FROM {SETTINGS_TABLE} WHERE id=? AND payload=? AND COALESCE(updated_at,'')=?)"
            ),
            vec![
                d1_param(CONTENT_VERSION_KEY),
                d1_param(payload),
                d1_param(version.updated_at.as_deref().unwrap_or_default()),
            ],
        )
    } else {
        (
            format!("NOT EXISTS(SELECT 1 FROM {SETTINGS_TABLE} WHERE id=?)"),
            vec![d1_param(CONTENT_VERSION_KEY)],
        )
    };
    if let Some(remote) = remote {
        let mut params = vec![
            d1_param(&draft.title),
            d1_param(&draft.content),
            d1_param(&draft.author),
            d1_param(0),
            d1_param(0),
            d1_param(draft.is_published as i64),
            d1_param(write_at),
            d1_param(remote.id),
            d1_param(&remote.title),
            d1_param(&remote.content),
            d1_param(&remote.author),
            d1_param(remote.is_pinned),
            d1_param(remote.display_order),
            d1_param(remote.is_published),
            d1_param(&remote.created_at),
            d1_param(&remote.updated_at),
        ];
        params.extend(version_params);
        return (
            format!(
                "UPDATE {} SET title=?,content=?,author=?,is_pinned=?,display_order=?,is_published=?,updated_at=? WHERE id=? AND title=? AND content=? AND COALESCE(author,'')=? AND is_pinned=? AND display_order=? AND is_published=? AND created_at=? AND COALESCE(updated_at,'')=? AND {version_guard}",
                target.table,
            ),
            params,
        );
    }

    if let Some(requested_id) = draft.requested_post_id {
        let mut params = vec![
            d1_param(requested_id),
            d1_param(&draft.title),
            d1_param(&draft.content),
            d1_param(&draft.author),
            d1_param(0),
            d1_param(0),
            d1_param(draft.is_published as i64),
            d1_param(write_at),
            d1_param(write_at),
        ];
        params.extend(version_params);
        (
            format!(
                "INSERT INTO {} (id,title,content,author,is_pinned,display_order,is_published,created_at,updated_at) SELECT ?,?,?,?,?,?,?,?,? WHERE {version_guard}",
                target.table,
            ),
            params,
        )
    } else {
        let mut params = vec![
            d1_param(&draft.title),
            d1_param(&draft.content),
            d1_param(&draft.author),
            d1_param(0),
            d1_param(0),
            d1_param(draft.is_published as i64),
            d1_param(write_at),
            d1_param(write_at),
        ];
        params.extend(version_params);
        (
            format!(
                "INSERT INTO {} (title,content,author,is_pinned,display_order,is_published,created_at,updated_at) SELECT ?,?,?,?,?,?,?,? WHERE {version_guard}",
                target.table,
            ),
            params,
        )
    }
}

fn build_content_version_statement(
    target: &ValidatedTarget,
    draft: &MainSiteDraft,
    remote: Option<&RemotePost>,
    previous: &ContentVersionRecord,
    next: u64,
    write_at: &str,
) -> (String, Vec<Value>, String) {
    let payload = content_version_payload(next, write_at);
    let (post_id_predicate, mut post_params) = if let Some(remote) = remote {
        ("id=?".to_string(), vec![d1_param(remote.id)])
    } else if let Some(requested_id) = draft.requested_post_id {
        ("id=?".to_string(), vec![d1_param(requested_id)])
    } else {
        ("id=last_insert_rowid()".to_string(), Vec::new())
    };
    post_params.extend([
        d1_param(&draft.title),
        d1_param(&draft.content),
        d1_param(&draft.author),
        d1_param(0),
        d1_param(0),
        d1_param(draft.is_published as i64),
        d1_param(write_at),
    ]);
    let post_guard = format!(
        "EXISTS(SELECT 1 FROM {} WHERE {post_id_predicate} AND title=? AND content=? AND COALESCE(author,'')=? AND is_pinned=? AND display_order=? AND is_published=? AND COALESCE(updated_at,'')=?)",
        target.table
    );
    let mut params = vec![d1_param(CONTENT_VERSION_KEY)];
    params.append(&mut post_params);
    params.extend([
        d1_param(&payload),
        d1_param(write_at),
        d1_param(previous.payload.as_deref().unwrap_or_default()),
        d1_param(previous.updated_at.as_deref().unwrap_or_default()),
    ]);
    (
        format!(
            "INSERT INTO {SETTINGS_TABLE} (id,payload,updated_at) VALUES (?,CASE WHEN {post_guard} THEN ? ELSE NULL END,?) ON CONFLICT(id) DO UPDATE SET payload=CASE WHEN {SETTINGS_TABLE}.payload=? AND COALESCE({SETTINGS_TABLE}.updated_at,'')=? AND excluded.payload IS NOT NULL THEN excluded.payload ELSE NULL END,updated_at=excluded.updated_at"
        ),
        params,
        payload,
    )
}

fn verify_readback(
    post: &RemotePost,
    draft: &MainSiteDraft,
    previous: Option<&RemotePost>,
    write_at: &str,
) -> Result<Vec<String>, String> {
    if post.title != draft.title
        || post.content != draft.content
        || post.author != draft.author
        || post.is_pinned != 0
        || post.display_order != 0
        || post.is_published != draft.is_published as i64
        || post.updated_at != write_at
    {
        return Err("D1_POST_READBACK_MISMATCH".to_string());
    }
    if let Some(previous) = previous {
        if post.id != previous.id || post.created_at != previous.created_at {
            return Err("D1_POST_READBACK_ID_OR_CREATED_AT_MISMATCH".to_string());
        }
    } else if post.created_at != write_at {
        return Err("D1_POST_READBACK_CREATED_AT_MISMATCH".to_string());
    }
    Ok([
        "id",
        "title",
        "content",
        "author",
        "is_pinned",
        "display_order",
        "is_published",
        "created_at",
        "updated_at",
        "content_sha256",
        "mainsite/content-version",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect())
}

fn publish_sync(
    request: MainSiteD1PublishRequest,
) -> Result<MainSiteD1PublishResult, String> {
    if !request.confirmed {
        return Err("CONFIRMATION_REQUIRED".to_string());
    }
    validate_publishable_draft(&request.draft)?;
    if request.preview.schema_version != PLAN_SCHEMA_VERSION || !request.preview.read_only {
        return Err("PREVIEW_SCHEMA_INVALID".to_string());
    }

    let target = validate_target(&request.target)?;
    let token = token_for_target(&request.target)?;
    let client = cloudflare_client().map_err(|_| "CLOUDFLARE_CLIENT_UNAVAILABLE".to_string())?;
    let database_id = resolve_database_id(&client, &token, &target)?;
    validate_remote_schema(&client, &token, &target, &database_id)?;

    // Re-read both optimistic-concurrency inputs. No write is attempted until
    // this newly built plan is byte-for-byte equivalent in all material fields.
    let remote = read_post(
        &client,
        &token,
        &target,
        &database_id,
        request.draft.requested_post_id,
    )?;
    let version = read_content_version(&client, &token, &target, &database_id)?;
    let current_plan = build_plan(
        &target,
        &database_id,
        &request.draft,
        remote.as_ref(),
        &version,
    )?;
    if !plans_match(&request.preview, &current_plan) {
        return Err(
            "REMOTE_STATE_CHANGED: create a new read-only preview before publishing".to_string(),
        );
    }

    let write_at = Utc::now().to_rfc3339();
    let (post_sql, post_params) =
        build_post_statement(&target, &request.draft, remote.as_ref(), &version, &write_at);
    let (version_sql, version_params, next_payload) = build_content_version_statement(
        &target,
        &request.draft,
        remote.as_ref(),
        &version,
        current_plan.content_version_next,
        &write_at,
    );
    let batch = json!({
        "batch": [
            { "sql": post_sql, "params": post_params },
            { "sql": version_sql, "params": version_params }
        ]
    });

    // This is the only mutation call. It is intentionally not retried: after a
    // transport failure the caller must inspect remote state through preview.
    let response = cloudflare_post_json(
        &client,
        &token,
        &d1_query_path(&target, &database_id),
        batch,
    )
    .map_err(|error| publish_error(&error))?;
    let results = parse_d1_results(&response, 2)
        .map_err(|_| "D1_PUBLISH_RESULT_INVALID_NO_RETRY".to_string())?;
    if !write_meta_reports_write(&results[0].meta)
        || !write_meta_reports_write(&results[1].meta)
    {
        return Err("D1_PUBLISH_WRITE_COUNT_MISMATCH_NO_RETRY".to_string());
    }

    let post_id = match current_plan.action {
        MainSiteD1PublishAction::Update => remote
            .as_ref()
            .map(|post| post.id)
            .ok_or_else(|| "D1_PUBLISH_ACTION_MISMATCH".to_string())?,
        MainSiteD1PublishAction::Insert => {
            if let Some(requested) = request.draft.requested_post_id {
                if results[0]
                    .meta
                    .last_row_id
                    .is_some_and(|returned| returned > 0 && returned != requested)
                {
                    return Err("D1_PUBLISH_LAST_ROW_ID_MISMATCH_NO_RETRY".to_string());
                }
                requested
            } else {
                results[0]
                    .meta
                    .last_row_id
                    .filter(|value| *value > 0)
                    .ok_or_else(|| "D1_PUBLISH_LAST_ROW_ID_INVALID_NO_RETRY".to_string())?
            }
        }
    };

    let post = read_post(
        &client,
        &token,
        &target,
        &database_id,
        Some(post_id),
    )?
    .ok_or_else(|| "D1_POST_READBACK_MISSING".to_string())?;
    let content_version = read_content_version(&client, &token, &target, &database_id)?;
    if content_version.version != current_plan.content_version_next
        || content_version.payload.as_deref() != Some(next_payload.as_str())
        || content_version.updated_at.as_deref() != Some(write_at.as_str())
    {
        return Err("D1_CONTENT_VERSION_READBACK_MISMATCH".to_string());
    }
    let fields_verified = verify_readback(
        &post,
        &request.draft,
        remote.as_ref(),
        &write_at,
    )?;

    Ok(MainSiteD1PublishResult {
        schema_version: PUBLISH_SCHEMA_VERSION.to_string(),
        plan_id: current_plan.plan_id,
        action: current_plan.action,
        post_id,
        draft_hash: current_plan.draft_hash,
        readback_hash: remote_post_hash(&post),
        content_version: content_version.version,
        fields_verified,
        verified: true,
        transport: "cloudflare_api".to_string(),
        published_at: write_at,
    })
}

#[tauri::command]
pub(crate) async fn probe_mainsite_d1(
    request: MainSiteD1ProbeRequest,
) -> Result<MainSiteD1ProbeResult, String> {
    tauri::async_runtime::spawn_blocking(move || probe_sync(request))
        .await
        .map_err(|_| "D1_PROBE_WORKER_FAILED".to_string())?
}

#[tauri::command]
pub(crate) async fn preview_mainsite_d1_publish(
    request: MainSiteD1PreviewRequest,
) -> Result<MainSiteD1PublishPlan, String> {
    tauri::async_runtime::spawn_blocking(move || preview_sync(request))
        .await
        .map_err(|_| "D1_PREVIEW_WORKER_FAILED".to_string())?
}

#[tauri::command]
pub(crate) async fn publish_mainsite_d1(
    request: MainSiteD1PublishRequest,
) -> Result<MainSiteD1PublishResult, String> {
    tauri::async_runtime::spawn_blocking(move || publish_sync(request))
        .await
        .map_err(|_| "D1_PUBLISH_WORKER_FAILED".to_string())?
}

fn validate_mainsite_html(html: &str) -> Result<(), String> {
    if html.chars().any(|character| {
        character.is_control() && !matches!(character, '\t' | '\n' | '\r')
    }) {
        return Err("HTML_CONTROL_CHARACTER_NOT_ALLOWED".to_string());
    }
    let mut position = 0;
    let mut stack = Vec::<String>::new();
    let mut tag_count = 0_usize;
    while let Some(relative) = html[position..].find('<') {
        tag_count += 1;
        if tag_count > 100_000 {
            return Err("HTML_TAG_COUNT_LIMIT_EXCEEDED".to_string());
        }
        let start = position + relative;
        let end = find_tag_end(html, start + 1)?;
        let raw = &html[start + 1..end];
        let tag = parse_tag(raw)?;
        validate_tag(&tag)?;
        if tag.closing {
            let open = stack
                .pop()
                .ok_or_else(|| "HTML_UNBALANCED_CLOSING_TAG".to_string())?;
            if open != tag.name {
                return Err("HTML_MISMATCHED_TAG".to_string());
            }
        } else if !tag.self_closing && !is_void_tag(&tag.name) {
            stack.push(tag.name);
            if stack.len() > 4_096 {
                return Err("HTML_NESTING_LIMIT_EXCEEDED".to_string());
            }
        }
        position = end + 1;
    }
    if !stack.is_empty() {
        return Err("HTML_UNCLOSED_TAG".to_string());
    }
    Ok(())
}

fn find_tag_end(html: &str, mut position: usize) -> Result<usize, String> {
    let bytes = html.as_bytes();
    let mut quote = None;
    while position < bytes.len() {
        let byte = bytes[position];
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Ok(position);
        }
        position += 1;
    }
    Err("HTML_UNTERMINATED_TAG".to_string())
}

fn parse_tag(raw: &str) -> Result<ParsedTag, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('!') || raw.starts_with('?') {
        return Err("HTML_UNSUPPORTED_DECLARATION".to_string());
    }
    let bytes = raw.as_bytes();
    let mut position = 0;
    let closing = bytes.first() == Some(&b'/');
    if closing {
        position += 1;
    }
    let name_start = position;
    while position < bytes.len() && bytes[position].is_ascii_alphanumeric() {
        position += 1;
    }
    if position == name_start {
        return Err("HTML_TAG_NAME_INVALID".to_string());
    }
    let original_name = &raw[name_start..position];
    let name = original_name.to_ascii_lowercase();
    if original_name != name.as_str() {
        return Err("HTML_TAG_NOT_CANONICAL".to_string());
    }
    if closing {
        if !raw[position..].trim().is_empty() {
            return Err("HTML_CLOSING_TAG_HAS_ATTRIBUTES".to_string());
        }
        return Ok(ParsedTag {
            name,
            closing: true,
            self_closing: false,
            attributes: Vec::new(),
        });
    }

    let mut attributes = Vec::new();
    let mut self_closing = false;
    while position < bytes.len() {
        while position < bytes.len() && bytes[position].is_ascii_whitespace() {
            position += 1;
        }
        if position == bytes.len() {
            break;
        }
        if bytes[position] == b'/' {
            position += 1;
            while position < bytes.len() && bytes[position].is_ascii_whitespace() {
                position += 1;
            }
            if position != bytes.len() {
                return Err("HTML_SELF_CLOSING_TAG_INVALID".to_string());
            }
            self_closing = true;
            break;
        }

        let attr_start = position;
        while position < bytes.len()
            && (bytes[position].is_ascii_alphanumeric()
                || matches!(bytes[position], b'-' | b'_'))
        {
            position += 1;
        }
        if position == attr_start {
            return Err("HTML_ATTRIBUTE_NAME_INVALID".to_string());
        }
        let original_attr_name = &raw[attr_start..position];
        let attr_name = original_attr_name.to_ascii_lowercase();
        if original_attr_name != attr_name.as_str() {
            return Err("HTML_ATTRIBUTE_NOT_CANONICAL".to_string());
        }
        while position < bytes.len() && bytes[position].is_ascii_whitespace() {
            position += 1;
        }
        let value = if position < bytes.len() && bytes[position] == b'=' {
            position += 1;
            while position < bytes.len() && bytes[position].is_ascii_whitespace() {
                position += 1;
            }
            let quote = *bytes
                .get(position)
                .filter(|byte| **byte == b'\'' || **byte == b'"')
                .ok_or_else(|| "HTML_ATTRIBUTE_VALUE_MUST_BE_QUOTED".to_string())?;
            if quote != b'"' {
                return Err("HTML_ATTRIBUTE_VALUE_NOT_CANONICAL".to_string());
            }
            position += 1;
            let value_start = position;
            while position < bytes.len() && bytes[position] != quote {
                position += 1;
            }
            if position == bytes.len() {
                return Err("HTML_ATTRIBUTE_VALUE_UNTERMINATED".to_string());
            }
            let value = raw[value_start..position].to_string();
            position += 1;
            Some(value)
        } else {
            None
        };
        if attributes
            .iter()
            .any(|(existing, _)| existing == &attr_name)
        {
            return Err("HTML_DUPLICATE_ATTRIBUTE".to_string());
        }
        attributes.push((attr_name, value));
    }

    Ok(ParsedTag {
        name,
        closing: false,
        self_closing,
        attributes,
    })
}

fn is_void_tag(tag: &str) -> bool {
    matches!(tag, "br" | "col" | "hr" | "img" | "input")
}

fn allowed_tag(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "abbr"
            | "b"
            | "blockquote"
            | "br"
            | "caption"
            | "code"
            | "col"
            | "colgroup"
            | "del"
            | "div"
            | "em"
            | "figcaption"
            | "figure"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "i"
            | "iframe"
            | "img"
            | "input"
            | "label"
            | "li"
            | "mark"
            | "ol"
            | "p"
            | "pre"
            | "s"
            | "span"
            | "strong"
            | "sub"
            | "sup"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "tr"
            | "u"
            | "ul"
    )
}

fn allowed_attribute(tag: &str, attribute: &str) -> bool {
    match tag {
        "a" => matches!(attribute, "href" | "name" | "target" | "rel" | "title"),
        "blockquote" => attribute == "cite",
        "code" | "pre" => attribute == "class",
        "col" => matches!(attribute, "span" | "style" | "width"),
        "colgroup" => matches!(attribute, "span" | "width"),
        "div" => matches!(attribute, "class" | "data-youtube-video" | "style"),
        "figure" => matches!(attribute, "class" | "style"),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => attribute == "style",
        "iframe" => matches!(
            attribute,
            "allow"
                | "allowfullscreen"
                | "frameborder"
                | "height"
                | "scrolling"
                | "src"
                | "style"
                | "title"
                | "width"
        ),
        "img" => matches!(
            attribute,
            "alt" | "data-width" | "height" | "loading" | "src" | "style" | "title" | "width"
        ),
        "input" => matches!(attribute, "checked" | "disabled" | "type"),
        "label" => attribute == "for",
        "li" => matches!(attribute, "data-checked" | "data-type" | "style"),
        "mark" => attribute == "style",
        "ol" => matches!(attribute, "start" | "style" | "type"),
        "p" => attribute == "style",
        "span" => matches!(attribute, "class" | "style"),
        "table" => matches!(attribute, "style" | "width"),
        "td" | "th" => matches!(
            attribute,
            "colspan" | "colwidth" | "rowspan" | "scope" | "style"
        ),
        "ul" => matches!(attribute, "data-type" | "style"),
        _ => false,
    }
}

fn validate_tag(tag: &ParsedTag) -> Result<(), String> {
    if !allowed_tag(&tag.name) {
        return Err("HTML_TAG_NOT_ALLOWED".to_string());
    }
    if tag.self_closing {
        return Err("HTML_SELF_CLOSING_TAG_NOT_CANONICAL".to_string());
    }
    if tag.closing {
        return Ok(());
    }
    for (name, value) in &tag.attributes {
        if name.starts_with("on") || !allowed_attribute(&tag.name, name) {
            return Err("HTML_ATTRIBUTE_NOT_ALLOWED".to_string());
        }
        validate_attribute_value(&tag.name, name, value.as_deref())?;
    }
    if tag.name == "iframe"
        && !tag.attributes.iter().any(|(name, _)| name == "src")
    {
        return Err("HTML_IFRAME_SRC_REQUIRED".to_string());
    }
    if tag.name == "a" {
        if let Some((_, Some(href))) = tag.attributes.iter().find(|(name, _)| name == "href") {
            let has_rel = tag.attributes.iter().any(|(name, value)| {
                name == "rel" && value.as_deref() == Some("noopener noreferrer")
            });
            if !has_rel {
                return Err("HTML_LINK_TRANSFORM_MISSING".to_string());
            }
            if !is_youtube_link(href)? {
                let has_target = tag.attributes.iter().any(|(name, value)| {
                    name == "target" && value.as_deref() == Some("_blank")
                });
                if !has_target {
                    return Err("HTML_LINK_TRANSFORM_MISSING".to_string());
                }
            }
        }
    }
    if tag.name == "img"
        && !tag.attributes.iter().any(|(name, value)| {
            name == "loading"
                && matches!(value.as_deref(), Some("lazy") | Some("eager"))
        })
    {
        return Err("HTML_IMAGE_LOADING_MISSING".to_string());
    }
    if tag.name == "input"
        && !tag
            .attributes
            .iter()
            .any(|(name, value)| name == "type" && value.as_deref() == Some("checkbox"))
    {
        return Err("HTML_INPUT_TYPE_NOT_ALLOWED".to_string());
    }
    Ok(())
}

fn validate_attribute_value(
    tag: &str,
    name: &str,
    value: Option<&str>,
) -> Result<(), String> {
    let boolean = matches!(name, "allowfullscreen" | "checked" | "disabled" | "data-youtube-video");
    if value.is_none() && !boolean {
        return Err("HTML_ATTRIBUTE_VALUE_REQUIRED".to_string());
    }
    let Some(value) = value else {
        return Ok(());
    };
    if value.len() > 2_048 || value.chars().any(char::is_control) {
        return Err("HTML_ATTRIBUTE_VALUE_INVALID".to_string());
    }
    match name {
        "href" | "src" | "cite" => validate_url_attribute(tag, name, value),
        "style" => validate_style(value),
        "class" => {
            if value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, ' ' | '_' | '-'))
            {
                Ok(())
            } else {
                Err("HTML_CLASS_VALUE_INVALID".to_string())
            }
        }
        "target" if value != "_blank" => Err("HTML_LINK_TARGET_INVALID".to_string()),
        "rel" if value != "noopener noreferrer" => Err("HTML_LINK_REL_INVALID".to_string()),
        "loading" if !matches!(value, "lazy" | "eager") => {
            Err("HTML_IMAGE_LOADING_INVALID".to_string())
        }
        "type" if tag == "input" && value != "checkbox" => {
            Err("HTML_INPUT_TYPE_NOT_ALLOWED".to_string())
        }
        _ => Ok(()),
    }
}

fn decode_url_entities(value: &str) -> Result<String, String> {
    let mut decoded = String::with_capacity(value.len());
    let mut remainder = value;
    while let Some(index) = remainder.find('&') {
        decoded.push_str(&remainder[..index]);
        remainder = &remainder[index..];
        if let Some(rest) = remainder.strip_prefix("&amp;") {
            decoded.push('&');
            remainder = rest;
        } else {
            return Err("HTML_URL_ENTITY_NOT_ALLOWED".to_string());
        }
    }
    decoded.push_str(remainder);
    Ok(decoded)
}

fn validate_url_attribute(tag: &str, name: &str, value: &str) -> Result<(), String> {
    let decoded = decode_url_entities(value)?;
    let trimmed = decoded.trim();
    if decoded != trimmed {
        return Err("HTML_URL_NOT_CANONICAL".to_string());
    }
    if trimmed.is_empty()
        || trimmed.starts_with("//")
        || trimmed.contains('\\')
        || trimmed.chars().any(char::is_control)
    {
        return Err("HTML_URL_INVALID".to_string());
    }
    let compact_scheme = trimmed
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect::<String>();
    if has_explicit_url_scheme(&compact_scheme) && !has_explicit_url_scheme(trimmed) {
        return Err("HTML_URL_SCHEME_OBFUSCATED".to_string());
    }
    if trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || (name == "href" && (trimmed.starts_with('#') || trimmed.starts_with('?')))
    {
        if tag == "iframe" {
            return Err("HTML_IFRAME_URL_INVALID".to_string());
        }
        return Ok(());
    }
    let parsed = match Url::parse(trimmed) {
        Ok(parsed) => parsed,
        Err(_) if tag != "iframe" && !has_explicit_url_scheme(trimmed) => return Ok(()),
        Err(_) => return Err("HTML_URL_INVALID".to_string()),
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("HTML_URL_CREDENTIALS_NOT_ALLOWED".to_string());
    }
    if tag == "iframe" {
        let hostname = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if !matches!(parsed.scheme(), "http" | "https")
            || !matches!(
                hostname.as_str(),
                "youtube.com"
                    | "www.youtube.com"
                    | "youtube-nocookie.com"
                    | "www.youtube-nocookie.com"
            )
            || !parsed.path().starts_with("/embed/")
        {
            return Err("HTML_IFRAME_URL_INVALID".to_string());
        }
        return Ok(());
    }
    if matches!(parsed.scheme(), "http" | "https")
        || (name == "href" && parsed.scheme() == "mailto")
    {
        Ok(())
    } else {
        Err("HTML_URL_SCHEME_NOT_ALLOWED".to_string())
    }
}

fn has_explicit_url_scheme(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    let prefix = &value[..colon];
    !prefix.is_empty()
        && prefix
            .chars()
            .enumerate()
            .all(|(index, character)| {
                if index == 0 {
                    character.is_ascii_alphabetic()
                } else {
                    character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
                }
            })
}

fn is_youtube_link(value: &str) -> Result<bool, String> {
    let decoded = decode_url_entities(value)?;
    let Ok(parsed) = Url::parse(decoded.trim()) else {
        return Ok(false);
    };
    Ok(matches!(
        parsed.host_str().unwrap_or_default().to_ascii_lowercase().as_str(),
        "youtube.com"
            | "www.youtube.com"
            | "youtu.be"
            | "youtube-nocookie.com"
            | "www.youtube-nocookie.com"
    ))
}

fn numeric_with_units(value: &str, units: &[&str], allow_unitless: bool) -> bool {
    if value == "0" {
        return true;
    }
    let number = units
        .iter()
        .find_map(|unit| value.strip_suffix(unit))
        .or_else(|| allow_unitless.then_some(value));
    let Some(number) = number else {
        return false;
    };
    if number.is_empty()
        || number.starts_with('-')
        || number.matches('.').count() > 1
        || !number.as_bytes().first().is_some_and(u8::is_ascii_digit)
        || !number.as_bytes().last().is_some_and(u8::is_ascii_digit)
        || !number
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
    {
        return false;
    }
    number.parse::<f64>().is_ok_and(|number| number.is_finite())
}

fn safe_color(value: &str) -> bool {
    if let Some(hex) = value.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8)
            && hex.chars().all(|character| character.is_ascii_hexdigit());
    }
    if value.chars().all(|character| character.is_ascii_alphabetic()) {
        return true;
    }
    let lowercase = value.to_ascii_lowercase();
    let (prefix, components, hsl) = if lowercase.starts_with("rgba(") {
        ("rgba(", 4, false)
    } else if lowercase.starts_with("rgb(") {
        ("rgb(", 3, false)
    } else if lowercase.starts_with("hsla(") {
        ("hsla(", 4, true)
    } else if lowercase.starts_with("hsl(") {
        ("hsl(", 3, true)
    } else {
        return false;
    };
    let Some(inner) = lowercase
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(')'))
    else {
        return false;
    };
    let parts = inner.split(',').map(str::trim).collect::<Vec<_>>();
    if parts.len() != components {
        return false;
    }
    parts.iter().enumerate().all(|(index, part)| {
        if components == 4 && index == 3 {
            return matches!(*part, "0" | "1")
                || part
                    .strip_prefix("0.")
                    .or_else(|| part.strip_prefix('.'))
                    .is_some_and(|fraction| {
                        !fraction.is_empty()
                            && fraction.chars().all(|character| character.is_ascii_digit())
                    });
        }
        if hsl && matches!(index, 1 | 2) {
            return part
                .strip_suffix('%')
                .is_some_and(|number| {
                    !number.is_empty()
                        && number.len() <= 3
                        && number.chars().all(|character| character.is_ascii_digit())
                });
        }
        !part.is_empty()
            && part.len() <= 3
            && part.chars().all(|character| character.is_ascii_digit())
    })
}

fn style_value_allowed(property: &str, value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    let value = normalized.as_str();
    match property {
        "background-color" | "color" => safe_color(value),
        "display" => matches!(value, "block" | "inline" | "inline-block" | "flex" | "inline-flex"),
        "font-family" => value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(character, ',' | '\'' | '"' | '_' | '-')
        }),
        "font-size" | "margin-bottom" | "margin-top" | "text-indent" => {
            numeric_with_units(value, &["rem", "px", "em", "%", "pt", "vh", "vw"], false)
        }
        "font-style" => matches!(value, "normal" | "italic" | "oblique"),
        "font-weight" => {
            matches!(value, "normal" | "bold" | "bolder" | "lighter")
                || matches!(value, "100" | "200" | "300" | "400" | "500" | "600" | "700" | "800" | "900")
        }
        "height" | "max-width" | "min-width" | "width" => {
            value == "auto"
                || numeric_with_units(value, &["rem", "px", "em", "%", "vw", "vh"], false)
        }
        "line-height" => {
            value == "normal"
                || numeric_with_units(value, &["rem", "px", "em", "%"], true)
        }
        "text-align" => matches!(value, "left" | "right" | "center" | "justify" | "start" | "end"),
        "text-decoration" => matches!(value, "none" | "underline" | "line-through" | "overline"),
        "vertical-align" => matches!(value, "baseline" | "sub" | "super" | "top" | "middle" | "bottom"),
        _ => false,
    }
}

fn validate_style(value: &str) -> Result<(), String> {
    let forbidden = value.to_ascii_lowercase();
    if forbidden.contains("url(")
        || forbidden.contains("expression")
        || forbidden.contains("javascript")
        || forbidden.contains("data:")
        || forbidden.contains("@import")
        || forbidden.contains("var(")
        || forbidden.contains('\\')
    {
        return Err("HTML_STYLE_VALUE_INVALID".to_string());
    }
    let mut canonical = Vec::new();
    for declaration in value.split(';').filter(|part| !part.trim().is_empty()) {
        let (property, raw_value) = declaration
            .split_once(':')
            .ok_or_else(|| "HTML_STYLE_DECLARATION_INVALID".to_string())?;
        let property = property.trim().to_ascii_lowercase();
        let raw_value = raw_value.trim();
        if raw_value.is_empty()
            || raw_value.len() > 160
            || !raw_value.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || character.is_ascii_whitespace()
                    || matches!(
                        character,
                        '#' | '(' | ')' | ',' | '.' | '%' | '-' | '_' | '\'' | '"' | '/'
                    )
            })
            || !style_value_allowed(&property, raw_value)
        {
            return Err("HTML_STYLE_VALUE_INVALID".to_string());
        }
        canonical.push(format!("{property}: {raw_value}"));
    }
    if canonical.is_empty() || canonical.join("; ") != value {
        return Err("HTML_STYLE_NOT_CANONICAL".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mainsite_draft::{MAINSITE_DRAFT_SCHEMA_VERSION, MAINSITE_SANITIZER_PROFILE};

    fn draft(content: &str) -> MainSiteDraft {
        let now = Utc::now().to_rfc3339();
        MainSiteDraft {
            schema_version: MAINSITE_DRAFT_SCHEMA_VERSION.to_string(),
            requested_post_id: Some(7),
            title: "Titulo".to_string(),
            author: "Autoria".to_string(),
            content: content.to_string(),
            is_pinned: false,
            display_order: 0,
            is_published: false,
            is_about_site: false,
            sanitizer_profile: MAINSITE_SANITIZER_PROFILE.to_string(),
            content_sha256: sha256_bytes(content.as_bytes()),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    fn target() -> ValidatedTarget {
        ValidatedTarget {
            account_id: "account_fixture".to_string(),
            database: "example_db".to_string(),
            table: "mainsite_posts".to_string(),
            allow_wrangler_fallback: false,
        }
    }

    #[test]
    fn identifiers_fail_closed_before_sql_interpolation() {
        assert_eq!(
            validate_table_identifier("mainsite_posts").as_deref(),
            Ok("mainsite_posts")
        );
        assert!(validate_table_identifier("mainsite_posts;DROP TABLE x").is_err());
        assert!(validate_database_name("../secret").is_err());
    }

    #[test]
    fn html_validator_accepts_contract_markup_and_rejects_active_content() {
        let safe = r#"<h2 style="text-align: center">Titulo</h2><p><a href="https://example.com/x?a=1&amp;b=2" target="_blank" rel="noopener noreferrer">Fonte</a></p><ul data-type="taskList"><li data-type="taskItem" data-checked="true"><input type="checkbox" checked disabled></li></ul>"#;
        assert!(validate_mainsite_html(safe).is_ok());
        assert!(validate_mainsite_html("<script>alert(1)</script>").is_err());
        assert!(validate_mainsite_html("<img src=\"javascript:alert(1)\">").is_err());
        assert!(validate_mainsite_html("<p onclick=\"x()\">x</p>").is_err());
        assert!(validate_mainsite_html("<img src=\"data:text/html,x\">").is_err());
        assert!(validate_mainsite_html(
            "<a href=\"https://example.com\" target=\"_blank\">sem rel</a>"
        )
        .is_err());
        assert!(validate_mainsite_html(
            "<a href=\"java script:alert(1)\" target=\"_blank\" rel=\"noopener noreferrer\">x</a>"
        )
        .is_err());
        assert!(validate_mainsite_html("<p style=\"text-align: unsafe\">x</p>").is_err());
    }

    #[test]
    fn about_site_is_an_explicit_capability_block() {
        let mut value = draft("<p>Conteudo</p>");
        value.is_about_site = true;
        let error = validate_publishable_draft(&value).expect_err("about must block");
        assert!(error.starts_with("ABOUT_SITE_UNSUPPORTED"));
    }

    #[test]
    fn plan_changes_when_remote_or_content_version_changes() {
        let value = draft("<p>Conteudo</p>");
        let first_version = ContentVersionRecord {
            version: 1,
            payload: Some(r#"{"version":1,"updated_at":"2026-08-21T00:00:00Z"}"#.to_string()),
            updated_at: Some("2026-08-21T00:00:00Z".to_string()),
        };
        let first = build_plan(
            &target(),
            "database-id-fixture-a",
            &value,
            None,
            &first_version,
        )
        .expect("plan");
        let mut changed_version = first_version.clone();
        changed_version.version = 2;
        changed_version.payload = Some(
            r#"{"version":2,"updated_at":"2026-08-21T00:01:00Z"}"#.to_string(),
        );
        let second = build_plan(
            &target(),
            "database-id-fixture-a",
            &value,
            None,
            &changed_version,
        )
        .expect("plan");
        assert_ne!(first.confirmation_token, second.confirmation_token);

        let recreated = build_plan(
            &target(),
            "database-id-fixture-b",
            &value,
            None,
            &first_version,
        )
        .expect("plan");
        assert_ne!(first.confirmation_token, recreated.confirmation_token);
    }

    #[test]
    fn write_result_requires_each_item_and_all_write_meta() {
        let valid = json!({
            "success": true,
            "result": [
                {"success": true, "results": [], "meta": {"changes": 1, "rows_written": 1, "last_row_id": 7}},
                {"success": true, "results": [], "meta": {"changes": 1, "rows_written": 1, "last_row_id": 4}}
            ]
        });
        let parsed = parse_d1_results(&valid, 2).expect("valid response");
        assert!(parsed.iter().all(|item| write_meta_reports_write(&item.meta)));

        let mut invalid = valid;
        invalid["result"][1]["meta"]["rows_written"] = json!(0);
        let parsed = parse_d1_results(&invalid, 2).expect("shape still parses");
        assert!(!write_meta_reports_write(&parsed[1].meta));
    }

    #[test]
    fn post_sql_contains_only_validated_identifier_and_placeholders() {
        let value = draft("<p>segredo editorial</p>");
        let (sql, params) = build_post_statement(
            &target(),
            &value,
            None,
            &ContentVersionRecord {
                version: 0,
                payload: None,
                updated_at: None,
            },
            "2026-08-21T00:00:00Z",
        );
        assert!(sql.contains("mainsite_posts"));
        assert!(!sql.contains("segredo editorial"));
        assert!(params.iter().all(|param| param.as_str().is_some()));
        assert!(params
            .iter()
            .any(|param| param.as_str() == Some("<p>segredo editorial</p>")));
    }
}
