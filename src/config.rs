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

/// Controls how often the loop engine posts comments to the active issue.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum CommentCadence {
    /// Comment at run start, run end, blockers, and failed iterations (default).
    #[default]
    Milestones,
    /// Comment after every iteration regardless of outcome.
    EveryIteration,
    /// Engine never posts comments; the agent is still prompted to do so.
    OffEngine,
}

impl std::fmt::Display for CommentCadence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommentCadence::Milestones => write!(f, "milestones"),
            CommentCadence::EveryIteration => write!(f, "every-iteration"),
            CommentCadence::OffEngine => write!(f, "off-engine"),
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
    /// GitHub issue number the engine should post run-lifecycle comments on.
    /// When `None` (or mode is `local`), engine comments are skipped.
    pub comment_issue_number: Option<u32>,
    /// How often the engine posts comments to the linked issue.
    #[serde(default)]
    pub comment_cadence: CommentCadence,
    /// When `true` the engine closes the owned issue at end-of-run if the
    /// agent left it open after completing all checklist items.  When `false`
    /// (default) the engine only logs a warning.
    #[serde(default)]
    pub auto_close_owned_issues: bool,
    /// Labels the engine ensures exist on the GitHub repository before the
    /// first iteration.  Only applied when `mode = "github"`.
    /// Default: `["bug", "enhancement", "tech-debt", "discovered-during-loop"]`.
    #[serde(default = "default_standard_labels")]
    pub standard_labels: Vec<String>,
}

fn default_issue_tracking_mode() -> IssueTrackingMode {
    IssueTrackingMode::Local
}

fn default_standard_labels() -> Vec<String> {
    vec![
        "bug".to_string(),
        "enhancement".to_string(),
        "tech-debt".to_string(),
        "discovered-during-loop".to_string(),
    ]
}

impl Default for IssueTrackingConfig {
    fn default() -> Self {
        Self {
            mode: IssueTrackingMode::Local,
            repo_owner: None,
            repo_name: None,
            local_promise_path: None,
            comment_issue_number: None,
            comment_cadence: CommentCadence::default(),
            auto_close_owned_issues: false,
            standard_labels: default_standard_labels(),
        }
    }
}

// ── PR management ────────────────────────────────────────────────────────────

/// PR iteration mode.
#[allow(clippy::enum_variant_names)]
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

fn default_triage_priority() -> TriagePriority {
    TriagePriority::Oldest
}

fn default_skip_labels() -> Vec<String> {
    vec!["do-not-loop".to_string(), "wip".to_string()]
}

/// How the multi-PR triage step orders open PRs when selecting which one to
/// advance first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TriagePriority {
    /// Oldest PR first (by creation date).  This is the default.
    Oldest,
    /// Newest PR first (by creation date).
    Newest,
    /// PRs with the fewest merge conflicts first.
    LeastConflicts,
}

impl std::fmt::Display for TriagePriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TriagePriority::Oldest => write!(f, "oldest"),
            TriagePriority::Newest => write!(f, "newest"),
            TriagePriority::LeastConflicts => write!(f, "least-conflicts"),
        }
    }
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
    /// When `true` the loop is allowed to force-push feature branches using
    /// `--force-with-lease`.  Default: `false` (safe default).
    #[serde(default)]
    pub allow_force_push: bool,
    /// Sentinel string the loop looks for in agent output to trigger PR
    /// creation.  Default: `LOOPER_READY_FOR_REVIEW`.
    #[serde(default)]
    pub ready_marker: Option<String>,
    /// Ordering policy for PR triage in `multi-pr` mode.  Default: `oldest`.
    #[serde(default = "default_triage_priority")]
    pub triage_priority: TriagePriority,
    /// Labels that cause a PR to be skipped during `multi-pr` triage.
    /// Default: `["do-not-loop", "wip"]`.
    #[serde(default = "default_skip_labels")]
    pub skip_labels: Vec<String>,
}

impl Default for PrManagementConfig {
    fn default() -> Self {
        Self {
            mode: default_pr_mode(),
            base_branch: default_base_branch(),
            branch_prefix: default_branch_prefix(),
            require_human_review: default_require_human_review(),
            allow_force_push: false,
            ready_marker: None,
            triage_priority: default_triage_priority(),
            skip_labels: default_skip_labels(),
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

fn default_stream_output() -> bool {
    true
}
fn default_artifacts_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(".code-looper/runs")
}
fn default_keep_runs() -> usize {
    10
}

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

/// Condition that must be satisfied for a policy rule to match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyCondition {
    /// Matches when the repository has at least one open pull request.
    HasOpenPrs,
    /// Matches when the repository has at least one open issue (and no open PRs
    /// unless a prior rule already handled them).
    HasOpenIssues,
    /// Always matches — use as the final fallback rule.
    Always,
}

impl std::fmt::Display for PolicyCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyCondition::HasOpenPrs => write!(f, "has_open_prs"),
            PolicyCondition::HasOpenIssues => write!(f, "has_open_issues"),
            PolicyCondition::Always => write!(f, "always"),
        }
    }
}

