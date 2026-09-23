// Modulo: src-tauri/src/provider_perplexity.rs
// Descricao: Perplexity Agent API peer runner for Maestro Editorial AI.
//
// Perplexity is API-only in maestro-app. Web search is an explicit Agent API
// tool, and assistant text is carried by typed output items.

use std::time::Instant;

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::logging::{write_log_record, LogEventInput, LogSession};
use crate::provider_retry::{
    build_api_client_async, provider_http_error_status, provider_reqwest_error_status,
    send_with_retry_async, ProviderRequestOutcome,
};
use crate::provider_runners::{
    api_cost_preflight_result, editorial_api_system_prompt, log_provider_api_started,
    log_provider_cache_configured, write_provider_error_result,
    write_provider_error_result_with_accounting, write_provider_failure_result,
    write_provider_missing_key_result, write_provider_success_result, EditorialAgentRequest,
    ProviderInvocation,
};
use crate::session_controls::{
    api_role_max_tokens, estimate_provider_cost_from_input_chars, provider_cache_plan,
    provider_cache_telemetry_with_plan, provider_cost, usage_tokens,
};
use crate::{
    api_error_message, api_input_estimate_chars, first_env_value, provider_key_for_agent,
    provider_remote_present, sanitize_short, sanitize_text,
};

const PERPLEXITY_ENDPOINT: &str = "https://api.perplexity.ai/v1/agent";
pub(crate) const PERPLEXITY_WEB_SEARCH_COST_USD: f64 = 0.0025;

fn is_agent_model(model: &str) -> bool {
    if model.len() > 120 {
        return false;
    }
    let Some((provider, name)) = model.split_once('/') else {
        return false;
    };
    !provider.is_empty()
        && !name.is_empty()
        && provider.chars().all(|ch| ch.is_ascii_lowercase() || ch == '-')
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_'))
}

fn perplexity_agent_request_body(
    model: &str,
    system_prompt: &str,
    prompt: &str,
    max_output_tokens: u64,
) -> Value {
    json!({
        "model": model,
        "instructions": system_prompt,
        "input": prompt,
        "max_output_tokens": max_output_tokens,
        "reasoning": { "effort": "high" },
        "tools": [{ "type": "web_search" }],
        "max_steps": 1,
        "stream": false,
        "store": false
    })
}

pub(crate) async fn run_perplexity_api_agent(
    request: EditorialAgentRequest<'_>,
    cancel_token: &CancellationToken,
) -> crate::EditorialAgentResult {
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
    let name = "Perplexity";
    let cli = "perplexity-api";
    let provider = "perplexity";
    let model = perplexity_model();
    let invocation = ProviderInvocation {
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        output_path,
    };

    if !is_agent_model(&model) {
        return write_provider_failure_result(
            &invocation,
            &model,
            "PERPLEXITY_AGENT_MODEL_REQUIRED",
            "blocked",
            "Configure um modelo Agent API no formato provider/model; IDs Sonar legados não são aceitos.",
            started.elapsed().as_millis(),
            None,
        );
    }

    let Some((api_key, key_source)) = provider_key_for_agent(config, "perplexity") else {
        return write_provider_missing_key_result(
            &invocation,
            &model,
            provider_remote_present(config, "perplexity"),
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
    if let Some(guard) = cost_guard.as_ref() {
        if let Some(limit) = guard.max_session_cost_usd {
            let projected =
                estimate_provider_cost_from_input_chars(input_estimate_chars, max_tokens, guard.rates)
                    + PERPLEXITY_WEB_SEARCH_COST_USD;
            if guard.observed_cost_usd + projected > limit {
                return write_provider_failure_result(
                    &invocation,
                    &model,
                    "COST_LIMIT_REACHED",
                    "blocked",
                    "Perplexity nao foi chamado: custo projetado com busca excede o limite da sessao.",
                    started.elapsed().as_millis(),
                    Some(projected),
                );
            }
        }
    }

    let async_client = match build_api_client_async(timeout) {
        Ok(client) => client,
        Err(error) => {
            let status = provider_reqwest_error_status("CLIENT_ERROR", error);
            return write_provider_error_result(
                &invocation,
                &model,
                &status,
                started.elapsed().as_millis(),
            );
        }
    };
    let system_prompt = editorial_api_system_prompt(name);
    let cache_plan = provider_cache_plan(provider, &model, role, name, &system_prompt);
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

    let body = perplexity_agent_request_body(&model, &system_prompt, &prompt, max_tokens);
    let request_builder = async_client
        .post(PERPLEXITY_ENDPOINT)
        .bearer_auth(&api_key)
        .json(&body);
    let response = match send_with_retry_async(
        log_session,
        run_id,
        "perplexity",
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
    let reported_cost_usd = parsed
        .pointer("/usage/cost/total_cost")
        .and_then(Value::as_f64)
        .filter(|cost| cost.is_finite() && *cost >= 0.0);
    if parsed.get("status").and_then(Value::as_str) != Some("completed") {
        return write_provider_error_result_with_accounting(
            &invocation,
            &model,
            "PROVIDER_INCOMPLETE_RESPONSE",
            started.elapsed().as_millis(),
            usage_input_tokens,
            usage_output_tokens,
            reported_cost_usd,
        );
    }
    let stdout = perplexity_response_text(&parsed)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_default();
    if stdout.trim().is_empty() {
        return write_provider_error_result_with_accounting(
            &invocation,
            &model,
            "PROVIDER_EMPTY_CONTENT",
            started.elapsed().as_millis(),
            usage_input_tokens,
            usage_output_tokens,
            reported_cost_usd,
        );
    }
    if !perplexity_has_search_evidence(&parsed) {
        return write_provider_error_result_with_accounting(
            &invocation,
            &model,
            "PROVIDER_UNGROUNDED_RESPONSE",
            started.elapsed().as_millis(),
            usage_input_tokens,
            usage_output_tokens,
            reported_cost_usd,
        );
    }
    log_perplexity_sources(log_session, run_id, &parsed, output_path);
    let cache = Some(provider_cache_telemetry_with_plan(&cache_plan, None));
    let cost_usd = reported_cost_usd.or_else(|| {
        cost_guard.as_ref().and_then(|guard| {
            usage_input_tokens.zip(usage_output_tokens).map(|(input, output)| {
                provider_cost(input, output, guard.rates) + PERPLEXITY_WEB_SEARCH_COST_USD
            })
        })
    });
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
        cost_usd.map(|_| reported_cost_usd.is_none()),
        cache,
        started.elapsed().as_millis(),
        prompt.chars().count(),
        PERPLEXITY_ENDPOINT,
    )
}

pub(crate) fn perplexity_model() -> String {
    first_env_value(&["MAESTRO_PERPLEXITY_MODEL", "PERPLEXITY_MODEL"])
        .map(|(_, _, value)| normalize_agent_model_override(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "perplexity/kimi-k3".to_string())
}

fn normalize_agent_model_override(value: &str) -> String {
    value.trim().to_string()
}

fn perplexity_has_search_evidence(value: &Value) -> bool {
    value
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some("search_results")
                    && item
                        .get("results")
                        .and_then(Value::as_array)
                        .is_some_and(|results| !results.is_empty())
            })
        })
}

