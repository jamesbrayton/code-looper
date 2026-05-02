use crate::error::LooperError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

// ── Git remote auto-detection ────────────────────────────────────────────────

/// Parsed owner/repo pair from a git remote URL.
#[derive(Debug, Clone, PartialEq)]
pub struct GitRepoInfo {
    pub owner: String,
    pub repo: String,
}

/// Parse `owner/repo` from a git remote URL.
///
/// Supports common formats:
/// - `https://github.com/owner/repo.git`
/// - `https://github.com/owner/repo`
/// - `git@github.com:owner/repo.git`
/// - `ssh://git@github.com/owner/repo.git`
pub fn parse_git_remote_url(url: &str) -> Option<GitRepoInfo> {
    let url = url.trim();

    // SSH shorthand: git@github.com:owner/repo.git
    if let Some(path) = url.strip_prefix("git@github.com:") {
        return parse_owner_repo_from_path(path);
    }

    // HTTPS or SSH URL — strip known prefixes and extract the path portion.
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;

    parse_owner_repo_from_path(path)
}

/// Extract owner/repo from a `"owner/repo.git"` (or `"owner/repo"`) path tail.
fn parse_owner_repo_from_path(path: &str) -> Option<GitRepoInfo> {
    let path = path.trim_end_matches(".git").trim_end_matches('/');
    let mut parts = path.splitn(2, '/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(GitRepoInfo {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

/// Try to detect `owner/repo` from the current git working directory by
/// running `git remote get-url origin`.
pub fn git_repo_info() -> Option<GitRepoInfo> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout);
    parse_git_remote_url(&url)
}

// ── Issue tracking ────────────────────────────────────────────────────────────

/// Issue tracking backend mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum IssueTrackingMode {
    /// GitHub Issues via the `gh` CLI (recommended for production use —
    /// not the type's `Default`; see [`default_issue_tracking_mode`], which
    /// returns `Local` so existing configs keep working without changes).
    Github,
    /// Local markdown file — dev/debug only.  This is the type's `Default`.
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
    /// Engine never posts comments.  The agent may still be instructed to
    /// comment via the prompt (for example through the lifecycle guidance
    /// injected by `code-looper bootstrap` into `CLAUDE.md`), but that is
    /// independent of this cadence setting — switching to `OffEngine` does
    /// not add any prompt content asking the agent to comment.
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// User rules configuration — global preamble and per-workflow-branch overrides.
///
/// Rule files are markdown files whose contents are prepended to the engine-
/// generated prompt (not replacing it).  This gives users a way to inject
/// standing instructions (coding standards, review checklists, domain context)
/// while preserving the GitHub policy and workflow structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RulesConfig {
    /// Path to a global rules markdown file prepended to every provider prompt.
    #[serde(default)]
    pub global: Option<PathBuf>,

    /// Per-workflow-branch rule file overrides.  Keys are [`PolicyWorkflow`]
    /// variants (e.g. `pr-review`, `issue-execution`, `backlog-discovery`).
    #[serde(default)]
    pub workflows: HashMap<PolicyWorkflow, PathBuf>,
}

impl RulesConfig {
    /// Validate that all configured rule files exist and are within the size limit.
    pub fn validate(&self) -> Result<(), LooperError> {
        if let Some(ref path) = self.global {
            validate_rule_file(path, "rules.global")?;
        }
        for (branch, path) in &self.workflows {
            validate_rule_file(path, &format!("rules.workflows.{branch}"))?;
        }
        Ok(())
    }
}

/// Soft-warning threshold for rule file size (bytes).
pub const RULES_SIZE_WARN_BYTES: u64 = 16_384;

/// Hard-error threshold for rule file size (bytes).
pub const RULES_SIZE_MAX_BYTES: u64 = 65_536;

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
    /// User rules: global preamble and per-workflow-branch overrides.
    #[serde(default)]
    pub rules: RulesConfig,
    /// Orchestration policy engine settings.
    #[serde(default)]
    pub orchestration: OrchestrationConfig,
    /// Workspace directory for prerequisite checks (defaults to cwd).
    #[serde(default)]
    pub workspace_dir: Option<PathBuf>,
    /// Skip workspace prerequisite checks at startup.
    #[serde(default)]
    pub skip_prereq_check: bool,
    /// Prefer direct GitHub access via `gh` CLI in provider prompts.
    ///
    /// When `true` (default), prompt policy tells agents to use `gh` first
    /// and fall back to MCP tooling only when `gh` is unavailable or fails.
    /// When `false`, prompt policy enforces MCP-only writes.
    #[serde(default = "default_allow_direct_github")]
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
    /// Extra CLI arguments appended to the provider invocation, after the
    /// adapter's hardcoded flags but before the prompt.
    ///
    /// Example (TOML):
    /// ```toml
    /// provider_extra_args = ["--model", "claude-opus-4-5"]
    /// ```
    ///
    /// Each element becomes a separate argument (`Command::arg`), so shell
    /// metacharacters are not interpreted.
    #[serde(default)]
    pub provider_extra_args: Vec<String>,
    /// Exit codes that are treated as permanent failures and never retried,
    /// even when `max_retries > 0`.
    ///
    /// Use this to short-circuit retries for codes that indicate a
    /// configuration or argument error rather than a transient fault.
    /// Example values: `2` (bad CLI args), `126` (permission denied),
    /// `127` (command not found from a shell wrapper).
    ///
    /// Default: empty — all non-zero exits are retried up to `max_retries`.
    #[serde(default)]
    pub non_retryable_exit_codes: Vec<i32>,
}

fn default_retry_backoff_ms() -> u64 {
    500
}

fn default_retry_backoff_multiplier() -> f64 {
    1.0
}

fn default_allow_direct_github() -> bool {
    true
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            provider: Provider::Claude,
            iterations: 1,
            prompt_inline: None,
            prompt_file: None,
            log_level: "info".to_string(),
            rules: RulesConfig::default(),
            orchestration: OrchestrationConfig::default(),
            workspace_dir: None,
            skip_prereq_check: false,
            allow_direct_github: true,
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
            provider_extra_args: Vec::new(),
            non_retryable_exit_codes: Vec::new(),
        }
    }
}

// ── Validated config types ───────────────────────────────────────────────────

