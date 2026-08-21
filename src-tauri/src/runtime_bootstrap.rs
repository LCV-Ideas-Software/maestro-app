//! Runtime dependency inventory and operator-approved bootstrap actions.
//!
//! The IPC boundary never accepts a command, executable, argument, URL, or
//! package name from the frontend.  `runtime_bootstrap_plan` persists an
//! expiring, hashed plan assembled from the fixed operations below;
//! `execute_runtime_bootstrap_action` accepts only an `action_id`, the exact
//! `plan_hash`, and explicit approval.  This keeps system mutation behind a
//! small allowlist and makes stale/tampered plans fail closed.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::app_init::hidden_command;
use crate::app_paths::{checked_data_child_path, data_dir};
use crate::command_path::resolve_command;
use crate::command_spawn::{
    run_resolved_command_observed, run_resolved_command_with_timeout,
};
use crate::editorial_io::write_text_file;
use crate::sanitize::{redact_secrets, sanitize_short, sanitize_text};

const BOOTSTRAP_SCHEMA_VERSION: u8 = 1;
const PLAN_TTL_MINUTES: i64 = 30;
const PROBE_TIMEOUT_SECS: u64 = 10;
const ACTION_TIMEOUT_SECS: u64 = 15 * 60;
const OUTPUT_CAP_CHARS: usize = 6_000;
const NODE_MINIMUM_MAJOR: u64 = 24;
const PROGRESS_EVENT: &str = "runtime-bootstrap-progress";

