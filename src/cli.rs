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

    #[test]
    fn cli_overrides_provider() {
        let cli = Cli {
            provider: Some(Provider::Copilot),
            iterations: None,
            prompt_inline: None,
            prompt_file: None,
            config: None,
            log_level: None,
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.provider, Provider::Copilot);
    }

    #[test]
    fn cli_overrides_iterations() {
        let cli = Cli {
            provider: None,
            iterations: Some(10),
            prompt_inline: None,
            prompt_file: None,
            config: None,
            log_level: None,
        };
        let config = cli.apply_overrides(default_config());
        assert_eq!(config.iterations, 10);
    }

    #[test]
    fn cli_inline_prompt_clears_file_prompt() {
        let mut base = default_config();
        base.prompt_file = Some("old.md".into());

        let cli = Cli {
            provider: None,
            iterations: None,
            prompt_inline: Some("new prompt".to_string()),
            prompt_file: None,
            config: None,
            log_level: None,
        };
        let config = cli.apply_overrides(base);
        assert_eq!(config.prompt_inline.as_deref(), Some("new prompt"));
        assert!(config.prompt_file.is_none());
    }

    #[test]
    fn cli_no_overrides_preserves_base() {
        let mut base = default_config();
        base.iterations = 7;
        base.log_level = "debug".to_string();

        let cli = Cli {
            provider: None,
            iterations: None,
            prompt_inline: None,
            prompt_file: None,
            config: None,
            log_level: None,
        };
        let config = cli.apply_overrides(base);
        assert_eq!(config.iterations, 7);
        assert_eq!(config.log_level, "debug");
    }
}
