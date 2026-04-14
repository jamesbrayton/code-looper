//! Config bootstrap subcommand — scaffold the `.code-looper/` configuration
//! directory with annotated defaults and example rule files.
//!
//! This is **distinct** from the workspace bootstrap (`code-looper bootstrap`),
//! which prepares a repository for any Code Looper run.  Config bootstrap
//! prepares a specific configuration layout under `.code-looper/` (or a
//! user-specified directory).

use std::path::{Path, PathBuf};

/// A single action taken (or that would be taken) by config bootstrap.
#[derive(Debug, PartialEq)]
pub enum ConfigBootstrapAction {
    /// Created a new file at the given path.
    Created(PathBuf),
    /// Created a new directory at the given path.
    CreatedDir(PathBuf),
    /// The file already exists; not overwritten without `--force`.
    AlreadySatisfied(PathBuf),
    /// Overwrote an existing file (when `--force` was set).
    Overwritten(PathBuf),
}

impl std::fmt::Display for ConfigBootstrapAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigBootstrapAction::Created(p) => {
                write!(f, "[config bootstrap] {}: created", p.display())
            }
            ConfigBootstrapAction::CreatedDir(p) => {
                write!(f, "[config bootstrap] {}: directory created", p.display())
            }
            ConfigBootstrapAction::AlreadySatisfied(p) => {
                write!(
                    f,
                    "[config bootstrap] {}: already exists (use --force to overwrite)",
                    p.display()
                )
            }
            ConfigBootstrapAction::Overwritten(p) => {
                write!(f, "[config bootstrap] {}: overwritten", p.display())
            }
        }
    }
}

/// Supported config file formats for the scaffolded config file.
#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum ConfigFormat {
    Toml,
    Yaml,
}

impl std::fmt::Display for ConfigFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigFormat::Toml => write!(f, "toml"),
            ConfigFormat::Yaml => write!(f, "yaml"),
        }
    }
}

// ── Scaffolded file contents ─────────────────────────────────────────────────

/// Annotated config.toml template with all fields commented and documented.
const CONFIG_TOML_TEMPLATE: &str = r#"# Code Looper configuration
# See: https://github.com/jamesbrayton/code-looper/blob/main/docs/configuration.md

# ── Provider ─────────────────────────────────────────────────────────────
# Agent CLI to use for each iteration: "claude", "copilot", or "codex".
provider = "claude"

# Number of iterations to run. Use -1 for infinite (until interrupted).
iterations = 1

# ── Prompt ───────────────────────────────────────────────────────────────
# Provide a prompt inline or via a file (mutually exclusive).
# prompt_inline = "Work on the next open issue."
# prompt_file = ".code-looper/prompts/example.md"

# ── User rules ───────────────────────────────────────────────────────────
# Global rules prepended to every prompt (all workflow branches).
# Workflow-specific rules prepended only for matching branches.
# See: docs/prompt-injection.md for the full layering order.
#
# [rules]
# global = ".code-looper/rules/global.md"
#
# [rules.workflows]
# pr_review = ".code-looper/rules/pr-review.md"
# issue_execution = ".code-looper/rules/issue-execution.md"
# backlog_discovery = ".code-looper/rules/backlog-discovery.md"

# ── Orchestration ────────────────────────────────────────────────────────
# Enable the policy engine to auto-select workflow branches.
# [orchestration]
# enabled = true
# repo_owner = "your-org"
# repo_name = "your-repo"

# ── Issue tracking ───────────────────────────────────────────────────────
# [issue_tracking]
# mode = "github"     # "github" or "local"
# repo_owner = ""     # Falls back to orchestration.repo_owner
# repo_name = ""      # Falls back to orchestration.repo_name

# ── PR management ────────────────────────────────────────────────────────
# [pr_management]
# mode = "single-pr"  # "single-pr" or "multi-pr"
# base_branch = "main"

# ── Retry / backoff ──────────────────────────────────────────────────────
# max_retries = 0
# retry_backoff_ms = 500
# retry_backoff_multiplier = 1.0
# stop_on_failure = false
# iteration_timeout_secs = 0
# non_retryable_exit_codes = []

