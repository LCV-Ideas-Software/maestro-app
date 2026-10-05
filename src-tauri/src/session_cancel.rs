// Modulo: src-tauri/src/session_cancel.rs
// Descricao: Per-run-id cancellation token registry for editorial sessions
// shipped in v0.5.0 to support the operator-driven "Stop session" button.
//
// Rationale: a long editorial session may run for many minutes (Claude/Codex/
// Google/Antigravity CLI peers regularly take 3-7 minutes each in real operator logs).
// Pre-v0.5.0 the only way to abort was killing the entire app. v0.5.0 wires a
// `tokio_util::sync::CancellationToken` per run_id so the operator can press
// "Parar sessao" in the UI and:
//
//   - CLI peer in flight: `command_spawn::run_resolved_command_observed` polls
//     the token every 250ms and invokes `kill_process_tree` when fired (cancel
//     resolves in <500ms).
//   - API peer in flight: `provider_retry::send_with_retry_async` wraps
//     `client.send()` in `tokio::select!` against `cancel.cancelled()` so the
//     reqwest future is dropped and the connection closed (cancel resolves in
//     <2s, bounded by network round-trip).
//   - Between rounds: `run_editorial_session_core` checks `is_cancelled()` and
//     returns early with status `STOPPED_BY_USER`, leaving artifacts intact for
//     resume via `FinalizeRunningArtifactsGuard` (Drop semantics from v0.3.16).
//
// Threading model: the static `SESSION_CANCEL` map is keyed by run_id so
// concurrent sessions (rare in single-operator desktop, but possible) each get
// their own token. The `Tauri` `stop_editorial_session` command is sync (sets
// the flag immediately, returns a bool indicating whether a matching run_id
// was found) so it never blocks even if the session loop is mid-API-call.
//
// Idempotency: `signal_session_cancel` returns false for unknown run_ids
// (already finished, never started, typo) without erroring. The operator's UI
// surfaces the bool but treats both cases as "stop request acknowledged".

use std::collections::HashMap;
use std::fs::{self, File, TryLockError};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use tokio_util::sync::CancellationToken;

use crate::app_paths::{checked_data_child_path, sanitize_path_segment, sessions_dir};

static SESSION_CANCEL: OnceLock<Mutex<HashMap<String, CancellationToken>>> = OnceLock::new();

fn cancel_map() -> &'static Mutex<HashMap<String, CancellationToken>> {
    SESSION_CANCEL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a cancellation token for the given run_id and return a clone
/// usable by the session loop. A live registration cannot be replaced:
/// cancellation and guard cleanup must continue to address its owner.
fn register_session_cancel(run_id: &str) -> Result<CancellationToken, String> {
    let token = CancellationToken::new();
    let mut guard = cancel_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.entry(run_id.to_string()) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(token.clone());
            Ok(token)
        }
        std::collections::hash_map::Entry::Occupied(_) => {
            Err("session is already running".to_string())
        }
    }
}

fn acquire_session_file_lock(session_dir: &Path) -> Result<File, String> {
    let session_dir = checked_data_child_path(session_dir)?;
    fs::create_dir_all(&session_dir)
        .map_err(|error| format!("failed to create session custody directory: {error}"))?;
    let lock_path = checked_data_child_path(&session_dir.join(".editorial-custody"))?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| format!("failed to open session custody handle: {error}"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err("session is already running".to_string()),
        Err(TryLockError::Error(error)) => {
            Err(format!("failed to acquire session custody: {error}"))
        }
    }
}

/// The native OS lock excludes another worker or app process. File Drop
/// releases custody even after an error or panic; no stale-file cleanup is
/// needed. Remove the process-local token before releasing OS custody.
pub(crate) struct SessionExecutionGuard {
    _cancel_guard: CancelTokenGuard,
    _session_lock: File,
}

pub(crate) fn acquire_session_execution(
    run_id: &str,
) -> Result<(CancellationToken, SessionExecutionGuard), String> {
    if run_id.is_empty() || sanitize_path_segment(run_id, 120) != run_id {
        return Err("invalid session run_id".to_string());
    }
    let session_lock = acquire_session_file_lock(&sessions_dir().join(run_id))?;
    let token = register_session_cancel(run_id)?;
    Ok((
        token,
        SessionExecutionGuard {
            _cancel_guard: CancelTokenGuard::new(run_id.to_string()),
            _session_lock: session_lock,
        },
    ))
}

pub(crate) fn session_execution_is_active(session_dir: &Path) -> Result<bool, String> {
    let lock_path = checked_data_child_path(&session_dir.join(".editorial-custody"))?;
    let file = match File::options().read(true).write(true).open(&lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("failed to inspect session custody: {error}")),
    };
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(error)) => {
            Err(format!("failed to inspect session custody: {error}"))
        }
    }
}

/// Signal cancellation for the given run_id. Returns true if a matching
/// token was found and signaled, false otherwise (idempotent: repeated calls
/// or unknown run_ids return false without erroring).
pub(crate) fn signal_session_cancel(run_id: &str) -> bool {
    let guard = cancel_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(token) = guard.get(run_id) {
        token.cancel();
        true
    } else {
        false
    }
}

/// Remove the cancellation token for a run_id. Should be called by the
/// session loop when the session completes (success, failure, or cancellation)
/// so the static map does not grow unbounded across many sessions.
pub(crate) fn unregister_session_cancel(run_id: &str) {
    let mut guard = cancel_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.remove(run_id);
}

