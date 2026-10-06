// Modulo: src-tauri/src/command_spawn.rs
// Descricao: Child-process spawn machinery (timeout, progress logging, pipe
// readers, command builders, environment policy) extracted from lib.rs in
// v0.3.35 per `docs/code-split-plan.md` migration step 5.
//
// What's here (8 items):
//   - `CommandProgressContext<'a>` — per-spawn context (log_session,
//     run_id, agent, role, cli, output_path) used to emit
//     `session.agent.spawned` / `session.agent.running` NDJSON entries.
//   - `command_check` — diagnostic helper called by `dependency_preflight`
//     to verify each peer CLI is on PATH and answers `--version`.
//   - `run_resolved_command_with_timeout` — convenience wrapper that
//     forwards to `run_resolved_command_observed` with no progress context.
//   - `run_resolved_command_observed` — the heavy spawn loop: builds the
//     command via `resolved_command_builder`, sets working dir from the
//     progress's output_path (or `app_root()` fallback), spawns, drains
//     stdout/stderr in 2 reader threads with byte counters, polls every
//     250ms, emits `session.agent.running` every 30s, honors optional
//     timeout, and returns `TimedCommandOutput`.
//     Antigravity CLI (`agy`) is routed through a PTY because its print mode
//     writes through terminal APIs that are not reliably captured by pipes.
//   - `read_pipe_to_end_counting_classified` — pipe reader that increments
//     a shared atomic byte counter and classifies any I/O error.
//   - `classify_pipe_error` — Windows-aware classifier (raw_os_error 109/
//     232/233 + std ErrorKind variants).
//   - `resolved_command_builder` — Windows: lets Rust's native process API
//     escape `.cmd`/`.bat` arguments and routes `.ps1` through `powershell.exe -NoProfile
//     -ExecutionPolicy Bypass -File`; everything else via `hidden_command`.
//     Always applies `apply_editorial_agent_environment`.
//   - `apply_editorial_agent_environment` — sets UTF-8 (`PYTHONIOENCODING`/
//     `PYTHONUTF8`/`LC_ALL`/`LANG`) and terminal policy on every child.
//
// What stays in lib.rs (consumed via `pub(crate)` imports):
//   - `TimedCommandOutput` struct (pub(crate) since v0.3.35 with all 5
//     fields).
//   - `hidden_command` (pub(crate)) — only entry point that funnels through
//     `apply_hidden_window_policy` per the v0.3.16 `clippy.toml`
//     `disallowed-methods` policy on `Command::new`.
//   - `app_root` (already pub(crate)).
//   - `command_working_dir_for_output` (pub(crate)) — wrapper around the
//     output_path's parent dir.
//   - `log_editorial_agent_spawned`, `log_editorial_agent_running` (both
//     pub(crate)) — NDJSON helpers tightly coupled with the editorial
//     orchestration log schema.
//   - `sanitize_text` (already pub(crate) via v0.3.34 re-export).
//   - `resolve_command` from `crate::command_path` (pub(crate) since v0.3.33).
//
// v0.3.35 is a pure move: every signature, format string, sleep cadence,
// 30-second progress interval, and Windows error code is identical to the
// v0.3.34 lib.rs source (commit e00538e).

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::command_path::{command_search_dirs, resolve_command};
use crate::logging::LogSession;
use crate::{
    app_root, command_working_dir_for_output, hidden_command, log_editorial_agent_running,
    log_editorial_agent_spawned, sanitize_text, CommandExitStatus, CommandOutput,
    TimedCommandOutput,
};

/// Cap per-pipe buffer at 64 MiB. Beyond the cap, bytes continue to be drained
/// from the pipe (so the child does not block on a full pipe), but they are not
/// retained; the pipe_error field gets the truncation marker so the artifact
/// surfaces the cause.
pub(crate) const MAX_PIPE_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) fn command_check(label: &str, command: &str, args: &[&str]) -> Value {
    let Some(path) = resolve_command(command) else {
        return json!({
            "label": label,
            "value": "nao encontrado no PATH efetivo",
            "tone": "blocked"
        });
    };
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    // Version diagnostics need ordinary stdout, including agy metadata. The
    // PTY transport is reserved for editorial calls that require a terminal.
    let output = run_resolved_command_observed_piped(
        &path,
        &args,
        Some(Duration::from_secs(12)),
        None,
        None,
        None,
        None,
    );

    match output {
        Ok(result) if result.timed_out => json!({
            "label": label,
            "value": sanitize_text("diagnostico excedeu 12s; CLI pode exigir login ou inicializacao lenta", 220),
            "tone": "warn"
        }),
        Ok(result) if result.output.status.success() => {
            let stdout = String::from_utf8_lossy(&result.output.stdout);
            let stderr = String::from_utf8_lossy(&result.output.stderr);
            let detail = stdout
                .lines()
                .chain(stderr.lines())
                .find(|line| !line.trim().is_empty())
                .unwrap_or("detectado")
                .trim();
            let resolved_note = format!(" via {}", path.to_string_lossy());
            json!({
                "label": label,
                "value": sanitize_text(&format!("{detail}{resolved_note}"), 220),
                "tone": "ok"
            })
        }
        Ok(result) => {
            let stderr = String::from_utf8_lossy(&result.output.stderr);
            let stdout = String::from_utf8_lossy(&result.output.stdout);
            let detail = stderr
                .lines()
                .chain(stdout.lines())
                .find(|line| !line.trim().is_empty())
                .unwrap_or("comando retornou falha")
                .trim();
            json!({
                "label": label,
                "value": sanitize_text(detail, 220),
                "tone": "warn"
            })
        }
        Err(error) => json!({
            "label": label,
            "value": sanitize_text(&format!("nao encontrado/executado: {error}"), 220),
            "tone": "blocked"
        }),
    }
}

pub(crate) struct CommandProgressContext<'a> {
    pub(crate) log_session: &'a LogSession,
    pub(crate) run_id: &'a str,
    pub(crate) agent: &'a str,
    pub(crate) role: &'a str,
    pub(crate) cli: &'a str,
    pub(crate) output_path: &'a Path,
}

