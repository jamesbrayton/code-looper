use crate::config::{CommentCadence, IssueTrackingMode, LoopConfig, PrMode, Provider};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Subcommands available in addition to the default loop run.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Initialise or patch workspace prerequisites so a repository is ready
    /// to run code-looper.
    ///
    /// Checks the same prerequisites as the startup validator and, for each
    /// gap, produces a minimal, idempotent fix:
    ///
    /// - No instruction file → creates `CLAUDE.md` with a Code Looper section.
    /// - Instruction file lacks a Code Looper section → appends a delimited block.
    /// - `.mcp.json` missing → creates a minimal stub with the GitHub server entry.
    /// - `.mcp.json` lacks a `"github"` key → merges the entry in.
    Bootstrap {
        /// Directory to treat as the workspace root.  Defaults to the current
        /// working directory.
        #[arg(long)]
        workspace_dir: Option<PathBuf>,

        /// Print what would be created or modified without writing anything.
        #[arg(long)]
        dry_run: bool,
    },

    /// Start Code Looper in service mode, listening for JSON-lines requests
    /// over a local TCP socket.
    ///
    /// Clients connect and send one request per line (newline-delimited JSON).
    /// The server responds with one JSON object per line.
    ///
    /// Supported commands:
    ///
    ///   {"cmd":"run","prompt":"<text>"}           — execute one provider iteration
    ///   {"cmd":"run","prompt":"<text>","provider":"codex"} — override provider
    ///   {"cmd":"status"}                          — service uptime and run counts
    ///   {"cmd":"shutdown"}                        — stop the service
    ///
    /// Example (using netcat):
    ///   echo '{"cmd":"status"}' | nc 127.0.0.1 7979
    Serve {
        /// TCP port to listen on.  Defaults to 7979.
        #[arg(long, default_value_t = 7979)]
        port: u16,

        /// Address to bind to.  Defaults to 127.0.0.1 (loopback only).
        #[arg(long, default_value = "127.0.0.1")]
        bind_addr: String,
    },
}

/// Pluggable loop engine for coding-agent CLIs.
#[derive(Debug, Parser)]
#[command(name = "code-looper", version, about)]
pub struct Cli {
    /// Subcommand (optional; omit to run the default loop).
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Provider to use for loop execution.
    #[arg(long)]
    pub provider: Option<Provider>,

    /// Number of iterations (-1 for infinite looping).
    #[arg(long)]
    pub iterations: Option<i64>,

    /// Inline prompt string (mutually exclusive with --prompt-file).
    #[arg(long, conflicts_with = "prompt_file")]
    pub prompt_inline: Option<String>,

    /// Path to a markdown prompt file (mutually exclusive with --prompt-inline).
    #[arg(long, conflicts_with = "prompt_inline")]
    pub prompt_file: Option<PathBuf>,

    /// Path to a config file to load as a base configuration.
    /// Supports TOML (`.toml`, default) and YAML (`.yaml` / `.yml`).
    /// Format is detected automatically from the file extension.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Log level: trace, debug, info, warn, or error.
    #[arg(long)]
    pub log_level: Option<String>,

    /// Enable orchestration policy engine (auto-selects workflow branch from repo context).
    #[arg(long)]
    pub orchestration: bool,

    /// GitHub repository owner (required when --orchestration is set).
    #[arg(long)]
    pub repo_owner: Option<String>,

    /// GitHub repository name (required when --orchestration is set).
    #[arg(long)]
    pub repo_name: Option<String>,

    /// Directory to treat as the workspace root for prerequisite checks.
    /// Defaults to the current working directory.
    #[arg(long)]
    pub workspace_dir: Option<PathBuf>,

    /// Skip workspace prerequisite checks (instruction file, MCP config).
    /// Use only when you know the workspace is not a git repository or the
    /// checks are not applicable.
    #[arg(long)]
    pub skip_prereq_check: bool,

