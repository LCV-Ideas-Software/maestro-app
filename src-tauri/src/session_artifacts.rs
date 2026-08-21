// Modulo: src-tauri/src/session_artifacts.rs
// Descricao: Resumable-session inspection + agent-runs/* artifact reading
// helpers extracted from lib.rs in v0.3.29 per `docs/code-split-plan.md`
// migration step 5.
//
// What's here (9 functions):
//   - `inspect_resumable_session_dir` — top-level entry that decides whether
//     a session directory is resumable (`prompt.md` + `protocolo.md` present
//     and no `texto-final.md`); enriches the result with saved-contract
//     defaults (active_agents/initial_agent/caps) for the picker UI.
//   - `load_resume_session_state` — reads the latest draft + existing agent
//     results so the orchestrator can pick up mid-session.
//   - `find_latest_draft_artifact`, `find_latest_draft_artifact_from_artifacts`,
//     `artifact_resume_rank` — find the most-advanced (round, role) draft in
//     the agent-runs/ directory.
//   - `load_agent_results_from_dir`, `read_agent_artifacts` — recover the
//     per-round agent result vector from disk.
//   - `parse_agent_artifact_name`, `parse_agent_artifact_result` — parse the
//     canonical `round-NNN-{peer}-{role}.md` filename and the bullet-list
//     metadata at the top of the artifact body.
//
// What stays in lib.rs (consumed via `pub(crate)` imports):
//   - `SessionArtifact` (pub(crate) struct) — referenced by both
//     `session_resume.rs` (extracted in v0.3.28) and this module.
//   - `ResumableSessionInfo`, `ResumeSessionState` (v0.3.29 upgrades fields
//     to pub(crate) so the migrated functions can construct values).
//   - `EditorialAgentResult` (already pub(crate)).
//   - `extract_stdout_block`, `read_text_file` (already pub(crate)).
//
// v0.3.29 is a pure move: every signature, format string, and bullet label
// is identical to the v0.3.28 lib.rs source (commit 5f35960).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app_paths::{checked_data_child_path, sanitize_path_segment};
use crate::session_persistence::load_session_contract;
use crate::session_resume::{
    count_known_session_markdown_artifacts, extract_bullet_code_value, extract_saved_session_name,
    humanize_agent_name, known_session_activity_unix,
};
use crate::{
    extract_stdout_block, extract_tagged_block, read_text_file, write_text_file,
    EditorialAgentResult, ProviderCacheTelemetry, ResumableSessionInfo, ResumeSessionState,
    SessionArtifact,
};

const CIRCULAR_REVIEW_STATE_FILE: &str = "circular-review-state.json";
pub(crate) const CIRCULAR_REVIEW_STATE_SCHEMA_VERSION: u8 = 3;
pub(crate) const CIRCULAR_REVIEW_ROSTER_SCHEMA_VERSION: u8 = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CircularReviewState {
    pub(crate) schema_version: u8,
    pub(crate) run_id: String,
    pub(crate) current_draft_artifact: String,
    pub(crate) current_draft_author_key: String,
    pub(crate) current_draft_sha256: String,
    pub(crate) round: usize,
    pub(crate) turn_index: usize,
    #[serde(default)]
    pub(crate) round_roster: Vec<String>,
    #[serde(default)]
    pub(crate) valid_round_agents: Vec<String>,
    #[serde(default)]
    pub(crate) stable_serial_approval_agents: Vec<String>,
    /// Slots de retry corretivo pago já reservados, indexados pela rodada.
    ///
    /// O campo integra o schema v3 e usa default vazio para que estados
    /// anteriores sejam lidos e pausados com segurança. A reserva é persistida antes do
    /// dispatch do provider: um crash não pode zerar e contornar o teto.
    #[serde(default)]
    pub(crate) paid_corrective_retries_by_round: std::collections::BTreeMap<usize, u32>,
    /// Contadores corretivos por turno. As chaves contêm apenas posição,
    /// agente e SHA-256 do draft — nunca o texto editorial.
    #[serde(default)]
    pub(crate) corrective_contract_retry_counts: std::collections::BTreeMap<String, u32>,
    /// `true` somente quando o estado nasceu ou foi continuado com accounting
    /// v3. Sessões antigas não recebem orçamento pago novo por default.
    #[serde(default)]
    pub(crate) retry_accounting_authoritative: bool,
    pub(crate) updated_at: String,
}

