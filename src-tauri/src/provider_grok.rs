// Modulo: src-tauri/src/provider_grok.rs
// Descricao: Grok / xAI API peer runner for Maestro Editorial AI.
//
// Grok is API-only in maestro-app. It deliberately follows the DeepSeek path
// rather than introducing a fake CLI layer: local CLI mode disables Grok, while
// hybrid/API modes call xAI's OpenAI-compatible HTTPS API directly.

use std::time::Instant;

use reqwest::Client;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::api_payloads::{
    api_attachment_delivery_note, grok_api_input, validate_native_provider_payload,
};
use crate::provider_retry::{
    build_api_client_async, provider_http_error_status, provider_reqwest_error_status,
    send_with_retry_async, ProviderRequestOutcome,
};
use crate::provider_runners::{
    api_cost_preflight_result, completed_provider_response_text, editorial_api_system_prompt,
    fetch_provider_model_catalog, log_provider_api_started, log_provider_cache_configured,
    write_provider_error_result, write_provider_error_result_with_accounting,
    write_provider_failure_result, write_provider_missing_key_result,
    write_provider_success_result, EditorialAgentRequest, ProviderInvocation,
};
use crate::session_controls::{
    api_role_max_tokens, provider_cache_plan, provider_cache_telemetry,
    provider_cache_telemetry_with_plan, provider_cost, usage_tokens,
};
use crate::{
    api_error_message, api_input_estimate_chars, first_env_value, provider_key_for_agent,
    provider_remote_present, sanitize_short,
};

const GROK_ENDPOINT: &str = "https://api.x.ai/v1/responses";
const GROK_MODELS_ENDPOINT: &str = "https://api.x.ai/v1/models";

pub(crate) async fn run_grok_api_agent(
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
    let prompt = format!(
        "{prompt}{}",
        api_attachment_delivery_note("grok", attachments)
    );
    let started = Instant::now();
    let name = "Grok";
    let cli = "grok-api";
    let provider = "grok";
    let model_hint = grok_model();
    let invocation = ProviderInvocation {
        log_session,
        run_id,
        name,
        cli,
        provider,
        role,
        output_path,
    };

    let Some((api_key, key_source)) = provider_key_for_agent(config, "grok") else {
        return write_provider_missing_key_result(
            &invocation,
            &model_hint,
            provider_remote_present(config, "grok"),
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
                &model_hint,
                &status,
                started.elapsed().as_millis(),
            );
        }
    };
    let model = resolve_grok_model(&async_client, &api_key, cancel_token).await;
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

    let input = match grok_api_input(&system_prompt, &prompt, attachments) {
        Ok(input) => input,
        Err(_) => {
            return write_provider_error_result(
                &invocation,
                &model,
                "ATTACHMENT_ERROR",
                started.elapsed().as_millis(),
            )
        }
    };
    let mut body = json!({
        "model": model,
        "input": input,
        "store": false,
        "max_output_tokens": max_tokens,
        "prompt_cache_key": cache_plan.cache_key
    });
    if model.starts_with("grok-4.7") || model.starts_with("grok-4.6") {
        body["reasoning"] = json!({ "effort": "xhigh" });
    } else if model.starts_with("grok-4.5") {
        body["reasoning"] = json!({ "effort": "high" });
    }
    if let Err(error) = validate_native_provider_payload(provider, &body) {
        return write_provider_failure_result(
            &invocation,
            &model,
            "ATTACHMENT_ERROR",
            "error",
            &error,
            started.elapsed().as_millis(),
            None,
        );
    }
    let request_builder = async_client
        .post(GROK_ENDPOINT)
        .bearer_auth(&api_key)
        .json(&body);
    let response =
        match send_with_retry_async(log_session, run_id, "grok", cancel_token, request_builder)
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
    let reported_cost = parsed
        .pointer("/usage/cost_in_usd_ticks")
        .and_then(Value::as_u64)
        .map(|ticks| ticks as f64 / 10_000_000_000.0);
    let cost_usd = reported_cost.or_else(|| {
        cost_guard.as_ref().and_then(|guard| {
            usage_input_tokens
                .zip(usage_output_tokens)
                .map(|(input, output)| provider_cost(input, output, guard.rates))
        })
    });
    let cost_estimated = cost_usd.map(|_| reported_cost.is_none());
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
                cost_estimated,
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
        cost_estimated,
        cache,
        started.elapsed().as_millis(),
        prompt.chars().count(),
        GROK_ENDPOINT,
    )
}

pub(crate) fn grok_model() -> String {
    first_env_value(&[
        "MAESTRO_GROK_MODEL",
        "CROSS_REVIEW_GROK_MODEL",
        "GROK_MODEL",
        "XAI_MODEL",
    ])
    .map(|(_, _, value)| sanitize_short(&value, 120))
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| "grok-4.7".to_string())
}

pub(crate) async fn resolve_grok_model(
    client: &Client,
    api_key: &str,
    cancel_token: &CancellationToken,
) -> String {
    if let Some((_, _, value)) = first_env_value(&[
        "MAESTRO_GROK_MODEL",
        "CROSS_REVIEW_GROK_MODEL",
        "GROK_MODEL",
        "XAI_MODEL",
    ]) {
        let model = sanitize_short(&value, 120);
        if !model.is_empty() {
            return model;
        }
    }

    let request = client.get(GROK_MODELS_ENDPOINT).bearer_auth(api_key);
    if let Some(value) = fetch_provider_model_catalog(request, cancel_token).await {
        return grok_catalog_model(&value);
    }

    "grok-4.7".to_string()
}

fn grok_catalog_model(value: &Value) -> String {
    crate::provider_runners::choose_preferred_model(
        &grok_model_ids(value),
        &[
            "grok-4.7",
            "grok-4.6",
            "grok-4.5",
            "grok-4.20-multi-agent-0309",
            "grok-4.20-0309-reasoning",
            "grok-4.3",
        ],
        "grok-4.7",
    )
}

pub(crate) fn grok_model_ids(value: &Value) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_model_ids_extract_openai_compatible_shape() {
        let value = json!({
            "object": "list",
            "data": [
                { "id": "grok-4.3", "object": "model" },
                { "id": "grok-4.5", "object": "model" }
            ]
        });

        assert_eq!(
            grok_model_ids(&value),
            vec!["grok-4.3".to_string(), "grok-4.5".to_string()]
        );
    }

    #[test]
    fn grok_catalog_prefers_known_flagship_and_never_image_or_audio_models() {
        let mut catalog = json!({ "data": [
            { "id": "grok-imagine-image" }, { "id": "grok-audio-only" },
            { "id": "unknown-new-model" }
        ] });
        assert_eq!(grok_catalog_model(&catalog), "grok-4.7");
        catalog["data"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "id": "grok-4.6" }));
        assert_eq!(grok_catalog_model(&catalog), "grok-4.6");
        catalog["data"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "id": "grok-4.7" }));
        assert_eq!(grok_catalog_model(&catalog), "grok-4.7");
        assert_eq!(grok_catalog_model(&json!({})), "grok-4.7");
    }

    #[test]
    fn grok_response_text_extracts_responses_content() {
        let value = json!({
            "status": "completed",
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "MAESTRO_STATUS: READY\nRevisao aprovada."
                        }
                    ]
                }
            ]
        });

        assert_eq!(
            completed_provider_response_text("grok", &value).unwrap(),
            "MAESTRO_STATUS: READY\nRevisao aprovada."
        );
    }
}