# ── Telemetry ────────────────────────────────────────────────────────────
# [telemetry]
# stream_output = true
# log_dir = ".code-looper/runs"

# ── Completion hook ──────────────────────────────────────────────────────
# on_complete = "echo 'Loop finished'"
"#;

/// Annotated config.yaml template.
const CONFIG_YAML_TEMPLATE: &str = r#"# Code Looper configuration
# See: https://github.com/jamesbrayton/code-looper/blob/main/docs/configuration.md

# ── Provider ─────────────────────────────────────────────────────────────
# Agent CLI to use: "claude", "copilot", or "codex".
provider: claude

# Number of iterations (-1 for infinite).
iterations: 1

# ── Prompt ───────────────────────────────────────────────────────────────
# prompt_inline: "Work on the next open issue."
# prompt_file: ".code-looper/prompts/example.md"

# ── User rules ───────────────────────────────────────────────────────────
# rules:
#   global: ".code-looper/rules/global.md"
#   workflows:
#     pr_review: ".code-looper/rules/pr-review.md"
#     issue_execution: ".code-looper/rules/issue-execution.md"
#     backlog_discovery: ".code-looper/rules/backlog-discovery.md"

# ── Orchestration ────────────────────────────────────────────────────────
# orchestration:
#   enabled: true
#   repo_owner: your-org
#   repo_name: your-repo

# ── Issue tracking ───────────────────────────────────────────────────────
# issue_tracking:
#   mode: github

# ── PR management ────────────────────────────────────────────────────────
# pr_management:
#   mode: single-pr
#   base_branch: main

# ── Retry / backoff ──────────────────────────────────────────────────────
# max_retries: 0
# retry_backoff_ms: 500
# stop_on_failure: false
"#;

/// Example prompt file content.
const EXAMPLE_PROMPT: &str = r#"# Example prompt file for Code Looper
#
# Reference this file with:
#   prompt_file = ".code-looper/prompts/example.md"
#
# Or pass it on the CLI:
#   code-looper --prompt-file .code-looper/prompts/example.md

Work on open GitHub issues in this repository. Pick the highest-priority
unassigned issue, implement the changes, and update the issue when done.
"#;

/// Example global rules file.
const EXAMPLE_GLOBAL_RULES: &str = r#"# Global rules — prepended to every provider prompt
#
# Rename this file to global.md to activate it.
# See: docs/prompt-injection.md for the full prompt layering order.

## Coding standards

- Follow the project's existing code style and conventions.
- Write tests for new functionality.
- Keep commits focused and well-described.

## Communication

- Comment on the linked issue at meaningful milestones.
- If you discover out-of-scope work, create a new issue rather than expanding scope.
"#;

/// Example PR review rules file.
const EXAMPLE_PR_REVIEW_RULES: &str = r#"# PR review rules — prepended when the workflow branch is pr-review
#
# Rename this file to pr-review.md to activate it.

## Review checklist

- Check for correctness, edge cases, and error handling.
- Verify tests cover the changes.
- Look for security issues (injection, auth bypass, data exposure).
- Ensure documentation is updated if behaviour changes.
"#;

/// Example issue execution rules file.
const EXAMPLE_ISSUE_EXECUTION_RULES: &str = r#"# Issue execution rules — prepended when the workflow branch is issue-execution
#
# Rename this file to issue-execution.md to activate it.

## Implementation guidelines

- Read the full issue description before starting.
- Break large changes into smaller, reviewable commits.
- Update the issue checklist as you make progress.
"#;

/// Example backlog discovery rules file.
const EXAMPLE_BACKLOG_DISCOVERY_RULES: &str = r#"# Backlog discovery rules — prepended when the workflow branch is backlog-discovery
#
# Rename this file to backlog-discovery.md to activate it.

## Discovery priorities

- Missing or incomplete test coverage.
- Documentation gaps.
- Code quality improvements (dead code, duplication, complexity).
- Security hardening opportunities.
"#;

/// Example multi-PR triage rules file.
const EXAMPLE_MULTI_PR_TRIAGE_RULES: &str = r#"# Multi-PR triage rules — prepended when the workflow branch is multi-pr-triage
#
# Rename this file to multi-pr-triage.md to activate it.