static BOOTSTRAP_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static BOOTSTRAP_PLAN_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static RUNNING_ACTIONS: OnceLock<Mutex<BTreeMap<String, CancellationToken>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DependencyState {
    Ready,
    Missing,
    Outdated,
    Misconfigured,
    AuthRequired,
    ManualActionRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BootstrapActionKind {
    Install,
    Update,
    Authenticate,
    Manual,
    RetryProbe,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BootstrapDisposition {
    Retry,
    Skip,
    Defer,
    Cancel,
}

/// The complete executable allowlist.  Deserializing any value outside this
/// enum fails before an action can reach the process runner.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BootstrapOperation {
    InstallClaudeNpmUser,
    UpdateClaudeOfficial,
    InstallCodexNpmUser,
    UpdateCodexNpmUser,
    VerifyWranglerLatest,
    InstallNodeWingetUser,
    InstallNodeManual,
    RetryNetworkProbe,
    AuthenticateClaudeManual,
    AuthenticateCodexManual,
    AuthenticateAgyManual,
    InstallAgyManual,
    ConfigureDeepseekManual,
    ConfigureCloudflareManual,
    RepairPortableDataManual,
    InstallWebviewManual,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RuntimeDependency {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) required: bool,
    pub(crate) state: DependencyState,
    pub(crate) installed_version: Option<String>,
    pub(crate) latest_version: Option<String>,
    pub(crate) resolved_path: Option<String>,
    pub(crate) detail: String,
    pub(crate) recommended_action_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RuntimeBootstrapAction {
    pub(crate) action_id: String,
    pub(crate) dependency_key: String,
    pub(crate) kind: BootstrapActionKind,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) source: String,
    pub(crate) command_preview: Option<String>,
    pub(crate) install_scope: String,
    pub(crate) requires_elevation: bool,
    pub(crate) requires_interaction: bool,
    pub(crate) execution_fingerprint: String,
    operation: BootstrapOperation,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RuntimeBootstrapPlan {
    pub(crate) schema_version: u8,
    pub(crate) plan_hash: String,
    pub(crate) created_at: String,
    pub(crate) expires_at: String,
    pub(crate) dependencies: Vec<RuntimeDependency>,
    pub(crate) actions: Vec<RuntimeBootstrapAction>,
    pub(crate) required_ready: bool,
    pub(crate) report_path: String,
    pub(crate) events_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RuntimeBootstrapActionResult {
    pub(crate) action_id: String,
    pub(crate) plan_hash: String,
    pub(crate) status: String,
    pub(crate) message: String,
    pub(crate) command_preview: Option<String>,
    pub(crate) source: String,
    pub(crate) handoff_opened: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) duration_ms: Option<u128>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) post_action_dependency: Option<RuntimeDependency>,
    pub(crate) refreshed_plan: RuntimeBootstrapPlan,
    pub(crate) support_bundle_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RuntimeBootstrapControlResult {
    pub(crate) action_id: String,
    pub(crate) plan_hash: String,
    pub(crate) disposition: BootstrapDisposition,
    pub(crate) status: String,
    pub(crate) recorded_at: String,
}

#[derive(Clone, Debug, Serialize)]
struct BootstrapProgressEvent {
    action_id: String,
    plan_hash: String,
    phase: String,
    message: String,
    at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BootstrapControlStateEntry {
    disposition: BootstrapDisposition,
    suppress_until: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct BootstrapControlState {
    entries: BTreeMap<String, BootstrapControlStateEntry>,
}

#[derive(Clone, Debug)]
struct FixedCommandSpec {
    program: &'static str,
    args: Vec<String>,
}

#[tauri::command]
pub(crate) async fn runtime_bootstrap_plan() -> Result<RuntimeBootstrapPlan, String> {
    if !running_actions()
        .lock()
        .map_err(|_| "bootstrap action registry poisoned".to_string())?
        .is_empty()
    {
        return Err("cannot replace the bootstrap plan while an action is running".to_string());
    }
    tauri::async_runtime::spawn_blocking(|| build_and_persist_plan(Utc::now()))
        .await
        .map_err(|error| format!("runtime bootstrap inventory worker failed: {error}"))?
}

/// Execute one action from the persisted plan.  Tauri injects `app`; the only
/// values supplied by the caller are action_id, plan_hash, and approved.
#[tauri::command]
pub(crate) async fn execute_runtime_bootstrap_action(
    app: tauri::AppHandle,
    action_id: String,
    plan_hash: String,
    approved: bool,
) -> Result<RuntimeBootstrapActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        execute_runtime_bootstrap_action_inner(&app, &action_id, &plan_hash, approved)
    })
    .await
    .map_err(|error| format!("runtime bootstrap action worker failed: {error}"))?
}

#[tauri::command]
pub(crate) fn runtime_bootstrap_action_control(
    app: tauri::AppHandle,
    action_id: String,
    plan_hash: String,
    disposition: BootstrapDisposition,
) -> Result<RuntimeBootstrapControlResult, String> {
    if disposition == BootstrapDisposition::Cancel {
        let run_key = action_run_key(&plan_hash, &action_id);
        let running_token = running_actions()
            .lock()
            .map_err(|_| "bootstrap cancellation registry poisoned".to_string())?
            .get(&run_key)
            .cloned();
        if let Some(token) = running_token {
            token.cancel();
            persist_control_disposition(&action_id, disposition, Utc::now())?;
            let result = RuntimeBootstrapControlResult {
                action_id: action_id.clone(),
                plan_hash: plan_hash.clone(),
                disposition,
                status: "cancellation_requested".to_string(),
                recorded_at: Utc::now().to_rfc3339(),
            };
            append_ndjson(&bootstrap_dir()?.join("controls.ndjson"), &result)?;
            emit_progress(
                &app,
                &action_id,
                &plan_hash,
                "control",
                &result.status,
            );
            return Ok(result);
        }
    }
    let plan = load_and_validate_plan(&plan_hash, Utc::now())?;
    let action = plan
        .actions
        .iter()
        .find(|action| action.action_id == action_id)
        .ok_or_else(|| "unknown bootstrap action_id for this plan".to_string())?;
    if !operation_matches_action_id(&action.action_id, &action.operation) {
        return Err("unknown or mismatched bootstrap action_id".to_string());
    }

    let run_key = action_run_key(&plan_hash, &action_id);
    let status = match disposition {
        BootstrapDisposition::Retry => "ready_to_retry",
        BootstrapDisposition::Skip => "skipped",
        BootstrapDisposition::Defer => "deferred",
        BootstrapDisposition::Cancel => {
            if let Some(token) = running_actions()
                .lock()
                .map_err(|_| "bootstrap cancellation registry poisoned".to_string())?
                .get(&run_key)
            {
                token.cancel();
                "cancellation_requested"
            } else {
                "cancelled_before_start"
            }
        }
    }
    .to_string();

    let result = RuntimeBootstrapControlResult {
        action_id: action.action_id.clone(),
        plan_hash: plan_hash.clone(),
        disposition,
        status,
        recorded_at: Utc::now().to_rfc3339(),
    };
    persist_control_disposition(&action_id, disposition, Utc::now())?;
    append_ndjson(&bootstrap_dir()?.join("controls.ndjson"), &result)?;
    emit_progress(
        &app,
        &action_id,
        &plan_hash,
        "control",
        &result.status,
    );
    Ok(result)
}

fn build_and_persist_plan(now: DateTime<Utc>) -> Result<RuntimeBootstrapPlan, String> {
    let _plan_guard = bootstrap_plan_lock()
        .lock()
        .map_err(|_| "bootstrap plan lock poisoned".to_string())?;
    let mut dependencies = inventory_runtime_dependencies();
    let mut actions = actions_for_inventory(&mut dependencies);
    for action in &mut actions {
        action.execution_fingerprint = operation_execution_fingerprint(&action.operation)?;
    }
    apply_control_suppression(&mut dependencies, &mut actions, now)?;
    let required_ready = dependencies
        .iter()
        .filter(|dependency| dependency.required)
        .all(|dependency| dependency.state == DependencyState::Ready);
    let dir = bootstrap_dir()?;
    let report_path = dir.join("current-plan.json");
    let events_path = dir.join("events.ndjson");
    let mut plan = RuntimeBootstrapPlan {
        schema_version: BOOTSTRAP_SCHEMA_VERSION,
        plan_hash: String::new(),
        created_at: now.to_rfc3339(),
        expires_at: (now + ChronoDuration::minutes(PLAN_TTL_MINUTES)).to_rfc3339(),
        dependencies,
        actions,
        required_ready,
        report_path: report_path.to_string_lossy().to_string(),
        events_path: events_path.to_string_lossy().to_string(),
    };
    plan.plan_hash = compute_plan_hash(&plan)?;
    write_json(&report_path, &plan)?;
    write_json(&dir.join("inventory.json"), &plan.dependencies)?;
    Ok(plan)
}

fn inventory_runtime_dependencies() -> Vec<RuntimeDependency> {
    let mut dependencies = Vec::new();
    dependencies.push(simple_dependency(
        "webview2",
        "WebView2 Runtime",
        true,
        DependencyState::Ready,
        "active through the running Tauri WebView",
    ));
    dependencies.push(probe_portable_data());
    dependencies.push(probe_network());

    let mut claude = probe_cli("claude", "Claude CLI", "claude", &["--version"], true);
    if claude.state == DependencyState::Ready {
        apply_latest_npm_version(&mut claude, "@anthropic-ai/claude-code");
        if claude.state == DependencyState::Ready {
            apply_auth_probe(&mut claude, "claude", &["auth", "status"]);
        }
    }
    dependencies.push(claude);

    let mut codex = probe_cli("codex", "Codex CLI", "codex", &["--version"], true);
    if codex.state == DependencyState::Ready {
        apply_latest_npm_version(&mut codex, "@openai/codex");
        if codex.state == DependencyState::Ready {
            apply_auth_probe(&mut codex, "codex", &["login", "status"]);
        }
    }
    dependencies.push(codex);

    let agy = probe_cli(
        "agy",
        "Antigravity CLI (agy)",
        "agy",
        &["--version"],
        true,
    );
    let agy_present = agy.state == DependencyState::Ready;
    dependencies.push(agy);
    dependencies.push(simple_dependency(
        "agy_auth",
        "Antigravity interactive authentication",
        true,
        if agy_present {
            DependencyState::ManualActionRequired
        } else {
            DependencyState::Missing
        },
        if agy_present {
            "agy authentication is interactive and must be confirmed in its official flow"
        } else {
            "install agy before starting its interactive authentication"
        },
    ));

    let mut node = probe_cli("node", "Node.js", "node", &["--version"], true);
    if node.state == DependencyState::Ready
        && version_major(node.installed_version.as_deref()).unwrap_or_default()
            < NODE_MINIMUM_MAJOR
    {
        node.state = DependencyState::Outdated;
        node.detail = format!("Node.js {} or newer is required", NODE_MINIMUM_MAJOR);
    }
    dependencies.push(node);
    dependencies.push(probe_cli("npm", "npm", "npm", &["--version"], true));
    dependencies.push(probe_cli("npx", "npx", "npx", &["--version"], true));

    let npx_ready = dependencies
        .iter()
        .find(|dependency| dependency.key == "npx")
        .map(|dependency| dependency.state == DependencyState::Ready)
        .unwrap_or(false);
    dependencies.push(probe_wrangler_latest(npx_ready));

    dependencies.push(probe_legacy_gemini());
    dependencies.push(probe_deepseek_credential());
    dependencies.push(probe_cloudflare_credential());
    dependencies.push(probe_cli("git", "Git", "git", &["--version"], false));
    dependencies.push(probe_cli(
        "winget",
        "Windows Package Manager",
        "winget",
        &["--version"],
        false,
    ));
    dependencies.push(probe_cli(
        "rustup",
        "Rustup (development only)",
        "rustup",
        &["--version"],
        false,
    ));
    dependencies.push(probe_cli(
        "cargo",
        "Cargo (development only)",
        "cargo",
        &["--version"],
        false,
    ));
    dependencies
}

fn actions_for_inventory(
    dependencies: &mut [RuntimeDependency],
) -> Vec<RuntimeBootstrapAction> {
    let mut actions = Vec::new();
    let winget_ready = dependencies
        .iter()
        .find(|dependency| dependency.key == "winget")
        .map(|dependency| dependency.state == DependencyState::Ready)
        .unwrap_or(false);
    for dependency in dependencies.iter_mut() {
        let candidates = match (dependency.key.as_str(), dependency.state) {
            ("claude", DependencyState::Missing) => vec![action(
                "install.claude.npm.user",
                "claude",
                BootstrapActionKind::Install,
                "Install Claude Code in Maestro's portable data",
                "Official npm distribution in data/bootstrap/npm-user; no shell script is downloaded or piped to a shell.",
                "https://www.npmjs.com/package/@anthropic-ai/claude-code",
                Some("npm install --global --prefix <data/bootstrap/npm-user> @anthropic-ai/claude-code"),
                BootstrapOperation::InstallClaudeNpmUser,
            )],
            ("claude", DependencyState::Outdated) => vec![action(
                "update.claude.official",
                "claude",
                BootstrapActionKind::Update,
                "Update Claude Code",
                "Uses the installed CLI's official updater.",
                "https://docs.anthropic.com/",
                Some("claude update"),
                BootstrapOperation::UpdateClaudeOfficial,
            )],
            (
                "claude",
                DependencyState::AuthRequired | DependencyState::ManualActionRequired,
            ) => vec![manual_action(
                "auth.claude.interactive",
                "claude",
                BootstrapActionKind::Authenticate,
                "Authenticate Claude Code",
                "Start Claude interactively and complete its browser/login flow.",
                "https://docs.anthropic.com/",
                Some("claude"),
                BootstrapOperation::AuthenticateClaudeManual,
            )],
            ("codex", DependencyState::Missing) => vec![action(
                "install.codex.npm.user",
                "codex",
                BootstrapActionKind::Install,
                "Install Codex in Maestro's portable data",
                "Uses the official npm package in data/bootstrap/npm-user; the vendor PowerShell installer remains a manual alternative.",
                "https://www.npmjs.com/package/@openai/codex",
                Some("npm install --global --prefix <data/bootstrap/npm-user> @openai/codex"),
                BootstrapOperation::InstallCodexNpmUser,
            )],
            ("codex", DependencyState::Outdated) => vec![action(
                "update.codex.npm.user",
                "codex",
                BootstrapActionKind::Update,
                "Update Codex in Maestro's portable data",
                "Uses the official npm package in data/bootstrap/npm-user.",
                "https://www.npmjs.com/package/@openai/codex",
                Some("npm install --global --prefix <data/bootstrap/npm-user> @openai/codex@latest"),
                BootstrapOperation::UpdateCodexNpmUser,
            )],
            (
                "codex",
                DependencyState::AuthRequired | DependencyState::ManualActionRequired,
            ) => vec![manual_action(
                "auth.codex.interactive",
                "codex",
                BootstrapActionKind::Authenticate,
                "Authenticate Codex",
                "Run the official interactive login flow; credentials never pass through Maestro output.",
                "https://developers.openai.com/codex/",
                Some("codex login"),
                BootstrapOperation::AuthenticateCodexManual,
            )],
            ("agy", DependencyState::Missing) => vec![manual_action(
                "install.agy.vendor.manual",
                "agy",
                BootstrapActionKind::Manual,
                "Install Antigravity CLI",
                "Open the official instructions and run the vendor installer manually after reviewing it.",
                "https://antigravity.google/cli/",
                Some("irm https://antigravity.google/cli/install.ps1 | iex"),
                BootstrapOperation::InstallAgyManual,
            )],
            ("agy_auth", DependencyState::ManualActionRequired) => vec![manual_action(
                "auth.agy.interactive",
                "agy_auth",
                BootstrapActionKind::Authenticate,
                "Authenticate Antigravity",
                "Launch agy interactively and finish browser login/MFA outside captured output.",
                "https://antigravity.google/cli/",
                Some("agy"),
                BootstrapOperation::AuthenticateAgyManual,
            )],
            ("node", DependencyState::Missing | DependencyState::Outdated) => {
                let kind = if dependency.state == DependencyState::Missing {
                    BootstrapActionKind::Install
                } else {
                    BootstrapActionKind::Update
                };
                if winget_ready {
                    vec![action(
                        "install.node.winget.user",
                        "node",
                        kind,
                        "Install the supported Node.js LTS for the current user",
                        "Uses the verified WinGet manifest and requests user scope.",
                        "https://learn.microsoft.com/windows/package-manager/winget/",
                        Some("winget install --exact --id OpenJS.NodeJS.LTS --scope user"),
                        BootstrapOperation::InstallNodeWingetUser,
                    )]
                } else {
                    vec![manual_action(
                        "install.node.vendor.manual",
                        "node",
                        kind,
                        "Install the supported Node.js LTS manually",
                        "WinGet is unavailable; use the official per-user Node.js installer.",
                        "https://nodejs.org/en/download",
                        None,
                        BootstrapOperation::InstallNodeManual,
                    )]
                }
            }
            ("wrangler_latest", DependencyState::ManualActionRequired) => vec![action(
                "verify.wrangler.latest",
                "wrangler_latest",
                BootstrapActionKind::Update,
                "Resolve and verify Wrangler @latest",
                "Uses the official latest npm tag for the fallback CLI; it does not replace Cloudflare API readiness.",
                "https://www.npmjs.com/package/wrangler",
                Some("npx --yes wrangler@latest --version"),
                BootstrapOperation::VerifyWranglerLatest,
            )],
            (
                "npm",
                DependencyState::Missing | DependencyState::Misconfigured,
            ) => vec![manual_action(
                "repair.npm.node.manual",
                "npm",
                BootstrapActionKind::Manual,
                "Repair npm through the supported Node.js distribution",
                "npm is supplied by Node.js; repair or reinstall Node.js from the official distribution.",
                "https://nodejs.org/en/download",
                None,
                BootstrapOperation::InstallNodeManual,
            )],
            (
                "npx",
                DependencyState::Missing | DependencyState::Misconfigured,
            ) => vec![manual_action(
                "repair.npx.node.manual",
                "npx",
                BootstrapActionKind::Manual,
                "Repair npx through the supported Node.js distribution",
                "npx is supplied by npm/Node.js; repair or reinstall Node.js from the official distribution.",
                "https://nodejs.org/en/download",
                None,
                BootstrapOperation::InstallNodeManual,
            )],
            ("network", DependencyState::Misconfigured) => vec![action(
                "retry.network.probe",
                "network",
                BootstrapActionKind::RetryProbe,
                "Retry network readiness probe",
                "Performs no system mutation; rechecks the official npm registry endpoint.",
                "https://registry.npmjs.org/-/ping",
                None,
                BootstrapOperation::RetryNetworkProbe,
            )],
            ("portable_data", DependencyState::Misconfigured) => vec![manual_action(
                "repair.portable_data.manual",
                "portable_data",
                BootstrapActionKind::Manual,
                "Repair portable data-folder permissions",
                "Choose a writable app folder or repair current-user ACLs; no elevation is performed automatically.",
                "local Windows filesystem",
                None,
                BootstrapOperation::RepairPortableDataManual,
            )],
            ("webview2", DependencyState::Missing | DependencyState::Misconfigured) => vec![manual_elevated_action(
                "install.webview2.manual",
                "webview2",
                BootstrapActionKind::Manual,
                "Install or repair WebView2 Runtime",
                "Use Microsoft's official installer and approve UAC only if Microsoft requests it.",
                "https://developer.microsoft.com/microsoft-edge/webview2/",
                None,
                BootstrapOperation::InstallWebviewManual,
            )],
            (
                "deepseek_credential",
                DependencyState::AuthRequired | DependencyState::ManualActionRequired,
            ) => vec![manual_action(
                "configure.deepseek.credential",
                "deepseek_credential",
                BootstrapActionKind::Authenticate,
                "Configure DeepSeek credential",
                "Enter the credential through Maestro's secure configuration surface; never paste it into command output.",
                "Maestro credential settings",
                None,
                BootstrapOperation::ConfigureDeepseekManual,
            )],
            (
                "cloudflare_credential",
                DependencyState::AuthRequired | DependencyState::ManualActionRequired,
            ) => vec![manual_action(
                "configure.cloudflare.credential",
                "cloudflare_credential",
                BootstrapActionKind::Authenticate,
                "Configure Cloudflare API credentials",
                "Use Maestro settings for the primary API path; Wrangler remains fallback only.",
                "Maestro Cloudflare settings",
                None,
                BootstrapOperation::ConfigureCloudflareManual,
            )],
            _ => Vec::new(),
        };
        dependency.recommended_action_ids = candidates
            .iter()
            .map(|candidate| candidate.action_id.clone())
            .collect();
        actions.extend(candidates);
    }
    actions
}

fn execute_runtime_bootstrap_action_inner(
    app: &tauri::AppHandle,
    action_id: &str,
    plan_hash: &str,
    approved: bool,
) -> Result<RuntimeBootstrapActionResult, String> {
    let now = Utc::now();
    let plan = load_and_validate_plan(plan_hash, now)?;
    let action = validate_action_request(&plan, action_id, plan_hash, approved, now)?.clone();
    if operation_execution_fingerprint(&action.operation)? != action.execution_fingerprint {
        return Err(
            "bootstrap execution environment changed; request a fresh plan before writing"
                .to_string(),
        );
    }
    let run_key = action_run_key(plan_hash, action_id);
    let token = CancellationToken::new();
    {
        let mut running = running_actions()
            .lock()
            .map_err(|_| "bootstrap action registry poisoned".to_string())?;
        if !running.is_empty() {
            return Err("another bootstrap action is already running".to_string());
        }
        running.insert(run_key.clone(), token.clone());
    }

    emit_progress(app, action_id, plan_hash, "started", &action.title);
    emit_progress(
        app,
        action_id,
        plan_hash,
        "running",
        "allowlisted action started; output is redacted before UI or disk persistence",
    );
    let started = std::time::Instant::now();
    let has_manual_handoff = manual_handoff_url(&action.operation).is_some();
    let execution = execute_allowlisted_operation(app, plan_hash, &action, &token);
    let handoff_opened = has_manual_handoff && execution.is_ok();
    let cancelled = token.is_cancelled();
    let _ = running_actions().lock().map(|mut running| running.remove(&run_key));

    let (status, message, exit_code, duration_ms, stdout, stderr) = match execution {
        Ok(Some(output)) if cancelled => (
            "cancelled".to_string(),
            "action cancelled by operator".to_string(),
            output.output.status.code(),
            Some(output.duration_ms),
            sanitized_output(&output.output.stdout),
            sanitized_output(&output.output.stderr),
        ),
        Ok(Some(output)) if output.timed_out => (
            "failed".to_string(),
            "action exceeded its fixed timeout".to_string(),
            output.output.status.code(),
            Some(output.duration_ms),
            sanitized_output(&output.output.stdout),
            sanitized_output(&output.output.stderr),
        ),
        Ok(Some(output)) if output.output.status.success() => (
            "completed".to_string(),
            "allowlisted action completed; post-action probe executed".to_string(),
            output.output.status.code(),
            Some(output.duration_ms),
            sanitized_output(&output.output.stdout),
            sanitized_output(&output.output.stderr),
        ),
        Ok(Some(output)) => (
            "failed".to_string(),
            "allowlisted action returned a non-zero exit status".to_string(),
            output.output.status.code(),
            Some(output.duration_ms),
            sanitized_output(&output.output.stdout),
            sanitized_output(&output.output.stderr),
        ),
        Ok(None) if action.operation == BootstrapOperation::RetryNetworkProbe => (
            "completed".to_string(),
            "read-only network probe retried; refreshed inventory contains the result".to_string(),
            None,
            Some(started.elapsed().as_millis()),
            String::new(),
            String::new(),
        ),
        Ok(None) => (
            "manual_action_required".to_string(),
            if handoff_opened {
                "official handoff opened without capturing credentials; complete it, then retry the probe"
                    .to_string()
            } else {
                "complete this action in the current Settings surface; no credential is accepted through command output"
                    .to_string()
            },
            None,
            Some(started.elapsed().as_millis()),
            String::new(),
            String::new(),
        ),
        Err(error) => (
            "failed".to_string(),
            sanitize_text(&error, 500),
            None,
            Some(started.elapsed().as_millis()),
            String::new(),
            String::new(),
        ),
    };

    if action.dependency_key == "wrangler_latest" && status == "completed" {
        let marker = json!({
            "schema_version": BOOTSTRAP_SCHEMA_VERSION,
            "verified_at": Utc::now().to_rfc3339(),
            "version": first_nonempty_text(&stdout, &stderr)
        });
        write_json(&bootstrap_dir()?.join("wrangler-latest.json"), &marker)?;
    }
    emit_progress(app, action_id, plan_hash, "verifying", "running post-action probe");
    let refreshed_plan = build_and_persist_plan(Utc::now())?;
    let post_action_dependency = refreshed_plan
        .dependencies
        .iter()
        .find(|dependency| dependency.key == action.dependency_key)
        .cloned();
    let support_bundle_path = bootstrap_dir()?.join("support-bundle.json");
    let result = RuntimeBootstrapActionResult {
        action_id: action.action_id,
        plan_hash: plan_hash.to_string(),
        status,
        message,
        command_preview: action.command_preview,
        source: action.source,
        handoff_opened,
        exit_code,
        duration_ms,
        stdout,
        stderr,
        post_action_dependency,
        refreshed_plan,
        support_bundle_path: support_bundle_path.to_string_lossy().to_string(),
    };
    append_ndjson(&bootstrap_dir()?.join("actions.ndjson"), &result)?;
    write_support_bundle(&support_bundle_path, &result)?;
    emit_progress(app, action_id, plan_hash, "finished", &result.status);
    Ok(result)
}

fn execute_allowlisted_operation(
    app: &tauri::AppHandle,
    plan_hash: &str,
    action: &RuntimeBootstrapAction,
    cancel_token: &CancellationToken,
) -> Result<Option<crate::TimedCommandOutput>, String> {
    let npm_prefix = npm_user_prefix()?;
    let Some(spec) = fixed_command_spec(&action.operation, &npm_prefix) else {
        open_manual_handoff(&action.operation)?;
        return Ok(None);
    };
    let path = resolve_command(spec.program).ok_or_else(|| {
        format!(
            "allowlisted executable '{}' is not available on the effective PATH",
            spec.program
        )
    })?;
    let log_session = app.state::<crate::logging::LogSession>();
    let progress_path = bootstrap_dir()?.join("actions.ndjson");
    run_resolved_command_observed(
        &path,
        &spec.args,
        Some(Duration::from_secs(ACTION_TIMEOUT_SECS)),
        None,
        Some(crate::command_spawn::CommandProgressContext {
            log_session: &log_session,
            run_id: plan_hash,
            agent: "runtime-bootstrap",
            role: "bootstrap_action",
            cli: spec.program,
            output_path: &progress_path,
        }),
        Some(cancel_token),
    )
    .map(Some)
    .map_err(|error| format!("failed to execute allowlisted bootstrap action: {error}"))
}

fn manual_handoff_url(operation: &BootstrapOperation) -> Option<&'static str> {
    match operation {
        BootstrapOperation::AuthenticateClaudeManual => Some("https://docs.anthropic.com/"),
        BootstrapOperation::AuthenticateCodexManual => {
            Some("https://developers.openai.com/codex/auth/")
        }
        BootstrapOperation::AuthenticateAgyManual | BootstrapOperation::InstallAgyManual => {
            Some("https://antigravity.google/cli/")
        }
        BootstrapOperation::InstallNodeManual => Some("https://nodejs.org/en/download"),
        BootstrapOperation::InstallWebviewManual => Some(
            "https://developer.microsoft.com/microsoft-edge/webview2/consumer/",
        ),
        BootstrapOperation::InstallClaudeNpmUser
        | BootstrapOperation::UpdateClaudeOfficial
        | BootstrapOperation::InstallCodexNpmUser
        | BootstrapOperation::UpdateCodexNpmUser
        | BootstrapOperation::VerifyWranglerLatest
        | BootstrapOperation::InstallNodeWingetUser
        | BootstrapOperation::RetryNetworkProbe
        | BootstrapOperation::ConfigureDeepseekManual
        | BootstrapOperation::ConfigureCloudflareManual
        | BootstrapOperation::RepairPortableDataManual => None,
    }
}

