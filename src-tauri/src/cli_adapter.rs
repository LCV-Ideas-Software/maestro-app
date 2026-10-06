// Modulo: src-tauri/src/cli_adapter.rs
// Descricao: CLI adapter smoke probe spec table + per-CLI probe runner
// extracted from lib.rs in v0.3.39.
//
// This module owns the dependency_preflight smoke probe machinery that
// validates each external CLI (Claude, Codex, Gemini via Antigravity) is callable and
// returns the expected marker. The Tauri command wrapper
// (`run_cli_adapter_smoke`) stays in lib.rs because it lives on the
// `#[tauri::command]` registry boundary and orchestrates calls to these
// helpers across all 3 CLI adapters.
//
// What's here:
//   - `cli_adapter_specs` — builds the 3-element spec table (name, command,
//     marker, CLI args, per-CLI timeout). Args differ per CLI: Claude uses
//     `--print --output-format text --tools "" --disallowedTools mcp__*
//     --permission-mode dontAsk`, Codex uses
//     `--ask-for-approval never exec --skip-git-repo-check --sandbox
//     read-only --color never`, and Gemini uses Antigravity CLI (`agy`) in
//     print mode.
//   - `run_cli_adapter_probe` — single-spec runner: resolves the command
//     against the effective PATH (returns `blocked` tone with status "CLI
//     nao encontrada no PATH efetivo" when missing), invokes
//     `run_resolved_command_with_timeout`, then classifies the outcome
//     (timeout/ok+marker/ok-without-marker/nonzero-exit).
//
// What stayed in lib.rs:
//   - `CliAdapterSmokeRequest` / `CliAdapterSmokeResult` /
//     `CliAdapterProbeResult` / `CliAdapterSpec` structs (consumed by both
//     `cli_adapter.rs` and the Tauri command wrapper; live in lib.rs as
//     `pub(crate)` for cross-module access).
//   - `run_cli_adapter_smoke` Tauri command wrapper (registry boundary).
//
// v0.3.39 is a pure move: every signature, log line, format string and
// status string is identical to the v0.3.38 lib.rs source (commit b7509b9).

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::command_path::resolve_command;
use crate::command_spawn::{
    run_resolved_command_observed_piped, run_resolved_command_with_timeout,
};
use crate::{
    app_root, sanitize_short, sanitize_text, AiProviderConfig, CliAdapterProbeResult,
    CliAdapterSmokeRequest, CliAdapterSpec,
};

pub(crate) fn cli_adapter_specs(request: &CliAdapterSmokeRequest) -> Vec<CliAdapterSpec> {
    let run_id = sanitize_short(&request.run_id, 120);
    let protocol_name = sanitize_text(&request.protocol_name, 160);
    let protocol_hash_prefix = sanitize_short(&request.protocol_hash, 16);
    let prompt_base = format!(
        "Maestro Editorial AI adapter smoke. Run {run_id}. Prompt chars: {}. Protocol: {protocol_name}; lines: {}; hash prefix: {protocol_hash_prefix}. Do not use tools. Reply only with the exact marker requested.",
        request.prompt_chars, request.protocol_lines
    );

    vec![
        CliAdapterSpec {
            name: "Claude",
            command: "claude",
            marker: "MAESTRO_CLI_SMOKE_CLAUDE_READY",
            args: vec![
                "--print".to_string(),
                "--output-format".to_string(),
                "text".to_string(),
                "--tools".to_string(),
                String::new(),
                "--disallowedTools".to_string(),
                "mcp__*".to_string(),
                "--strict-mcp-config".to_string(),
                "--permission-mode".to_string(),
                "dontAsk".to_string(),
                format!("{prompt_base} Marker: MAESTRO_CLI_SMOKE_CLAUDE_READY"),
            ],
            timeout: Duration::from_secs(90),
        },
        CliAdapterSpec {
            name: "Codex",
            command: "codex",
            marker: "MAESTRO_CLI_SMOKE_CODEX_READY",
            args: vec![
                "--ask-for-approval".to_string(),
                "never".to_string(),
                "exec".to_string(),
                "--skip-git-repo-check".to_string(),
                "--sandbox".to_string(),
                "read-only".to_string(),
                "--color".to_string(),
                "never".to_string(),
                format!("{prompt_base} Marker: MAESTRO_CLI_SMOKE_CODEX_READY"),
            ],
            timeout: Duration::from_secs(90),
        },
        CliAdapterSpec {
            name: "Gemini",
            command: "agy",
            marker: "MAESTRO_CLI_SMOKE_AGY_READY",
            args: vec![
                "--print".to_string(),
                format!("{prompt_base} Marker: MAESTRO_CLI_SMOKE_AGY_READY"),
                "--print-timeout".to_string(),
                "90s".to_string(),
            ],
            timeout: Duration::from_secs(90),
        },
    ]
}

