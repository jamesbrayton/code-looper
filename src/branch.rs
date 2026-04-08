//! Feature branch lifecycle management.
//!
//! Centralises branch naming, creation, push, and cleanup so that all three
//! PR strategies (`no-pr`, `single-pr`, `multi-pr`) use a single, tested
//! surface instead of open-coding git commands.
//!
//! # Safety invariants
//! * Never operates on `base_branch` — any attempt returns an error.
//! * Never passes `--force-with-lease` to `git push` unless `allow_force_push`
//!   is explicitly `true`.  The manager relies on git's default
//!   non-fast-forward rejection to prevent accidental overwrites — there is
//!   no separate "refuse the force push" error path (the aspirational
//!   `BranchError::ForcePushDisabled` variant was removed in #79).
//! * Never deletes a branch that has unmerged commits or uncommitted changes.
use std::process::Command;

use crate::config::PrManagementConfig;

// ── Branch name derivation ────────────────────────────────────────────────────

/// Maximum slug length used when `max_slug_length` is not configured.
const DEFAULT_MAX_SLUG_LEN: usize = 40;

/// Derive a deterministic, URL-safe branch name for a given issue.
///
/// The resulting name is `{prefix}{issue_number}-{slug}` where `slug` is the
/// issue title lowercased, with non-alphanumeric characters replaced by `-`,
/// consecutive dashes collapsed, leading/trailing dashes stripped, and the
/// whole slug truncated to `max_slug_length` characters (trailing dash also
/// stripped after truncation).
///
/// # Example
/// ```
/// use code_looper::branch::derive_branch_name;
/// let name = derive_branch_name("loop/", 42, "Fix auth bug!", 40);
/// assert_eq!(name, "loop/42-fix-auth-bug");
/// ```
pub fn derive_branch_name(
    prefix: &str,
    issue_number: u64,
    title: &str,
    max_slug_length: usize,
) -> String {
    let max_slug = if max_slug_length == 0 {
        DEFAULT_MAX_SLUG_LEN
    } else {
        max_slug_length
    };

    // Lower-case and replace non-alnum with '-'
    let raw: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive dashes and trim leading/trailing
    let mut slug = String::new();
    let mut prev_dash = true; // treat start as dash so leading ones are dropped
    for ch in raw.chars() {
        if ch == '-' {
            if !prev_dash {
                slug.push('-');
            }
            prev_dash = true;
        } else {
            slug.push(ch);
            prev_dash = false;
        }
    }
    // Strip trailing dash
    let slug = slug.trim_end_matches('-');

    // Truncate and strip any trailing dash introduced by truncation
    let slug = if slug.len() > max_slug {
        slug[..max_slug].trim_end_matches('-')
    } else {
        slug
    };

    format!("{prefix}{issue_number}-{slug}")
}

// ── Git helpers ───────────────────────────────────────────────────────────────

/// Error type for branch operations.
#[derive(Debug, thiserror::Error)]
pub enum BranchError {
    #[error("refused to operate on protected base branch '{0}'")]
    BaseBranchProtected(String),

    #[error("git command failed: {0}")]
    GitCommand(String),

    /// Returned by [`BranchManager::cleanup_branch`] when it would have to
    /// delete a branch that has uncommitted changes (current worktree) or
    /// commits not yet in `base_branch`.
    ///
    /// Only the single-PR cleanup path reaches this; multi-PR mode uses
    /// [`BranchManager::cleanup_merged_remote_branch`] and never touches
    /// the local branch.
    #[error("branch has uncommitted changes or unmerged commits — refusing to delete '{0}'")]
    UnsafeDelete(String),
}

/// Run a git command and return trimmed stdout on success, or a `BranchError`
/// on non-zero exit.
fn git(args: &[&str]) -> Result<String, BranchError> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| BranchError::GitCommand(format!("failed to spawn git: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(BranchError::GitCommand(format!(
            "git {} failed: {stderr}",
            args.join(" ")
        )))
    }
}

/// Return the name of the currently checked-out branch.
pub fn current_branch() -> Result<String, BranchError> {
    git(&["rev-parse", "--abbrev-ref", "HEAD"])
}

