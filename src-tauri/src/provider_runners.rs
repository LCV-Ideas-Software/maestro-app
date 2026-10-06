// Modulo: src-tauri/src/provider_runners.rs
// Descricao: OpenAI/Anthropic/Gemini API peer runners + shared provider helpers
// extracted from lib.rs in v0.3.22.
//
// This module owns the 3 isomorphic editorial-peer runners that share the unified
// provider helper family (`write_provider_failure_result` and friends). DeepSeek
// lives in `provider_deepseek.rs` (extracted in v0.3.21) because it predates the
// unified family and keeps its own custom error helper.
//
// What's here:
//   - `run_openai_api_agent` / `run_anthropic_api_agent` / `run_gemini_api_agent`
//     — the 3 chat-completions runners. Same shape: cost pre-flight, retry,
//     response parsing, success/error envelope building.
//   - Unified provider helpers: `editorial_api_system_prompt`,
//     `api_cost_preflight_result`, `write_provider_missing_key_result`,
//     `write_provider_error_result`, `write_provider_failure_result`,
//     `write_provider_success_result`, `log_provider_api_started`.
//   - Per-provider model resolvers: `resolve_openai_model`,
//     `resolve_anthropic_model`, `resolve_gemini_model` (with shared
//     `choose_preferred_model`/`api_model_ids` and gemini-specific
//     `gemini_model_ids`).
//   - Per-provider response parsers: `openai_response_text`,
//     `anthropic_response_text`, `gemini_response_text`, `gemini_usage_tokens`.
//
// What stayed in lib.rs (consumed via `pub(crate)` re-imports):
//   - `provider_label_for_agent`, `provider_remote_present`,
//     `provider_key_for_agent` — the last has an external caller in
//     `should_run_agent_via_api`.
//   - `api_input_estimate_chars`, `provider_supports_native_attachment`,
//     `attachment_within_native_payload_cap`, the 4 `*_api_attachment_supported`
//     helpers, `openai_api_input`, `anthropic_api_user_content`,
//     `gemini_api_user_parts` — all have unit tests in `lib.rs::tests`.
//
// v0.3.22 is a pure move: every signature, log line, format string and status
// string is identical to the v0.3.21 lib.rs source (commit 8ef11ba).

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use chrono::Utc;
use reqwest::Client;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::api_payloads::validate_native_provider_payload;
use crate::app_paths::checked_data_child_path;

use crate::logging::{write_log_record, LogEventInput, LogSession};
use crate::provider_retry::{
    build_api_client_async, provider_http_error_status, provider_reqwest_error_status,
    send_with_retry_async, ProviderRequestOutcome,
};
use crate::session_controls::{
    anthropic_provider_cost, api_role_max_tokens, estimate_provider_cost_from_input_chars,
    openai_provider_cost, provider_cache_plan, provider_cache_telemetry,
    provider_cache_telemetry_with_plan, provider_cost, usage_tokens, ProviderCachePlan,
    ProviderCostGuard,
};
use crate::session_evidence::AttachmentManifestEntry;
use crate::{
    anthropic_api_user_content, api_error_message, api_input_estimate_chars,
    extract_maestro_status, gemini_api_user_parts, log_editorial_agent_finished, openai_api_input,
    provider_key_for_agent, provider_label_for_agent, provider_remote_present, sanitize_short,
    sanitize_text, write_text_file, AiProviderConfig, EditorialAgentResult, ProviderCacheTelemetry,
};

/// Provider invocation identity bundle. Groups the 7 parameters that flow
/// through every editorial helper (`api_cost_preflight_result`, the
/// `write_provider_*_result` family) so signatures fit within clippy's
/// `too_many_arguments` 7-arg threshold without per-helper `#[allow]`
/// annotations. Built once inside each runner from its hardcoded
/// `name`/`cli`/`provider` constants plus the shared spawn context.
#[derive(Clone, Copy)]
pub(crate) struct ProviderInvocation<'a> {
    pub log_session: &'a LogSession,
    pub run_id: &'a str,
    pub name: &'a str,
    pub cli: &'a str,
    pub provider: &'a str,
    pub role: &'a str,
    pub output_path: &'a Path,
}

