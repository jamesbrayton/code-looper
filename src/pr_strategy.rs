use crate::config::{PrManagementConfig, PrMode};
use crate::pr_manager::{PrTriage, PrLifecycleTriage, TriageAction, build_pr_triage};

/// The plan produced by a `PrStrategy` before each iteration.
///
/// The loop engine passes this to the orchestration policy engine so the
/// prompt can reflect the active PR strategy.
#[derive(Debug, Clone)]
pub struct IterationPlan {
    /// Human-readable description of what the strategy intends to do this
    /// iteration (used in structured logs and prompt preambles).
    pub description: String,
    /// The PR mode that produced this plan.
    pub mode: PrMode,
    /// When set, the loop engine uses this prompt instead of the normal
    /// resolved prompt.  Used by multi-PR triage to direct the agent toward
    /// a specific PR action.
    pub prompt_override: Option<String>,
    /// The triage action that produced this plan (present for multi-PR mode).
    pub triage_action: Option<TriageAction>,
}

/// Trait implemented by all PR strategy variants.
///
/// Each strategy is constructed once at loop startup from the resolved
/// `PrManagementConfig` and called once per iteration to produce an
/// `IterationPlan`.  The loop engine passes the plan to the orchestration
/// policy engine; the actual provider invocation is unchanged.
pub trait PrStrategy: Send + Sync {
    /// Produce an `IterationPlan` for the upcoming iteration.
    ///
    /// `iteration` is 1-based.
    fn plan_iteration(&self, iteration: u64) -> IterationPlan;
}

// ── Strategy implementations ──────────────────────────────────────────────────

/// No-PR strategy: commit and push to a feature branch; never open a PR.
pub struct NoPrStrategy {
    config: PrManagementConfig,
}

impl NoPrStrategy {
    pub fn new(config: PrManagementConfig) -> Self {
        Self { config }
    }
}

impl PrStrategy for NoPrStrategy {
    fn plan_iteration(&self, iteration: u64) -> IterationPlan {
        tracing::debug!(
            iteration,
            mode = %self.config.mode,
            base_branch = %self.config.base_branch,
            "NoPrStrategy: committing to feature branch only"
        );
        IterationPlan {
            description: format!(
                "no-pr: commit work to feature branch (base: {})",
                self.config.base_branch
            ),
            mode: PrMode::NoPr,
            prompt_override: None,
            triage_action: None,
        }
    }
}

/// Single-PR strategy: open one PR when work is shippable; keep pushing to it.
pub struct SinglePrStrategy {
    config: PrManagementConfig,
}

impl SinglePrStrategy {
    pub fn new(config: PrManagementConfig) -> Self {
        Self { config }
    }
}

impl PrStrategy for SinglePrStrategy {
    fn plan_iteration(&self, iteration: u64) -> IterationPlan {
        tracing::debug!(
            iteration,
            mode = %self.config.mode,
            base_branch = %self.config.base_branch,
            require_human_review = self.config.require_human_review,
            "SinglePrStrategy: work on feature branch; open PR when shippable"
        );
        IterationPlan {
            description: format!(
                "single-pr: work on feature branch; open PR into {} when shippable \
                 (human review required: {})",
                self.config.base_branch, self.config.require_human_review
            ),
            mode: PrMode::SinglePr,
            prompt_override: None,
            triage_action: None,
        }
    }
}

/// Multi-PR strategy: triage open PRs first; start new feature branch work
/// only when no PR can be advanced.
pub struct MultiPrStrategy<L: PrLifecycleTriage + 'static> {
    config: PrManagementConfig,
    triage: PrTriage<L>,
}

impl MultiPrStrategy<crate::pr_manager::GhPrLifecycle> {
    /// Build a production `MultiPrStrategy` backed by the `gh` CLI.
    pub fn new(config: PrManagementConfig) -> Self {
        let triage = build_pr_triage(config.clone());
        Self { config, triage }
    }
}

impl<L: PrLifecycleTriage + 'static> MultiPrStrategy<L> {
    /// Build a `MultiPrStrategy` with a custom triage lifecycle (for testing).
    pub fn with_triage(config: PrManagementConfig, triage: PrTriage<L>) -> Self {
        Self { config, triage }
    }
}

