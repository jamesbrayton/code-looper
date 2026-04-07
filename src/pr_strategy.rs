use crate::config::{PrManagementConfig, PrMode};

/// The plan produced by a `PrStrategy` before each iteration.
///
/// The loop engine passes this to the orchestration policy engine so the
/// prompt can reflect the active PR strategy.
#[derive(Debug, Clone, PartialEq)]
pub struct IterationPlan {
    /// Human-readable description of what the strategy intends to do this
    /// iteration (used in structured logs and prompt preambles).
    pub description: String,
    /// The PR mode that produced this plan.
    pub mode: PrMode,
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
        }
    }
}

/// Multi-PR strategy: triage open PRs first; start new feature branch work
/// only when no PR can be advanced.
pub struct MultiPrStrategy {
    config: PrManagementConfig,
}

impl MultiPrStrategy {
    pub fn new(config: PrManagementConfig) -> Self {
        Self { config }
    }
}

impl PrStrategy for MultiPrStrategy {
    fn plan_iteration(&self, iteration: u64) -> IterationPlan {
        tracing::debug!(
            iteration,
            mode = %self.config.mode,
            base_branch = %self.config.base_branch,
            require_human_review = self.config.require_human_review,
            "MultiPrStrategy: triage open PRs first, then issue work"
        );
        IterationPlan {
            description: format!(
                "multi-pr: triage open PRs first, then open new feature branch for issue \
                 work (base: {}, human review required: {})",
                self.config.base_branch, self.config.require_human_review
            ),
            mode: PrMode::MultiPr,
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

    #[test]
    fn multi_pr_strategy_plan_contains_mode() {
        let strategy = MultiPrStrategy::new(config_with_mode(PrMode::MultiPr));
        let plan = strategy.plan_iteration(1);
        assert_eq!(plan.mode, PrMode::MultiPr);
        assert!(plan.description.contains("multi-pr"));
    }

    #[test]
    fn multi_pr_strategy_mentions_triage() {
        let strategy = MultiPrStrategy::new(config_with_mode(PrMode::MultiPr));
        let plan = strategy.plan_iteration(2);
        assert!(plan.description.contains("triage"));
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

    #[test]
    fn build_strategy_multi_pr() {
        let strategy = build_strategy(config_with_mode(PrMode::MultiPr));
        let plan = strategy.plan_iteration(1);
        assert_eq!(plan.mode, PrMode::MultiPr);
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
