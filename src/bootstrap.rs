//! Bootstrap subcommand — create or patch workspace prerequisites so a
//! repository is ready to run `code-looper`.
//!
//! For each prerequisite that the [`crate::workspace::PrerequisiteChecker`]
//! validates, bootstrap produces a minimal, safe fix:
//!
//! | Prerequisite | Action |
//! |---|---|
//! | No instruction file | Create `CLAUDE.md` with a Code Looper section |
//! | Instruction file lacks a Code Looper section | Append a delimited section |
//! | `.mcp.json` missing | Create a minimal stub |
//! | `.mcp.json` lacks a `"github"` key | Merge the entry into the existing file |
//!
//! All changes are idempotent.  In `--dry-run` mode nothing is written.

use std::path::{Path, PathBuf};

pub const SECTION_BEGIN: &str = "<!-- code-looper begin -->";
pub const SECTION_END: &str = "<!-- code-looper end -->";

/// Minimal Code Looper section to inject into an instruction file.
const CLAUDE_MD_SECTION: &str = r#"<!-- code-looper begin -->
## Code Looper

This repository is configured to run with [Code Looper](https://github.com/jamesbrayton/code-looper).

### GitHub mutation policy

All GitHub operations (issue create/update/comment, PR review/comment/merge,
branch actions) **must** be performed via the GitHub MCP server.  Direct `gh`
CLI mutations are disabled by default.

### Work-log discipline

During loop runs the agent should:
- Comment on the linked issue at meaningful milestones (scope clarified, first
  implementation pass complete, tests added, blocker found, handoff).
- Keep the issue body current (checklist, decisions, blockers/dependencies).
- Create new issues (via GitHub MCP) when discovered work falls outside the
  current issue's scope, using labels: `bug`, `enhancement`, `tech-debt`,
  `discovered-during-loop`.
- Close the issue with a summary comment when the checklist is complete and
  the work is committed.
<!-- code-looper end -->"#;

/// Minimal `.mcp.json` stub created when no file exists at all.
const MCP_STUB: &str = r#"{
  "mcpServers": {
    "github": {
      "command": "docker",
      "args": [
        "run",
        "-i",
        "--rm",
        "-e",
        "GITHUB_PERSONAL_ACCESS_TOKEN",
        "ghcr.io/github/github-mcp-server"
      ],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "${GITHUB_TOKEN}"
      }
    }
  }
}
"#;

/// A single action taken (or that would be taken) by bootstrap.
#[derive(Debug, PartialEq)]
pub enum BootstrapAction {
    /// Created a new file at the given path.
    Created(PathBuf),
    /// Appended content to an existing file.
    Appended(PathBuf),
    /// Merged a JSON key into an existing file.
    MergedJson(PathBuf),
    /// The prerequisite was already satisfied; no change needed.
    AlreadySatisfied(PathBuf),
}

impl std::fmt::Display for BootstrapAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootstrapAction::Created(p) => write!(f, "[bootstrap] {}: created", p.display()),
            BootstrapAction::Appended(p) => {
                write!(
                    f,
                    "[bootstrap] {}: appended Code Looper section",
                    p.display()
                )
            }
            BootstrapAction::MergedJson(p) => {
                write!(
                    f,
                    "[bootstrap] {}: added \"github\" server entry",
                    p.display()
                )
            }
            BootstrapAction::AlreadySatisfied(p) => {
                write!(f, "[bootstrap] {}: already satisfied", p.display())
            }
        }
    }
}

/// Run the bootstrap process for `workspace_dir`.
///
/// When `dry_run` is `true` the function returns the same actions it would
/// have taken but writes nothing to disk.
///
/// Returns the list of actions taken (or that would be taken).
pub fn run_bootstrap(workspace_dir: &Path, dry_run: bool) -> anyhow::Result<Vec<BootstrapAction>> {
    Ok(vec![
        bootstrap_instruction_file(workspace_dir, dry_run)?,
        bootstrap_mcp_config(workspace_dir, dry_run)?,
    ])
}

// ── Instruction file ──────────────────────────────────────────────────────────