pub(crate) fn run_resolved_command_with_timeout(
    path: &Path,
    args: &[String],
    timeout: Duration,
    stdin_text: Option<&str>,
) -> std::io::Result<TimedCommandOutput> {
    run_resolved_command_observed(path, args, Some(timeout), stdin_text, None, None)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_resolved_command_observed(
    path: &Path,
    args: &[String],
    timeout: Option<Duration>,
    stdin_text: Option<&str>,
    progress: Option<CommandProgressContext<'_>>,
    cancel_token: Option<&CancellationToken>,
) -> std::io::Result<TimedCommandOutput> {
    if uses_pty_transport(path) {
        #[cfg(windows)]
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ps1"))
        {
            // Windows PowerShell startup requests terminal cursor responses
            // that this capture transport does not provide. Fail explicitly
            // rather than admit a shim that cannot complete its prompt turn.
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "PTY PowerShell launchers are unsupported for editorial prompts; install the provider's official native executable",
            ));
        }
        #[cfg(windows)]
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
            })
        {
            // portable-pty encodes Windows arguments for native executables,
            // not cmd.exe's shell grammar. Preserve the requested transport
            // explicitly: require the official native CLI rather than execute
            // an unescaped prompt or silently switch providers/transports.
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "PTY batch launchers cannot safely carry editorial prompts; install the provider's official native executable",
            ));
        }
        return run_resolved_command_observed_pty(
            path,
            args,
            timeout,
            stdin_text,
            progress,
            cancel_token,
        );
    }

    run_resolved_command_observed_piped(
        path,
        args,
        timeout,
        stdin_text,
        progress,
        cancel_token,
        None,
    )
}

/// Native metadata commands require structured stdout rather than the
/// provider's interactive PTY. This uses the same managed pipe lifecycle and
/// permits the caller to bind metadata and subsequent prompt to one directory.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_resolved_command_observed_piped(
    path: &Path,
    args: &[String],
    timeout: Option<Duration>,
    stdin_text: Option<&str>,
    progress: Option<CommandProgressContext<'_>>,
    cancel_token: Option<&CancellationToken>,
    working_dir_override: Option<&Path>,
) -> std::io::Result<TimedCommandOutput> {
    if cancel_token.is_some_and(CancellationToken::is_cancelled) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "command cancelled before launch",
        ));
    }
    let started = Instant::now();
    let mut command = resolved_command_builder(path, args);
    let working_dir = working_dir_override
        .map(Path::to_path_buf)
        .or_else(|| {
            progress
                .as_ref()
                .map(|progress| command_working_dir_for_output(progress.output_path))
        })
        .unwrap_or_else(app_root);
    command
        .current_dir(&working_dir)
        .stdin(if stdin_text.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (mut child, process_job) = spawn_managed_piped_command(&mut command)?;
    let child_id = child.id();
    if let Some(progress) = progress.as_ref() {
        log_editorial_agent_spawned(progress, child_id, path, &working_dir);
    }
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let mut io_handles = match PipeCancellationHandles::new(
        stdout.as_ref(),
        stderr.as_ref(),
        child.stdin.as_ref(),
    ) {
        Ok(handles) => handles,
        Err(error) => {
            kill_managed_piped_child(&mut child, &process_job);
            let _ = child.wait();
            return Err(error);
        }
    };
    let stdout_bytes = Arc::new(AtomicU64::new(0));
    let stderr_bytes = Arc::new(AtomicU64::new(0));
    let stdout_counter = Arc::clone(&stdout_bytes);
    let stderr_counter = Arc::clone(&stderr_bytes);
    let io_cancel = CancellationToken::new();
    let stdout_cancel = io_cancel.clone();
    let stderr_cancel = io_cancel.clone();
    let stdout_handle = thread::spawn(move || {
        read_pipe_with_cancellation(stdout, stdout_counter, Some(&stdout_cancel))
    });
    let stderr_handle = thread::spawn(move || {
        read_pipe_with_cancellation(stderr, stderr_counter, Some(&stderr_cancel))
    });
    // Drain both output pipes before sending input. A peer may write its
    // preamble before reading the prompt, so a synchronous write here can
    // deadlock on two full pipes and prevent timeout/cancellation polling.
    let mut stdin_handle = stdin_text.map(|text| {
        let bytes = text.as_bytes().to_vec();
        let stdin = child.stdin.take();
        let writer_cancel = io_cancel.clone();
        thread::spawn(move || {
            if let Some(mut stdin) = stdin {
                let mut offset = 0;
                while offset < bytes.len() {
                    if writer_cancel.is_cancelled() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "stdin delivery cancelled",
                        ));
                    }
                    match stdin.write(&bytes[offset..]) {
                        Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                        Ok(count) => offset += count,
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => return Err(error),
                    }
                }
            }
            Ok(())
        })
    });
    let mut last_progress = Instant::now();

    loop {
        if stdin_handle
            .as_ref()
            .is_some_and(thread::JoinHandle::is_finished)
        {
            // The duplicate write handle must close with its writer so a peer
            // reading stdin to EOF can observe the end of the delivered prompt.
            io_handles.release_stdin();
            if let Err(error) = finish_stdin_writer(stdin_handle.take()) {
                kill_managed_piped_child(&mut child, &process_job);
                let _ = child.wait();
                io_cancel.cancel();
                wait_for_io_threads(
                    &stdout_handle,
                    &stderr_handle,
                    None,
                    &io_cancel,
                    &mut io_handles,
                    started,
                    timeout,
                    cancel_token,
                    &process_job,
                );
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(error);
            }
        }
        if let Some(status) = child.try_wait()? {
            let interrupted = wait_for_io_threads(
                &stdout_handle,
                &stderr_handle,
                stdin_handle.as_ref(),
                &io_cancel,
                &mut io_handles,
                started,
                timeout,
                cancel_token,
                &process_job,
            );
            if interrupted {
                let _ = finish_stdin_writer(stdin_handle.take());
            } else {
                finish_stdin_writer(stdin_handle.take())?;
            }
            let (stdout, stdout_pipe_error) = stdout_handle
                .join()
                .unwrap_or_else(|_| (Vec::new(), Some("stdout_thread_panic".to_string())));
            let (stderr, stderr_pipe_error) = stderr_handle
                .join()
                .unwrap_or_else(|_| (Vec::new(), Some("stderr_thread_panic".to_string())));
            return Ok(TimedCommandOutput {
                output: CommandOutput {
                    status: CommandExitStatus::from_std(status),
                    stdout,
                    stderr,
                },
                duration_ms: started.elapsed().as_millis(),
                timed_out: interrupted,
                stdout_pipe_error,
                stderr_pipe_error,
            });
        }

        if let Some(timeout) = timeout {
            if started.elapsed() >= timeout {
                kill_managed_piped_child(&mut child, &process_job);
                let status = child.wait()?;
                io_cancel.cancel();
                wait_for_io_threads(
                    &stdout_handle,
                    &stderr_handle,
                    stdin_handle.as_ref(),
                    &io_cancel,
                    &mut io_handles,
                    started,
                    Some(timeout),
                    cancel_token,
                    &process_job,
                );
                let _ = finish_stdin_writer(stdin_handle.take());
                let (stdout, stdout_pipe_error) = stdout_handle
                    .join()
                    .unwrap_or_else(|_| (Vec::new(), Some("stdout_thread_panic".to_string())));
                let (stderr, stderr_pipe_error) = stderr_handle
                    .join()
                    .unwrap_or_else(|_| (Vec::new(), Some("stderr_thread_panic".to_string())));
                return Ok(TimedCommandOutput {
                    output: CommandOutput {
                        status: CommandExitStatus::from_std(status),
                        stdout,
                        stderr,
                    },
                    duration_ms: started.elapsed().as_millis(),
                    timed_out: true,
                    stdout_pipe_error,
                    stderr_pipe_error,
                });
            }
        }

        // Cancellation check fires every 250ms (next loop iteration). When
        // operator presses "Stop session" the token is signaled; we kill the
        // child process tree and return with `timed_out: true` so the caller
        // surfaces it as a STOPPED_BY_USER artifact.
        if let Some(token) = cancel_token {
            if token.is_cancelled() {
                kill_managed_piped_child(&mut child, &process_job);
                let status = child.wait()?;
                io_cancel.cancel();
                wait_for_io_threads(
                    &stdout_handle,
                    &stderr_handle,
                    stdin_handle.as_ref(),
                    &io_cancel,
                    &mut io_handles,
                    started,
                    timeout,
                    cancel_token,
                    &process_job,
                );
                let _ = finish_stdin_writer(stdin_handle.take());
                let (stdout, stdout_pipe_error) = stdout_handle
                    .join()
                    .unwrap_or_else(|_| (Vec::new(), Some("stdout_thread_panic".to_string())));
                let (stderr, stderr_pipe_error) = stderr_handle
                    .join()
                    .unwrap_or_else(|_| (Vec::new(), Some("stderr_thread_panic".to_string())));
                return Ok(TimedCommandOutput {
                    output: CommandOutput {
                        status: CommandExitStatus::from_std(status),
                        stdout,
                        stderr,
                    },
                    duration_ms: started.elapsed().as_millis(),
                    timed_out: true,
                    stdout_pipe_error,
                    stderr_pipe_error,
                });
            }
        }

        if last_progress.elapsed() >= Duration::from_secs(30) {
            if let Some(progress) = progress.as_ref() {
                log_editorial_agent_running(
                    progress,
                    child_id,
                    started.elapsed(),
                    stdout_bytes.load(Ordering::Relaxed),
                    stderr_bytes.load(Ordering::Relaxed),
                );
            }
            last_progress = Instant::now();
        }

        thread::sleep(Duration::from_millis(250));
    }
}

