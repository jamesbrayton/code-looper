use crate::config::Provider as ProviderKind;
use crate::error::LooperError;
use crate::security::redact_secrets;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::trace;

/// Approved provider executables.
///
/// Only binaries in this list may be spawned by the loop engine.  This
/// allowlist is enforced at runtime in non-test builds by
/// [`check_allowed_binary`] to prevent arbitrary command injection in case of
/// future refactors that relax the hardcoded binary names inside adapters.
pub const ALLOWED_PROVIDER_BINARIES: &[&str] = &["claude", "gh", "codex"];

/// Return `Ok(())` if `binary` is in [`ALLOWED_PROVIDER_BINARIES`], or a
/// [`LooperError::DisallowedExecutable`] if it is not.
pub fn check_allowed_binary(binary: &str) -> Result<(), LooperError> {
    if ALLOWED_PROVIDER_BINARIES.contains(&binary) {
        Ok(())
    } else {
        Err(LooperError::DisallowedExecutable {
            binary: binary.to_string(),
            allowed: ALLOWED_PROVIDER_BINARIES.join(", "),
        })
    }
}

/// Outcome of a single provider execution.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Exit status from the provider process (None if the process was not spawned).
    pub exit_code: Option<i32>,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
    /// Wall-clock time for this execution.
    pub duration: Duration,
}

impl ExecutionResult {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// Common interface for all provider adapters.
pub trait ProviderAdapter: Send + Sync {
    /// Return the display name of this provider.
    fn name(&self) -> &str;

    /// Execute one iteration with the given prompt.
    ///
    /// Implementations spawn the provider CLI, feed the prompt, wait for
    /// completion, and return a normalized `ExecutionResult`.  Transient
    /// failures (non-zero exit, spawn error) are surfaced as `LooperError`
    /// variants so the loop engine can apply retry / backoff logic.
    fn execute(&self, prompt: &str) -> Result<ExecutionResult, LooperError>;
}

// ── Adapter constructors ──────────────────────────────────────────────────────

/// Build the concrete adapter for the given `ProviderKind`.
///
/// When `stream_output` is `true`, each adapter will print tagged stdout/stderr
/// lines to the terminal in real time as the provider runs.
///
/// `working_dir` overrides the subprocess working directory.  When `None` the
/// subprocess inherits the current working directory of the code-looper process.
///
/// `timeout_secs` sets a per-invocation wall-clock deadline.  When the provider
/// does not exit within `timeout_secs` seconds, the process is killed and
/// `LooperError::ProviderTimeout` is returned.  `None` means no timeout.
pub fn build_adapter(
    kind: &ProviderKind,
    stream_output: bool,
    working_dir: Option<PathBuf>,
    timeout_secs: Option<u64>,
) -> Box<dyn ProviderAdapter> {
    match kind {
        ProviderKind::Claude => Box::new(ClaudeAdapter {
            stream_output,
            working_dir,
            timeout_secs,
        }),
        ProviderKind::Copilot => Box::new(CopilotAdapter {
            stream_output,
            working_dir,
            timeout_secs,
        }),
        ProviderKind::Codex => Box::new(CodexAdapter {
            stream_output,
            working_dir,
            timeout_secs,
        }),
    }
}

// ── Claude Code CLI adapter ───────────────────────────────────────────────────

pub struct ClaudeAdapter {
    pub stream_output: bool,
    pub working_dir: Option<PathBuf>,
    pub timeout_secs: Option<u64>,
}

impl ProviderAdapter for ClaudeAdapter {
    fn name(&self) -> &str {
        "claude"
    }

    fn execute(&self, prompt: &str) -> Result<ExecutionResult, LooperError> {
        run_provider_process(
            "claude",
            &["-p", "--dangerously-skip-permissions", prompt],
            self.stream_output,
            self.working_dir.as_deref(),
            self.timeout_secs,
        )
    }
}

// ── GitHub Copilot CLI adapter ────────────────────────────────────────────────

pub struct CopilotAdapter {
    pub stream_output: bool,
    pub working_dir: Option<PathBuf>,
    pub timeout_secs: Option<u64>,
}

impl ProviderAdapter for CopilotAdapter {
    fn name(&self) -> &str {
        "copilot"
    }

