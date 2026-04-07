use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::warn;

// ── IterationOutcome ──────────────────────────────────────────────────────────

/// Classified outcome of a single iteration attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IterationOutcome {
    /// Provider exited with status 0.
    Success,
    /// Provider exited with a non-zero status code.
    NonZeroExit { exit_code: i32 },
    /// The provider binary could not be spawned.
    SpawnFailure { message: String },
    /// The policy guard blocked a disallowed operation.
    PolicyGuardBlock { message: String },
    /// Process was terminated by a signal (Unix only; exit_code is None).
    Signal,
    /// Provider invocation exceeded the configured timeout.
    Timeout,
    /// Exit status was not available (e.g. process exit status unknown).
    Unknown,
}

impl IterationOutcome {
    /// Returns `true` when the iteration is considered successful.
    pub fn is_success(&self) -> bool {
        matches!(self, IterationOutcome::Success)
    }

    /// Returns `true` when the loop should abort immediately (spawn failure).
    #[allow(dead_code)]
    pub fn is_fatal(&self) -> bool {
        matches!(self, IterationOutcome::SpawnFailure { .. })
    }

    /// Returns `true` when a retry attempt may recover from this failure.
    ///
    /// Non-zero exits are considered transient (e.g. API rate limit, flaky
    /// network) and are retried by default.  Signal terminations, unknown
    /// exit states, and policy guard blocks are treated as non-retryable
    /// because a retry is unlikely to change the outcome.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            IterationOutcome::NonZeroExit { .. } | IterationOutcome::Timeout
        )
    }

    /// Classify an exit code into an outcome.
    pub fn from_exit_code(code: Option<i32>) -> Self {
        match code {
            Some(0) => IterationOutcome::Success,
            Some(c) => IterationOutcome::NonZeroExit { exit_code: c },
            None => IterationOutcome::Signal,
        }
    }

    /// Short label for display / logging.
    pub fn label(&self) -> &'static str {
        match self {
            IterationOutcome::Success => "success",
            IterationOutcome::NonZeroExit { .. } => "non_zero_exit",
            IterationOutcome::SpawnFailure { .. } => "spawn_failure",
            IterationOutcome::PolicyGuardBlock { .. } => "policy_guard_block",
            IterationOutcome::Signal => "signal",
            IterationOutcome::Timeout => "timeout",
            IterationOutcome::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for IterationOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IterationOutcome::Success => write!(f, "success"),
            IterationOutcome::NonZeroExit { exit_code } => {
                write!(f, "non_zero_exit({})", exit_code)
            }
            IterationOutcome::SpawnFailure { message } => {
                write!(f, "spawn_failure: {}", message)
            }
            IterationOutcome::PolicyGuardBlock { message } => {
                write!(f, "policy_guard_block: {}", message)
            }
            IterationOutcome::Signal => write!(f, "signal"),
            IterationOutcome::Timeout => write!(f, "timeout"),
            IterationOutcome::Unknown => write!(f, "unknown"),
        }
    }
}

// ── IterationRecord ───────────────────────────────────────────────────────────

/// Durable record for a single completed iteration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationRecord {
    /// 1-based iteration number within this run.
    pub iteration: u64,
    /// Provider name (e.g. "claude", "copilot", "codex").
    pub provider: String,
    /// Prompt source: "inline", "file", or "none".
    pub prompt_source: String,
    /// Orchestration workflow branch selected (if orchestration is enabled).
    pub workflow_branch: Option<String>,
    /// Classified outcome.
    pub outcome: IterationOutcome,
    /// Wall-clock duration of the provider execution (excluding retries).
    pub duration_ms: u128,
    /// Total retry attempts consumed (0 = no retries).
    pub retries: u32,
    /// First line of stderr, if the iteration failed.
    pub stderr_excerpt: Option<String>,
    /// Path to the full transcript file, relative to the run directory.
    pub transcript_path: Option<String>,
    /// Unix timestamp (seconds) when this iteration started.
    pub started_at: u64,
}

impl IterationRecord {
    /// Extract the first non-empty line from a stderr string for the log excerpt.
    pub fn stderr_first_line(stderr: &str) -> Option<String> {
        stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
    }
}

// ── RunManifest ───────────────────────────────────────────────────────────────

