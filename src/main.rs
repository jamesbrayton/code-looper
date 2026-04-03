mod cli;
mod config;
mod error;
mod loop_engine;
mod provider;

use anyhow::Context;
use clap::Parser;
use tracing::info;

fn main() -> anyhow::Result<()> {
    let cli_args = cli::Cli::parse();

    // Determine base config: file-loaded or default.
    let base = if let Some(ref path) = cli_args.config {
        config::LoopConfig::from_toml_file(path)
            .with_context(|| format!("failed to load config from {}", path.display()))?
    } else {
        config::LoopConfig::default()
    };

    // Apply CLI overrides on top of base.
    let resolved = cli_args.apply_overrides(base);

    // Initialize tracing now that we have the log level.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&resolved.log_level)),
        )
        .init();

    // Validate resolved config.
    resolved.validate().context("invalid configuration")?;

    info!(
        provider = %resolved.provider,
        iterations = resolved.iterations,
        "Code Looper initialized"
    );

    // Build the loop engine, install signal handler, and run.
    let engine = loop_engine::LoopEngine::new(resolved);
    engine.install_signal_handler();
    engine.run();

    Ok(())
}