/// Return `true` if a local branch with `name` exists.
fn local_branch_exists(name: &str) -> bool {
    Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{name}"),
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Return `true` if a remote-tracking branch `origin/{name}` exists.
fn remote_branch_exists(name: &str) -> bool {
    Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{name}"),
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Return `true` if the working tree has uncommitted changes (tracked or staged).
#[allow(dead_code)]
fn has_uncommitted_changes() -> bool {
    Command::new("git")
        .args(["diff", "--quiet", "HEAD"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
}

/// Return `true` if `branch` contains commits not present in `base_branch`
/// that are not yet merged (i.e. the branch tip is ahead of `base_branch`).
#[allow(dead_code)]
fn has_unmerged_commits(branch: &str, base_branch: &str) -> bool {
    // Count commits in branch that are not in base_branch
    let result = Command::new("git")
        .args(["rev-list", "--count", &format!("{base_branch}..{branch}")])
        .output();
    match result {
        Ok(out) if out.status.success() => {
            let count: u64 = String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse()
                .unwrap_or(1);
            count > 0
        }
        _ => true, // assume unmerged on error — fail safe
    }
}

// ── BranchManager ─────────────────────────────────────────────────────────────

/// Manages the lifecycle of a single feature branch tied to one issue.
///
/// Safety-affecting knobs (`no_pr_push`, `delete_remote_branch_on_merge`) are
/// private and can only be changed at construction time via the `with_*`
/// builder methods — the aim is that no consumer can flip a destructive flag
/// *after* handing the manager to some inner component (see #73).
pub struct BranchManager {
    config: PrManagementConfig,
    /// Maximum character length for the title slug portion of the branch name.
    max_slug_length: usize,
    /// Push to origin even in `no-pr` mode (default: `true`).
    no_pr_push: bool,
    /// Delete the remote branch after a PR merge (default: `true`).
    delete_remote_branch_on_merge: bool,
}

impl BranchManager {
    /// Create a new manager from `PrManagementConfig` with sensible defaults.
    pub fn new(config: PrManagementConfig) -> Self {
        Self {
            config,
            max_slug_length: DEFAULT_MAX_SLUG_LEN,
            no_pr_push: true,
            delete_remote_branch_on_merge: true,
        }
    }

    /// Override the `no_pr_push` flag at construction time.
    ///
    /// When `false`, `push_branch` is a no-op in `no-pr` mode.  Meant for
    /// tests and for rare configurations that explicitly want the loop to
    /// keep work local.
    #[allow(dead_code)]
    pub fn with_no_pr_push(mut self, no_pr_push: bool) -> Self {
        self.no_pr_push = no_pr_push;
        self
    }

    /// Override the `delete_remote_branch_on_merge` flag at construction
    /// time.
    ///
    /// When `false`, [`Self::cleanup_merged_remote_branch`] short-circuits
    /// without touching the remote.  This field is destructive and must not
    /// be mutable on a live `BranchManager` — use this builder on a fresh
    /// instance instead.
    #[allow(dead_code)]
    pub fn with_delete_remote_branch_on_merge(mut self, enabled: bool) -> Self {
        self.delete_remote_branch_on_merge = enabled;
        self
    }

    /// The base branch name (e.g. `"main"`).
    fn base(&self) -> &str {
        &self.config.base_branch
    }

    /// Derive the feature branch name for this issue.
    pub fn branch_name(&self, issue_number: u64, title: &str) -> String {
        derive_branch_name(
            &self.config.branch_prefix,
            issue_number,
            title,
            self.max_slug_length,
        )
    }

    /// Ensure a feature branch exists for `issue_number`/`title` and check it
    /// out.  Idempotent: if the branch already exists locally or remotely,
    /// reuses it without creating a duplicate.
    ///
    /// Returns the branch name.
    pub fn ensure_branch(&self, issue_number: u64, title: &str) -> Result<String, BranchError> {
        let branch = self.branch_name(issue_number, title);

        // Guard: never operate on base_branch
        if branch == self.base() {
            return Err(BranchError::BaseBranchProtected(branch));
        }

        if local_branch_exists(&branch) {
            // Already exists locally — just switch to it
            tracing::debug!(
                branch,
                "feature branch already exists locally; checking out"
            );
            git(&["checkout", &branch])?;
        } else if remote_branch_exists(&branch) {
            // Exists on remote but not locally — create tracking branch
            tracing::debug!(
                branch,
                "feature branch exists on remote; creating local tracking branch"
            );
            git(&["checkout", "-b", &branch, &format!("origin/{branch}")])?;
        } else {
            // New branch — create from latest base_branch.
            //
            // The fetch is still best-effort (we don't fail the iteration
            // when `origin` is unreachable, since the user may be working
            // offline), but we now *log loudly* when it fails or is skipped
            // — a silent fetch failure followed by a checkout from a stale
            // `origin/{base}` is the classic source of "why are my PRs full
            // of conflicts" debugging sessions (see #68).
            tracing::debug!(branch, base = self.base(), "creating new feature branch");
            let fetch_status = Command::new("git")
                .args(["fetch", "origin", self.base()])
                .status();
            match fetch_status {
                Ok(status) if status.success() => {}
                Ok(status) => {
                    tracing::warn!(
                        base = self.base(),
                        exit_code = status.code(),
                        "git fetch origin {base} failed; new branch will be created from \
                         possibly-stale origin/{base} (expect merge conflicts)",
                        base = self.base()
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        base = self.base(),
                        error = %e,
                        "could not spawn `git fetch`; new branch will be created from \
                         possibly-stale origin/{base} (expect merge conflicts)",
                        base = self.base()
                    );
                }
            }
            git(&[
                "checkout",
                "-b",
                &branch,
                &format!("origin/{}", self.base()),
            ])?;
        }

        Ok(branch)
    }

    /// Push the active feature branch to `origin`.
    ///
    /// In `no-pr` mode, the push happens only when `no_pr_push` is `true`
    /// (the default), so work is not lost.  Force-push is only allowed when
    /// `allow_force_push` is `true` in `config`.
    pub fn push_branch(&self, branch: &str) -> Result<(), BranchError> {
        // Guard: never push base_branch via this path
        if branch == self.base() {
            return Err(BranchError::BaseBranchProtected(branch.to_string()));
        }

        use crate::config::PrMode;
        if self.config.mode == PrMode::NoPr && !self.no_pr_push {
            tracing::debug!(branch, "no-pr mode with no_pr_push=false; skipping push");
            return Ok(());
        }

        let mut args = vec!["push", "-u", "origin", branch];
        if self.config.allow_force_push {
            args.push("--force-with-lease");
        } else {
            // Non-force push — if the remote branch already has commits not
            // in our local branch we let git reject it (correct behaviour).
        }

        tracing::info!(
            branch,
            force = self.config.allow_force_push,
            "pushing feature branch"
        );
        git(&args).map(|_| ())
    }

    /// Delete a feature branch that the engine itself checked out locally
    /// (used by the `single-pr` flow when a PR is merged from the loop's
    /// own worktree).
    ///
    /// Safety checks:
    /// * Refuses to delete `base_branch`.
    /// * Refuses to delete a branch with uncommitted changes (when it is
    ///   currently checked out) or with commits not yet in `base_branch`.
    ///
    /// After deleting the local branch, deletes the remote branch when
    /// `delete_remote_branch_on_merge` is `true`.
    ///
    /// **Do not call this from multi-PR mode** — the engine never checks
    /// out the PR's local branch there, so this method would operate on
    /// whatever branch the engine's CWD happens to be on (see #65).  Use
    /// [`Self::cleanup_merged_remote_branch`] instead for multi-PR merges.
    #[allow(dead_code)]
    pub fn cleanup_branch(&self, branch: &str) -> Result<(), BranchError> {
        // Guard: never delete base_branch
        if branch == self.base() {
            return Err(BranchError::BaseBranchProtected(branch.to_string()));
        }

        // If this is the currently checked-out branch, refuse when there are
        // uncommitted changes.
        let on_branch = current_branch().ok().as_deref() == Some(branch);
        if on_branch && has_uncommitted_changes() {
            return Err(BranchError::UnsafeDelete(branch.to_string()));
        }

        // Refuse to delete if there are commits not in base_branch
        if has_unmerged_commits(branch, self.base()) {
            return Err(BranchError::UnsafeDelete(branch.to_string()));
        }

        // Switch away from the branch before deleting it (if currently on it)
        if current_branch().ok().as_deref() == Some(branch) {
            git(&["checkout", self.base()])?;
        }

        tracing::info!(branch, "deleting local feature branch after merge");
        git(&["branch", "-d", branch])?;

        if self.delete_remote_branch_on_merge && remote_branch_exists(branch) {
            tracing::info!(branch, "deleting remote feature branch after merge");
            git(&["push", "origin", "--delete", branch]).map(|_| ())?;
        }

        Ok(())
    }

    /// Delete the *remote* feature branch for a PR the engine did not check
    /// out locally.
    ///
    /// Used by the multi-PR triage flow after `gh pr merge` succeeds.  In
    /// that flow the engine never created or checked out the local branch,
    /// so the original [`Self::cleanup_branch`] (which runs `git branch -d`
    /// against whatever the engine's CWD happens to be on) is actively
    /// wrong — see #65.  This method is the remote-only alternative:
    ///
    /// * Refuses to delete `base_branch`.
    /// * Skips silently when `delete_remote_branch_on_merge` is `false`.
    /// * Skips silently when the remote branch no longer exists (another
    ///   process may have deleted it already).
    /// * Never touches the local worktree — no `git checkout`, no
    ///   `git branch -d`.
    pub fn cleanup_merged_remote_branch(&self, branch: &str) -> Result<(), BranchError> {
        if branch == self.base() {
            return Err(BranchError::BaseBranchProtected(branch.to_string()));
        }
        if !self.delete_remote_branch_on_merge {
            tracing::debug!(
                branch,
                "delete_remote_branch_on_merge=false; skipping remote cleanup"
            );
            return Ok(());
        }
        if !remote_branch_exists(branch) {
            tracing::debug!(
                branch,
                "remote branch no longer exists; skipping remote cleanup"
            );
            return Ok(());
        }
        tracing::info!(
            branch,
            "deleting remote feature branch after multi-PR merge"
        );
        git(&["push", "origin", "--delete", branch]).map(|_| ())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PrManagementConfig, PrMode};

    fn config() -> PrManagementConfig {
        PrManagementConfig::default()
    }

    fn config_with_prefix(prefix: &str) -> PrManagementConfig {
        PrManagementConfig {
            branch_prefix: prefix.to_string(),
            ..config()
        }
    }

    // ── derive_branch_name ────────────────────────────────────────────────────

    #[test]
    fn basic_name_derivation() {
        assert_eq!(
            derive_branch_name("loop/", 42, "Fix auth bug", 40),
            "loop/42-fix-auth-bug"
        );
    }

    #[test]
    fn special_characters_replaced() {
        assert_eq!(
            derive_branch_name("loop/", 1, "Add feature: fast-path (v2)!", 40),
            "loop/1-add-feature-fast-path-v2"
        );
    }

    #[test]
    fn consecutive_dashes_collapsed() {
        // Multiple punctuation in a row should produce a single dash
        assert_eq!(
            derive_branch_name("loop/", 7, "Fix   multiple   spaces", 40),
            "loop/7-fix-multiple-spaces"
        );
    }

    #[test]
    fn leading_trailing_dashes_stripped() {
        assert_eq!(
            derive_branch_name("loop/", 3, "!!! urgent fix !!!", 40),
            "loop/3-urgent-fix"
        );
    }

    #[test]
    fn title_truncated_to_max_slug_length() {
        let long_title = "A".repeat(60);
        let name = derive_branch_name("loop/", 10, &long_title, 20);
        // "10-" is 3 chars, slug portion is 20 chars
        let prefix_and_num = "loop/10-";
        assert!(name.starts_with(prefix_and_num));
        let slug = &name[prefix_and_num.len()..];
        assert!(slug.len() <= 20, "slug too long: {slug}");
    }

    #[test]
    fn truncation_strips_trailing_dash() {
        // Title that produces a slug ending with dash at truncation boundary
        // "aaa-bbb-ccc..." — truncation at 7 → "aaa-bbb" is fine, but "aaa-bbb-" → "aaa-bbb"
        let title = "aaa bbb ccc ddd"; // slug: "aaa-bbb-ccc-ddd", truncate at 8 → "aaa-bbb-" → "aaa-bbb"
        let name = derive_branch_name("loop/", 1, title, 8);
        assert!(
            !name.ends_with('-'),
            "branch name must not end with dash: {name}"
        );
    }

    #[test]
    fn empty_title_produces_valid_name() {
        let name = derive_branch_name("loop/", 5, "", 40);
        // Should be "loop/5-" — not pretty but not a crash
        assert!(name.starts_with("loop/5"));
    }

    #[test]
    fn unicode_title_handled() {
        let name = derive_branch_name("loop/", 99, "Add émoji 🚀 support", 40);
        // Non-ASCII replaced with '-'
        assert!(!name.contains(' '));
        assert!(name.starts_with("loop/99-"));
    }

    #[test]
    fn custom_prefix_propagated() {
        assert_eq!(
            derive_branch_name("feat/", 1, "hello world", 40),
            "feat/1-hello-world"
        );
    }

    #[test]
    fn zero_max_slug_uses_default() {
        // max_slug=0 uses DEFAULT_MAX_SLUG_LEN internally; just verify no panic
        let name = derive_branch_name("loop/", 1, "a title", 0);
        assert!(name.starts_with("loop/1-"));
    }

    // ── BranchManager ─────────────────────────────────────────────────────────

    #[test]
    fn branch_name_uses_config_prefix() {
        let manager = BranchManager::new(config_with_prefix("feat/"));
        assert_eq!(
            manager.branch_name(1, "do something"),
            "feat/1-do-something"
        );
    }

    #[test]
    fn ensure_branch_rejects_base_branch_name() {
        // Construct a pathological config where derived name equals base_branch
        let cfg = PrManagementConfig {
            base_branch: "loop/1-main".to_string(),
            branch_prefix: "loop/".to_string(),
            ..config()
        };
        let mgr = BranchManager::new(cfg);
        let result = mgr.ensure_branch(1, "main");
        assert!(matches!(result, Err(BranchError::BaseBranchProtected(_))));
    }

    #[test]
    fn cleanup_rejects_base_branch() {
        let mgr = BranchManager::new(config());
        let result = mgr.cleanup_branch("main");
        assert!(matches!(result, Err(BranchError::BaseBranchProtected(_))));
    }

    #[test]
    fn cleanup_merged_remote_branch_rejects_base_branch() {
        let mgr = BranchManager::new(config());
        let result = mgr.cleanup_merged_remote_branch("main");
        assert!(matches!(result, Err(BranchError::BaseBranchProtected(_))));
    }

    #[test]
    fn cleanup_merged_remote_branch_is_noop_when_disabled() {
        // When `delete_remote_branch_on_merge=false`, the method must return
        // Ok without shelling out to git at all — even the base-branch guard
        // is the only early-exit before the disabled check.  We can't cheaply
        // mock git from here, but this at least verifies the disabled path
        // short-circuits for a non-base branch.
        let mgr = BranchManager::new(config()).with_delete_remote_branch_on_merge(false);
        let result = mgr.cleanup_merged_remote_branch("loop/1-some-feature");
        assert!(
            result.is_ok(),
            "disabled flag should short-circuit without attempting git: {result:?}"
        );
    }

    #[test]
    fn push_rejects_base_branch() {
        let mgr = BranchManager::new(config());
        let result = mgr.push_branch("main");
        assert!(matches!(result, Err(BranchError::BaseBranchProtected(_))));
    }

    #[test]
    fn no_pr_push_false_skips_push() {
        let cfg = PrManagementConfig {
            mode: PrMode::NoPr,
            ..config()
        };
        let mgr = BranchManager::new(cfg).with_no_pr_push(false);
        // Should return Ok without calling git (since no_pr_push=false in no-pr mode)
        assert!(mgr.push_branch("loop/1-some-branch").is_ok());
    }

    // ── current_branch ───────────────────────────────────────────────────────

    #[test]
    fn current_branch_returns_ok_in_git_repo() {
        // Skip when the test process is not running inside a git checkout
        // (e.g., source-archive packaging contexts without VCS metadata).
        // We deliberately don't `git init` a temp dir and `set_current_dir`
        // into it: cargo runs unit tests in parallel by default, and
        // `set_current_dir` is process-global, so doing so would race with
        // any other test that observes CWD.
        let in_git_repo = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !in_git_repo {
            eprintln!("skipping current_branch_returns_ok_in_git_repo: not inside a git repo");
            return;
        }

        let branch = current_branch();
        assert!(branch.is_ok(), "current_branch() failed: {branch:?}");
        let name = branch.unwrap();
        assert!(!name.is_empty(), "current_branch() returned empty string");
    }

    #[test]
    fn branch_manager_branch_name_with_zero_issue_has_trailing_separator() {
        // When issue_number=0 and title="" the name is "{prefix}0-".
        // This is the fallback used by the loop engine when no issue is linked.
        let mgr = BranchManager::new(config());
        let name = mgr.branch_name(0, "");
        assert!(name.starts_with("loop/0"));
    }
}
