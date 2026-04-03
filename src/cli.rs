use crate::config::{LoopConfig, Provider};
use clap::Parser;
use std::path::PathBuf;

/// Pluggable loop engine for coding-agent CLIs.
#[derive(Debug, Parser)]
#[command(name = "code-looper", version, about)]
pub struct Cli {
    /// Provider to use for loop execution.
    #[arg(long)]
    pub provider: Option<Provider>,

    /// Number of iterations (-1 for infinite looping).
    #[arg(long)]
    pub iterations: Option<i64>,

    /// Inline prompt string (mutually exclusive with --prompt-file).
    #[arg(long, conflicts_with = "prompt_file")]
    pub prompt_inline: Option<String>,

    /// Path to a markdown prompt file (mutually exclusive with --prompt-inline).
    #[arg(long, conflicts_with = "prompt_inline")]
    pub prompt_file: Option<PathBuf>,

    /// Path to a TOML config file to load as a base configuration.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Log level: trace, debug, info, warn, or error.
    #[arg(long)]
    pub log_level: Option<String>,

    /// Enable orchestration policy engine (auto-selects workflow branch from repo context).
    #[arg(long)]
    pub orchestration: bool,

    /// GitHub repository owner (required when --orchestration is set).
    #[arg(long)]
    pub repo_owner: Option<String>,

    /// GitHub repository name (required when --orchestration is set).
    #[arg(long)]
    pub repo_name: Option<String>,

    /// Directory to treat as the workspace root for prerequisite checks.
    /// Defaults to the current working directory.
    #[arg(long)]
    pub workspace_dir: Option<PathBuf>,

    /// Skip workspace prerequisite checks (instruction file, MCP config).
    /// Use only when you know the workspace is not a git repository or the
    /// checks are not applicable.
    #[arg(long)]
    pub skip_prereq_check: bool,

    /// [UNSAFE] Allow GitHub context resolution via direct `gh` CLI calls
    /// instead of requiring a GitHub MCP server.  Disables the MCP-only
    /// write-path enforcement preamble in provider prompts.
    #[arg(long)]
    pub allow_direct_github: bool,
}

impl Cli {
    /// Merge CLI overrides onto a base `LoopConfig`.
    ///
    /// The base is either `LoopConfig::default()` or values loaded from a
    /// TOML config file; any CLI flag that is explicitly set takes precedence.
    pub fn apply_overrides(self, mut base: LoopConfig) -> LoopConfig {
        if let Some(p) = self.provider {
            base.provider = p;
        }
        if let Some(i) = self.iterations {
            base.iterations = i;
        }
        if let Some(s) = self.prompt_inline {
            base.prompt_inline = Some(s);
            base.prompt_file = None;
        }
        if let Some(f) = self.prompt_file {
            base.prompt_file = Some(f);
            base.prompt_inline = None;
        }
        if let Some(l) = self.log_level {
            base.log_level = l;
        }
        if self.orchestration {
            base.orchestration.enabled = true;
        }
        if let Some(owner) = self.repo_owner {
            base.orchestration.repo_owner = Some(owner);
        }
        if let Some(name) = self.repo_name {
            base.orchestration.repo_name = Some(name);
        }
        if let Some(dir) = self.workspace_dir {
            base.workspace_dir = Some(dir);
        }
        if self.skip_prereq_check {
            base.skip_prereq_check = true;
        }
        if self.allow_direct_github {
            base.allow_direct_github = true;
        }
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;

    fn default_config() -> LoopConfig {
        LoopConfig::default()
    }

    fn blank_cli() -> Cli {
        Cli {
            provider: None,
            iterations: None,
            prompt_inline: None,
            prompt_file: None,
            config: None,
            log_level: None,
            orchestration: false,
            repo_owner: None,
            repo_name: None,
            workspace_dir: None,
            skip_prereq_check: false,
            allow_direct_github: false,
        }
    }

    #[test]
    fn cli_overrides_provider() {
        let cli = Cli { provider: Some(Provider::Copilot), ..blank_cli() };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.provider, Provider::Copilot);
    }

    #[test]
    fn cli_overrides_iterations() {
        let cli = Cli { iterations: Some(10), ..blank_cli() };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.iterations, 10);
    }

    #[test]
    fn cli_inline_prompt_clears_file_prompt() {
        let mut base = default_config();
        base.prompt_file = Some("old.md".into());

        let cli = Cli { prompt_inline: Some("new prompt".to_string()), ..blank_cli() };
        let config = cli.apply_overrides(base);
        assert_eq!(config.prompt_inline.as_deref(), Some("new prompt"));
        assert!(config.prompt_file.is_none());
    }

    #[test]
    fn cli_no_overrides_preserves_base() {
        let mut base = default_config();
        base.iterations = 7;
        base.log_level = "debug".to_string();

        let config = blank_cli().apply_overrides(base);
        assert_eq!(config.iterations, 7);
        assert_eq!(config.log_level, "debug");
    }

    #[test]
    fn cli_orchestration_flag_enables_engine() {
        let cli = Cli {
            orchestration: true,
            repo_owner: Some("acme".to_string()),
            repo_name: Some("project".to_string()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert!(config.orchestration.enabled);
        assert_eq!(config.orchestration.repo_owner.as_deref(), Some("acme"));
        assert_eq!(config.orchestration.repo_name.as_deref(), Some("project"));
    }

    #[test]
    fn cli_orchestration_false_leaves_config_disabled() {
        let config = blank_cli().apply_overrides(default_config());
        assert!(!config.orchestration.enabled);
    }

    #[test]
    fn cli_skip_prereq_check_sets_flag() {
        let cli = Cli { skip_prereq_check: true, ..blank_cli() };
        let config = cli.apply_overrides(default_config());
        assert!(config.skip_prereq_check);
    }

    #[test]
    fn cli_allow_direct_github_sets_flag() {
        let cli = Cli { allow_direct_github: true, ..blank_cli() };
        let config = cli.apply_overrides(default_config());
        assert!(config.allow_direct_github);
    }

    #[test]
    fn cli_workspace_dir_propagates() {
        let cli = Cli {
            workspace_dir: Some("/tmp/my-repo".into()),
            ..blank_cli()
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.workspace_dir, Some("/tmp/my-repo".into()));
    }

    #[test]
    fn cli_defaults_leave_safe_flags_false() {
        let config = blank_cli().apply_overrides(default_config());
        assert!(!config.skip_prereq_check);
        assert!(!config.allow_direct_github);
        assert!(config.workspace_dir.is_none());
    }
}
