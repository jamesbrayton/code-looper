use crate::error::LooperError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Issue tracking ────────────────────────────────────────────────────────────

/// Issue tracking backend mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum IssueTrackingMode {
    /// GitHub Issues via the `gh` CLI (default for production use).
    Github,
    /// Local markdown file — dev/debug only.
    Local,
}

impl std::fmt::Display for IssueTrackingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueTrackingMode::Github => write!(f, "github"),
            IssueTrackingMode::Local => write!(f, "local"),
        }
    }
}

/// Issue tracking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueTrackingConfig {
    /// Backend mode (`github` or `local`).  Defaults to `local` so that
    /// existing configs work without changes — but production use should
    /// always set `github`.
    #[serde(default = "default_issue_tracking_mode")]
    pub mode: IssueTrackingMode,
    /// GitHub repository owner (required when `mode = "github"`, unless
    /// inherited from `[orchestration].repo_owner`).
    pub repo_owner: Option<String>,
    /// GitHub repository name (required when `mode = "github"`, unless
    /// inherited from `[orchestration].repo_name`).
    pub repo_name: Option<String>,
    /// Path to the local promise markdown file (when `mode = "local"`).
    /// Defaults to `.code-looper/promise.md`.
    pub local_promise_path: Option<PathBuf>,
}

fn default_issue_tracking_mode() -> IssueTrackingMode {
    IssueTrackingMode::Local
}

impl Default for IssueTrackingConfig {
    fn default() -> Self {
        Self {
            mode: IssueTrackingMode::Local,
            repo_owner: None,
            repo_name: None,
            local_promise_path: None,
        }
    }
}

// ── PR management ────────────────────────────────────────────────────────────

/// PR iteration mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum PrMode {
    /// Commit and push to a feature branch only; never open a PR.
    NoPr,
    /// Work on one feature branch; open a PR when work is shippable, then
    /// continue pushing to that branch until merged.
    SinglePr,
    /// On each iteration, triage open PRs first (review, fix, merge); open new
    /// feature branches for issue work only when no PR can be advanced.
    MultiPr,
}

impl std::fmt::Display for PrMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrMode::NoPr => write!(f, "no-pr"),
            PrMode::SinglePr => write!(f, "single-pr"),
            PrMode::MultiPr => write!(f, "multi-pr"),
        }
    }
}

fn default_pr_mode() -> PrMode {
    PrMode::NoPr
}

fn default_base_branch() -> String {
    "main".to_string()
}

fn default_branch_prefix() -> String {
    "loop/".to_string()
}

fn default_require_human_review() -> bool {
    true
}

/// Pull-request management configuration (`[pr_management]` TOML section).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrManagementConfig {
    /// PR strategy mode.  Default: `no-pr`.
    #[serde(default = "default_pr_mode")]
    pub mode: PrMode,
    /// Branch to open PRs into.  Default: `main`.
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
    /// Prefix for feature branches created by the loop.  Default: `loop/`.
    #[serde(default = "default_branch_prefix")]
    pub branch_prefix: String,
    /// When `true` the loop never merges a PR itself — human review is the
    /// gate.  Default: `true`.
    #[serde(default = "default_require_human_review")]
    pub require_human_review: bool,
}

impl Default for PrManagementConfig {
    fn default() -> Self {
        Self {
            mode: default_pr_mode(),
            base_branch: default_base_branch(),
            branch_prefix: default_branch_prefix(),
            require_human_review: default_require_human_review(),
        }
    }
}

// ── Orchestration ─────────────────────────────────────────────────────────────

// ── Telemetry ─────────────────────────────────────────────────────────────────

/// Telemetry / artifact collection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Stream provider stdout/stderr to the terminal in real time (tagged).
    /// Default: `true`.
    #[serde(default = "default_stream_output")]
    pub stream_output: bool,
    /// Root directory for per-run artifact directories.
    /// Default: `.code-looper/runs`.
    #[serde(default = "default_artifacts_dir")]
    pub artifacts_dir: std::path::PathBuf,
    /// Number of most-recent run directories to retain.
    /// Older runs are pruned after each new run completes.
    /// Default: 10.
    #[serde(default = "default_keep_runs")]
    pub keep_runs: usize,
    /// When `true`, skip writing the markdown summary and printing the
    /// condensed terminal summary.  Useful for scripted/CI use.
    /// Default: `false`.
    #[serde(default)]
    pub no_summary: bool,
}

fn default_stream_output() -> bool { true }
fn default_artifacts_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(".code-looper/runs")
}
fn default_keep_runs() -> usize { 10 }

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            stream_output: default_stream_output(),
            artifacts_dir: default_artifacts_dir(),
            keep_runs: default_keep_runs(),
            no_summary: false,
        }
    }
}