/// Workflow branch to execute when a policy rule matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyWorkflow {
    /// Review open pull requests.
    PrReview,
    /// Work on open GitHub issues.
    IssueExecution,
    /// Discover and create backlog items.
    BacklogDiscovery,
}

impl std::fmt::Display for PolicyWorkflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyWorkflow::PrReview => write!(f, "pr-review"),
            PolicyWorkflow::IssueExecution => write!(f, "issue-execution"),
            PolicyWorkflow::BacklogDiscovery => write!(f, "backlog-discovery"),
        }
    }
}

/// A single rule in the orchestration policy chain.
///
/// Rules are evaluated in order; the first rule whose condition matches the
/// current repository context determines the workflow branch and prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Condition that triggers this rule.
    pub condition: PolicyCondition,
    /// Workflow to execute when the condition matches.
    pub workflow: PolicyWorkflow,
    /// Optional prompt override for this rule.  When `None` the workflow's
    /// built-in default prompt is used.
    #[serde(default)]
    pub prompt_override: Option<String>,
}

/// Returns the default policy chain, which mirrors the hardcoded behaviour that
/// existed before pluggable policies were introduced.
pub fn default_policy_rules() -> Vec<PolicyRule> {
    vec![
        PolicyRule {
            condition: PolicyCondition::HasOpenPrs,
            workflow: PolicyWorkflow::PrReview,
            prompt_override: None,
        },
        PolicyRule {
            condition: PolicyCondition::HasOpenIssues,
            workflow: PolicyWorkflow::IssueExecution,
            prompt_override: None,
        },
        PolicyRule {
            condition: PolicyCondition::Always,
            workflow: PolicyWorkflow::BacklogDiscovery,
            prompt_override: None,
        },
    ]
}

/// Orchestration policy engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationConfig {
    /// Enable the policy engine; when true the engine selects a workflow branch
    /// per iteration and generates the prompt automatically.
    #[serde(default)]
    pub enabled: bool,
    /// GitHub repository owner (user or org). Required when enabled.
    pub repo_owner: Option<String>,
    /// GitHub repository name. Required when enabled.
    pub repo_name: Option<String>,
    /// Ordered list of policy rules evaluated against repository context each
    /// iteration.  First matching rule wins.  Defaults to the standard three-
    /// rule chain (pr-review → issue-execution → backlog-discovery).
    #[serde(default = "default_policy_rules")]
    pub policies: Vec<PolicyRule>,
}

impl Default for OrchestrationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            repo_owner: None,
            repo_name: None,
            policies: default_policy_rules(),
        }
    }
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

/// A single repository target for multi-repo orchestration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoTarget {
    /// Filesystem path to the repository root.
    /// Relative paths are resolved from the working directory at startup.
    pub path: PathBuf,
    /// Optional human-readable label used in run logs and summaries.
    /// Defaults to the final path component when omitted.
    pub name: Option<String>,
    /// Optional per-repo prompt that overrides the top-level `prompt_inline`
    /// or `prompt_file` setting for this specific target.
    pub prompt_override: Option<String>,
}

impl RepoTarget {
    /// Return the display name: explicit `name` or the last path component.
    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            self.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.path.display().to_string())
        })
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
    /// Milliseconds to wait between retry attempts (base delay for attempt 1).
    #[serde(default = "default_retry_backoff_ms")]
    pub retry_backoff_ms: u64,
    /// Exponential backoff multiplier applied per retry attempt.
    ///
    /// The delay for attempt N (1-indexed) is computed as:
    /// `retry_backoff_ms * retry_backoff_multiplier^(N-1)`.
    ///
    /// `1.0` (default) gives flat backoff; `2.0` doubles the delay each retry.
    #[serde(default = "default_retry_backoff_multiplier")]
    pub retry_backoff_multiplier: f64,
    /// Optional shell command to execute once after the loop finishes.
    /// The command is run via the system shell (`sh -c` on Unix).
    #[serde(default)]
    pub on_complete: Option<String>,
    /// Maximum seconds a single provider invocation may run before it is killed
    /// and the iteration is classified as `IterationOutcome::Timeout`.
    /// `None` (default) means no timeout — the provider runs until it exits.
    #[serde(default)]
    pub iteration_timeout_secs: Option<u64>,
    /// Issue tracking configuration.
    #[serde(default)]
    pub issue_tracking: IssueTrackingConfig,
    /// Pull-request management configuration.
    #[serde(default)]
    pub pr_management: PrManagementConfig,
    /// Telemetry / artifact collection configuration.
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    /// Additional repository targets for multi-repo orchestration.
    ///
    /// When non-empty, code-looper runs the configured loop for each entry in
    /// sequence instead of the default single-repo mode.  Each entry may
    /// supply a `prompt_override` to use a different prompt for that repo.
    #[serde(default)]
    pub multi_repo: Vec<RepoTarget>,
}