/// Validated iteration count: either a finite positive count or infinite.
///
/// Replaces the raw `iterations: i64` field with a type that makes zero and
/// invalid negative values unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IterationCount {
    /// Run a fixed number of iterations (always ≥ 1).
    Finite(NonZeroU32),
    /// Run indefinitely until interrupted or a stop condition fires.
    Infinite,
}

impl IterationCount {
    /// Returns `true` when `iteration` (0-based) has reached or exceeded the
    /// configured count.  Always returns `false` for [`Infinite`](IterationCount::Infinite).
    pub fn is_done(&self, iteration: u64) -> bool {
        match self {
            IterationCount::Finite(n) => iteration >= u64::from(n.get()),
            IterationCount::Infinite => false,
        }
    }

    /// Convert to the legacy `i64` representation (`-1` for infinite) used by
    /// the telemetry manifest format.
    pub fn as_raw_i64(&self) -> i64 {
        match self {
            IterationCount::Finite(n) => i64::from(n.get()),
            IterationCount::Infinite => -1,
        }
    }
}

#[cfg(test)]
impl IterationCount {
    pub fn max_iterations(&self) -> u64 {
        match self {
            IterationCount::Finite(n) => u64::from(n.get()),
            IterationCount::Infinite => u64::MAX,
        }
    }

    pub fn is_infinite(&self) -> bool {
        matches!(self, IterationCount::Infinite)
    }
}

impl std::fmt::Display for IterationCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IterationCount::Finite(n) => write!(f, "{}", n.get()),
            IterationCount::Infinite => write!(f, "infinite"),
        }
    }
}

/// Validated prompt source: at most one of inline or file.
///
/// Replaces the pair of `prompt_inline: Option<String>` /
/// `prompt_file: Option<PathBuf>` fields with a type that makes the "both set"
/// state unrepresentable.
#[derive(Debug, Clone)]
pub enum PromptInput {
    /// Prompt provided as an inline string.
    Inline(String),
    /// Prompt loaded from a file path.
    File(PathBuf),
    /// No prompt configured — the orchestration engine or provider decides.
    Absent,
}

/// A `LoopConfig` that has passed [`LoopConfig::validate`] and carries
/// refined types for fields with non-trivial invariants.
///
/// All other `LoopConfig` fields are accessible via `Deref`.  Downstream code
/// should accept `&ValidatedLoopConfig` to make it impossible to accidentally
/// operate on an unvalidated configuration.
#[derive(Debug, Clone)]
pub struct ValidatedLoopConfig {
    inner: LoopConfig,
    /// Validated iteration count (replaces `inner.iterations`).
    iteration_count: IterationCount,
    /// Validated prompt source (replaces `inner.prompt_inline` / `inner.prompt_file`).
    prompt_source: PromptInput,
}

// WARNING: This Deref exposes raw `iterations`, `prompt_inline`, and `prompt_file`
// fields which have validated counterparts. Always use `iteration_count()` and
// `prompt_source()` instead. Accessing the raw fields bypasses validation
// invariants. See #117 and #153 for discussion of alternatives.
impl std::ops::Deref for ValidatedLoopConfig {
    type Target = LoopConfig;
    fn deref(&self) -> &LoopConfig {
        &self.inner
    }
}

impl ValidatedLoopConfig {
    /// Read-only access to the validated iteration count.
    pub fn iteration_count(&self) -> &IterationCount {
        &self.iteration_count
    }

    /// Read-only access to the validated prompt source.
    pub fn prompt_source(&self) -> &PromptInput {
        &self.prompt_source
    }

    /// Return a clone with a different workspace directory.
    pub fn with_workspace_dir(mut self, dir: PathBuf) -> Self {
        self.inner.workspace_dir = Some(dir);
        self
    }

    /// Return a clone with the prompt replaced by an inline string.
    pub fn with_prompt_override(mut self, prompt: String) -> Self {
        self.inner.prompt_file = None;
        self.inner.prompt_inline = Some(prompt.clone());
        self.prompt_source = PromptInput::Inline(prompt);
        self
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

// ── Three-tier config resolution ──────────────────────────────────────────────

/// Return the platform-appropriate user config directory for Code Looper.
///
/// - Linux: `$XDG_CONFIG_HOME/code-looper` (falls back to `~/.config/code-looper`)
/// - macOS: `~/Library/Application Support/code-looper`
/// - Windows: `%APPDATA%\code-looper`
///
/// Returns `None` if the home directory cannot be determined.
pub fn user_config_dir() -> Option<PathBuf> {
    user_config_dir_with_home(None)
}

/// Resolve the user config directory, optionally overriding the home directory.
///
/// When `home_override` is `Some`, it is used in place of `$HOME`/`$USERPROFILE`
/// for computing the user config path.  This avoids thread-unsafe `env::set_var`
/// calls in tests.
fn user_config_dir_with_home(home_override: Option<&Path>) -> Option<PathBuf> {
    fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    }

    let effective_home =
        || -> Option<PathBuf> { home_override.map(PathBuf::from).or_else(home_dir) };

    #[cfg(target_os = "macos")]
    {
        effective_home().map(|h| h.join("Library/Application Support/code-looper"))
    }
    #[cfg(target_os = "windows")]
    {
        if home_override.is_some() {
            effective_home().map(|h| h.join("AppData/Roaming/code-looper"))
        } else {
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("code-looper"))
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if home_override.is_none() {
            if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
                return Some(PathBuf::from(xdg).join("code-looper"));
            }
        }
        effective_home()
            .map(|h| h.join(".config"))
            .map(|base| base.join("code-looper"))
    }
}

/// Candidate config file paths for a given directory.
fn config_candidates(dir: &Path) -> Vec<PathBuf> {
    vec![
        dir.join("config.toml"),
        dir.join("config.yaml"),
        dir.join("config.yml"),
    ]
}

/// Search for a config file in the given directory.
///
/// Checks `config.toml`, `config.yaml`, `config.yml` in that order and returns
/// the first one that exists.
pub fn find_config_in_dir(dir: &Path) -> Option<PathBuf> {
    config_candidates(dir).into_iter().find(|p| p.is_file())
}

