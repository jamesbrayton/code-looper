use crate::config::{default_policy_rules, PolicyCondition, PolicyRule, PolicyWorkflow};
use crate::error::LooperError;
use tracing::info;

/// Summary of relevant repository state used for policy decisions.
#[derive(Debug, Clone, Default)]
pub struct RepoContext {
    pub open_pr_count: u32,
    pub open_issue_count: u32,
}

impl RepoContext {
    pub fn has_open_prs(&self) -> bool {
        self.open_pr_count > 0
    }

    pub fn has_open_issues(&self) -> bool {
        self.open_issue_count > 0
    }
}

/// Milestone-aware repository context for the lifecycle engine.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct MilestoneContext {
    /// Open issues in the current milestone with `ready-for-dev` label.
    pub milestone_ready_for_dev: u32,
    /// Open issues in the current milestone with no state label.
    pub milestone_ungroomed: u32,
    /// Total open issues in the current milestone.
    pub milestone_open_issues: u32,
    /// Total open PRs in the repository.
    pub open_pr_count: u32,
    /// Issues with `ready-for-dev` label and no milestone assigned.
    pub backlog_ready_for_dev: u32,
}

impl MilestoneContext {
    #[allow(dead_code)]
    pub fn is_milestone_complete(&self) -> bool {
        self.milestone_open_issues == 0 && self.open_pr_count == 0
    }
}

/// Abstraction for fetching milestone-aware repository context.
#[allow(dead_code)]
pub trait MilestoneContextResolver: Send + Sync {
    fn resolve(&self) -> Result<MilestoneContext, LooperError>;
}

/// The five named lifecycles.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum Lifecycle {
    /// Work items with `ready-for-dev` in the current milestone.
    Execution,
    /// Open PRs exist (and no ready-for-dev items).
    PrReview,
    /// Milestone is fully closed — time to release.
    Release,
    /// Ungroomed issues in the milestone need scoping.
    Grooming,
    /// Ready-for-dev items with no milestone need milestone assignment.
    Planning,
}

impl std::fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lifecycle::Execution => write!(f, "execution"),
            Lifecycle::PrReview => write!(f, "pr-review"),
            Lifecycle::Release => write!(f, "release"),
            Lifecycle::Grooming => write!(f, "grooming"),
            Lifecycle::Planning => write!(f, "planning"),
        }
    }
}

/// Result of lifecycle selection.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LifecycleSelection {
    pub lifecycle: Lifecycle,
    pub context: MilestoneContext,
}

/// Lifecycle engine: queries milestone-aware context and selects the active lifecycle.
#[allow(dead_code)]
pub struct LifecycleEngine {
    resolver: Box<dyn MilestoneContextResolver>,
    mode: crate::config::OrchestrationMode,
}

impl LifecycleEngine {
    #[allow(dead_code)]
    pub fn new(
        resolver: Box<dyn MilestoneContextResolver>,
        mode: crate::config::OrchestrationMode,
    ) -> Self {
        Self { resolver, mode }
    }

    #[allow(dead_code)]
    pub fn select(&self) -> Result<LifecycleSelection, LooperError> {
        use crate::config::OrchestrationMode;
        let ctx = self.resolver.resolve()?;

        // Priority order: execution → pr-review → release → grooming → planning
        let lifecycle = if ctx.milestone_ready_for_dev > 0 {
            Some(Lifecycle::Execution)
        } else if ctx.open_pr_count > 0 {
            Some(Lifecycle::PrReview)
        } else if ctx.is_milestone_complete() {
            Some(Lifecycle::Release)
        } else if ctx.milestone_ungroomed > 0
            && matches!(self.mode, OrchestrationMode::Assisted | OrchestrationMode::Autonomous)
        {
            Some(Lifecycle::Grooming)
        } else if ctx.backlog_ready_for_dev > 0
            && matches!(self.mode, OrchestrationMode::Autonomous)
        {
            Some(Lifecycle::Planning)
        } else {
            None
        };

        match lifecycle {
            Some(lc) => {
                tracing::info!(
                    lifecycle = %lc,
                    milestone_ready_for_dev = ctx.milestone_ready_for_dev,
                    open_pr_count = ctx.open_pr_count,
                    "Lifecycle engine selected lifecycle"
                );
                Ok(LifecycleSelection { lifecycle: lc, context: ctx })
            }
            None => Err(LooperError::InvalidArgument(format!(
                "No lifecycle applies to current state in `{}` mode. \
                 Ensure the current milestone has issues with `ready-for-dev` label, \
                 or upgrade to a higher autonomy mode.",
                match self.mode {
                    OrchestrationMode::ExecutionOnly => "execution-only",
                    OrchestrationMode::Assisted => "assisted",
                    OrchestrationMode::Autonomous => "autonomous",
                }
            ))),
        }
    }
}

