mod bootstrap;
mod branch;
mod cli;
mod config;
mod error;
mod issue_tracker;
mod loop_engine;
mod multi_repo;
mod orchestration;
mod policy_guard;
mod pr_manager;
mod pr_strategy;
mod provider;
mod security;
mod service;
mod telemetry;
mod workspace;

use anyhow::Context;
use clap::Parser;
use tracing::info;

fn main() -> anyhow::Result<()> {
    let cli_args = cli::Cli::parse();

    // Dispatch subcommands before config resolution so they can run without a
    // full loop configuration.
    match cli_args.command {
        Some(cli::Commands::Bootstrap {
            workspace_dir,
            dry_run,
        }) => {
            let ws_dir = workspace::resolve_workspace_dir(workspace_dir.as_deref());
            let prefix = if dry_run { "[dry-run]" } else { "[bootstrap]" };
            let actions = bootstrap::run_bootstrap(&ws_dir, dry_run).context("bootstrap failed")?;
            for action in &actions {
                let msg = action.to_string();
                let display = if dry_run {
                    msg.replacen("[bootstrap]", prefix, 1)
                } else {
                    msg
                };
                println!("{display}");
            }
            let all_satisfied = actions
                .iter()
                .all(|a| matches!(a, bootstrap::BootstrapAction::AlreadySatisfied(_)));
            if all_satisfied {
                println!("{prefix} workspace prerequisites already satisfied — nothing to do.");
            } else if !dry_run {
                println!(
                    "[bootstrap] Done — workspace prerequisites satisfied. \
                     Run \"code-looper --help\" to get started."
                );
            }
            return Ok(());
        }

        Some(cli::Commands::Serve {
            port,
            ref bind_addr,
            unsafe_bind,
        }) => {
            let bind_addr = bind_addr.clone();
            // Build config from file / CLI overrides, then hand off to service mode.
            let base = if let Some(ref path) = cli_args.config {
                config::LoopConfig::from_file(path)
                    .with_context(|| format!("failed to load config from {}", path.display()))?
            } else {
                config::LoopConfig::default()
            };
            let resolved = cli_args.apply_overrides(base);

            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                        tracing_subscriber::EnvFilter::new(&resolved.log_level)
                    }),
                )
                .init();

            info!(
                port = port,
                bind_addr = %bind_addr,
                unsafe_bind = unsafe_bind,
                "Starting service mode"
            );
            let svc = service::ServiceMode::new(resolved, bind_addr, port, unsafe_bind);
            return svc.run();
        }

        None => {}
    }

    // Determine base config: file-loaded or default.
    let base = if let Some(ref path) = cli_args.config {
        config::LoopConfig::from_file(path)
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
        let ws_dir = workspace::resolve_workspace_dir(resolved.workspace_dir.as_deref());
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
    let violations = guard.check_startup(resolved.orchestration.enabled);
    if !violations.is_empty() {
        for v in &violations {
            eprintln!("{v}");
        }
        anyhow::bail!("Policy guard validation failed");
    }

    // ── Multi-repo mode ──────────────────────────────────────────────────────
    // When `multi_repo` entries are present, run the loop for each target in
    // sequence and print a combined summary.  The single-repo path is skipped.
    if !resolved.multi_repo.is_empty() {
        info!(
            provider = %resolved.provider,
            repos = resolved.multi_repo.len(),
            "Code Looper initializing in multi-repo mode"
        );
        let targets = resolved.multi_repo.clone();
        let results = multi_repo::run_multi_repo(&resolved, &targets);
        multi_repo::print_multi_repo_summary(&results);
        return Ok(());
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
