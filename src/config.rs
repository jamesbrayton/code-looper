use crate::error::LooperError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Supported agent CLI providers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Copilot,
    Codex,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Claude => write!(f, "claude"),
            Provider::Copilot => write!(f, "copilot"),
            Provider::Codex => write!(f, "codex"),
        }
    }
}

/// Resolved runtime configuration for a single loop run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopConfig {
    /// Provider to use for loop execution.
    pub provider: Provider,
    /// Number of iterations; -1 means infinite.
    pub iterations: i64,
    /// Inline prompt string (mutually exclusive with prompt_file).
    pub prompt_inline: Option<String>,
    /// Path to a markdown prompt file (mutually exclusive with prompt_inline).
    pub prompt_file: Option<PathBuf>,
    /// Tracing log level (e.g. "info", "debug").
    pub log_level: String,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            provider: Provider::Claude,
            iterations: 1,
            prompt_inline: None,
            prompt_file: None,
            log_level: "info".to_string(),
        }
    }
}

impl LoopConfig {
    /// Load config from a TOML file.
    pub fn from_toml_file(path: &std::path::Path) -> Result<Self, LooperError> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Validate that the config is internally consistent.
    pub fn validate(&self) -> Result<(), LooperError> {
        if self.prompt_inline.is_some() && self.prompt_file.is_some() {
            return Err(LooperError::InvalidArgument(
                "--prompt-inline and --prompt-file are mutually exclusive".to_string(),
            ));
        }
        if self.iterations < -1 || self.iterations == 0 {
            return Err(LooperError::InvalidArgument(
                "--iterations must be a positive integer or -1 for infinite".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn default_config_is_valid() {
        assert!(LoopConfig::default().validate().is_ok());
    }

    #[test]
    fn conflicting_prompts_are_invalid() {
        let config = LoopConfig {
            prompt_inline: Some("hello".to_string()),
            prompt_file: Some("prompt.md".into()),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn zero_iterations_is_invalid() {
        let config = LoopConfig {
            iterations: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn negative_two_iterations_is_invalid() {
        let config = LoopConfig {
            iterations: -2,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn negative_one_iterations_is_valid() {
        let config = LoopConfig {
            iterations: -1,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn parse_toml_config_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "copilot"
iterations = 5
log_level = "debug"
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.provider, Provider::Copilot);
        assert_eq!(config.iterations, 5);
        assert_eq!(config.log_level, "debug");
    }

    #[test]
    fn parse_toml_with_inline_prompt() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
provider = "codex"
iterations = 3
log_level = "info"
prompt_inline = "run the tests"
"#
        )
        .unwrap();
        let config = LoopConfig::from_toml_file(file.path()).unwrap();
        assert_eq!(config.provider, Provider::Codex);
        assert_eq!(config.prompt_inline.as_deref(), Some("run the tests"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn provider_display() {
        assert_eq!(Provider::Claude.to_string(), "claude");
        assert_eq!(Provider::Copilot.to_string(), "copilot");
        assert_eq!(Provider::Codex.to_string(), "codex");
    }
}
