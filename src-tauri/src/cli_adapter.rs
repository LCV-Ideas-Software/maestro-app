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

use crate::command_path::resolve_command;
use crate::command_spawn::run_resolved_command_with_timeout;
use crate::{
    sanitize_short, sanitize_text, AiProviderConfig, CliAdapterProbeResult, CliAdapterSmokeRequest,
    CliAdapterSpec,
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
    spec: CliAdapterSpec,
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

    run_cli_adapter_probe_resolved(spec, config, &path, started)
}

fn run_cli_adapter_probe_resolved(
    mut spec: CliAdapterSpec,
    config: &AiProviderConfig,
    path: &Path,
    started: Instant,
) -> CliAdapterProbeResult {
    if spec.command == "agy" {
        match agy_cli_project_argument(config) {
            Ok(Some(project_argument)) => spec.args.push(project_argument),
            Ok(None) => {}
            Err(status) => {
                return CliAdapterProbeResult {
                    name: spec.name.to_string(),
                    cli: spec.command.to_string(),
                    tone: "blocked".to_string(),
                    status: status.to_string(),
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
            status: "prazo da CLI expirou antes da execucao".to_string(),
            duration_ms: started.elapsed().as_millis(),
            exit_code: None,
            marker_found: false,
        };
    }
    match run_resolved_command_with_timeout(path, &spec.args, remaining, None) {
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

/// Select an optional native project; an empty setting leaves the CLI default.
/// This only validates the existing identifier shape, never project permissions.
pub(crate) fn agy_cli_project_argument(
    config: &AiProviderConfig,
) -> Result<Option<String>, &'static str> {
    let Some(raw_project) = config.agy_cli_project_id.as_deref() else {
        return Ok(None);
    };
    let project = raw_project.trim();
    if raw_project.chars().any(char::is_control)
        || project.len() > 128
        || !project
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_'))
    {
        return Err("AGY CLI: ID de projeto nativo invalido.");
    }
    if project.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!("--project={project}")))
}

#[cfg(test)]
mod project_tests {
    use super::*;

    #[test]
    fn agy_optional_project_uses_native_default_and_rejects_malformed_identifiers() {
        for project in [None, Some(""), Some("  ")] {
            let config = AiProviderConfig {
                agy_cli_project_id: project.map(str::to_string),
                ..AiProviderConfig::default()
            };
            assert_eq!(agy_cli_project_argument(&config).unwrap(), None);
        }
        for project in [
            "native\0project",
            "\nnative-project",
            "../other",
            "id --force",
        ] {
            let config = AiProviderConfig {
                agy_cli_project_id: Some(project.to_string()),
                ..AiProviderConfig::default()
            };
            assert!(agy_cli_project_argument(&config).is_err());
        }
    }

    #[cfg(windows)]
    fn run_native_project_fixture(project: Option<&str>) {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let directory = std::env::temp_dir().join(format!(
            "maestro-agy-optional-project-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let script = directory.join("native-probe.cmd");
        let called = directory.join("called-once.txt");
        let expected = project
            .map(|id| format!("--project={}", id.trim()))
            .unwrap_or_default();
        fs::write(
            &script,
            format!(
                "@echo off\r\nif not \"%~1\"==\"--print\" exit /b 11\r\nif not \"%~3\"==\"--print-timeout\" exit /b 12\r\nif not \"%~4\"==\"90s\" exit /b 13\r\nif not \"%~5\"==\"{expected}\" exit /b 14\r\nif not \"%~6\"==\"\" exit /b 15\r\nif exist \"%~dp0called-once.txt\" exit /b 16\r\n> \"%~dp0called-once.txt\" echo native-prompt\r\necho MAESTRO_CLI_SMOKE_AGY_READY\r\n"
            ),
        )
        .unwrap();
        let request = CliAdapterSmokeRequest {
            run_id: "native-project-fixture".to_string(),
            prompt_chars: 32,
            protocol_name: "fixture".to_string(),
            protocol_lines: 1,
            protocol_hash: "fixture".to_string(),
        };
        let spec = cli_adapter_specs(&request)
            .into_iter()
            .find(|spec| spec.command == "agy")
            .unwrap();
        let config = AiProviderConfig {
            agy_cli_project_id: project.map(str::to_string),
            ..AiProviderConfig::default()
        };
        // The actual managed Windows child receives the production print args.
        // A metadata query or extra permissions flag cannot produce this marker.
        let result = run_cli_adapter_probe_resolved(spec, &config, &script, Instant::now());
        let invocation = fs::read_to_string(&called);
        fs::remove_file(&script).unwrap();
        if called.exists() {
            fs::remove_file(&called).unwrap();
        }
        fs::remove_dir(&directory).unwrap();
        assert_eq!(result.tone, "ok", "{}", result.status);
        assert!(result.marker_found);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(invocation.unwrap().trim(), "native-prompt");
    }

    #[test]
    #[cfg(windows)]
    fn agy_smoke_launches_native_prompt_without_a_selected_project_or_policy_query() {
        run_native_project_fixture(None);
    }

    #[test]
    #[cfg(windows)]
    fn agy_smoke_passes_the_optional_native_project_without_a_policy_query() {
        run_native_project_fixture(Some("  selected-native-project  "));
    }
}
