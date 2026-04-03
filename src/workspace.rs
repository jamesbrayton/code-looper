use std::path::{Path, PathBuf};

/// A single failed prerequisite check with actionable guidance.
#[derive(Debug, Clone)]
pub struct DiagnosticError {
    /// Short identifier for this check (e.g. "instruction-file").
    pub check: String,
    /// Human-readable description of what was wrong.
    pub message: String,
    /// What the user should do to resolve the issue.
    pub remediation: String,
}

impl std::fmt::Display for DiagnosticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}\n  → Remediation: {}",
            self.check, self.message, self.remediation
        )
    }
}

/// Aggregate result of running all prerequisite checks.
#[derive(Debug, Default)]
pub struct CheckResult {
    /// Names of checks that passed.
    pub passed: Vec<String>,
    /// Details for every check that failed.
    pub failed: Vec<DiagnosticError>,
}

impl CheckResult {
    /// Returns `true` when every check passed.
    pub fn is_ok(&self) -> bool {
        self.failed.is_empty()
    }

    /// Print a human-readable summary to stderr.
    pub fn print_summary(&self) {
        for name in &self.passed {
            eprintln!("  ✓ {name}");
        }
        for diag in &self.failed {
            eprintln!("  ✗ {diag}");
        }
    }
}

/// Checks that a workspace directory satisfies Code Looper prerequisites.
///
/// Validates:
/// 1. An instruction file exists (`CLAUDE.md`, `AGENTS.md`, or
///    `.github/copilot-instructions.md`).
/// 2. An MCP config file (`.mcp.json`) exists and contains a `"github"` key,
///    indicating the GitHub MCP server is configured.
pub struct PrerequisiteChecker {
    workspace_dir: PathBuf,
}

impl PrerequisiteChecker {
    pub fn new(workspace_dir: impl Into<PathBuf>) -> Self {
        Self { workspace_dir: workspace_dir.into() }
    }

    /// Run all prerequisite checks and return the aggregate result.
    pub fn run(&self) -> CheckResult {
        let mut result = CheckResult::default();
        self.check_instruction_file(&mut result);
        self.check_mcp_config(&mut result);
        result
    }

    // ── Individual checks ─────────────────────────────────────────────────────

    fn check_instruction_file(&self, result: &mut CheckResult) {
        const CHECK: &str = "instruction-file";
        let candidates = [
            "CLAUDE.md",
            "AGENTS.md",
            ".github/copilot-instructions.md",
        ];

        let found = candidates
            .iter()
            .any(|name| self.workspace_dir.join(name).is_file());

        if found {
            result.passed.push(CHECK.to_string());
        } else {
            result.failed.push(DiagnosticError {
                check: CHECK.to_string(),
                message: format!(
                    "No instruction file found in '{}'. Expected one of: {}",
                    self.workspace_dir.display(),
                    candidates.join(", ")
                ),
                remediation: "Create a CLAUDE.md (or AGENTS.md) at the repository root \
                              with agent instructions for this project."
                    .to_string(),
            });
        }
    }

    fn check_mcp_config(&self, result: &mut CheckResult) {
        const CHECK: &str = "mcp-github-server";
        let mcp_path = self.workspace_dir.join(".mcp.json");

        if !mcp_path.is_file() {
            result.failed.push(DiagnosticError {
                check: CHECK.to_string(),
                message: format!(
                    "No .mcp.json found in '{}'",
                    self.workspace_dir.display()
                ),
                remediation: "Create a .mcp.json that includes a \"github\" MCP server entry. \
                              See https://docs.anthropic.com/en/docs/claude-code/mcp for details."
                    .to_string(),
            });
            return;
        }

        match std::fs::read_to_string(&mcp_path) {
            Err(e) => {
                result.failed.push(DiagnosticError {
                    check: CHECK.to_string(),
                    message: format!("Failed to read {}: {e}", mcp_path.display()),
                    remediation: "Ensure .mcp.json is readable.".to_string(),
                });
            }
            Ok(contents) => {
                if has_github_server(&contents) {
                    result.passed.push(CHECK.to_string());
                } else {
                    result.failed.push(DiagnosticError {
                        check: CHECK.to_string(),
                        message: format!(
                            "{} does not contain a \"github\" MCP server entry",
                            mcp_path.display()
                        ),
                        remediation: "Add a GitHub MCP server block under the \"mcpServers\" \
                                      key in .mcp.json so that orchestration flows can use \
                                      GitHub tools."
                            .to_string(),
                    });
                }
            }
        }
    }
}

/// Returns `true` when the MCP config JSON contains a top-level or nested
/// `"github"` server key.
///
/// We deliberately avoid a full JSON parse dependency here: a simple string
/// search for `"github"` as a JSON key is sufficient for this check.
fn has_github_server(json: &str) -> bool {
    // Look for `"github"` as a JSON object key (preceded/followed by typical
    // JSON delimiters).  This avoids adding a json parsing dependency while
    // being robust enough for the expected .mcp.json structure.
    json.contains("\"github\"")
}