fn open_manual_handoff(operation: &BootstrapOperation) -> Result<bool, String> {
    let Some(url) = manual_handoff_url(operation) else {
        return Ok(false);
    };
    #[cfg(windows)]
    let mut command = hidden_command("explorer.exe");
    #[cfg(not(windows))]
    let mut command = hidden_command("xdg-open");
    command.arg(url);
    command
        .spawn()
        .map_err(|error| format!("failed to open official bootstrap handoff: {error}"))?;
    Ok(true)
}

fn fixed_command_spec(
    operation: &BootstrapOperation,
    npm_prefix: &Path,
) -> Option<FixedCommandSpec> {
    let prefix = npm_prefix.to_string_lossy().to_string();
    match operation {
        BootstrapOperation::InstallClaudeNpmUser => Some(FixedCommandSpec {
            program: "npm",
            args: vec![
                "install".into(),
                "--global".into(),
                "--prefix".into(),
                prefix,
                "@anthropic-ai/claude-code".into(),
            ],
        }),
        BootstrapOperation::UpdateClaudeOfficial => Some(FixedCommandSpec {
            program: "claude",
            args: vec!["update".into()],
        }),
        BootstrapOperation::InstallCodexNpmUser => Some(FixedCommandSpec {
            program: "npm",
            args: vec![
                "install".into(),
                "--global".into(),
                "--prefix".into(),
                prefix,
                "@openai/codex".into(),
            ],
        }),
        BootstrapOperation::UpdateCodexNpmUser => Some(FixedCommandSpec {
            program: "npm",
            args: vec![
                "install".into(),
                "--global".into(),
                "--prefix".into(),
                prefix,
                "@openai/codex@latest".into(),
            ],
        }),
        BootstrapOperation::VerifyWranglerLatest => Some(FixedCommandSpec {
            program: "npx",
            args: vec!["--yes".into(), "wrangler@latest".into(), "--version".into()],
        }),
        BootstrapOperation::InstallNodeWingetUser => Some(FixedCommandSpec {
            program: "winget",
            args: vec![
                "install".into(),
                "--exact".into(),
                "--id".into(),
                "OpenJS.NodeJS.LTS".into(),
                "--scope".into(),
                "user".into(),
                "--accept-package-agreements".into(),
                "--accept-source-agreements".into(),
                "--disable-interactivity".into(),
            ],
        }),
        BootstrapOperation::RetryNetworkProbe => None,
        BootstrapOperation::AuthenticateClaudeManual
        | BootstrapOperation::AuthenticateCodexManual
        | BootstrapOperation::AuthenticateAgyManual
        | BootstrapOperation::InstallAgyManual
        | BootstrapOperation::InstallNodeManual
        | BootstrapOperation::ConfigureDeepseekManual
        | BootstrapOperation::ConfigureCloudflareManual
        | BootstrapOperation::RepairPortableDataManual
        | BootstrapOperation::InstallWebviewManual => None,
    }
}

