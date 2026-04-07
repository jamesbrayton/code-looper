use crate::config::{CommentCadence, IssueTrackingMode, LoopConfig};
use crate::issue_tracker::{GitHubIssueTracker, IssueTracker, LocalPromiseTracker};
use crate::orchestration::{GhCliContextResolver, PolicyEngine};
use crate::policy_guard::PolicyGuard;
use crate::pr_strategy::{PrStrategy, build_strategy};
use crate::provider::{build_adapter, ProviderAdapter};
use crate::telemetry::{
    IterationOutcome, IterationRecord, RunArtifacts, RunManifest, unix_now,
};
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
    /// An iteration failed and `stop_on_failure` was set.
    StoppedOnFailure,
}

impl std::fmt::Display for TerminationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerminationReason::Completed => write!(f, "completed"),
            TerminationReason::Interrupted => write!(f, "interrupted"),
            TerminationReason::ProviderError(msg) => write!(f, "provider error: {msg}"),
            TerminationReason::StoppedOnFailure => write!(f, "stopped on failure"),
        }
    }
}

/// Aggregate statistics for a finished loop session.
#[derive(Debug, Default)]
pub struct SessionSummary {
    pub iterations_run: u64,
    pub successes: u64,
    pub failures: u64,
    pub retries: u64,
    pub termination_reason: Option<TerminationReason>,
}

impl SessionSummary {
    fn print(&self) {
        info!(
            iterations_run = self.iterations_run,
            successes = self.successes,
            failures = self.failures,
            retries = self.retries,
            termination_reason = %self.termination_reason.as_ref().map(|r| r.to_string()).unwrap_or_default(),
            "Session summary"
        );
        println!();
        println!("─── Session Summary ────────────────────────────");
        println!("  Iterations run : {}", self.iterations_run);
        println!("  Successes      : {}", self.successes);
        println!("  Failures       : {}", self.failures);
        println!("  Retries        : {}", self.retries);
        if let Some(reason) = &self.termination_reason {
            println!("  Termination    : {reason}");
        }
        println!("────────────────────────────────────────────────");
    }
}

/// Construct the appropriate `IssueTracker` from the resolved config.
fn build_tracker(config: &LoopConfig) -> Box<dyn IssueTracker> {
    match config.issue_tracking.mode {
        IssueTrackingMode::Github => {
            let owner = config
                .issue_tracking
                .repo_owner
                .clone()
                .or_else(|| config.orchestration.repo_owner.clone())
                .unwrap_or_default();
            let repo = config
                .issue_tracking
                .repo_name
                .clone()
                .or_else(|| config.orchestration.repo_name.clone())
                .unwrap_or_default();
            Box::new(GitHubIssueTracker::new(owner, repo))
        }
        IssueTrackingMode::Local => {
            let path = config
                .issue_tracking
                .local_promise_path
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from(".code-looper/promise.md"));
            Box::new(LocalPromiseTracker::new(path))
        }
    }
}

/// Drives the main iteration loop.
pub struct LoopEngine {
    config: LoopConfig,
    adapter: Box<dyn ProviderAdapter>,
    /// Optional orchestration policy engine (present when orchestration is enabled).
    policy_engine: Option<PolicyEngine>,
    /// Policy guard used to augment prompts with MCP-use requirements.
    guard: PolicyGuard,
    /// Issue tracker for this run.
    tracker: Box<dyn IssueTracker>,
    /// PR strategy: consulted once per iteration before the provider is invoked.
    pr_strategy: Box<dyn PrStrategy>,
    /// Shared flag set to `true` when SIGINT is received.
    interrupted: Arc<AtomicBool>,
}

impl LoopEngine {
    pub fn new(config: LoopConfig, guard: PolicyGuard) -> Self {
        let adapter = build_adapter(&config.provider, config.telemetry.stream_output);
        let policy_engine = if config.orchestration.enabled {
            let owner = config.orchestration.repo_owner.clone().unwrap_or_default();
            let repo = config.orchestration.repo_name.clone().unwrap_or_default();
            Some(PolicyEngine::new(Box::new(GhCliContextResolver { owner, repo })))
        } else {
            None
        };
        let tracker = build_tracker(&config);
        let pr_strategy = build_strategy(config.pr_management.clone());
        let interrupted = Arc::new(AtomicBool::new(false));
        Self { config, adapter, policy_engine, guard, tracker, pr_strategy, interrupted }
    }

    /// Constructor that accepts a custom adapter; uses a default (safe) policy guard.
    #[allow(dead_code)]
    pub fn with_adapter(config: LoopConfig, adapter: Box<dyn ProviderAdapter>) -> Self {
        let interrupted = Arc::new(AtomicBool::new(false));
        let guard = PolicyGuard::new(crate::policy_guard::UnsafeOverrides::default());
        let tracker = build_tracker(&config);
        let pr_strategy = build_strategy(config.pr_management.clone());
        Self { config, adapter, policy_engine: None, guard, tracker, pr_strategy, interrupted }
    }