pub(crate) fn perplexity_response_text(value: &Value) -> Option<String> {
    if value.get("status").and_then(Value::as_str) != Some("completed") {
        return None;
    }
    let parts = value.get("output")?.as_array()?.iter().filter_map(|item| {
        if item.get("type").and_then(Value::as_str) != Some("message")
            || item.get("role").and_then(Value::as_str) != Some("assistant")
        {
            return None;
        }
        item.get("content")?.as_array().map(|content| {
            content.iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
    }).collect::<Vec<_>>();
    let text = parts.join("\n");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn log_perplexity_sources(
    log_session: &LogSession,
    run_id: &str,
    parsed: &Value,
    output_path: &std::path::Path,
) {
    let citations = parsed
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter_map(|part| part.get("annotations").and_then(Value::as_array))
        .flatten()
        .filter_map(|annotation| annotation.get("url").and_then(Value::as_str))
        .map(|url| sanitize_text(url, 300))
        .filter(|url| !url.is_empty())
        .take(12)
        .collect::<Vec<_>>();
    let search_results = parsed
        .get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("search_results"))
                .filter_map(|item| item.get("results").and_then(Value::as_array))
                .flatten()
                .filter_map(|item| {
                    let title = item.get("title").and_then(Value::as_str).unwrap_or("");
                    let url = item.get("url").and_then(Value::as_str).unwrap_or("");
                    if title.trim().is_empty() && url.trim().is_empty() {
                        return None;
                    }
                    Some(json!({
                        "title": sanitize_text(title, 180),
                        "url": sanitize_text(url, 300),
                    }))
                })
                .take(12)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if citations.is_empty() && search_results.is_empty() {
        return;
    }
    let _ = write_log_record(
        log_session,
        LogEventInput {
            level: "info".to_string(),
            category: "session.provider.perplexity.sources".to_string(),
            message: "perplexity returned source metadata".to_string(),
            context: Some(json!({
                "run_id": sanitize_short(run_id, 120),
                "provider": "perplexity",
                "citations": citations,
                "search_results": search_results,
                "output_path": output_path.to_string_lossy().to_string(),
            })),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_request_uses_documented_fields_and_web_search() {
        let body = perplexity_agent_request_body("perplexity/kimi-k3", "system", "user", 100);
        assert_eq!(body["model"], "perplexity/kimi-k3");
        assert_eq!(body["instructions"], "system");
        assert_eq!(body["input"], "user");
        assert_eq!(body["max_output_tokens"], 100);
        assert_eq!(body["reasoning"]["effort"], "high");
        assert_eq!(body["tools"][0]["type"], "web_search");
        assert!(body.get("messages").is_none());
        assert!(body.get("max_tokens").is_none());
        assert!(!is_agent_model("sonar-reasoning-pro"));
        assert_eq!(
            normalize_agent_model_override(" perplexity/kimi-k3 "),
            "perplexity/kimi-k3"
        );
        assert!(is_agent_model(&normalize_agent_model_override(
            " perplexity/kimi-k3 "
        )));
    }

    #[test]
    fn perplexity_response_text_extracts_only_completed_assistant_output() {
        let value = json!({
            "status": "completed",
            "output": [
                { "type": "search_results", "results": [{"title":"Source","url":"https://example.org"}] },
                { "type": "message", "role": "assistant", "content": [
                    { "type": "output_text", "text": "MAESTRO_STATUS: READY\nReview approved." }
                ] }
            ]
        });

        assert_eq!(
            perplexity_response_text(&value).unwrap(),
            "MAESTRO_STATUS: READY\nReview approved."
        );
        assert!(perplexity_response_text(&json!({"status":"failed","output":value["output"]})).is_none());
        assert!(perplexity_response_text(&json!({"status":"completed","output":[{"type":"search_results","results":[]}]})).is_none());
        assert!(perplexity_has_search_evidence(&value));
        assert!(!perplexity_has_search_evidence(&json!({
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type":"output_text","text":"answer"}]
            }]
        })));
    }
}
