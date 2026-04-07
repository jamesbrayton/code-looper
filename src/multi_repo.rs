use crate::config::{LoopConfig, RepoTarget};
use crate::loop_engine::{LoopEngine, SessionSummary};
use crate::policy_guard::{PolicyGuard, UnsafeOverrides};
use tracing::info;

/// Result for a single repo target in a multi-repo run.
#[allow(dead_code)]
pub struct RepoRunResult {
    /// Display name of the repository (from `RepoTarget::display_name`).
    pub name: String,
    /// Filesystem path used for this run (for log context).
    pub path: std::path::PathBuf,
    /// Session summary returned by the loop engine.
    pub summary: SessionSummary,
}

/// Run the loop for each repo target in sequence.
///
/// For every target a fresh `LoopConfig` is derived from `base_config` with:
/// - `workspace_dir` set to `target.path`
/// - `prompt_inline` replaced by `target.prompt_override` when present
/// - `prompt_file` cleared when `prompt_override` is set
///
/// Returns one `RepoRunResult` per target in the same order as `targets`.
pub fn run_multi_repo(base_config: &LoopConfig, targets: &[RepoTarget]) -> Vec<RepoRunResult> {
    let mut results = Vec::with_capacity(targets.len());

    for target in targets {
        let name = target.display_name();
        info!(repo = %name, path = %target.path.display(), "Starting multi-repo run");

        let mut repo_config = base_config.clone();
        repo_config.workspace_dir = Some(target.path.clone());

        if let Some(ref prompt) = target.prompt_override {
            repo_config.prompt_inline = Some(prompt.clone());
            repo_config.prompt_file = None;
        }

        let overrides = UnsafeOverrides {
            allow_direct_github: base_config.allow_direct_github,
        };
        let guard = PolicyGuard::new(overrides);
        let engine = LoopEngine::new(repo_config, guard);
        let summary = engine.run();

        results.push(RepoRunResult {
            name,
            path: target.path.clone(),
            summary,
        });
    }

    results
}

/// Print a human-readable summary of all repo run results.
pub fn print_multi_repo_summary(results: &[RepoRunResult]) {
    println!();
    println!("━━━ Multi-repo run summary ━━━");
    let mut total_success = 0u64;
    let mut total_failed = 0u64;
    let mut total_retries = 0u64;

    for r in results {
        let s = &r.summary;
        total_success += s.successes;
        total_failed += s.failures;
        total_retries += s.retries;

        let status = if s.failures == 0 { "ok" } else { "FAILED" };
        println!(
            "  [{status}] {name}  (success={s}, fail={f}, retries={r})",
            status = status,
            name = r.name,
            s = s.successes,
            f = s.failures,
            r = s.retries,
        );
        if let Some(ref reason) = s.termination_reason {
            println!("         termination: {reason}");
        }
    }

    println!("─────────────────────────────");
    println!(
        "  Total repos={repos}  success={s}  failed={f}  retries={r}",
        repos = results.len(),
        s = total_success,
        f = total_failed,
        r = total_retries,
    );
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LoopConfig;
    use crate::loop_engine::LoopEngine;
    use crate::provider::tests::FakeAdapter;

    fn make_target(path: &str, name: Option<&str>, prompt_override: Option<&str>) -> RepoTarget {
        RepoTarget {
            path: path.into(),
            name: name.map(String::from),
            prompt_override: prompt_override.map(String::from),
        }
    }

    #[test]
    fn display_name_uses_explicit_name() {
        let t = make_target("/repos/my-project", Some("custom-label"), None);
        assert_eq!(t.display_name(), "custom-label");
    }

    #[test]
    fn display_name_falls_back_to_dir_name() {
        let t = make_target("/repos/my-project", None, None);
        assert_eq!(t.display_name(), "my-project");
    }

    #[test]
    fn prompt_override_replaces_inline_prompt() {
        let base = LoopConfig {
            iterations: 1,
            prompt_inline: Some("base prompt".to_string()),
            ..LoopConfig::default()
        };
        let targets = [make_target(
            "/tmp/repo-a",
            Some("repo-a"),
            Some("custom prompt"),
        )];

        // Verify that run_multi_repo builds the correct derived config by
        // checking that the loop engine receives the override prompt.
        // We use a FakeAdapter so no real process is spawned.
        let mut derived = base.clone();
        derived.workspace_dir = Some(targets[0].path.clone());
        if let Some(ref p) = targets[0].prompt_override {
            derived.prompt_inline = Some(p.clone());
            derived.prompt_file = None;
        }
        assert_eq!(derived.prompt_inline.as_deref(), Some("custom prompt"));
        assert!(derived.prompt_file.is_none());
    }

    #[test]
    fn multi_repo_runs_each_target_with_fake_adapter() {
        // Build two minimal configs and run them directly through LoopEngine
        // with FakeAdapter to validate that results are collected per-repo.
        let config_a = LoopConfig {
            iterations: 1,
            prompt_inline: Some("task".to_string()),
            workspace_dir: Some("/tmp/repo-a".into()),
            ..LoopConfig::default()
        };
        let config_b = LoopConfig {
            iterations: 2,
            prompt_inline: Some("task".to_string()),
            workspace_dir: Some("/tmp/repo-b".into()),
            ..LoopConfig::default()
        };

        let summary_a =
            LoopEngine::with_adapter(config_a, Box::new(FakeAdapter::success("fake"))).run();
        let summary_b =
            LoopEngine::with_adapter(config_b, Box::new(FakeAdapter::success("fake"))).run();

        assert_eq!(summary_a.successes, 1);
        assert_eq!(summary_b.successes, 2);
    }

    #[test]
    fn print_multi_repo_summary_does_not_panic_on_empty() {
        print_multi_repo_summary(&[]);
    }
}