/// Workflow branch selected by the policy engine.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowBranch {
    /// There are open PRs that need review.
    PrReview,
    /// There are open issues to work on.
    IssueExecution,
    /// No open PRs or issues; discover backlog work.
    BacklogDiscovery,
}

impl WorkflowBranch {
    /// Return the default prompt payload for this workflow branch.
    pub fn default_prompt(&self) -> &'static str {
        match self {
            WorkflowBranch::PrReview => {
                "Review open pull requests in this repository. For each open PR, \
                 check the diff, verify tests pass, and leave a constructive review comment \
                 using the MCP GitHub tools. Do not merge without explicit approval."
            }
            WorkflowBranch::IssueExecution => {
                "Work on open GitHub issues in this repository. Pick the highest-priority \
                 unassigned issue, understand the requirements, implement the changes, \
                 and update the issue via MCP GitHub tools when done. \
                 \n\n**Issue lifecycle rules:**\
                 \n- Comment at meaningful milestones (plan finalised, first pass done, \
                 tests added, blocker found).\
                 \n- If you discover work that is out of scope for the current issue, create \
                 a new GitHub issue (via MCP) with a clear title, body, and one of the \
                 standard labels: `bug`, `enhancement`, `tech-debt`, or \
                 `discovered-during-loop`. Post a cross-reference comment on both issues.\
                 \n- When the issue checklist is fully checked and changes are committed, \
                 close the issue with a short summary comment explaining what was done."
            }
            WorkflowBranch::BacklogDiscovery => {
                "Explore the repository codebase and identify areas for improvement: \
                 missing tests, documentation gaps, refactoring opportunities, or potential \
                 new features. Create GitHub issues for the most impactful opportunities \
                 using the MCP GitHub tools. Use one of the standard labels for each new \
                 issue: `bug`, `enhancement`, `tech-debt`, or `discovered-during-loop`."
            }
        }
    }

    /// Convert to the corresponding [`PolicyWorkflow`] variant.
    pub fn to_policy_workflow(&self) -> PolicyWorkflow {
        match self {
            WorkflowBranch::PrReview => PolicyWorkflow::PrReview,
            WorkflowBranch::IssueExecution => PolicyWorkflow::IssueExecution,
            WorkflowBranch::BacklogDiscovery => PolicyWorkflow::BacklogDiscovery,
        }
    }
}

impl std::fmt::Display for WorkflowBranch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkflowBranch::PrReview => write!(f, "pr-review"),
            WorkflowBranch::IssueExecution => write!(f, "issue-execution"),
            WorkflowBranch::BacklogDiscovery => write!(f, "backlog-discovery"),
        }
    }
}

/// Abstraction for fetching repository context.
///
/// Implementations can shell out to `gh`, call an MCP server, or return
/// stubbed data for testing.
pub trait ContextResolver: Send + Sync {
    fn resolve(&self) -> Result<RepoContext, LooperError>;
}

