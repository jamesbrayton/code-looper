use crate::config::Provider as ProviderKind;
use crate::error::LooperError;
use std::time::Duration;

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
pub fn build_adapter(kind: &ProviderKind) -> Box<dyn ProviderAdapter> {
    match kind {
        ProviderKind::Claude => Box::new(ClaudeAdapter),
        ProviderKind::Copilot => Box::new(CopilotAdapter),
        ProviderKind::Codex => Box::new(CodexAdapter),
    }
}

// ── Claude Code CLI adapter ───────────────────────────────────────────────────

pub struct ClaudeAdapter;

impl ProviderAdapter for ClaudeAdapter {
    fn name(&self) -> &str {
        "claude"
    }

    fn execute(&self, prompt: &str) -> Result<ExecutionResult, LooperError> {
        run_provider_process("claude", &["-p", "--dangerously-skip-permissions", prompt])
    }
}

// ── GitHub Copilot CLI adapter ────────────────────────────────────────────────

pub struct CopilotAdapter;

impl ProviderAdapter for CopilotAdapter {
    fn name(&self) -> &str {
        "copilot"
    }

    fn execute(&self, prompt: &str) -> Result<ExecutionResult, LooperError> {
        run_provider_process("gh", &["copilot", "suggest", "-t", "shell", prompt])
    }
}

// ── Codex CLI adapter ─────────────────────────────────────────────────────────

pub struct CodexAdapter;

impl ProviderAdapter for CodexAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    fn execute(&self, prompt: &str) -> Result<ExecutionResult, LooperError> {
        run_provider_process("codex", &[prompt])
    }
}

// ── Shared helper ─────────────────────────────────────────────────────────────

fn run_provider_process(binary: &str, args: &[&str]) -> Result<ExecutionResult, LooperError> {
    use std::process::Command;
    use std::time::Instant;

    let start = Instant::now();
    let output =
        Command::new(binary)
            .args(args)
            .output()
            .map_err(|e| LooperError::ProviderSpawn {
                binary: binary.to_string(),
                source: e,
            })?;
    let duration = start.elapsed();

    Ok(ExecutionResult {
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        duration,
    })
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
        assert_eq!(build_adapter(&ProviderKind::Claude).name(), "claude");
        assert_eq!(build_adapter(&ProviderKind::Copilot).name(), "copilot");
        assert_eq!(build_adapter(&ProviderKind::Codex).name(), "codex");
    }
}