fn operation_execution_fingerprint(operation: &BootstrapOperation) -> Result<String, String> {
    let npm_prefix = npm_user_prefix()?;
    let payload = if let Some(spec) = fixed_command_spec(operation, &npm_prefix) {
        json!({
            "operation": operation,
            "program": spec.program,
            "resolved_path": resolve_command(spec.program).map(|path| path.to_string_lossy().to_string()),
            "args": spec.args
        })
    } else {
        json!({ "operation": operation, "manual_or_probe": true })
    };
    let encoded = serde_json::to_vec(&payload)
        .map_err(|error| format!("failed to fingerprint bootstrap operation: {error}"))?;
    Ok(Sha256::digest(encoded)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn validate_action_request<'a>(
    plan: &'a RuntimeBootstrapPlan,
    action_id: &str,
    plan_hash: &str,
    approved: bool,
    now: DateTime<Utc>,
) -> Result<&'a RuntimeBootstrapAction, String> {
    verify_plan_integrity(plan)?;
    if plan.plan_hash != plan_hash {
        return Err("bootstrap plan_hash does not match the persisted plan".to_string());
    }
    if !approved {
        return Err("bootstrap action requires explicit operator approval".to_string());
    }
    if plan_is_expired(plan, now)? {
        return Err("bootstrap plan expired; request a fresh inventory before writing".to_string());
    }
    let action = plan
        .actions
        .iter()
        .find(|action| action.action_id == action_id)
        .ok_or_else(|| "unknown bootstrap action_id for this plan".to_string())?;
    if !operation_matches_action_id(&action.action_id, &action.operation) {
        return Err("unknown or mismatched bootstrap action_id".to_string());
    }
    Ok(action)
}