pub(crate) fn run_cli_adapter_probe(
    mut spec: CliAdapterSpec,
    config: &AiProviderConfig,
) -> CliAdapterProbeResult {
    let started = Instant::now();
    let Some(path) = resolve_command(spec.command) else {
        return CliAdapterProbeResult {
            name: spec.name.to_string(),
            cli: spec.command.to_string(),
            tone: "blocked".to_string(),
            status: "CLI nao encontrada no PATH efetivo".to_string(),
            duration_ms: started.elapsed().as_millis(),
            exit_code: None,
            marker_found: false,
        };
    };

    if spec.command == "agy" {
        match verify_agy_cli_project_permissions(
            &path,
            config,
            &app_root(),
            Some(spec.timeout),
            None,
        ) {
            Ok(project_argument) => spec.args.push(project_argument),
            Err(status) => {
                return CliAdapterProbeResult {
                    name: spec.name.to_string(),
                    cli: spec.command.to_string(),
                    tone: "blocked".to_string(),
                    status,
                    duration_ms: started.elapsed().as_millis(),
                    exit_code: None,
                    marker_found: false,
                }
            }
        }
    }
    let remaining = spec.timeout.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return CliAdapterProbeResult {
            name: spec.name.to_string(),
            cli: spec.command.to_string(),
            tone: "blocked".to_string(),
            status: "prazo da CLI expirou durante a verificacao nativa".to_string(),
            duration_ms: started.elapsed().as_millis(),
            exit_code: None,
            marker_found: false,
        };
    }
    match run_resolved_command_with_timeout(&path, &spec.args, remaining, None) {
        Ok(result) => {
            let exit_code = result.output.status.code();
            let stdout = String::from_utf8_lossy(&result.output.stdout);
            let stderr = String::from_utf8_lossy(&result.output.stderr);
            let marker_found = stdout.contains(spec.marker) || stderr.contains(spec.marker);

            let (tone, status) = if result.timed_out {
                ("error", "timeout aguardando resposta da CLI")
            } else if result.output.status.success() && marker_found {
                ("ok", "CLI executada e marcador recebido")
            } else if result.output.status.success() {
                ("warn", "CLI executada, mas marcador esperado nao apareceu")
            } else {
                ("error", "CLI retornou codigo de saida diferente de zero")
            };

            CliAdapterProbeResult {
                name: spec.name.to_string(),
                cli: spec.command.to_string(),
                tone: tone.to_string(),
                status: status.to_string(),
                duration_ms: started.elapsed().as_millis(),
                exit_code,
                marker_found,
            }
        }
        Err(error) => CliAdapterProbeResult {
            name: spec.name.to_string(),
            cli: spec.command.to_string(),
            tone: "error".to_string(),
            status: sanitize_text(&format!("falha ao executar CLI: {error}"), 240),
            duration_ms: started.elapsed().as_millis(),
            exit_code: None,
            marker_found: false,
        },
    }
}

const AGY_PROJECT_PERMISSION_GUIDANCE: &str = "AGY CLI bloqueada: selecione um projeto nativo Antigravity com negativas de escrita, comandos, MCP e execute_url verificaveis por /permissions, ou selecione explicitamente a API.";
const AGY_PROJECT_DENIES: [&str; 4] = ["write_file(*)", "command(*)", "mcp(*)", "execute_url(*)"];