fn optional_u64_label(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn optional_string_label(value: Option<&str>) -> String {
    value
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn provider_cache_artifact_lines(cache: Option<&ProviderCacheTelemetry>) -> String {
    let Some(cache) = cache else {
        return "- Cache provider mode: `none`\n- Cache key hash: `unknown`\n- Cache control status: `unknown`\n- Cache retention: `unknown`\n- Cache cached input tokens: `unknown`\n- Cache hit tokens: `unknown`\n- Cache miss tokens: `unknown`\n- Cache read input tokens: `unknown`\n- Cache creation input tokens: `unknown`\n"
            .to_string();
    };
    format!(
        "- Cache provider mode: `{}`\n- Cache key hash: `{}`\n- Cache control status: `{}`\n- Cache retention: `{}`\n- Cache cached input tokens: `{}`\n- Cache hit tokens: `{}`\n- Cache miss tokens: `{}`\n- Cache read input tokens: `{}`\n- Cache creation input tokens: `{}`\n",
        sanitize_text(&cache.provider_mode, 80),
        optional_string_label(cache.cache_key_hash.as_deref()),
        optional_string_label(cache.cache_control_status.as_deref()),
        optional_string_label(cache.cache_retention.as_deref()),
        optional_u64_label(cache.cached_input_tokens),
        optional_u64_label(cache.cache_hit_tokens),
        optional_u64_label(cache.cache_miss_tokens),
        optional_u64_label(cache.cache_read_input_tokens),
        optional_u64_label(cache.cache_creation_input_tokens),
    )
}

/// Per-call editorial-agent request bundle. Groups the 9 parameters that
/// `run_*_api_agent` runners receive from
/// `editorial_agent_runners::run_editorial_agent_for_spec`. Passed by-value
/// so each runner takes ownership of `prompt: String` and
/// `cost_guard: Option<ProviderCostGuard>`. The DeepSeek runner ignores
/// `attachments` because chat-completions does not accept inline payloads.
pub(crate) struct EditorialAgentRequest<'a> {
    pub log_session: &'a LogSession,
    pub run_id: &'a str,
    pub role: &'a str,
    pub prompt: String,
    pub attachments: &'a [AttachmentManifestEntry],
    pub output_path: &'a Path,
    pub timeout: Option<Duration>,
    pub config: &'a AiProviderConfig,
    pub cost_guard: Option<ProviderCostGuard>,
}

pub(crate) fn editorial_api_system_prompt(agent_name: &str) -> String {
    format!(
        "You are the {agent_name} peer inside Maestro Editorial AI. Internal coordination between agents must be in en_US. Operator-facing draft/revision deliverables must be in Brazilian Portuguese (pt_BR). Read the complete operator request, editorial protocol, and provided artifacts. Answer only what the current role asks for. In review roles, the first line must follow the MAESTRO_STATUS contract exactly."
    )
}

fn anthropic_system_prompt_with_cache_control(system_prompt: &str) -> Value {
    json!([
        {
            "type": "text",
            "text": system_prompt,
            "cache_control": {
                "type": "ephemeral"
            }
        }
    ])
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn log_provider_cache_configured(
    log_session: &LogSession,
    run_id: &str,
    provider: &str,
    model: &str,
    role: &str,
    output_path: &Path,
    prompt_chars: usize,
    cache_plan: &ProviderCachePlan,
) {
    let _ = write_log_record(
        log_session,
        LogEventInput {
            level: "info".to_string(),
            category: "session.provider.cache.configured".to_string(),
            message: "provider prompt cache policy configured".to_string(),
            context: Some(json!({
                "run_id": sanitize_short(run_id, 120),
                "provider": provider,
                "model": sanitize_text(model, 120),
                "role": role,
                "provider_mode": cache_plan.provider_mode,
                "cache_key_hash": cache_plan.cache_key_hash,
                "cache_control_status": cache_plan.cache_control_status,
                "cache_retention": cache_plan.cache_retention,
                "stable_prefix_chars": cache_plan.stable_prefix_chars,
                "prompt_chars": prompt_chars,
                "output_path": output_path.to_string_lossy().to_string()
            })),
        },
    );
    write_provider_cache_manifest(
        run_id,
        provider,
        model,
        role,
        output_path,
        prompt_chars,
        cache_plan,
    );
}

fn write_provider_cache_manifest(
    run_id: &str,
    provider: &str,
    model: &str,
    role: &str,
    output_path: &Path,
    prompt_chars: usize,
    cache_plan: &ProviderCachePlan,
) {
    let Some(agent_dir) = output_path.parent() else {
        return;
    };
    let Some(session_dir) = agent_dir.parent() else {
        return;
    };
    let manifest_path = match checked_data_child_path(&session_dir.join("cache-manifest.ndjson")) {
        Ok(path) => path,
        Err(_) => return,
    };
    if let Some(parent) = manifest_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let record = json!({
        "timestamp": Utc::now().to_rfc3339(),
        "run_id": sanitize_short(run_id, 120),
        "provider": provider,
        "model": sanitize_text(model, 120),
        "role": role,
        "provider_mode": cache_plan.provider_mode,
        "cache_key_hash": cache_plan.cache_key_hash,
        "cache_control_status": cache_plan.cache_control_status,
        "cache_retention": cache_plan.cache_retention,
        "stable_prefix_chars": cache_plan.stable_prefix_chars,
        "prompt_chars": prompt_chars
    });
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&manifest_path)
    {
        let _ = writeln!(file, "{record}");
    }
}

pub(crate) fn api_cost_preflight_result(
    invocation: &ProviderInvocation,
    input_estimate_chars: usize,
    max_tokens: u64,
    cost_guard: Option<&ProviderCostGuard>,
    duration_ms: u128,
) -> Option<EditorialAgentResult> {
    let guard = cost_guard?;
    let max_session_cost_usd = guard.max_session_cost_usd?;
    let estimated_cost =
        estimate_provider_cost_from_input_chars(input_estimate_chars, max_tokens, guard.rates);
    if guard.observed_cost_usd + estimated_cost <= max_session_cost_usd {
        return None;
    }
    let note = format!(
        "{} nao foi chamado: custo projetado ${:.6} somado ao observado ${:.6} excede o limite ${:.6}.",
        provider_label_for_agent(match invocation.provider {
            "anthropic" => "claude",
            "openai" => "codex",
            "gemini" => "gemini",
            "deepseek" => "deepseek",
            "grok" => "grok",
            "perplexity" => "perplexity",
            other => other,
        }),
        estimated_cost,
        guard.observed_cost_usd,
        max_session_cost_usd
    );
    Some(write_provider_failure_result(
        invocation,
        "unknown",
        "COST_LIMIT_REACHED",
        "blocked",
        &note,
        duration_ms,
        Some(estimated_cost),
    ))
}

pub(crate) fn write_provider_missing_key_result(
    invocation: &ProviderInvocation,
    model: &str,
    remote_present: bool,
) -> EditorialAgentResult {
    let status = if remote_present {
        "REMOTE_SECRET_NOT_READABLE"
    } else {
        "API_KEY_NOT_AVAILABLE"
    };
    let note = if remote_present {
        format!(
            "A referencia do segredo de {} existe no Cloudflare Secrets Store, mas a Cloudflare nao devolve o valor bruto ao app local. Informe a chave na tela Ajustes > Agentes via API ou use a variavel de API correspondente.",
            invocation.name,
        )
    } else {
        format!(
            "{} precisa de chave informada na tela Ajustes > Agentes via API para executar no modo API.",
            invocation.name,
        )
    };
    write_provider_failure_result(invocation, model, status, "blocked", &note, 0, None)
}

pub(crate) fn write_provider_error_result(
    invocation: &ProviderInvocation,
    model: &str,
    status: &str,
    duration_ms: u128,
) -> EditorialAgentResult {
    write_provider_failure_result(
        invocation,
        model,
        status,
        "error",
        status,
        duration_ms,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_provider_error_result_with_accounting(
    invocation: &ProviderInvocation,
    model: &str,
    status: &str,
    duration_ms: u128,
    usage_input_tokens: Option<u64>,
    usage_output_tokens: Option<u64>,
    cost_usd: Option<f64>,
    cost_estimated: Option<bool>,
) -> EditorialAgentResult {
    write_provider_failure_result_accounted(
        invocation,
        model,
        status,
        "error",
        status,
        duration_ms,
        None,
        usage_input_tokens,
        usage_output_tokens,
        cost_usd,
        cost_estimated,
    )
}

pub(crate) fn write_provider_failure_result(
    invocation: &ProviderInvocation,
    model: &str,
    status: &str,
    tone: &str,
    note: &str,
    duration_ms: u128,
    projected_cost_usd: Option<f64>,
) -> EditorialAgentResult {
    write_provider_failure_result_accounted(
        invocation,
        model,
        status,
        tone,
        note,
        duration_ms,
        projected_cost_usd,
        None,
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn write_provider_failure_result_accounted(
    invocation: &ProviderInvocation,
    model: &str,
    status: &str,
    tone: &str,
    note: &str,
    duration_ms: u128,
    projected_cost_usd: Option<f64>,
    usage_input_tokens: Option<u64>,
    usage_output_tokens: Option<u64>,
    cost_usd: Option<f64>,
    cost_estimated: Option<bool>,
) -> EditorialAgentResult {
    let safe_status = sanitize_text(status, 240);
    let safe_note = sanitize_text(note, 2000);
    let projected_line = projected_cost_usd
        .map(|value| format!("- Cost projected USD: `{value:.6}`\n"))
        .unwrap_or_default();
    let usage_lines = format!(
        "- Usage input tokens: `{}`\n- Usage output tokens: `{}`\n",
        optional_u64_label(usage_input_tokens),
        optional_u64_label(usage_output_tokens)
    );
    let billed_line = cost_usd
        .map(|value| {
            let label = match cost_estimated {
                Some(true) => "Cost estimated USD",
                Some(false) => "Cost observed USD",
                None => "Cost USD",
            };
            format!("- {label}: `{value:.8}`\n")
        })
        .unwrap_or_default();
    let _ = write_text_file(
        invocation.output_path,
        &format!(
            "# {} - {}\n\n- CLI: `{}`\n- Provider: `{}`\n- Model: `{}`\n- Status: `{}`\n- Tone: `{}`\n- Exit code: `unknown`\n- Duration ms: `{duration_ms}`\n{}{}{}{}\n## Stdout\n\n```text\n\n```\n\n## Stderr\n\n```text\n{}\n```\n",
            invocation.name,
            invocation.role,
            invocation.cli,
            invocation.provider,
            sanitize_text(model, 120),
            safe_status,
            tone,
            usage_lines,
            projected_line,
            billed_line,
            if safe_note.is_empty() {
                String::new()
            } else {
                format!("\n{safe_note}\n")
            },
            safe_note
        ),
    );
    let result = EditorialAgentResult {
        name: invocation.name.to_string(),
        role: invocation.role.to_string(),
        cli: invocation.cli.to_string(),
        tone: tone.to_string(),
        status: safe_status,
        duration_ms,
        exit_code: None,
        output_path: invocation.output_path.to_string_lossy().to_string(),
        usage_input_tokens,
        usage_output_tokens,
        cost_usd,
        cost_estimated,
        cache: None,
    };
    log_editorial_agent_finished(
        invocation.log_session,
        invocation.run_id,
        &result,
        None,
        Some(safe_note.len()),
        None,
        false,
    );
    result
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_provider_success_result(
    log_session: &LogSession,
    run_id: &str,
    name: &str,
    cli: &str,
    provider: &str,
    role: &str,
    output_path: &Path,
    model: &str,
    model_reported: &str,
    key_source: &str,
    stdout: &str,
    usage_input_tokens: Option<u64>,
    usage_output_tokens: Option<u64>,
    cost_usd: Option<f64>,
    cost_estimated: Option<bool>,
    cache: Option<ProviderCacheTelemetry>,
    duration_ms: u128,
    prompt_chars: usize,
    endpoint: &str,
) -> EditorialAgentResult {
    let status = if role == "review" {
        extract_maestro_status(stdout).unwrap_or("NOT_READY")
    } else if stdout.trim().is_empty() {
        "EMPTY_DRAFT"
    } else {
        "DRAFT_CREATED"
    };
    let tone = if status == "READY" || status == "DRAFT_CREATED" {
        "ok"
    } else if status == "EMPTY_DRAFT" {
        "error"
    } else {
        "warn"
    };
    let cost_label = match cost_estimated {
        Some(true) => "Cost estimated USD",
        Some(false) => "Cost observed USD",
        None => "Cost USD",
    };
    let artifact = format!(
        "# {name} - {role}\n\n- CLI: `{cli}`\n- Provider: `{provider}`\n- Model: `{}`\n- Model reported: `{}`\n- Key source: `{}`\n- Status: `{status}`\n- Tone: `{tone}`\n- Exit code: `0`\n- Duration ms: `{duration_ms}`\n- Prompt chars: `{prompt_chars}`\n- Stdout chars: `{}`\n- Usage input tokens: `{}`\n- Usage output tokens: `{}`\n{}- {cost_label}: `{}`\n- Stderr chars: `0`\n\n## Stdout\n\n```text\n{}\n```\n\n## Stderr\n\n```text\n\n```\n",
        sanitize_text(model, 120),
        sanitize_text(model_reported, 120),
        sanitize_text(key_source, 120),
        stdout.chars().count(),
        usage_input_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        usage_output_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        provider_cache_artifact_lines(cache.as_ref()),
        cost_usd
            .map(|value| format!("{value:.8}"))
            .unwrap_or_else(|| "unknown".to_string()),
        stdout
    );
    let _ = write_text_file(output_path, &artifact);
    let result = EditorialAgentResult {
        name: name.to_string(),
        role: role.to_string(),
        cli: cli.to_string(),
        tone: tone.to_string(),
        status: status.to_string(),
        duration_ms,
        exit_code: Some(0),
        output_path: output_path.to_string_lossy().to_string(),
        usage_input_tokens,
        usage_output_tokens,
        cost_usd,
        cost_estimated,
        cache,
    };
    log_editorial_agent_finished(
        log_session,
        run_id,
        &result,
        Some(stdout.chars().count()),
        Some(0),
        Some(endpoint.to_string()),
        false,
    );
    result
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn log_provider_api_started(
    log_session: &LogSession,
    run_id: &str,
    name: &str,
    cli: &str,
    provider: &str,
    role: &str,
    model: &str,
    prompt_chars: usize,
    output_path: &Path,
    timeout: Option<Duration>,
    cost_guard: Option<&ProviderCostGuard>,
) {
    let _ = write_log_record(
        log_session,
        LogEventInput {
            level: "info".to_string(),
            category: "session.agent.started".to_string(),
            message: "editorial API peer request starting".to_string(),
            context: Some(json!({
                "run_id": sanitize_short(run_id, 120),
                "agent": name,
                "role": role,
                "cli": cli,
                "provider": provider,
                "model": model,
                "prompt_chars": prompt_chars,
                "output_path": output_path.to_string_lossy().to_string(),
                "timeout_seconds": timeout.map(|value| value.as_secs()),
                "timeout_policy": if timeout.is_some() { "session_deadline" } else { "none_editorial_session" },
                "cost_limit_usd": cost_guard.and_then(|guard| guard.max_session_cost_usd),
                "observed_cost_usd": cost_guard.map(|guard| guard.observed_cost_usd)
            })),
        },
    );
}

pub(crate) async fn run_openai_api_agent(
    request: EditorialAgentRequest<'_>,
    cancel_token: &CancellationToken,
) -> EditorialAgentResult {
    let EditorialAgentRequest {
        log_session,
        run_id,
        role,
        prompt,
        attachments,
        output_path,
        timeout,
        config,
        cost_guard,
    } = request;
    let started = Instant::now();
    let name = "Codex";
    let cli = "openai-api";
    let provider = "openai";
    let model_hint = "gpt-6-astra";
    let invocation = ProviderInvocation {
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        output_path,
    };
    let Some((api_key, key_source)) = provider_key_for_agent(config, "codex") else {
        return write_provider_missing_key_result(
            &invocation,
            model_hint,
            provider_remote_present(config, "codex"),
        );
    };

    let max_tokens = api_role_max_tokens(role);
    let input_estimate_chars = api_input_estimate_chars(&prompt, attachments, provider);
    if let Some(result) = api_cost_preflight_result(
        &invocation,
        input_estimate_chars,
        max_tokens,
        cost_guard.as_ref(),
        started.elapsed().as_millis(),
    ) {
        return result;
    }

    let async_client = match build_api_client_async(timeout) {
        Ok(client) => client,
        Err(error) => {
            let status = provider_reqwest_error_status("CLIENT_ERROR", error);
            return write_provider_error_result(
                &invocation,
                model_hint,
                &status,
                started.elapsed().as_millis(),
            );
        }
    };
    let model = resolve_openai_model(&async_client, &api_key, cancel_token).await;
    let system_prompt = editorial_api_system_prompt(name);
    let cache_plan = provider_cache_plan(provider, &model, role, name, &system_prompt);
    let input = match openai_api_input(&prompt, attachments) {
        Ok(input) => input,
        Err(error) => {
            let status = sanitize_text(&format!("ATTACHMENT_ERROR: {error}"), 240);
            return write_provider_error_result(
                &invocation,
                &model,
                &status,
                started.elapsed().as_millis(),
            );
        }
    };
    log_provider_api_started(
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        &model,
        prompt.chars().count(),
        output_path,
        timeout,
        cost_guard.as_ref(),
    );
    log_provider_cache_configured(
        log_session,
        run_id,
        provider,
        &model,
        role,
        output_path,
        prompt.chars().count(),
        &cache_plan,
    );

    let mut body = json!({
        "model": model,
        "instructions": system_prompt,
        "input": input,
        "max_output_tokens": if model.starts_with("gpt-4.1") { max_tokens.min(32_768) } else { max_tokens },
        "store": false,
        "service_tier": "default",
        "prompt_cache_key": cache_plan.cache_key
    });
    if let Some(retention) = cache_plan.cache_retention.as_deref() {
        if retention == "30m" {
            body["prompt_cache_options"] = json!({ "ttl": "30m" });
        } else {
            body["prompt_cache_retention"] = json!(retention);
        }
    }
    if model.starts_with("gpt-6") || model.starts_with("gpt-5.6") {
        body["reasoning"] = json!({ "effort": "max" });
    }
    if let Err(error) = validate_native_provider_payload(provider, &body) {
        return write_provider_error_result(
            &invocation,
            &model,
            &format!("ATTACHMENT_ERROR: {error}"),
            started.elapsed().as_millis(),
        );
    }
    let endpoint = "https://api.openai.com/v1/responses";
    let request_builder = async_client
        .post(endpoint)
        .bearer_auth(&api_key)
        .json(&body);
    let response =
        match send_with_retry_async(log_session, run_id, "openai", cancel_token, request_builder)
            .await
        {
            Ok(response) => response,
            Err(ProviderRequestOutcome::Cancelled) => {
                return write_provider_failure_result(
                    &invocation,
                    &model,
                    "STOPPED_BY_USER",
                    "blocked",
                    "Sessao parada pelo operador antes da resposta do provedor.",
                    started.elapsed().as_millis(),
                    None,
                );
            }
            Err(ProviderRequestOutcome::Network(error)) => {
                let status = provider_reqwest_error_status("PROVIDER_NETWORK_ERROR", error);
                return write_provider_error_result(
                    &invocation,
                    &model,
                    &status,
                    started.elapsed().as_millis(),
                );
            }
        };

    let http_status = response.status();
    let body_text = tokio::select! {
        biased;
        _ = cancel_token.cancelled() => {
            return write_provider_failure_result(
                &invocation,
                &model,
                "STOPPED_BY_USER",
                "blocked",
                "Sessao parada pelo operador durante leitura da resposta do provedor.",
                started.elapsed().as_millis(),
                None,
            );
        }
        r = response.text() => r.unwrap_or_default(),
    };

    if !http_status.is_success() {
        let status =
            provider_http_error_status(http_status.as_u16(), &api_error_message(&body_text));
        return write_provider_error_result(
            &invocation,
            &model,
            &status,
            started.elapsed().as_millis(),
        );
    }

    let parsed: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| json!({}));
    let (usage_input_tokens, usage_output_tokens) = usage_tokens(&parsed);
    let cache = Some(provider_cache_telemetry_with_plan(
        &cache_plan,
        provider_cache_telemetry(provider, &parsed, usage_input_tokens),
    ));
    let cost_usd = cost_guard
        .as_ref()
        .and_then(|guard| openai_provider_cost(&parsed, &model, guard.rates));
    let stdout = match completed_provider_response_text(provider, &parsed) {
        Ok(text) => text,
        Err(status) => {
            return write_provider_error_result_with_accounting(
                &invocation,
                &model,
                status,
                started.elapsed().as_millis(),
                usage_input_tokens,
                usage_output_tokens,
                cost_usd,
                cost_usd.map(|_| true),
            );
        }
    };
    let model_reported = parsed
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(&model);
    write_provider_success_result(
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        output_path,
        &model,
        model_reported,
        &key_source,
        &stdout,
        usage_input_tokens,
        usage_output_tokens,
        cost_usd,
        cost_usd.map(|_| true),
        cache,
        started.elapsed().as_millis(),
        prompt.chars().count(),
        endpoint,
    )
}

pub(crate) async fn run_anthropic_api_agent(
    request: EditorialAgentRequest<'_>,
    cancel_token: &CancellationToken,
) -> EditorialAgentResult {
    let EditorialAgentRequest {
        log_session,
        run_id,
        role,
        prompt,
        attachments,
        output_path,
        timeout,
        config,
        cost_guard,
    } = request;
    let started = Instant::now();
    let name = "Claude";
    let cli = "anthropic-api";
    let provider = "anthropic";
    let model_hint = "claude-fable-5-1";
    let invocation = ProviderInvocation {
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        output_path,
    };
    let Some((api_key, key_source)) = provider_key_for_agent(config, "claude") else {
        return write_provider_missing_key_result(
            &invocation,
            model_hint,
            provider_remote_present(config, "claude"),
        );
    };

    let max_tokens = api_role_max_tokens(role);
    let input_estimate_chars = api_input_estimate_chars(&prompt, attachments, provider);
    if let Some(result) = api_cost_preflight_result(
        &invocation,
        input_estimate_chars,
        max_tokens,
        cost_guard.as_ref(),
        started.elapsed().as_millis(),
    ) {
        return result;
    }

    let async_client = match build_api_client_async(timeout) {
        Ok(client) => client,
        Err(error) => {
            let status = provider_reqwest_error_status("CLIENT_ERROR", error);
            return write_provider_error_result(
                &invocation,
                model_hint,
                &status,
                started.elapsed().as_millis(),
            );
        }
    };
    let model = resolve_anthropic_model(&async_client, &api_key, cancel_token).await;
    let system_prompt = editorial_api_system_prompt(name);
    let cache_plan = provider_cache_plan(provider, &model, role, name, &system_prompt);
    let content = match anthropic_api_user_content(&prompt, attachments) {
        Ok(content) => content,
        Err(error) => {
            let status = sanitize_text(&format!("ATTACHMENT_ERROR: {error}"), 240);
            return write_provider_error_result(
                &invocation,
                &model,
                &status,
                started.elapsed().as_millis(),
            );
        }
    };
    let system_content = anthropic_system_prompt_with_cache_control(&system_prompt);
    log_provider_api_started(
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        &model,
        prompt.chars().count(),
        output_path,
        timeout,
        cost_guard.as_ref(),
    );
    log_provider_cache_configured(
        log_session,
        run_id,
        provider,
        &model,
        role,
        output_path,
        prompt.chars().count(),
        &cache_plan,
    );

    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system_content,
        "messages": [
            { "role": "user", "content": content }
        ]
    });
    if model.starts_with("claude-fable-") || model.starts_with("claude-opus-5") {
        body["thinking"] = json!({ "type": "adaptive" });
        body["output_config"] = json!({ "effort": "max" });
    }
    if let Err(error) = validate_native_provider_payload(provider, &body) {
        return write_provider_error_result(
            &invocation,
            &model,
            &format!("ATTACHMENT_ERROR: {error}"),
            started.elapsed().as_millis(),
        );
    }
    let endpoint = "https://api.anthropic.com/v1/messages";
    let request_builder = async_client
        .post(endpoint)
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body);
    let response = match send_with_retry_async(
        log_session,
        run_id,
        "anthropic",
        cancel_token,
        request_builder,
    )
    .await
    {
        Ok(response) => response,
        Err(ProviderRequestOutcome::Cancelled) => {
            return write_provider_failure_result(
                &invocation,
                &model,
                "STOPPED_BY_USER",
                "blocked",
                "Sessao parada pelo operador antes da resposta do provedor.",
                started.elapsed().as_millis(),
                None,
            );
        }
        Err(ProviderRequestOutcome::Network(error)) => {
            let status = provider_reqwest_error_status("PROVIDER_NETWORK_ERROR", error);
            return write_provider_error_result(
                &invocation,
                &model,
                &status,
                started.elapsed().as_millis(),
            );
        }
    };

    let http_status = response.status();
    let body_text = tokio::select! {
        biased;
        _ = cancel_token.cancelled() => {
            return write_provider_failure_result(
                &invocation,
                &model,
                "STOPPED_BY_USER",
                "blocked",
                "Sessao parada pelo operador durante leitura da resposta do provedor.",
                started.elapsed().as_millis(),
                None,
            );
        }
        r = response.text() => r.unwrap_or_default(),
    };

    if !http_status.is_success() {
        let status =
            provider_http_error_status(http_status.as_u16(), &api_error_message(&body_text));
        return write_provider_error_result(
            &invocation,
            &model,
            &status,
            started.elapsed().as_millis(),
        );
    }

    let parsed: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| json!({}));
    let (usage_input_tokens, usage_output_tokens) = usage_tokens(&parsed);
    let cache = Some(provider_cache_telemetry_with_plan(
        &cache_plan,
        provider_cache_telemetry(provider, &parsed, usage_input_tokens),
    ));
    let cost_usd = cost_guard
        .as_ref()
        .and_then(|guard| anthropic_provider_cost(&parsed, guard.rates));
    let stdout = match completed_provider_response_text(provider, &parsed) {
        Ok(text) => text,
        Err(status) => {
            return write_provider_error_result_with_accounting(
                &invocation,
                &model,
                status,
                started.elapsed().as_millis(),
                usage_input_tokens,
                usage_output_tokens,
                cost_usd,
                cost_usd.map(|_| true),
            );
        }
    };
    let model_reported = parsed
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(&model);
    write_provider_success_result(
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        output_path,
        &model,
        model_reported,
        &key_source,
        &stdout,
        usage_input_tokens,
        usage_output_tokens,
        cost_usd,
        cost_usd.map(|_| true),
        cache,
        started.elapsed().as_millis(),
        prompt.chars().count(),
        endpoint,
    )
}

pub(crate) async fn run_gemini_api_agent(
    request: EditorialAgentRequest<'_>,
    cancel_token: &CancellationToken,
) -> EditorialAgentResult {
    let EditorialAgentRequest {
        log_session,
        run_id,
        role,
        prompt,
        attachments,
        output_path,
        timeout,
        config,
        cost_guard,
    } = request;
    let started = Instant::now();
    let name = "Gemini";
    let cli = "gemini-api";
    let provider = "gemini";
    let model_hint = "gemini-3.1-pro-preview";
    let invocation = ProviderInvocation {
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        output_path,
    };
    let Some((api_key, key_source)) = provider_key_for_agent(config, "gemini") else {
        return write_provider_missing_key_result(
            &invocation,
            model_hint,
            provider_remote_present(config, "gemini"),
        );
    };

    let max_tokens = api_role_max_tokens(role);
    let input_estimate_chars = api_input_estimate_chars(&prompt, attachments, provider);
    if let Some(result) = api_cost_preflight_result(
        &invocation,
        input_estimate_chars,
        max_tokens,
        cost_guard.as_ref(),
        started.elapsed().as_millis(),
    ) {
        return result;
    }

    let async_client = match build_api_client_async(timeout) {
        Ok(client) => client,
        Err(error) => {
            let status = provider_reqwest_error_status("CLIENT_ERROR", error);
            return write_provider_error_result(
                &invocation,
                model_hint,
                &status,
                started.elapsed().as_millis(),
            );
        }
    };
    let model = resolve_gemini_model(&async_client, &api_key, cancel_token).await;
    let system_prompt = editorial_api_system_prompt(name);
    let cache_plan = provider_cache_plan(provider, &model, role, name, &system_prompt);
    let parts = match gemini_api_user_parts(&prompt, attachments) {
        Ok(parts) => parts,
        Err(error) => {
            let status = sanitize_text(&format!("ATTACHMENT_ERROR: {error}"), 240);
            return write_provider_error_result(
                &invocation,
                &model,
                &status,
                started.elapsed().as_millis(),
            );
        }
    };
    log_provider_api_started(
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        &model,
        prompt.chars().count(),
        output_path,
        timeout,
        cost_guard.as_ref(),
    );
    log_provider_cache_configured(
        log_session,
        run_id,
        provider,
        &model,
        role,
        output_path,
        prompt.chars().count(),
        &cache_plan,
    );

    let endpoint =
        format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent");
    let mut body = json!({
        "systemInstruction": {
            "parts": [{ "text": system_prompt }]
        },
        "contents": [
            {
                "role": "user",
                "parts": parts
            }
        ],
        "generationConfig": {
            "maxOutputTokens": max_tokens
        }
    });
    if model.starts_with("gemini-3") {
        body["generationConfig"]["thinkingConfig"] = json!({ "thinkingLevel": "high" });
    }
    if let Err(error) = validate_native_provider_payload(provider, &body) {
        return write_provider_error_result(
            &invocation,
            &model,
            &format!("ATTACHMENT_ERROR: {error}"),
            started.elapsed().as_millis(),
        );
    }
    let request_builder = async_client
        .post(&endpoint)
        .header("x-goog-api-key", &api_key)
        .json(&body);
    let response =
        match send_with_retry_async(log_session, run_id, "gemini", cancel_token, request_builder)
            .await
        {
            Ok(response) => response,
            Err(ProviderRequestOutcome::Cancelled) => {
                return write_provider_failure_result(
                    &invocation,
                    &model,
                    "STOPPED_BY_USER",
                    "blocked",
                    "Sessao parada pelo operador antes da resposta do provedor.",
                    started.elapsed().as_millis(),
                    None,
                );
            }
            Err(ProviderRequestOutcome::Network(error)) => {
                let status = provider_reqwest_error_status("PROVIDER_NETWORK_ERROR", error);
                return write_provider_error_result(
                    &invocation,
                    &model,
                    &status,
                    started.elapsed().as_millis(),
                );
            }
        };

    let http_status = response.status();
    let body_text = tokio::select! {
        biased;
        _ = cancel_token.cancelled() => {
            return write_provider_failure_result(
                &invocation,
                &model,
                "STOPPED_BY_USER",
                "blocked",
                "Sessao parada pelo operador durante leitura da resposta do provedor.",
                started.elapsed().as_millis(),
                None,
            );
        }
        r = response.text() => r.unwrap_or_default(),
    };

    if !http_status.is_success() {
        let status =
            provider_http_error_status(http_status.as_u16(), &api_error_message(&body_text));
        return write_provider_error_result(
            &invocation,
            &model,
            &status,
            started.elapsed().as_millis(),
        );
    }

    let parsed: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| json!({}));
    let (usage_input_tokens, usage_output_tokens) = gemini_usage_tokens(&parsed);
    let cache = Some(provider_cache_telemetry_with_plan(
        &cache_plan,
        provider_cache_telemetry(provider, &parsed, usage_input_tokens),
    ));
    let cost_usd = cost_guard.as_ref().and_then(|guard| {
        usage_input_tokens
            .zip(usage_output_tokens)
            .map(|(input, output)| provider_cost(input, output, guard.rates))
    });
    let stdout = match completed_provider_response_text(provider, &parsed) {
        Ok(text) => text,
        Err(status) => {
            return write_provider_error_result_with_accounting(
                &invocation,
                &model,
                status,
                started.elapsed().as_millis(),
                usage_input_tokens,
                usage_output_tokens,
                cost_usd,
                cost_usd.map(|_| true),
            );
        }
    };
    let model_reported = parsed
        .pointer("/modelVersion")
        .and_then(Value::as_str)
        .unwrap_or(&model);
    write_provider_success_result(
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        output_path,
        &model,
        model_reported,
        &key_source,
        &stdout,
        usage_input_tokens,
        usage_output_tokens,
        cost_usd,
        cost_usd.map(|_| true),
        cache,
        started.elapsed().as_millis(),
        prompt.chars().count(),
        &endpoint,
    )
}

pub(crate) async fn fetch_provider_model_catalog(
    request: reqwest::RequestBuilder,
    cancel_token: &CancellationToken,
) -> Option<Value> {
    // Bound only metadata discovery; editorial generation keeps its own policy.
    let fetch = async {
        let response = request.timeout(Duration::from_secs(30)).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.json::<Value>().await.ok()
    };
    tokio::select! {
        biased;
        _ = cancel_token.cancelled() => None,
        catalog = fetch => catalog,
    }
}

pub(crate) async fn resolve_openai_model(
    client: &Client,
    api_key: &str,
    cancel_token: &CancellationToken,
) -> String {
    let request = client
        .get("https://api.openai.com/v1/models")
        .bearer_auth(api_key);
    if let Some(value) = fetch_provider_model_catalog(request, cancel_token).await {
        return openai_catalog_model(&value);
    }
    "gpt-6-astra".to_string()
}

fn openai_catalog_model(value: &Value) -> String {
    choose_preferred_model(
        &api_model_ids(value),
        &[
            "gpt-6-astra",
            "gpt-6.1-sol",
            "gpt-6-sol",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
            "gpt-5.4",
            "gpt-5.2",
            "gpt-5",
            "gpt-4.1",
        ],
        "gpt-6-astra",
    )
}

pub(crate) async fn resolve_anthropic_model(
    client: &Client,
    api_key: &str,
    cancel_token: &CancellationToken,
) -> String {
    let request = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01");
    if let Some(models) = fetch_anthropic_model_ids(request, cancel_token).await {
        return anthropic_catalog_model(&models);
    }
    "claude-fable-5-1".to_string()
}

async fn fetch_anthropic_model_ids(
    request: reqwest::RequestBuilder,
    cancel_token: &CancellationToken,
) -> Option<Vec<String>> {
    let request = request.query(&[("limit", 1000)]);
    let mut cursor = None;
    let mut seen_cursors = HashSet::new();
    let mut models = Vec::new();
    loop {
        let mut page_request = request.try_clone()?;
        if let Some(after_id) = &cursor {
            page_request = page_request.query(&[("after_id", after_id)]);
        }
        let page = fetch_provider_model_catalog(page_request, cancel_token).await?;
        models.extend(api_model_ids(&page));
        if !page.get("has_more")?.as_bool()? {
            return Some(models);
        }
        let after_id = page.get("last_id")?.as_str()?.to_string();
        if after_id.is_empty() || !seen_cursors.insert(after_id.clone()) {
            return None;
        }
        cursor = Some(after_id);
    }
}

fn anthropic_catalog_model(models: &[String]) -> String {
    choose_preferred_model(
        models,
        &[
            "claude-fable-5-1",
            "claude-fable-5",
            "claude-opus-5-5",
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-sonnet-5",
            "claude-opus-4-1-20250805",
        ],
        "claude-fable-5-1",
    )
}

pub(crate) async fn resolve_gemini_model(
    client: &Client,
    api_key: &str,
    cancel_token: &CancellationToken,
) -> String {
    let request = client
        .get("https://generativelanguage.googleapis.com/v1beta/models")
        .header("x-goog-api-key", api_key);
    if let Some(models) = fetch_gemini_model_ids(request, cancel_token).await {
        return gemini_catalog_model(&models);
    }
    "gemini-3.1-pro-preview".to_string()
}

async fn fetch_gemini_model_ids(
    request: reqwest::RequestBuilder,
    cancel_token: &CancellationToken,
) -> Option<Vec<String>> {
    let request = request.query(&[("pageSize", 1000)]);
    let mut cursor = None;
    let mut seen_cursors = HashSet::new();
    let mut models = Vec::new();
    loop {
        let mut page_request = request.try_clone()?;
        if let Some(page_token) = &cursor {
            page_request = page_request.query(&[("pageToken", page_token)]);
        }
        let page = fetch_provider_model_catalog(page_request, cancel_token).await?;
        models.extend(gemini_model_ids(&page));
        let Some(next_page_token) = page.get("nextPageToken") else {
            return Some(models);
        };
        let next_page_token = next_page_token.as_str()?.to_string();
        if next_page_token.is_empty() {
            return Some(models);
        }
        if !seen_cursors.insert(next_page_token.clone()) {
            return None;
        }
        cursor = Some(next_page_token);
    }
}

fn gemini_catalog_model(models: &[String]) -> String {
    choose_preferred_model(
        models,
        &["gemini-3.1-pro-preview", "gemini-2.5-pro"],
        "gemini-3.1-pro-preview",
    )
}

pub(crate) fn choose_preferred_model(
    models: &[String],
    candidates: &[&str],
    fallback: &str,
) -> String {
    for candidate in candidates {
        if models.iter().any(|model| model == candidate) {
            return (*candidate).to_string();
        }
    }
    // Catalog ordering does not imply editorial or even text-generation support.
    fallback.to_string()
}

fn api_model_ids(value: &Value) -> Vec<String> {
    value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("id")
                        .and_then(Value::as_str)
                        .map(|id| sanitize_short(id, 120))
                })
                .filter(|id| !id.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn gemini_model_ids(value: &Value) -> Vec<String> {
    value
        .get("models")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let methods = item
                        .get("supportedGenerationMethods")
                        .and_then(Value::as_array);
                    if let Some(methods) = methods {
                        let supports_generate_content = methods.iter().any(|method| {
                            method
                                .as_str()
                                .map(|value| value == "generateContent")
                                .unwrap_or(false)
                        });
                        if !supports_generate_content {
                            return None;
                        }
                    }
                    item.get("name").and_then(Value::as_str).map(|name| {
                        sanitize_short(name.strip_prefix("models/").unwrap_or(name), 120)
                    })
                })
                .filter(|id| !id.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// A successful HTTP status does not prove a completed editorial response.
/// Reject partial, refused, blocked, and tool-only output before its first line
/// can become a READY vote, while the caller keeps the paid usage in the ledger.
pub(crate) fn completed_provider_response_text(
    provider: &str,
    value: &Value,
) -> Result<String, &'static str> {
    let text = match provider {
        "openai" | "grok" => {
            if value.get("status").and_then(Value::as_str) != Some("completed")
                || value.get("error").is_some_and(|error| !error.is_null())
                || value
                    .get("incomplete_details")
                    .is_some_and(|detail| !detail.is_null())
                || value
                    .get("output")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("type").and_then(Value::as_str) == Some("message")
                                && item
                                    .get("status")
                                    .and_then(Value::as_str)
                                    .is_some_and(|status| status != "completed")
                        })
                    })
            {
                return Err("PROVIDER_INCOMPLETE_RESPONSE");
            }
            openai_response_text(value)
        }
        "anthropic" => {
            if !matches!(
                value.get("stop_reason").and_then(Value::as_str),
                Some("end_turn" | "stop_sequence")
            ) {
                return Err("PROVIDER_INCOMPLETE_RESPONSE");
            }
            anthropic_response_text(value)
        }
        "gemini" => {
            if value.pointer("/promptFeedback/blockReason").is_some()
                || value
                    .pointer("/candidates/0/finishReason")
                    .and_then(Value::as_str)
                    != Some("STOP")
            {
                return Err("PROVIDER_INCOMPLETE_RESPONSE");
            }
            gemini_response_text(value)
        }
        _ => return Err("PROVIDER_INVALID_RESPONSE"),
    };
    text.filter(|text| !text.trim().is_empty())
        .ok_or("PROVIDER_EMPTY_CONTENT")
}

