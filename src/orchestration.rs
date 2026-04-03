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
                 and update the issue via MCP GitHub tools when done."
            }
            WorkflowBranch::BacklogDiscovery => {
                "Explore the repository codebase and identify areas for improvement: \
                 missing tests, documentation gaps, refactoring opportunities, or potential \
                 new features. Create GitHub issues for the most impactful opportunities \
                 using the MCP GitHub tools."
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

/// Policy engine: resolves context and selects the appropriate workflow branch.
pub struct PolicyEngine {
    resolver: Box<dyn ContextResolver>,
}

impl PolicyEngine {
    pub fn new(resolver: Box<dyn ContextResolver>) -> Self {
        Self { resolver }
    }

    /// Evaluate repository context and select the highest-priority workflow branch.
    ///
    /// Decision order:
    /// 1. Open PRs → PrReview
    /// 2. Open issues (no PRs) → IssueExecution
    /// 3. Nothing open → BacklogDiscovery
    pub fn select_branch(&self) -> Result<(WorkflowBranch, RepoContext), LooperError> {
        let ctx = self.resolver.resolve()?;
        let branch = if ctx.has_open_prs() {
            WorkflowBranch::PrReview
        } else if ctx.has_open_issues() {
            WorkflowBranch::IssueExecution
        } else {
            WorkflowBranch::BacklogDiscovery
        };
        info!(
            open_prs = ctx.open_pr_count,
            open_issues = ctx.open_issue_count,
            selected_branch = %branch,
            "Policy engine selected workflow branch"
        );
        Ok((branch, ctx))
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
        let (branch, _) = engine.select_branch().unwrap();
        assert_eq!(branch, WorkflowBranch::PrReview);
    }

    #[test]
    fn selects_issue_execution_when_no_prs_but_issues() {
        let engine = PolicyEngine::new(Box::new(StubContextResolver {
            context: RepoContext { open_pr_count: 0, open_issue_count: 3 },
        }));
        let (branch, _) = engine.select_branch().unwrap();
        assert_eq!(branch, WorkflowBranch::IssueExecution);
    }

    #[test]
    fn selects_backlog_discovery_when_nothing_open() {
        let engine = PolicyEngine::new(Box::new(StubContextResolver {
            context: RepoContext { open_pr_count: 0, open_issue_count: 0 },
        }));
        let (branch, _) = engine.select_branch().unwrap();
        assert_eq!(branch, WorkflowBranch::BacklogDiscovery);
    }

    #[test]
    fn prs_take_precedence_over_issues() {
        let engine = PolicyEngine::new(Box::new(StubContextResolver {
            context: RepoContext { open_pr_count: 1, open_issue_count: 10 },
        }));
        let (branch, ctx) = engine.select_branch().unwrap();
        assert_eq!(branch, WorkflowBranch::PrReview);
        assert_eq!(ctx.open_issue_count, 10);
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
}