type PipeReaderResult = (Vec<u8>, Option<String>);

struct ManagedProcessJob {
    #[cfg(windows)]
    handle: std::os::windows::io::OwnedHandle,
}

impl ManagedProcessJob {
    #[cfg(windows)]
    fn new() -> std::io::Result<Self> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: null security attributes make this anonymous handle
        // noninheritable; OwnedHandle closes it on every error/success exit.
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the buffer is a live correctly sized native structure.
        if unsafe {
            SetInformationJobObject(
                handle.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    fn terminate(&self) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            // SAFETY: this owned handle refers only to this invocation's job.
            if unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(
                    self.handle.as_raw_handle(),
                    1,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

fn spawn_managed_piped_command(
    command: &mut Command,
) -> std::io::Result<(Child, ManagedProcessJob)> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
        let job = ManagedProcessJob::new()?;
        // Preserve the hidden-window policy and Rust's native argument/batch
        // escaping. The new primary thread cannot execute before assignment.
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        let mut child = command.spawn()?;
        // SAFETY: Child and job own their process/job handles throughout this
        // call. No BREAKAWAY or UI limits relax normal inherited job custody.
        let assigned =
            unsafe { AssignProcessToJobObject(job.handle.as_raw_handle(), child.as_raw_handle()) };
        let admitted = if assigned == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            resume_suspended_primary_thread(&child)
        };
        if let Err(error) = admitted {
            let _ = job.terminate();
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok((child, job))
    }
    #[cfg(not(windows))]
    {
        Ok((command.spawn()?, ManagedProcessJob {}))
    }
}

#[cfg(windows)]
fn resume_suspended_primary_thread(child: &Child) -> std::io::Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessIdOfThread, OpenThread, ResumeThread, THREAD_QUERY_LIMITED_INFORMATION,
        THREAD_SUSPEND_RESUME,
    };
    // Stable Rust exposes the process handle, but not the primary thread.
    // A never-resumed child must have exactly one native thread; its held
    // process handle prevents PID reuse while the snapshot is checked.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut found = None;
    let mut next = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    while next != 0 {
        if entry.dwSize < std::mem::size_of::<THREADENTRY32>() as u32 {
            return Err(std::io::Error::other(
                "native thread snapshot entry is incomplete",
            ));
        }
        if entry.th32OwnerProcessID == child.id() {
            if found.replace(entry.th32ThreadID).is_some() {
                return Err(std::io::Error::other(
                    "suspended child has multiple native threads",
                ));
            }
        }
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        next = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(error);
    }
    let thread_id =
        found.ok_or_else(|| std::io::Error::other("suspended child has no native thread"))?;
    let raw = unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
            0,
            thread_id,
        )
    };
    if raw.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let thread = unsafe { OwnedHandle::from_raw_handle(raw) };
    if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != child.id() {
        return Err(std::io::Error::other(
            "native thread does not belong to suspended child",
        ));
    }
    let previous = unsafe { ResumeThread(thread.as_raw_handle()) };
    if previous == u32::MAX {
        return Err(std::io::Error::last_os_error());
    }
    if previous != 1 {
        return Err(std::io::Error::other(
            "native primary thread suspend count is unexpected",
        ));
    }
    Ok(())
}

fn kill_managed_piped_child(child: &mut Child, job: &ManagedProcessJob) {
    #[cfg(windows)]
    {
        // Job termination also addresses descendants after the direct child
        // exits. Last-handle close remains an error-path cleanup safeguard.
        let _ = job.terminate();
        let _ = child.kill();
    }
    #[cfg(not(windows))]
    {
        let _ = job;
        kill_process_tree(child);
    }
}

struct PipeCancellationHandles {
    #[cfg(windows)]
    handles: Vec<std::os::windows::io::OwnedHandle>,
    #[cfg(windows)]
    stdin: Option<std::os::windows::io::OwnedHandle>,
}