pub(crate) fn circular_draft_sha256(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn write_circular_review_state(
    session_dir: &Path,
    state: &CircularReviewState,
) -> Result<(), String> {
    let path = checked_data_child_path(&session_dir.join(CIRCULAR_REVIEW_STATE_FILE))?;
    let body = serde_json::to_string_pretty(state)
        .map_err(|error| format!("failed to serialize circular review state: {error}"))?;
    write_text_file(&path, &format!("{body}\n"))
}

pub(crate) fn inspect_resumable_session_dir(
    path: &Path,
) -> Result<Option<ResumableSessionInfo>, String> {
    let session_dir = checked_data_child_path(path)?;
    let run_id = session_dir
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| sanitize_path_segment(value, 120))
        .unwrap_or_default();
    if run_id.is_empty() {
        return Ok(None);
    }

    let prompt_path = session_dir.join("prompt.md");
    let protocol_path = session_dir.join("protocolo.md");
    if !prompt_path.is_file() || !protocol_path.is_file() {
        return Ok(None);
    }

    let final_path = session_dir.join("texto-final.md");
    if final_path.is_file() {
        return Ok(None);
    }

    let prompt_text = read_text_file(&prompt_path)?;
    let protocol_text = read_text_file(&protocol_path)?;
    let agent_dir = checked_data_child_path(&session_dir.join("agent-runs"))?;
    let artifacts = read_agent_artifacts(&agent_dir)?;
    let latest_draft = find_latest_draft_artifact_from_artifacts(&agent_dir, &artifacts)?;
    let next_round = latest_draft
        .as_ref()
        .map(|artifact| artifact.round.max(1))
        .unwrap_or(1);
    let artifact_count = count_known_session_markdown_artifacts(&session_dir, &artifacts)?;
    let last_activity_unix =
        known_session_activity_unix(&session_dir, &prompt_path, &protocol_path, &artifacts)
            .unwrap_or(0);
    let status = if latest_draft.is_some() {
        "pronta para continuar".to_string()
    } else {
        "aguardando primeiro rascunho".to_string()
    };

    let saved_contract = load_session_contract(&session_dir);
    let saved_active_agents = saved_contract
        .as_ref()
        .map(|contract| contract.active_agents.clone())
        .unwrap_or_default();
    let saved_initial_agent = saved_contract
        .as_ref()
        .and_then(|contract| {
            contract
                .original_initial_agent
                .clone()
                .or_else(|| Some(contract.initial_agent.clone()))
        })
        .filter(|value| !value.trim().is_empty());
    let saved_max_session_cost_usd = saved_contract
        .as_ref()
        .and_then(|contract| contract.max_session_cost_usd);
    let saved_max_session_minutes = saved_contract
        .as_ref()
        .and_then(|contract| contract.max_session_minutes);

    Ok(Some(ResumableSessionInfo {
        run_id,
        session_name: extract_saved_session_name(&prompt_text)
            .unwrap_or_else(|| "Sessao editorial".to_string()),
        session_dir: session_dir.to_string_lossy().to_string(),
        prompt_path: prompt_path.to_string_lossy().to_string(),
        protocol_path: protocol_path.to_string_lossy().to_string(),
        draft_path: latest_draft
            .as_ref()
            .map(|artifact| artifact.path.to_string_lossy().to_string()),
        final_markdown_path: None,
        next_round,
        last_activity_unix,
        artifact_count,
        protocol_lines: protocol_text.lines().count(),
        status,
        saved_active_agents,
        saved_initial_agent,
        saved_max_session_cost_usd,
        saved_max_session_minutes,
    }))
}

