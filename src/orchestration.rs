use crate::config::{PolicyCondition, PolicyRule, PolicyWorkflow, default_policy_rules};
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
        Self { resolver, rules: default_policy_rules() }
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
        .args([kind, "list", "--repo", &repo_slug, "--state", "open", "--json", "number"])
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

    // `gh ... --json number` returns a JSON array like `[{"number":1},{"number":2}]`.
    // Count top-level `{` characters to get the item count without a JSON parser dep.
    let text = String::from_utf8_lossy(&output.stdout);
    let count = text.chars().filter(|&c| c == '{').count() as u32;
    Ok(count)
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
            context: RepoContext { open_pr_count: 2, open_issue_count: 5 },
        }));
        let sel = engine.select_branch().unwrap();
        assert_eq!(sel.branch, WorkflowBranch::PrReview);
    }

    #[test]
    fn selects_issue_execution_when_no_prs_but_issues() {
        let engine = PolicyEngine::new(Box::new(StubContextResolver {
            context: RepoContext { open_pr_count: 0, open_issue_count: 3 },
        }));
        let sel = engine.select_branch().unwrap();
        assert_eq!(sel.branch, WorkflowBranch::IssueExecution);
    }

    #[test]
    fn selects_backlog_discovery_when_nothing_open() {
        let engine = PolicyEngine::new(Box::new(StubContextResolver {
            context: RepoContext { open_pr_count: 0, open_issue_count: 0 },
        }));
        let sel = engine.select_branch().unwrap();
        assert_eq!(sel.branch, WorkflowBranch::BacklogDiscovery);
    }

    #[test]
    fn prs_take_precedence_over_issues() {
        let engine = PolicyEngine::new(Box::new(StubContextResolver {
            context: RepoContext { open_pr_count: 1, open_issue_count: 10 },
        }));
        let sel = engine.select_branch().unwrap();
        assert_eq!(sel.branch, WorkflowBranch::PrReview);
        assert_eq!(sel.context.open_issue_count, 10);
    }

    #[test]
    fn workflow_branch_display() {
        assert_eq!(WorkflowBranch::PrReview.to_string(), "pr-review");
        assert_eq!(WorkflowBranch::IssueExecution.to_string(), "issue-execution");
        assert_eq!(WorkflowBranch::BacklogDiscovery.to_string(), "backlog-discovery");
    }

    #[test]
    fn repo_context_helpers() {
        let ctx = RepoContext { open_pr_count: 1, open_issue_count: 0 };
        assert!(ctx.has_open_prs());
        assert!(!ctx.has_open_issues());

        let ctx2 = RepoContext { open_pr_count: 0, open_issue_count: 2 };
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
                context: RepoContext { open_pr_count: 5, open_issue_count: 0 },
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
                context: RepoContext { open_pr_count: 1, open_issue_count: 0 },
            }),
            rules,
        );
        let sel = engine.select_branch().unwrap();
        assert_eq!(sel.prompt_override.as_deref(), Some("Custom PR review prompt."));
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
                context: RepoContext { open_pr_count: 0, open_issue_count: 0 },
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
                context: RepoContext { open_pr_count: 0, open_issue_count: 2 },
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
                context: RepoContext { open_pr_count: 0, open_issue_count: 0 },
            }),
            rules,
        );
        assert!(engine.select_branch().is_err());
    }

    #[test]
    fn policy_condition_display() {
        use crate::config::PolicyCondition;
        assert_eq!(PolicyCondition::HasOpenPrs.to_string(), "has_open_prs");
        assert_eq!(PolicyCondition::HasOpenIssues.to_string(), "has_open_issues");
        assert_eq!(PolicyCondition::Always.to_string(), "always");
    }
}