fn operation_matches_action_id(action_id: &str, operation: &BootstrapOperation) -> bool {
    matches!(
        (action_id, *operation),
        (
            "install.claude.npm.user",
            BootstrapOperation::InstallClaudeNpmUser
        ) | (
            "update.claude.official",
            BootstrapOperation::UpdateClaudeOfficial
        ) | (
            "install.codex.npm.user",
            BootstrapOperation::InstallCodexNpmUser
        ) | (
            "update.codex.npm.user",
            BootstrapOperation::UpdateCodexNpmUser
        ) | (
            "verify.wrangler.latest",
            BootstrapOperation::VerifyWranglerLatest
        ) | (
            "install.node.winget.user",
            BootstrapOperation::InstallNodeWingetUser
        ) | (
            "install.node.vendor.manual",
            BootstrapOperation::InstallNodeManual
        ) | (
            "repair.npm.node.manual",
            BootstrapOperation::InstallNodeManual
        ) | (
            "repair.npx.node.manual",
            BootstrapOperation::InstallNodeManual
        ) | (
            "retry.network.probe",
            BootstrapOperation::RetryNetworkProbe
        ) | (
            "auth.claude.interactive",
            BootstrapOperation::AuthenticateClaudeManual
        ) | (
            "auth.codex.interactive",
            BootstrapOperation::AuthenticateCodexManual
        ) | (
            "auth.agy.interactive",
            BootstrapOperation::AuthenticateAgyManual
        ) | (
            "install.agy.vendor.manual",
            BootstrapOperation::InstallAgyManual
        ) | (
            "configure.deepseek.credential",
            BootstrapOperation::ConfigureDeepseekManual
        ) | (
            "configure.cloudflare.credential",
            BootstrapOperation::ConfigureCloudflareManual
        ) | (
            "repair.portable_data.manual",
            BootstrapOperation::RepairPortableDataManual
        ) | (
            "install.webview2.manual",
            BootstrapOperation::InstallWebviewManual
        )
    )
}

fn load_and_validate_plan(
    expected_hash: &str,
    now: DateTime<Utc>,
) -> Result<RuntimeBootstrapPlan, String> {
    let path = bootstrap_dir()?.join("current-plan.json");
    let checked = checked_data_child_path(&path)?;
    let text = fs::read_to_string(&checked)
        .map_err(|error| format!("failed to read persisted bootstrap plan: {error}"))?;
    let plan: RuntimeBootstrapPlan = serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse persisted bootstrap plan: {error}"))?;
    verify_plan_integrity(&plan)?;
    if plan.plan_hash != expected_hash {
        return Err("bootstrap plan_hash is stale or does not match current-plan.json".to_string());
    }
    if plan_is_expired(&plan, now)? {
        return Err("bootstrap plan expired; request a fresh inventory before writing".to_string());
    }
    Ok(plan)
}

fn compute_plan_hash(plan: &RuntimeBootstrapPlan) -> Result<String, String> {
    let mut canonical = plan.clone();
    canonical.plan_hash.clear();
    let encoded = serde_json::to_vec(&canonical)
        .map_err(|error| format!("failed to serialize bootstrap plan for hashing: {error}"))?;
    Ok(Sha256::digest(encoded)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn verify_plan_integrity(plan: &RuntimeBootstrapPlan) -> Result<(), String> {
    if plan.schema_version != BOOTSTRAP_SCHEMA_VERSION {
        return Err(format!(
            "unsupported bootstrap plan schema_version {}; expected {}",
            plan.schema_version, BOOTSTRAP_SCHEMA_VERSION
        ));
    }
    let computed = compute_plan_hash(plan)?;
    if computed != plan.plan_hash {
        return Err("bootstrap plan integrity check failed".to_string());
    }
    Ok(())
}

fn plan_is_expired(plan: &RuntimeBootstrapPlan, now: DateTime<Utc>) -> Result<bool, String> {
    let expires_at = DateTime::parse_from_rfc3339(&plan.expires_at)
        .map_err(|error| format!("invalid bootstrap plan expiry: {error}"))?
        .with_timezone(&Utc);
    Ok(now >= expires_at)
}

fn probe_cli(
    key: &str,
    label: &str,
    command: &str,
    args: &[&str],
    required: bool,
) -> RuntimeDependency {
    let Some(path) = resolve_command(command) else {
        return RuntimeDependency {
            key: key.to_string(),
            label: label.to_string(),
            required,
            state: DependencyState::Missing,
            installed_version: None,
            latest_version: None,
            resolved_path: None,
            detail: "executable not found on the effective PATH".to_string(),
            recommended_action_ids: Vec::new(),
        };
    };
    let owned_args = args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>();
    match run_resolved_command_with_timeout(
        &path,
        &owned_args,
        Duration::from_secs(PROBE_TIMEOUT_SECS),
        None,
    ) {
        Ok(result) if result.timed_out => RuntimeDependency {
            key: key.to_string(),
            label: label.to_string(),
            required,
            state: DependencyState::Misconfigured,
            installed_version: None,
            latest_version: None,
            resolved_path: Some(path.to_string_lossy().to_string()),
            detail: "version probe timed out".to_string(),
            recommended_action_ids: Vec::new(),
        },
        Ok(result) => {
            let output = first_output_line(&result.output.stdout, &result.output.stderr);
            RuntimeDependency {
                key: key.to_string(),
                label: label.to_string(),
                required,
                state: if result.output.status.success() {
                    DependencyState::Ready
                } else {
                    DependencyState::Misconfigured
                },
                installed_version: if output.is_empty() { None } else { Some(output.clone()) },
                latest_version: None,
                resolved_path: Some(path.to_string_lossy().to_string()),
                detail: if result.output.status.success() {
                    "version probe succeeded".to_string()
                } else {
                    format!("version probe failed: {output}")
                },
                recommended_action_ids: Vec::new(),
            }
        }
        Err(error) => RuntimeDependency {
            key: key.to_string(),
            label: label.to_string(),
            required,
            state: DependencyState::Misconfigured,
            installed_version: None,
            latest_version: None,
            resolved_path: Some(path.to_string_lossy().to_string()),
            detail: sanitize_text(&format!("version probe failed to start: {error}"), 300),
            recommended_action_ids: Vec::new(),
        },
    }
}

fn apply_auth_probe(dependency: &mut RuntimeDependency, command: &str, args: &[&str]) {
    let Some(path) = resolve_command(command) else {
        dependency.state = DependencyState::Missing;
        return;
    };
    let args = args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>();
    match run_resolved_command_with_timeout(
        &path,
        &args,
        Duration::from_secs(PROBE_TIMEOUT_SECS),
        None,
    ) {
        Ok(result) if result.output.status.success() && !result.timed_out => {
            dependency.detail = "version and authenticated-status probes succeeded".to_string();
        }
        Ok(result) if result.timed_out => {
            dependency.state = DependencyState::ManualActionRequired;
            dependency.detail = "authentication probe became interactive or timed out".to_string();
        }
        Ok(result) => {
            dependency.state = DependencyState::AuthRequired;
            let _ = result;
            dependency.detail =
                "authentication status probe did not confirm an authenticated session"
                    .to_string();
        }
        Err(error) => {
            dependency.state = DependencyState::Misconfigured;
            dependency.detail = sanitize_text(&format!("authentication probe failed: {error}"), 300);
        }
    }
}

fn apply_latest_npm_version(dependency: &mut RuntimeDependency, package: &str) {
    let encoded = package.replace('/', "%2f");
    let url = format!("https://registry.npmjs.org/{encoded}/latest");
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
    {
        Ok(client) => client,
        Err(_) => return,
    };
    let latest = client
        .get(url)
        .send()
        .ok()
        .filter(|response| response.status().is_success())
        .and_then(|response| response.json::<serde_json::Value>().ok())
        .and_then(|value| value.get("version").and_then(|version| version.as_str()).map(str::to_string));
    dependency.latest_version = latest.clone();
    let installed = version_triplet(dependency.installed_version.as_deref());
    let latest_triplet = version_triplet(latest.as_deref());
    if matches!((installed, latest_triplet), (Some(current), Some(latest)) if current < latest) {
        dependency.state = DependencyState::Outdated;
        dependency.detail = "installed version is older than the official npm latest metadata".to_string();
    }
}

fn probe_portable_data() -> RuntimeDependency {
    let result = (|| -> Result<(), String> {
        let dir = bootstrap_dir()?;
        let path = checked_data_child_path(&dir.join("write-probe.tmp"))?;
        fs::write(&path, b"maestro-bootstrap-write-probe")
            .map_err(|error| format!("write failed: {error}"))?;
        fs::remove_file(&path).map_err(|error| format!("cleanup failed: {error}"))?;
        Ok(())
    })();
    simple_dependency(
        "portable_data",
        "Portable app data folder",
        true,
        if result.is_ok() {
            DependencyState::Ready
        } else {
            DependencyState::Misconfigured
        },
        &result
            .map(|_| "data/bootstrap is writable".to_string())
            .unwrap_or_else(|error| sanitize_text(&error, 300)),
    )
}

fn probe_network() -> RuntimeDependency {
    let result = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .and_then(|client| client.get("https://registry.npmjs.org/-/ping").send());
    let ready = result
        .as_ref()
        .map(|response| response.status().is_success())
        .unwrap_or(false);
    simple_dependency(
        "network",
        "Network access",
        true,
        if ready {
            DependencyState::Ready
        } else {
            DependencyState::Misconfigured
        },
        if ready {
            "official npm registry is reachable"
        } else {
            "official npm registry probe failed; check proxy, DNS, TLS, or firewall policy"
        },
    )
}

fn probe_wrangler_latest(npx_ready: bool) -> RuntimeDependency {
    if !npx_ready {
        return simple_dependency(
            "wrangler_latest",
            "Wrangler @latest fallback",
            true,
            DependencyState::Missing,
            "npx is required before the fixed wrangler@latest fallback can be verified",
        );
    }

    let marker = bootstrap_dir()
        .ok()
        .and_then(|dir| checked_data_child_path(&dir.join("wrangler-latest.json")).ok())
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    if let Some(marker) = marker {
        let verified_at = marker
            .get("verified_at")
            .and_then(|value| value.as_str())
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc));
        let version = marker
            .get("version")
            .and_then(|value| value.as_str())
            .map(|value| sanitize_text(value, 160));
        if verified_at
            .map(|value| {
                let age = Utc::now().signed_duration_since(value);
                age >= ChronoDuration::zero() && age < ChronoDuration::hours(24)
            })
            .unwrap_or(false)
        {
            let mut dependency = simple_dependency(
                "wrangler_latest",
                "Wrangler @latest fallback",
                true,
                DependencyState::Ready,
                "wrangler@latest was resolved and verified with operator approval in the last 24 hours",
            );
            dependency.installed_version = version;
            return dependency;
        }
    }

    simple_dependency(
        "wrangler_latest",
        "Wrangler @latest fallback",
        true,
        DependencyState::ManualActionRequired,
        "operator approval is required before npx resolves and verifies wrangler@latest",
    )
}

