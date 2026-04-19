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
//! | `.gitignore` missing | Create with `.code-looper/runs/` entry |
//! | `.gitignore` lacks `.code-looper/runs/` | Append the entry |
//!
//! All changes are idempotent.  In `--dry-run` mode nothing is written.

use crate::workspace::has_github_server;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Atomically write `contents` to `path` using a temp file + rename.
///
/// The file is first written to a `NamedTempFile` in the same directory as
/// `path`, then atomically renamed via `persist`.  This prevents partial
/// writes from power loss or process kills.
fn atomic_write(path: &Path, contents: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let existing_perms = std::fs::metadata(path).ok().map(|m| m.permissions());
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(contents.as_bytes())?;
    if let Some(perms) = existing_perms {
        tmp.as_file().set_permissions(perms)?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

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
                write!(f, "[bootstrap] {}: appended entry", p.display())
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
        bootstrap_gitignore(workspace_dir, dry_run)?,
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
                atomic_write(
                    &path,
                    &format!("# Project Instructions\n\n{CLAUDE_MD_SECTION}\n"),
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
            } else if contents.contains(SECTION_BEGIN) && !contents.contains(SECTION_END) {
                // Orphaned begin marker (partial/corrupt injection from a prior
                // interrupted run).  Remove everything from the orphaned begin
                // marker to the end of the file before re-appending a clean section.
                if !dry_run {
                    let orphan_pos = contents
                        .find(SECTION_BEGIN)
                        .expect("contains() confirmed presence");
                    let truncated = contents[..orphan_pos].trim_end();
                    let separator = if truncated.is_empty() { "" } else { "\n\n" };
                    let updated = format!("{truncated}{separator}{CLAUDE_MD_SECTION}\n");
                    atomic_write(&path, &updated)
                        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
                }
                Ok(BootstrapAction::Appended(path))
            } else {
                // Append the delimited section.
                if !dry_run {
                    let separator = if contents.ends_with('\n') {
                        "\n"
                    } else {
                        "\n\n"
                    };
                    let updated = format!("{contents}{separator}{CLAUDE_MD_SECTION}\n");
                    atomic_write(&path, &updated)
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
            atomic_write(&path, MCP_STUB)
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
        atomic_write(&path, &merged)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    }

    Ok(BootstrapAction::MergedJson(path))
}

/// Strip trailing commas that appear before `}` or `]` in a JSONC string.
///
/// Handles trailing commas in both JSON objects and arrays, covering
/// the JSONC convention that editors like VS Code produce by default.
fn strip_trailing_commas_in_object(tail: &str) -> String {
    let mut result = String::with_capacity(tail.len());
    let chars: Vec<char> = tail.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i] == ',' {
            // Look ahead past whitespace/newlines for `}` or `]`. If found, skip this comma.
            let mut j = i + 1;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            if j < len && (chars[j] == '}' || chars[j] == ']') {
                // Skip the trailing comma — don't push it.
                i += 1;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// Insert a `"github"` server entry into a JSON object.
///
/// Supports two layouts:
///   `{ "mcpServers": { … } }` — inserts under `mcpServers`
///   `{ … }` — inserts directly at the top level
///
/// Returns `None` if the file does not look like a JSON object.
///
/// Uses `serde_json` for parsing and modification to avoid false-match
/// issues with raw string search (see #171).  Trailing commas (JSONC) are
/// stripped before parsing.
fn merge_github_server(json: &str) -> Option<String> {
    let github_value: serde_json::Value = serde_json::from_str(
        r#"{
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
        }"#,
    )
    .expect("static github entry is valid JSON");

    // Strip trailing commas so we can parse JSONC-style input.
    let cleaned = strip_trailing_commas_in_object(json);
    let mut doc: serde_json::Value = serde_json::from_str(&cleaned).ok()?;
    let root = doc.as_object_mut()?;

    if let Some(servers) = root.get_mut("mcpServers") {
        let servers = servers.as_object_mut()?;
        servers.insert("github".to_string(), github_value);
    } else {
        let mut servers = serde_json::Map::new();
        servers.insert("github".to_string(), github_value);
        root.insert("mcpServers".to_string(), serde_json::Value::Object(servers));
    }

    Some(serde_json::to_string_pretty(&doc).expect("serialization cannot fail"))
}

// ── .gitignore ───────────────────────────────────────────────────────────────

/// The gitignore entry appended (or used to seed) `.gitignore`.
///
/// Only the `runs/` subdirectory is ignored — config, rules, and prompts
/// under `.code-looper/` are intended to be committed to version control.
const GITIGNORE_ENTRY: &str = ".code-looper/runs/";
const GITIGNORE_COMMENT: &str = "# Code Looper run artifacts";

fn bootstrap_gitignore(workspace_dir: &Path, dry_run: bool) -> anyhow::Result<BootstrapAction> {
    let path = workspace_dir.join(".gitignore");

    if !path.is_file() {
        if !dry_run {
            atomic_write(&path, &format!("{GITIGNORE_COMMENT}\n{GITIGNORE_ENTRY}\n"))
                .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", path.display()))?;
        }
        return Ok(BootstrapAction::Created(path));
    }

    let contents = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;

    if has_code_looper_ignore(&contents) {
        return Ok(BootstrapAction::AlreadySatisfied(path));
    }

    if !dry_run {
        let separator = if contents.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        let updated = format!("{contents}{separator}{GITIGNORE_COMMENT}\n{GITIGNORE_ENTRY}\n");
        atomic_write(&path, &updated)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    }

    Ok(BootstrapAction::Appended(path))
}

/// Returns `true` when `.gitignore` already contains a Code Looper ignore
/// entry — either the narrow `.code-looper/runs/` rule or the legacy broad
/// `.code-looper/` rule (with or without trailing slash).  Ignores leading
/// and trailing whitespace on each line.
fn has_code_looper_ignore(contents: &str) -> bool {
    contents.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == ".code-looper"
            || trimmed == ".code-looper/"
            || trimmed == ".code-looper/runs"
            || trimmed == ".code-looper/runs/"
    })
}

/// Returns `true` when `.gitignore` contains the broad `.code-looper/` rule
/// (as opposed to the narrower `.code-looper/runs/` rule).
///
/// When this returns `true` **and** a `.code-looper/config.toml` exists,
/// callers should warn the user that the broad rule will hide version-
/// controlled config files.
pub fn has_broad_code_looper_ignore(contents: &str) -> bool {
    contents.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == ".code-looper" || trimmed == ".code-looper/"
    })
}

