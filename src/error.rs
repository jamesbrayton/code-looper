use thiserror::Error;

#[derive(Debug, Error)]
pub enum LooperError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
}
