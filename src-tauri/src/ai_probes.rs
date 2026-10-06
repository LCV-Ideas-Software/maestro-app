// Modulo: src-tauri/src/ai_probes.rs
// Descricao: Six-provider credential checks using authenticated native
// metadata endpoints. These checks never request editorial generation and
// distinguish authentication failure from access, credit and rate limits.

use std::time::Duration;

use chrono::Utc;
use reqwest::blocking::Client;

use crate::{
    api_error_message, effective_provider_key, sanitize_short, sanitize_text, AiProviderConfig,
    AiProviderProbeResult, AiProviderProbeRow,
};

#[derive(Clone, Copy)]
enum AiProbeEndpoint {
    ModelCatalog,
    PerplexitySkills,
}

pub(crate) fn run_ai_provider_probe(config: &AiProviderConfig) -> AiProviderProbeResult {
    let client = match Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!(
            "Maestro Editorial AI/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return AiProviderProbeResult {
                rows: vec![ai_probe_row(
                    "APIs",
                    format!("cliente HTTP falhou: {}", error.without_url()),
                    "error",
                )],
                checked_at: Utc::now().to_rfc3339(),
            };
        }
    };

    AiProviderProbeResult {
        rows: vec![
            probe_openai_api(&client, config),
            probe_anthropic_api(&client, config),
            probe_gemini_api(&client, config),
            probe_deepseek_api(&client, config),
            probe_grok_api(&client, config),
            probe_perplexity_api(&client, config),
        ],
        checked_at: Utc::now().to_rfc3339(),
    }
}

fn probe_openai_api(client: &Client, config: &AiProviderConfig) -> AiProviderProbeRow {
    let Some((key, _source)) = effective_provider_key(
        config.openai_api_key.as_deref(),
        &["MAESTRO_OPENAI_API_KEY", "OPENAI_API_KEY"],
    ) else {
        return missing_provider_key_row("OpenAI / Codex", config.openai_api_key_remote);
    };

    let response = client
        .get("https://api.openai.com/v1/models")
        .bearer_auth(&key)
        .send();
    summarize_ai_probe_response("OpenAI / Codex", response, AiProbeEndpoint::ModelCatalog)
}

fn probe_anthropic_api(client: &Client, config: &AiProviderConfig) -> AiProviderProbeRow {
    let Some((key, _source)) = effective_provider_key(
        config.anthropic_api_key.as_deref(),
        &["MAESTRO_ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY"],
    ) else {
        return missing_provider_key_row("Anthropic / Claude", config.anthropic_api_key_remote);
    };

    let response = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .send();
    summarize_ai_probe_response(
        "Anthropic / Claude",
        response,
        AiProbeEndpoint::ModelCatalog,
    )
}

fn probe_gemini_api(client: &Client, config: &AiProviderConfig) -> AiProviderProbeRow {
    let Some((key, _source)) = effective_provider_key(
        config.gemini_api_key.as_deref(),
        &["MAESTRO_GEMINI_API_KEY", "GEMINI_API_KEY"],
    ) else {
        return missing_provider_key_row("Google / Gemini", config.gemini_api_key_remote);
    };

    let response = client
        .get("https://generativelanguage.googleapis.com/v1beta/models")
        .header("x-goog-api-key", &key)
        .send();
    summarize_ai_probe_response("Google / Gemini", response, AiProbeEndpoint::ModelCatalog)
}

fn probe_deepseek_api(client: &Client, config: &AiProviderConfig) -> AiProviderProbeRow {
    let Some((key, _source)) = effective_provider_key(
        config.deepseek_api_key.as_deref(),
        &["MAESTRO_DEEPSEEK_API_KEY", "DEEPSEEK_API_KEY"],
    ) else {
        return missing_provider_key_row("DeepSeek", config.deepseek_api_key_remote);
    };

    let response = client
        .get("https://api.deepseek.com/models")
        .bearer_auth(&key)
        .send();
    summarize_ai_probe_response("DeepSeek", response, AiProbeEndpoint::ModelCatalog)
}

fn probe_grok_api(client: &Client, config: &AiProviderConfig) -> AiProviderProbeRow {
    let Some((key, _source)) = effective_provider_key(
        config.grok_api_key.as_deref(),
        &["MAESTRO_GROK_API_KEY", "GROK_API_KEY", "XAI_API_KEY"],
    ) else {
        return missing_provider_key_row("Grok / xAI", config.grok_api_key_remote);
    };

    let response = client
        .get("https://api.x.ai/v1/models")
        .bearer_auth(&key)
        .send();
    summarize_ai_probe_response("Grok / xAI", response, AiProbeEndpoint::ModelCatalog)
}

fn probe_perplexity_api(client: &Client, config: &AiProviderConfig) -> AiProviderProbeRow {
    let Some((key, _source)) = effective_provider_key(
        config.perplexity_api_key.as_deref(),
        &["MAESTRO_PERPLEXITY_API_KEY", "PERPLEXITY_API_KEY"],
    ) else {
        return missing_provider_key_row(
            "Perplexity / Agent API",
            config.perplexity_api_key_remote,
        );
    };

    let response = client
        .get("https://api.perplexity.ai/v1/skills")
        .query(&[("limit", 1)])
        .bearer_auth(&key)
        .send();
    // The model catalog is public. Skills listing requires the project API
    // key and reads metadata without creating a skill or requesting inference.
    // https://docs.perplexity.ai/api-reference/skills-list-get
    summarize_ai_probe_response(
        "Perplexity / Agent API",
        response,
        AiProbeEndpoint::PerplexitySkills,
    )
}