fn probe_legacy_gemini() -> RuntimeDependency {
    if let Some(path) = resolve_command("gemini") {
        RuntimeDependency {
            key: "gemini_legacy".to_string(),
            label: "Legacy Gemini CLI diagnostic".to_string(),
            required: false,
            state: DependencyState::Misconfigured,
            installed_version: None,
            latest_version: None,
            resolved_path: Some(path.to_string_lossy().to_string()),
            detail: "legacy gemini executable detected; Maestro uses agy and will not invoke this binary"
                .to_string(),
            recommended_action_ids: Vec::new(),
        }
    } else {
        simple_dependency(
            "gemini_legacy",
            "Legacy Gemini CLI diagnostic",
            false,
            DependencyState::Ready,
            "deprecated executable not present",
        )
    }
}

fn probe_deepseek_credential() -> RuntimeDependency {
    let configured = ["MAESTRO_DEEPSEEK_API_KEY", "DEEPSEEK_API_KEY"]
        .iter()
        .any(|name| std::env::var_os(name).is_some());
    simple_dependency(
        "deepseek_credential",
        "DeepSeek API credential",
        false,
        if configured {
            DependencyState::Ready
        } else {
            DependencyState::ManualActionRequired
        },
        if configured {
            "credential source detected; value not inspected or persisted"
        } else {
            "not configured; required only when the DeepSeek peer is enabled"
        },
    )
}

fn probe_cloudflare_credential() -> RuntimeDependency {
    let account = [
        "MAESTRO_CLOUDFLARE_ACCOUNT_ID",
        "CLOUDFLARE_ACCOUNT_ID",
        "CF_ACCOUNT_ID",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some());
    let token = [
        "MAESTRO_CLOUDFLARE_API_TOKEN",
        "CLOUDFLARE_API_TOKEN",
        "CF_API_TOKEN",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some());
    simple_dependency(
        "cloudflare_credential",
        "Cloudflare API credential",
        false,
        match (account, token) {
            (true, true) => DependencyState::Ready,
            (false, false) => DependencyState::ManualActionRequired,
            _ => DependencyState::AuthRequired,
        },
        match (account, token) {
            (true, true) => "account and token sources detected; values not inspected or persisted",
            (false, false) => "not configured; required only when Cloudflare D1 features are enabled",
            _ => "incomplete Cloudflare credential pair",
        },
    )
}

fn simple_dependency(
    key: &str,
    label: &str,
    required: bool,
    state: DependencyState,
    detail: &str,
) -> RuntimeDependency {
    RuntimeDependency {
        key: key.to_string(),
        label: label.to_string(),
        required,
        state,
        installed_version: None,
        latest_version: None,
        resolved_path: None,
        detail: sanitize_text(detail, 500),
        recommended_action_ids: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn action(
    action_id: &str,
    dependency_key: &str,
    kind: BootstrapActionKind,
    title: &str,
    description: &str,
    source: &str,
    command_preview: Option<&str>,
    operation: BootstrapOperation,
) -> RuntimeBootstrapAction {
    let install_scope = match operation {
        BootstrapOperation::InstallClaudeNpmUser
        | BootstrapOperation::InstallCodexNpmUser
        | BootstrapOperation::UpdateCodexNpmUser => "portable_app_data",
        _ => "current_user",
    };
    RuntimeBootstrapAction {
        action_id: action_id.to_string(),
        dependency_key: dependency_key.to_string(),
        kind,
        title: title.to_string(),
        description: description.to_string(),
        source: source.to_string(),
        command_preview: command_preview.map(str::to_string),
        install_scope: install_scope.to_string(),
        requires_elevation: false,
        requires_interaction: false,
        execution_fingerprint: String::new(),
        operation,
    }
}

#[allow(clippy::too_many_arguments)]
fn manual_action(
    action_id: &str,
    dependency_key: &str,
    kind: BootstrapActionKind,
    title: &str,
    description: &str,
    source: &str,
    command_preview: Option<&str>,
    operation: BootstrapOperation,
) -> RuntimeBootstrapAction {
    let mut value = action(
        action_id,
        dependency_key,
        kind,
        title,
        description,
        source,
        command_preview,
        operation,
    );
    value.install_scope = "manual_operator_handoff".to_string();
    value.requires_interaction = true;
    value
}

#[allow(clippy::too_many_arguments)]
fn manual_elevated_action(
    action_id: &str,
    dependency_key: &str,
    kind: BootstrapActionKind,
    title: &str,
    description: &str,
    source: &str,
    command_preview: Option<&str>,
    operation: BootstrapOperation,
) -> RuntimeBootstrapAction {
    let mut value = manual_action(
        action_id,
        dependency_key,
        kind,
        title,
        description,
        source,
        command_preview,
        operation,
    );
    value.requires_elevation = true;
    value
}

fn first_output_line(primary: &[u8], secondary: &[u8]) -> String {
    let primary = String::from_utf8_lossy(primary);
    let secondary = String::from_utf8_lossy(secondary);
    primary
        .lines()
        .chain(secondary.lines())
        .find(|line| !line.trim().is_empty())
        .map(|line| sanitize_bootstrap_text(line.trim(), 300))
        .unwrap_or_default()
}

fn first_nonempty_text(primary: &str, secondary: &str) -> String {
    primary
        .lines()
        .chain(secondary.lines())
        .find(|line| !line.trim().is_empty())
        .map(|value| sanitize_bootstrap_text(value.trim(), 160))
        .unwrap_or_else(|| "verified".to_string())
}

fn sanitized_output(bytes: &[u8]) -> String {
    sanitize_bootstrap_text(&String::from_utf8_lossy(bytes), OUTPUT_CAP_CHARS)
}

fn sanitize_bootstrap_text(value: &str, max_chars: usize) -> String {
    let known_redacted = redact_secrets(value);
    let header_redacted = bootstrap_header_secret_regex()
        .replace_all(&known_redacted, "$1<redacted>")
        .to_string();
    let query_redacted = bootstrap_query_secret_regex()
        .replace_all(&header_redacted, "$1<redacted>")
        .to_string();
    sanitize_text(&query_redacted, max_chars)
}

fn bootstrap_header_secret_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)\b(authorization\s*:\s*(?:bearer|basic)\s+|(?:set-)?cookie\s*:\s*|(?:api[_ -]?key|access[_ -]?token|refresh[_ -]?token|device[_ -]?code|password)\s*[:=]\s*)[^\r\n]+",
        )
        .expect("valid bootstrap output header redaction regex")
    })
}