/// Result of the policy engine's branch selection.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BranchSelection {
    /// The workflow branch to execute.
    pub branch: WorkflowBranch,
    /// Prompt override from the matching rule, if any.
    pub prompt_override: Option<String>,
    /// Repository context snapshot used to make the decision.
    pub context: RepoContext,
}

/// Policy engine: resolves context and selects the appropriate workflow branch.
///
/// Rules are evaluated in order; the first rule whose condition matches wins.
/// Construct with [`PolicyEngine::new`] (uses the default three-rule chain) or
/// [`PolicyEngine::with_rules`] (uses caller-supplied rules).
pub struct PolicyEngine {
    resolver: Box<dyn ContextResolver>,
    rules: Vec<PolicyRule>,
}

impl PolicyEngine {
    /// Create a policy engine with the default three-rule chain.
    #[allow(dead_code)]
    pub fn new(resolver: Box<dyn ContextResolver>) -> Self {
        Self {
            resolver,
            rules: default_policy_rules(),
        }
    }

    /// Create a policy engine with a caller-supplied rule list.
    ///
    /// An empty `rules` list is accepted; the engine will return an error at
    /// runtime if no rule matches (since there is no `Always` fallback).
    pub fn with_rules(resolver: Box<dyn ContextResolver>, rules: Vec<PolicyRule>) -> Self {
        Self { resolver, rules }
    }

    /// Evaluate repository context against the configured rule list and return
    /// the first matching branch selection.
    ///
    /// Returns an error if the resolver fails or if no rule matches.
    pub fn select_branch(&self) -> Result<BranchSelection, LooperError> {
        let ctx = self.resolver.resolve()?;

        for rule in &self.rules {
            let matches = match rule.condition {
                PolicyCondition::HasOpenPrs => ctx.has_open_prs(),
                PolicyCondition::HasOpenIssues => ctx.has_open_issues(),
                PolicyCondition::Always => true,
            };

            if matches {
                let branch = workflow_to_branch(&rule.workflow);
                info!(
                    open_prs = ctx.open_pr_count,
                    open_issues = ctx.open_issue_count,
                    selected_branch = %branch,
                    condition = %rule.condition,
                    "Policy engine selected workflow branch"
                );
                return Ok(BranchSelection {
                    branch,
                    prompt_override: rule.prompt_override.clone(),
                    context: ctx,
                });
            }
        }

        Err(LooperError::InvalidArgument(
            "No policy rule matched the current repository context. \
             Add an `always` fallback rule to your [[orchestration.policies]] config."
                .to_string(),
        ))
    }
}

fn workflow_to_branch(workflow: &PolicyWorkflow) -> WorkflowBranch {
    match workflow {
        PolicyWorkflow::PrReview => WorkflowBranch::PrReview,
        PolicyWorkflow::IssueExecution => WorkflowBranch::IssueExecution,
        PolicyWorkflow::BacklogDiscovery => WorkflowBranch::BacklogDiscovery,
    }
}

// ── GitHub CLI context resolver ───────────────────────────────────────────────

/// Fetches repository context by shelling out to `gh` (GitHub CLI).
pub struct GhCliContextResolver {
    pub owner: String,
    pub repo: String,
}

impl ContextResolver for GhCliContextResolver {
    fn resolve(&self) -> Result<RepoContext, LooperError> {
        let pr_count = count_gh_items(&self.owner, &self.repo, "pr")?;
        let issue_count = count_gh_items(&self.owner, &self.repo, "issue")?;
        Ok(RepoContext {
            open_pr_count: pr_count,
            open_issue_count: issue_count,
        })
    }
}