fn bootstrap_instruction_file(
    workspace_dir: &Path,
    dry_run: bool,
) -> anyhow::Result<BootstrapAction> {
    const CANDIDATES: &[&str] = &["CLAUDE.md", "AGENTS.md", ".github/copilot-instructions.md"];

    // Find the first existing instruction file.
    let existing = CANDIDATES.iter().find_map(|name| {
        let p = workspace_dir.join(name);
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    });

    match existing {
        None => {
            // No instruction file at all → create CLAUDE.md.
            let path = workspace_dir.join("CLAUDE.md");
            if !dry_run {
                std::fs::write(
                    &path,
                    format!("# Project Instructions\n\n{CLAUDE_MD_SECTION}\n"),
                )
                .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", path.display()))?;
            }
            Ok(BootstrapAction::Created(path))
        }
        Some(path) => {
            let contents = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;

            if contents.contains(SECTION_BEGIN) && contents.contains(SECTION_END) {
                // Complete section already present.
                Ok(BootstrapAction::AlreadySatisfied(path))
            } else {
                // Append the delimited section.
                if !dry_run {
                    let separator = if contents.ends_with('\n') {
                        "\n"
                    } else {
                        "\n\n"
                    };
                    let updated = format!("{contents}{separator}{CLAUDE_MD_SECTION}\n");
                    std::fs::write(&path, updated)
                        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
                }
                Ok(BootstrapAction::Appended(path))
            }
        }
    }
}

// ── MCP config ────────────────────────────────────────────────────────────────

fn bootstrap_mcp_config(workspace_dir: &Path, dry_run: bool) -> anyhow::Result<BootstrapAction> {
    let path = workspace_dir.join(".mcp.json");

    if !path.is_file() {
        if !dry_run {
            std::fs::write(&path, MCP_STUB)
                .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", path.display()))?;
        }
        return Ok(BootstrapAction::Created(path));
    }

    let contents = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;

    if has_github_server(&contents) {
        return Ok(BootstrapAction::AlreadySatisfied(path));
    }

    // The file exists but lacks a "github" entry.  Merge it in.
    let merged = merge_github_server(&contents)
        .ok_or_else(|| anyhow::anyhow!("could not parse {} as a JSON object", path.display()))?;

    if !dry_run {
        std::fs::write(&path, merged)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    }

    Ok(BootstrapAction::MergedJson(path))
}

/// Returns `true` when the JSON contains a `"github"` MCP server key.
fn has_github_server(json: &str) -> bool {
    // Same heuristic as workspace.rs — avoids a full JSON parse dependency.
    let trimmed_key = "\"github\"";
    json.contains(trimmed_key)
}