/// Read the installed CLI's effective project policy before every prompt.
/// /permissions is a native metadata command; zero model turns and zero usage
/// are mandatory. Vendor files are neither inferred nor rewritten here.
pub(crate) fn verify_agy_cli_project_permissions(
    path: &Path,
    config: &AiProviderConfig,
    working_dir: &Path,
    timeout: Option<Duration>,
    cancel_token: Option<&CancellationToken>,
) -> Result<String, String> {
    let timeout = timeout
        .unwrap_or(Duration::from_secs(30))
        .min(Duration::from_secs(30));
    admit_agy_cli_project(config, |args| {
        if timeout.is_zero() || cancel_token.is_some_and(CancellationToken::is_cancelled) {
            return Err(AGY_PROJECT_PERMISSION_GUIDANCE.to_string());
        }
        let result = run_resolved_command_observed_piped(
            path,
            args,
            Some(timeout),
            None,
            None,
            cancel_token,
            Some(working_dir),
        )
        .map_err(|_| AGY_PROJECT_PERMISSION_GUIDANCE.to_string())?;
        if result.timed_out
            || !result.output.status.success()
            || result.stdout_pipe_error.is_some()
            || result.stderr_pipe_error.is_some()
        {
            return Err(AGY_PROJECT_PERMISSION_GUIDANCE.to_string());
        }
        Ok(result.output.stdout)
    })
}

fn admit_agy_cli_project(
    config: &AiProviderConfig,
    read_native_policy: impl FnOnce(&[String]) -> Result<Vec<u8>, String>,
) -> Result<String, String> {
    let project = config
        .agy_cli_project_id
        .as_deref()
        .map(str::trim)
        .filter(|project| {
            !project.is_empty()
                && project.len() <= 128
                && project
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_'))
        })
        .ok_or_else(|| AGY_PROJECT_PERMISSION_GUIDANCE.to_string())?;
    let project_argument = format!("--project={project}");
    let args = vec![
        project_argument.clone(),
        "--print".to_string(),
        "/permissions".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
    ];
    let bytes = read_native_policy(&args)?;
    validate_agy_project_permissions(&bytes)
        .map_err(|_| AGY_PROJECT_PERMISSION_GUIDANCE.to_string())?;
    Ok(project_argument)
}

#[derive(Deserialize)]
struct NativePermissionsReadback {
    status: String,
    num_turns: u64,
    usage: NativePermissionsUsage,
    command: NativePermissionsCommand,
}

#[derive(Deserialize)]
struct NativePermissionsUsage {
    input_tokens: u64,
    output_tokens: u64,
    thinking_tokens: u64,
    cache_read_tokens: u64,
    total_tokens: u64,
}

#[derive(Deserialize)]
struct NativePermissionsCommand {
    name: String,
    data: NativePermissionsData,
}

#[derive(Deserialize)]
struct NativePermissionsData {
    permissions: Vec<NativePermissionScope>,
}

#[derive(Deserialize)]
struct NativePermissionScope {
    scope: String,
    #[serde(default)]
    deny: Vec<String>,
}