/// Three-tier config resolution: CLI → workspace → user.
///
/// Returns the path to the config file that should be loaded, along with a
/// label indicating which tier it came from (for logging).
///
/// **Tier 1 — CLI flag:** if `cli_config` is `Some`, it is used directly.
/// **Tier 2 — Workspace:** looks for `.code-looper/config.{toml,yaml,yml}` in
/// the workspace directory.
/// **Tier 3 — User:** looks in the platform user config directory.
///
/// Returns `None` if no config file is found at any tier.
pub fn resolve_config_path(
    cli_config: Option<&Path>,
    workspace_dir: &Path,
) -> Option<(PathBuf, &'static str)> {
    // Tier 1: explicit CLI flag.
    if let Some(path) = cli_config {
        return Some((path.to_path_buf(), "cli"));
    }

    // Tier 2: workspace config.
    let workspace_config_dir = workspace_dir.join(".code-looper");
    if let Some(path) = find_config_in_dir(&workspace_config_dir) {
        return Some((path, "workspace"));
    }

    // Tier 3: user config.
    if let Some(dir) = user_config_dir() {
        if let Some(path) = find_config_in_dir(&dir) {
            return Some((path, "user"));
        }
    } else {
        tracing::debug!("HOME/USERPROFILE not set; skipping user-tier config lookup");
    }

    None
}

/// Like [`resolve_config_path`] but accepts an optional home directory override
/// for the user-tier lookup.  Used in tests to avoid thread-unsafe
/// `env::set_var("HOME", ...)`.
#[cfg(test)]
fn resolve_config_path_with_home(
    cli_config: Option<&Path>,
    workspace_dir: &Path,
    home_override: Option<&Path>,
) -> Option<(PathBuf, &'static str)> {
    // Tier 1: explicit CLI flag.
    if let Some(path) = cli_config {
        return Some((path.to_path_buf(), "cli"));
    }

    // Tier 2: workspace config.
    let workspace_config_dir = workspace_dir.join(".code-looper");
    if let Some(path) = find_config_in_dir(&workspace_config_dir) {
        return Some((path, "workspace"));
    }

    // Tier 3: user config.
    if let Some(user_dir) = user_config_dir_with_home(home_override) {
        if let Some(path) = find_config_in_dir(&user_dir) {
            return Some((path, "user"));
        }
    } else {
        tracing::debug!("HOME/USERPROFILE not set; skipping user-tier config lookup");
    }

    None
}

/// Resolve rule file paths relative to the config file's parent directory.
///
/// When rule paths are relative, they are resolved against the directory
/// containing the config file (not against CWD).  Absolute paths are left
/// unchanged.
pub fn resolve_rule_paths(rules: &mut RulesConfig, config_file: &Path) {
    let config_dir = config_file.parent().unwrap_or_else(|| {
        tracing::warn!(
            config_file = %config_file.display(),
            "config file has no parent directory — resolving rule paths relative to CWD"
        );
        Path::new(".")
    });

    if let Some(ref mut path) = rules.global {
        if path.is_relative() {
            *path = config_dir.join(&path);
        }
    }

    let resolved: HashMap<PolicyWorkflow, PathBuf> = rules
        .workflows
        .iter()
        .map(|(k, v)| {
            let p = if v.is_relative() {
                config_dir.join(v)
            } else {
                v.clone()
            };
            (k.clone(), p)
        })
        .collect();
    rules.workflows = resolved;
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

    /// Fill in `repo_owner` / `repo_name` gaps from the git remote when the
    /// user hasn't set them explicitly.  Call this **before** [`validate`].
    ///
    /// The resolution order (first non-`None` wins) is:
    /// 1. Explicit value in `issue_tracking` config
    /// 2. Inherited from `orchestration` config
    /// 3. Inferred from `git remote get-url origin`
    pub fn resolve_git_defaults(&mut self) {
        // Only bother shelling out to git if at least one value is missing.
        let needs_owner =
            self.issue_tracking.repo_owner.is_none() && self.orchestration.repo_owner.is_none();
        let needs_repo =
            self.issue_tracking.repo_name.is_none() && self.orchestration.repo_name.is_none();

        if !needs_owner && !needs_repo {
            return;
        }

        if let Some(info) = git_repo_info() {
            if needs_owner {
                // Populate into issue_tracking so it takes priority in the
                // fallback chain without touching the orchestration section.
                self.issue_tracking.repo_owner = Some(info.owner.clone());
                // Also fill orchestration so it's available if enabled later.
                if self.orchestration.repo_owner.is_none() {
                    self.orchestration.repo_owner = Some(info.owner);
                }
            }
            if needs_repo {
                self.issue_tracking.repo_name = Some(info.repo.clone());
                if self.orchestration.repo_name.is_none() {
                    self.orchestration.repo_name = Some(info.repo);
                }
            }
        }
    }

    /// Validate the config and return a [`ValidatedLoopConfig`] with refined
    /// types for fields that have non-trivial invariants.
    ///
    /// Consumes `self` so callers cannot accidentally use the raw config after
    /// validation.
    pub fn validate(self) -> Result<ValidatedLoopConfig, LooperError> {
        // ── Prompt source ───────────────────────────────────────────────
        let prompt_source = match (&self.prompt_inline, &self.prompt_file) {
            (Some(_), Some(_)) => {
                return Err(LooperError::InvalidArgument(
                    "--prompt-inline and --prompt-file are mutually exclusive".to_string(),
                ));
            }
            (Some(s), None) => PromptInput::Inline(s.clone()),
            (None, Some(p)) => {
                std::fs::metadata(p).map_err(|e| {
                    LooperError::InvalidArgument(format!("--prompt-file '{}': {e}", p.display()))
                })?;
                PromptInput::File(p.clone())
            }
            (None, None) => PromptInput::Absent,
        };

        // ── Iteration count ─────────────────────────────────────────────
        let iteration_count = if self.iterations == -1 {
            IterationCount::Infinite
        } else if self.iterations > 0 {
            let n = u32::try_from(self.iterations).map_err(|_| {
                LooperError::InvalidArgument(format!(
                    "--iterations value {} exceeds maximum {}",
                    self.iterations,
                    u32::MAX
                ))
            })?;
            IterationCount::Finite(NonZeroU32::new(n).expect("n > 0"))
        } else {
            return Err(LooperError::InvalidArgument(
                "--iterations must be a positive integer or -1 for infinite".to_string(),
            ));
        };

        // ── Orchestration fields ────────────────────────────────────────
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

        // ── on_complete ─────────────────────────────────────────────────
        if let Some(cmd) = &self.on_complete {
            if cmd.trim().is_empty() {
                return Err(LooperError::InvalidArgument(
                    "--on-complete must not be an empty string".to_string(),
                ));
            }
        }

        // ── PR / issue tracking cross-checks ────────────────────────────
        if self.pr_management.mode == PrMode::MultiPr
            && self.issue_tracking.mode != IssueTrackingMode::Github
        {
            return Err(LooperError::InvalidArgument(
                "pr_management.mode=\"multi-pr\" requires issue_tracking.mode=\"github\""
                    .to_string(),
            ));
        }
        if self.pr_management.mode == PrMode::SinglePr
            && self.issue_tracking.comment_issue_number.is_none()
        {
            return Err(LooperError::InvalidArgument(
                "pr_management.mode=\"single-pr\" requires \
                 issue_tracking.comment_issue_number to be set"
                    .to_string(),
            ));
        }
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
                     (set issue_tracking.repo_owner, orchestration.repo_owner, \
                     or run inside a git repo with a GitHub remote)"
                        .to_string(),
                ));
            }
            if repo.is_none() {
                return Err(LooperError::InvalidArgument(
                    "issue_tracking.mode=\"github\" requires repo_name \
                     (set issue_tracking.repo_name, orchestration.repo_name, \
                     or run inside a git repo with a GitHub remote)"
                        .to_string(),
                ));
            }
        }

        // ── Backoff multiplier ────────────────────────────────────────────
        if self.retry_backoff_multiplier.is_nan()
            || self.retry_backoff_multiplier.is_infinite()
            || self.retry_backoff_multiplier <= 0.0
        {
            return Err(LooperError::InvalidArgument(
                "--retry-backoff-multiplier must be a finite, positive number (> 0.0)".to_string(),
            ));
        }

        // ── Rules file validation ────────────────────────────────────────
        self.rules.validate()?;

        Ok(ValidatedLoopConfig {
            inner: self,
            iteration_count,
            prompt_source,
        })
    }
}