fn default_retry_backoff_ms() -> u64 {
    500
}

fn default_retry_backoff_multiplier() -> f64 {
    1.0
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
            retry_backoff_multiplier: default_retry_backoff_multiplier(),
            on_complete: None,
            iteration_timeout_secs: None,
            issue_tracking: IssueTrackingConfig::default(),
            pr_management: PrManagementConfig::default(),
            telemetry: TelemetryConfig::default(),
            multi_repo: Vec::new(),
        }
    }
}

/// Supported config file formats, detected from the file extension.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigFormat {
    /// TOML (`.toml`) — default format.
    Toml,
    /// YAML (`.yaml` or `.yml`).
    Yaml,
}

impl ConfigFormat {
    /// Detect format from a file path's extension.
    /// `.yaml` and `.yml` map to [`ConfigFormat::Yaml`]; everything else is
    /// treated as [`ConfigFormat::Toml`].
    pub fn detect(path: &std::path::Path) -> Self {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("yaml") | Some("yml") => ConfigFormat::Yaml,
            _ => ConfigFormat::Toml,
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

    /// Load config from a YAML file (`.yaml` / `.yml`).
    pub fn from_yaml_file(path: &std::path::Path) -> Result<Self, LooperError> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&content)
            .map_err(|e| LooperError::InvalidArgument(format!("YAML parse error: {e}")))?;
        Ok(config)
    }

    /// Load config from a file, auto-detecting format by extension.
    ///
    /// `.yaml` / `.yml` extensions are parsed as YAML; everything else is
    /// parsed as TOML.
    pub fn from_file(path: &std::path::Path) -> Result<Self, LooperError> {
        match ConfigFormat::detect(path) {
            ConfigFormat::Yaml => Self::from_yaml_file(path),
            ConfigFormat::Toml => Self::from_toml_file(path),
        }
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
        if let Some(path) = &self.prompt_file {
            if !path.exists() {
                return Err(LooperError::InvalidArgument(format!(
                    "--prompt-file '{}' does not exist",
                    path.display()
                )));
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
    fn prompt_file_pointing_to_missing_path_is_invalid() {
        let config = LoopConfig {
            prompt_file: Some("/nonexistent/path/to/prompt.md".into()),
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("does not exist"),
            "expected 'does not exist' in error: {err}"
        );
    }

    #[test]
    fn prompt_file_pointing_to_existing_file_is_valid() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "prompt content").unwrap();
        let config = LoopConfig {
            prompt_file: Some(f.path().to_path_buf()),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
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
                ..OrchestrationConfig::default()
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
                ..OrchestrationConfig::default()
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
                ..OrchestrationConfig::default()
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
        assert!(config.iteration_timeout_secs.is_none());
    }

    #[test]
    fn parse_toml_iteration_timeout_secs() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "claude"
iterations = 1
log_level = "info"
iteration_timeout_secs = 120
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.iteration_timeout_secs, Some(120));
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

    // ── Pluggable policy tests ────────────────────────────────────────────────

    #[test]
    fn default_orchestration_has_three_policy_rules() {
        let cfg = OrchestrationConfig::default();
        assert_eq!(cfg.policies.len(), 3);
        assert_eq!(cfg.policies[0].condition, PolicyCondition::HasOpenPrs);
        assert_eq!(cfg.policies[1].condition, PolicyCondition::HasOpenIssues);
        assert_eq!(cfg.policies[2].condition, PolicyCondition::Always);
    }

    #[test]
    fn parse_toml_with_custom_policies() {
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

[[orchestration.policies]]
condition = "always"
workflow = "issue-execution"
prompt_override = "Only work on issues."
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.orchestration.policies.len(), 1);
        let rule = &config.orchestration.policies[0];
        assert_eq!(rule.condition, PolicyCondition::Always);
        assert_eq!(rule.workflow, PolicyWorkflow::IssueExecution);
        assert_eq!(
            rule.prompt_override.as_deref(),
            Some("Only work on issues.")
        );
    }

    #[test]
    fn parse_toml_with_multiple_policies() {
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

[[orchestration.policies]]
condition = "has_open_prs"
workflow = "pr-review"

[[orchestration.policies]]
condition = "always"
workflow = "backlog-discovery"
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.orchestration.policies.len(), 2);
        assert_eq!(
            config.orchestration.policies[0].condition,
            PolicyCondition::HasOpenPrs
        );
        assert_eq!(
            config.orchestration.policies[1].condition,
            PolicyCondition::Always
        );
    }

    #[test]
    fn policy_workflow_display() {
        assert_eq!(PolicyWorkflow::PrReview.to_string(), "pr-review");
        assert_eq!(
            PolicyWorkflow::IssueExecution.to_string(),
            "issue-execution"
        );
        assert_eq!(
            PolicyWorkflow::BacklogDiscovery.to_string(),
            "backlog-discovery"
        );
    }

    // ── Exponential backoff config tests ─────────────────────────────────────

    #[test]
    fn default_retry_backoff_multiplier_is_one() {
        let config = LoopConfig::default();
        assert_eq!(config.retry_backoff_multiplier, 1.0);
    }

    #[test]
    fn parse_toml_with_exponential_backoff() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "claude"