/// Orchestration policy engine configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrchestrationConfig {
    /// Enable the policy engine; when true the engine selects a workflow branch
    /// per iteration and generates the prompt automatically.
    pub enabled: bool,
    /// GitHub repository owner (user or org). Required when enabled.
    pub repo_owner: Option<String>,
    /// GitHub repository name. Required when enabled.
    pub repo_name: Option<String>,
}


/// Supported agent CLI providers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Copilot,
    Codex,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Claude => write!(f, "claude"),
            Provider::Copilot => write!(f, "copilot"),
            Provider::Codex => write!(f, "codex"),
        }
    }
}

/// Resolved runtime configuration for a single loop run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopConfig {
    /// Provider to use for loop execution.
    pub provider: Provider,
    /// Number of iterations; -1 means infinite.
    pub iterations: i64,
    /// Inline prompt string (mutually exclusive with prompt_file).
    pub prompt_inline: Option<String>,
    /// Path to a markdown prompt file (mutually exclusive with prompt_inline).
    pub prompt_file: Option<PathBuf>,
    /// Tracing log level (e.g. "info", "debug").
    pub log_level: String,
    /// Orchestration policy engine settings.
    #[serde(default)]
    pub orchestration: OrchestrationConfig,
    /// Workspace directory for prerequisite checks (defaults to cwd).
    #[serde(default)]
    pub workspace_dir: Option<PathBuf>,
    /// Skip workspace prerequisite checks at startup.
    #[serde(default)]
    pub skip_prereq_check: bool,
    /// [UNSAFE] Allow GitHub context resolution via direct gh CLI calls and
    /// disable the MCP-only prompt preamble.
    #[serde(default)]
    pub allow_direct_github: bool,
    /// Stop the loop after the first iteration that fails (non-zero exit after
    /// all retries are exhausted).
    #[serde(default)]
    pub stop_on_failure: bool,
    /// Number of additional retry attempts per iteration on non-zero exit.
    /// `0` means no retries (fail fast).
    #[serde(default)]
    pub max_retries: u32,
    /// Milliseconds to wait between retry attempts.
    #[serde(default = "default_retry_backoff_ms")]
    pub retry_backoff_ms: u64,
    /// Optional shell command to execute once after the loop finishes.
    /// The command is run via the system shell (`sh -c` on Unix).
    #[serde(default)]
    pub on_complete: Option<String>,
    /// Issue tracking configuration.
    #[serde(default)]
    pub issue_tracking: IssueTrackingConfig,
    /// Pull-request management configuration.
    #[serde(default)]
    pub pr_management: PrManagementConfig,
    /// Telemetry / artifact collection configuration.
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

fn default_retry_backoff_ms() -> u64 {
    500
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            provider: Provider::Claude,
            iterations: 1,
            prompt_inline: None,
            prompt_file: None,
            log_level: "info".to_string(),
            orchestration: OrchestrationConfig::default(),
            workspace_dir: None,
            skip_prereq_check: false,
            allow_direct_github: false,
            stop_on_failure: false,
            max_retries: 0,
            retry_backoff_ms: default_retry_backoff_ms(),
            on_complete: None,
            issue_tracking: IssueTrackingConfig::default(),
            pr_management: PrManagementConfig::default(),
            telemetry: TelemetryConfig::default(),
        }
    }
}