fn bootstrap_query_secret_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)([?&](?:code|token|access_token|refresh_token)=)[^&\s]+")
            .expect("valid bootstrap output query redaction regex")
    })
}

fn version_major(value: Option<&str>) -> Option<u64> {
    version_triplet(value).map(|version| version.0)
}

fn version_triplet(value: Option<&str>) -> Option<(u64, u64, u64)> {
    let value = value?;
    let numeric = value
        .trim_start_matches(|character: char| !character.is_ascii_digit())
        .split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .next()?;
    let mut parts = numeric.split('.').filter_map(|part| part.parse::<u64>().ok());
    Some((
        parts.next()?,
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    ))
}

fn npm_user_prefix() -> Result<PathBuf, String> {
    checked_data_child_path(&data_dir().join("bootstrap").join("npm-user"))
}

fn bootstrap_dir() -> Result<PathBuf, String> {
    let dir = checked_data_child_path(&data_dir().join("bootstrap"))?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create runtime bootstrap data dir: {error}"))?;
    Ok(dir)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let encoded = serde_json::to_string_pretty(value)
        .map_err(|error| format!("failed to serialize runtime bootstrap artifact: {error}"))?;
    write_text_file(path, &encoded)
        .map_err(|error| format!("failed to persist runtime bootstrap artifact: {error}"))
}

fn append_ndjson(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let checked = checked_data_child_path(path)?;
    if let Some(parent) = checked.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create bootstrap event dir: {error}"))?;
    }
    let mut encoded = serde_json::to_string(value)
        .map_err(|error| format!("failed to serialize bootstrap event: {error}"))?;
    encoded.push('\n');
    let _guard = bootstrap_io_lock()
        .lock()
        .map_err(|_| "bootstrap event write lock poisoned".to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&checked)
        .map_err(|error| format!("failed to open bootstrap event log: {error}"))?;
    file.write_all(encoded.as_bytes())
        .map_err(|error| format!("failed to append bootstrap event: {error}"))?;
    file.sync_data()
        .map_err(|error| format!("failed to flush bootstrap event: {error}"))
}

fn write_support_bundle(
    path: &Path,
    result: &RuntimeBootstrapActionResult,
) -> Result<(), String> {
    let bundle = json!({
        "schema_version": BOOTSTRAP_SCHEMA_VERSION,
        "generated_at": Utc::now().to_rfc3339(),
        "os": std::env::consts::OS,
        "os_version": support_os_version(),
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "plan_hash": result.plan_hash,
        "action": {
            "action_id": result.action_id,
            "status": result.status,
            "message": result.message,
            "command_preview": result.command_preview,
            "source": result.source,
            "handoff_opened": result.handoff_opened,
            "exit_code": result.exit_code,
            "duration_ms": result.duration_ms,
            "stdout": result.stdout,
            "stderr": result.stderr
        },
        "dependency_matrix": result.refreshed_plan.dependencies.iter().map(|dependency| json!({
            "key": dependency.key,
            "label": dependency.label,
            "required": dependency.required,
            "state": dependency.state,
            "installed_version": dependency.installed_version,
            "latest_version": dependency.latest_version,
            "executable_resolved": dependency.resolved_path.is_some(),
            "detail": dependency.detail,
            "recommended_action_ids": dependency.recommended_action_ids
        })).collect::<Vec<_>>(),
        "next_actions": result.refreshed_plan.actions.iter().map(|action| json!({
            "action_id": action.action_id,
            "dependency_key": action.dependency_key,
            "kind": action.kind,
            "command_preview": action.command_preview,
            "source": action.source,
            "requires_elevation": action.requires_elevation,
            "requires_interaction": action.requires_interaction
        })).collect::<Vec<_>>()
    });
    write_json(path, &bundle)
}

fn apply_control_suppression(
    dependencies: &mut [RuntimeDependency],
    actions: &mut Vec<RuntimeBootstrapAction>,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let state = read_control_state()?;
    let suppressed = state
        .entries
        .into_iter()
        .filter_map(|(action_id, entry)| {
            let until = DateTime::parse_from_rfc3339(&entry.suppress_until)
                .ok()?
                .with_timezone(&Utc);
            (until > now && entry.disposition != BootstrapDisposition::Retry).then_some(action_id)
        })
        .collect::<std::collections::BTreeSet<_>>();
    actions.retain(|action| !suppressed.contains(&action.action_id));
    for dependency in dependencies {
        dependency
            .recommended_action_ids
            .retain(|action_id| !suppressed.contains(action_id));
    }
    Ok(())
}

fn persist_control_disposition(
    action_id: &str,
    disposition: BootstrapDisposition,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let mut state = read_control_state()?;
    if disposition == BootstrapDisposition::Retry {
        state.entries.remove(action_id);
    } else {
        let duration = match disposition {
            BootstrapDisposition::Skip | BootstrapDisposition::Cancel => ChronoDuration::hours(1),
            BootstrapDisposition::Defer => ChronoDuration::hours(24),
            BootstrapDisposition::Retry => ChronoDuration::zero(),
        };
        state.entries.insert(
            action_id.to_string(),
            BootstrapControlStateEntry {
                disposition,
                suppress_until: (now + duration).to_rfc3339(),
            },
        );
    }
    write_json(&bootstrap_dir()?.join("control-state.json"), &state)
}

fn read_control_state() -> Result<BootstrapControlState, String> {
    let path = checked_data_child_path(&bootstrap_dir()?.join("control-state.json"))?;
    if !path.exists() {
        return Ok(BootstrapControlState::default());
    }
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read bootstrap control state: {error}"))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse bootstrap control state: {error}"))
}

fn emit_progress(
    app: &tauri::AppHandle,
    action_id: &str,
    plan_hash: &str,
    phase: &str,
    message: &str,
) {
    let event = BootstrapProgressEvent {
        action_id: sanitize_short(action_id, 120),
        plan_hash: sanitize_short(plan_hash, 80),
        phase: sanitize_short(phase, 80),
        message: sanitize_text(message, 500),
        at: Utc::now().to_rfc3339(),
    };
    let _ = app.emit(PROGRESS_EVENT, &event);
    if let Ok(dir) = bootstrap_dir() {
        let _ = append_ndjson(&dir.join("events.ndjson"), &event);
    }
    let log_session = app.state::<crate::logging::LogSession>();
    let _ = crate::logging::write_log_record(
        &log_session,
        crate::logging::LogEventInput {
            level: if phase == "finished" { "info" } else { "debug" }.to_string(),
            category: "runtime.bootstrap.progress".to_string(),
            message: event.message.clone(),
            context: Some(json!({
                "action_id": event.action_id,
                "plan_hash_prefix": event.plan_hash.chars().take(12).collect::<String>(),
                "phase": event.phase
            })),
        },
    );
}

fn support_os_version() -> String {
    #[cfg(windows)]
    {
        if let Some(path) = resolve_command("cmd") {
            let args = vec!["/D".to_string(), "/C".to_string(), "ver".to_string()];
            if let Ok(result) = run_resolved_command_with_timeout(
                &path,
                &args,
                Duration::from_secs(5),
                None,
            ) {
                let version = first_output_line(&result.output.stdout, &result.output.stderr);
                if !version.is_empty() {
                    return version;
                }
            }
        }
    }
    std::env::consts::OS.to_string()
}

fn action_run_key(plan_hash: &str, action_id: &str) -> String {
    format!("{plan_hash}:{action_id}")
}