## Triage guidelines

- Prioritise PRs with failing checks or merge conflicts.
- Review the oldest PRs first unless a newer one is blocking.
- Leave actionable feedback — avoid vague comments.
"#;

/// Run the config bootstrap process.
///
/// Scaffolds the `.code-looper/` directory structure with annotated config,
/// example prompts, and example rule files.
///
/// When `dry_run` is `true`, returns the actions that would be taken without
/// writing to disk.  When `force` is `true`, existing files are overwritten.
pub fn run_config_bootstrap(
    dir: &Path,
    format: ConfigFormat,
    dry_run: bool,
    force: bool,
) -> anyhow::Result<Vec<ConfigBootstrapAction>> {
    let mut actions = Vec::new();

    // Create subdirectories.
    for subdir in &["prompts", "rules", "runs"] {
        let path = dir.join(subdir);
        if path.is_dir() {
            actions.push(ConfigBootstrapAction::AlreadySatisfied(path));
        } else {
            if !dry_run {
                std::fs::create_dir_all(&path)?;
            }
            actions.push(ConfigBootstrapAction::CreatedDir(path));
        }
    }

    // Config file.
    let (config_filename, config_content) = match format {
        ConfigFormat::Toml => ("config.toml", CONFIG_TOML_TEMPLATE),
        ConfigFormat::Yaml => ("config.yaml", CONFIG_YAML_TEMPLATE),
    };
    actions.push(write_scaffold_file(
        &dir.join(config_filename),
        config_content,
        dry_run,
        force,
    )?);

    // Example prompt.
    actions.push(write_scaffold_file(
        &dir.join("prompts/example.md"),
        EXAMPLE_PROMPT,
        dry_run,
        force,
    )?);

    // Example rule files (with .example suffix — user renames to activate).
    let rule_files: &[(&str, &str)] = &[
        ("rules/global.md.example", EXAMPLE_GLOBAL_RULES),
        ("rules/pr-review.md.example", EXAMPLE_PR_REVIEW_RULES),
        (
            "rules/issue-execution.md.example",
            EXAMPLE_ISSUE_EXECUTION_RULES,
        ),
        (
            "rules/backlog-discovery.md.example",
            EXAMPLE_BACKLOG_DISCOVERY_RULES,
        ),
        (
            "rules/multi-pr-triage.md.example",
            EXAMPLE_MULTI_PR_TRIAGE_RULES,
        ),
    ];
    for (name, content) in rule_files {
        actions.push(write_scaffold_file(
            &dir.join(name),
            content,
            dry_run,
            force,
        )?);
    }

    Ok(actions)
}

/// Build the "next steps" message printed after a successful config bootstrap.
pub fn next_steps_message(dir: &Path, format: ConfigFormat) -> String {
    let config_filename = match format {
        ConfigFormat::Toml => "config.toml",
        ConfigFormat::Yaml => "config.yaml",
    };
    let config_path = dir.join(config_filename);
    format!(
        "\nAll set up. Once you are satisfied with your config, run:\n\n    \
         code-looper --config {}\n",
        config_path.display()
    )
}