/// Index written to `manifest.json` inside the run directory.
#[derive(Debug, Serialize, Deserialize)]
pub struct RunManifest {
    pub run_id: String,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub provider: String,
    pub iterations_requested: i64,
    pub termination_reason: Option<String>,
    /// Number of planned orchestration actions that were intentionally skipped
    /// (e.g. PR blocked on human review, no actionable PR found).
    #[serde(default)]
    pub skipped_decisions: u64,
    /// Operator identity: value of `$USER` (or `$USERNAME` on Windows) at run
    /// start.  `None` when neither environment variable is set.
    ///
    /// Satisfies the PRD audit-provenance requirement: "who/when/provider/policy".
    #[serde(default)]
    pub run_by: Option<String>,
    /// Filesystem path of the workspace directory in which the run executed.
    /// `None` when the workspace dir was not resolved or not stored.
    #[serde(default)]
    pub workspace_dir: Option<String>,
    pub iterations: Vec<IterationRecord>,
}

// ── RunArtifacts ──────────────────────────────────────────────────────────────

/// Manages the on-disk artifacts directory for a single run.
pub struct RunArtifacts {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub enabled: bool,
}

impl RunArtifacts {
    /// Create the run directory and return a `RunArtifacts` handle.
    /// When `enabled` is false, all write operations are no-ops.
    pub fn create(artifacts_root: &Path, enabled: bool) -> Self {
        let run_id = run_id_now();
        let run_dir = artifacts_root.join(&run_id);
        if enabled {
            if let Err(e) = std::fs::create_dir_all(&run_dir) {
                warn!(
                    "Could not create run artifacts dir {}: {e}",
                    run_dir.display()
                );
            }
        }
        Self {
            run_id,
            run_dir,
            enabled,
        }
    }