fn validate_agy_project_permissions(bytes: &[u8]) -> Result<(), ()> {
    let readback: NativePermissionsReadback = serde_json::from_slice(bytes).map_err(|_| ())?;
    if readback.status != "SUCCESS"
        || readback.num_turns != 0
        || readback.command.name != "permissions"
        || readback.usage.input_tokens != 0
        || readback.usage.output_tokens != 0
        || readback.usage.thinking_tokens != 0
        || readback.usage.cache_read_tokens != 0
        || readback.usage.total_tokens != 0
    {
        return Err(());
    }
    let projects = readback
        .command
        .data
        .permissions
        .iter()
        .filter(|scope| scope.scope == "project")
        .collect::<Vec<_>>();
    if projects.len() != 1
        || !AGY_PROJECT_DENIES
            .iter()
            .all(|required| projects[0].deny.iter().any(|deny| deny == required))
    {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod native_permission_tests {
    use super::*;
    use serde_json::json;

    // Synthetic positive schema fixture, not evidence that an installed
    // Antigravity project has accepted this policy. Real metadata readback is
    // always required at runtime; its absence never admits an agent turn.
    fn policy() -> serde_json::Value {
        json!({"status":"SUCCESS","num_turns":0,
        "usage":{"input_tokens":0,"output_tokens":0,"thinking_tokens":0,
            "cache_read_tokens":0,"total_tokens":0},
        "command":{"name":"permissions","data":{"permissions":[
            {"scope":"project","deny":AGY_PROJECT_DENIES},
            {"scope":"shared","allow":["write_file(*)","command(*)","mcp(*)"]}
        ]}}})
    }

    #[test]
    fn agy_native_permissions_current_unprotected_project_is_rejected() {
        // Shape read back from the installed native /permissions command:
        // Project is present, while dangerous allow grants are in shared scope.
        let mut current = policy();
        current["command"]["data"]["permissions"][0] = json!({"scope":"project"});
        assert!(validate_agy_project_permissions(&serde_json::to_vec(&current).unwrap()).is_err());
        current["command"]["data"]["permissions"][1]["deny"] = json!(AGY_PROJECT_DENIES);
        assert!(validate_agy_project_permissions(&serde_json::to_vec(&current).unwrap()).is_err());
    }

    #[test]
    fn agy_native_permissions_require_exact_project_denies_and_zero_turns() {
        assert!(validate_agy_project_permissions(&serde_json::to_vec(&policy()).unwrap()).is_ok());
        for index in 0..AGY_PROJECT_DENIES.len() {
            let mut value = policy();
            value["command"]["data"]["permissions"][0]["deny"]
                .as_array_mut()
                .unwrap()
                .remove(index);
            assert!(
                validate_agy_project_permissions(&serde_json::to_vec(&value).unwrap()).is_err()
            );
        }
        for pointer in [
            "/num_turns",
            "/usage/input_tokens",
            "/usage/output_tokens",
            "/usage/thinking_tokens",
            "/usage/cache_read_tokens",
            "/usage/total_tokens",
        ] {
            let mut value = policy();
            *value.pointer_mut(pointer).unwrap() = json!(1);
            assert!(
                validate_agy_project_permissions(&serde_json::to_vec(&value).unwrap()).is_err(),
                "{pointer}"
            );
        }
        for pointer in ["/status", "/command/name"] {
            let mut value = policy();
            *value.pointer_mut(pointer).unwrap() = json!("unexpected");
            assert!(
                validate_agy_project_permissions(&serde_json::to_vec(&value).unwrap()).is_err()
            );
        }
        let mut value = policy();
        let duplicate = value["command"]["data"]["permissions"][0].clone();
        value["command"]["data"]["permissions"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(validate_agy_project_permissions(&serde_json::to_vec(&value).unwrap()).is_err());
        for bytes in [b"{}".as_slice(), b"null", b"not JSON"] {
            assert!(validate_agy_project_permissions(bytes).is_err());
        }
    }

    #[test]
    fn agy_native_permissions_bind_selected_project_and_never_query_missing_selection() {
        for project in [None, Some(""), Some("../other"), Some("id --force")] {
            let config = AiProviderConfig {
                agy_cli_project_id: project.map(str::to_string),
                ..AiProviderConfig::default()
            };
            assert!(
                admit_agy_cli_project(&config, |_| panic!("invalid selection launched CLI"))
                    .is_err()
            );
        }
        let config = AiProviderConfig {
            agy_cli_project_id: Some("  native-test-project  ".to_string()),
            ..AiProviderConfig::default()
        };
        let admitted = admit_agy_cli_project(&config, |args| {
            assert_eq!(
                args,
                &[
                    "--project=native-test-project",
                    "--print",
                    "/permissions",
                    "--output-format",
                    "json"
                ]
            );
            Ok(serde_json::to_vec(&policy()).unwrap())
        })
        .unwrap();
        assert_eq!(admitted, "--project=native-test-project");
        let error =
            admit_agy_cli_project(&config, |_| Ok(b"private policy content".to_vec())).unwrap_err();
        assert!(!error.contains("private policy content"));
    }
}
