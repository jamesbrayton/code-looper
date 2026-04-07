use thiserror::Error;

#[derive(Debug, Error)]
pub enum LooperError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("failed to spawn provider binary '{binary}': {source}")]
    ProviderSpawn {
        binary: String,
        #[source]
        source: std::io::Error,
    },

    #[error("provider '{binary}' timed out after {timeout_secs}s")]
    ProviderTimeout { binary: String, timeout_secs: u64 },

    #[error(
        "executable '{binary}' is not in the provider allowlist; permitted binaries: {allowed}"
    )]
    DisallowedExecutable { binary: String, allowed: String },
}