/// Write a scaffold file, respecting dry_run and force flags.
fn write_scaffold_file(
    path: &Path,
    content: &str,
    dry_run: bool,
    force: bool,
) -> anyhow::Result<ConfigBootstrapAction> {
    if path.exists() && !force {
        return Ok(ConfigBootstrapAction::AlreadySatisfied(path.to_path_buf()));
    }
    let overwriting = path.exists() && force;
    if !dry_run {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
    }
    if overwriting {
        Ok(ConfigBootstrapAction::Overwritten(path.to_path_buf()))
    } else {
        Ok(ConfigBootstrapAction::Created(path.to_path_buf()))
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn scaffolds_full_directory_structure() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".code-looper");
        let actions = run_config_bootstrap(&dir, ConfigFormat::Toml, false, false).unwrap();

        // Verify directories exist.
        assert!(dir.join("prompts").is_dir());
        assert!(dir.join("rules").is_dir());
        assert!(dir.join("runs").is_dir());

        // Verify files exist.
        assert!(dir.join("config.toml").is_file());
        assert!(dir.join("prompts/example.md").is_file());
        assert!(dir.join("rules/global.md.example").is_file());
        assert!(dir.join("rules/pr-review.md.example").is_file());
        assert!(dir.join("rules/issue-execution.md.example").is_file());
        assert!(dir.join("rules/backlog-discovery.md.example").is_file());
        assert!(dir.join("rules/multi-pr-triage.md.example").is_file());

        // Should have 3 dirs + 1 config + 1 prompt + 5 rules = 10 actions.
        assert_eq!(actions.len(), 10);
    }

    #[test]
    fn idempotent_reports_already_satisfied() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".code-looper");

        // First run creates everything.
        run_config_bootstrap(&dir, ConfigFormat::Toml, false, false).unwrap();

        // Second run reports AlreadySatisfied for everything.
        let actions = run_config_bootstrap(&dir, ConfigFormat::Toml, false, false).unwrap();
        for action in &actions {
            assert!(
                matches!(action, ConfigBootstrapAction::AlreadySatisfied(_)),
                "expected AlreadySatisfied, got: {action}"
            );
        }
    }

    #[test]
    fn force_overwrites_existing_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".code-looper");

        // First run.
        run_config_bootstrap(&dir, ConfigFormat::Toml, false, false).unwrap();

        // Modify a file to confirm it gets overwritten.
        std::fs::write(dir.join("config.toml"), "modified").unwrap();

        // Second run with force.
        let actions = run_config_bootstrap(&dir, ConfigFormat::Toml, false, true).unwrap();
        let overwritten_count = actions
            .iter()
            .filter(|a| matches!(a, ConfigBootstrapAction::Overwritten(_)))
            .count();
        assert!(overwritten_count > 0, "expected at least one overwrite");

        // Config file should be restored to the template.
        let content = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(content.contains("Code Looper configuration"));
    }

    #[test]
    fn dry_run_writes_nothing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".code-looper");
        let actions = run_config_bootstrap(&dir, ConfigFormat::Toml, true, false).unwrap();

        // Directory should not exist.
        assert!(!dir.exists());

        // But we should still get planned actions.
        assert!(!actions.is_empty());
    }

    #[test]
    fn yaml_format_creates_yaml_config() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".code-looper");
        run_config_bootstrap(&dir, ConfigFormat::Yaml, false, false).unwrap();

        assert!(dir.join("config.yaml").is_file());
        assert!(!dir.join("config.toml").exists());

        let content = std::fs::read_to_string(dir.join("config.yaml")).unwrap();
        assert!(content.contains("provider: claude"));
    }

    #[test]
    fn next_steps_message_includes_config_path() {
        let msg = next_steps_message(Path::new(".code-looper"), ConfigFormat::Toml);
        assert!(msg.contains("code-looper --config .code-looper/config.toml"));

        let msg = next_steps_message(
            Path::new("/home/user/.config/code-looper"),
            ConfigFormat::Yaml,
        );
        assert!(msg.contains("code-looper --config /home/user/.config/code-looper/config.yaml"));
    }

    #[test]
    fn toml_template_is_valid_toml() {
        // The template should parse as valid TOML (all uncommented lines).
        let result: Result<toml::Value, _> = toml::from_str(CONFIG_TOML_TEMPLATE);
        assert!(result.is_ok(), "TOML template is invalid: {result:?}");
    }

    #[test]
    fn partial_directory_is_filled_in() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".code-looper");

        // Create only the rules directory.
        std::fs::create_dir_all(dir.join("rules")).unwrap();

        let actions = run_config_bootstrap(&dir, ConfigFormat::Toml, false, false).unwrap();

        // rules/ should be AlreadySatisfied, but prompts/ and runs/ should be CreatedDir.
        let created_dirs: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, ConfigBootstrapAction::CreatedDir(_)))
            .collect();
        assert_eq!(created_dirs.len(), 2, "expected 2 new dirs (prompts, runs)");

        // All files should be Created.
        let created_files = actions
            .iter()
            .filter(|a| matches!(a, ConfigBootstrapAction::Created(_)))
            .count();
        assert_eq!(created_files, 7, "expected 7 new files");
    }
}
