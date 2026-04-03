use crate::config::LoopConfig;
use crate::provider::{build_adapter, ProviderAdapter};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

/// Reason the loop terminated.
#[derive(Debug, Clone, PartialEq)]
pub enum TerminationReason {
    /// All requested iterations completed normally.
    Completed,
    /// User interrupted via SIGINT (Ctrl+C).
    Interrupted,
    /// A provider spawn error occurred and the loop was aborted.
    ProviderError(String),
}

impl std::fmt::Display for TerminationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerminationReason::Completed => write!(f, "completed"),
            TerminationReason::Interrupted => write!(f, "interrupted"),
            TerminationReason::ProviderError(msg) => write!(f, "provider error: {msg}"),
        }
    }
}

/// Aggregate statistics for a finished loop session.
#[derive(Debug, Default)]
pub struct SessionSummary {
    pub iterations_run: u64,
    pub successes: u64,
    pub failures: u64,
    pub termination_reason: Option<TerminationReason>,
}

impl SessionSummary {
    fn print(&self) {
        info!(
            iterations_run = self.iterations_run,
            successes = self.successes,
            failures = self.failures,
            termination_reason = %self.termination_reason.as_ref().map(|r| r.to_string()).unwrap_or_default(),
            "Session summary"
        );
        println!();
        println!("─── Session Summary ────────────────────────────");
        println!("  Iterations run : {}", self.iterations_run);
        println!("  Successes      : {}", self.successes);
        println!("  Failures       : {}", self.failures);
        if let Some(reason) = &self.termination_reason {
            println!("  Termination    : {reason}");
        }
        println!("────────────────────────────────────────────────");
    }
}

/// Drives the main iteration loop.
pub struct LoopEngine {
    config: LoopConfig,
    adapter: Box<dyn ProviderAdapter>,
    /// Shared flag set to `true` when SIGINT is received.
    interrupted: Arc<AtomicBool>,
}

impl LoopEngine {
    pub fn new(config: LoopConfig) -> Self {
        let adapter = build_adapter(&config.provider);
        Self::with_adapter(config, adapter)
    }

    /// Constructor that accepts a custom adapter (useful for testing).
    pub fn with_adapter(config: LoopConfig, adapter: Box<dyn ProviderAdapter>) -> Self {
        let interrupted = Arc::new(AtomicBool::new(false));
        Self {
            config,
            adapter,
            interrupted,
        }
    }

    /// Install a Ctrl+C handler that sets the interrupted flag.
    ///
    /// Returns an `Arc` to the flag so callers can observe it if needed.
    pub fn install_signal_handler(&self) -> Arc<AtomicBool> {
        let flag = Arc::clone(&self.interrupted);
        ctrlc::set_handler(move || {
            flag.store(true, Ordering::SeqCst);
            eprintln!("\nInterrupt received — finishing current iteration and stopping…");
        })
        .unwrap_or_else(|e| warn!("Failed to install Ctrl+C handler: {e}"));
        Arc::clone(&self.interrupted)
    }

    /// Resolve the prompt string from the config, or return a default.
    fn resolve_prompt(&self) -> anyhow::Result<String> {
        if let Some(inline) = &self.config.prompt_inline {
            return Ok(inline.clone());
        }
        if let Some(path) = &self.config.prompt_file {
            let content = std::fs::read_to_string(path).map_err(|e| {
                anyhow::anyhow!("failed to read prompt file {}: {e}", path.display())
            })?;
            return Ok(content);
        }
        // No prompt configured — use empty string; provider decides behaviour.
        Ok(String::new())
    }

    /// Run the loop and return a session summary.
    pub fn run(self) -> SessionSummary {
        let prompt = match self.resolve_prompt() {
            Ok(p) => p,
            Err(e) => {
                error!("Could not resolve prompt: {e}");
                return SessionSummary {
                    termination_reason: Some(TerminationReason::ProviderError(e.to_string())),
                    ..Default::default()
                };
            }
        };

        let infinite = self.config.iterations == -1;
        let max = if infinite {
            u64::MAX
        } else {
            self.config.iterations as u64
        };

        let mut summary = SessionSummary::default();
        let session_start = Instant::now();

        info!(
            provider = self.adapter.name(),
            iterations = if infinite {
                "infinite".to_string()
            } else {
                max.to_string()
            },
            prompt_source = if self.config.prompt_file.is_some() {
                "file"
            } else if self.config.prompt_inline.is_some() {
                "inline"
            } else {
                "none"
            },
            "Loop starting"
        );

        for i in 1..=max {
            if self.interrupted.load(Ordering::SeqCst) {
                summary.termination_reason = Some(TerminationReason::Interrupted);
                break;
            }

            let iter_start = Instant::now();

            info!(
                iteration = i,
                provider = self.adapter.name(),
                "Iteration start"
            );

            match self.adapter.execute(&prompt) {
                Ok(result) => {
                    let duration_ms = result.duration.as_millis();
                    summary.iterations_run += 1;

                    if result.succeeded() {
                        summary.successes += 1;
                        info!(
                            iteration = i,
                            provider = self.adapter.name(),
                            exit_code = result.exit_code,
                            duration_ms,
                            output = %result.stdout.trim(),
                            "Iteration succeeded"
                        );
                    } else {
                        summary.failures += 1;
                        warn!(
                            iteration = i,
                            provider = self.adapter.name(),
                            exit_code = result.exit_code,
                            duration_ms,
                            stderr = %result.stderr.trim(),
                            "Iteration failed (non-zero exit)"
                        );
                    }
                }
                Err(crate::error::LooperError::ProviderSpawn { binary, source }) => {
                    summary.iterations_run += 1;
                    summary.failures += 1;
                    let msg = format!("failed to spawn '{binary}': {source}");
                    error!(iteration = i, provider = self.adapter.name(), "{msg}");
                    summary.termination_reason = Some(TerminationReason::ProviderError(msg));
                    break;
                }
                Err(e) => {
                    summary.iterations_run += 1;
                    summary.failures += 1;
                    error!(iteration = i, provider = self.adapter.name(), error = %e, "Iteration error");
                }
            }

            let _ = iter_start; // elapsed captured inside result.duration
        }

        if summary.termination_reason.is_none() {
            if self.interrupted.load(Ordering::SeqCst) {
                summary.termination_reason = Some(TerminationReason::Interrupted);
            } else {
                summary.termination_reason = Some(TerminationReason::Completed);
            }
        }

        let total_ms = session_start.elapsed().as_millis();
        info!(
            total_duration_ms = total_ms,
            termination_reason = %summary.termination_reason.as_ref().unwrap(),
            "Loop finished"
        );

        summary.print();
        summary
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LoopConfig, Provider};
    use crate::provider::tests::FakeAdapter;