iterations = 3
log_level = "info"
max_retries = 3
retry_backoff_ms = 100
retry_backoff_multiplier = 2.0
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.retry_backoff_ms, 100);
        assert_eq!(config.retry_backoff_multiplier, 2.0);
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
                ..IssueTrackingConfig::default()
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
                ..IssueTrackingConfig::default()
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
                ..IssueTrackingConfig::default()
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
                ..IssueTrackingConfig::default()
            },
            orchestration: OrchestrationConfig {
                enabled: true,
                repo_owner: Some("org".to_string()),
                repo_name: Some("project".to_string()),
                ..OrchestrationConfig::default()
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
                ..IssueTrackingConfig::default()
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
                ..IssueTrackingConfig::default()
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
    fn issue_tracking_defaults_include_standard_labels() {
        let config = LoopConfig::default();
        assert!(!config.issue_tracking.standard_labels.is_empty());
        assert!(config
            .issue_tracking
            .standard_labels
            .contains(&"bug".to_string()));
        assert!(config
            .issue_tracking
            .standard_labels
            .contains(&"discovered-during-loop".to_string()));
        assert!(!config.issue_tracking.auto_close_owned_issues);
    }

    #[test]
    fn parse_toml_with_auto_close_and_custom_labels() {
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
auto_close_owned_issues = true
standard_labels = ["bug", "wip", "my-team"]
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert!(config.issue_tracking.auto_close_owned_issues);
        assert_eq!(
            config.issue_tracking.standard_labels,
            vec!["bug".to_string(), "wip".to_string(), "my-team".to_string()]
        );
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

    // ── Multi-repo config tests ───────────────────────────────────────────────

    #[test]
    fn multi_repo_is_empty_by_default() {
        assert!(LoopConfig::default().multi_repo.is_empty());
    }

    #[test]
    fn parse_toml_with_multi_repo_entries() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "claude"
iterations = 1
log_level = "info"

[[multi_repo]]
path = "/repos/project-a"
name = "project-a"

[[multi_repo]]
path = "/repos/project-b"
prompt_override = "Run linting only"
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.multi_repo.len(), 2);

        let a = &config.multi_repo[0];
        assert_eq!(a.path, std::path::PathBuf::from("/repos/project-a"));
        assert_eq!(a.name.as_deref(), Some("project-a"));
        assert!(a.prompt_override.is_none());

        let b = &config.multi_repo[1];
        assert_eq!(b.path, std::path::PathBuf::from("/repos/project-b"));
        assert!(b.name.is_none());
        assert_eq!(b.prompt_override.as_deref(), Some("Run linting only"));
    }

    #[test]
    fn repo_target_display_name_uses_explicit_name() {
        let t = RepoTarget {
            path: "/repos/my-project".into(),
            name: Some("custom".to_string()),
            prompt_override: None,
        };
        assert_eq!(t.display_name(), "custom");
    }

    #[test]
    fn repo_target_display_name_falls_back_to_dir() {
        let t = RepoTarget {
            path: "/repos/my-project".into(),
            name: None,
            prompt_override: None,
        };
        assert_eq!(t.display_name(), "my-project");
    }

    #[test]
    fn multi_repo_serde_round_trip() {
        let config = LoopConfig {
            multi_repo: vec![
                RepoTarget {
                    path: "/tmp/repo-a".into(),
                    name: Some("repo-a".to_string()),
                    prompt_override: None,
                },
                RepoTarget {
                    path: "/tmp/repo-b".into(),
                    name: None,
                    prompt_override: Some("custom task".to_string()),
                },
            ],
            ..LoopConfig::default()
        };
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: LoopConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.multi_repo.len(), 2);
        assert_eq!(deserialized.multi_repo[0], config.multi_repo[0]);
        assert_eq!(deserialized.multi_repo[1], config.multi_repo[1]);
    }

    // ── YAML config tests ─────────────────────────────────────────────────────

    /// Helper: write content to a temp file with the given suffix.
    fn write_temp_file(suffix: &str, content: &str) -> NamedTempFile {
        let file = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        std::fs::write(file.path(), content).unwrap();
        file
    }

    #[test]
    fn parse_yaml_config_file() {
        let file = write_temp_file(
            ".yaml",
            "provider: copilot\niterations: 7\nlog_level: debug\n",
        );
        let config = LoopConfig::from_yaml_file(file.path()).unwrap();
        assert_eq!(config.provider, Provider::Copilot);
        assert_eq!(config.iterations, 7);
        assert_eq!(config.log_level, "debug");
    }

    #[test]
    fn parse_yml_config_file() {
        let file = write_temp_file(
            ".yml",
            "provider: codex\niterations: 3\nlog_level: info\nprompt_inline: \"run lints\"\n",
        );
        let config = LoopConfig::from_yaml_file(file.path()).unwrap();
        assert_eq!(config.provider, Provider::Codex);
        assert_eq!(config.prompt_inline.as_deref(), Some("run lints"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn from_file_detects_yaml_extension() {
        let file = write_temp_file(
            ".yaml",
            "provider: claude\niterations: 2\nlog_level: info\n",
        );
        let config = LoopConfig::from_file(file.path()).unwrap();
        assert_eq!(config.provider, Provider::Claude);
        assert_eq!(config.iterations, 2);
    }

    #[test]
    fn from_file_detects_toml_extension() {
        let file = write_temp_file(
            ".toml",
            "provider = \"copilot\"\niterations = 4\nlog_level = \"info\"\n",
        );
        let config = LoopConfig::from_file(file.path()).unwrap();
        assert_eq!(config.provider, Provider::Copilot);
        assert_eq!(config.iterations, 4);
    }

    #[test]
    fn from_file_defaults_to_toml_for_unknown_extension() {
        let file = write_temp_file(
            ".conf",
            "provider = \"codex\"\niterations = 1\nlog_level = \"info\"\n",
        );
        let config = LoopConfig::from_file(file.path()).unwrap();
        assert_eq!(config.provider, Provider::Codex);
    }

    #[test]
    fn config_format_detect_yaml() {
        assert_eq!(
            ConfigFormat::detect(std::path::Path::new("looper.yaml")),
            ConfigFormat::Yaml
        );
        assert_eq!(
            ConfigFormat::detect(std::path::Path::new("looper.yml")),
            ConfigFormat::Yaml
        );
        assert_eq!(
            ConfigFormat::detect(std::path::Path::new("looper.YAML")),
            ConfigFormat::Yaml
        );
    }

    #[test]
    fn config_format_detect_toml() {
        assert_eq!(
            ConfigFormat::detect(std::path::Path::new("looper.toml")),
            ConfigFormat::Toml
        );
        assert_eq!(
            ConfigFormat::detect(std::path::Path::new("looper")),
            ConfigFormat::Toml
        );
    }

    #[test]
    fn yaml_parse_error_returns_invalid_argument() {
        let file = write_temp_file(".yaml", "provider: [invalid yaml structure\n");
        let err = LoopConfig::from_yaml_file(file.path()).unwrap_err();
        assert!(err.to_string().contains("YAML parse error"));
    }

    #[test]
    fn parse_yaml_with_orchestration() {
        let file = write_temp_file(
            ".yaml",
            "provider: claude\niterations: 1\nlog_level: info\norchestration:\n  enabled: true\n  repo_owner: acme\n  repo_name: my-repo\n",
        );
        let config = LoopConfig::from_yaml_file(file.path()).unwrap();
        assert!(config.orchestration.enabled);
        assert_eq!(config.orchestration.repo_owner.as_deref(), Some("acme"));
        assert_eq!(config.orchestration.repo_name.as_deref(), Some("my-repo"));
    }
}