/// Shell out to `gh <kind> list` and count the returned JSON objects.
fn count_gh_items(owner: &str, repo: &str, kind: &str) -> Result<u32, LooperError> {
    use std::process::Command;

    let repo_slug = format!("{owner}/{repo}");
    let output = Command::new("gh")
        .args([
            kind, "list", "--repo", &repo_slug, "--state", "open", "--json", "number",
        ])
        .output()
        .map_err(|e| LooperError::ProviderSpawn {
            binary: "gh".to_string(),
            source: e,
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LooperError::InvalidArgument(format!(
            "gh {kind} list failed for {repo_slug}: {stderr}"
        )));
    }

    // `gh ... --json number` returns a JSON array like
    // `[{"number":1},{"number":2}]`.  Parse it with serde_json instead of
    // counting `{` characters — the old approach broke as soon as anyone
    // added a field that contained a brace or pretty-printed the output
    // (see #79).  `serde_json` is already a workspace dependency.
    let text = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = serde_json::from_str(&text).map_err(|e| {
        LooperError::InvalidArgument(format!(
            "failed to parse `gh {kind} list` output as JSON array: {e}"
        ))
    })?;
    Ok(items.len() as u32)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod tests {
    use super::*;

    pub struct StubContextResolver {
        pub context: RepoContext,
    }

    impl ContextResolver for StubContextResolver {
        fn resolve(&self) -> Result<RepoContext, LooperError> {
            Ok(self.context.clone())
        }
    }

    #[test]
    fn selects_pr_review_when_prs_open() {
        let engine = PolicyEngine::new(Box::new(StubContextResolver {
            context: RepoContext {
                open_pr_count: 2,
                open_issue_count: 5,
            },
        }));
        let sel = engine.select_branch().unwrap();
        assert_eq!(sel.branch, WorkflowBranch::PrReview);
    }

    #[test]
    fn selects_issue_execution_when_no_prs_but_issues() {
        let engine = PolicyEngine::new(Box::new(StubContextResolver {
            context: RepoContext {
                open_pr_count: 0,
                open_issue_count: 3,
            },
        }));
        let sel = engine.select_branch().unwrap();
        assert_eq!(sel.branch, WorkflowBranch::IssueExecution);
    }

    #[test]
    fn selects_backlog_discovery_when_nothing_open() {
        let engine = PolicyEngine::new(Box::new(StubContextResolver {
            context: RepoContext {
                open_pr_count: 0,
                open_issue_count: 0,
            },
        }));
        let sel = engine.select_branch().unwrap();
        assert_eq!(sel.branch, WorkflowBranch::BacklogDiscovery);
    }

    #[test]
    fn prs_take_precedence_over_issues() {
        let engine = PolicyEngine::new(Box::new(StubContextResolver {
            context: RepoContext {
                open_pr_count: 1,
                open_issue_count: 10,
            },
        }));
        let sel = engine.select_branch().unwrap();
        assert_eq!(sel.branch, WorkflowBranch::PrReview);
        assert_eq!(sel.context.open_issue_count, 10);
    }

    #[test]
    fn workflow_branch_display() {
        assert_eq!(WorkflowBranch::PrReview.to_string(), "pr-review");
        assert_eq!(
            WorkflowBranch::IssueExecution.to_string(),
            "issue-execution"
        );
        assert_eq!(
            WorkflowBranch::BacklogDiscovery.to_string(),
            "backlog-discovery"
        );
    }

    #[test]
    fn repo_context_helpers() {
        let ctx = RepoContext {
            open_pr_count: 1,
            open_issue_count: 0,
        };
        assert!(ctx.has_open_prs());
        assert!(!ctx.has_open_issues());

        let ctx2 = RepoContext {
            open_pr_count: 0,
            open_issue_count: 2,
        };
        assert!(!ctx2.has_open_prs());
        assert!(ctx2.has_open_issues());
    }

    #[test]
    fn default_prompts_are_nonempty() {
        assert!(!WorkflowBranch::PrReview.default_prompt().is_empty());
        assert!(!WorkflowBranch::IssueExecution.default_prompt().is_empty());
        assert!(!WorkflowBranch::BacklogDiscovery.default_prompt().is_empty());
    }

    // ── Pluggable policy rules ────────────────────────────────────────────────

    #[test]
    fn custom_rules_override_default_chain() {
        use crate::config::{PolicyCondition, PolicyRule, PolicyWorkflow};
        // Single rule: always → issue-execution (reversed from default).
        let rules = vec![PolicyRule {
            condition: PolicyCondition::Always,
            workflow: PolicyWorkflow::IssueExecution,
            prompt_override: None,
        }];
        let engine = PolicyEngine::with_rules(
            Box::new(StubContextResolver {
                context: RepoContext {
                    open_pr_count: 5,
                    open_issue_count: 0,
                },
            }),
            rules,
        );
        // Even though there are open PRs, our single rule maps Always → IssueExecution.
        let sel = engine.select_branch().unwrap();
        assert_eq!(sel.branch, WorkflowBranch::IssueExecution);
    }

    #[test]
    fn prompt_override_is_returned_when_set() {
        use crate::config::{PolicyCondition, PolicyRule, PolicyWorkflow};
        let rules = vec![PolicyRule {
            condition: PolicyCondition::Always,
            workflow: PolicyWorkflow::PrReview,
            prompt_override: Some("Custom PR review prompt.".to_string()),
        }];
        let engine = PolicyEngine::with_rules(
            Box::new(StubContextResolver {
                context: RepoContext {
                    open_pr_count: 1,
                    open_issue_count: 0,
                },
            }),
            rules,
        );
        let sel = engine.select_branch().unwrap();
        assert_eq!(
            sel.prompt_override.as_deref(),
            Some("Custom PR review prompt.")
        );
    }

    #[test]
    fn no_prompt_override_when_none_set() {
        use crate::config::{PolicyCondition, PolicyRule, PolicyWorkflow};
        let rules = vec![PolicyRule {
            condition: PolicyCondition::Always,
            workflow: PolicyWorkflow::BacklogDiscovery,
            prompt_override: None,
        }];
        let engine = PolicyEngine::with_rules(
            Box::new(StubContextResolver {
                context: RepoContext {
                    open_pr_count: 0,
                    open_issue_count: 0,
                },
            }),
            rules,
        );
        let sel = engine.select_branch().unwrap();
        assert!(sel.prompt_override.is_none());
    }

    #[test]
    fn first_matching_rule_wins() {
        use crate::config::{PolicyCondition, PolicyRule, PolicyWorkflow};
        let rules = vec![
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
        ];
        let engine = PolicyEngine::with_rules(
            Box::new(StubContextResolver {
                context: RepoContext {
                    open_pr_count: 0,
                    open_issue_count: 2,
                },
            }),
            rules,
        );
        let sel = engine.select_branch().unwrap();
        // HasOpenIssues matches first, so IssueExecution wins over Always fallback.
        assert_eq!(sel.branch, WorkflowBranch::IssueExecution);
    }

    #[test]
    fn no_matching_rule_returns_error() {
        use crate::config::{PolicyCondition, PolicyRule, PolicyWorkflow};
        // Rules require open PRs, but context has none.
        let rules = vec![PolicyRule {
            condition: PolicyCondition::HasOpenPrs,
            workflow: PolicyWorkflow::PrReview,
            prompt_override: None,
        }];
        let engine = PolicyEngine::with_rules(
            Box::new(StubContextResolver {
                context: RepoContext {
                    open_pr_count: 0,
                    open_issue_count: 0,
                },
            }),
            rules,
        );
        assert!(engine.select_branch().is_err());
    }

    #[test]
    fn policy_condition_display() {
        use crate::config::PolicyCondition;
        assert_eq!(PolicyCondition::HasOpenPrs.to_string(), "has_open_prs");
        assert_eq!(
            PolicyCondition::HasOpenIssues.to_string(),
            "has_open_issues"
        );
        assert_eq!(PolicyCondition::Always.to_string(), "always");
    }

    pub struct StubLifecycleResolver {
        pub ctx: MilestoneContext,
    }

    impl MilestoneContextResolver for StubLifecycleResolver {
        fn resolve(&self) -> Result<MilestoneContext, LooperError> {
            Ok(self.ctx.clone())
        }
    }

    #[test]
    fn selects_execution_when_ready_for_dev_in_milestone() {
        use crate::config::OrchestrationMode;
        let engine = LifecycleEngine::new(
            Box::new(StubLifecycleResolver {
                ctx: MilestoneContext { milestone_ready_for_dev: 2, ..Default::default() },
            }),
            OrchestrationMode::ExecutionOnly,
        );
        let sel = engine.select().unwrap();
        assert_eq!(sel.lifecycle, Lifecycle::Execution);
    }

    #[test]
    fn selects_pr_review_when_prs_open_and_no_ready_for_dev() {
        use crate::config::OrchestrationMode;
        let engine = LifecycleEngine::new(
            Box::new(StubLifecycleResolver {
                ctx: MilestoneContext { open_pr_count: 1, ..Default::default() },
            }),
            OrchestrationMode::ExecutionOnly,
        );
        let sel = engine.select().unwrap();
        assert_eq!(sel.lifecycle, Lifecycle::PrReview);
    }

    #[test]
    fn selects_release_when_milestone_complete() {
        use crate::config::OrchestrationMode;
        // All fields default to 0, so is_milestone_complete() returns true
        let engine = LifecycleEngine::new(
            Box::new(StubLifecycleResolver {
                ctx: MilestoneContext::default(),
            }),
            OrchestrationMode::ExecutionOnly,
        );
        let sel = engine.select().unwrap();
        assert_eq!(sel.lifecycle, Lifecycle::Release);
    }

    #[test]
    fn selects_grooming_when_ungroomed_and_mode_is_assisted() {
        use crate::config::OrchestrationMode;
        let engine = LifecycleEngine::new(
            Box::new(StubLifecycleResolver {
                // milestone_open_issues > 0 so is_milestone_complete() returns false
                ctx: MilestoneContext { milestone_ungroomed: 3, milestone_open_issues: 3, ..Default::default() },
            }),
            OrchestrationMode::Assisted,
        );
        let sel = engine.select().unwrap();
        assert_eq!(sel.lifecycle, Lifecycle::Grooming);
    }

    #[test]
    fn execution_only_mode_blocks_grooming() {
        use crate::config::OrchestrationMode;
        // milestone_open_issues > 0 prevents is_milestone_complete() from firing;
        // no ready-for-dev, no PRs → grooming would fire in Assisted mode but is
        // blocked in ExecutionOnly → engine returns an error.
        let engine = LifecycleEngine::new(
            Box::new(StubLifecycleResolver {
                ctx: MilestoneContext {
                    milestone_ungroomed: 3,
                    milestone_open_issues: 3,
                    ..Default::default()
                },
            }),
            OrchestrationMode::ExecutionOnly,
        );
        assert!(engine.select().is_err());
    }

    #[test]
    fn selects_planning_when_backlog_ready_and_autonomous() {
        use crate::config::OrchestrationMode;
        let engine = LifecycleEngine::new(
            Box::new(StubLifecycleResolver {
                // milestone_open_issues > 0 so Release doesn't fire; no ready-for-dev, no PRs
                ctx: MilestoneContext { milestone_open_issues: 1, backlog_ready_for_dev: 2, ..Default::default() },
            }),
            OrchestrationMode::Autonomous,
        );
        let sel = engine.select().unwrap();
        assert_eq!(sel.lifecycle, Lifecycle::Planning);
    }

    #[test]
    fn lifecycle_display() {
        assert_eq!(Lifecycle::Execution.to_string(), "execution");
        assert_eq!(Lifecycle::PrReview.to_string(), "pr-review");
        assert_eq!(Lifecycle::Release.to_string(), "release");
        assert_eq!(Lifecycle::Grooming.to_string(), "grooming");
        assert_eq!(Lifecycle::Planning.to_string(), "planning");
    }
}
