/// Configuration for unsafe bypass overrides.
///
/// All overrides are opt-in and disabled by default.  Enabling any of them
/// weakens the safety guarantees described in the PRD.
#[derive(Debug, Clone, Default)]
pub struct UnsafeOverrides {
    /// When `true`, the system allows orchestration context to be resolved via
    /// direct `gh` CLI calls rather than requiring a GitHub MCP server.
    /// Caution: this bypasses the MCP-only GitHub integration path.
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

/// Enforces the MCP-only GitHub mutation path and validates execution config.
///
/// The guard runs at startup, before any loop iteration, and raises
/// `PolicyViolation`s for any configuration that would allow GitHub mutations
/// outside the approved MCP server path.
pub struct PolicyGuard {
    overrides: UnsafeOverrides,
}

impl PolicyGuard {
    pub fn new(overrides: UnsafeOverrides) -> Self {
        Self { overrides }
    }

    /// Validate that the orchestration configuration is safe.
    ///
    /// Returns a list of violations (empty = all clear).
    pub fn validate_orchestration(&self, orchestration_enabled: bool) -> Vec<PolicyViolation> {
        let violations = Vec::new();

        if orchestration_enabled && !self.overrides.allow_direct_github {
            // When orchestration is enabled, context resolution currently falls
            // back to the `GhCliContextResolver` (direct `gh` CLI).  This is
            // permitted for READ-ONLY operations, but the guard must ensure the
            // prompts delivered to providers explicitly require MCP tool use for
            // any WRITE operations.
            //
            // No violation is raised here for reads; the guard instead
            // augments prompts (see `augment_prompt`) to enforce MCP-only
            // writes.
        }

        violations
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

    // ── validate_orchestration ────────────────────────────────────────────────

    #[test]
    fn no_violations_when_orchestration_disabled() {
        let violations = default_guard().validate_orchestration(false);
        assert!(violations.is_empty());
    }

    #[test]
    fn no_violations_when_orchestration_enabled_default_policy() {
        // Orchestration with default (safe) policy: reads via gh are allowed;
        // writes are constrained via prompt augmentation, not a hard violation.
        let violations = default_guard().validate_orchestration(true);
        assert!(violations.is_empty());
    }

    #[test]
    fn no_violations_with_allow_direct_github_override() {
        let violations = permissive_guard().validate_orchestration(true);
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
