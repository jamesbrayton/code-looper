//! Policy guard — agent-prompt enforcement of GitHub tool selection policy.
//!
//! The default policy is `gh`-first with MCP fallback:
//! [`PolicyGuard::augment_prompt`] prepends a preamble telling providers to
//! use `gh` CLI first for GitHub operations and fall back to MCP tools when
//! `gh` is unavailable or fails.  A strict mode remains available for
//! MCP-only write paths.
//!
//! This module does **not** enforce anything at the process level; enforcement
//! is prompt-level and trusts the agent to honour the preamble.  A stronger
//! sandbox would require removing `gh` from the provider's `PATH`, which is
//! out of scope for this ADR.

/// Configuration for policy behavior.
#[derive(Debug, Clone, Default)]
pub struct UnsafeOverrides {
    /// When `true` (runtime default), prompts enforce `gh`-first with MCP
    /// fallback.  When `false`, strict MCP-only preamble is used.
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

/// Agent-prompt enforcement of GitHub operation policy.
///
/// The guard has a single real responsibility: prepend a preamble to every
/// provider prompt describing the GitHub-operation policy (see
/// [`Self::augment_prompt`]). There is no process-level sandboxing; this is
/// prompt-level enforcement only.
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
    /// beyond what the config loader already validates, and policy enforcement
    /// happens at the prompt level via [`Self::augment_prompt`].  The method
    /// is kept so callers have a single startup hook to attach future
    /// validations to.
    pub fn check_startup(&self, _orchestration_enabled: bool) -> Vec<PolicyViolation> {
        Vec::new()
    }

    /// Augment a provider prompt with a GitHub policy preamble.
    ///
    /// When `allow_direct_github` is `true`, a preamble is prepended that
    /// instructs the agent to use `gh` first and MCP as fallback.
    /// When `allow_direct_github` is `false`, strict MCP-only write policy is
    /// prepended.
    ///
    /// If the prompt is empty the preamble alone is returned so providers
    /// always receive the guard constraint.
    pub fn augment_prompt(&self, prompt: &str) -> String {
        let preamble = if self.overrides.allow_direct_github {
            GITHUB_CLI_FIRST_PREAMBLE
        } else {
            MCP_ONLY_PREAMBLE
        };
        if prompt.is_empty() {
            preamble.to_string()
        } else {
            format!("{preamble}\n\n{prompt}")
        }
    }
}

/// Preamble injected into provider prompts for gh-first operation.
const GITHUB_CLI_FIRST_PREAMBLE: &str = "\
IMPORTANT - GitHub operations policy:
Use `gh` CLI as the default path for GitHub operations (issues, pull requests, \
comments, branch operations, and merges). \
If a `gh` command is unavailable or fails for a tool-capability reason, fall \
back to the configured GitHub MCP tools for that action. \
Do not stop after a `gh`-tooling failure without attempting MCP fallback when \
the action is still required.";

/// Preamble injected into provider prompts for strict MCP-only mode.
const MCP_ONLY_PREAMBLE: &str = "\
IMPORTANT - GitHub operations policy:
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
        PolicyGuard::new(UnsafeOverrides {
            allow_direct_github: true,
        })
    }

    fn strict_guard() -> PolicyGuard {
        PolicyGuard::new(UnsafeOverrides {
            allow_direct_github: false,
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
        let violations = default_guard().check_startup(true);
        assert!(violations.is_empty());
    }

    // ── augment_prompt ────────────────────────────────────────────────────────

    #[test]
    fn augments_prompt_with_gh_first_preamble_by_default() {
        let guard = default_guard();
        let result = guard.augment_prompt("do some work");
        assert!(result.contains(GITHUB_CLI_FIRST_PREAMBLE));
        assert!(result.contains("do some work"));
    }

    #[test]
    fn preamble_appears_before_original_prompt() {
        let guard = default_guard();
        let result = guard.augment_prompt("do some work");
        let preamble_pos = result.find(GITHUB_CLI_FIRST_PREAMBLE).unwrap();
        let prompt_pos = result.find("do some work").unwrap();
        assert!(preamble_pos < prompt_pos);
    }

    #[test]
    fn empty_prompt_returns_only_gh_first_preamble() {
        let guard = default_guard();
        let result = guard.augment_prompt("");
        assert_eq!(result, GITHUB_CLI_FIRST_PREAMBLE);
    }

    #[test]
    fn strict_mode_uses_mcp_only_preamble() {
        let guard = strict_guard();
        let result = guard.augment_prompt("do some work");
        assert!(result.contains(MCP_ONLY_PREAMBLE));
    }

    #[test]
    fn strict_mode_empty_prompt_returns_mcp_only_preamble() {
        let guard = strict_guard();
        let result = guard.augment_prompt("");
        assert_eq!(result, MCP_ONLY_PREAMBLE);
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

    // ── preamble content ─────────────────────────────────────────────────────

    #[test]
    fn mcp_only_preamble_mentions_key_operations() {
        assert!(MCP_ONLY_PREAMBLE.contains("MCP server tools"));
        assert!(MCP_ONLY_PREAMBLE.contains("issues"));
        assert!(MCP_ONLY_PREAMBLE.contains("pull request"));
    }

    #[test]
    fn gh_first_preamble_mentions_gh_and_fallback() {
        assert!(GITHUB_CLI_FIRST_PREAMBLE.contains("`gh` CLI"));
        assert!(GITHUB_CLI_FIRST_PREAMBLE.contains("fall"));
    }
}