/// Emit a warning when a broad `.code-looper/` gitignore rule coexists with
/// a `.code-looper/config.toml` in the workspace.  The broad rule would
/// silently hide version-controlled config, rules, and prompt files.
///
/// Returns `true` if the warning was emitted.
pub fn warn_if_broad_ignore_hides_config(workspace_dir: &Path) -> bool {
    let gitignore_path = workspace_dir.join(".gitignore");

    // Check all config file candidates, not just config.toml (#118).
    let config_exists = ["config.toml", "config.yaml", "config.yml"]
        .iter()
        .any(|f| workspace_dir.join(".code-looper").join(f).is_file());

    if !config_exists || !gitignore_path.is_file() {
        return false;
    }

    let contents = match std::fs::read_to_string(&gitignore_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(path = %gitignore_path.display(), "could not read .gitignore: {e}");
            eprintln!(
                "[bootstrap] warning: could not read {}: {e}",
                gitignore_path.display()
            );
            return false;
        }
    };

    if has_broad_code_looper_ignore(&contents) {
        eprintln!(
            "warning: .gitignore contains a broad \".code-looper/\" rule that hides \
             .code-looper/config.toml and other version-controlled files.\n  \
             → Replace \".code-looper/\" with \".code-looper/runs/\" in your .gitignore."
        );
        return true;
    }

    false
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
    fn has_github_server_rejects_false_positive_in_value() {
        // "github" appearing as a string value, not as an mcpServers key,
        // must not be treated as a GitHub server entry (#110).
        assert!(!has_github_server(
            r#"{"mcpServers":{"myserver":{"description":"see github for details"}}}"#
        ));
    }

    #[test]
    fn has_github_server_rejects_github_outside_mcp_servers() {
        // "github" as a top-level key (not under mcpServers) must not match.
        assert!(!has_github_server(r#"{"github":"some-value"}"#));
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

    // ── merge_github_server unit tests (#100) ────────────────────────────────

    #[test]
    fn merge_handles_trailing_comma_in_mcp_servers() {
        let input = r#"{
  "mcpServers": {
    "context7": {},
  }
}"#;
        let result = merge_github_server(input).expect("should produce output");
        // Must be valid JSON (no double commas).
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("output must be valid JSON");
        let servers = parsed["mcpServers"].as_object().unwrap();
        assert!(
            servers.contains_key("github"),
            "github entry must be present"
        );
        assert!(
            servers.contains_key("context7"),
            "existing keys must be preserved"
        );
    }

    #[test]
    fn merge_handles_no_trailing_comma() {
        let input = r#"{"mcpServers":{"context7":{}}}"#;
        let result = merge_github_server(input).expect("should produce output");
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("output must be valid JSON");
        let servers = parsed["mcpServers"].as_object().unwrap();
        assert!(servers.contains_key("github"));
        assert!(servers.contains_key("context7"));
    }

    #[test]
    fn merge_handles_empty_mcp_servers() {
        let input = r#"{"mcpServers":{}}"#;
        let result = merge_github_server(input).expect("should produce output");
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("output must be valid JSON");
        let servers = parsed["mcpServers"].as_object().unwrap();
        assert!(servers.contains_key("github"));
    }

    #[test]
    fn merge_handles_multiple_trailing_commas() {
        // Multiple entries each with trailing commas.
        let input = r#"{
  "mcpServers": {
    "context7": {},
    "markitdown": {},
  }
}"#;
        let result = merge_github_server(input).expect("should produce output");
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("output must be valid JSON");
        let servers = parsed["mcpServers"].as_object().unwrap();
        assert!(servers.contains_key("github"));
        assert!(servers.contains_key("context7"));
        assert!(servers.contains_key("markitdown"));
    }

    #[test]
    fn merge_top_level_with_trailing_comma() {
        // No mcpServers block — top-level insertion with trailing comma.
        let input = r#"{
  "someKey": "value",
}"#;
        let result = merge_github_server(input).expect("should produce output");
        assert!(!result.contains(",,"), "must not produce double commas");
        assert!(result.contains("\"github\""));
    }

    #[test]
    fn merge_handles_non_ascii_content_before_mcp_servers() {
        // Non-ASCII characters (a comment value) before the mcpServers key.
        // The byte-offset arithmetic must not panic on valid UTF-8.
        let input = "{\n  \"description\": \"Ünïcödé ☕\",\n  \"mcpServers\": {\n    \"context7\": {}\n  }\n}";
        let result = merge_github_server(input).expect("should produce output with non-ASCII");
        assert!(result.contains("\"github\""));
    }

    #[test]
    fn strip_trailing_commas_removes_comma_before_brace() {
        assert_eq!(strip_trailing_commas_in_object(r#""a":{},}"#), r#""a":{}}"#);
    }

    #[test]
    fn strip_trailing_commas_preserves_valid_commas() {
        let input = r#""a":{}, "b":{}}"#;
        assert_eq!(strip_trailing_commas_in_object(input), input);
    }

    #[test]
    fn strip_trailing_commas_removes_comma_before_bracket() {
        let input = r#"["a", "b",]"#;
        let result = strip_trailing_commas_in_object(input);
        assert_eq!(result, r#"["a", "b"]"#);
    }

    #[test]
    fn merge_handles_trailing_comma_in_array_value() {
        // JSONC with a trailing comma inside a nested array
        let input =
            r#"{"mcpServers": {"existing": {"command": "docker", "args": ["run", "--rm",]}}}"#;
        let result = merge_github_server(input);
        assert!(
            result.is_some(),
            "should parse JSONC with trailing comma in array args"
        );
        let out = result.unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&out).expect("merge_github_server output should be valid JSON");
        assert!(
            v["mcpServers"]["github"].is_object(),
            "github server should be inserted"
        );
        assert!(
            v["mcpServers"]["existing"].is_object(),
            "existing server should be preserved"
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
        fs::write(dir.path().join(".gitignore"), ".code-looper/\n").unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[0], BootstrapAction::AlreadySatisfied(_)));
        assert!(matches!(&actions[1], BootstrapAction::AlreadySatisfied(_)));
        assert!(matches!(&actions[2], BootstrapAction::AlreadySatisfied(_)));
    }

    // ── .gitignore tests ──────────────────────────────────────────────────────

    fn setup_satisfied_workspace(dir: &std::path::Path) {
        fs::write(
            dir.join("CLAUDE.md"),
            format!("{SECTION_BEGIN}\n{SECTION_END}\n"),
        )
        .unwrap();
        fs::write(dir.join(".mcp.json"), r#"{"mcpServers":{"github":{}}}"#).unwrap();
    }

    #[test]
    fn creates_gitignore_when_missing() {
        let dir = tmp();
        setup_satisfied_workspace(dir.path());
        let path = dir.path().join(".gitignore");
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[2], BootstrapAction::Created(p) if p == &path));
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains(".code-looper/runs/"),
            "must write narrow .code-looper/runs/ rule"
        );
        assert!(content.contains("# Code Looper"));
    }

    #[test]
    fn appends_to_existing_gitignore_without_entry() {
        let dir = tmp();
        setup_satisfied_workspace(dir.path());
        let path = dir.path().join(".gitignore");
        fs::write(&path, "node_modules/\n*.log\n").unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[2], BootstrapAction::Appended(p) if p == &path));
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("node_modules/"),
            "existing entries preserved"
        );
        assert!(
            content.contains(".code-looper/runs/"),
            "must write narrow .code-looper/runs/ rule"
        );
    }

    #[test]
    fn gitignore_with_entry_is_satisfied() {
        let dir = tmp();
        setup_satisfied_workspace(dir.path());
        let path = dir.path().join(".gitignore");
        fs::write(&path, "*.log\n.code-looper/\n").unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[2], BootstrapAction::AlreadySatisfied(p) if p == &path));
    }

    #[test]
    fn gitignore_with_entry_no_trailing_slash_is_satisfied() {
        let dir = tmp();
        setup_satisfied_workspace(dir.path());
        let path = dir.path().join(".gitignore");
        fs::write(&path, ".code-looper\n").unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[2], BootstrapAction::AlreadySatisfied(p) if p == &path));
    }

    #[test]
    fn second_bootstrap_is_idempotent_on_gitignore() {
        let dir = tmp();
        setup_satisfied_workspace(dir.path());
        run_bootstrap(dir.path(), false).unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[2], BootstrapAction::AlreadySatisfied(_)));
    }

    #[test]
    fn dry_run_does_not_create_gitignore() {
        let dir = tmp();
        setup_satisfied_workspace(dir.path());
        let path = dir.path().join(".gitignore");
        let actions = run_bootstrap(dir.path(), true).unwrap();
        assert!(matches!(&actions[2], BootstrapAction::Created(_)));
        assert!(!path.exists(), "dry-run must not create .gitignore");
    }

    #[test]
    fn gitignore_with_narrow_runs_entry_is_satisfied() {
        let dir = tmp();
        setup_satisfied_workspace(dir.path());
        let path = dir.path().join(".gitignore");
        fs::write(&path, ".code-looper/runs/\n").unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[2], BootstrapAction::AlreadySatisfied(p) if p == &path));
    }

    #[test]
    fn gitignore_with_narrow_runs_entry_no_trailing_slash_is_satisfied() {
        let dir = tmp();
        setup_satisfied_workspace(dir.path());
        let path = dir.path().join(".gitignore");
        fs::write(&path, ".code-looper/runs\n").unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[2], BootstrapAction::AlreadySatisfied(p) if p == &path));
    }

    // ── Broad-rule detection tests (#88) ─────────────────────────────────────

    #[test]
    fn has_broad_detects_legacy_broad_rule() {
        assert!(has_broad_code_looper_ignore(".code-looper/\n"));
        assert!(has_broad_code_looper_ignore(".code-looper\n"));
        assert!(has_broad_code_looper_ignore(
            "*.log\n.code-looper/\nnode_modules/\n"
        ));
    }

    #[test]
    fn has_broad_does_not_flag_narrow_rule() {
        assert!(!has_broad_code_looper_ignore(".code-looper/runs/\n"));
        assert!(!has_broad_code_looper_ignore(".code-looper/runs\n"));
    }

    #[test]
    fn warn_if_broad_ignore_no_config_file_returns_false() {
        let dir = tmp();
        // Broad rule in .gitignore but no config.toml — no warning.
        fs::write(dir.path().join(".gitignore"), ".code-looper/\n").unwrap();
        assert!(!warn_if_broad_ignore_hides_config(dir.path()));
    }

    #[test]
    fn warn_if_broad_ignore_with_config_file_returns_true() {
        let dir = tmp();
        fs::write(dir.path().join(".gitignore"), ".code-looper/\n").unwrap();
        fs::create_dir_all(dir.path().join(".code-looper")).unwrap();
        fs::write(dir.path().join(".code-looper/config.toml"), "").unwrap();
        assert!(warn_if_broad_ignore_hides_config(dir.path()));
    }

    #[test]
    fn warn_if_broad_ignore_with_yaml_config_file_returns_true() {
        let dir = tmp();
        fs::write(dir.path().join(".gitignore"), ".code-looper/\n").unwrap();
        fs::create_dir_all(dir.path().join(".code-looper")).unwrap();
        fs::write(dir.path().join(".code-looper/config.yaml"), "").unwrap();
        assert!(warn_if_broad_ignore_hides_config(dir.path()));
    }

    #[test]
    fn warn_if_narrow_ignore_with_config_file_returns_false() {
        let dir = tmp();
        fs::write(dir.path().join(".gitignore"), ".code-looper/runs/\n").unwrap();
        fs::create_dir_all(dir.path().join(".code-looper")).unwrap();
        fs::write(dir.path().join(".code-looper/config.toml"), "").unwrap();
        assert!(!warn_if_broad_ignore_hides_config(dir.path()));
    }

    // ── #172: Orphaned SECTION_BEGIN ──────────────────────────────────────────

    #[test]
    fn orphaned_section_begin_is_replaced_cleanly() {
        let dir = tmp();
        let path = dir.path().join("CLAUDE.md");
        // Simulate a partial/corrupt injection: SECTION_BEGIN without SECTION_END.
        fs::write(
            &path,
            format!("# Project\n\n{SECTION_BEGIN}\norphaned stuff\n"),
        )
        .unwrap();
        let actions = run_bootstrap(dir.path(), false).unwrap();
        assert!(matches!(&actions[0], BootstrapAction::Appended(_)));
        let content = fs::read_to_string(&path).unwrap();
        // Should have exactly one complete section now.
        assert_eq!(content.matches(SECTION_BEGIN).count(), 1);
        assert_eq!(content.matches(SECTION_END).count(), 1);
        assert!(content.contains("# Project"), "original content preserved");
        assert!(
            !content.contains("orphaned stuff"),
            "orphaned content removed"
        );
    }

    #[test]
    fn orphaned_section_begin_no_unbounded_growth() {
        let dir = tmp();
        let path = dir.path().join("CLAUDE.md");
        fs::write(&path, format!("# Project\n\n{SECTION_BEGIN}\norphaned\n")).unwrap();
        // Run bootstrap twice — should not accumulate multiple sections.
        run_bootstrap(dir.path(), false).unwrap();
        run_bootstrap(dir.path(), false).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches(SECTION_BEGIN).count(), 1);
        assert_eq!(content.matches(SECTION_END).count(), 1);
    }

    // ── #171: merge_github_server with serde_json ────────────────────────────

    #[test]
    fn merge_github_server_no_false_match_on_string_value() {
        // A JSON string value containing "mcpServers" should not confuse
        // the function (which previously used raw string search).
        let json = r#"{
  "description": "This file has mcpServers mentioned in a string",
  "mcpServers": {
    "context7": {}
  }
}"#;
        let result = merge_github_server(json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(
            parsed["mcpServers"]["github"].is_object(),
            "github entry should be inserted under mcpServers"
        );
        assert!(
            parsed["mcpServers"]["context7"].is_object(),
            "existing entries should be preserved"
        );
    }
}