pub(crate) fn load_resume_session_state(agent_dir: &Path) -> Result<ResumeSessionState, String> {
    let agent_dir = checked_data_child_path(agent_dir)?;
    let persisted = load_persisted_circular_custody(&agent_dir)?;
    let latest_draft = find_latest_draft_artifact(&agent_dir)?;
    let existing_agents = load_agent_results_from_dir(&agent_dir)?;

    if let Some(artifact) = latest_draft {
        let text = read_text_file(&artifact.path)?;
        let stdout = extract_stdout_block(&text).unwrap_or(text.as_str());
        let draft = resumable_text_from_artifact_stdout(&artifact.role, stdout).unwrap_or_default();
        if !draft.is_empty() {
            return Ok(ResumeSessionState {
                current_draft: draft,
                current_draft_path: Some(artifact.path),
                next_review_round: artifact.round.max(1),
                existing_agents,
                circular_state: persisted.map(|(state, _, _)| state),
            });
        }
    }

    Ok(ResumeSessionState {
        current_draft: String::new(),
        current_draft_path: None,
        next_review_round: 1,
        existing_agents,
        circular_state: None,
    })
}

pub(crate) fn find_latest_draft_artifact(
    agent_dir: &Path,
) -> Result<Option<SessionArtifact>, String> {
    let artifacts = read_agent_artifacts(agent_dir)?;
    find_latest_draft_artifact_from_artifacts(agent_dir, &artifacts)
}

fn find_latest_draft_artifact_from_artifacts(
    agent_dir: &Path,
    artifacts: &[SessionArtifact],
) -> Result<Option<SessionArtifact>, String> {
    if let Some((_, artifact, _)) = load_persisted_circular_custody(agent_dir)? {
        return Ok(Some(artifact));
    }

    let mut artifacts = artifacts
        .iter()
        .filter(|artifact| artifact.role == "revision" || artifact.role == "draft")
        .cloned()
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| {
        artifact_modified_at(left)
            .cmp(&artifact_modified_at(right))
            .then_with(|| artifact_resume_rank(left).cmp(&artifact_resume_rank(right)))
            .then_with(|| left.agent.cmp(&right.agent))
    });

    let mut latest = None::<(SessionArtifact, String)>;
    for artifact in artifacts {
        let Some(result) = parse_agent_artifact_result(&artifact) else {
            continue;
        };
        if !is_accepted_custody_status(&result.status) {
            continue;
        }
        let text = read_text_file(&artifact.path).unwrap_or_default();
        let stdout = extract_stdout_block(&text).unwrap_or(text.as_str());
        let draft = resumable_text_from_artifact_stdout(&artifact.role, stdout).unwrap_or_default();
        if draft.trim().is_empty() {
            continue;
        }
        if artifact.role == "draft" {
            if latest.is_none() {
                latest = Some((artifact, draft));
            }
            continue;
        }
        let changed = latest
            .as_ref()
            .map(|(_, current)| normalize_resume_text(current) != normalize_resume_text(&draft))
            .unwrap_or(true);
        if changed {
            latest = Some((artifact, draft));
        }
    }

    Ok(latest.map(|(artifact, _)| artifact))
}

fn resumable_text_from_artifact_stdout(role: &str, stdout: &str) -> Option<String> {
    if role == "revision" {
        return extract_tagged_block(stdout, "maestro_final_text");
    }
    extract_tagged_block(stdout, "maestro_final_text").or_else(|| {
        let text = stdout.trim();
        (!text.is_empty()).then(|| text.to_string())
    })
}

fn artifact_resume_rank(artifact: &SessionArtifact) -> (usize, usize, usize) {
    let role_rank = if artifact.role == "revision" { 1 } else { 0 };
    (artifact.round, role_rank, artifact.attempt)
}