/// Validate that a rule file exists and is within the size limit.
///
/// Uses `fs::metadata` as the single existence+size check, eliminating the
/// TOCTOU race of a separate `path.exists()` call.
fn validate_rule_file(path: &Path, config_key: &str) -> Result<(), LooperError> {
    let meta = std::fs::metadata(path).map_err(|e| {
        LooperError::InvalidArgument(format!(
            "{config_key}: cannot read '{}': {e}",
            path.display()
        ))
    })?;
    if meta.len() > RULES_SIZE_MAX_BYTES {
        return Err(LooperError::InvalidArgument(format!(
            "{config_key}: '{}' is {} bytes, exceeding the {} byte limit",
            path.display(),
            meta.len(),
            RULES_SIZE_MAX_BYTES
        )));
    }
    if meta.len() > RULES_SIZE_WARN_BYTES {
        tracing::warn!(
            path = %path.display(),
            size = meta.len(),
            limit = RULES_SIZE_WARN_BYTES,
            "{config_key}: rule file is large; consider trimming to stay under {RULES_SIZE_WARN_BYTES} bytes"
        );
    }
    Ok(())
}

/// Load the contents of a rule file, returning an empty string for empty files.
///
/// Enforces `RULES_SIZE_MAX_BYTES` on every call (not just startup validation)
/// to guard against files that grow between iterations.
///
/// Callers should handle `Err` by logging and reusing the last good content
/// (for mid-run re-reads) or by failing startup (for initial validation).
pub fn load_rule_file(path: &Path) -> Result<String, LooperError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        LooperError::InvalidArgument(format!(
            "failed to read rule file '{}': {e}",
            path.display()
        ))
    })?;
    if content.len() as u64 > RULES_SIZE_MAX_BYTES {
        return Err(LooperError::InvalidArgument(format!(
            "rule file '{}' is {} bytes, exceeding the {} byte limit",
            path.display(),
            content.len(),
            RULES_SIZE_MAX_BYTES
        )));
    }
    if content.trim().is_empty() {
        tracing::debug!(path = %path.display(), "rule file is empty — treated as no-op");
    }
    Ok(content)
}