    fn config_with_iterations(n: i64) -> LoopConfig {
        LoopConfig {
            iterations: n,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn runs_exact_iterations_on_success() {
        let config = config_with_iterations(3);
        let adapter = FakeAdapter::success("fake");
        let engine = LoopEngine::with_adapter(config, Box::new(adapter));
        let summary = engine.run();
        assert_eq!(summary.iterations_run, 3);
        assert_eq!(summary.successes, 3);
        assert_eq!(summary.failures, 0);
        assert_eq!(
            summary.termination_reason,
            Some(TerminationReason::Completed)
        );
    }

    #[test]
    fn counts_failures_correctly() {
        let config = config_with_iterations(4);
        let adapter = FakeAdapter::failure("fake");
        let engine = LoopEngine::with_adapter(config, Box::new(adapter));
        let summary = engine.run();
        assert_eq!(summary.iterations_run, 4);
        assert_eq!(summary.successes, 0);
        assert_eq!(summary.failures, 4);
        assert_eq!(
            summary.termination_reason,
            Some(TerminationReason::Completed)
        );
    }

    #[test]
    fn single_iteration() {
        let config = config_with_iterations(1);
        let adapter = FakeAdapter::success("fake");
        let engine = LoopEngine::with_adapter(config, Box::new(adapter));
        let summary = engine.run();
        assert_eq!(summary.iterations_run, 1);
        assert_eq!(summary.successes, 1);
        assert_eq!(
            summary.termination_reason,
            Some(TerminationReason::Completed)
        );
    }

    #[test]
    fn spawn_error_terminates_loop() {
        use crate::error::LooperError;
        use crate::provider::ExecutionResult;

        struct SpawnFailAdapter;
        impl ProviderAdapter for SpawnFailAdapter {
            fn name(&self) -> &str {
                "fail"
            }
            fn execute(&self, _prompt: &str) -> Result<ExecutionResult, LooperError> {
                Err(LooperError::ProviderSpawn {
                    binary: "nonexistent".to_string(),
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, "binary not found"),
                })
            }
        }

        let config = config_with_iterations(5);
        let engine = LoopEngine::with_adapter(config, Box::new(SpawnFailAdapter));
        let summary = engine.run();
        // Loop should abort after first spawn failure.
        assert_eq!(summary.iterations_run, 1);
        assert_eq!(summary.failures, 1);
        assert!(matches!(
            summary.termination_reason,
            Some(TerminationReason::ProviderError(_))
        ));
    }

    #[test]
    fn termination_reason_display() {
        assert_eq!(TerminationReason::Completed.to_string(), "completed");
        assert_eq!(TerminationReason::Interrupted.to_string(), "interrupted");
        assert_eq!(
            TerminationReason::ProviderError("bad".to_string()).to_string(),
            "provider error: bad"
        );
    }

    #[test]
    fn reads_prompt_from_inline() {
        let config = LoopConfig {
            iterations: 1,
            provider: Provider::Claude,
            prompt_inline: Some("hello world".to_string()),
            ..Default::default()
        };
        let adapter = FakeAdapter::success("fake");
        let engine = LoopEngine::with_adapter(config, Box::new(adapter));
        let summary = engine.run();
        assert_eq!(summary.successes, 1);
    }

    #[test]
    fn reads_prompt_from_file() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "prompt from file").unwrap();

        let config = LoopConfig {
            iterations: 1,
            provider: Provider::Claude,
            prompt_file: Some(f.path().to_path_buf()),
            ..Default::default()
        };
        let adapter = FakeAdapter::success("fake");
        let engine = LoopEngine::with_adapter(config, Box::new(adapter));
        let summary = engine.run();
        assert_eq!(summary.successes, 1);
    }
}