fn artifact_modified_at(artifact: &SessionArtifact) -> SystemTime {
    fs::metadata(&artifact.path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(UNIX_EPOCH)
}

fn normalize_resume_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_accepted_custody_status(status: &str) -> bool {
    matches!(status, "DRAFT_CREATED" | "READY" | "NOT_READY")
}

fn load_persisted_circular_custody(
    agent_dir: &Path,
) -> Result<Option<(CircularReviewState, SessionArtifact, String)>, String> {
    let session_dir = agent_dir
        .parent()
        .ok_or_else(|| "agent-runs directory has no session parent".to_string())?;
    let state_path = checked_data_child_path(&session_dir.join(CIRCULAR_REVIEW_STATE_FILE))?;
    if !state_path.is_file() {
        return Ok(None);
    }

    let body = read_text_file(&state_path)?;
    let state = serde_json::from_str::<CircularReviewState>(&body)
        .map_err(|error| format!("circular review state is invalid JSON: {error}"))?;
    if !(1..=CIRCULAR_REVIEW_STATE_SCHEMA_VERSION).contains(&state.schema_version) {
        return Err(format!(
            "unsupported circular review state schema version: {}",
            state.schema_version
        ));
    }
    let expected_run_id = session_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if state.run_id != expected_run_id {
        return Err(
            "circular review state run_id does not match its session directory".to_string(),
        );
    }
    if !is_supported_agent_key(&state.current_draft_author_key) {
        return Err("circular review state contains an unknown draft author".to_string());
    }
    let artifact_name = Path::new(&state.current_draft_artifact);
    if artifact_name
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty())
        || artifact_name.file_name().and_then(|value| value.to_str())
            != Some(state.current_draft_artifact.as_str())
    {
        return Err(
            "circular review state current_draft_artifact must be a canonical file name"
                .to_string(),
        );
    }
    let artifact =
        parse_agent_artifact_name(agent_dir, &state.current_draft_artifact).ok_or_else(|| {
            "circular review state references a noncanonical agent artifact".to_string()
        })?;
    if !matches!(artifact.role.as_str(), "draft" | "revision")
        || artifact.agent != state.current_draft_author_key
    {
        return Err(
            "circular review state artifact role or author does not match accepted custody"
                .to_string(),
        );
    }
    let result = parse_agent_artifact_result(&artifact).ok_or_else(|| {
        "circular review state references an unreadable agent artifact".to_string()
    })?;
    if !is_accepted_custody_status(&result.status) {
        return Err(format!(
            "circular review state references rejected artifact status {}",
            result.status
        ));
    }
    if state
        .round_roster
        .iter()
        .chain(state.valid_round_agents.iter())
        .chain(state.stable_serial_approval_agents.iter())
        .any(|agent| !is_supported_agent_key(agent))
    {
        return Err("circular review state contains an unknown reviewer".to_string());
    }
    let unique_roster = state
        .round_roster
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    if unique_roster.len() != state.round_roster.len() {
        return Err("circular review state contains a duplicate roster member".to_string());
    }
    if state
        .valid_round_agents
        .iter()
        .chain(state.stable_serial_approval_agents.iter())
        .any(|agent| {
            state.schema_version >= CIRCULAR_REVIEW_ROSTER_SCHEMA_VERSION
                && !state.round_roster.contains(agent)
        })
    {
        return Err(
            "circular review state progress references an agent outside its roster".to_string(),
        );
    }

    let text = read_text_file(&artifact.path)?;
    let stdout = extract_stdout_block(&text).unwrap_or(text.as_str());
    let draft = resumable_text_from_artifact_stdout(&artifact.role, stdout)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "circular review state references an artifact without accepted text".to_string()
        })?;
    if circular_draft_sha256(&draft) != state.current_draft_sha256 {
        return Err(
            "circular review state draft hash does not match the accepted artifact".to_string(),
        );
    }

    Ok(Some((state, artifact, draft)))
}

fn is_supported_agent_key(agent: &str) -> bool {
    matches!(
        agent,
        "claude" | "codex" | "gemini" | "deepseek" | "grok" | "perplexity"
    )
}

pub(crate) fn load_agent_results_from_dir(
    agent_dir: &Path,
) -> Result<Vec<EditorialAgentResult>, String> {
    let mut artifacts = read_agent_artifacts(agent_dir)?;
    artifacts.sort_by(|left, right| {
        left.round
            .cmp(&right.round)
            .then_with(|| left.role.cmp(&right.role))
            .then_with(|| left.attempt.cmp(&right.attempt))
            .then_with(|| left.agent.cmp(&right.agent))
    });

    let mut agents = Vec::new();
    for artifact in artifacts {
        if let Some(result) = parse_agent_artifact_result(&artifact) {
            agents.push(result);
        }
    }
    Ok(agents)
}