impl LoopConfig {
    /// Load config from a TOML file.
    pub fn from_toml_file(path: &std::path::Path) -> Result<Self, LooperError> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Validate that the config is internally consistent.
    pub fn validate(&self) -> Result<(), LooperError> {
        if self.prompt_inline.is_some() && self.prompt_file.is_some() {
            return Err(LooperError::InvalidArgument(
                "--prompt-inline and --prompt-file are mutually exclusive".to_string(),
            ));
        }
        if self.iterations < -1 || self.iterations == 0 {
            return Err(LooperError::InvalidArgument(
                "--iterations must be a positive integer or -1 for infinite".to_string(),
            ));
        }
        if self.orchestration.enabled {
            if self.orchestration.repo_owner.is_none() {
                return Err(LooperError::InvalidArgument(
                    "orchestration requires --repo-owner".to_string(),
                ));
            }
            if self.orchestration.repo_name.is_none() {
                return Err(LooperError::InvalidArgument(
                    "orchestration requires --repo-name".to_string(),
                ));
            }
        }
        if let Some(cmd) = &self.on_complete {
            if cmd.trim().is_empty() {
                return Err(LooperError::InvalidArgument(
                    "--on-complete must not be an empty string".to_string(),
                ));
            }
        }
        // multi-pr mode requires GitHub issue tracking (local mode can't track PRs).
        if self.pr_management.mode == PrMode::MultiPr
            && self.issue_tracking.mode != IssueTrackingMode::Github
        {
            return Err(LooperError::InvalidArgument(
                "pr_management.mode=\"multi-pr\" requires issue_tracking.mode=\"github\""
                    .to_string(),
            ));
        }
        // When github mode is active, owner and repo must be resolvable.
        if self.issue_tracking.mode == IssueTrackingMode::Github {
            let owner = self
                .issue_tracking
                .repo_owner
                .as_deref()
                .or(self.orchestration.repo_owner.as_deref());
            let repo = self
                .issue_tracking
                .repo_name
                .as_deref()
                .or(self.orchestration.repo_name.as_deref());
            if owner.is_none() {
                return Err(LooperError::InvalidArgument(
                    "issue_tracking.mode=\"github\" requires repo_owner \
                     (set issue_tracking.repo_owner or orchestration.repo_owner)"
                        .to_string(),
                ));
            }
            if repo.is_none() {
                return Err(LooperError::InvalidArgument(
                    "issue_tracking.mode=\"github\" requires repo_name \
                     (set issue_tracking.repo_name or orchestration.repo_name)"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn default_config_is_valid() {
        assert!(LoopConfig::default().validate().is_ok());
    }

    #[test]
    fn conflicting_prompts_are_invalid() {
        let config = LoopConfig {
            prompt_inline: Some("hello".to_string()),
            prompt_file: Some("prompt.md".into()),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn zero_iterations_is_invalid() {
        let config = LoopConfig {
            iterations: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn negative_two_iterations_is_invalid() {
        let config = LoopConfig {
            iterations: -2,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn negative_one_iterations_is_valid() {
        let config = LoopConfig {
            iterations: -1,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn parse_toml_config_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "copilot"
iterations = 5
log_level = "debug"
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.provider, Provider::Copilot);
        assert_eq!(config.iterations, 5);
        assert_eq!(config.log_level, "debug");
    }

    #[test]
    fn parse_toml_with_inline_prompt() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "codex"
iterations = 3
log_level = "info"
prompt_inline = "run the tests"
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.provider, Provider::Codex);
        assert_eq!(config.prompt_inline.as_deref(), Some("run the tests"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn provider_display() {
        assert_eq!(Provider::Claude.to_string(), "claude");
        assert_eq!(Provider::Copilot.to_string(), "copilot");
        assert_eq!(Provider::Codex.to_string(), "codex");
    }

    #[test]
    fn orchestration_disabled_by_default() {
        assert!(!LoopConfig::default().orchestration.enabled);
    }

    #[test]
    fn orchestration_enabled_requires_owner_and_name() {
        let config = LoopConfig {
            orchestration: OrchestrationConfig {
                enabled: true,
                repo_owner: None,
                repo_name: None,
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn orchestration_enabled_requires_repo_name() {
        let config = LoopConfig {
            orchestration: OrchestrationConfig {
                enabled: true,
                repo_owner: Some("owner".to_string()),
                repo_name: None,
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn orchestration_enabled_with_both_fields_is_valid() {
        let config = LoopConfig {
            orchestration: OrchestrationConfig {
                enabled: true,
                repo_owner: Some("owner".to_string()),
                repo_name: Some("repo".to_string()),
            },
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn default_retry_fields() {
        let config = LoopConfig::default();
        assert!(!config.stop_on_failure);
        assert_eq!(config.max_retries, 0);
        assert_eq!(config.retry_backoff_ms, 500);
        assert!(config.on_complete.is_none());
    }

    #[test]
    fn empty_on_complete_is_invalid() {
        let config = LoopConfig {
            on_complete: Some("  ".to_string()),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn nonempty_on_complete_is_valid() {
        let config = LoopConfig {
            on_complete: Some("echo done".to_string()),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn parse_toml_with_retry_fields() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "claude"
iterations = 5
log_level = "info"
stop_on_failure = true
max_retries = 2
retry_backoff_ms = 250
on_complete = "echo done"
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert!(config.stop_on_failure);
        assert_eq!(config.max_retries, 2);
        assert_eq!(config.retry_backoff_ms, 250);
        assert_eq!(config.on_complete.as_deref(), Some("echo done"));
    }

    #[test]
    fn parse_toml_with_orchestration() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "claude"
iterations = 1
log_level = "info"

[orchestration]
enabled = true
repo_owner = "acme"
repo_name = "my-repo"
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert!(config.orchestration.enabled);
        assert_eq!(config.orchestration.repo_owner.as_deref(), Some("acme"));
        assert_eq!(config.orchestration.repo_name.as_deref(), Some("my-repo"));
    }

    // ── Issue tracking config tests ───────────────────────────────────────────

    #[test]
    fn issue_tracking_defaults_to_local() {
        let config = LoopConfig::default();
        assert_eq!(config.issue_tracking.mode, IssueTrackingMode::Local);
        assert!(config.issue_tracking.repo_owner.is_none());
        assert!(config.issue_tracking.repo_name.is_none());
        assert!(config.issue_tracking.local_promise_path.is_none());
    }

    #[test]
    fn github_mode_without_credentials_is_invalid() {
        let config = LoopConfig {
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: None,
                repo_name: None,
                local_promise_path: None,
            },
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("repo_owner"));
    }

    #[test]
    fn github_mode_without_repo_name_is_invalid() {
        let config = LoopConfig {
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: Some("owner".to_string()),
                repo_name: None,
                local_promise_path: None,
            },
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("repo_name"));
    }

    #[test]
    fn github_mode_with_credentials_is_valid() {
        let config = LoopConfig {
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: Some("owner".to_string()),
                repo_name: Some("repo".to_string()),
                local_promise_path: None,
            },
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn github_mode_inherits_orchestration_credentials() {
        let config = LoopConfig {
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: None,
                repo_name: None,
                local_promise_path: None,
            },
            orchestration: OrchestrationConfig {
                enabled: true,
                repo_owner: Some("org".to_string()),
                repo_name: Some("project".to_string()),
            },
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn local_mode_is_always_valid() {
        let config = LoopConfig {
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Local,
                repo_owner: None,
                repo_name: None,
                local_promise_path: None,
            },
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn parse_toml_with_issue_tracking_github() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "claude"
iterations = 1
log_level = "info"

[issue_tracking]
mode = "github"
repo_owner = "acme"
repo_name = "my-repo"
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.issue_tracking.mode, IssueTrackingMode::Github);
        assert_eq!(config.issue_tracking.repo_owner.as_deref(), Some("acme"));
        assert_eq!(config.issue_tracking.repo_name.as_deref(), Some("my-repo"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn parse_toml_with_issue_tracking_local() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "claude"
iterations = 1
log_level = "info"

[issue_tracking]
mode = "local"
local_promise_path = ".code-looper/dev.md"
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.issue_tracking.mode, IssueTrackingMode::Local);
        assert_eq!(
            config.issue_tracking.local_promise_path,
            Some(PathBuf::from(".code-looper/dev.md"))
        );
        assert!(config.validate().is_ok());
    }

    // ── PR management config tests ─────────────────────────────────────────────

    #[test]
    fn pr_management_defaults() {
        let config = LoopConfig::default();
        assert_eq!(config.pr_management.mode, PrMode::NoPr);
        assert_eq!(config.pr_management.base_branch, "main");
        assert_eq!(config.pr_management.branch_prefix, "loop/");
        assert!(config.pr_management.require_human_review);
    }

    #[test]
    fn multi_pr_requires_github_issue_tracking() {
        let config = LoopConfig {
            pr_management: PrManagementConfig {
                mode: PrMode::MultiPr,
                ..PrManagementConfig::default()
            },
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Local,
                ..IssueTrackingConfig::default()
            },
            ..LoopConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("multi-pr"));
        assert!(err.to_string().contains("github"));
    }

    #[test]
    fn multi_pr_with_github_issue_tracking_is_valid() {
        let config = LoopConfig {
            pr_management: PrManagementConfig {
                mode: PrMode::MultiPr,
                ..PrManagementConfig::default()
            },
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: Some("owner".to_string()),
                repo_name: Some("repo".to_string()),
                local_promise_path: None,
            },
            ..LoopConfig::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn no_pr_and_single_pr_work_with_local_issue_tracking() {
        for mode in [PrMode::NoPr, PrMode::SinglePr] {
            let config = LoopConfig {
                pr_management: PrManagementConfig {
                    mode,
                    ..PrManagementConfig::default()
                },
                ..LoopConfig::default()
            };
            assert!(config.validate().is_ok());
        }
    }

    #[test]
    fn parse_toml_with_pr_management() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "claude"
iterations = 1
log_level = "info"

[pr_management]
mode = "single-pr"
base_branch = "develop"
branch_prefix = "feat/"
require_human_review = false
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.pr_management.mode, PrMode::SinglePr);
        assert_eq!(config.pr_management.base_branch, "develop");
        assert_eq!(config.pr_management.branch_prefix, "feat/");
        assert!(!config.pr_management.require_human_review);
    }
}