    /// Write a transcript for iteration `n` (1-based).  Returns the path
    /// written (relative to the run dir), or `None` when disabled or on error.
    pub fn write_transcript(&self, iteration: u64, stdout: &str, stderr: &str) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let filename = format!("iteration-{iteration}.log");
        let path = self.run_dir.join(&filename);
        let content = format!("=== STDOUT ===\n{stdout}\n=== STDERR ===\n{stderr}\n");
        if let Err(e) = std::fs::write(&path, content) {
            warn!("Could not write transcript {}: {e}", path.display());
            return None;
        }
        Some(filename)
    }

    /// Write `manifest.json` for the run.
    pub fn write_manifest(&self, manifest: &RunManifest) {
        if !self.enabled {
            return;
        }
        let path = self.run_dir.join("manifest.json");
        match serde_json::to_string_pretty(manifest) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    warn!("Could not write manifest {}: {e}", path.display());
                }
            }
            Err(e) => warn!("Could not serialize manifest: {e}"),
        }
    }

    /// Write the human-readable `summary.md` and return its path.
    pub fn write_summary(&self, manifest: &RunManifest, no_summary: bool) -> Option<PathBuf> {
        let summary = build_summary_markdown(manifest);
        if !no_summary {
            print_terminal_summary(&summary);
        }
        if !self.enabled {
            return None;
        }
        let path = self.run_dir.join("summary.md");
        if let Err(e) = std::fs::write(&path, &summary) {
            warn!("Could not write summary {}: {e}", path.display());
            return None;
        }
        Some(path)
    }

    /// Prune old run directories under `artifacts_root`, keeping only the
    /// `keep_runs` most recent (by directory name, which is timestamp-ordered).
    pub fn prune_old_runs(artifacts_root: &Path, keep_runs: usize) {
        let entries = match std::fs::read_dir(artifacts_root) {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        dirs.sort();
        if dirs.len() > keep_runs {
            for old in dirs.iter().take(dirs.len() - keep_runs) {
                if let Err(e) = std::fs::remove_dir_all(old) {
                    warn!("Could not prune old run dir {}: {e}", old.display());
                }
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Generate a run ID from the current timestamp (sortable, filesystem-safe).
fn run_id_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    format!("{secs:020}")
}

/// Resolve the operator identity from the environment.
///
/// Checks `$USER` first (Unix convention), then `$USERNAME` (Windows
/// convention).  Returns `None` when neither variable is set.
pub fn resolve_operator() -> Option<String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|s| !s.is_empty())
}

/// Current Unix timestamp in seconds.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

/// Build the full markdown summary string.
fn build_summary_markdown(manifest: &RunManifest) -> String {
    let started = manifest.started_at;
    let ended = manifest.ended_at.unwrap_or(started);
    let total_secs = ended.saturating_sub(started);
    let iterations_executed = manifest.iterations.len();
    let successes = manifest
        .iterations
        .iter()
        .filter(|i| i.outcome.is_success())
        .count();
    let failures = iterations_executed - successes;
    let total_retries: u32 = manifest.iterations.iter().map(|i| i.retries).sum();
    let term_reason = manifest.termination_reason.as_deref().unwrap_or("unknown");

    let mut md = String::new();
    md.push_str("# Code Looper — Run Summary\n\n");

    // Metadata table
    md.push_str("## Run Metadata\n\n");
    md.push_str("| Field | Value |\n");
    md.push_str("|---|---|\n");
    md.push_str(&format!("| Run ID | `{}` |\n", manifest.run_id));
    md.push_str(&format!("| Provider | {} |\n", manifest.provider));
    if let Some(ref who) = manifest.run_by {
        md.push_str(&format!("| Run by | {} |\n", who));
    }
    if let Some(ref ws) = manifest.workspace_dir {
        md.push_str(&format!("| Workspace | {} |\n", ws));
    }
    md.push_str(&format!("| Started | {} |\n", started));
    md.push_str(&format!("| Ended | {} |\n", ended));
    md.push_str(&format!("| Duration | {}s |\n", total_secs));
    md.push_str(&format!(
        "| Iterations requested | {} |\n",
        manifest.iterations_requested
    ));
    md.push_str(&format!(
        "| Iterations executed | {} |\n",
        iterations_executed
    ));
    md.push_str(&format!("| Termination reason | {} |\n\n", term_reason));

    // Totals
    md.push_str("## Totals\n\n");
    md.push_str("| Metric | Count |\n");
    md.push_str("|---|---|\n");
    md.push_str(&format!("| Successes | {} |\n", successes));
    md.push_str(&format!("| Failures | {} |\n", failures));
    md.push_str(&format!("| Retries | {} |\n", total_retries));
    md.push_str(&format!(
        "| Skipped decisions | {} |\n\n",
        manifest.skipped_decisions
    ));

    // Per-iteration table
    if !manifest.iterations.is_empty() {
        md.push_str("## Iterations\n\n");
        md.push_str("| # | Branch | Status | Duration (ms) | Retries | Stderr excerpt |\n");
        md.push_str("|---|---|---|---|---|---|\n");
        for rec in &manifest.iterations {
            let branch = rec.workflow_branch.as_deref().unwrap_or("-");
            let excerpt = rec.stderr_excerpt.as_deref().unwrap_or("-");
            md.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                rec.iteration,
                branch,
                rec.outcome.label(),
                rec.duration_ms,
                rec.retries,
                excerpt,
            ));
        }
        md.push('\n');
    }

    // Transcript links
    let with_transcripts: Vec<_> = manifest
        .iterations
        .iter()
        .filter(|r| r.transcript_path.is_some())
        .collect();
    if !with_transcripts.is_empty() {
        md.push_str("## Transcripts\n\n");
        for rec in with_transcripts {
            md.push_str(&format!(
                "- Iteration {}: [{}]({})\n",
                rec.iteration,
                rec.transcript_path.as_deref().unwrap_or(""),
                rec.transcript_path.as_deref().unwrap_or(""),
            ));
        }
        md.push('\n');
    }

    md
}