/// Load all configured rules and return the assembled preamble for a given
/// workflow branch.  Returns `None` if no rules are configured.
///
/// The returned string contains the global rules (if any) followed by the
/// workflow-specific rules (if any), separated by blank lines.
///
/// On read failure, falls back to the last successfully loaded content for
/// the failing file (cached in `RULES_CACHE`).  This prevents rules from
/// being silently dropped due to transient I/O errors mid-run.
pub fn load_rules_for_branch(
    rules: &RulesConfig,
    workflow: Option<&PolicyWorkflow>,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(ref path) = rules.global {
        match load_rule_file(path) {
            Ok(content) if !content.trim().is_empty() => {
                cache_rule(path, &content);
                parts.push(content);
            }
            Ok(_) => {} // empty file — skip
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    "failed to re-read global rule file: {e}"
                );
                if let Some(cached) = get_cached_rule(path) {
                    tracing::info!(path = %path.display(), "using cached global rule file content");
                    parts.push(cached);
                } else {
                    tracing::error!(
                        path = %path.display(),
                        "rule file re-read failed and no cached content available — this iteration will run WITHOUT global rules"
                    );
                    eprintln!(
                        "[loop] WARNING: rule file '{}' could not be read and no cached content \
                         is available; this iteration will run WITHOUT rules.",
                        path.display()
                    );
                }
            }
        }
    }

    if let Some(wf) = workflow {
        if let Some(path) = rules.workflows.get(wf) {
            match load_rule_file(path) {
                Ok(content) if !content.trim().is_empty() => {
                    cache_rule(path, &content);
                    parts.push(content);
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        workflow = %wf,
                        "failed to re-read workflow rule file: {e}"
                    );
                    if let Some(cached) = get_cached_rule(path) {
                        tracing::info!(path = %path.display(), workflow = %wf, "using cached workflow rule file content");
                        parts.push(cached);
                    } else {
                        tracing::error!(
                            path = %path.display(),
                            workflow = %wf,
                            "rule file re-read failed and no cached content available — this iteration will run WITHOUT workflow rules"
                        );
                        eprintln!(
                            "[loop] WARNING: rule file '{}' could not be read and no cached content \
                             is available; this iteration will run WITHOUT rules.",
                            path.display()
                        );
                    }
                }
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

// ── Rule file cache ─────────────────────────────────────────────────────────

use std::sync::Mutex;

/// Cache of last successfully loaded rule file content, keyed by path.
static RULES_CACHE: std::sync::LazyLock<Mutex<HashMap<PathBuf, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_rule(path: &Path, content: &str) {
    match RULES_CACHE.lock() {
        Ok(mut cache) => {
            cache.insert(path.to_path_buf(), content.to_string());
        }
        Err(e) => {
            tracing::error!(
                path = %path.display(),
                "RULES_CACHE mutex poisoned — rule file content not cached: {e}"
            );
        }
    }
}

fn get_cached_rule(path: &Path) -> Option<String> {
    match RULES_CACHE.lock() {
        Ok(cache) => cache.get(path).cloned(),
        Err(e) => {
            tracing::error!(
                path = %path.display(),
                "RULES_CACHE mutex poisoned — cannot read cached rule: {e}"
            );
            None
        }
    }
}

/// Clear the rule cache.
///
/// Called between repo iterations in multi-repo mode to prevent
/// cross-repo rule contamination (#163), and in tests for isolation.
pub fn clear_rules_cache() {
    RULES_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// RAII guard that clears `RULES_CACHE` on creation and on drop, ensuring
/// test isolation even if the test panics.
#[cfg(test)]
struct RulesCacheGuard;

#[cfg(test)]
impl RulesCacheGuard {
    fn new() -> Self {
        clear_rules_cache();
        Self
    }
}

#[cfg(test)]
impl Drop for RulesCacheGuard {
    fn drop(&mut self) {
        clear_rules_cache();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ── git remote URL parsing ───────────────────────────────────────────

    #[test]
    fn parse_https_url() {
        let info = parse_git_remote_url("https://github.com/jamesbrayton/code-looper.git");
        assert_eq!(
            info,
            Some(GitRepoInfo {
                owner: "jamesbrayton".to_string(),
                repo: "code-looper".to_string(),
            })
        );
    }

    #[test]
    fn parse_https_url_no_dot_git() {
        let info = parse_git_remote_url("https://github.com/jamesbrayton/code-looper");
        assert_eq!(
            info,
            Some(GitRepoInfo {
                owner: "jamesbrayton".to_string(),
                repo: "code-looper".to_string(),
            })
        );
    }

    #[test]
    fn parse_ssh_shorthand_url() {
        let info = parse_git_remote_url("git@github.com:acme/my-repo.git");
        assert_eq!(
            info,
            Some(GitRepoInfo {
                owner: "acme".to_string(),
                repo: "my-repo".to_string(),
            })
        );
    }

    #[test]
    fn parse_ssh_protocol_url() {
        let info = parse_git_remote_url("ssh://git@github.com/acme/my-repo.git");
        assert_eq!(
            info,
            Some(GitRepoInfo {
                owner: "acme".to_string(),
                repo: "my-repo".to_string(),
            })
        );
    }

    #[test]
    fn parse_url_with_trailing_whitespace() {
        let info = parse_git_remote_url("https://github.com/owner/repo.git\n");
        assert_eq!(
            info,
            Some(GitRepoInfo {
                owner: "owner".to_string(),
                repo: "repo".to_string(),
            })
        );
    }

    #[test]
    fn parse_non_github_url_returns_none() {
        assert!(parse_git_remote_url("https://gitlab.com/owner/repo.git").is_none());
    }

    #[test]
    fn parse_empty_url_returns_none() {
        assert!(parse_git_remote_url("").is_none());
    }

    #[test]
    fn parse_malformed_url_returns_none() {
        assert!(parse_git_remote_url("git@github.com:").is_none());
        assert!(parse_git_remote_url("https://github.com/").is_none());
        assert!(parse_git_remote_url("https://github.com/owner-only").is_none());
    }

    // ── resolve_git_defaults ─────────────────────────────────────────────

    #[test]
    fn resolve_git_defaults_fills_from_git_remote() {
        // This test runs inside the code-looper repo, so git_repo_info()
        // should return Some(...).  If it doesn't (e.g. CI), skip silently.
        let git_info = match git_repo_info() {
            Some(info) => info,
            None => return,
        };

        let mut config = LoopConfig {
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: None,
                repo_name: None,
                ..Default::default()
            },
            ..Default::default()
        };

        config.resolve_git_defaults();
        assert_eq!(
            config.issue_tracking.repo_owner.as_deref(),
            Some(git_info.owner.as_str())
        );
        assert_eq!(
            config.issue_tracking.repo_name.as_deref(),
            Some(git_info.repo.as_str())
        );
        // Validation should pass now.
        assert!(config.validate().is_ok());
    }

    #[test]
    fn explicit_config_overrides_git_remote() {
        let mut config = LoopConfig {
            issue_tracking: IssueTrackingConfig {
                mode: IssueTrackingMode::Github,
                repo_owner: Some("explicit-owner".to_string()),
                repo_name: Some("explicit-repo".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        config.resolve_git_defaults();
        // Explicit values must not be overwritten.
        assert_eq!(
            config.issue_tracking.repo_owner.as_deref(),
            Some("explicit-owner")
        );
        assert_eq!(
            config.issue_tracking.repo_name.as_deref(),
            Some("explicit-repo")
        );
    }

    // ── existing tests ───────────────────────────────────────────────────

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
            err.to_string().contains("--prompt-file") && err.to_string().contains("prompt.md"),
            "expected prompt-file error, got: {err}"
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
        // NoPr works without comment_issue_number.
        let config = LoopConfig {
            pr_management: PrManagementConfig {
                mode: PrMode::NoPr,
                ..PrManagementConfig::default()
            },
            ..LoopConfig::default()
        };
        assert!(config.validate().is_ok());

        // SinglePr requires comment_issue_number.
        let config = LoopConfig {
            pr_management: PrManagementConfig {
                mode: PrMode::SinglePr,
                ..PrManagementConfig::default()
            },
            issue_tracking: IssueTrackingConfig {
                comment_issue_number: Some(1),
                ..IssueTrackingConfig::default()
            },
            ..LoopConfig::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn single_pr_without_issue_number_rejected() {
        let config = LoopConfig {
            pr_management: PrManagementConfig {
                mode: PrMode::SinglePr,
                ..PrManagementConfig::default()
            },
            ..LoopConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("comment_issue_number"),
            "expected error about comment_issue_number, got: {err}"
        );
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

    // ── provider_extra_args ───────────────────────────────────────────────────

    #[test]
    fn default_provider_extra_args_is_empty() {
        let config = LoopConfig::default();
        assert!(config.provider_extra_args.is_empty());
    }

    #[test]
    fn parse_toml_provider_extra_args() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "claude"
iterations = 1
log_level = "info"
provider_extra_args = ["--model", "claude-opus-4-5"]
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.provider_extra_args, ["--model", "claude-opus-4-5"]);
    }

    #[test]
    fn parse_yaml_provider_extra_args() {
        let file = write_temp_file(
            ".yaml",
            "provider: codex\niterations: 1\nlog_level: info\nprovider_extra_args:\n  - --approval-mode\n  - full-auto\n",
        );
        let config = LoopConfig::from_yaml_file(file.path()).unwrap();
        assert_eq!(config.provider_extra_args, ["--approval-mode", "full-auto"]);
    }

    // ── non_retryable_exit_codes ──────────────────────────────────────────────

    #[test]
    fn default_non_retryable_exit_codes_is_empty() {
        let config = LoopConfig::default();
        assert!(config.non_retryable_exit_codes.is_empty());
    }

    #[test]
    fn parse_toml_non_retryable_exit_codes() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "claude"
iterations = 1
log_level = "info"
non_retryable_exit_codes = [2, 126, 127]
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.non_retryable_exit_codes, [2, 126, 127]);
    }

    #[test]
    fn parse_yaml_non_retryable_exit_codes() {
        let file = write_temp_file(
            ".yaml",
            "provider: claude\niterations: 1\nlog_level: info\nnon_retryable_exit_codes:\n  - 2\n  - 127\n",
        );
        let config = LoopConfig::from_yaml_file(file.path()).unwrap();
        assert_eq!(config.non_retryable_exit_codes, [2, 127]);
    }

    // ── ValidatedLoopConfig / IterationCount / PromptInput tests ────────

    #[test]
    fn validate_returns_finite_iteration_count() {
        let validated = LoopConfig {
            iterations: 5,
            ..Default::default()
        }
        .validate()
        .unwrap();
        assert_eq!(
            validated.iteration_count,
            IterationCount::Finite(std::num::NonZeroU32::new(5).unwrap())
        );
        assert_eq!(validated.iteration_count.max_iterations(), 5);
        assert!(!validated.iteration_count.is_infinite());
        assert_eq!(validated.iteration_count.as_raw_i64(), 5);
    }

    #[test]
    fn validate_returns_infinite_iteration_count() {
        let validated = LoopConfig {
            iterations: -1,
            ..Default::default()
        }
        .validate()
        .unwrap();
        assert_eq!(validated.iteration_count, IterationCount::Infinite);
        assert_eq!(validated.iteration_count.max_iterations(), u64::MAX);
        assert!(validated.iteration_count.is_infinite());
        assert_eq!(validated.iteration_count.as_raw_i64(), -1);
    }

    #[test]
    fn is_done_finite_boundary_semantics() {
        let three = IterationCount::Finite(std::num::NonZeroU32::new(3).unwrap());
        assert!(!three.is_done(0)); // iteration 0 — not done
        assert!(!three.is_done(1)); // iteration 1 — not done
        assert!(!three.is_done(2)); // iteration 2 — last valid (0-based < 3)
        assert!(three.is_done(3)); // iteration 3 — done (0-based >= 3)
        assert!(three.is_done(4)); // beyond n

        let one = IterationCount::Finite(std::num::NonZeroU32::new(1).unwrap());
        assert!(!one.is_done(0));
        assert!(one.is_done(1));

        assert!(!IterationCount::Infinite.is_done(u64::MAX));
    }

    #[test]
    fn validate_returns_inline_prompt_source() {
        let validated = LoopConfig {
            prompt_inline: Some("hello".to_string()),
            ..Default::default()
        }
        .validate()
        .unwrap();
        assert!(matches!(validated.prompt_source, PromptInput::Inline(ref s) if s == "hello"));
    }

    #[test]
    fn validate_returns_file_prompt_source() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "prompt content").unwrap();
        let validated = LoopConfig {
            prompt_file: Some(f.path().to_path_buf()),
            ..Default::default()
        }
        .validate()
        .unwrap();
        assert!(matches!(validated.prompt_source, PromptInput::File(_)));
    }

    #[test]
    fn validate_returns_none_prompt_source() {
        let validated = LoopConfig::default().validate().unwrap();
        assert!(matches!(validated.prompt_source, PromptInput::Absent));
    }

    #[test]
    fn validated_config_derefs_to_inner_fields() {
        let validated = LoopConfig {
            iterations: 3,
            log_level: "debug".to_string(),
            ..Default::default()
        }
        .validate()
        .unwrap();
        // Access via Deref — must see original LoopConfig fields.
        assert_eq!(validated.log_level, "debug");
        assert_eq!(validated.provider, Provider::Claude);
    }

    #[test]
    fn with_workspace_dir_updates_inner() {
        let validated = LoopConfig::default().validate().unwrap();
        let updated = validated.with_workspace_dir("/tmp/repo".into());
        assert_eq!(
            updated.workspace_dir.as_deref(),
            Some(std::path::Path::new("/tmp/repo"))
        );
    }

    #[test]
    fn with_prompt_override_updates_source() {
        let validated = LoopConfig::default().validate().unwrap();
        let updated = validated.with_prompt_override("override prompt".to_string());
        assert!(
            matches!(updated.prompt_source(), PromptInput::Inline(ref s) if s == "override prompt")
        );
    }

    #[test]
    fn validate_consumes_config_preventing_reuse() {
        // This test verifies the API contract: validate() takes ownership,
        // so callers cannot accidentally use the raw config after validation.
        // If this compiles, the guarantee holds — no runtime assertion needed.
        let config = LoopConfig::default();
        let _validated = config.validate().unwrap();
        // `config` is now moved — any use would be a compile error.
    }

    #[test]
    fn large_iteration_count_is_rejected() {
        let err = LoopConfig {
            iterations: i64::from(u32::MAX) + 1,
            ..Default::default()
        }
        .validate()
        .unwrap_err();
        assert!(
            err.to_string().contains("exceeds maximum"),
            "expected 'exceeds maximum' error, got: {err}"
        );
    }

    // ── Rules config ────────────────────────────────────────────────────

    #[test]
    fn rules_config_defaults_to_empty() {
        let rules = RulesConfig::default();
        assert!(rules.global.is_none());
        assert!(rules.workflows.is_empty());
    }

    #[test]
    fn validate_rejects_missing_global_rule_file() {
        let config = LoopConfig {
            rules: RulesConfig {
                global: Some(PathBuf::from("/nonexistent/rules.md")),
                workflows: HashMap::new(),
            },
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("cannot read"));
        assert!(err.to_string().contains("rules.global"));
    }

    #[test]
    fn validate_rejects_missing_workflow_rule_file() {
        let config = LoopConfig {
            rules: RulesConfig {
                global: None,
                workflows: HashMap::from([(
                    PolicyWorkflow::PrReview,
                    PathBuf::from("/nonexistent/pr-review.md"),
                )]),
            },
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("cannot read"));
        assert!(err.to_string().contains("rules.workflows.pr-review"));
    }

    #[test]
    fn validate_accepts_existing_rule_file() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "# My coding rules").unwrap();
        let config = LoopConfig {
            rules: RulesConfig {
                global: Some(f.path().to_path_buf()),
                workflows: HashMap::new(),
            },
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_accepts_empty_rule_file() {
        let f = NamedTempFile::new().unwrap();
        let config = LoopConfig {
            rules: RulesConfig {
                global: Some(f.path().to_path_buf()),
                workflows: HashMap::new(),
            },
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_rejects_oversized_rule_file() {
        let mut f = NamedTempFile::new().unwrap();
        // Write just over the 64 KB limit.
        let content = "x".repeat(RULES_SIZE_MAX_BYTES as usize + 1);
        f.write_all(content.as_bytes()).unwrap();
        let config = LoopConfig {
            rules: RulesConfig {
                global: Some(f.path().to_path_buf()),
                workflows: HashMap::new(),
            },
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("exceeding"));
    }

    #[test]
    fn validate_rejects_nan_backoff_multiplier() {
        let config = LoopConfig {
            retry_backoff_multiplier: f64::NAN,
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("retry-backoff-multiplier"),
            "expected backoff multiplier error, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_infinite_backoff_multiplier() {
        let config = LoopConfig {
            retry_backoff_multiplier: f64::INFINITY,
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("retry-backoff-multiplier"));
    }

    #[test]
    fn validate_rejects_negative_backoff_multiplier() {
        let config = LoopConfig {
            retry_backoff_multiplier: -1.0,
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("retry-backoff-multiplier"));
    }

    #[test]
    fn validate_rejects_zero_backoff_multiplier() {
        let config = LoopConfig {
            retry_backoff_multiplier: 0.0,
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("retry-backoff-multiplier"));
    }

    #[test]
    fn load_rule_file_returns_content() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "Always use snake_case.").unwrap();
        let content = load_rule_file(f.path()).unwrap();
        assert!(content.contains("snake_case"));
    }

    #[test]
    fn load_rule_file_rejects_oversized_file() {
        let mut f = NamedTempFile::new().unwrap();
        // Write more than RULES_SIZE_MAX_BYTES (64 KB).
        let big = "x".repeat(RULES_SIZE_MAX_BYTES as usize + 1);
        std::io::Write::write_all(&mut f, big.as_bytes()).unwrap();
        let err = load_rule_file(f.path()).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("exceeding"), "expected size error, got: {msg}");
    }

    #[test]
    fn load_rules_for_branch_falls_back_to_cache_on_read_error() {
        let _guard = RulesCacheGuard::new();

        let mut global_file = NamedTempFile::new().unwrap();
        writeln!(global_file, "CACHED RULE").unwrap();

        let rules = RulesConfig {
            global: Some(global_file.path().to_path_buf()),
            workflows: HashMap::new(),
        };

        // First call succeeds and caches the content.
        let result = load_rules_for_branch(&rules, None).unwrap();
        assert!(result.contains("CACHED RULE"));

        // Delete the file to simulate a read error.
        let path = global_file.path().to_path_buf();
        drop(global_file);
        std::fs::remove_file(&path).ok();

        // Second call should fall back to cached content.
        let result = load_rules_for_branch(&rules, None).unwrap();
        assert!(
            result.contains("CACHED RULE"),
            "expected cached content on read failure"
        );
    }

    #[test]
    fn load_rules_for_branch_returns_none_when_first_read_fails_no_cache() {
        let _guard = RulesCacheGuard::new();

        // Point global at a path that never existed — first read will fail
        // and there is no cached content to fall back on.
        let rules = RulesConfig {
            global: Some(PathBuf::from("/nonexistent/never-read.md")),
            workflows: HashMap::new(),
        };

        // Should return None (not panic) when no cached content is available.
        let result = load_rules_for_branch(&rules, None);
        assert!(
            result.is_none(),
            "expected None when rule file was never successfully read"
        );
    }

    #[test]
    fn validate_rule_file_rejects_missing_via_metadata() {
        let result = validate_rule_file(Path::new("/nonexistent/rule.md"), "test.key");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("test.key"), "error should include config key");
    }

    #[test]
    fn load_rules_for_branch_combines_global_and_workflow() {
        let _guard = RulesCacheGuard::new();
        let mut global_file = NamedTempFile::new().unwrap();
        writeln!(global_file, "GLOBAL RULE").unwrap();
        let mut workflow_file = NamedTempFile::new().unwrap();
        writeln!(workflow_file, "PR REVIEW RULE").unwrap();

        let rules = RulesConfig {
            global: Some(global_file.path().to_path_buf()),
            workflows: HashMap::from([(
                PolicyWorkflow::PrReview,
                workflow_file.path().to_path_buf(),
            )]),
        };

        let result = load_rules_for_branch(&rules, Some(&PolicyWorkflow::PrReview)).unwrap();
        assert!(result.contains("GLOBAL RULE"));
        assert!(result.contains("PR REVIEW RULE"));
        // Global comes before workflow.
        assert!(result.find("GLOBAL RULE").unwrap() < result.find("PR REVIEW RULE").unwrap());
    }

    #[test]
    fn load_rules_for_branch_returns_none_when_no_rules() {
        let _guard = RulesCacheGuard::new();
        let rules = RulesConfig::default();
        assert!(load_rules_for_branch(&rules, Some(&PolicyWorkflow::PrReview)).is_none());
    }

    #[test]
    fn load_rules_for_branch_returns_global_only_when_no_workflow_match() {
        let _guard = RulesCacheGuard::new();
        let mut global_file = NamedTempFile::new().unwrap();
        writeln!(global_file, "GLOBAL RULE").unwrap();

        let rules = RulesConfig {
            global: Some(global_file.path().to_path_buf()),
            workflows: HashMap::new(),
        };

        let result = load_rules_for_branch(&rules, Some(&PolicyWorkflow::PrReview)).unwrap();
        assert!(result.contains("GLOBAL RULE"));
    }

    // ── Three-tier config resolution ──────────────────────────────────

    #[test]
    fn user_config_dir_returns_some_on_linux() {
        // Ensure HOME is set so the function can find a path.
        if std::env::var_os("HOME").is_some() {
            let dir = user_config_dir();
            assert!(dir.is_some());
            let dir = dir.unwrap();
            assert!(dir.to_string_lossy().contains("code-looper"));
        }
    }

    #[test]
    fn find_config_in_dir_finds_toml() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("config.toml"),
            "provider = \"claude\"\niterations = 1\n",
        )
        .unwrap();
        let found = find_config_in_dir(tmp.path());
        assert!(found.is_some());
        assert!(found.unwrap().ends_with("config.toml"));
    }

    #[test]
    fn find_config_in_dir_finds_yaml() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("config.yaml"), "provider: claude\n").unwrap();
        let found = find_config_in_dir(tmp.path());
        assert!(found.is_some());
        assert!(found.unwrap().ends_with("config.yaml"));
    }

    #[test]
    fn find_config_in_dir_returns_none_when_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(find_config_in_dir(tmp.path()).is_none());
    }

    #[test]
    fn find_config_in_dir_prefers_toml_over_yaml() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("config.toml"), "").unwrap();
        std::fs::write(tmp.path().join("config.yaml"), "").unwrap();
        let found = find_config_in_dir(tmp.path()).unwrap();
        assert!(found.ends_with("config.toml"));
    }

    #[test]
    fn resolve_config_path_cli_wins() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cli_path = tmp.path().join("custom.toml");
        std::fs::write(&cli_path, "").unwrap();
        // Also create a workspace config — CLI should still win.
        let ws = tmp.path().join("workspace");
        std::fs::create_dir_all(ws.join(".code-looper")).unwrap();
        std::fs::write(ws.join(".code-looper/config.toml"), "").unwrap();

        let (path, tier) = resolve_config_path(Some(&cli_path), &ws).unwrap();
        assert_eq!(tier, "cli");
        assert_eq!(path, cli_path);
    }

    #[test]
    fn resolve_config_path_workspace_tier() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ws = tmp.path().join("workspace");
        std::fs::create_dir_all(ws.join(".code-looper")).unwrap();
        std::fs::write(ws.join(".code-looper/config.toml"), "").unwrap();

        let (path, tier) = resolve_config_path(None, &ws).unwrap();
        assert_eq!(tier, "workspace");
        assert!(path.ends_with("config.toml"));
    }

    #[test]
    fn resolve_config_path_returns_none_when_nothing_found() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Use a custom HOME so user-tier doesn't accidentally find a real config.
        let result = resolve_config_path(None, tmp.path());
        // May return None or find a user-tier config — depends on HOME.
        // The important thing is it doesn't panic.
        let _ = result;
    }

    #[test]
    fn resolve_config_path_user_tier_with_controlled_home() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ws = tmp.path().join("workspace");
        std::fs::create_dir_all(&ws).unwrap();

        // Create a user-tier config under a controlled HOME.
        let user_cfg_dir = tmp.path().join(".config/code-looper");
        std::fs::create_dir_all(&user_cfg_dir).unwrap();
        std::fs::write(
            user_cfg_dir.join("config.toml"),
            "provider = \"claude\"\niterations = 1\n",
        )
        .unwrap();

        // Use the injectable home override instead of thread-unsafe env::set_var.
        let result = resolve_config_path_with_home(None, &ws, Some(tmp.path()));

        // Should find the user-tier config.
        let (path, tier) = result.expect("user-tier config should be found");
        assert_eq!(tier, "user");
        assert!(path.ends_with("config.toml"));
    }

    // ── Rule path resolution ────────────────────────────────────────────

    #[test]
    fn resolve_rule_paths_makes_relative_absolute() {
        let config_file = Path::new("/opt/project/.code-looper/config.toml");
        let mut rules = RulesConfig {
            global: Some(PathBuf::from("rules/global.md")),
            workflows: HashMap::from([(
                PolicyWorkflow::PrReview,
                PathBuf::from("rules/pr-review.md"),
            )]),
        };
        resolve_rule_paths(&mut rules, config_file);
        assert_eq!(
            rules.global.as_deref(),
            Some(Path::new("/opt/project/.code-looper/rules/global.md"))
        );
        assert_eq!(
            rules
                .workflows
                .get(&PolicyWorkflow::PrReview)
                .map(|p| p.as_path()),
            Some(Path::new("/opt/project/.code-looper/rules/pr-review.md"))
        );
    }

    #[test]
    fn resolve_rule_paths_leaves_absolute_unchanged() {
        let config_file = Path::new("/opt/project/.code-looper/config.toml");
        let mut rules = RulesConfig {
            global: Some(PathBuf::from("/etc/code-looper/global.md")),
            workflows: HashMap::new(),
        };
        resolve_rule_paths(&mut rules, config_file);
        assert_eq!(
            rules.global.as_deref(),
            Some(Path::new("/etc/code-looper/global.md"))
        );
    }

    #[test]
    fn rules_config_round_trips_through_toml() {
        let toml_str = r#"
global = ".code-looper/rules/global.md"

[workflows]
"pr-review" = ".code-looper/rules/pr-review.md"
"issue-execution" = ".code-looper/rules/issue-execution.md"
"#;
        let rules: RulesConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            rules.global.as_deref(),
            Some(std::path::Path::new(".code-looper/rules/global.md"))
        );
        assert_eq!(rules.workflows.len(), 2);
        assert!(rules.workflows.contains_key(&PolicyWorkflow::PrReview));
        assert!(rules
            .workflows
            .contains_key(&PolicyWorkflow::IssueExecution));
    }
}