/// Insert a `"github"` server entry into a JSON object.
///
/// Supports two layouts:
///   `{ "mcpServers": { … } }` — inserts under `mcpServers`
///   `{ … }` — inserts directly at the top level
///
/// Returns `None` if the file does not look like a JSON object.
fn merge_github_server(json: &str) -> Option<String> {
    let github_entry = r#""github": {
      "command": "docker",
      "args": [
        "run",
        "-i",
        "--rm",
        "-e",
        "GITHUB_PERSONAL_ACCESS_TOKEN",
        "ghcr.io/github/github-mcp-server"
      ],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "${GITHUB_TOKEN}"
      }
    }"#;

    // Locate `"mcpServers"` block if present.
    if let Some(mcp_start) = json.find("\"mcpServers\"") {
        // Find the opening `{` of the mcpServers value.
        let after_key = &json[mcp_start + "\"mcpServers\"".len()..];
        let brace_offset = after_key.find('{')?;
        let insert_pos = mcp_start + "\"mcpServers\"".len() + brace_offset + 1;
        // Determine whether there's already content in the object.
        let inner = &json[insert_pos..];
        let needs_comma = !inner.trim_start().starts_with('}');
        let comma = if needs_comma { "," } else { "" };
        let indented = github_entry
            .lines()
            .map(|l| format!("    {l}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = format!(
            "{}\n{indented}{comma}{}",
            &json[..insert_pos],
            &json[insert_pos..]
        );
        return Some(result);
    }

    // No mcpServers block: insert at the top-level object.
    let open = json.find('{')?;
    let insert_pos = open + 1;
    let inner = &json[insert_pos..];
    let needs_comma = !inner.trim_start().starts_with('}');
    let comma = if needs_comma { "," } else { "" };
    let result = format!(
        "{}\n  {}{comma}{}",
        &json[..insert_pos],
        github_entry.lines().collect::<Vec<_>>().join("\n  "),
        &json[insert_pos..]
    );
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // ── Instruction file tests ─────────────────────────────────────────────────

    #[test]
    fn creates_claude_md_when_no_instruction_file() {
        let dir = tmp();
        let path = dir.path().join("CLAUDE.md");
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[0], BootstrapAction::Created(p) if p == &path));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains(SECTION_BEGIN));
        assert!(content.contains(SECTION_END));
    }

    #[test]
    fn second_run_is_no_op_for_claude_md() {
        let dir = tmp();
        run_bootstrap(dir.path(), false).unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[0], BootstrapAction::AlreadySatisfied(_)));
    }

    #[test]
    fn appends_section_to_existing_claude_md_without_section() {
        let dir = tmp();
        let path = dir.path().join("CLAUDE.md");
        fs::write(&path, "# My Project\n\nExisting content.\n").unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[0], BootstrapAction::Appended(p) if p == &path));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("# My Project"));
        assert!(content.contains("Existing content."));
        assert!(content.contains(SECTION_BEGIN));
        assert!(content.contains(SECTION_END));
    }

    #[test]
    fn existing_claude_md_with_section_is_satisfied() {
        let dir = tmp();
        let path = dir.path().join("CLAUDE.md");
        fs::write(
            &path,
            format!("# Proj\n{SECTION_BEGIN}\nstuff\n{SECTION_END}\n"),
        )
        .unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[0], BootstrapAction::AlreadySatisfied(p) if p == &path));
        // Content must not change.
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches(SECTION_BEGIN).count(), 1);
    }

    #[test]
    fn dry_run_does_not_create_claude_md() {
        let dir = tmp();
        let path = dir.path().join("CLAUDE.md");
        let actions = run_bootstrap(dir.path(), true).unwrap();
        assert!(matches!(&actions[0], BootstrapAction::Created(_)));
        assert!(!path.exists(), "dry-run must not create files");
    }

    #[test]
    fn recognises_agents_md_as_instruction_file() {
        let dir = tmp();
        let path = dir.path().join("AGENTS.md");
        fs::write(&path, "# Agents\n").unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        // Should append (no section yet), not create a new CLAUDE.md.
        assert!(matches!(&actions[0], BootstrapAction::Appended(p) if p == &path));
    }

    // ── MCP config tests ──────────────────────────────────────────────────────

    #[test]
    fn creates_mcp_json_when_missing() {
        let dir = tmp();
        let path = dir.path().join(".mcp.json");
        // Run bootstrap with a CLAUDE.md already present so only .mcp.json changes.
        fs::write(
            dir.path().join("CLAUDE.md"),
            format!("{SECTION_BEGIN}\n{SECTION_END}\n"),
        )
        .unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[1], BootstrapAction::Created(p) if p == &path));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"github\""));
    }

    #[test]
    fn mcp_json_with_github_key_is_satisfied() {
        let dir = tmp();
        fs::write(
            dir.path().join("CLAUDE.md"),
            format!("{SECTION_BEGIN}\n{SECTION_END}\n"),
        )
        .unwrap();
        let mcp_path = dir.path().join(".mcp.json");
        fs::write(&mcp_path, r#"{"mcpServers":{"github":{}}}"#).unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[1], BootstrapAction::AlreadySatisfied(p) if p == &mcp_path));
    }

    #[test]
    fn mcp_json_without_github_key_gets_merged() {
        let dir = tmp();
        fs::write(
            dir.path().join("CLAUDE.md"),
            format!("{SECTION_BEGIN}\n{SECTION_END}\n"),
        )
        .unwrap();
        let mcp_path = dir.path().join(".mcp.json");
        fs::write(&mcp_path, r#"{"mcpServers":{"context7":{}}}"#).unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[1], BootstrapAction::MergedJson(p) if p == &mcp_path));
        let content = fs::read_to_string(&mcp_path).unwrap();
        assert!(content.contains("\"github\""));
        assert!(
            content.contains("\"context7\""),
            "existing keys must be preserved"
        );
    }

    #[test]
    fn dry_run_does_not_create_mcp_json() {
        let dir = tmp();
        fs::write(
            dir.path().join("CLAUDE.md"),
            format!("{SECTION_BEGIN}\n{SECTION_END}\n"),
        )
        .unwrap();
        let mcp_path = dir.path().join(".mcp.json");
        let actions = run_bootstrap(dir.path(), true).unwrap();
        assert!(matches!(&actions[1], BootstrapAction::Created(_)));
        assert!(!mcp_path.exists(), "dry-run must not create .mcp.json");
    }

    // ── Fully configured workspace ─────────────────────────────────────────────

    #[test]
    fn fully_configured_workspace_produces_all_satisfied() {
        let dir = tmp();
        fs::write(
            dir.path().join("CLAUDE.md"),
            format!("{SECTION_BEGIN}\n{SECTION_END}\n"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".mcp.json"),
            r#"{"mcpServers":{"github":{}}}"#,
        )
        .unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[0], BootstrapAction::AlreadySatisfied(_)));
        assert!(matches!(&actions[1], BootstrapAction::AlreadySatisfied(_)));
    }
}
