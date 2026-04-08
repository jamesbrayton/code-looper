//! Policy guard — agent-prompt enforcement of the MCP-only GitHub mutation
//! path.
//!
//! Per ADR-001 (revised), MCP-only is scoped to **agent prompts**:
//! [`PolicyGuard::augment_prompt`] prepends a preamble telling the provider
//! to use only MCP server tools for GitHub writes.  The engine itself is
//! explicitly permitted to shell out to `gh` for its own bookkeeping (PR
//! creation, issue comments, merge-cleanup branch ops) — see the module
//! docs in `pr_manager.rs` and `issue_tracker.rs`.
//!
//! This module does **not** enforce anything at the process level; enforcement
//! is prompt-level and trusts the agent to honour the preamble.  A stronger
//! sandbox would require removing `gh` from the provider's `PATH`, which is
//! out of scope for this ADR.

/// Configuration for unsafe bypass overrides.
///
/// All overrides are opt-in and disabled by default.  Enabling any of them
/// weakens the safety guarantees described in ADR-001.
#[derive(Debug, Clone, Default)]
pub struct UnsafeOverrides {
    /// When `true`, the agent-prompt MCP-only preamble is suppressed, so
    /// providers are free to use `gh` CLI or raw REST calls for GitHub
    /// writes.  Engine-side `gh` usage is unaffected by this flag — it is
    /// already permitted by ADR-001.
    pub allow_direct_github: bool,
}

/// Validation error emitted by the policy guard.
#[derive(Debug, Clone)]
pub struct PolicyViolation {
    /// Short identifier for the violated rule.
    pub rule: String,
    /// Human-readable description of the violation.
    pub message: String,
    /// How to resolve or work around the violation.
    pub remediation: String,
}

impl std::fmt::Display for PolicyViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[policy:{}] {}\n  → Remediation: {}",
            self.rule, self.message, self.remediation
        )
    }
}

/// Agent-prompt enforcement of the MCP-only GitHub mutation path.
///
/// The guard has a single real responsibility: prepend a preamble to every
/// provider prompt telling the agent to use MCP server tools for any GitHub
/// writes (see [`Self::augment_prompt`]).  There is no process-level or
/// config-level validation — enforcement is prompt-level only, and the
/// engine itself is explicitly permitted to use `gh` directly per the
/// revised ADR-001.
pub struct PolicyGuard {
    overrides: UnsafeOverrides,
}

impl PolicyGuard {
    pub fn new(overrides: UnsafeOverrides) -> Self {
        Self { overrides }
    }

    /// Startup sanity check for orchestration config.
    ///
    /// Returns a list of violations (empty = all clear).  At present this is
    /// intentionally a no-op: the guard has no config invariants to check
    /// beyond what the config loader already validates, and the actual
    /// MCP-only enforcement happens at the prompt level via
    /// [`Self::augment_prompt`].  The method is kept so callers have a single
    /// startup hook to attach future validations to.
    pub fn check_startup(&self, _orchestration_enabled: bool) -> Vec<PolicyViolation> {
        Vec::new()
    }

    /// Augment a provider prompt with an MCP-use preamble.
    ///
    /// When `allow_direct_github` is `false` (the default), a preamble is
    /// prepended that instructs the agent to use only MCP server tools for any
    /// GitHub operations (issues, PRs, branches, etc.).
    ///
    /// If the prompt is empty the preamble alone is returned so providers
    /// always receive the guard constraint.
    pub fn augment_prompt(&self, prompt: &str) -> String {
        if self.overrides.allow_direct_github {
            return prompt.to_string();
        }

        let preamble = MCP_ONLY_PREAMBLE;
        if prompt.is_empty() {
            preamble.to_string()
        } else {
            format!("{preamble}\n\n{prompt}")
        }
    }
}

/// Preamble injected into every provider prompt to enforce the MCP-only policy.
const MCP_ONLY_PREAMBLE: &str = "\
IMPORTANT — GitHub operations policy:
All GitHub mutations (creating or updating issues, pull request reviews, \
comments, branch operations, and merges) MUST be performed exclusively \
through the configured GitHub MCP server tools. \
Direct use of `gh` CLI commands or raw GitHub REST API calls for write \
operations is not permitted in this session.";

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_guard() -> PolicyGuard {
        PolicyGuard::new(UnsafeOverrides::default())
    }

    fn permissive_guard() -> PolicyGuard {
        PolicyGuard::new(UnsafeOverrides {
            allow_direct_github: true,
        })
    }

    // ── check_startup ────────────────────────────────────────────────

    #[test]
    fn no_violations_when_orchestration_disabled() {
        let violations = default_guard().check_startup(false);
        assert!(violations.is_empty());
    }

    #[test]
    fn no_violations_when_orchestration_enabled_default_policy() {
        // Orchestration with default (safe) policy: reads via gh are allowed;
        // writes are constrained via prompt augmentation, not a hard violation.
        let violations = default_guard().check_startup(true);
        assert!(violations.is_empty());
    }

    #[test]
    fn no_violations_with_allow_direct_github_override() {
        let violations = permissive_guard().check_startup(true);
        assert!(violations.is_empty());
    }

    // ── augment_prompt ────────────────────────────────────────────────────────

    #[test]
    fn augments_prompt_with_mcp_preamble_by_default() {
        let guard = default_guard();
        let result = guard.augment_prompt("do some work");
        assert!(result.contains(MCP_ONLY_PREAMBLE));
        assert!(result.contains("do some work"));
    }

    #[test]
    fn preamble_appears_before_original_prompt() {
        let guard = default_guard();
        let result = guard.augment_prompt("do some work");
        let preamble_pos = result.find(MCP_ONLY_PREAMBLE).unwrap();
        let prompt_pos = result.find("do some work").unwrap();
        assert!(preamble_pos < prompt_pos);
    }

    #[test]
    fn empty_prompt_returns_only_preamble() {
        let guard = default_guard();
        let result = guard.augment_prompt("");
        assert_eq!(result, MCP_ONLY_PREAMBLE);
    }

    #[test]
    fn skips_augmentation_when_allow_direct_github() {
        let guard = permissive_guard();
        let result = guard.augment_prompt("do some work");
        assert_eq!(result, "do some work");
        assert!(!result.contains(MCP_ONLY_PREAMBLE));
    }

    #[test]
    fn skips_augmentation_for_empty_prompt_when_override_set() {
        let guard = permissive_guard();
        let result = guard.augment_prompt("");
        assert_eq!(result, "");
    }

    // ── PolicyViolation display ───────────────────────────────────────────────

    #[test]
    fn policy_violation_display_includes_all_fields() {
        let v = PolicyViolation {
            rule: "test-rule".to_string(),
            message: "something bad".to_string(),
            remediation: "fix it".to_string(),
        };
        let s = v.to_string();
        assert!(s.contains("test-rule"));
        assert!(s.contains("something bad"));
        assert!(s.contains("fix it"));
    }

    // ── MCP_ONLY_PREAMBLE content ─────────────────────────────────────────────

    #[test]
    fn mcp_only_preamble_mentions_key_operations() {
        assert!(MCP_ONLY_PREAMBLE.contains("MCP server tools"));
        assert!(MCP_ONLY_PREAMBLE.contains("issues"));
        assert!(MCP_ONLY_PREAMBLE.contains("pull request"));
    }
}