/// RAII guard ensuring the cancellation token entry is removed even if the
/// session loop panics or returns early. Constructed by the session loop
/// after `register_session_cancel`; dropped at session end.
pub(crate) struct CancelTokenGuard {
    run_id: String,
}

impl CancelTokenGuard {
    pub(crate) fn new(run_id: String) -> Self {
        Self { run_id }
    }
}

impl Drop for CancelTokenGuard {
    fn drop(&mut self) {
        unregister_session_cancel(&self.run_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_unknown_run_id_returns_false() {
        // Use a synthetic run_id that no real session would have to avoid
        // colliding with concurrent tests in the same process.
        let id = "test-unknown-run-id-7d8a9c2e";
        assert!(!signal_session_cancel(id));
    }

    #[test]
    fn register_then_signal_then_unregister_roundtrips() {
        let id = "test-register-roundtrip-8e9b0f1d";
        let token = register_session_cancel(id).unwrap();
        assert!(!token.is_cancelled(), "fresh token must not be cancelled");
        assert!(
            signal_session_cancel(id),
            "signal must succeed for registered run_id"
        );
        assert!(token.is_cancelled(), "token must be cancelled after signal");
        unregister_session_cancel(id);
        assert!(
            !signal_session_cancel(id),
            "second signal after unregister must return false"
        );
    }

    #[test]
    fn signal_is_idempotent_after_first_cancel() {
        let id = "test-idempotent-cancel-2a3b4c5d";
        let _token = register_session_cancel(id).unwrap();
        assert!(signal_session_cancel(id));
        assert!(
            signal_session_cancel(id),
            "second signal must still return true while token registered"
        );
        unregister_session_cancel(id);
    }

    #[test]
    fn cancel_token_guard_unregisters_on_drop() {
        let id = "test-guard-drop-9f0e1d2c";
        let _token = register_session_cancel(id).unwrap();
        {
            let _guard = CancelTokenGuard::new(id.to_string());
            assert!(signal_session_cancel(id));
        }
        assert!(
            !signal_session_cancel(id),
            "guard Drop must unregister so subsequent signal returns false"
        );
    }

    #[test]
    fn guard_drop_runs_on_panic() {
        // Anti-regression: ensures the cancel registry stays clean even when
        // the session loop panics mid-flight.
        let id = "test-guard-panic-3a4b5c6d";
        let _token = register_session_cancel(id).unwrap();
        let result = std::panic::catch_unwind(|| {
            let _guard = CancelTokenGuard::new(id.to_string());
            panic!("synthetic panic for Drop semantics test");
        });
        assert!(result.is_err());
        assert!(
            !signal_session_cancel(id),
            "guard Drop must run on panic so subsequent signal returns false"
        );
    }

    #[test]
    fn duplicate_registration_preserves_the_original_cancellation_owner() {
        let id = "test-duplicate-registration-b93c2d";
        let original = register_session_cancel(id).unwrap();
        let original_guard = CancelTokenGuard::new(id.to_string());
        assert!(register_session_cancel(id).is_err());
        assert!(signal_session_cancel(id));
        assert!(original.is_cancelled());
        drop(original_guard);
        assert!(!signal_session_cancel(id));
        let replacement = register_session_cancel(id).unwrap();
        assert!(!replacement.is_cancelled());
        unregister_session_cancel(id);
    }

    fn custody_fixture_id(label: &str) -> String {
        format!(
            "test-custody-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    #[test]
    fn session_custody_and_token_are_released_after_a_panicking_worker() {
        let id = custody_fixture_id("panic");
        let dir = sessions_dir().join(&id);
        let result = std::panic::catch_unwind(|| {
            let (token, _guard) = acquire_session_execution(&id).unwrap();
            assert!(session_execution_is_active(&dir).unwrap());
            assert!(acquire_session_execution(&id).is_err());
            assert!(signal_session_cancel(&id));
            assert!(token.is_cancelled());
            panic!("synthetic worker panic");
        });
        assert!(result.is_err());
        assert!(!session_execution_is_active(&dir).unwrap());
        assert!(!signal_session_cancel(&id));
        let (token, guard) = acquire_session_execution(&id).unwrap();
        assert!(!token.is_cancelled());
        drop(guard);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn another_app_process_cannot_acquire_an_active_sessions_custody() {
        let id = custody_fixture_id("process");
        let dir = sessions_dir().join(&id);
        let (_token, guard) = acquire_session_execution(&id).unwrap();
        let child = crate::hidden_command(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "session_cancel::tests::session_lock_child_fixture",
            ])
            .env("MAESTRO_SESSION_CUSTODY_FIXTURE_DIR", &dir)
            .output()
            .unwrap();
        drop(guard);
        let reacquired = acquire_session_file_lock(&dir).unwrap();
        drop(reacquired);
        let inactive = session_execution_is_active(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert!(
            child.status.success(),
            "separate-process custody check failed: {} {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr),
        );
        assert!(!inactive);
        assert!(!signal_session_cancel(&id));
    }

    #[test]
    #[ignore = "invoked as a separate-process custody fixture by the parent test"]
    fn session_lock_child_fixture() {
        let dir = std::env::var_os("MAESTRO_SESSION_CUSTODY_FIXTURE_DIR").unwrap();
        let dir = std::path::PathBuf::from(dir);
        assert!(session_execution_is_active(&dir).unwrap());
        let error = acquire_session_file_lock(&dir).unwrap_err();
        assert_eq!(error, "session is already running");
    }
}