fn missing_provider_key_row(label: &str, remote_present: bool) -> AiProviderProbeRow {
    if remote_present {
        ai_probe_row(
            label,
            "segredo no Cloudflare; valor nao pode ser lido de volta neste app local",
            "warn",
        )
    } else {
        ai_probe_row(label, "API key nao informada", "warn")
    }
}

fn summarize_ai_probe_response(
    label: &str,
    response: Result<reqwest::blocking::Response, reqwest::Error>,
    endpoint: AiProbeEndpoint,
) -> AiProviderProbeRow {
    match response {
        Ok(response) => {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            summarize_ai_probe_http_response(label, status, &body, endpoint)
        }
        Err(error) => {
            let safe_error = error.without_url();
            ai_probe_row(label, format!("falha de rede: {safe_error}"), "error")
        }
    }
}

fn summarize_ai_probe_http_response(
    label: &str,
    status: reqwest::StatusCode,
    body: &str,
    endpoint: AiProbeEndpoint,
) -> AiProviderProbeRow {
    if status.is_success() {
        return match endpoint {
            AiProbeEndpoint::ModelCatalog => {
                ai_probe_row(label, "API respondeu; credencial aceita", "ok")
            }
            AiProbeEndpoint::PerplexitySkills => {
                let valid_skills = serde_json::from_str::<serde_json::Value>(body)
                    .ok()
                    .is_some_and(|parsed| {
                        parsed.get("error").is_none()
                            && parsed.get("skills").is_some_and(|skills| skills.is_array())
                    });
                if valid_skills {
                    ai_probe_row(
                        label,
                        "credencial aceita pela Agent API; geracao nao testada",
                        "ok",
                    )
                } else {
                    ai_probe_row(
                        label,
                        "resposta de metadados inesperada; validacao da credencial inconclusiva",
                        "warn",
                    )
                }
            }
        };
    }

    let (reason, tone) = match status.as_u16() {
        401 => ("credencial recusada", "error"),
        402 => (
            "creditos indisponiveis; validacao da credencial inconclusiva",
            "warn",
        ),
        403 => (
            "acesso negado; validacao da credencial inconclusiva",
            "warn",
        ),
        429 => ("limite ativo; validacao da credencial inconclusiva", "warn"),
        _ => ("resposta inesperada", "warn"),
    };
    ai_probe_row(
        label,
        format!(
            "{reason} (HTTP {}): {}",
            status.as_u16(),
            api_error_message(body)
        ),
        tone,
    )
}

fn ai_probe_row(
    label: impl Into<String>,
    value: impl Into<String>,
    tone: impl Into<String>,
) -> AiProviderProbeRow {
    AiProviderProbeRow {
        label: sanitize_text(&label.into(), 80),
        value: sanitize_text(&value.into(), 240),
        tone: sanitize_short(&tone.into(), 16),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn perplexity_empty_project_proves_authenticated_metadata_without_generation() {
        let row = summarize_ai_probe_http_response(
            "Perplexity / Agent API",
            StatusCode::OK,
            r#"{"skills": []}"#,
            AiProbeEndpoint::PerplexitySkills,
        );
        assert_eq!(row.tone, "ok");
        assert_eq!(
            row.value,
            "credencial aceita pela Agent API; geracao nao testada"
        );
    }

    #[test]
    fn perplexity_does_not_accept_public_catalog_html_or_malformed_skill_responses() {
        for body in [
            r#"{"data": [{"id": "perplexity/kimi-k3"}]}"#,
            "<html>available</html>",
            "",
            r#"{"skills": null}"#,
            r#"{"skills": {}}"#,
            r#"{"skills": [], "error": {"message": "denied"}}"#,
        ] {
            let row = summarize_ai_probe_http_response(
                "Perplexity / Agent API",
                StatusCode::OK,
                body,
                AiProbeEndpoint::PerplexitySkills,
            );
            assert_eq!(row.tone, "warn", "Unexpected body: {body}");
            assert!(row.value.contains("inconclusiva"));
        }
    }

    #[test]
    fn credential_probe_distinguishes_authentication_from_access_and_quota() {
        for endpoint in [
            AiProbeEndpoint::ModelCatalog,
            AiProbeEndpoint::PerplexitySkills,
        ] {
            let unauthorized = summarize_ai_probe_http_response(
                "API",
                StatusCode::UNAUTHORIZED,
                r#"{"error": {"message": "invalid API key"}}"#,
                endpoint,
            );
            assert_eq!(unauthorized.tone, "error");
            assert!(unauthorized
                .value
                .contains("credencial recusada (HTTP 401)"));

            for (status, reason) in [
                (StatusCode::PAYMENT_REQUIRED, "creditos indisponiveis"),
                (StatusCode::FORBIDDEN, "acesso negado"),
                (StatusCode::TOO_MANY_REQUESTS, "limite ativo"),
            ] {
                let row = summarize_ai_probe_http_response(
                    "API",
                    status,
                    r#"{"error": {"message": "request denied"}}"#,
                    endpoint,
                );
                assert_eq!(row.tone, "warn");
                assert!(row.value.contains(reason));
                assert!(row.value.contains("inconclusiva"));
                assert!(!row.value.contains("credencial recusada"));
            }
        }
    }

    #[test]
    fn unavailable_native_metadata_does_not_reject_the_key() {
        let row = summarize_ai_probe_http_response(
            "Perplexity / Agent API",
            StatusCode::BAD_GATEWAY,
            r#"{"error": {"message": "Skill service unavailable"}}"#,
            AiProbeEndpoint::PerplexitySkills,
        );
        assert_eq!(row.tone, "warn");
        assert!(row.value.contains("HTTP 502"));
        assert!(!row.value.contains("credencial recusada"));
    }
}