pub(crate) fn read_agent_artifacts(agent_dir: &Path) -> Result<Vec<SessionArtifact>, String> {
    let agent_dir = checked_data_child_path(agent_dir)?;
    if !agent_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut artifacts = Vec::new();
    for entry in
        fs::read_dir(&agent_dir).map_err(|error| format!("failed to read agent dir: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("failed to read agent artifact entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to read agent artifact type: {error}"))?;
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(artifact) = parse_agent_artifact_name(&agent_dir, name) {
            artifacts.push(artifact);
        }
    }
    Ok(artifacts)
}

pub(crate) fn parse_agent_artifact_name(agent_dir: &Path, name: &str) -> Option<SessionArtifact> {
    let rest = name.strip_prefix("round-")?;
    let (round_text, rest) = rest.split_once('-')?;
    let round = round_text.parse::<usize>().ok()?;
    let mut stem = rest.strip_suffix(".md")?;
    let mut attempt = 1usize;
    if let Some((base, attempt_text)) = stem.rsplit_once("-attempt-") {
        attempt = attempt_text.parse::<usize>().ok()?;
        if attempt < 2 {
            return None;
        }
        stem = base;
    }
    let (agent, role) = stem.rsplit_once('-')?;
    let agent = match agent {
        "claude" | "codex" | "gemini" | "deepseek" | "grok" | "perplexity" => agent,
        _ => return None,
    };
    if !matches!(role, "draft" | "review" | "revision") {
        return None;
    }
    let canonical_name = if attempt == 1 {
        format!("round-{round:03}-{agent}-{role}.md")
    } else {
        format!("round-{round:03}-{agent}-{role}-attempt-{attempt:03}.md")
    };
    if canonical_name != name {
        return None;
    }
    Some(SessionArtifact {
        round,
        attempt,
        agent: agent.to_string(),
        role: role.to_string(),
        path: agent_dir.join(canonical_name),
    })
}

pub(crate) fn parse_agent_artifact_result(
    artifact: &SessionArtifact,
) -> Option<EditorialAgentResult> {
    let text = read_text_file(&artifact.path).ok()?;
    let cli = extract_bullet_code_value(&text, "CLI").unwrap_or_else(|| artifact.agent.clone());
    let status = extract_bullet_code_value(&text, "Status").unwrap_or_else(|| {
        if artifact.role == "draft" || artifact.role == "revision" {
            "DRAFT_CREATED".to_string()
        } else {
            "NOT_READY".to_string()
        }
    });
    let duration_ms = extract_bullet_code_value(&text, "Duration ms")
        .and_then(|value| value.parse::<u128>().ok())
        .unwrap_or(0);
    let exit_code =
        extract_bullet_code_value(&text, "Exit code").and_then(|value| value.parse::<i32>().ok());
    let usage_input_tokens = extract_bullet_code_value(&text, "Usage input tokens")
        .and_then(|value| value.parse::<u64>().ok());
    let usage_output_tokens = extract_bullet_code_value(&text, "Usage output tokens")
        .and_then(|value| value.parse::<u64>().ok());
    let cost_usd =
        extract_bullet_code_value(&text, "Cost USD").and_then(|value| value.parse::<f64>().ok());
    let cache = parse_cache_telemetry_from_artifact(&text);
    let tone = if status == "READY" || status == "DRAFT_CREATED" {
        "ok"
    } else if status == "CLI_NOT_FOUND"
        || status == "API_KEY_NOT_AVAILABLE"
        || status == "REMOTE_SECRET_NOT_READABLE"
    {
        "blocked"
    } else if status.starts_with("EXEC_ERROR")
        || status.starts_with("PROVIDER_")
        || status == "AGENT_FAILED_NO_OUTPUT"
        || status == "AGENT_FAILED_EMPTY"
        || status == "EMPTY_DRAFT"
        || status == "RUNNING"
        || status == "STOPPED_BY_USER"
        || status == "COST_LIMIT_REACHED"
        || status == "CODEX_CLI_NO_FINAL_OUTPUT"
        || status == "CODEX_WINDOWS_SANDBOX_UPSTREAM"
        || status == "GEMINI_CLI_NO_FINAL_OUTPUT"
        || status == "GEMINI_RIPGREP_UNAVAILABLE"
        || status == "GEMINI_WORKSPACE_VIOLATION"
    {
        "error"
    } else {
        "warn"
    };

    Some(EditorialAgentResult {
        name: humanize_agent_name(&artifact.agent),
        role: artifact.role.clone(),
        cli,
        tone: tone.to_string(),
        status,
        duration_ms,
        exit_code,
        output_path: artifact.path.to_string_lossy().to_string(),
        usage_input_tokens,
        usage_output_tokens,
        cost_usd,
        cost_estimated: cost_usd.map(|_| true),
        cache,
    })
}

fn optional_cache_u64(text: &str, label: &str) -> Option<u64> {
    extract_bullet_code_value(text, label)
        .filter(|value| value != "unknown")
        .and_then(|value| value.parse::<u64>().ok())
}

fn parse_cache_telemetry_from_artifact(text: &str) -> Option<ProviderCacheTelemetry> {
    let provider_mode = extract_bullet_code_value(text, "Cache provider mode")?;
    if provider_mode == "none" || provider_mode == "unknown" {
        return None;
    }
    Some(ProviderCacheTelemetry {
        provider_mode,
        cache_key_hash: extract_bullet_code_value(text, "Cache key hash")
            .filter(|value| value != "unknown"),
        cache_control_status: extract_bullet_code_value(text, "Cache control status")
            .filter(|value| value != "unknown"),
        cache_retention: extract_bullet_code_value(text, "Cache retention")
            .filter(|value| value != "unknown"),
        cached_input_tokens: optional_cache_u64(text, "Cache cached input tokens"),
        cache_hit_tokens: optional_cache_u64(text, "Cache hit tokens"),
        cache_miss_tokens: optional_cache_u64(text, "Cache miss tokens"),
        cache_read_input_tokens: optional_cache_u64(text, "Cache read input tokens"),
        cache_creation_input_tokens: optional_cache_u64(text, "Cache creation input tokens"),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        circular_draft_sha256, load_resume_session_state, parse_agent_artifact_name,
        write_circular_review_state, CircularReviewState, CIRCULAR_REVIEW_STATE_FILE,
        CIRCULAR_REVIEW_STATE_SCHEMA_VERSION,
    };
    use crate::{sessions_dir, write_text_file};
    use std::path::PathBuf;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn circular_state_defaults_missing_paid_retry_accounting_for_legacy_v2() {
        let state: CircularReviewState = serde_json::from_str(
            r#"{
                "schema_version": 2,
                "run_id": "legacy-run",
                "current_draft_artifact": "round-001-codex-draft.md",
                "current_draft_author_key": "codex",
                "current_draft_sha256": "digest",
                "round": 1,
                "turn_index": 0,
                "round_roster": ["claude", "codex"],
                "valid_round_agents": [],
                "stable_serial_approval_agents": [],
                "updated_at": "2026-08-21T00:00:00Z"
            }"#,
        )
        .expect("additive paid-retry accounting must remain compatible with v2 state");

        assert!(state.paid_corrective_retries_by_round.is_empty());
        assert!(state.corrective_contract_retry_counts.is_empty());
        assert!(!state.retry_accounting_authoritative);
    }

    #[test]
    fn parse_agent_artifact_name_accepts_append_only_attempt_suffix() {
        let agent_dir = PathBuf::from("agent-runs");
        let artifact =
            parse_agent_artifact_name(&agent_dir, "round-018-codex-revision-attempt-002.md")
                .expect("attempt artifact should parse");

        assert_eq!(artifact.round, 18);
        assert_eq!(artifact.attempt, 2);
        assert_eq!(artifact.agent, "codex");
        assert_eq!(artifact.role, "revision");
        assert_eq!(
            artifact.path,
            agent_dir.join("round-018-codex-revision-attempt-002.md")
        );
    }

    #[test]
    fn parse_agent_artifact_name_rejects_invalid_attempt_suffixes() {
        let agent_dir = PathBuf::from("agent-runs");

        assert!(
            parse_agent_artifact_name(&agent_dir, "round-018-codex-revision-attempt-001.md")
                .is_none()
        );
        assert!(
            parse_agent_artifact_name(&agent_dir, "round-018-codex-revision-attempt-two.md")
                .is_none()
        );
    }

    #[test]
    fn legacy_resume_skips_rejected_and_unchanged_revision_artifacts() {
        let session_dir = sessions_dir().join(format!(
            "maestro-legacy-custody-test-{}",
            std::process::id()
        ));
        let agent_dir = session_dir.join("agent-runs");
        let _ = std::fs::remove_dir_all(&session_dir);
        std::fs::create_dir_all(&agent_dir).unwrap();

        write_test_artifact(
            &agent_dir.join("round-001-claude-draft.md"),
            "DRAFT_CREATED",
            "Texto inicial.",
            false,
        );
        thread::sleep(Duration::from_millis(20));
        let accepted_path = agent_dir.join("round-001-codex-revision-attempt-002.md");
        write_test_artifact(&accepted_path, "READY", "Texto aceito revisado.", true);
        thread::sleep(Duration::from_millis(20));
        write_test_artifact(
            &agent_dir.join("round-001-gemini-revision-attempt-010.md"),
            "READY",
            "Texto aceito revisado.",
            true,
        );
        thread::sleep(Duration::from_millis(20));
        write_test_artifact(
            &agent_dir.join("round-001-perplexity-revision-attempt-014.md"),
            "CONTRACT_VIOLATION",
            "Texto rejeitado que jamais pode assumir custodia.",
            true,
        );

        let state = load_resume_session_state(&agent_dir).unwrap();

        assert_eq!(state.current_draft, "Texto aceito revisado.");
        assert_eq!(state.current_draft_path.as_ref(), Some(&accepted_path));
        assert!(state.circular_state.is_none());
        let _ = std::fs::remove_dir_all(&session_dir);
    }

    #[test]
    fn persisted_circular_state_preserves_custody_and_version_bound_approvals() {
        let run_id = format!("maestro-persisted-custody-test-{}", std::process::id());
        let session_dir = sessions_dir().join(&run_id);
        let agent_dir = session_dir.join("agent-runs");
        let _ = std::fs::remove_dir_all(&session_dir);
        std::fs::create_dir_all(&agent_dir).unwrap();

        let accepted_path = agent_dir.join("round-003-codex-revision-attempt-006.md");
        write_test_artifact(&accepted_path, "READY", "Versao corrente aceita.", true);
        write_test_artifact(
            &agent_dir.join("round-003-perplexity-revision-attempt-014.md"),
            "CONTRACT_VIOLATION",
            "Versao rejeitada posterior.",
            true,
        );
        write_circular_review_state(
            &session_dir,
            &CircularReviewState {
                schema_version: CIRCULAR_REVIEW_STATE_SCHEMA_VERSION,
                run_id,
                current_draft_artifact: "round-003-codex-revision-attempt-006.md".to_string(),
                current_draft_author_key: "codex".to_string(),
                current_draft_sha256: circular_draft_sha256("Versao corrente aceita."),
                round: 3,
                turn_index: 4,
                round_roster: vec![
                    "claude".to_string(),
                    "codex".to_string(),
                    "gemini".to_string(),
                    "deepseek".to_string(),
                    "grok".to_string(),
                    "perplexity".to_string(),
                ],
                valid_round_agents: vec![
                    "gemini".to_string(),
                    "deepseek".to_string(),
                    "grok".to_string(),
                ],
                stable_serial_approval_agents: vec![
                    "gemini".to_string(),
                    "deepseek".to_string(),
                    "grok".to_string(),
                ],
                paid_corrective_retries_by_round: std::collections::BTreeMap::from([(3, 2)]),
                corrective_contract_retry_counts: std::collections::BTreeMap::from([(
                    format!(
                        "3:4:gemini:{}",
                        circular_draft_sha256("Versao corrente aceita.")
                    ),
                    1,
                )]),
                retry_accounting_authoritative: true,
                updated_at: "2026-07-25T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let state = load_resume_session_state(&agent_dir).unwrap();
        let circular = state
            .circular_state
            .expect("persisted circular state should be restored");

        assert_eq!(state.current_draft, "Versao corrente aceita.");
        assert_eq!(state.current_draft_path.as_ref(), Some(&accepted_path));
        assert_eq!(
            circular.stable_serial_approval_agents,
            vec!["gemini", "deepseek", "grok"]
        );
        assert_eq!(circular.paid_corrective_retries_by_round.get(&3), Some(&2));
        assert_eq!(circular.corrective_contract_retry_counts.len(), 1);
        assert!(circular.retry_accounting_authoritative);
        let _ = std::fs::remove_dir_all(&session_dir);
    }

    #[test]
    fn persisted_circular_state_ignores_later_artifact_not_committed_to_state() {
        let run_id = format!("maestro-crash-window-test-{}", std::process::id());
        let session_dir = sessions_dir().join(&run_id);
        let agent_dir = session_dir.join("agent-runs");
        let _ = std::fs::remove_dir_all(&session_dir);
        std::fs::create_dir_all(&agent_dir).unwrap();

        let committed_path = agent_dir.join("round-002-codex-revision-attempt-003.md");
        write_test_artifact(&committed_path, "READY", "Versao confirmada.", true);
        write_circular_review_state(
            &session_dir,
            &CircularReviewState {
                schema_version: CIRCULAR_REVIEW_STATE_SCHEMA_VERSION,
                run_id,
                current_draft_artifact: "round-002-codex-revision-attempt-003.md".to_string(),
                current_draft_author_key: "codex".to_string(),
                current_draft_sha256: circular_draft_sha256("Versao confirmada."),
                round: 2,
                turn_index: 3,
                round_roster: vec![
                    "claude".to_string(),
                    "codex".to_string(),
                    "gemini".to_string(),
                    "deepseek".to_string(),
                    "grok".to_string(),
                    "perplexity".to_string(),
                ],
                valid_round_agents: vec!["gemini".to_string()],
                stable_serial_approval_agents: vec!["gemini".to_string()],
                paid_corrective_retries_by_round: std::collections::BTreeMap::new(),
                corrective_contract_retry_counts: std::collections::BTreeMap::new(),
                retry_accounting_authoritative: true,
                updated_at: "2026-07-25T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        // Simulate a crash after the next peer artifact is durable but before
        // the accepted-state snapshot advances to that artifact.
        write_test_artifact(
            &agent_dir.join("round-002-claude-revision-attempt-004.md"),
            "READY",
            "Versao posterior ainda nao confirmada pelo estado.",
            true,
        );

        let state = load_resume_session_state(&agent_dir).unwrap();
        let circular = state
            .circular_state
            .expect("the last committed circular state must remain authoritative");

        assert_eq!(state.current_draft, "Versao confirmada.");
        assert_eq!(state.current_draft_path.as_ref(), Some(&committed_path));
        assert_eq!(circular.current_draft_author_key, "codex");
        assert_eq!(
            circular.stable_serial_approval_agents,
            vec!["gemini".to_string()]
        );
        let _ = std::fs::remove_dir_all(&session_dir);
    }

    #[test]
    fn corrupt_circular_state_fails_closed_instead_of_falling_back_to_artifacts() {
        let run_id = format!("maestro-corrupt-state-test-{}", std::process::id());
        let session_dir = sessions_dir().join(&run_id);
        let agent_dir = session_dir.join("agent-runs");
        let _ = std::fs::remove_dir_all(&session_dir);
        std::fs::create_dir_all(&agent_dir).unwrap();

        write_test_artifact(
            &agent_dir.join("round-001-codex-draft.md"),
            "DRAFT_CREATED",
            "Artefato legado que nao pode contornar estado corrompido.",
            false,
        );
        write_text_file(
            &session_dir.join(CIRCULAR_REVIEW_STATE_FILE),
            "{\"schema_version\":",
        )
        .unwrap();

        let error = match load_resume_session_state(&agent_dir) {
            Ok(_) => panic!("invalid authoritative state must stop resume"),
            Err(error) => error,
        };
        assert!(error.contains("circular review state is invalid JSON"));
        let _ = std::fs::remove_dir_all(&session_dir);
    }

    fn write_test_artifact(path: &std::path::Path, status: &str, text: &str, revision: bool) {
        let stdout = if revision {
            format!(
                "MAESTRO_STATUS: READY\n<maestro_revision_report>{{\"custody\":\"revised\",\"changes\":[]}}</maestro_revision_report>\n<maestro_final_text>{text}</maestro_final_text>"
            )
        } else {
            text.to_string()
        };
        write_text_file(
            path,
            &format!(
                "# Test\n\n- CLI: `test`\n- Status: `{status}`\n- Exit code: `0`\n- Duration ms: `1`\n\n## Stdout\n\n```text\n{stdout}\n```\n"
            ),
        )
        .unwrap();
    }
}