    /// Constructor that accepts a custom adapter and issue tracker (useful for testing).
    #[cfg(test)]
    pub fn with_adapter_and_tracker(
        config: LoopConfig,
        adapter: Box<dyn ProviderAdapter>,
        tracker: Box<dyn IssueTracker>,
    ) -> Self {
        let interrupted = Arc::new(AtomicBool::new(false));
        let guard = PolicyGuard::new(crate::policy_guard::UnsafeOverrides::default());
        let pr_strategy = build_strategy(config.pr_management.clone());
        Self { config, adapter, policy_engine: None, guard, tracker, pr_strategy, interrupted }
    }

    /// Constructor that accepts a custom adapter and policy engine (useful for testing).
    #[allow(dead_code)]
    pub fn with_adapter_and_policy(
        config: LoopConfig,
        adapter: Box<dyn ProviderAdapter>,
        policy_engine: PolicyEngine,
    ) -> Self {
        let interrupted = Arc::new(AtomicBool::new(false));
        let guard = PolicyGuard::new(crate::policy_guard::UnsafeOverrides::default());
        let tracker = build_tracker(&config);
        let pr_strategy = build_strategy(config.pr_management.clone());
        Self { config, adapter, policy_engine: Some(policy_engine), guard, tracker, pr_strategy, interrupted }
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

    /// Post a comment to the configured issue, logging a warning on failure.
    ///
    /// No-ops when `comment_issue_number` is `None` or when in local mode.
    fn post_comment(&self, body: &str) {
        let issue_number = match self.config.issue_tracking.comment_issue_number {
            Some(n) => n,
            None => return,
        };
        if self.config.issue_tracking.mode != IssueTrackingMode::Github {
            return;
        }
        if let Err(e) = self.tracker.add_comment(issue_number, body) {
            warn!(issue = issue_number, error = %e, "Failed to post comment to issue");
        }
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

        let prompt_source = if self.config.prompt_file.is_some() {
            "file"
        } else if self.config.prompt_inline.is_some() {
            "inline"
        } else {
            "none"
        };

        let mut summary = SessionSummary::default();
        let run_started_at = unix_now();
        let session_start = Instant::now();

        // Set up run artifact directory.
        let artifacts = RunArtifacts::create(
            &self.config.telemetry.artifacts_dir,
            // Only persist artifacts when not in no_summary mode.  The
            // directory is always created when telemetry is on by default.
            !self.config.telemetry.no_summary,
        );
        let mut iteration_records: Vec<IterationRecord> = Vec::new();

        // Log issue tracking mode; warn loudly when running in local/dev mode.
        match self.config.issue_tracking.mode {
            IssueTrackingMode::Local => {
                warn!(
                    issue_tracking_mode = "local",
                    "Issue tracking is in LOCAL mode — run state is not durably tracked. \
                     Set issue_tracking.mode=\"github\" for production use."
                );
            }
            IssueTrackingMode::Github => {
                let owner = self
                    .config
                    .issue_tracking
                    .repo_owner
                    .as_deref()
                    .or(self.config.orchestration.repo_owner.as_deref())
                    .unwrap_or("(unknown)");
                let repo = self
                    .config
                    .issue_tracking
                    .repo_name
                    .as_deref()
                    .or(self.config.orchestration.repo_name.as_deref())
                    .unwrap_or("(unknown)");
                info!(
                    issue_tracking_mode = "github",
                    repo = %format!("{owner}/{repo}"),
                    "Issue tracking active"
                );
                // Ensure standard labels exist on the repository.
                let labels = self.config.issue_tracking.standard_labels.clone();
                if !labels.is_empty() {
                    if let Err(e) = self.tracker.ensure_labels(&labels) {
                        warn!(error = %e, "Failed to ensure standard labels on repository");
                    } else {
                        info!(labels = ?labels, "Standard labels ensured on repository");
                    }
                }
            }
        }

        info!(
            provider = self.adapter.name(),
            run_id = %artifacts.run_id,
            iterations = if infinite {
                "infinite".to_string()
            } else {
                max.to_string()
            },
            prompt_source,
            "Loop starting"
        );

        // Post run-start comment when a linked issue is configured.
        if self.config.issue_tracking.comment_cadence != CommentCadence::OffEngine {
            let iter_display = if infinite {
                "infinite".to_string()
            } else {
                max.to_string()
            };
            self.post_comment(&format!(
                "**Loop run started** — run-id: `{run_id}`, provider: `{provider}`, \
                 iterations: `{iter_display}`, prompt-source: `{prompt_source}`",
                run_id = artifacts.run_id,
                provider = self.adapter.name(),
            ));
        }

        // Tracks the body of the most recently posted failure comment for deduplication.
        let mut last_failure_comment: Option<String> = None;

        for i in 1..=max {
            if self.interrupted.load(Ordering::SeqCst) {
                summary.termination_reason = Some(TerminationReason::Interrupted);
                break;
            }

            let iter_started_at = unix_now();
            let iter_start = Instant::now();

            // Consult the PR strategy before the provider is invoked.
            let pr_plan = self.pr_strategy.plan_iteration(i);
            info!(
                iteration = i,
                pr_mode = %pr_plan.mode,
                pr_plan = %pr_plan.description,
                "PR strategy plan"
            );

            // If orchestration is enabled, select a workflow branch and use its prompt.
            let (raw_prompt, workflow_branch) = if let Some(ref engine) = self.policy_engine {
                match engine.select_branch() {
                    Ok((branch, _ctx)) => {
                        let branch_name = branch.to_string();
                        info!(
                            iteration = i,
                            provider = self.adapter.name(),
                            workflow_branch = %branch_name,
                            "Iteration start"
                        );
                        let p = branch.default_prompt().to_string();
                        (p, Some(branch_name))
                    }
                    Err(e) => {
                        error!(iteration = i, "Policy engine failed: {e}");
                        let outcome = IterationOutcome::PolicyGuardBlock {
                            message: e.to_string(),
                        };
                        iteration_records.push(IterationRecord {
                            iteration: i,
                            provider: self.adapter.name().to_string(),
                            prompt_source: prompt_source.to_string(),
                            workflow_branch: None,
                            outcome: outcome.clone(),
                            duration_ms: iter_start.elapsed().as_millis(),
                            retries: 0,
                            stderr_excerpt: Some(e.to_string()),
                            transcript_path: None,
                            started_at: iter_started_at,
                        });
                        summary.iterations_run += 1;
                        summary.failures += 1;
                        summary.termination_reason =
                            Some(TerminationReason::ProviderError(e.to_string()));
                        break;
                    }
                }
            } else {
                info!(
                    iteration = i,
                    provider = self.adapter.name(),
                    "Iteration start"
                );
                (prompt.clone(), None)
            };

            // Augment prompt with MCP-use preamble (no-op when allow_direct_github is set).
            let effective_prompt = self.guard.augment_prompt(&raw_prompt);

            // Execute with retry/backoff.
            let mut attempt = 0u32;
            #[allow(unused_assignments)]
            let mut final_outcome = IterationOutcome::Unknown;
            let mut final_stdout = String::new();
            let mut final_stderr = String::new();
            let mut final_duration_ms = 0u128;
            let mut abort_loop = false;

            loop {
                match self.adapter.execute(&effective_prompt) {
                    Ok(result) => {
                        let duration_ms = result.duration.as_millis();
                        final_duration_ms = duration_ms;
                        final_stdout = result.stdout.clone();
                        final_stderr = result.stderr.clone();

                        if result.succeeded() {
                            final_outcome = IterationOutcome::Success;
                            if attempt > 0 {
                                info!(
                                    iteration = i,
                                    attempt = attempt + 1,
                                    provider = self.adapter.name(),
                                    exit_code = result.exit_code,
                                    duration_ms,
                                    "Iteration succeeded after retry"
                                );
                            } else {
                                info!(
                                    iteration = i,
                                    provider = self.adapter.name(),
                                    exit_code = result.exit_code,
                                    duration_ms,
                                    output = %result.stdout.trim(),
                                    "Iteration succeeded"
                                );
                            }
                            break;
                        } else if attempt < self.config.max_retries {
                            summary.retries += 1;
                            warn!(
                                iteration = i,
                                attempt = attempt + 1,
                                max_retries = self.config.max_retries,
                                provider = self.adapter.name(),
                                exit_code = result.exit_code,
                                backoff_ms = self.config.retry_backoff_ms,
                                "Iteration failed, retrying"
                            );
                            std::thread::sleep(std::time::Duration::from_millis(
                                self.config.retry_backoff_ms,
                            ));
                            attempt += 1;
                        } else {
                            final_outcome = IterationOutcome::from_exit_code(result.exit_code);
                            let first_err =
                                IterationRecord::stderr_first_line(&result.stderr);
                            warn!(
                                iteration = i,
                                provider = self.adapter.name(),
                                exit_code = result.exit_code,
                                duration_ms,
                                stderr = %result.stderr.trim(),
                                "Iteration failed (non-zero exit)"
                            );
                            if let Some(ref excerpt) = first_err {
                                warn!(iteration = i, stderr_first_line = %excerpt, "");
                            }
                            break;
                        }
                    }
                    Err(crate::error::LooperError::ProviderSpawn { binary, source }) => {
                        let msg = format!("failed to spawn '{binary}': {source}");
                        error!(iteration = i, provider = self.adapter.name(), "{msg}");
                        final_outcome =
                            IterationOutcome::SpawnFailure { message: msg.clone() };
                        if self.config.issue_tracking.comment_cadence != CommentCadence::OffEngine {
                            self.post_comment(&format!(
                                "**Blocker** — iteration {i} aborted: provider spawn failure. \
                                 `{msg}`"
                            ));
                        }
                        summary.termination_reason = Some(TerminationReason::ProviderError(msg));
                        abort_loop = true;
                        break;
                    }
                    Err(e) => {
                        error!(iteration = i, provider = self.adapter.name(), error = %e, "Iteration error");
                        final_outcome = IterationOutcome::Unknown;
                        break;
                    }
                }
            }

            // Persist iteration transcript and build the record.
            let transcript_path = artifacts.write_transcript(i, &final_stdout, &final_stderr);
            let stderr_excerpt = IterationRecord::stderr_first_line(&final_stderr);

            info!(
                iteration = i,
                outcome = final_outcome.label(),
                duration_ms = final_duration_ms,
                retries = attempt,
                "Iteration complete"
            );

            // Post iteration comment based on cadence.
            let cadence = &self.config.issue_tracking.comment_cadence;
            let is_failure = !final_outcome.is_success();
            let should_comment = match cadence {
                CommentCadence::OffEngine => false,
                CommentCadence::EveryIteration => true,
                CommentCadence::Milestones => is_failure || abort_loop,
            };
            if should_comment {
                let retry_note = if attempt > 0 {
                    format!(", retries: {attempt}")
                } else {
                    String::new()
                };
                let err_note = if let Some(ref exc) = IterationRecord::stderr_first_line(&final_stderr) {
                    format!("\n> `{exc}`")
                } else {
                    String::new()
                };
                let comment_body = format!(
                    "**Iteration {i}** — outcome: `{outcome}`, duration: {ms}ms{retry}{err}",
                    outcome = final_outcome.label(),
                    ms = final_duration_ms,
                    retry = retry_note,
                    err = err_note,
                );
                // Deduplicate consecutive identical failure comments.
                let is_duplicate = last_failure_comment.as_deref() == Some(&comment_body);
                if !is_duplicate {
                    self.post_comment(&comment_body);
                    if is_failure {
                        last_failure_comment = Some(comment_body);
                    } else {
                        last_failure_comment = None;
                    }
                }
            } else if !is_failure {
                last_failure_comment = None;
            }

            // Post blocker comment when aborting due to stop_on_failure.
            let stop_on_fail = !final_outcome.is_success() && self.config.stop_on_failure && !abort_loop;
            if stop_on_fail && cadence != &CommentCadence::OffEngine {
                self.post_comment(&format!(
                    "**Blocker** — iteration {i} failed and `stop_on_failure` is set; \
                     halting loop. Outcome: `{outcome}`",
                    outcome = final_outcome.label(),
                ));
            }

            iteration_records.push(IterationRecord {
                iteration: i,
                provider: self.adapter.name().to_string(),
                prompt_source: prompt_source.to_string(),
                workflow_branch: workflow_branch.clone(),
                outcome: final_outcome.clone(),
                duration_ms: final_duration_ms,
                retries: attempt,
                stderr_excerpt,
                transcript_path,
                started_at: iter_started_at,
            });

            summary.iterations_run += 1;
            if final_outcome.is_success() {
                summary.successes += 1;
            } else {
                summary.failures += 1;
            }

            if abort_loop {
                break;
            }

            if !final_outcome.is_success() && self.config.stop_on_failure {
                info!(iteration = i, "stop_on_failure is set; halting loop after failed iteration");
                summary.termination_reason = Some(TerminationReason::StoppedOnFailure);
                break;
            }
        }

        if summary.termination_reason.is_none() {
            if self.interrupted.load(Ordering::SeqCst) {
                summary.termination_reason = Some(TerminationReason::Interrupted);
            } else {
                summary.termination_reason = Some(TerminationReason::Completed);
            }
        }

        let total_ms = session_start.elapsed().as_millis();
        let run_ended_at = unix_now();

        info!(
            total_duration_ms = total_ms,
            termination_reason = %summary.termination_reason.as_ref().unwrap(),
            "Loop finished"
        );

        summary.print();

        // End-of-run owned-issue lifecycle verification.
        // When a linked issue is configured and we're in GitHub mode, check
        // whether the issue is still open.  If the run succeeded and all work
        // appears done, warn (or close, when auto_close_owned_issues=true).
        if self.config.issue_tracking.mode == IssueTrackingMode::Github {
            if let Some(issue_number) = self.config.issue_tracking.comment_issue_number {
                match self.tracker.get_issue(issue_number) {
                    Ok(issue) if issue.state == crate::issue_tracker::IssueState::Open => {
                        if self.config.issue_tracking.auto_close_owned_issues {
                            info!(
                                issue = issue_number,
                                "auto_close_owned_issues: closing issue at end of run"
                            );
                            let close_comment = format!(
                                "Loop run `{run_id}` completed — closing issue automatically \
                                 (`auto_close_owned_issues=true`).",
                                run_id = artifacts.run_id
                            );
                            let _ = self.tracker.add_comment(issue_number, &close_comment);
                            if let Err(e) = self.tracker.close_issue(
                                issue_number,
                                crate::issue_tracker::CloseReason::Completed,
                            ) {
                                warn!(
                                    issue = issue_number,
                                    error = %e,
                                    "auto_close_owned_issues: failed to close issue"
                                );
                            }
                        } else {
                            warn!(
                                issue = issue_number,
                                "Owned issue is still open at end of run. \
                                 Set auto_close_owned_issues=true to close automatically."
                            );
                        }
                    }
                    Ok(_) => {
                        info!(issue = issue_number, "Owned issue is closed — lifecycle complete");
                    }
                    Err(e) => {
                        warn!(
                            issue = issue_number,
                            error = %e,
                            "Could not verify owned issue state at end of run"
                        );
                    }
                }
            }
        }

        // Post run-end comment.
        if self.config.issue_tracking.comment_cadence != CommentCadence::OffEngine {
            let reason = summary
                .termination_reason
                .as_ref()
                .map(|r| r.to_string())
                .unwrap_or_default();
            self.post_comment(&format!(
                "**Loop run finished** — iterations: {iters}, successes: {ok}, \
                 failures: {fail}, retries: {retries}, termination: `{reason}`",
                iters = summary.iterations_run,
                ok = summary.successes,
                fail = summary.failures,
                retries = summary.retries,
            ));
        }

        // Write run manifest and summary.
        let manifest = RunManifest {
            run_id: artifacts.run_id.clone(),
            started_at: run_started_at,
            ended_at: Some(run_ended_at),
            provider: self.adapter.name().to_string(),
            iterations_requested: self.config.iterations,
            termination_reason: summary
                .termination_reason
                .as_ref()
                .map(|r| r.to_string()),
            iterations: iteration_records,
        };
        artifacts.write_manifest(&manifest);
        if let Some(summary_path) =
            artifacts.write_summary(&manifest, self.config.telemetry.no_summary)
        {
            info!(path = %summary_path.display(), "Run summary written");
        }

        // Prune old run directories.
        RunArtifacts::prune_old_runs(
            &self.config.telemetry.artifacts_dir,
            self.config.telemetry.keep_runs,
        );

        // Run completion hook if configured.
        if let Some(cmd) = &self.config.on_complete {
            info!(command = %cmd, "Running on_complete hook");
            match std::process::Command::new("sh").args(["-c", cmd]).status() {
                Ok(status) => {
                    if status.success() {
                        info!(command = %cmd, "on_complete hook succeeded");
                    } else {
                        warn!(
                            command = %cmd,
                            exit_code = status.code().unwrap_or(-1),
                            "on_complete hook exited with non-zero status"
                        );
                    }
                }
                Err(e) => {
                    error!(command = %cmd, error = %e, "Failed to spawn on_complete hook");
                }
            }
        }

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
        assert_eq!(TerminationReason::StoppedOnFailure.to_string(), "stopped on failure");
    }

    #[test]
    fn stop_on_failure_halts_after_first_failed_iteration() {
        let config = LoopConfig {
            iterations: 5,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            stop_on_failure: true,
            ..Default::default()
        };
        let adapter = FakeAdapter::failure("fake");
        let engine = LoopEngine::with_adapter(config, Box::new(adapter));
        let summary = engine.run();
        assert_eq!(summary.iterations_run, 1);
        assert_eq!(summary.failures, 1);
        assert_eq!(summary.termination_reason, Some(TerminationReason::StoppedOnFailure));
    }

    #[test]
    fn stop_on_failure_false_continues_after_failure() {
        let config = LoopConfig {
            iterations: 3,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            stop_on_failure: false,
            ..Default::default()
        };
        let adapter = FakeAdapter::failure("fake");
        let engine = LoopEngine::with_adapter(config, Box::new(adapter));
        let summary = engine.run();
        assert_eq!(summary.iterations_run, 3);
        assert_eq!(summary.failures, 3);
        assert_eq!(summary.termination_reason, Some(TerminationReason::Completed));
    }

    #[test]
    fn retries_counted_in_summary_on_repeated_failure() {
        use crate::error::LooperError;
        use crate::provider::ExecutionResult;
        use std::time::Duration;

        // Adapter that always returns exit code 1.
        struct AlwaysFailAdapter;
        impl ProviderAdapter for AlwaysFailAdapter {
            fn name(&self) -> &str { "always-fail" }
            fn execute(&self, _prompt: &str) -> Result<ExecutionResult, LooperError> {
                Ok(ExecutionResult {
                    exit_code: Some(1),
                    stdout: String::new(),
                    stderr: "error".to_string(),
                    duration: Duration::from_millis(1),
                })
            }
        }

        let config = LoopConfig {
            iterations: 1,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            max_retries: 2,
            retry_backoff_ms: 0, // no sleep in tests
            ..Default::default()
        };
        let engine = LoopEngine::with_adapter(config, Box::new(AlwaysFailAdapter));
        let summary = engine.run();
        assert_eq!(summary.iterations_run, 1);
        assert_eq!(summary.failures, 1);
        assert_eq!(summary.retries, 2);
    }

    #[test]
    fn retry_succeeds_on_second_attempt() {
        use crate::error::LooperError;
        use crate::provider::ExecutionResult;
        use std::sync::atomic::{AtomicU32, Ordering as AtomOrd};
        use std::sync::Arc;
        use std::time::Duration;

        // Adapter that fails on first call, succeeds on subsequent calls.
        struct FlipFlopAdapter {
            calls: Arc<AtomicU32>,
        }
        impl ProviderAdapter for FlipFlopAdapter {
            fn name(&self) -> &str { "flip-flop" }
            fn execute(&self, _prompt: &str) -> Result<ExecutionResult, LooperError> {
                let n = self.calls.fetch_add(1, AtomOrd::SeqCst);
                Ok(ExecutionResult {
                    exit_code: if n == 0 { Some(1) } else { Some(0) },
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: Duration::from_millis(1),
                })
            }
        }

        let config = LoopConfig {
            iterations: 1,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            max_retries: 3,
            retry_backoff_ms: 0,
            ..Default::default()
        };
        let calls = Arc::new(AtomicU32::new(0));
        let adapter = FlipFlopAdapter { calls };
        let engine = LoopEngine::with_adapter(config, Box::new(adapter));
        let summary = engine.run();
        assert_eq!(summary.iterations_run, 1);
        assert_eq!(summary.successes, 1);
        assert_eq!(summary.failures, 0);
        assert_eq!(summary.retries, 1);
    }

    #[test]
    fn on_complete_hook_runs_without_error() {
        // Use a shell command that always succeeds.
        let config = LoopConfig {
            iterations: 1,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            on_complete: Some("true".to_string()),
            ..Default::default()
        };
        let adapter = FakeAdapter::success("fake");
        let engine = LoopEngine::with_adapter(config, Box::new(adapter));
        let summary = engine.run();
        // The hook runs after run() returns the summary — just confirm loop completed.
        assert_eq!(summary.termination_reason, Some(TerminationReason::Completed));
    }

    #[test]
    fn retries_zero_means_no_retry() {
        let config = LoopConfig {
            iterations: 2,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            max_retries: 0,
            ..Default::default()
        };
        let adapter = FakeAdapter::failure("fake");
        let engine = LoopEngine::with_adapter(config, Box::new(adapter));
        let summary = engine.run();
        assert_eq!(summary.retries, 0);
        assert_eq!(summary.failures, 2);
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

    #[test]
    fn orchestration_selects_branch_and_succeeds() {
        use crate::config::OrchestrationConfig;
        use crate::orchestration::tests::StubContextResolver;
        use crate::orchestration::{PolicyEngine, RepoContext};

        let config = LoopConfig {
            iterations: 2,
            provider: Provider::Claude,
            orchestration: OrchestrationConfig {
                enabled: true,
                repo_owner: Some("owner".to_string()),
                repo_name: Some("repo".to_string()),
            },
            ..Default::default()
        };
        let resolver = StubContextResolver {
            context: RepoContext { open_pr_count: 0, open_issue_count: 1 },
        };
        let policy_engine = PolicyEngine::new(Box::new(resolver));
        let adapter = FakeAdapter::success("fake");
        let engine = LoopEngine::with_adapter_and_policy(config, Box::new(adapter), policy_engine);
        let summary = engine.run();
        assert_eq!(summary.iterations_run, 2);
        assert_eq!(summary.successes, 2);
        assert_eq!(summary.termination_reason, Some(TerminationReason::Completed));
    }

    #[test]
    fn orchestration_policy_error_terminates_loop() {
        use crate::config::OrchestrationConfig;
        use crate::error::LooperError;
        use crate::orchestration::{ContextResolver, PolicyEngine, RepoContext};

        struct FailingResolver;
        impl ContextResolver for FailingResolver {
            fn resolve(&self) -> Result<RepoContext, LooperError> {
                Err(LooperError::InvalidArgument("gh failed".to_string()))
            }
        }

        let config = LoopConfig {
            iterations: 3,
            provider: Provider::Claude,
            orchestration: OrchestrationConfig {
                enabled: true,
                repo_owner: Some("owner".to_string()),
                repo_name: Some("repo".to_string()),
            },
            ..Default::default()
        };
        let engine = LoopEngine::with_adapter_and_policy(
            config,
            Box::new(FakeAdapter::success("fake")),
            PolicyEngine::new(Box::new(FailingResolver)),
        );
        let summary = engine.run();
        // Should abort on the first policy failure.
        assert_eq!(summary.iterations_run, 1);
        assert_eq!(summary.failures, 1);
        assert!(matches!(
            summary.termination_reason,
            Some(TerminationReason::ProviderError(_))
        ));
    }

    // ── Engine-driven issue comment tests ────────────────────────────────────

    use crate::config::{CommentCadence, IssueTrackingConfig, IssueTrackingMode};
    use crate::issue_tracker::{MockCall, MockIssueTracker};
    use std::sync::Arc;

    fn github_tracker_config(issue: u32, cadence: CommentCadence) -> LoopConfig {
        LoopConfig {
            iterations: 2,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: Some("owner".to_string()),
                repo_name: Some("repo".to_string()),
                comment_issue_number: Some(issue),
                comment_cadence: cadence,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn milestones_cadence_posts_start_and_end_on_success() {
        let tracker = Arc::new(MockIssueTracker::new());
        let config = github_tracker_config(42, CommentCadence::Milestones);
        let engine = LoopEngine::with_adapter_and_tracker(
            config,
            Box::new(FakeAdapter::success("fake")),
            Box::new(crate::issue_tracker::SharedMockIssueTracker(Arc::clone(&tracker))),
        );
        engine.run();
        let calls = tracker.recorded_calls();
        // With milestones cadence and all successes: start comment + end comment only.
        let add_comment_calls: Vec<_> = calls
            .iter()
            .filter(|c| matches!(c, MockCall::AddComment { .. }))
            .collect();
        assert_eq!(add_comment_calls.len(), 2, "expected start + end comments, got {add_comment_calls:?}");
        if let MockCall::AddComment { number, body } = &add_comment_calls[0] {
            assert_eq!(*number, 42);
            assert!(body.contains("Loop run started"), "start comment body: {body}");
        }
        if let MockCall::AddComment { number, body } = &add_comment_calls[1] {
            assert_eq!(*number, 42);
            assert!(body.contains("Loop run finished"), "end comment body: {body}");
        }
    }

    #[test]
    fn milestones_cadence_posts_failure_comment() {
        let tracker = Arc::new(MockIssueTracker::new());
        let config = github_tracker_config(7, CommentCadence::Milestones);
        let engine = LoopEngine::with_adapter_and_tracker(
            config,
            Box::new(FakeAdapter::failure("fake")),
            Box::new(crate::issue_tracker::SharedMockIssueTracker(Arc::clone(&tracker))),
        );
        engine.run();
        let calls = tracker.recorded_calls();
        let comment_bodies: Vec<_> = calls
            .iter()
            .filter_map(|c| {
                if let MockCall::AddComment { body, .. } = c { Some(body.as_str()) } else { None }
            })
            .collect();
        // start + 2 failure iteration comments (deduplicated if identical) + end
        assert!(
            comment_bodies.iter().any(|b| b.contains("Loop run started")),
            "missing start comment; got: {comment_bodies:?}"
        );
        assert!(
            comment_bodies.iter().any(|b| b.contains("Loop run finished")),
            "missing end comment"
        );
        assert!(
            comment_bodies.iter().any(|b| b.contains("Iteration")),
            "missing iteration comment"
        );
    }

    #[test]
    fn every_iteration_cadence_posts_comment_per_iteration() {
        let tracker = Arc::new(MockIssueTracker::new());
        let config = github_tracker_config(1, CommentCadence::EveryIteration);
        let engine = LoopEngine::with_adapter_and_tracker(
            config,
            Box::new(FakeAdapter::success("fake")),
            Box::new(crate::issue_tracker::SharedMockIssueTracker(Arc::clone(&tracker))),
        );
        engine.run();
        let calls = tracker.recorded_calls();
        let add_comment_count = calls
            .iter()
            .filter(|c| matches!(c, MockCall::AddComment { .. }))
            .count();
        // 1 start + 2 iteration comments + 1 end = 4
        assert_eq!(add_comment_count, 4, "expected 4 comments for 2 iterations, got {add_comment_count}");
    }

    #[test]
    fn off_engine_cadence_posts_no_comments() {
        let tracker = Arc::new(MockIssueTracker::new());
        let config = github_tracker_config(99, CommentCadence::OffEngine);
        let engine = LoopEngine::with_adapter_and_tracker(
            config,
            Box::new(FakeAdapter::success("fake")),
            Box::new(crate::issue_tracker::SharedMockIssueTracker(Arc::clone(&tracker))),
        );
        engine.run();
        let calls = tracker.recorded_calls();
        let has_comment = calls.iter().any(|c| matches!(c, MockCall::AddComment { .. }));
        assert!(!has_comment, "off-engine cadence should post no comments");
    }

    #[test]
    fn local_mode_posts_no_comments() {
        let tracker = Arc::new(MockIssueTracker::new());
        let config = LoopConfig {
            iterations: 1,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Local, // local mode — no comments
                comment_issue_number: Some(5),
                comment_cadence: CommentCadence::EveryIteration,
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = LoopEngine::with_adapter_and_tracker(
            config,
            Box::new(FakeAdapter::success("fake")),
            Box::new(crate::issue_tracker::SharedMockIssueTracker(Arc::clone(&tracker))),
        );
        engine.run();
        let calls = tracker.recorded_calls();
        assert!(
            !calls.iter().any(|c| matches!(c, MockCall::AddComment { .. })),
            "local mode should never post comments"
        );
    }

    // ── Label-ensure and ownership verification tests ────────────────────────

    #[test]
    fn github_mode_calls_ensure_labels_at_startup() {
        let tracker = Arc::new(MockIssueTracker::new());
        let config = LoopConfig {
            iterations: 1,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: Some("o".to_string()),
                repo_name: Some("r".to_string()),
                comment_issue_number: None,
                comment_cadence: CommentCadence::OffEngine,
                standard_labels: vec!["bug".to_string(), "tech-debt".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = LoopEngine::with_adapter_and_tracker(
            config,
            Box::new(FakeAdapter::success("fake")),
            Box::new(crate::issue_tracker::SharedMockIssueTracker(Arc::clone(&tracker))),
        );
        engine.run();
        let calls = tracker.recorded_calls();
        assert!(
            calls.iter().any(|c| matches!(c, MockCall::EnsureLabels(l) if l.contains(&"bug".to_string()))),
            "expected EnsureLabels call; got: {calls:?}"
        );
    }

    #[test]
    fn local_mode_does_not_call_ensure_labels() {
        let tracker = Arc::new(MockIssueTracker::new());
        let config = LoopConfig {
            iterations: 1,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Local,
                comment_cadence: CommentCadence::OffEngine,
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = LoopEngine::with_adapter_and_tracker(
            config,
            Box::new(FakeAdapter::success("fake")),
            Box::new(crate::issue_tracker::SharedMockIssueTracker(Arc::clone(&tracker))),
        );
        engine.run();
        let calls = tracker.recorded_calls();
        assert!(
            !calls.iter().any(|c| matches!(c, MockCall::EnsureLabels(_))),
            "local mode should not call ensure_labels; got: {calls:?}"
        );
    }

    #[test]
    fn auto_close_owned_issues_closes_open_issue_at_end_of_run() {
        let tracker = Arc::new(MockIssueTracker::new());
        // Default mock returns an open issue for get_issue.
        let config = LoopConfig {
            iterations: 1,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: Some("o".to_string()),
                repo_name: Some("r".to_string()),
                comment_issue_number: Some(42),
                comment_cadence: CommentCadence::OffEngine,
                auto_close_owned_issues: true,
                standard_labels: vec![],
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = LoopEngine::with_adapter_and_tracker(
            config,
            Box::new(FakeAdapter::success("fake")),
            Box::new(crate::issue_tracker::SharedMockIssueTracker(Arc::clone(&tracker))),
        );
        engine.run();
        let calls = tracker.recorded_calls();
        assert!(
            calls.iter().any(|c| matches!(c, MockCall::CloseIssue(42))),
            "auto_close_owned_issues should close issue 42; calls: {calls:?}"
        );
    }

    #[test]
    fn owned_issue_already_closed_does_not_close_again() {
        let tracker = Arc::new(MockIssueTracker::new());
        // Override next_issue to return a closed issue.
        *tracker.next_issue.lock().unwrap() = Some(crate::issue_tracker::Issue {
            id: 1,
            number: 42,
            title: "already done".to_string(),
            body: "".to_string(),
            state: crate::issue_tracker::IssueState::Closed,
            labels: vec![],
            assignees: vec![],
            url: "https://example.com/42".to_string(),
        });
        let config = LoopConfig {
            iterations: 1,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: Some("o".to_string()),
                repo_name: Some("r".to_string()),
                comment_issue_number: Some(42),
                comment_cadence: CommentCadence::OffEngine,
                auto_close_owned_issues: true,
                standard_labels: vec![],
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = LoopEngine::with_adapter_and_tracker(
            config,
            Box::new(FakeAdapter::success("fake")),
            Box::new(crate::issue_tracker::SharedMockIssueTracker(Arc::clone(&tracker))),
        );
        engine.run();
        let calls = tracker.recorded_calls();
        assert!(
            !calls.iter().any(|c| matches!(c, MockCall::CloseIssue(_))),
            "already-closed issue should not be closed again; calls: {calls:?}"
        );
    }

    #[test]
    fn stop_on_failure_posts_blocker_comment() {
        let tracker = Arc::new(MockIssueTracker::new());
        let config = LoopConfig {
            iterations: 5,
            provider: Provider::Claude,
            prompt_inline: Some("test".to_string()),
            stop_on_failure: true,
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: Some("o".to_string()),
                repo_name: Some("r".to_string()),
                comment_issue_number: Some(3),
                comment_cadence: CommentCadence::Milestones,
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = LoopEngine::with_adapter_and_tracker(
            config,
            Box::new(FakeAdapter::failure("fake")),
            Box::new(crate::issue_tracker::SharedMockIssueTracker(Arc::clone(&tracker))),
        );
        engine.run();
        let calls = tracker.recorded_calls();
        let comment_bodies: Vec<_> = calls
            .iter()
            .filter_map(|c| {
                if let MockCall::AddComment { body, .. } = c { Some(body.as_str()) } else { None }
            })
            .collect();
        assert!(
            comment_bodies.iter().any(|b| b.contains("Blocker")),
            "expected a blocker comment; got: {comment_bodies:?}"
        );
    }
}
