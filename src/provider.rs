use crate::config::Provider as ProviderKind;
use crate::error::LooperError;
use crate::security::redact_secrets;
use std::time::Duration;
use tracing::trace;

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
pub fn build_adapter(kind: &ProviderKind, stream_output: bool) -> Box<dyn ProviderAdapter> {
    match kind {
        ProviderKind::Claude => Box::new(ClaudeAdapter { stream_output }),
        ProviderKind::Copilot => Box::new(CopilotAdapter { stream_output }),
        ProviderKind::Codex => Box::new(CodexAdapter { stream_output }),
    }
}

// ── Claude Code CLI adapter ───────────────────────────────────────────────────

pub struct ClaudeAdapter {
    pub stream_output: bool,
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
        )
    }
}

// ── GitHub Copilot CLI adapter ────────────────────────────────────────────────

pub struct CopilotAdapter {
    pub stream_output: bool,
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
        )
    }
}

// ── Codex CLI adapter ─────────────────────────────────────────────────────────

pub struct CodexAdapter {
    pub stream_output: bool,
}

impl ProviderAdapter for CodexAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    fn execute(&self, prompt: &str) -> Result<ExecutionResult, LooperError> {
        run_provider_process("codex", &[prompt], self.stream_output)
    }
}

// ── Shared helper ─────────────────────────────────────────────────────────────

/// Run a provider process, optionally streaming stdout/stderr to the terminal
/// as tagged lines (`[stdout]` / `[stderr]`).
fn run_provider_process(
    binary: &str,
    args: &[&str],
    stream: bool,
) -> Result<ExecutionResult, LooperError> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let start = Instant::now();

    if stream {
        let mut child = Command::new(binary)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| LooperError::ProviderSpawn {
                binary: binary.to_string(),
                source: e,
            })?;

        let stdout_pipe = child.stdout.take().expect("stdout piped");
        let stderr_pipe = child.stderr.take().expect("stderr piped");

        // Read stdout on a background thread so we don't deadlock on stderr.
        let stdout_handle = std::thread::spawn(move || {
            let reader = BufReader::new(stdout_pipe);
            let mut captured = String::new();
            for line in reader.lines().flatten() {
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
        for line in stderr_reader.lines().flatten() {
            let line = redact_secrets(&line);
            eprintln!("[stderr] {}", line);
            trace!(stream = "stderr", "{}", line);
            stderr_captured.push_str(&line);
            stderr_captured.push('\n');
        }

        let stdout_captured = stdout_handle.join().unwrap_or_default();
        let status = child.wait().map_err(|e| LooperError::ProviderSpawn {
            binary: binary.to_string(),
            source: e,
        })?;
        let duration = start.elapsed();

        Ok(ExecutionResult {
            exit_code: status.code(),
            stdout: stdout_captured,
            stderr: stderr_captured,
            duration,
        })
    } else {
        let output = Command::new(binary)
            .args(args)
            .output()
            .map_err(|e| LooperError::ProviderSpawn {
                binary: binary.to_string(),
                source: e,
            })?;
        let duration = start.elapsed();

        Ok(ExecutionResult {
            exit_code: output.status.code(),
            stdout: redact_secrets(&String::from_utf8_lossy(&output.stdout)),
            stderr: redact_secrets(&String::from_utf8_lossy(&output.stderr)),
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
        assert_eq!(build_adapter(&ProviderKind::Claude, false).name(), "claude");
        assert_eq!(build_adapter(&ProviderKind::Copilot, false).name(), "copilot");
        assert_eq!(build_adapter(&ProviderKind::Codex, false).name(), "codex");
    }
}