/// Convenience helper — returns the path to use as the workspace dir.
///
/// If `override_path` is `Some`, that path is used.  Otherwise falls back to
/// the current working directory.
pub fn resolve_workspace_dir(override_path: Option<&Path>) -> PathBuf {
    if let Some(p) = override_path {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn setup_dir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write_file(dir: &TempDir, name: &str, contents: &str) {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    fn valid_mcp_json() -> &'static str {
        r#"{"mcpServers":{"github":{"command":"npx","args":["@github/mcp"]}}}"#
    }

    // ── instruction-file checks ───────────────────────────────────────────────

    #[test]
    fn passes_when_claude_md_exists() {
        let dir = setup_dir();
        write_file(&dir, "CLAUDE.md", "# instructions");
        write_file(&dir, ".mcp.json", valid_mcp_json());

        let result = PrerequisiteChecker::new(dir.path()).run();
        assert!(result.is_ok(), "unexpected failures: {:?}", result.failed);
        assert!(result.passed.contains(&"instruction-file".to_string()));
    }

    #[test]
    fn passes_when_agents_md_exists() {
        let dir = setup_dir();
        write_file(&dir, "AGENTS.md", "# instructions");
        write_file(&dir, ".mcp.json", valid_mcp_json());

        let result = PrerequisiteChecker::new(dir.path()).run();
        assert!(result.is_ok());
    }

    #[test]
    fn passes_when_copilot_instructions_exist() {
        let dir = setup_dir();
        write_file(&dir, ".github/copilot-instructions.md", "# instructions");
        write_file(&dir, ".mcp.json", valid_mcp_json());

        let result = PrerequisiteChecker::new(dir.path()).run();
        assert!(result.is_ok());
    }

    #[test]
    fn fails_when_no_instruction_file() {
        let dir = setup_dir();
        write_file(&dir, ".mcp.json", valid_mcp_json());

        let result = PrerequisiteChecker::new(dir.path()).run();
        assert!(!result.is_ok());
        assert!(result.failed.iter().any(|d| d.check == "instruction-file"));
    }

    // ── mcp-github-server checks ──────────────────────────────────────────────

    #[test]
    fn passes_when_mcp_json_has_github_key() {
        let dir = setup_dir();
        write_file(&dir, "CLAUDE.md", "# instructions");
        write_file(&dir, ".mcp.json", valid_mcp_json());

        let result = PrerequisiteChecker::new(dir.path()).run();
        assert!(result.is_ok());
        assert!(result.passed.contains(&"mcp-github-server".to_string()));
    }

    #[test]
    fn fails_when_mcp_json_missing() {
        let dir = setup_dir();
        write_file(&dir, "CLAUDE.md", "# instructions");

        let result = PrerequisiteChecker::new(dir.path()).run();
        assert!(!result.is_ok());
        assert!(result.failed.iter().any(|d| d.check == "mcp-github-server"));
    }

    #[test]
    fn fails_when_mcp_json_lacks_github_key() {
        let dir = setup_dir();
        write_file(&dir, "CLAUDE.md", "# instructions");
        write_file(&dir, ".mcp.json", r#"{"mcpServers":{"context7":{}}}"#);

        let result = PrerequisiteChecker::new(dir.path()).run();
        assert!(!result.is_ok());
        let diag = result.failed.iter().find(|d| d.check == "mcp-github-server").unwrap();
        assert!(!diag.remediation.is_empty());
    }

    // ── multiple failures ─────────────────────────────────────────────────────

    #[test]
    fn reports_all_failures_when_everything_missing() {
        let dir = setup_dir();

        let result = PrerequisiteChecker::new(dir.path()).run();
        assert_eq!(result.failed.len(), 2);
        assert_eq!(result.passed.len(), 0);
    }

    #[test]
    fn is_ok_returns_false_on_any_failure() {
        let dir = setup_dir();
        let result = PrerequisiteChecker::new(dir.path()).run();
        assert!(!result.is_ok());
    }

    // ── diagnostic formatting ─────────────────────────────────────────────────

    #[test]
    fn diagnostic_error_display_includes_all_fields() {
        let d = DiagnosticError {
            check: "test-check".to_string(),
            message: "something wrong".to_string(),
            remediation: "fix it this way".to_string(),
        };
        let s = d.to_string();
        assert!(s.contains("test-check"));
        assert!(s.contains("something wrong"));
        assert!(s.contains("fix it this way"));
    }

    // ── resolve_workspace_dir ────────────────────────────────────────────────

    #[test]
    fn resolve_workspace_dir_uses_override() {
        let dir = setup_dir();
        let resolved = resolve_workspace_dir(Some(dir.path()));
        assert_eq!(resolved, dir.path());
    }

    #[test]
    fn resolve_workspace_dir_falls_back_to_cwd_when_none() {
        let resolved = resolve_workspace_dir(None);
        assert!(resolved.is_absolute() || resolved == PathBuf::from("."));
    }
}