impl<L: PrLifecycleTriage + 'static> PrStrategy for MultiPrStrategy<L> {
    fn plan_iteration(&self, iteration: u64) -> IterationPlan {
        tracing::debug!(
            iteration,
            mode = %self.config.mode,
            base_branch = %self.config.base_branch,
            require_human_review = self.config.require_human_review,
            "MultiPrStrategy: running PR triage step"
        );

        let action = self.triage.select_action();

        let (description, prompt_override) = match &action {
            TriageAction::FixChecks { pr, prompt } => (
                format!("multi-pr: fix CI checks on PR #{} ({})", pr.number, pr.title),
                Some(prompt.clone()),
            ),
            TriageAction::AddressReviewFeedback { pr, prompt } => (
                format!(
                    "multi-pr: address review feedback on PR #{} ({})",
                    pr.number, pr.title
                ),
                Some(prompt.clone()),
            ),
            TriageAction::Merge { pr } => (
                format!("multi-pr: merging PR #{} ({})", pr.number, pr.title),
                None,
            ),
            TriageAction::BlockedOnHumanReview { pr } => (
                format!(
                    "multi-pr: PR #{} ready but blocked on human review — falling through",
                    pr.number
                ),
                None,
            ),
            TriageAction::NoActionablePr => (
                format!(
                    "multi-pr: no actionable open PR — proceed with issue work (base: {})",
                    self.config.base_branch
                ),
                None,
            ),
        };

        IterationPlan {
            description,
            mode: PrMode::MultiPr,
            prompt_override,
            triage_action: Some(action),
        }
    }
}

// ── Constructor ───────────────────────────────────────────────────────────────

/// Build the concrete `PrStrategy` for the given config.
pub fn build_strategy(config: PrManagementConfig) -> Box<dyn PrStrategy> {
    match config.mode {
        PrMode::NoPr => Box::new(NoPrStrategy::new(config)),
        PrMode::SinglePr => Box::new(SinglePrStrategy::new(config)),
        PrMode::MultiPr => Box::new(MultiPrStrategy::new(config)),
    }
}