pub(crate) fn openai_response_text(value: &Value) -> Option<String> {
    let items = value.get("output")?.as_array()?;
    let mut text = String::new();
    for item in items {
        if item.get("type").and_then(Value::as_str) != Some("message")
            || item.get("role").and_then(Value::as_str) != Some("assistant")
        {
            continue;
        }
        if let Some(content) = item.get("content").and_then(Value::as_array) {
            for part in content {
                if part
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|kind| kind == "output_text")
                    .unwrap_or(false)
                {
                    if let Some(piece) = part.get("text").and_then(Value::as_str) {
                        text.push_str(piece);
                    }
                }
            }
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn anthropic_response_text(value: &Value) -> Option<String> {
    let parts = value.get("content")?.as_array()?;
    let mut text = String::new();
    for part in parts {
        if part
            .get("type")
            .and_then(Value::as_str)
            .map(|kind| kind == "text")
            .unwrap_or(false)
        {
            if let Some(piece) = part.get("text").and_then(Value::as_str) {
                text.push_str(piece);
            }
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn gemini_response_text(value: &Value) -> Option<String> {
    let candidates = value.get("candidates")?.as_array()?;
    let mut text = String::new();
    if let Some(candidate) = candidates.first() {
        if let Some(parts) = candidate
            .pointer("/content/parts")
            .and_then(Value::as_array)
        {
            for part in parts {
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    continue;
                }
                if let Some(piece) = part.get("text").and_then(Value::as_str) {
                    text.push_str(piece);
                }
            }
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn gemini_usage_tokens(value: &Value) -> (Option<u64>, Option<u64>) {
    let input = value
        .pointer("/usageMetadata/promptTokenCount")
        .and_then(Value::as_u64);
    let output = value
        .pointer("/usageMetadata/candidatesTokenCount")
        .or_else(|| value.pointer("/usageMetadata/outputTokenCount"))
        .and_then(Value::as_u64)
        .map(|response_tokens| {
            response_tokens.saturating_add(
                value
                    .pointer("/usageMetadata/thoughtsTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            )
        });
    (input, output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_system_prompt_with_cache_control_marks_stable_system_block() {
        let content = anthropic_system_prompt_with_cache_control("stable system");

        assert_eq!(
            content
                .pointer("/0/cache_control/type")
                .and_then(Value::as_str),
            Some("ephemeral")
        );
        assert_eq!(
            content.pointer("/0/text").and_then(Value::as_str),
            Some("stable system")
        );
    }

    #[test]
    fn provider_cache_artifact_lines_include_policy_metadata() {
        let cache = ProviderCacheTelemetry {
            provider_mode: "prompt_cache_key".to_string(),
            cache_key_hash: Some("abc123".to_string()),
            cache_control_status: Some("prompt_cache_key_24h".to_string()),
            cache_retention: Some("24h".to_string()),
            cached_input_tokens: Some(1024),
            cache_hit_tokens: Some(1024),
            cache_miss_tokens: Some(0),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
        };

        let lines = provider_cache_artifact_lines(Some(&cache));

        assert!(lines.contains("Cache provider mode: `prompt_cache_key`"));
        assert!(lines.contains("Cache key hash: `abc123`"));
        assert!(lines.contains("Cache control status: `prompt_cache_key_24h`"));
        assert!(lines.contains("Cache retention: `24h`"));
    }

    #[test]
    fn choose_preferred_model_uses_known_candidates_or_known_default() {
        let models = vec![
            "gpt-image-2".to_string(),
            "gpt-audio".to_string(),
            "gpt-5.5".to_string(),
            "gpt-5.6-sol".to_string(),
        ];

        assert_eq!(
            choose_preferred_model(&models, &["gpt-5.6-sol", "gpt-5.5"], "fallback"),
            "gpt-5.6-sol"
        );
        assert_eq!(
            choose_preferred_model(&models, &["gpt-5.6"], "fallback"),
            "fallback"
        );
        assert_eq!(
            choose_preferred_model(&[], &["gpt-5.6-sol"], "fallback"),
            "fallback"
        );
    }

    #[test]
    fn editorial_catalog_pickers_reject_arbitrary_modalities_and_prefer_known_strength() {
        let mut openai = json!({ "data": [
            { "id": "gpt-image-2" }, { "id": "gpt-realtime" },
            { "id": "unknown-new-model" }
        ] });
        assert_eq!(openai_catalog_model(&openai), "gpt-6-astra");
        openai["data"]
            .as_array_mut()
            .unwrap()
            .extend([json!({ "id": "gpt-4.1" }), json!({ "id": "gpt-6-astra" })]);
        assert_eq!(openai_catalog_model(&openai), "gpt-6-astra");
        openai["data"].as_array_mut().unwrap().pop();
        assert_eq!(openai_catalog_model(&openai), "gpt-4.1");

        let mut anthropic = api_model_ids(&json!({ "data": [
            { "id": "unknown-audio-only" }, { "id": "unknown-image-only" }
        ] }));
        assert_eq!(anthropic_catalog_model(&anthropic), "claude-fable-5-1");
        anthropic.extend(["claude-opus-5".into(), "claude-fable-5-1".into()]);
        assert_eq!(anthropic_catalog_model(&anthropic), "claude-fable-5-1");

        let mut gemini = gemini_model_ids(&json!({ "models": [
            { "name": "models/gemini-image-only", "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-audio-only", "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-3.1-pro-preview", "supportedGenerationMethods": ["embedContent"] }
        ] }));
        assert_eq!(gemini.len(), 2);
        assert_eq!(gemini_catalog_model(&gemini), "gemini-3.1-pro-preview");
        gemini.extend(["gemini-2.5-pro".into(), "gemini-3.1-pro-preview".into()]);
        assert_eq!(gemini_catalog_model(&gemini), "gemini-3.1-pro-preview");
    }

    fn catalog_test_server(
        pages: Vec<(u16, Value)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::Read;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/models", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, page) in pages {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "expected catalog request");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("catalog test server failed: {error}"),
                    }
                };
                // Winsock accepted sockets inherit the listener's nonblocking
                // mode. Header reads must use the bounded blocking timeout.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let length = stream.read(&mut buffer).unwrap();
                    assert!(length > 0);
                    request.extend_from_slice(&buffer[..length]);
                }
                requests.push(String::from_utf8(request).unwrap());
                let body = serde_json::to_string(&page).unwrap();
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (endpoint, server)
    }

    fn catalog_request_query(request: &str) -> Vec<(String, String)> {
        let target = request.split_whitespace().nth(1).unwrap();
        reqwest::Url::parse(&format!("http://localhost{target}"))
            .unwrap()
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect()
    }

    #[tokio::test]
    async fn anthropic_catalog_follows_native_cursor_and_prefers_later_flagship() {
        let (endpoint, server) = catalog_test_server(vec![
            (
                200,
                json!({ "data": [{ "id": "claude-opus-5" }], "has_more": true, "last_id": "claude-opus-5" }),
            ),
            (
                200,
                json!({ "data": [{ "id": "claude-fable-5-1" }], "has_more": false, "last_id": "claude-fable-5-1" }),
            ),
        ]);
        let client = build_api_client_async(None).unwrap();
        let models = fetch_anthropic_model_ids(
            client
                .get(endpoint)
                .header("x-api-key", "test-key")
                .header("anthropic-version", "2023-06-01"),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(anthropic_catalog_model(&models), "claude-fable-5-1");
        let requests = server.join().unwrap();
        assert_eq!(
            catalog_request_query(&requests[0]),
            vec![("limit".into(), "1000".into())]
        );
        assert_eq!(
            catalog_request_query(&requests[1]),
            vec![
                ("limit".into(), "1000".into()),
                ("after_id".into(), "claude-opus-5".into())
            ]
        );
        assert!(requests
            .iter()
            .all(|request| request.contains("x-api-key: test-key\r\n")
                && request.contains("anthropic-version: 2023-06-01\r\n")));
    }

    #[tokio::test]
    async fn gemini_catalog_follows_native_opaque_token_and_prefers_later_pro() {
        let (endpoint, server) = catalog_test_server(vec![
            (
                200,
                json!({ "models": [
                { "name": "models/gemini-audio-only", "supportedGenerationMethods": ["generateContent"] },
                { "name": "models/gemini-2.5-pro", "supportedGenerationMethods": ["generateContent"] }
            ], "nextPageToken": "opaque+/=" }),
            ),
            (
                200,
                json!({ "models": [{ "name": "models/gemini-3.1-pro-preview", "supportedGenerationMethods": ["generateContent"] }] }),
            ),
        ]);
        let client = build_api_client_async(None).unwrap();
        let models = fetch_gemini_model_ids(
            client.get(endpoint).header("x-goog-api-key", "test-key"),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(gemini_catalog_model(&models), "gemini-3.1-pro-preview");
        let requests = server.join().unwrap();
        assert_eq!(
            catalog_request_query(&requests[0]),
            vec![("pageSize".into(), "1000".into())]
        );
        assert_eq!(
            catalog_request_query(&requests[1]),
            vec![
                ("pageSize".into(), "1000".into()),
                ("pageToken".into(), "opaque+/=".into())
            ]
        );
        assert!(requests
            .iter()
            .all(|request| request.contains("x-goog-api-key: test-key\r\n")));
    }

    #[tokio::test]
    async fn incomplete_or_repeating_catalog_pagination_discards_partial_choices() {
        let client = build_api_client_async(None).unwrap();
        let cancel = CancellationToken::new();
        let anthropic_page = json!({ "data": [{ "id": "claude-opus-5" }], "has_more": true, "last_id": "same-cursor" });
        let (endpoint, server) =
            catalog_test_server(vec![(200, anthropic_page.clone()), (200, anthropic_page)]);
        assert_eq!(
            fetch_anthropic_model_ids(client.get(endpoint), &cancel).await,
            None
        );
        assert_eq!(server.join().unwrap().len(), 2);

        let gemini_page = json!({ "models": [{ "name": "models/gemini-2.5-pro" }], "nextPageToken": "same-token" });
        let (endpoint, server) =
            catalog_test_server(vec![(200, gemini_page.clone()), (200, gemini_page)]);
        assert_eq!(
            fetch_gemini_model_ids(client.get(endpoint), &cancel).await,
            None
        );
        assert_eq!(server.join().unwrap().len(), 2);

        let (endpoint, server) = catalog_test_server(vec![
            (
                200,
                json!({ "models": [{ "name": "models/gemini-2.5-pro" }], "nextPageToken": "next" }),
            ),
            (503, json!({ "error": "unavailable" })),
        ]);
        assert_eq!(
            fetch_gemini_model_ids(client.get(endpoint), &cancel).await,
            None
        );
        assert_eq!(server.join().unwrap().len(), 2);

        cancel.cancel();
        assert_eq!(
            fetch_anthropic_model_ids(client.get("http://127.0.0.1:1/models"), &cancel).await,
            None
        );
        assert_eq!(
            fetch_gemini_model_ids(client.get("http://127.0.0.1:1/models"), &cancel).await,
            None
        );
    }

    #[test]
    fn responses_reject_partial_ready_votes_and_tool_or_user_text() {
        let mut value = json!({ "status": "completed", "output": [{
            "type": "message", "role": "assistant", "status": "completed",
            "content": [{ "type": "output_text", "text": "MAESTRO_STATUS: READY\nComplete." }]
        }] });
        assert!(completed_provider_response_text("openai", &value)
            .unwrap()
            .starts_with("MAESTRO_STATUS: READY"));
        for status in ["incomplete", "failed", "in_progress", "cancelled"] {
            value["status"] = json!(status);
            assert_eq!(
                completed_provider_response_text("openai", &value),
                Err("PROVIDER_INCOMPLETE_RESPONSE")
            );
            assert_eq!(
                completed_provider_response_text("grok", &value),
                Err("PROVIDER_INCOMPLETE_RESPONSE")
            );
        }
        value["status"] = json!("completed");
        value["output"][0]["status"] = json!("incomplete");
        assert_eq!(
            completed_provider_response_text("openai", &value),
            Err("PROVIDER_INCOMPLETE_RESPONSE")
        );
        value["output"][0]["status"] = json!("completed");
        value["output"][0]["role"] = json!("user");
        assert_eq!(
            completed_provider_response_text("openai", &value),
            Err("PROVIDER_EMPTY_CONTENT")
        );
        value["output"][0]["role"] = json!("assistant");
        value["output"][0]["type"] = json!("function_call");
        assert_eq!(
            completed_provider_response_text("openai", &value),
            Err("PROVIDER_EMPTY_CONTENT")
        );
        assert!(completed_provider_response_text("openai", &json!({})).is_err());
    }

    #[test]
    fn anthropic_rejects_truncated_and_tool_only_editorial_outputs() {
        let mut value = json!({ "stop_reason": "end_turn", "content": [
            { "type": "thinking", "thinking": "private" },
            { "type": "text", "text": "MAESTRO_STATUS: READY" }
        ] });
        assert_eq!(
            completed_provider_response_text("anthropic", &value).unwrap(),
            "MAESTRO_STATUS: READY"
        );
        for reason in ["max_tokens", "tool_use", "pause_turn", "refusal"] {
            value["stop_reason"] = json!(reason);
            assert_eq!(
                completed_provider_response_text("anthropic", &value),
                Err("PROVIDER_INCOMPLETE_RESPONSE")
            );
        }
        value["stop_reason"] = json!("end_turn");
        value["content"] = json!([{ "type": "thinking", "thinking": "private" }]);
        assert_eq!(
            completed_provider_response_text("anthropic", &value),
            Err("PROVIDER_EMPTY_CONTENT")
        );
    }

    #[test]
    fn gemini_uses_completed_first_candidate_without_thoughts_and_bills_thinking() {
        let mut value = json!({ "candidates": [
            { "finishReason": "STOP", "content": { "parts": [
                { "thought": true, "text": "MAESTRO_STATUS: NOT_READY" },
                { "text": "MAESTRO_STATUS: READY" }
            ] } },
            { "finishReason": "STOP", "content": { "parts": [{ "text": "different alternative" }] } }
        ], "usageMetadata": { "promptTokenCount": 10, "candidatesTokenCount": 20, "thoughtsTokenCount": 30 } });
        assert_eq!(
            completed_provider_response_text("gemini", &value).unwrap(),
            "MAESTRO_STATUS: READY"
        );
        assert_eq!(gemini_usage_tokens(&value), (Some(10), Some(50)));
        value["candidates"][0]["finishReason"] = json!("MAX_TOKENS");
        assert_eq!(
            completed_provider_response_text("gemini", &value),
            Err("PROVIDER_INCOMPLETE_RESPONSE")
        );
        value["candidates"][0]["finishReason"] = json!("STOP");
        value["promptFeedback"] = json!({ "blockReason": "SAFETY" });
        assert_eq!(
            completed_provider_response_text("gemini", &value),
            Err("PROVIDER_INCOMPLETE_RESPONSE")
        );
    }

    #[tokio::test]
    async fn cancelled_model_lookup_never_blocks_the_async_runtime() {
        let client = build_api_client_async(None).unwrap();
        let token = CancellationToken::new();
        token.cancel();
        // This request must not reach the network, even on an invalid endpoint.
        let result =
            fetch_provider_model_catalog(client.get("http://127.0.0.1:1/models"), &token).await;
        assert_eq!(result, None);
    }
}