/// Print a condensed run summary to stdout.
fn print_terminal_summary(markdown: &str) {
    // Extract the totals section for the condensed terminal view.
    println!();
    println!("─── Run Summary ─────────────────────────────────");
    for line in markdown.lines() {
        if line.starts_with("| Run ID")
            || line.starts_with("| Provider")
            || line.starts_with("| Duration")
            || line.starts_with("| Iterations executed")
            || line.starts_with("| Successes")
            || line.starts_with("| Failures")
            || line.starts_with("| Retries")
            || line.starts_with("| Termination")
        {
            // Format: `| Key | Value |`
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                let key = parts[1].trim();
                let val = parts[2].trim();
                println!("  {:<26}: {}", key, val);
            }
        }
    }
    println!("─────────────────────────────────────────────────");
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_from_exit_code_zero_is_success() {
        assert_eq!(
            IterationOutcome::from_exit_code(Some(0)),
            IterationOutcome::Success
        );
        assert!(IterationOutcome::from_exit_code(Some(0)).is_success());
    }

    #[test]
    fn outcome_from_exit_code_nonzero() {
        let o = IterationOutcome::from_exit_code(Some(1));
        assert_eq!(o, IterationOutcome::NonZeroExit { exit_code: 1 });
        assert!(!o.is_success());
    }

    #[test]
    fn outcome_from_exit_code_none_is_signal() {
        assert_eq!(
            IterationOutcome::from_exit_code(None),
            IterationOutcome::Signal
        );
    }

    #[test]
    fn spawn_failure_is_fatal() {
        let o = IterationOutcome::SpawnFailure {
            message: "not found".to_string(),
        };
        assert!(o.is_fatal());
        assert!(!o.is_success());
    }

    #[test]
    fn retryable_outcomes() {
        assert!(IterationOutcome::NonZeroExit { exit_code: 1 }.is_retryable());
        assert!(IterationOutcome::Timeout.is_retryable());
    }

    #[test]
    fn non_retryable_outcomes() {
        assert!(!IterationOutcome::Success.is_retryable());
        assert!(!IterationOutcome::Signal.is_retryable());
        assert!(!IterationOutcome::Unknown.is_retryable());
        assert!(!IterationOutcome::SpawnFailure {
            message: String::new()
        }
        .is_retryable());
        assert!(!IterationOutcome::PolicyGuardBlock {
            message: String::new()
        }
        .is_retryable());
    }

    #[test]
    fn outcome_labels_are_stable() {
        assert_eq!(IterationOutcome::Success.label(), "success");
        assert_eq!(
            IterationOutcome::NonZeroExit { exit_code: 2 }.label(),
            "non_zero_exit"
        );
        assert_eq!(
            IterationOutcome::SpawnFailure {
                message: String::new()
            }
            .label(),
            "spawn_failure"
        );
        assert_eq!(
            IterationOutcome::PolicyGuardBlock {
                message: String::new()
            }
            .label(),
            "policy_guard_block"
        );
        assert_eq!(IterationOutcome::Signal.label(), "signal");
        assert_eq!(IterationOutcome::Timeout.label(), "timeout");
        assert_eq!(IterationOutcome::Unknown.label(), "unknown");
    }

    #[test]
    fn stderr_first_line_skips_empty_lines() {
        assert_eq!(
            IterationRecord::stderr_first_line("\n\nerror: thing failed\nmore"),
            Some("error: thing failed".to_string())
        );
        assert_eq!(IterationRecord::stderr_first_line(""), None);
    }

    #[test]
    fn run_artifacts_disabled_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let art = RunArtifacts::create(dir.path(), false);
        assert!(art.write_transcript(1, "out", "err").is_none());
        // No directories should have been created under the root.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn run_artifacts_enabled_writes_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let art = RunArtifacts::create(dir.path(), true);
        let name = art.write_transcript(1, "hello", "world").unwrap();
        assert_eq!(name, "iteration-1.log");
        let content = std::fs::read_to_string(art.run_dir.join(&name)).unwrap();
        assert!(content.contains("hello"));
        assert!(content.contains("world"));
    }

    #[test]
    fn run_artifacts_writes_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let art = RunArtifacts::create(dir.path(), true);
        let manifest = RunManifest {
            run_id: art.run_id.clone(),
            started_at: 0,
            ended_at: Some(10),
            provider: "claude".to_string(),
            iterations_requested: 2,
            termination_reason: Some("completed".to_string()),
            skipped_decisions: 0,
            run_by: None,
            workspace_dir: None,
            iterations: vec![],
        };
        art.write_manifest(&manifest);
        let json_path = art.run_dir.join("manifest.json");
        assert!(json_path.exists());
        let raw = std::fs::read_to_string(json_path).unwrap();
        assert!(raw.contains("completed"));
    }

    #[test]
    fn prune_keeps_n_most_recent() {
        let dir = tempfile::tempdir().unwrap();
        // Create 5 fake run dirs with sortable names.
        for i in 0..5u32 {
            std::fs::create_dir(dir.path().join(format!("{:020}", i))).unwrap();
        }
        RunArtifacts::prune_old_runs(dir.path(), 3);
        let remaining = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .count();
        assert_eq!(remaining, 3);
    }

    #[test]
    fn summary_markdown_contains_key_fields() {
        let manifest = RunManifest {
            run_id: "test-run".to_string(),
            started_at: 1000,
            ended_at: Some(1060),
            provider: "claude".to_string(),
            iterations_requested: 2,
            termination_reason: Some("completed".to_string()),
            skipped_decisions: 2,
            run_by: None,
            workspace_dir: None,
            iterations: vec![IterationRecord {
                iteration: 1,
                provider: "claude".to_string(),
                prompt_source: "inline".to_string(),
                workflow_branch: None,
                outcome: IterationOutcome::Success,
                duration_ms: 500,
                retries: 0,
                stderr_excerpt: None,
                transcript_path: Some("iteration-1.log".to_string()),
                started_at: 1000,
            }],
        };
        let md = build_summary_markdown(&manifest);
        assert!(md.contains("test-run"));
        assert!(md.contains("claude"));
        assert!(md.contains("completed"));
        assert!(md.contains("success"));
        assert!(md.contains("iteration-1.log"));
        assert!(md.contains("Skipped decisions"));
    }

    #[test]
    fn summary_includes_run_by_when_set() {
        let manifest = RunManifest {
            run_id: "r1".to_string(),
            started_at: 0,
            ended_at: None,
            provider: "claude".to_string(),
            iterations_requested: 1,
            termination_reason: None,
            skipped_decisions: 0,
            run_by: Some("alice".to_string()),
            workspace_dir: None,
            iterations: vec![],
        };
        let md = build_summary_markdown(&manifest);
        assert!(md.contains("alice"));
        assert!(md.contains("Run by"));
    }

    #[test]
    fn summary_includes_workspace_dir_when_set() {
        let manifest = RunManifest {
            run_id: "r2".to_string(),
            started_at: 0,
            ended_at: None,
            provider: "claude".to_string(),
            iterations_requested: 1,
            termination_reason: None,
            skipped_decisions: 0,
            run_by: None,
            workspace_dir: Some("/home/alice/my-repo".to_string()),
            iterations: vec![],
        };
        let md = build_summary_markdown(&manifest);
        assert!(md.contains("/home/alice/my-repo"));
        assert!(md.contains("Workspace"));
    }

    #[test]
    fn summary_omits_run_by_when_none() {
        let manifest = RunManifest {
            run_id: "r3".to_string(),
            started_at: 0,
            ended_at: None,
            provider: "claude".to_string(),
            iterations_requested: 1,
            termination_reason: None,
            skipped_decisions: 0,
            run_by: None,
            workspace_dir: None,
            iterations: vec![],
        };
        let md = build_summary_markdown(&manifest);
        assert!(!md.contains("Run by"));
        assert!(!md.contains("Workspace"));
    }

    #[test]
    fn manifest_run_by_round_trips_through_json() {
        let manifest = RunManifest {
            run_id: "r4".to_string(),
            started_at: 42,
            ended_at: None,
            provider: "codex".to_string(),
            iterations_requested: 3,
            termination_reason: None,
            skipped_decisions: 0,
            run_by: Some("bob".to_string()),
            workspace_dir: Some("/tmp/proj".to_string()),
            iterations: vec![],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let restored: RunManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.run_by.as_deref(), Some("bob"));
        assert_eq!(restored.workspace_dir.as_deref(), Some("/tmp/proj"));
    }

    #[test]
    fn manifest_missing_run_by_deserializes_to_none() {
        // Simulates reading an old manifest.json that pre-dates the run_by field.
        let json = r#"{
            "run_id": "old",
            "started_at": 0,
            "ended_at": null,
            "provider": "claude",
            "iterations_requested": 1,
            "termination_reason": null,
            "skipped_decisions": 0,
            "iterations": []
        }"#;
        let manifest: RunManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.run_by.is_none());
        assert!(manifest.workspace_dir.is_none());
    }
}