/// Build a `MultiPrStrategy` with a custom triage lifecycle (for testing).
pub fn build_multi_pr_strategy_with_triage<L: PrLifecycleTriage + 'static>(
    config: PrManagementConfig,
    triage: PrTriage<L>,
) -> Box<dyn PrStrategy> {
    Box::new(MultiPrStrategy::with_triage(config, triage))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PrManagementConfig;

    fn default_config() -> PrManagementConfig {
        PrManagementConfig::default()
    }

    fn config_with_mode(mode: PrMode) -> PrManagementConfig {
        PrManagementConfig { mode, ..default_config() }
    }

    // ── NoPrStrategy ─────────────────────────────────────────────────────────

    #[test]
    fn no_pr_strategy_plan_contains_mode() {
        let strategy = NoPrStrategy::new(config_with_mode(PrMode::NoPr));
        let plan = strategy.plan_iteration(1);
        assert_eq!(plan.mode, PrMode::NoPr);
        assert!(plan.description.contains("no-pr"));
    }

    #[test]
    fn no_pr_strategy_plan_references_base_branch() {
        let mut cfg = config_with_mode(PrMode::NoPr);
        cfg.base_branch = "develop".to_string();
        let strategy = NoPrStrategy::new(cfg);
        let plan = strategy.plan_iteration(3);
        assert!(plan.description.contains("develop"));
    }

    // ── SinglePrStrategy ─────────────────────────────────────────────────────

    #[test]
    fn single_pr_strategy_plan_contains_mode() {
        let strategy = SinglePrStrategy::new(config_with_mode(PrMode::SinglePr));
        let plan = strategy.plan_iteration(1);
        assert_eq!(plan.mode, PrMode::SinglePr);
        assert!(plan.description.contains("single-pr"));
    }

    #[test]
    fn single_pr_strategy_reflects_human_review_flag() {
        let mut cfg = config_with_mode(PrMode::SinglePr);
        cfg.require_human_review = false;
        let strategy = SinglePrStrategy::new(cfg);
        let plan = strategy.plan_iteration(1);
        assert!(plan.description.contains("false"));
    }

    // ── MultiPrStrategy ──────────────────────────────────────────────────────

    fn mock_triage(config: PrManagementConfig) -> PrTriage<crate::pr_manager::MockPrLifecycleTriage> {
        PrTriage::new(config, crate::pr_manager::MockPrLifecycleTriage::new())
    }

    #[test]
    fn multi_pr_strategy_plan_contains_mode() {
        let cfg = config_with_mode(PrMode::MultiPr);
        let triage = mock_triage(cfg.clone());
        let strategy = MultiPrStrategy::with_triage(cfg, triage);
        let plan = strategy.plan_iteration(1);
        assert_eq!(plan.mode, PrMode::MultiPr);
        assert!(plan.description.contains("multi-pr"));
    }

    #[test]
    fn multi_pr_strategy_no_actionable_pr_falls_through() {
        // Empty mock → select_action returns NoActionablePr.
        let cfg = config_with_mode(PrMode::MultiPr);
        let triage = mock_triage(cfg.clone());
        let strategy = MultiPrStrategy::with_triage(cfg, triage);
        let plan = strategy.plan_iteration(1);
        assert_eq!(plan.mode, PrMode::MultiPr);
        assert!(plan.prompt_override.is_none());
        // Description should mention issue work fall-through.
        assert!(plan.description.contains("issue work") || plan.description.contains("no actionable"));
    }

    #[test]
    fn multi_pr_strategy_checks_failing_returns_prompt_override() {
        use crate::pr_manager::{MockPrLifecycleTriage, PrInfo, PrTriage, PrTriageState, PrWithState};
        let cfg = config_with_mode(PrMode::MultiPr);
        let pr = PrInfo { number: 7, url: "https://example.com/pull/7".into(), title: "Fix foo".into() };
        let mut mock = MockPrLifecycleTriage::new();
        mock.open_prs = vec![pr.clone()];
        mock.states.insert(7, PrWithState {
            pr,
            state: PrTriageState::ChecksFailing,
            created_at: "2026-01-01T00:00:00Z".into(),
        });
        let triage = PrTriage::new(cfg.clone(), mock);
        let strategy = MultiPrStrategy::with_triage(cfg, triage);
        let plan = strategy.plan_iteration(1);
        assert!(plan.prompt_override.is_some());
        assert!(plan.prompt_override.unwrap().contains("CI checks"));
    }

    #[test]
    fn multi_pr_strategy_changes_requested_returns_prompt_override() {
        use crate::pr_manager::{MockPrLifecycleTriage, PrInfo, PrTriage, PrTriageState, PrWithState};
        let cfg = config_with_mode(PrMode::MultiPr);
        let pr = PrInfo { number: 8, url: "https://example.com/pull/8".into(), title: "Add bar".into() };
        let mut mock = MockPrLifecycleTriage::new();
        mock.open_prs = vec![pr.clone()];
        mock.states.insert(8, PrWithState {
            pr,
            state: PrTriageState::ChangesRequested,
            created_at: "2026-01-01T00:00:00Z".into(),
        });
        let triage = PrTriage::new(cfg.clone(), mock);
        let strategy = MultiPrStrategy::with_triage(cfg, triage);
        let plan = strategy.plan_iteration(1);
        assert!(plan.prompt_override.is_some());
        assert!(plan.prompt_override.unwrap().contains("review comment"));
    }

    #[test]
    fn multi_pr_strategy_ready_to_merge_human_review_required() {
        use crate::pr_manager::{MockPrLifecycleTriage, PrInfo, PrTriage, PrTriageState, PrWithState};
        let mut cfg = config_with_mode(PrMode::MultiPr);
        cfg.require_human_review = true;
        let pr = PrInfo { number: 9, url: "https://example.com/pull/9".into(), title: "Ship it".into() };
        let mut mock = MockPrLifecycleTriage::new();
        mock.open_prs = vec![pr.clone()];
        mock.states.insert(9, PrWithState {
            pr,
            state: PrTriageState::ReadyToMerge,
            created_at: "2026-01-01T00:00:00Z".into(),
        });
        let triage = PrTriage::new(cfg.clone(), mock);
        let strategy = MultiPrStrategy::with_triage(cfg, triage);
        let plan = strategy.plan_iteration(1);
        // Blocked on human review → no prompt override, falls through
        assert!(plan.prompt_override.is_none());
        assert!(plan.description.contains("human review"));
    }

    #[test]
    fn multi_pr_strategy_skipped_pr_falls_through() {
        use crate::pr_manager::{MockPrLifecycleTriage, PrInfo, PrTriage, PrTriageState, PrWithState};
        let cfg = config_with_mode(PrMode::MultiPr);
        let pr = PrInfo { number: 10, url: "https://example.com/pull/10".into(), title: "WIP".into() };
        let mut mock = MockPrLifecycleTriage::new();
        mock.open_prs = vec![pr.clone()];
        mock.states.insert(10, PrWithState {
            pr,
            state: PrTriageState::Skipped { reason: "wip".into() },
            created_at: "2026-01-01T00:00:00Z".into(),
        });
        let triage = PrTriage::new(cfg.clone(), mock);
        let strategy = MultiPrStrategy::with_triage(cfg, triage);
        let plan = strategy.plan_iteration(1);
        // All PRs skipped → no prompt override
        assert!(plan.prompt_override.is_none());
    }

    // ── build_strategy ───────────────────────────────────────────────────────

    #[test]
    fn build_strategy_no_pr() {
        let strategy = build_strategy(config_with_mode(PrMode::NoPr));
        let plan = strategy.plan_iteration(1);
        assert_eq!(plan.mode, PrMode::NoPr);
    }

    #[test]
    fn build_strategy_single_pr() {
        let strategy = build_strategy(config_with_mode(PrMode::SinglePr));
        let plan = strategy.plan_iteration(1);
        assert_eq!(plan.mode, PrMode::SinglePr);
    }

    // ── Iteration number propagation ─────────────────────────────────────────

    #[test]
    fn strategy_called_with_correct_iteration_number() {
        let strategy = build_strategy(config_with_mode(PrMode::NoPr));
        // Just verifying plan_iteration is callable with any u64 without panic.
        for n in [1u64, 5, 100] {
            let plan = strategy.plan_iteration(n);
            assert_eq!(plan.mode, PrMode::NoPr);
        }
    }
}
