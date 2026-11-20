use refract_core::cancel::CancelToken;
use refract_core::error::{ErrorCode, RefractError};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(15);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
struct OwnedJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Drop for OwnedJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(windows)]
impl OwnedJob {
    fn attach_and_resume(child: &std::process::Child) -> std::io::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::*,
            System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
        };
        unsafe {
            let job = Self(CreateJobObjectW(std::ptr::null(), std::ptr::null()));
            if job.0.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(job.0, child.as_raw_handle()) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error());
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of_val(&entry) as u32;
            let mut found = Thread32First(snapshot, &mut entry) != 0;
            let mut resumed = false;
            while found {
                if entry.th32OwnerProcessID == child.id() {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        resumed = ResumeThread(thread) != u32::MAX;
                        CloseHandle(thread);
                    }
                    break;
                }
                found = Thread32Next(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            if !resumed {
                return Err(std::io::Error::other("Could not resume owned process"));
            }
            Ok(job)
        }
    }
    fn terminate(&self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}

fn terminate_owned(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        // Leader is unreaped and was started in its own group; this ID cannot be reused.
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
}

/// Captured result of a completed child process.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Remove any partial output files created before failure or cancellation.
fn cleanup_files(paths: &[PathBuf]) {
    for path in paths {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Spawn a subprocess once, drain stdout/stderr concurrently without deadlocking,
/// poll for cooperative cancellation, kill and reap the child if canceled, and
/// clean up any registered `cleanup_paths` on failure or cancellation.
///
/// Never holds any global lock while waiting.
pub fn run_command_cancellable<P, I, S>(
    program: P,
    args: I,
    cancel: &CancelToken,
    cleanup_paths: &[PathBuf],
    tool_label: &str,
) -> Result<ProcessOutput, RefractError>
where
    P: AsRef<Path>,
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    if cancel.is_canceled() {
        cleanup_files(cleanup_paths);
        return Err(RefractError::canceled());
    }

    let mut cmd = Command::new(program.as_ref());
    cmd.args(args.into_iter().map(Into::into))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(
            CREATE_NO_WINDOW | windows_sys::Win32::System::Threading::CREATE_SUSPENDED,
        );
    }

    let mut child = cmd.spawn().map_err(|e| {
        cleanup_files(cleanup_paths);
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::MissingDependency
        } else {
            ErrorCode::EngineFailed
        };
        RefractError::with_details(
            code,
            format!("Failed to start {}", tool_label),
            e.to_string(),
        )
    })?;

    #[cfg(windows)]
    let job = OwnedJob::attach_and_resume(&child).map_err(|e| {
        let _ = child.kill();
        let _ = child.wait();
        cleanup_files(cleanup_paths);
        RefractError::with_details(
            ErrorCode::EngineFailed,
            "Failed to own subprocess job",
            e.to_string(),
        )
    })?;

    // Drain stdout and stderr on dedicated reader threads immediately so the child
    // can never block on a full OS pipe buffer while the parent polls `try_wait()`.
    let stdout_handle = child.stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });
    let stderr_handle = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let status = loop {
        if cancel.is_canceled() {
            // Race-safe: child may have exited between the last poll and kill(); ignore kill error
            // and always call wait() to reap the child process cleanly.
            #[cfg(windows)]
            job.terminate();
            terminate_owned(&mut child);
            let _ = child.wait();
            if let Some(h) = stdout_handle {
                let _ = h.join();
            }
            if let Some(h) = stderr_handle {
                let _ = h.join();
            }
            cleanup_files(cleanup_paths);
            return Err(RefractError::canceled());
        }

        // Do not reap the leader until EOF: its PID pins the process-group ID even
        // if a descendant still owns a pipe. Cancellation can then safely kill the group.
        if stdout_handle.as_ref().is_some_and(|h| !h.is_finished())
            || stderr_handle.as_ref().is_some_and(|h| !h.is_finished())
        {
            thread::sleep(POLL_INTERVAL);
            continue;
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(e) => {
                #[cfg(windows)]
                job.terminate();
                terminate_owned(&mut child);
                let _ = child.wait();
                if let Some(h) = stdout_handle {
                    let _ = h.join();
                }
                if let Some(h) = stderr_handle {
                    let _ = h.join();
                }
                cleanup_files(cleanup_paths);
                return Err(RefractError::with_details(
                    ErrorCode::EngineFailed,
                    format!("Failed while waiting for {}", tool_label),
                    e.to_string(),
                ));
            }
        }
    };

    let stdout = stdout_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    // If cancellation arrived concurrently right as the child exited, honor cancellation
    // deterministically and remove partial/unwanted outputs.
    if cancel.is_canceled() {
        cleanup_files(cleanup_paths);
        return Err(RefractError::canceled());
    }

    if !status.success() {
        cleanup_files(cleanup_paths);
        let stderr_text = String::from_utf8_lossy(&stderr).trim().to_string();
        let stdout_text = String::from_utf8_lossy(&stdout).trim().to_string();
        let details = if !stderr_text.is_empty() {
            stderr_text
        } else {
            stdout_text
        };
        let message = match status.code() {
            Some(code) => format!("{} exited with code {}", tool_label, code),
            None => format!("{} was terminated unexpectedly", tool_label),
        };
        return Err(if details.is_empty() {
            RefractError::new(ErrorCode::EngineFailed, message)
        } else {
            RefractError::with_details(ErrorCode::EngineFailed, message, details)
        });
    }

    Ok(ProcessOutput {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[cfg(windows)]
    fn shell_command(script: &str) -> (&'static str, Vec<String>) {
        ("cmd", vec!["/C".to_string(), script.to_string()])
    }

    // Keep redirection syntax out of Command's Windows argument quoting.
    #[cfg(windows)]
    fn batch_command(dir: &Path, script: &str) -> (&'static str, Vec<String>) {
        let path = dir.join("test command.cmd");
        std::fs::write(&path, format!("@echo off\r\n{script}\r\n")).unwrap();
        (
            "cmd",
            vec![
                "/D".into(),
                "/C".into(),
                path.to_string_lossy().into_owned(),
            ],
        )
    }

    #[cfg(windows)]
    #[test]
    fn batch_command_creates_nonempty_output_at_path_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ready marker.out");
        let (prog, args) = batch_command(
            dir.path(),
            &format!("echo ready > \"{}\"\r\nexit /b 0", marker.display()),
        );
        run_command_cancellable(prog, args, &CancelToken::none(), &[], "batch-test").unwrap();
        assert_eq!(std::fs::read_to_string(marker).unwrap().trim(), "ready");
    }

    #[cfg(unix)]
    fn shell_command(script: &str) -> (&'static str, Vec<String>) {
        ("sh", vec!["-c".to_string(), script.to_string()])
    }

    #[test]
    fn cancels_before_launch_without_spawning_process() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("should_not_exist.txt");
        let token = CancelToken::new();
        token.cancel();

        #[cfg(windows)]
        let script = format!("echo ran > \"{}\"", marker.display());
        #[cfg(unix)]
        let script = format!("echo ran > '{}'", marker.display());

        let (prog, args) = shell_command(&script);
        let err = run_command_cancellable(prog, args, &token, &[], "test-shell").unwrap_err();
        assert_eq!(err.code, ErrorCode::Canceled);
        assert!(
            !marker.exists(),
            "process must not be spawned when pre-canceled"
        );
    }

    #[test]
    fn kills_reaps_and_cleans_up_partial_output_on_mid_run_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let partial = dir.path().join("partial.out");

        let token = CancelToken::new();
        let canceller = token.clone();
        let ready = partial.clone();
        let readiness = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !std::fs::metadata(&ready).is_ok_and(|m| m.len() > 0) {
                if Instant::now() >= deadline {
                    canceller.cancel();
                    return false;
                }
                thread::sleep(Duration::from_millis(10));
            }
            canceller.cancel();
            true
        });
        #[cfg(unix)]
        let script = format!("echo ready > '{}'; sleep 30", partial.display());
        #[cfg(unix)]
        let (prog, args) = shell_command(&script);
        #[cfg(windows)]
        let (prog, args) = batch_command(
            dir.path(),
            &format!(
                "echo ready > \"{}\"\r\nping -n 30 127.0.0.1",
                partial.display()
            ),
        );

        let started = Instant::now();
        let err = run_command_cancellable(
            prog,
            args,
            &token,
            std::slice::from_ref(&partial),
            "long-task",
        )
        .unwrap_err();

        assert!(readiness.join().unwrap(), "child did not signal readiness");
        assert_eq!(err.code, ErrorCode::Canceled);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "canceled child took too long to terminate: {:?}",
            started.elapsed()
        );
        assert!(
            !partial.exists(),
            "partial output file must be removed on cancellation"
        );
    }

    #[test]
    fn executes_once_captures_stderr_and_cleans_partial_file_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let count_file = dir.path().join("invocations.log");
        let partial = dir.path().join("failed.out");
        let untouched_user_file = dir.path().join("existing.txt");
        std::fs::write(&partial, b"incomplete").unwrap();
        std::fs::write(&untouched_user_file, b"keep-me").unwrap();

        #[cfg(windows)]
        let (prog, args) = {
            let script_path = dir.path().join("fail_once.cmd");
            std::fs::write(
                &script_path,
                format!(
                    "@echo off\r\necho run>>\"{}\"\r\necho failure-detail 1>&2\r\nexit /b 7\r\n",
                    count_file.display()
                ),
            )
            .unwrap();
            (
                "cmd",
                vec!["/C".to_string(), script_path.to_string_lossy().into_owned()],
            )
        };
        #[cfg(unix)]
        let (prog, args) = shell_command(&format!(
            "echo run >> '{}'; echo failure-detail >&2; exit 7",
            count_file.display()
        ));

        let err = run_command_cancellable(
            prog,
            args,
            &CancelToken::none(),
            std::slice::from_ref(&partial),
            "failing-cmd",
        )
        .unwrap_err();

        assert_eq!(err.code, ErrorCode::EngineFailed);
        assert!(err.message.contains("7"));
        assert!(
            err.details
                .as_deref()
                .unwrap_or_default()
                .contains("failure-detail"),
            "stderr was not captured: {:?}",
            err.details
        );
        assert!(
            !partial.exists(),
            "partial output must be cleaned up on failure"
        );
        assert_eq!(
            std::fs::read(&untouched_user_file).unwrap(),
            b"keep-me",
            "existing unrelated files must never be modified or removed on failure"
        );

        let lines = std::fs::read_to_string(&count_file).unwrap();
        assert_eq!(
            lines.lines().count(),
            1,
            "command must execute exactly once, not twice"
        );
    }

    #[test]
    fn large_stdout_and_stderr_do_not_deadlock() {
        // Write > 128 KB to both stdout and stderr (well past OS pipe buffers).
        #[cfg(windows)]
        let (prog, args) = (
            "powershell",
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "[Console]::Out.Write('A' * 131072); [Console]::Error.Write('B' * 131072)"
                    .to_string(),
            ],
        );
        #[cfg(unix)]
        let (prog, args) = (
            "sh",
            vec![
                "-c".to_string(),
                "head -c 131072 /dev/zero | tr '\\0' 'A'; head -c 131072 /dev/zero | tr '\\0' 'B' >&2"
                    .to_string(),
            ],
        );

        let output =
            run_command_cancellable(prog, args, &CancelToken::none(), &[], "large-stream-test")
                .expect("large output streams should complete without deadlock");

        assert!(output.stdout.len() >= 131_072);
        assert!(output.stderr.len() >= 131_072);
    }

    #[test]
    fn missing_binary_returns_missing_dependency_error() {
        let err = run_command_cancellable(
            "refract-nonexistent-binary-99999",
            Vec::<String>::new(),
            &CancelToken::none(),
            &[],
            "nonexistent",
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::MissingDependency);
    }
    #[cfg(unix)]
    #[test]
    fn cancellation_after_leader_exit_terminates_pipe_holding_descendant() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ready");
        let token = CancelToken::new();
        let canceller = token.clone();
        let ready = marker.clone();
        let task = thread::spawn(move || {
            let (program, args) = shell_command(&format!(
                "sleep 30 & echo ready > '{}'; exit 0",
                ready.display()
            ));
            run_command_cancellable(program, args, &token, &[], "descendant-test")
        });
        let start = Instant::now();
        while !marker.exists() && start.elapsed() < Duration::from_secs(2) {
            thread::sleep(POLL_INTERVAL);
        }
        let ready = marker.exists();
        thread::sleep(Duration::from_millis(50));
        canceller.cancel();
        while !task.is_finished() && start.elapsed() < Duration::from_secs(4) {
            thread::sleep(POLL_INTERVAL);
        }
        assert!(
            task.is_finished(),
            "descendant kept pipes open after cancellation"
        );
        assert!(ready, "child did not start");
        assert_eq!(task.join().unwrap().unwrap_err().code, ErrorCode::Canceled);
    }
}
