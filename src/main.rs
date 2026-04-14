mod bootstrap;
mod branch;
mod cli;
mod config;
mod config_bootstrap;
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
use std::path::PathBuf;
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
            // Warn if a broad .code-looper/ rule hides config files (#88).
            bootstrap::warn_if_broad_ignore_hides_config(&ws_dir);
            return Ok(());
        }

        Some(cli::Commands::Config(cli::ConfigCommands::Bootstrap {
            format,
            dir,
            dry_run,
            force,
        })) => {
            let target_dir = dir.unwrap_or_else(|| PathBuf::from(".code-looper"));
            let prefix = if dry_run {
                "[dry-run]"
            } else {
                "[config bootstrap]"
            };
            let actions =
                config_bootstrap::run_config_bootstrap(&target_dir, format, dry_run, force)
                    .context("config bootstrap failed")?;
            for action in &actions {
                let msg = action.to_string();
                let display = if dry_run {
                    msg.replacen("[config bootstrap]", prefix, 1)
                } else {
                    msg
                };
                println!("{display}");
            }
            let all_satisfied = actions.iter().all(|a| {
                matches!(
                    a,
                    config_bootstrap::ConfigBootstrapAction::AlreadySatisfied(_)
                )
            });
            if all_satisfied {
                println!("{prefix} all config files already exist — nothing to do.");
            } else if !dry_run {
                // Print next-steps message with the exact command to run.
                for line in config_bootstrap::next_steps_message(&target_dir, format).lines() {
                    println!("{line}");
                }
            }
            return Ok(());
        }

        Some(cli::Commands::Serve {
            port,
            ref bind_addr,
            unsafe_bind,
        }) => {
            let bind_addr = bind_addr.clone();
            // Three-tier config resolution (same as the loop path).
            let serve_ws = workspace::resolve_workspace_dir(cli_args.workspace_dir.as_deref());
            let (base, serve_config_source) = if let Some((path, tier)) =
                config::resolve_config_path(cli_args.config.as_deref(), &serve_ws)
            {
                let mut cfg = config::LoopConfig::from_file(&path)
                    .with_context(|| format!("failed to load config from {}", path.display()))?;
                config::resolve_rule_paths(&mut cfg.rules, &path);
                (cfg, Some((path, tier)))
            } else {
                (config::LoopConfig::default(), None)
            };
            let mut resolved = cli_args.apply_overrides(base);

            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                        tracing_subscriber::EnvFilter::new(&resolved.log_level)
                    }),
                )
                .init();

            // Log config source after tracing is initialized.
            if let Some((ref path, tier)) = serve_config_source {
                info!(
                    config = %path.display(),
                    tier = tier,
                    "Config loaded"
                );
            } else {
                info!("No config file found at any tier; using built-in defaults");
            }

            // Fill in repo_owner/repo_name from git remote (same as the loop path).
            resolved.resolve_git_defaults();

            let validated = resolved
                .validate()
                .context("invalid configuration for service mode")?;

            info!(
                port = port,
                bind_addr = %bind_addr,
                unsafe_bind = unsafe_bind,
                "Starting service mode"
            );
            let svc = service::ServiceMode::new(validated, bind_addr, port, unsafe_bind);
            return svc.run();
        }

        None => {}
    }

    // Three-tier config resolution: CLI flag → workspace → user directory.
    let ws_dir = workspace::resolve_workspace_dir(cli_args.workspace_dir.as_deref());
    let (base, config_source) = if let Some((path, tier)) =
        config::resolve_config_path(cli_args.config.as_deref(), &ws_dir)
    {
        let mut cfg = config::LoopConfig::from_file(&path)
            .with_context(|| format!("failed to load config from {}", path.display()))?;
        // Resolve rule file paths relative to the config file, not CWD.
        config::resolve_rule_paths(&mut cfg.rules, &path);
        (cfg, Some((path, tier)))
    } else {
        (config::LoopConfig::default(), None)
    };

    // Apply CLI overrides on top of base.
    let mut resolved = cli_args.apply_overrides(base);

    // Initialize tracing now that we have the log level.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&resolved.log_level)),
        )
        .init();

    // Log config source after tracing is initialized.
    if let Some((ref path, tier)) = config_source {
        info!(
            config = %path.display(),
            tier = tier,
            "Config loaded"
        );
    } else {
        info!("No config file found at any tier; using built-in defaults");
    }

    // Fill in repo_owner/repo_name from git remote if not set explicitly.
    resolved.resolve_git_defaults();

    // Validate resolved config — returns a ValidatedLoopConfig with refined types.
    let validated = resolved.validate().context("invalid configuration")?;

    // Run workspace prerequisite checks unless explicitly skipped.
    if !validated.skip_prereq_check {
        let ws_dir = workspace::resolve_workspace_dir(validated.workspace_dir.as_deref());
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
        // Warn if a broad .code-looper/ rule hides config files (#88).
        bootstrap::warn_if_broad_ignore_hides_config(&ws_dir);
    }

    // Validate orchestration policy and build the guard.
    let guard = policy_guard::PolicyGuard::new(policy_guard::UnsafeOverrides {
        allow_direct_github: validated.allow_direct_github,
    });
    let violations = guard.check_startup(validated.orchestration.enabled);
    if !violations.is_empty() {
        for v in &violations {
            eprintln!("{v}");
        }
        anyhow::bail!("Policy guard validation failed");
    }

    // ── Multi-repo mode ──────────────────────────────────────────────────────
    // When `multi_repo` entries are present, run the loop for each target in
    // sequence and print a combined summary.  The single-repo path is skipped.
    if !validated.multi_repo.is_empty() {
        info!(
            provider = %validated.provider,
            repos = validated.multi_repo.len(),
            "Code Looper initializing in multi-repo mode"
        );
        let targets = validated.multi_repo.clone();
        let results = multi_repo::run_multi_repo(validated, &targets);

        multi_repo::print_multi_repo_summary(&results);

        // Exit with failure if any repo had failures or a failing termination
        // reason — mirrors the single-repo exit-code logic below (#136).
        let any_failed = results.iter().any(|r| {
            r.summary.failures > 0
                || r.summary.hook_failed
                || matches!(
                    r.summary.termination_reason,
                    Some(loop_engine::TerminationReason::StoppedOnFailure)
                        | Some(loop_engine::TerminationReason::ProviderError(_))
                )
        });
        if any_failed {
            std::process::exit(1);
        }
        return Ok(());
    }

    info!(
        provider = %validated.provider,
        iterations = %validated.iteration_count(),
        "Code Looper initialized"
    );

    // Build the loop engine, install signal handler, and run.
    let engine = loop_engine::LoopEngine::new(validated, guard);
    engine.install_signal_handler();
    let summary = engine.run();

    if summary.failures > 0
        || summary.hook_failed
        || matches!(
            summary.termination_reason,
            Some(loop_engine::TerminationReason::StoppedOnFailure)
                | Some(loop_engine::TerminationReason::ProviderError(_))
                | Some(loop_engine::TerminationReason::Interrupted)
        )
    {
        std::process::exit(1);
    }

    Ok(())
}