fn bootstrap_io_lock() -> &'static Mutex<()> {
    BOOTSTRAP_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn bootstrap_plan_lock() -> &'static Mutex<()> {
    BOOTSTRAP_PLAN_LOCK.get_or_init(|| Mutex::new(()))
}

fn running_actions() -> &'static Mutex<BTreeMap<String, CancellationToken>> {
    RUNNING_ACTIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_plan(now: DateTime<Utc>) -> RuntimeBootstrapPlan {
        let mut dependency = simple_dependency(
            "claude",
            "Claude CLI",
            true,
            DependencyState::Missing,
            "missing",
        );
        let actions = actions_for_inventory(std::slice::from_mut(&mut dependency));
        let mut plan = RuntimeBootstrapPlan {
            schema_version: BOOTSTRAP_SCHEMA_VERSION,
            plan_hash: String::new(),
            created_at: now.to_rfc3339(),
            expires_at: (now + ChronoDuration::minutes(PLAN_TTL_MINUTES)).to_rfc3339(),
            dependencies: vec![dependency],
            actions,
            required_ready: false,
            report_path: "data/bootstrap/current-plan.json".to_string(),
            events_path: "data/bootstrap/events.ndjson".to_string(),
        };
        plan.plan_hash = compute_plan_hash(&plan).unwrap();
        plan
    }

    #[test]
    fn dependency_states_serialize_to_contract_values() {
        let states = [
            DependencyState::Ready,
            DependencyState::Missing,
            DependencyState::Outdated,
            DependencyState::Misconfigured,
            DependencyState::AuthRequired,
            DependencyState::ManualActionRequired,
        ];
        let encoded = states
            .iter()
            .map(|state| serde_json::to_string(state).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            encoded,
            [
                "\"ready\"",
                "\"missing\"",
                "\"outdated\"",
                "\"misconfigured\"",
                "\"auth_required\"",
                "\"manual_action_required\""
            ]
        );
    }

    #[test]
    fn plan_hash_is_deterministic_and_covers_actions() {
        let now = DateTime::parse_from_rfc3339("2026-08-21T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let plan = test_plan(now);
        assert_eq!(compute_plan_hash(&plan).unwrap(), plan.plan_hash);
        let mut tampered = plan.clone();
        tampered.actions[0].title.push_str(" changed");
        assert_ne!(compute_plan_hash(&tampered).unwrap(), plan.plan_hash);
        assert!(verify_plan_integrity(&tampered).is_err());

        let mut incompatible = plan.clone();
        incompatible.schema_version = BOOTSTRAP_SCHEMA_VERSION + 1;
        incompatible.plan_hash = compute_plan_hash(&incompatible).unwrap();
        assert!(verify_plan_integrity(&incompatible)
            .unwrap_err()
            .contains("schema_version"));
    }

    #[test]
    fn execution_rejects_unknown_unapproved_and_expired_requests() {
        let now = DateTime::parse_from_rfc3339("2026-08-21T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let plan = test_plan(now);
        assert!(validate_action_request(
            &plan,
            "install.claude.npm.user",
            &plan.plan_hash,
            false,
            now
        )
        .unwrap_err()
        .contains("approval"));
        assert!(validate_action_request(&plan, "arbitrary", &plan.plan_hash, true, now)
            .unwrap_err()
            .contains("unknown"));
        let mut injected = plan.clone();
        injected.actions[0].action_id = "arbitrary".to_string();
        injected.plan_hash = compute_plan_hash(&injected).unwrap();
        assert!(validate_action_request(
            &injected,
            "arbitrary",
            &injected.plan_hash,
            true,
            now
        )
        .unwrap_err()
        .contains("unknown"));
        assert!(validate_action_request(
            &plan,
            "install.claude.npm.user",
            &plan.plan_hash,
            true,
            now + ChronoDuration::minutes(PLAN_TTL_MINUTES)
        )
        .unwrap_err()
        .contains("expired"));
    }

    #[test]
    fn automated_specs_are_fixed_and_do_not_use_shell_pipelines() {
        let prefix = Path::new(r"C:\Users\Example\AppData\Roaming\npm");
        let operations = [
            BootstrapOperation::InstallClaudeNpmUser,
            BootstrapOperation::UpdateClaudeOfficial,
            BootstrapOperation::InstallCodexNpmUser,
            BootstrapOperation::UpdateCodexNpmUser,
            BootstrapOperation::VerifyWranglerLatest,
            BootstrapOperation::InstallNodeWingetUser,
        ];
        for operation in operations {
            let spec = fixed_command_spec(&operation, prefix).expect("automated operation");
            assert!(matches!(spec.program, "npm" | "claude" | "npx" | "winget"));
            assert!(spec.args.iter().all(|arg| !arg.contains('|')));
            assert!(spec.args.iter().all(|arg| !arg.contains("irm ")));
        }
        let claude = fixed_command_spec(&BootstrapOperation::InstallClaudeNpmUser, prefix).unwrap();
        assert!(claude.args.contains(&"@anthropic-ai/claude-code".to_string()));
        let codex = fixed_command_spec(&BootstrapOperation::InstallCodexNpmUser, prefix).unwrap();
        assert!(codex.args.contains(&"@openai/codex".to_string()));
        let wrangler = fixed_command_spec(&BootstrapOperation::VerifyWranglerLatest, prefix).unwrap();
        assert!(wrangler.args.contains(&"wrangler@latest".to_string()));
    }

    #[test]
    fn vendor_scripts_and_auth_flows_are_manual_only() {
        let prefix = Path::new("user-prefix");
        for operation in [
            BootstrapOperation::InstallAgyManual,
            BootstrapOperation::AuthenticateClaudeManual,
            BootstrapOperation::AuthenticateCodexManual,
            BootstrapOperation::AuthenticateAgyManual,
            BootstrapOperation::ConfigureDeepseekManual,
            BootstrapOperation::ConfigureCloudflareManual,
        ] {
            assert!(fixed_command_spec(&operation, prefix).is_none());
        }
    }

    #[test]
    fn action_generation_is_ordered_and_links_dependency_ids() {
        let mut inventory = vec![
            simple_dependency(
                "claude",
                "Claude",
                true,
                DependencyState::Missing,
                "missing",
            ),
            simple_dependency(
                "wrangler_latest",
                "Wrangler",
                true,
                DependencyState::ManualActionRequired,
                "approval",
            ),
        ];
        let actions = actions_for_inventory(&mut inventory);
        assert_eq!(actions[0].action_id, "install.claude.npm.user");
        assert_eq!(actions[1].action_id, "verify.wrangler.latest");
        assert_eq!(inventory[0].recommended_action_ids, vec![actions[0].action_id.clone()]);
        assert_eq!(inventory[1].recommended_action_ids, vec![actions[1].action_id.clone()]);
    }

    #[test]
    fn missing_npm_and_npx_receive_actionable_node_repair_handoffs() {
        let mut inventory = vec![
            simple_dependency(
                "npm",
                "npm",
                true,
                DependencyState::Missing,
                "missing",
            ),
            simple_dependency(
                "npx",
                "npx",
                true,
                DependencyState::Misconfigured,
                "broken",
            ),
        ];
        let actions = actions_for_inventory(&mut inventory);
        assert_eq!(actions[0].action_id, "repair.npm.node.manual");
        assert_eq!(actions[1].action_id, "repair.npx.node.manual");
        assert!(actions.iter().all(|action| action.requires_interaction));
        assert!(actions
            .iter()
            .all(|action| manual_handoff_url(&action.operation) == Some("https://nodejs.org/en/download")));
    }

    #[test]
    fn output_is_redacted_and_bounded_before_persistence() {
        let secret_value = ["s", "k-test-secret-material"].concat();
        let secret = format!("Authorization: Bearer {secret_value}\nmore");
        let output = sanitized_output(secret.as_bytes());
        assert!(!output.contains(&secret_value));
        assert!(output.contains("<redacted>"));
        assert!(!output.contains('\n'));
    }

    #[test]
    fn semantic_version_parser_handles_cli_prefixes() {
        assert_eq!(version_triplet(Some("claude 2.3.4")), Some((2, 3, 4)));
        assert_eq!(version_triplet(Some("v24.1.0")), Some((24, 1, 0)));
        assert_eq!(version_triplet(None), None);
    }

    #[test]
    fn npm_install_prefix_is_confined_to_portable_bootstrap_data() {
        let prefix = npm_user_prefix().unwrap();
        assert_eq!(prefix, data_dir().join("bootstrap").join("npm-user"));
        assert!(checked_data_child_path(&prefix).is_ok());
    }
}