impl PipeCancellationHandles {
    fn new(
        stdout: Option<&std::process::ChildStdout>,
        stderr: Option<&std::process::ChildStderr>,
        stdin: Option<&std::process::ChildStdin>,
    ) -> std::io::Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsHandle;
            let mut handles = Vec::new();
            if let Some(stdout) = stdout {
                handles.push(stdout.as_handle().try_clone_to_owned()?);
            }
            if let Some(stderr) = stderr {
                handles.push(stderr.as_handle().try_clone_to_owned()?);
            }
            let stdin = stdin
                .map(|stdin| stdin.as_handle().try_clone_to_owned())
                .transpose()?;
            Ok(Self { handles, stdin })
        }
        #[cfg(not(windows))]
        {
            let _ = (stdout, stderr, stdin);
            Ok(Self {})
        }
    }

    fn release_stdin(&mut self) {
        #[cfg(windows)]
        {
            self.stdin.take();
        }
    }

    fn cancel_pending_io(&self) {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            for handle in self.handles.iter().chain(self.stdin.iter()) {
                // SAFETY: each duplicate refers to the same pipe object as its
                // dedicated worker's original handle and remains owned through
                // all worker joins. A null OVERLAPPED cancels all outstanding
                // operations on this pipe, including Rust's ReadFileEx/WriteFileEx.
                // ERROR_NOT_FOUND is a completion/between-call race; the token
                // prevents a new call and the parent retries while still pending.
                let _ = unsafe {
                    windows_sys::Win32::System::IO::CancelIoEx(
                        handle.as_raw_handle(),
                        std::ptr::null(),
                    )
                };
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn wait_for_io_threads(
    stdout: &thread::JoinHandle<PipeReaderResult>,
    stderr: &thread::JoinHandle<PipeReaderResult>,
    stdin: Option<&thread::JoinHandle<std::io::Result<()>>>,
    io_cancel: &CancellationToken,
    io_handles: &mut PipeCancellationHandles,
    started: Instant,
    timeout: Option<Duration>,
    cancel_token: Option<&CancellationToken>,
    process_job: &ManagedProcessJob,
) -> bool {
    // A direct child may exit while a descendant retains its inherited pipe
    // handles. Keep the caller's deadline and Stop active until I/O completes.
    while !stdout.is_finished()
        || !stderr.is_finished()
        || stdin.is_some_and(|handle| !handle.is_finished())
    {
        if stdin.is_some_and(thread::JoinHandle::is_finished) {
            io_handles.release_stdin();
        }
        if timeout.is_some_and(|duration| started.elapsed() >= duration)
            || cancel_token.is_some_and(CancellationToken::is_cancelled)
        {
            io_cancel.cancel();
        }
        if io_cancel.is_cancelled() {
            let _ = process_job.terminate();
            io_handles.cancel_pending_io();
            cancel_blocking_thread_io(stdout);
            cancel_blocking_thread_io(stderr);
            if let Some(stdin) = stdin {
                cancel_blocking_thread_io(stdin);
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    io_cancel.is_cancelled()
}

#[cfg(windows)]
fn cancel_blocking_thread_io<T>(handle: &thread::JoinHandle<T>) {
    use std::os::windows::io::AsRawHandle;
    if !handle.is_finished() {
        // SAFETY: JoinHandle owns this live thread handle until its later join.
        // The thread performs only this pipe's I/O; its cooperative token stops
        // subsequent calls. Repeating handles the documented no-pending race.
        let _ =
            unsafe { windows_sys::Win32::System::IO::CancelSynchronousIo(handle.as_raw_handle()) };
    }
}

#[cfg(not(windows))]
fn cancel_blocking_thread_io<T>(_handle: &thread::JoinHandle<T>) {}

fn finish_stdin_writer(
    handle: Option<thread::JoinHandle<std::io::Result<()>>>,
) -> std::io::Result<()> {
    match handle {
        Some(handle) => handle
            .join()
            .unwrap_or_else(|_| Err(std::io::Error::other("stdin writer thread panicked"))),
        None => Ok(()),
    }
}

fn uses_pty_transport(path: &Path) -> bool {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(stem.as_str(), "agy" | "antigravity")
}

#[allow(clippy::too_many_arguments)]
fn run_resolved_command_observed_pty(
    path: &Path,
    args: &[String],
    timeout: Option<Duration>,
    stdin_text: Option<&str>,
    progress: Option<CommandProgressContext<'_>>,
    cancel_token: Option<&CancellationToken>,
) -> std::io::Result<TimedCommandOutput> {
    let started = Instant::now();
    let working_dir = progress
        .as_ref()
        .map(|progress| command_working_dir_for_output(progress.output_path))
        .unwrap_or_else(app_root);
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 40,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(pty_io_error)?;
    let mut command = pty_command_builder(path, args);
    command.cwd(working_dir.as_os_str());
    apply_editorial_agent_environment_to_pty(&mut command);
    let mut child = pair.slave.spawn_command(command).map_err(pty_io_error)?;
    let child_id = child.process_id().unwrap_or(0);
    if let Some(progress) = progress.as_ref() {
        log_editorial_agent_spawned(progress, child_id, path, &working_dir);
    }
    drop(pair.slave);

    let stdout_bytes = Arc::new(AtomicU64::new(0));
    let stdout_counter = Arc::clone(&stdout_bytes);
    let reader = pair.master.try_clone_reader().map_err(pty_io_error)?;
    let stdout_handle =
        thread::spawn(move || read_pipe_to_end_counting_classified(Some(reader), stdout_counter));

    let mut writer = pair.master.take_writer().map_err(pty_io_error)?;
    if let Some(text) = stdin_text {
        writer.write_all(text.as_bytes())?;
        writer.write_all(b"\r\n")?;
    }

    let mut last_progress = Instant::now();

    loop {
        if let Some(status) = child.try_wait()? {
            drop(writer);
            drop(pair.master);
            let (stdout, stdout_pipe_error) = stdout_handle
                .join()
                .unwrap_or_else(|_| (Vec::new(), Some("stdout_thread_panic".to_string())));
            return Ok(TimedCommandOutput {
                output: CommandOutput {
                    status: CommandExitStatus::new(
                        i32::try_from(status.exit_code()).ok(),
                        status.success(),
                    ),
                    stdout,
                    stderr: Vec::new(),
                },
                duration_ms: started.elapsed().as_millis(),
                timed_out: false,
                stdout_pipe_error,
                stderr_pipe_error: None,
            });
        }

        if let Some(timeout) = timeout {
            if started.elapsed() >= timeout {
                kill_pty_process_tree(child.as_mut());
                let status = child.wait()?;
                drop(writer);
                drop(pair.master);
                let (stdout, stdout_pipe_error) = stdout_handle
                    .join()
                    .unwrap_or_else(|_| (Vec::new(), Some("stdout_thread_panic".to_string())));
                return Ok(TimedCommandOutput {
                    output: CommandOutput {
                        status: CommandExitStatus::new(
                            i32::try_from(status.exit_code()).ok(),
                            status.success(),
                        ),
                        stdout,
                        stderr: Vec::new(),
                    },
                    duration_ms: started.elapsed().as_millis(),
                    timed_out: true,
                    stdout_pipe_error,
                    stderr_pipe_error: None,
                });
            }
        }

        if let Some(token) = cancel_token {
            if token.is_cancelled() {
                kill_pty_process_tree(child.as_mut());
                let status = child.wait()?;
                drop(writer);
                drop(pair.master);
                let (stdout, stdout_pipe_error) = stdout_handle
                    .join()
                    .unwrap_or_else(|_| (Vec::new(), Some("stdout_thread_panic".to_string())));
                return Ok(TimedCommandOutput {
                    output: CommandOutput {
                        status: CommandExitStatus::new(
                            i32::try_from(status.exit_code()).ok(),
                            status.success(),
                        ),
                        stdout,
                        stderr: Vec::new(),
                    },
                    duration_ms: started.elapsed().as_millis(),
                    timed_out: true,
                    stdout_pipe_error,
                    stderr_pipe_error: None,
                });
            }
        }

        if last_progress.elapsed() >= Duration::from_secs(30) {
            if let Some(progress) = progress.as_ref() {
                log_editorial_agent_running(
                    progress,
                    child_id,
                    started.elapsed(),
                    stdout_bytes.load(Ordering::Relaxed),
                    0,
                );
            }
            last_progress = Instant::now();
        }

        thread::sleep(Duration::from_millis(250));
    }
}

fn read_pipe_to_end_counting_classified(
    pipe: Option<impl Read>,
    byte_counter: Arc<AtomicU64>,
) -> (Vec<u8>, Option<String>) {
    read_pipe_with_cancellation(pipe, byte_counter, None)
}

fn read_pipe_with_cancellation(
    pipe: Option<impl Read>,
    byte_counter: Arc<AtomicU64>,
    cancel_token: Option<&CancellationToken>,
) -> PipeReaderResult {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut pipe_error: Option<String> = None;
    let mut truncated = false;
    if let Some(mut pipe) = pipe {
        loop {
            if cancel_token.is_some_and(CancellationToken::is_cancelled) {
                if pipe_error.is_none() {
                    pipe_error = Some("pipe_read_cancelled".to_string());
                }
                break;
            }
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    byte_counter.fetch_add(count as u64, Ordering::Relaxed);
                    if !truncated {
                        let projected = buffer.len() as u64 + count as u64;
                        if projected <= MAX_PIPE_BYTES {
                            buffer.extend_from_slice(&chunk[..count]);
                        } else {
                            // Append only the prefix that fits within MAX_PIPE_BYTES, then
                            // mark truncated and keep draining without retaining further bytes.
                            // Draining (instead of breaking) keeps the child unblocked when the
                            // OS pipe fills up, so the timeout branch can still reap it cleanly.
                            let remaining = MAX_PIPE_BYTES.saturating_sub(buffer.len() as u64);
                            if remaining > 0 {
                                let remaining = remaining as usize;
                                buffer.extend_from_slice(&chunk[..remaining]);
                            }
                            truncated = true;
                            pipe_error = Some(format!(
                                "stdout_truncated_oversize (cap={MAX_PIPE_BYTES} bytes; further output drained but not retained)"
                            ));
                        }
                    }
                }
                Err(error) => {
                    if pipe_error.is_none() {
                        pipe_error = Some(classify_pipe_error(&error));
                    }
                    break;
                }
            }
        }
    }
    (buffer, pipe_error)
}

pub(crate) fn classify_pipe_error(error: &std::io::Error) -> String {
    let raw = error.raw_os_error();
    let kind = error.kind();
    let label = match (raw, kind) {
        (Some(109), _) => "windows_error_109_broken_pipe",
        (Some(232), _) => "windows_error_232_pipe_closing",
        (Some(233), _) => "windows_error_233_pipe_no_listener",
        (_, std::io::ErrorKind::BrokenPipe) => "broken_pipe",
        (_, std::io::ErrorKind::UnexpectedEof) => "unexpected_eof",
        (_, std::io::ErrorKind::Interrupted) => "interrupted",
        (_, std::io::ErrorKind::TimedOut) => "timed_out",
        _ => "other",
    };
    let raw_label = raw
        .map(|code| code.to_string())
        .unwrap_or_else(|| "none".to_string());
    format!("{label} (kind={kind:?}, raw_os_error={raw_label})")
}

fn resolved_command_builder(path: &Path, args: &[String]) -> Command {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    #[cfg(windows)]
    {
        if extension == "ps1" {
            let mut command = hidden_command("powershell.exe");
            command
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(path)
                .args(args);
            apply_editorial_agent_environment(&mut command);
            return command;
        }
    }

    // Rust recognizes .cmd/.bat paths and applies its native cmd.exe-specific
    // escaping. Explicitly wrapping them in Command(cmd.exe).args would bypass
    // that boundary and reinterpret embedded quotes as shell commands.
    let mut command = hidden_command(path);
    command.args(args);
    apply_editorial_agent_environment(&mut command);
    command
}

fn pty_command_builder(path: &Path, args: &[String]) -> CommandBuilder {
    let mut command = CommandBuilder::new(path.as_os_str());
    append_pty_args(&mut command, args);
    command
}

fn append_pty_args(command: &mut CommandBuilder, args: &[String]) {
    for arg in args {
        command.arg(arg.as_str());
    }
}

/// Kill a child process AND its descendant tree.
///
/// On Windows, `child.kill()` only terminates the direct PID. When a peer is
/// reached through `cmd.exe /C <peer>.cmd` (or any other shim), the actual
/// peer process is a grandchild of `child` and survives the direct kill. We
/// invoke `taskkill /T /F /PID <pid>` to walk the descendant tree, then call
/// `child.wait()` to reap the cmd.exe wrapper. On non-Windows platforms,
/// `child.kill()` already sends SIGKILL to the process group when the child
/// was set up as a session leader; we keep the simple direct kill there.
pub(crate) fn kill_process_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let pid = child.id();
        let mut taskkill = hidden_command("taskkill");
        taskkill.args(["/T", "/F", "/PID", &pid.to_string()]);
        // Best-effort: if taskkill itself fails (rare; e.g. PATH stripped), we
        // still fall back to the direct kill below so the child does not leak.
        let _ = taskkill.output();
        let _ = child.kill();
    }
    #[cfg(not(windows))]
    {
        let _ = child.kill();
    }
}

fn kill_pty_process_tree(child: &mut dyn portable_pty::Child) {
    #[cfg(windows)]
    {
        if let Some(pid) = child.process_id() {
            let mut taskkill = hidden_command("taskkill");
            taskkill.args(["/T", "/F", "/PID", &pid.to_string()]);
            let _ = taskkill.output();
        }
    }
    let _ = child.kill();
}

pub(crate) fn apply_editorial_agent_environment(command: &mut Command) {
    for (key, value) in editorial_agent_environment() {
        command.env(key, value);
    }
}

fn apply_editorial_agent_environment_to_pty(command: &mut CommandBuilder) {
    for (key, value) in editorial_agent_environment() {
        command.env(key, value);
    }
}

fn editorial_agent_environment() -> Vec<(&'static str, OsString)> {
    let mut values = vec![
        ("PYTHONIOENCODING", OsString::from("utf-8")),
        ("PYTHONUTF8", OsString::from("1")),
        ("LC_ALL", OsString::from("C.UTF-8")),
        ("LANG", OsString::from("C.UTF-8")),
        ("NO_COLOR", OsString::from("1")),
        ("TERM", OsString::from("dumb")),
        ("CI", OsString::from("1")),
    ];

    if let Ok(path) = std::env::join_paths(command_search_dirs()) {
        values.push(("PATH", path));
    }

    values
}

fn pty_io_error(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use std::fs;
    use std::io::Cursor;

    #[test]
    #[cfg(windows)]
    fn pty_batch_prompt_is_rejected_before_the_launcher_can_execute() {
        let path = std::env::temp_dir().join("agy.cmd");
        let error = run_resolved_command_observed(
            &path,
            &[
                "--print".to_string(),
                "synthetic \" & echo SHELL_BOUNDARY & rem \"".to_string(),
            ],
            Some(Duration::from_secs(1)),
            None,
            None,
            None,
        )
        .err()
        .expect("unsafe PTY batch transport must fail before spawn");
        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
        assert!(error.to_string().contains("official native executable"));
    }

    #[test]
    #[cfg(windows)]
    fn windows_batch_arguments_do_not_execute_shell_commands() {
        let directory = std::env::temp_dir().join(format!(
            "maestro-batch-boundary-fixture-{}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let batch = directory.join("ignore-arguments.cmd");
        fs::write(&batch, b"@echo off\r\necho FIXTURE_ONLY\r\n").unwrap();
        let arguments = [
            "normal protocol name",
            "normal protocol name \" & echo MAESTRO_BROKEN_SHELL_BOUNDARY & rem \" end",
            "%MAESTRO_BATCH_FIXTURE_SYNTHETIC%",
        ];
        let mut outputs = Vec::new();
        for argument in arguments {
            let output = resolved_command_builder(&batch, &[argument.to_string()])
                .env_clear()
                .env("MAESTRO_BATCH_FIXTURE_SYNTHETIC", "SYNTHETIC_ONLY")
                .output()
                .unwrap();
            outputs.push(output);
        }
        fs::remove_file(&batch).unwrap();
        fs::remove_dir(&directory).unwrap();
        for output in outputs {
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                "FIXTURE_ONLY"
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn pty_powershell_prompt_is_rejected_before_the_launcher_can_execute() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "maestro-pty-powershell-refusal-{}-{nonce}",
            std::process::id(),
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("agy.ps1");
        let marker = directory.join("unexpected-script-execution.txt");
        std::fs::write(
            &path,
            b"param([string]$Marker)\r\n[IO.File]::WriteAllText($Marker, 'unexpected')\r\n",
        )
        .unwrap();
        let result = run_resolved_command_with_timeout(
            &path,
            &[marker.to_string_lossy().into_owned()],
            Duration::from_secs(1),
            None,
        );
        let executed = marker.exists();
        if executed {
            std::fs::remove_file(&marker).unwrap();
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
        let error = result
            .err()
            .expect("unsupported PTY shim must fail before spawn");
        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
        assert!(error.to_string().contains("official native executable"));
        assert!(!executed, "rejected launcher must never execute its script");
    }

    #[test]
    #[cfg(windows)]
    fn piped_powershell_shim_preserves_literal_prompt_quotes() {
        use base64::Engine;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "maestro-piped-powershell-fixture-{}-{nonce}",
            std::process::id(),
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("probe.ps1");
        std::fs::write(&path, b"param([string]$Prompt)\r\n[Console]::WriteLine('MAESTRO_LITERAL_PROMPT:' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Prompt)))\r\n").unwrap();
        let prompt = "Evidence with \"literal quoted words\" and an ampersand & kept verbatim";
        let started = Instant::now();
        let result = run_resolved_command_with_timeout(
            &path,
            &[prompt.to_string()],
            Duration::from_secs(60),
            None,
        );
        let elapsed = started.elapsed();
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
        let result = result
            .expect("harmless PowerShell fixture should execute through the actual piped path");
        let expected = format!(
            "MAESTRO_LITERAL_PROMPT:{}",
            base64::engine::general_purpose::STANDARD.encode(prompt)
        );
        let stdout = String::from_utf8_lossy(&result.output.stdout);
        let stderr = String::from_utf8_lossy(&result.output.stderr);
        println!(
            "actual harmless PowerShell piped result: elapsed={elapsed:?}, timed_out={}, success={}, stdout={stdout:?}, stderr={stderr:?}",
            result.timed_out,
            result.output.status.success()
        );
        assert!(!result.timed_out, "harmless PowerShell fixture must finish");
        assert!(
            result.output.status.success(),
            "harmless PowerShell fixture must exit successfully"
        );
        assert!(
            stdout.contains(&expected),
            "actual piped prompt bytes changed: {stdout}"
        );
    }

    #[test]
    fn piped_runner_drains_output_while_delivering_large_stdin() {
        fs_for_test_root();
        let result = run_resolved_command_with_timeout(
            &std::env::current_exe().unwrap(),
            &[
                "--exact".into(),
                "command_spawn::tests::large_output_before_input_fixture".into(),
                "--ignored".into(),
                "--nocapture".into(),
            ],
            Duration::from_secs(5),
            Some(&"i".repeat(1024 * 1024)),
        )
        .unwrap();
        assert!(!result.timed_out, "output/input pipes must not deadlock");
        assert!(result.output.status.success());
        assert!(String::from_utf8_lossy(&result.output.stdout).contains("INPUT_BYTES=1048576"));
    }

    #[test]
    fn piped_runner_cancels_when_peer_never_reads_stdin() {
        fs_for_test_root();
        let token = CancellationToken::new();
        let signal = token.clone();
        let signal_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            signal.cancel();
        });
        let result = run_resolved_command_observed(
            &std::env::current_exe().unwrap(),
            &[
                "--exact".into(),
                "command_spawn::tests::unresponsive_stdin_fixture".into(),
                "--ignored".into(),
                "--nocapture".into(),
            ],
            Some(Duration::from_secs(5)),
            Some(&"i".repeat(1024 * 1024)),
            None,
            Some(&token),
        )
        .unwrap();
        signal_thread.join().unwrap();
        assert!(
            result.timed_out,
            "cancellation must interrupt input delivery"
        );
        assert!(
            result.duration_ms < 4_000,
            "cancellation must precede the timeout"
        );
    }

    #[test]
    #[cfg(windows)]
    fn managed_job_retains_descendant_custody_after_direct_parent_exit() {
        use std::io::{BufRead, BufReader};
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::JobObjects::IsProcessInJob;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE,
        };
        for explicit_termination in [true, false] {
            let mut command = hidden_command(std::env::current_exe().unwrap());
            command
                .args(inherited_pipe_fixture_args())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let (mut child, job) = spawn_managed_piped_command(&mut command).unwrap();
            let mut output = BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            let pid = loop {
                line.clear();
                assert_ne!(output.read_line(&mut line).unwrap(), 0);
                if let Some(value) = line.trim().strip_prefix("INHERITED_FIXTURE_PID=") {
                    break value.parse::<u32>().unwrap();
                }
            };
            // Hold the real descendant process object before Stop or parent
            // reaping, so the proof cannot mistake PID reuse for termination.
            let raw = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            assert!(!raw.is_null(), "task-owned descendant must be alive");
            let descendant = unsafe { OwnedHandle::from_raw_handle(raw) };
            let mut is_member = 0;
            assert_ne!(
                unsafe {
                    IsProcessInJob(
                        descendant.as_raw_handle(),
                        job.handle.as_raw_handle(),
                        &mut is_member,
                    )
                },
                0
            );
            assert_ne!(
                is_member, 0,
                "descendant must inherit this invocation's native job"
            );
            assert!(
                child.wait().unwrap().success(),
                "direct fixture must exit normally"
            );
            assert_eq!(
                unsafe { WaitForSingleObject(descendant.as_raw_handle(), 0) },
                WAIT_TIMEOUT,
                "descendant must still be alive after its direct parent exits"
            );
            let started = Instant::now();
            if explicit_termination {
                job.terminate().unwrap();
            }
            drop(job);
            let exited = unsafe { WaitForSingleObject(descendant.as_raw_handle(), 1_000) };
            assert_eq!(
                exited, WAIT_OBJECT_0,
                "native job termination/Drop must reap its descendant"
            );
            assert!(started.elapsed() < Duration::from_secs(1));
        }
    }

    #[test]
    #[cfg(windows)]
    fn piped_runner_stop_remains_active_after_direct_exit_with_inherited_handles() {
        fs_for_test_root();
        let token = CancellationToken::new();
        let signal = token.clone();
        let signal_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            signal.cancel();
        });
        let result = run_resolved_command_observed(
            &std::env::current_exe().unwrap(),
            &inherited_pipe_fixture_args(),
            None,
            Some(&"i".repeat(1024 * 1024)),
            None,
            Some(&token),
        )
        .unwrap();
        signal_thread.join().unwrap();
        wait_for_inherited_fixture_descendant(&result);
        assert!(
            result.timed_out,
            "Stop must remain active while inherited I/O is pending"
        );
        assert!(
            result.duration_ms < 1_500,
            "Stop must precede the descendant's natural exit"
        );
        assert!(
            result.output.status.success(),
            "the direct fixture exited normally before Stop"
        );
    }

    #[test]
    #[cfg(windows)]
    fn piped_runner_preserves_successful_descendant_output() {
        fs_for_test_root();
        let result = run_resolved_command_with_timeout(
            &std::env::current_exe().unwrap(),
            &inherited_pipe_fixture_args(),
            Duration::from_secs(5),
            None,
        )
        .unwrap();
        wait_for_inherited_fixture_descendant(&result);
        assert!(
            !result.timed_out,
            "successful descendant output must drain normally"
        );
        assert!(result.output.status.success());
        assert!(
            String::from_utf8_lossy(&result.output.stdout).contains("INHERITED_FIXTURE_COMPLETED")
        );
        assert!(
            result.duration_ms >= 2_000,
            "Job must remain open until descendant output completes"
        );
    }

    #[test]
    #[cfg(windows)]
    fn piped_runner_deadline_remains_active_after_direct_exit_with_inherited_handles() {
        use std::io::{BufRead, BufReader};
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        let mut command = hidden_command(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "command_spawn::tests::inherited_pipe_deadline_parent_fixture",
                "--ignored",
                "--nocapture",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (mut child, job) = spawn_managed_piped_command(&mut command).unwrap();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let mut io_handles =
            PipeCancellationHandles::new(stdout.as_ref(), stderr.as_ref(), None).unwrap();
        let mut ready = BufReader::new(stdout.unwrap());
        let mut line = String::new();
        let pid = loop {
            line.clear();
            assert_ne!(ready.read_line(&mut line).unwrap(), 0);
            if let Some(value) = line.trim().strip_prefix("INHERITED_FIXTURE_PID=") {
                break value.parse::<u32>().unwrap();
            }
        };
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        assert!(
            !raw.is_null(),
            "readiness must identify a live task-owned descendant"
        );
        let descendant = unsafe { OwnedHandle::from_raw_handle(raw) };
        assert!(
            child.wait().unwrap().success(),
            "direct parent must exit before this deadline starts"
        );
        assert_eq!(
            unsafe { WaitForSingleObject(descendant.as_raw_handle(), 0) },
            WAIT_TIMEOUT
        );
        let io_cancel = CancellationToken::new();
        let stdout_cancel = io_cancel.clone();
        let stderr_cancel = io_cancel.clone();
        let stdout_reader = thread::spawn(move || {
            read_pipe_with_cancellation(
                Some(ready),
                Arc::new(AtomicU64::new(0)),
                Some(&stdout_cancel),
            )
        });
        let stderr_reader = thread::spawn(move || {
            read_pipe_with_cancellation(stderr, Arc::new(AtomicU64::new(0)), Some(&stderr_cancel))
        });
        let started = Instant::now();
        let interrupted = wait_for_io_threads(
            &stdout_reader,
            &stderr_reader,
            None,
            &io_cancel,
            &mut io_handles,
            started,
            Some(Duration::from_millis(250)),
            None,
            &job,
        );
        stdout_reader.join().unwrap();
        stderr_reader.join().unwrap();
        assert!(
            interrupted,
            "actual production deadline must interrupt inherited output pipe draining"
        );
        assert!(
            started.elapsed() < Duration::from_millis(1_500),
            "ready post-parent deadline must precede descendant's natural ten-second exit"
        );
        // Keep Job alive for this assertion: its Drop cannot substitute for
        // explicit post-parent deadline termination, and this is the same
        // descendant process object held before the deadline, not a new PID.
        assert_eq!(
            unsafe { WaitForSingleObject(descendant.as_raw_handle(), 500) },
            WAIT_OBJECT_0
        );
    }

    #[cfg(windows)]
    fn inherited_pipe_fixture_args() -> Vec<String> {
        vec![
            "--exact".into(),
            "command_spawn::tests::inherited_pipe_parent_fixture".into(),
            "--ignored".into(),
            "--nocapture".into(),
        ]
    }

    #[cfg(windows)]
    fn wait_for_inherited_fixture_descendant(result: &TimedCommandOutput) {
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        let output = String::from_utf8_lossy(&result.output.stdout);
        let pid = output
            .lines()
            .find_map(|line| line.strip_prefix("INHERITED_FIXTURE_PID="))
            .expect("direct fixture must publish its task-owned descendant PID")
            .parse::<u32>()
            .unwrap();
        // SAFETY: this PID is emitted by our fixture; only synchronization
        // access is requested, and its owned handle is closed after the wait.
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if !process.is_null() {
            let wait = unsafe { WaitForSingleObject(process, 500) };
            unsafe { CloseHandle(process) };
            assert_eq!(
                wait, WAIT_OBJECT_0,
                "Stop/deadline must terminate the descendant before its natural 2.5 s exit"
            );
        }
    }

    fn fs_for_test_root() {
        std::fs::create_dir_all(app_root()).unwrap();
    }

    #[test]
    #[ignore = "child-process fixture, launched explicitly by the pipe regression test"]
    fn large_output_before_input_fixture() {
        // An independent deadline keeps this regression bounded even if the
        // former write-before-drain implementation is restored accidentally.
        thread::spawn(|| {
            thread::sleep(Duration::from_secs(8));
            std::process::exit(90);
        });
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&vec![b'o'; 1024 * 1024]).unwrap();
        stdout.flush().unwrap();
        let mut input = Vec::new();
        std::io::stdin().read_to_end(&mut input).unwrap();
        writeln!(stdout, "INPUT_BYTES={}", input.len()).unwrap();
    }

    #[test]
    #[ignore = "child-process fixture, launched explicitly by the cancellation regression test"]
    fn unresponsive_stdin_fixture() {
        thread::sleep(Duration::from_secs(8));
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "child-process fixture for inherited-pipe cancellation/deadline regressions"]
    fn inherited_pipe_parent_fixture() {
        spawn_inherited_fixture_descendant(
            "command_spawn::tests::inherited_pipe_descendant_fixture",
        );
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "ready parent fixture for deterministic post-exit deadline proof"]
    fn inherited_pipe_deadline_parent_fixture() {
        spawn_inherited_fixture_descendant(
            "command_spawn::tests::inherited_pipe_deadline_descendant_fixture",
        );
    }

    #[cfg(windows)]
    fn spawn_inherited_fixture_descendant(fixture: &str) {
        let mut command = hidden_command(std::env::current_exe().unwrap());
        command.args(["--exact", fixture, "--ignored", "--nocapture"]);
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let descendant = command.spawn().unwrap();
        println!("INHERITED_FIXTURE_PID={}", descendant.id());
        // Dropping Child intentionally leaves this bounded fixture alive with
        // inherited stdin/stdout/stderr while the direct process exits.
        drop(descendant);
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "bounded descendant fixture; parent regression waits for actual process exit"]
    fn inherited_pipe_descendant_fixture() {
        thread::sleep(Duration::from_millis(2_500));
        println!("INHERITED_FIXTURE_COMPLETED");
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "bounded ten-second descendant; deadline proof requires earlier native termination"]
    fn inherited_pipe_deadline_descendant_fixture() {
        thread::sleep(Duration::from_secs(10));
    }

    #[test]
    fn pipe_reader_retains_short_payloads_without_truncation() {
        let payload = b"hello world".to_vec();
        let counter = Arc::new(AtomicU64::new(0));
        let (buffer, pipe_error) = read_pipe_to_end_counting_classified(
            Some(Cursor::new(payload.clone())),
            Arc::clone(&counter),
        );
        assert_eq!(buffer, payload);
        assert!(pipe_error.is_none());
        assert_eq!(counter.load(Ordering::Relaxed), payload.len() as u64);
    }

    #[test]
    fn pipe_reader_caps_buffer_at_max_pipe_bytes_and_keeps_draining() {
        let oversize = (MAX_PIPE_BYTES as usize) + 4096;
        let payload = vec![b'x'; oversize];
        let counter = Arc::new(AtomicU64::new(0));
        let (buffer, pipe_error) = read_pipe_to_end_counting_classified(
            Some(Cursor::new(payload.clone())),
            Arc::clone(&counter),
        );
        assert_eq!(buffer.len(), MAX_PIPE_BYTES as usize);
        let marker = pipe_error.expect("truncation marker must be set");
        assert!(
            marker.contains("stdout_truncated_oversize"),
            "marker must surface root cause: {marker}"
        );
        assert!(
            marker.contains(&MAX_PIPE_BYTES.to_string()),
            "marker must include cap value: {marker}"
        );
        // Counter must reflect the FULL input the child wrote, not the truncated retained slice.
        assert_eq!(counter.load(Ordering::Relaxed), oversize as u64);
    }

    #[test]
    fn pipe_reader_classifies_io_error_when_no_truncation_yet() {
        // A pipe that returns Err before any bytes is the classify-only path.
        struct FailingReader;
        impl Read for FailingReader {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "boom"))
            }
        }
        let counter = Arc::new(AtomicU64::new(0));
        let (buffer, pipe_error) =
            read_pipe_to_end_counting_classified(Some(FailingReader), Arc::clone(&counter));
        assert!(buffer.is_empty());
        let marker = pipe_error.expect("classifier must run on first-error path");
        assert!(
            marker.contains("broken_pipe"),
            "io error must be classified, not silenced: {marker}"
        );
    }

    #[test]
    fn pipe_reader_truncation_marker_takes_precedence_over_late_io_error() {
        // After the cap is reached, an Err should not overwrite the truncation marker; the
        // operator needs to see WHY the buffer was capped, not a downstream pipe close.
        struct CapThenError {
            remaining: usize,
        }
        impl Read for CapThenError {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.remaining > 0 {
                    let n = buf.len().min(self.remaining);
                    buf[..n].fill(b'x');
                    self.remaining -= n;
                    Ok(n)
                } else {
                    Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "late"))
                }
            }
        }
        let oversize = (MAX_PIPE_BYTES as usize) + 8192;
        let counter = Arc::new(AtomicU64::new(0));
        let (buffer, pipe_error) = read_pipe_to_end_counting_classified(
            Some(CapThenError {
                remaining: oversize,
            }),
            Arc::clone(&counter),
        );
        assert_eq!(buffer.len(), MAX_PIPE_BYTES as usize);
        let marker = pipe_error.expect("marker must be set");
        assert!(
            marker.contains("stdout_truncated_oversize"),
            "truncation cause must win over late io error: {marker}"
        );
    }

    #[test]
    fn max_pipe_bytes_is_64_mib() {
        // Pin the cap so accidental edits surface in CI.
        assert_eq!(MAX_PIPE_BYTES, 64 * 1024 * 1024);
    }

    #[cfg(windows)]
    #[test]
    fn pty_runner_times_out_for_agy_stem() {
        let dir =
            std::env::temp_dir().join(format!("maestro-agy-pty-timeout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("agy.exe");
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        fs::copy(Path::new(&system_root).join("System32\\ping.exe"), &shim).unwrap();

        let result = run_resolved_command_with_timeout(
            &shim,
            &["-n".to_string(), "6".to_string(), "127.0.0.1".to_string()],
            Duration::from_millis(750),
            None,
        )
        .unwrap();

        assert!(
            result.timed_out,
            "expected PTY timeout, got timed_out={}, status={:?}, success={}, duration_ms={}, stdout_pipe_error={:?}",
            result.timed_out,
            result.output.status.code(),
            result.output.status.success(),
            result.duration_ms,
            result.stdout_pipe_error
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