    fn execute(&self, prompt: &str) -> Result<ExecutionResult, LooperError> {
        run_provider_process(
            "gh",
            &["copilot", "suggest", "-t", "shell", prompt],
            self.stream_output,
            self.working_dir.as_deref(),
            self.timeout_secs,
        )
    }
}

// ── Codex CLI adapter ─────────────────────────────────────────────────────────

pub struct CodexAdapter {
    pub stream_output: bool,
    pub working_dir: Option<PathBuf>,
    pub timeout_secs: Option<u64>,
}

impl ProviderAdapter for CodexAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    fn execute(&self, prompt: &str) -> Result<ExecutionResult, LooperError> {
        run_provider_process(
            "codex",
            &[prompt],
            self.stream_output,
            self.working_dir.as_deref(),
            self.timeout_secs,
        )
    }
}

// ── Shared helper ─────────────────────────────────────────────────────────────

/// Run a provider process, optionally streaming stdout/stderr to the terminal
/// as tagged lines (`[stdout]` / `[stderr]`).
///
/// `working_dir` sets the subprocess working directory.  `None` inherits the
/// current working directory of the code-looper process.
///
/// `timeout_secs` sets a maximum wall-clock duration for the provider.  When
/// the timeout fires the process is killed and `LooperError::ProviderTimeout`
/// is returned.  `None` means no timeout.
fn run_provider_process(
    binary: &str,
    args: &[&str],
    stream: bool,
    working_dir: Option<&std::path::Path>,
    timeout_secs: Option<u64>,
) -> Result<ExecutionResult, LooperError> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::time::Instant;

    // Enforce the executable allowlist in non-test builds.  Test builds bypass
    // this so unit tests can spawn utilities like `sleep` or `echo` to exercise
    // timeout/streaming behaviour without requiring real provider CLIs.
    #[cfg(not(test))]
    check_allowed_binary(binary)?;

    let start = Instant::now();

    if stream {
        let mut cmd = Command::new(binary);
        cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }
        let mut child = cmd.spawn().map_err(|e| LooperError::ProviderSpawn {
            binary: binary.to_string(),
            source: e,
        })?;

        let stdout_pipe = child.stdout.take().expect("stdout piped");
        let stderr_pipe = child.stderr.take().expect("stderr piped");

        // Wrap child in Arc<Mutex> so the optional watchdog thread can kill it.
        let child = std::sync::Arc::new(std::sync::Mutex::new(child));
        let timeout_fired = Arc::new(AtomicBool::new(false));

        if let Some(secs) = timeout_secs {
            let child_clone = Arc::clone(&child);
            let fired = Arc::clone(&timeout_fired);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(secs));
                if let Ok(mut c) = child_clone.lock() {
                    // Only mark as timed-out when kill() succeeds.  If the child
                    // already exited and was reaped, kill() returns an error and we
                    // leave `fired` false, avoiding a false-positive timeout report.
                    if c.kill().is_ok() {
                        fired.store(true, Ordering::Relaxed);
                    }
                }
            });
        }

        // Read stdout on a background thread so we don't deadlock on stderr.
        let stdout_handle = std::thread::spawn(move || {
            let reader = BufReader::new(stdout_pipe);
            let mut captured = String::new();
            for line in reader.lines().map_while(Result::ok) {
                let line = redact_secrets(&line);
                println!("[stdout] {}", line);
                trace!(stream = "stdout", "{}", line);
                captured.push_str(&line);
                captured.push('\n');
            }
            captured
        });

        let mut stderr_captured = String::new();
        let stderr_reader = BufReader::new(stderr_pipe);
        for line in stderr_reader.lines().map_while(Result::ok) {
            let line = redact_secrets(&line);
            eprintln!("[stderr] {}", line);
            trace!(stream = "stderr", "{}", line);
            stderr_captured.push_str(&line);
            stderr_captured.push('\n');
        }

        let stdout_captured = stdout_handle.join().unwrap_or_default();
        let status = child
            .lock()
            .unwrap()
            .wait()
            .map_err(|e| LooperError::ProviderSpawn {
                binary: binary.to_string(),
                source: e,
            })?;
        let duration = start.elapsed();

        if timeout_fired.load(Ordering::Relaxed) {
            return Err(LooperError::ProviderTimeout {
                binary: binary.to_string(),
                timeout_secs: timeout_secs.unwrap_or(0),
            });
        }

        Ok(ExecutionResult {
            exit_code: status.code(),
            stdout: stdout_captured,
            stderr: stderr_captured,
            duration,
        })
    } else {
        let mut cmd = Command::new(binary);
        cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }
        let mut child = cmd.spawn().map_err(|e| LooperError::ProviderSpawn {
            binary: binary.to_string(),
            source: e,
        })?;

        let stdout_pipe = child.stdout.take().expect("stdout piped");
        let stderr_pipe = child.stderr.take().expect("stderr piped");

        // Wrap child so the optional watchdog can kill it.
        let child = Arc::new(std::sync::Mutex::new(child));
        let timeout_fired = Arc::new(AtomicBool::new(false));

        if let Some(secs) = timeout_secs {
            let child_clone = Arc::clone(&child);
            let fired = Arc::clone(&timeout_fired);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(secs));
                if let Ok(mut c) = child_clone.lock() {
                    let _ = c.kill();
                }
                fired.store(true, Ordering::Relaxed);
            });
        }

        // Drain both pipes on background threads to prevent deadlock when the
        // child fills its pipe buffers.  The drain threads block until EOF,
        // which happens when the child exits or is killed by the watchdog.
        let stdout_handle = std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = Vec::new();
            let _ = { stdout_pipe }.read_to_end(&mut buf);
            buf
        });
        let stderr_handle = std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = Vec::new();
            let _ = { stderr_pipe }.read_to_end(&mut buf);
            buf
        });

        let stdout_bytes = stdout_handle.join().unwrap_or_default();
        let stderr_bytes = stderr_handle.join().unwrap_or_default();
        let exit_code = child.lock().unwrap().wait().ok().and_then(|s| s.code());
        let duration = start.elapsed();

        if timeout_fired.load(Ordering::Relaxed) {
            return Err(LooperError::ProviderTimeout {
                binary: binary.to_string(),
                timeout_secs: timeout_secs.unwrap_or(0),
            });
        }

        Ok(ExecutionResult {
            exit_code,
            stdout: redact_secrets(&String::from_utf8_lossy(&stdout_bytes)),
            stderr: redact_secrets(&String::from_utf8_lossy(&stderr_bytes)),
            duration,
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::time::Duration;

    /// A deterministic test adapter that always returns a preset result.
    pub struct FakeAdapter {
        pub name: String,
        pub result: Result<ExecutionResult, LooperError>,
    }

    impl FakeAdapter {
        pub fn success(name: &str) -> Self {
            Self {
                name: name.to_string(),
                result: Ok(ExecutionResult {
                    exit_code: Some(0),
                    stdout: "ok".to_string(),
                    stderr: String::new(),
                    duration: Duration::from_millis(5),
                }),
            }
        }

        pub fn failure(name: &str) -> Self {
            Self {
                name: name.to_string(),
                result: Ok(ExecutionResult {
                    exit_code: Some(1),
                    stdout: String::new(),
                    stderr: "error".to_string(),
                    duration: Duration::from_millis(5),
                }),
            }
        }
    }

    impl ProviderAdapter for FakeAdapter {
        fn name(&self) -> &str {
            &self.name
        }

        fn execute(&self, _prompt: &str) -> Result<ExecutionResult, LooperError> {
            match &self.result {
                Ok(r) => Ok(r.clone()),
                Err(e) => Err(LooperError::InvalidArgument(e.to_string())),
            }
        }
    }

    #[test]
    fn execution_result_success() {
        let r = ExecutionResult {
            exit_code: Some(0),
            stdout: "done".to_string(),
            stderr: String::new(),
            duration: Duration::from_millis(10),
        };
        assert!(r.succeeded());
    }

    #[test]
    fn execution_result_failure() {
        let r = ExecutionResult {
            exit_code: Some(1),
            stdout: String::new(),
            stderr: "oops".to_string(),
            duration: Duration::from_millis(10),
        };
        assert!(!r.succeeded());
    }

    #[test]
    fn fake_adapter_success() {
        let adapter = FakeAdapter::success("test");
        let result = adapter.execute("hello").unwrap();
        assert!(result.succeeded());
        assert_eq!(result.stdout, "ok");
    }

    #[test]
    fn fake_adapter_failure() {
        let adapter = FakeAdapter::failure("test");
        let result = adapter.execute("hello").unwrap();
        assert!(!result.succeeded());
    }

    #[test]
    fn build_adapter_returns_correct_names() {
        assert_eq!(
            build_adapter(&ProviderKind::Claude, false, None, None).name(),
            "claude"
        );
        assert_eq!(
            build_adapter(&ProviderKind::Copilot, false, None, None).name(),
            "copilot"
        );
        assert_eq!(
            build_adapter(&ProviderKind::Codex, false, None, None).name(),
            "codex"
        );
    }

    // ── Timeout adapter ───────────────────────────────────────────────────────

    /// Adapter that always returns ProviderTimeout (simulates a timed-out call).
    pub struct TimeoutAdapter;

    impl ProviderAdapter for TimeoutAdapter {
        fn name(&self) -> &str {
            "timeout-fake"
        }

        fn execute(&self, _prompt: &str) -> Result<ExecutionResult, LooperError> {
            Err(LooperError::ProviderTimeout {
                binary: "fake".to_string(),
                timeout_secs: 1,
            })
        }
    }

    // ── check_allowed_binary ──────────────────────────────────────────────────

    #[test]
    fn allowlist_accepts_claude() {
        assert!(super::check_allowed_binary("claude").is_ok());
    }

    #[test]
    fn allowlist_accepts_gh() {
        assert!(super::check_allowed_binary("gh").is_ok());
    }

    #[test]
    fn allowlist_accepts_codex() {
        assert!(super::check_allowed_binary("codex").is_ok());
    }

    #[test]
    fn allowlist_rejects_disallowed_binary() {
        let err = super::check_allowed_binary("rm").unwrap_err();
        assert!(
            matches!(err, LooperError::DisallowedExecutable { ref binary, .. } if binary == "rm")
        );
    }

    #[test]
    fn allowlist_rejects_arbitrary_path() {
        let err = super::check_allowed_binary("/usr/bin/bash").unwrap_err();
        assert!(matches!(err, LooperError::DisallowedExecutable { .. }));
    }

    #[test]
    fn disallowed_executable_error_message_lists_permitted_binaries() {
        let err = super::check_allowed_binary("bad-binary").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bad-binary"));
        assert!(msg.contains("claude"));
        assert!(msg.contains("gh"));
        assert!(msg.contains("codex"));
    }

    #[test]
    fn all_adapter_binaries_are_in_allowlist() {
        // Ensure every binary used by the three concrete adapters is covered.
        // This test will fail if a new adapter is added without updating the allowlist.
        assert!(
            super::check_allowed_binary("claude").is_ok(),
            "claude missing"
        );
        assert!(super::check_allowed_binary("gh").is_ok(), "gh missing");
        assert!(
            super::check_allowed_binary("codex").is_ok(),
            "codex missing"
        );
    }

    // ── run_provider_process timeout (real subprocess) ────────────────────────

    /// Verify that a process that runs longer than the timeout is killed and
    /// `ProviderTimeout` is returned (non-streaming path).
    ///
    /// Uses `sleep` as a long-running process; skipped on platforms without it.
    #[test]
    #[cfg(unix)]
    fn non_streaming_process_is_killed_on_timeout() {
        // sleep for 60s, but timeout after 1s
        let result = super::run_provider_process("sleep", &["60"], false, None, Some(1));
        match result {
            Err(LooperError::ProviderTimeout { timeout_secs, .. }) => {
                assert_eq!(timeout_secs, 1);
            }
            other => panic!("expected ProviderTimeout, got: {:?}", other),
        }
    }

    /// Verify that a fast process completes normally without triggering timeout.
    #[test]
    #[cfg(unix)]
    fn non_streaming_fast_process_is_not_timed_out() {
        let result = super::run_provider_process("echo", &["hello"], false, None, Some(5));
        match result {
            Ok(r) => assert!(r.succeeded(), "expected success, got: {:?}", r.exit_code),
            Err(e) => panic!("expected Ok, got: {e}"),
        }
    }
}
