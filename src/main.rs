mod cli;
mod config;
mod error;
mod issue_tracker;
mod loop_engine;
mod orchestration;
mod policy_guard;
mod provider;
mod workspace;

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

    // Run workspace prerequisite checks unless explicitly skipped.
    if !resolved.skip_prereq_check {
        let ws_dir =
            workspace::resolve_workspace_dir(resolved.workspace_dir.as_deref());
        let checker = workspace::PrerequisiteChecker::new(&ws_dir);
        let check_result = checker.run();
        if !check_result.is_ok() {
            eprintln!("Workspace prerequisite checks failed:");
            check_result.print_summary();
            eprintln!(
                "\nRun with --skip-prereq-check to bypass (not recommended), or \
                 fix the issues above before running Code Looper."
            );
            std::process::exit(1);
        }
        info!(workspace = %ws_dir.display(), "Workspace prerequisite checks passed");
    }

    // Validate orchestration policy and build the guard.
    let guard = policy_guard::PolicyGuard::new(policy_guard::UnsafeOverrides {
        allow_direct_github: resolved.allow_direct_github,
    });
    let violations = guard.validate_orchestration(resolved.orchestration.enabled);
    if !violations.is_empty() {
        for v in &violations {
            eprintln!("{v}");
        }
        anyhow::bail!("Policy guard validation failed");
    }

    info!(
        provider = %resolved.provider,
        iterations = resolved.iterations,
        "Code Looper initialized"
    );

    // Build the loop engine, install signal handler, and run.
    let engine = loop_engine::LoopEngine::new(resolved, guard);
    engine.install_signal_handler();
    engine.run();

    Ok(())
}