    /// [UNSAFE] Allow GitHub context resolution via direct `gh` CLI calls
    /// instead of requiring a GitHub MCP server.  Disables the MCP-only
    /// write-path enforcement preamble in provider prompts.
    #[arg(long)]
    pub allow_direct_github: bool,

    /// Stop the loop after the first iteration that fails (after all retries
    /// are exhausted).
    #[arg(long)]
    pub stop_on_failure: bool,

    /// Number of additional retry attempts per iteration on non-zero exit.
    #[arg(long)]
    pub max_retries: Option<u32>,

    /// Milliseconds to wait between retry attempts (default: 500).
    #[arg(long)]
    pub retry_backoff_ms: Option<u64>,

    /// Exponential backoff multiplier applied per retry attempt.
    /// Delay for attempt N = retry_backoff_ms × multiplier^(N-1).
    /// `1.0` (default) gives flat backoff; `2.0` doubles the delay each retry.
    #[arg(long)]
    pub retry_backoff_multiplier: Option<f64>,

    /// Shell command to run once after the loop finishes (run via `sh -c`).
    #[arg(long)]
    pub on_complete: Option<String>,

    /// Maximum seconds a single provider invocation may run before being killed.
    /// When the timeout fires the iteration is recorded as `timeout` and may be
    /// retried if `--max-retries` is set.  Omit or set to 0 to disable.
    #[arg(long)]
    pub iteration_timeout_secs: Option<u64>,

    /// Issue tracking mode: `github` (production) or `local` (dev/debug).
    #[arg(long)]
    pub issue_tracking_mode: Option<IssueTrackingMode>,

    /// GitHub repository owner for issue tracking (when --issue-tracking-mode=github).
    /// Falls back to --repo-owner if not set.
    #[arg(long)]
    pub issue_tracking_owner: Option<String>,

    /// GitHub repository name for issue tracking (when --issue-tracking-mode=github).
    /// Falls back to --repo-name if not set.
    #[arg(long)]
    pub issue_tracking_repo: Option<String>,

    /// Path to the local promise markdown file (when --issue-tracking-mode=local).
    #[arg(long)]
    pub local_promise_path: Option<PathBuf>,

    /// GitHub issue number the engine should post run-lifecycle comments on.
    /// Requires --issue-tracking-mode=github.
    #[arg(long)]
    pub comment_issue: Option<u32>,

    /// How often the engine posts comments to the linked issue.
    /// One of: `milestones` (default), `every-iteration`, `off-engine`.
    #[arg(long)]
    pub comment_cadence: Option<CommentCadence>,

    /// Close the linked issue at end-of-run if the agent left it open after
    /// completing all checklist items.  When not set (default), the engine
    /// only logs a warning.  Requires --issue-tracking-mode=github and
    /// --comment-issue.
    #[arg(long)]
    pub auto_close_owned_issues: bool,

    /// Stream provider stdout/stderr to the terminal in real time (default: on).
    /// Use --no-stream-output to disable.
    #[arg(long, default_missing_value = "true", num_args = 0..=1)]
    pub stream_output: Option<bool>,

    /// Root directory for per-run artifact directories (transcripts, manifest,
    /// summary).  Defaults to `.code-looper/runs`.
    #[arg(long)]
    pub artifacts_dir: Option<PathBuf>,

    /// Number of most-recent run directories to retain.  Older runs are pruned
    /// after each new run.  Default: 10.
    #[arg(long)]
    pub keep_runs: Option<usize>,

    /// Suppress writing the markdown summary and the condensed terminal summary
    /// at the end of each run.  Useful for scripted / CI use.
    #[arg(long)]
    pub no_summary: bool,

    /// PR iteration mode: `no-pr` (default), `single-pr`, or `multi-pr`.
    #[arg(long)]
    pub pr_mode: Option<PrMode>,

    /// Branch to open PRs into (default: `main`).
    #[arg(long)]
    pub base_branch: Option<String>,

    /// Prefix for feature branches created by the loop (default: `loop/`).
    #[arg(long)]
    pub branch_prefix: Option<String>,

    /// Require human review before the loop merges a PR (default: on).
    /// Pass `--no-require-human-review` to allow automated merges.
    #[arg(long, default_missing_value = "true", num_args = 0..=1)]
    pub require_human_review: Option<bool>,

    /// Extra argument to append to the provider CLI invocation.
    /// Can be repeated to supply multiple arguments.
    /// Arguments are inserted after the adapter's hardcoded flags but before
    /// the prompt, so they are treated as option flags by the provider CLI.
    ///
    /// Example: `--provider-extra-arg --model --provider-extra-arg claude-opus-4-5`
    #[arg(long, action = clap::ArgAction::Append)]
    pub provider_extra_arg: Vec<String>,

    /// Exit code that should be treated as a permanent failure and never
    /// retried, even when `--max-retries` is set.  Can be repeated.
    ///
    /// Use this to short-circuit retries for codes that indicate a
    /// configuration or argument error rather than a transient fault.
    /// Example: `--non-retryable-exit-code 2 --non-retryable-exit-code 127`
    #[arg(long, action = clap::ArgAction::Append)]
    pub non_retryable_exit_code: Vec<i32>,
}

impl Cli {
    /// Merge CLI overrides onto a base `LoopConfig`.
    ///
    /// The base is either `LoopConfig::default()` or values loaded from a
    /// TOML / YAML config file; any CLI flag that is explicitly set takes precedence.
    pub fn apply_overrides(self, mut base: LoopConfig) -> LoopConfig {
        if let Some(p) = self.provider {
            base.provider = p;
        }
        if let Some(i) = self.iterations {
            base.iterations = i;
        }
        if let Some(s) = self.prompt_inline {
            base.prompt_inline = Some(s);
            base.prompt_file = None;
        }
        if let Some(f) = self.prompt_file {
            base.prompt_file = Some(f);
            base.prompt_inline = None;
        }
        if let Some(l) = self.log_level {
            base.log_level = l;
        }
        if self.orchestration {
            base.orchestration.enabled = true;
        }
        if let Some(owner) = self.repo_owner {
            base.orchestration.repo_owner = Some(owner);
        }
        if let Some(name) = self.repo_name {
            base.orchestration.repo_name = Some(name);
        }
        if let Some(dir) = self.workspace_dir {
            base.workspace_dir = Some(dir);
        }
        if self.skip_prereq_check {
            base.skip_prereq_check = true;
        }
        if self.allow_direct_github {
            base.allow_direct_github = true;
        }
        if self.stop_on_failure {
            base.stop_on_failure = true;
        }
        if let Some(n) = self.max_retries {
            base.max_retries = n;
        }
        if let Some(ms) = self.retry_backoff_ms {
            base.retry_backoff_ms = ms;
        }
        if let Some(m) = self.retry_backoff_multiplier {
            base.retry_backoff_multiplier = m;
        }
        if let Some(cmd) = self.on_complete {
            base.on_complete = Some(cmd);
        }
        if let Some(secs) = self.iteration_timeout_secs {
            if secs > 0 {
                base.iteration_timeout_secs = Some(secs);
            }
        }
        if let Some(mode) = self.issue_tracking_mode {
            base.issue_tracking.mode = mode;
        }
        if let Some(owner) = self.issue_tracking_owner {
            base.issue_tracking.repo_owner = Some(owner);
        }
        if let Some(repo) = self.issue_tracking_repo {
            base.issue_tracking.repo_name = Some(repo);
        }
        if let Some(path) = self.local_promise_path {
            base.issue_tracking.local_promise_path = Some(path);
        }
        if let Some(n) = self.comment_issue {
            base.issue_tracking.comment_issue_number = Some(n);
        }
        if let Some(cadence) = self.comment_cadence {
            base.issue_tracking.comment_cadence = cadence;
        }
        if self.auto_close_owned_issues {
            base.issue_tracking.auto_close_owned_issues = true;
        }
        if let Some(s) = self.stream_output {
            base.telemetry.stream_output = s;
        }
        if let Some(dir) = self.artifacts_dir {
            base.telemetry.artifacts_dir = dir;
        }
        if let Some(n) = self.keep_runs {
            base.telemetry.keep_runs = n;
        }
        if self.no_summary {
            base.telemetry.no_summary = true;
        }
        if let Some(mode) = self.pr_mode {
            base.pr_management.mode = mode;
        }
        if let Some(branch) = self.base_branch {
            base.pr_management.base_branch = branch;
        }
        if let Some(prefix) = self.branch_prefix {
            base.pr_management.branch_prefix = prefix;
        }
        if let Some(review) = self.require_human_review {
            base.pr_management.require_human_review = review;
        }
        if !self.provider_extra_arg.is_empty() {
            base.provider_extra_args = self.provider_extra_arg;
        }
        if !self.non_retryable_exit_code.is_empty() {
            base.non_retryable_exit_codes = self.non_retryable_exit_code;
        }
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;

    fn default_config() -> LoopConfig {
        LoopConfig::default()
    }

    fn blank_cli() -> Cli {
        Cli {
            command: None,
            provider: None,
            iterations: None,
            prompt_inline: None,
            prompt_file: None,
            config: None,
            log_level: None,
            orchestration: false,
            repo_owner: None,
            repo_name: None,
            workspace_dir: None,
            skip_prereq_check: false,
            allow_direct_github: false,
            stop_on_failure: false,
            max_retries: None,
            retry_backoff_ms: None,
            retry_backoff_multiplier: None,
            on_complete: None,
            iteration_timeout_secs: None,
            issue_tracking_mode: None,
            issue_tracking_owner: None,
            issue_tracking_repo: None,
            local_promise_path: None,
            comment_issue: None,
            comment_cadence: None,
            auto_close_owned_issues: false,
            stream_output: None,
            artifacts_dir: None,
            keep_runs: None,
            no_summary: false,
            pr_mode: None,
            base_branch: None,
            branch_prefix: None,
            require_human_review: None,
            provider_extra_arg: vec![],
            non_retryable_exit_code: vec![],
        }
    }

    #[test]
    fn cli_overrides_provider() {
        let cli = Cli {
            provider: Some(Provider::Copilot),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.provider, Provider::Copilot);
    }

    #[test]
    fn cli_overrides_iterations() {
        let cli = Cli {
            iterations: Some(10),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.iterations, 10);
    }

    #[test]
    fn cli_inline_prompt_clears_file_prompt() {
        let mut base = default_config();
        base.prompt_file = Some("old.md".into());

        let cli = Cli {
            prompt_inline: Some("new prompt".to_string()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(base);
        assert_eq!(config.prompt_inline.as_deref(), Some("new prompt"));
        assert!(config.prompt_file.is_none());
    }

    #[test]
    fn cli_no_overrides_preserves_base() {
        let mut base = default_config();
        base.iterations = 7;
        base.log_level = "debug".to_string();

        let config = blank_cli().apply_overrides(base);
        assert_eq!(config.iterations, 7);
        assert_eq!(config.log_level, "debug");
    }

    #[test]
    fn cli_orchestration_flag_enables_engine() {
        let cli = Cli {
            orchestration: true,
            repo_owner: Some("acme".to_string()),
            repo_name: Some("project".to_string()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert!(config.orchestration.enabled);
        assert_eq!(config.orchestration.repo_owner.as_deref(), Some("acme"));
        assert_eq!(config.orchestration.repo_name.as_deref(), Some("project"));
    }

    #[test]
    fn cli_orchestration_false_leaves_config_disabled() {
        let config = blank_cli().apply_overrides(default_config());
        assert!(!config.orchestration.enabled);
    }

    #[test]
    fn cli_skip_prereq_check_sets_flag() {
        let cli = Cli {
            skip_prereq_check: true,
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert!(config.skip_prereq_check);
    }

    #[test]
    fn cli_allow_direct_github_sets_flag() {
        let cli = Cli {
            allow_direct_github: true,
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert!(config.allow_direct_github);
    }

    #[test]
    fn cli_workspace_dir_propagates() {
        let cli = Cli {
            workspace_dir: Some("/tmp/my-repo".into()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.workspace_dir, Some("/tmp/my-repo".into()));
    }

    #[test]
    fn cli_defaults_leave_safe_flags_false() {
        let config = blank_cli().apply_overrides(default_config());
        assert!(!config.skip_prereq_check);
        assert!(!config.allow_direct_github);
        assert!(config.workspace_dir.is_none());
    }

    #[test]
    fn cli_stop_on_failure_sets_flag() {
        let cli = Cli {
            stop_on_failure: true,
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert!(config.stop_on_failure);
    }

    #[test]
    fn cli_max_retries_propagates() {
        let cli = Cli {
            max_retries: Some(3),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn cli_retry_backoff_ms_propagates() {
        let cli = Cli {
            retry_backoff_ms: Some(1000),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.retry_backoff_ms, 1000);
    }

    #[test]
    fn cli_on_complete_propagates() {
        let cli = Cli {
            on_complete: Some("echo done".to_string()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.on_complete.as_deref(), Some("echo done"));
    }

    #[test]
    fn cli_defaults_leave_retry_fields_at_defaults() {
        let config = blank_cli().apply_overrides(default_config());
        assert!(!config.stop_on_failure);
        assert_eq!(config.max_retries, 0);
        assert_eq!(config.retry_backoff_ms, 500);
        assert!(config.on_complete.is_none());
        assert!(config.iteration_timeout_secs.is_none());
    }

    #[test]
    fn cli_iteration_timeout_secs_propagates() {
        let cli = Cli {
            iteration_timeout_secs: Some(30),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.iteration_timeout_secs, Some(30));
    }

    #[test]
    fn cli_iteration_timeout_secs_zero_is_ignored() {
        // 0 means "no timeout" — treated the same as omitting the flag.
        let cli = Cli {
            iteration_timeout_secs: Some(0),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert!(config.iteration_timeout_secs.is_none());
    }

    #[test]
    fn cli_issue_tracking_mode_propagates() {
        let cli = Cli {
            issue_tracking_mode: Some(IssueTrackingMode::Github),
            issue_tracking_owner: Some("acme".to_string()),
            issue_tracking_repo: Some("proj".to_string()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.issue_tracking.mode, IssueTrackingMode::Github);
        assert_eq!(config.issue_tracking.repo_owner.as_deref(), Some("acme"));
        assert_eq!(config.issue_tracking.repo_name.as_deref(), Some("proj"));
    }

    #[test]
    fn cli_local_promise_path_propagates() {
        let cli = Cli {
            local_promise_path: Some("/tmp/promise.md".into()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(
            config.issue_tracking.local_promise_path,
            Some(std::path::PathBuf::from("/tmp/promise.md"))
        );
    }

    #[test]
    fn cli_defaults_leave_issue_tracking_at_local() {
        let config = blank_cli().apply_overrides(default_config());
        assert_eq!(config.issue_tracking.mode, IssueTrackingMode::Local);
        assert!(config.issue_tracking.repo_owner.is_none());
        assert!(config.issue_tracking.repo_name.is_none());
    }

    #[test]
    fn cli_stream_output_false_propagates() {
        let cli = Cli {
            stream_output: Some(false),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert!(!config.telemetry.stream_output);
    }

    #[test]
    fn cli_artifacts_dir_propagates() {
        let cli = Cli {
            artifacts_dir: Some("/tmp/runs".into()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(
            config.telemetry.artifacts_dir,
            std::path::PathBuf::from("/tmp/runs")
        );
    }

    #[test]
    fn cli_keep_runs_propagates() {
        let cli = Cli {
            keep_runs: Some(5),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.telemetry.keep_runs, 5);
    }

    #[test]
    fn cli_no_summary_propagates() {
        let cli = Cli {
            no_summary: true,
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert!(config.telemetry.no_summary);
    }

    #[test]
    fn cli_telemetry_defaults() {
        let config = blank_cli().apply_overrides(default_config());
        assert!(config.telemetry.stream_output);
        assert_eq!(config.telemetry.keep_runs, 10);
        assert!(!config.telemetry.no_summary);
    }

    #[test]
    fn cli_pr_mode_propagates() {
        use crate::config::PrMode;
        let cli = Cli {
            pr_mode: Some(PrMode::SinglePr),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.pr_management.mode, PrMode::SinglePr);
    }

    #[test]
    fn cli_base_branch_propagates() {
        let cli = Cli {
            base_branch: Some("develop".to_string()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.pr_management.base_branch, "develop");
    }

    #[test]
    fn cli_branch_prefix_propagates() {
        let cli = Cli {
            branch_prefix: Some("feat/".to_string()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.pr_management.branch_prefix, "feat/");
    }

    #[test]
    fn cli_require_human_review_false_propagates() {
        let cli = Cli {
            require_human_review: Some(false),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert!(!config.pr_management.require_human_review);
    }

    #[test]
    fn cli_pr_management_defaults() {
        use crate::config::PrMode;
        let config = blank_cli().apply_overrides(default_config());
        assert_eq!(config.pr_management.mode, PrMode::NoPr);
        assert_eq!(config.pr_management.base_branch, "main");
        assert_eq!(config.pr_management.branch_prefix, "loop/");
        assert!(config.pr_management.require_human_review);
    }

    #[test]
    fn cli_retry_backoff_multiplier_propagates() {
        let cli = Cli {
            retry_backoff_multiplier: Some(2.0),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.retry_backoff_multiplier, 2.0);
    }

    #[test]
    fn cli_retry_backoff_multiplier_default_is_flat() {
        // When not set via CLI, the TOML/config default (1.0 = flat) is preserved.
        let config = blank_cli().apply_overrides(default_config());
        assert_eq!(config.retry_backoff_multiplier, 1.0);
    }

    #[test]
    fn cli_auto_close_owned_issues_sets_flag() {
        let cli = Cli {
            auto_close_owned_issues: true,
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert!(config.issue_tracking.auto_close_owned_issues);
    }

    #[test]
    fn cli_auto_close_owned_issues_default_false() {
        let config = blank_cli().apply_overrides(default_config());
        assert!(!config.issue_tracking.auto_close_owned_issues);
    }

    // ── provider_extra_arg ────────────────────────────────────────────────────

    #[test]
    fn cli_provider_extra_arg_propagates() {
        let cli = Cli {
            provider_extra_arg: vec!["--model".to_string(), "claude-opus-4-5".to_string()],
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.provider_extra_args, ["--model", "claude-opus-4-5"]);
    }

    #[test]
    fn cli_provider_extra_arg_empty_does_not_override_base() {
        let mut base = default_config();
        base.provider_extra_args = vec!["--from-config".to_string()];
        let config = blank_cli().apply_overrides(base);
        // Empty CLI vec should NOT overwrite config-file-provided args.
        assert_eq!(config.provider_extra_args, ["--from-config"]);
    }

    #[test]
    fn cli_provider_extra_arg_default_empty() {
        let config = blank_cli().apply_overrides(default_config());
        assert!(config.provider_extra_args.is_empty());
    }

    // ── non_retryable_exit_code ───────────────────────────────────────────────

    #[test]
    fn cli_non_retryable_exit_code_propagates() {
        let cli = Cli {
            non_retryable_exit_code: vec![2, 127],
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.non_retryable_exit_codes, [2, 127]);
    }

    #[test]
    fn cli_non_retryable_exit_code_default_empty() {
        let config = blank_cli().apply_overrides(default_config());
        assert!(config.non_retryable_exit_codes.is_empty());
    }

    #[test]
    fn cli_non_retryable_exit_code_empty_does_not_override_base() {
        let mut base = default_config();
        base.non_retryable_exit_codes = vec![2];
        let config = blank_cli().apply_overrides(base);
        // Empty CLI vec should NOT overwrite config-file-provided codes.
        assert_eq!(config.non_retryable_exit_codes, [2]);
    }
}
