# ADR-004: Centralised Branch Lifecycle Manager

**Status:** Accepted  
**Date:** 2026-04-07  
**Deciders:** Code Looper project team

## Context

When the loop operates in `single-pr` mode, it needs to:
1. Ensure a feature branch exists before the agent runs (so commits land on the right branch).
2. Derive a deterministic, URL-safe branch name from the configured prefix and issue number.
3. Push the branch to `origin` after a shippable signal is detected (so `gh pr create` can find the commits).
4. Optionally clean up the local and remote branch after a PR is merged.

Early iterations handled branch names inline in `loop_engine.rs` with `branch_prefix.trim_end_matches('/')`, which produced the bare prefix (`loop`) instead of a valid feature branch name. All three PR strategies (`no-pr`, `single-pr`, `multi-pr`) also duplicated branch-string logic.

## Decision

Centralise all branch operations in `src/branch.rs` behind two surfaces:

1. **`derive_branch_name(prefix, issue_number, title, max_slug)`** — pure function; produces `{prefix}{issue_number}-{slug}`. URL-safe, collapses dashes, strips leading/trailing punctuation.
2. **`BranchManager`** — stateful struct wrapping `PrManagementConfig`; exposes `ensure_branch`, `push_branch`, and `cleanup_branch` with safety guards. Specifically: it refuses to operate on `base_branch`, it never adds `--force-with-lease` to `git push` unless `allow_force_push=true` (relying on git's default non-fast-forward rejection to prevent accidental overwrites — there is no separate refusal error path), and it refuses to delete branches with unmerged commits.

`LoopEngine` constructs a `BranchManager` when `mode == SinglePr` and:
- Calls `ensure_branch(issue_number, "")` before the iteration loop to checkout the feature branch.
- Calls `push_branch(branch)` before `PrManager::handle_milestone()` so commits are visible on GitHub.
- Uses the derived branch name (not the trimmed prefix) when calling `gh pr create`.

The public `current_branch()` helper allows the engine to detect the actual checked-out branch as a fallback when `BranchManager` cannot create the branch.

## Consequences

**Positive:**
- Single, tested surface for all branch operations — no duplication across strategies.
- Safety invariants (base-branch protection, force-push gate) enforced in one place.
- Correct branch names passed to `gh pr create` (fixes the `loop` → `loop/42-` bug).
- `cleanup_branch` is available for future automation of post-merge cleanup.

**Negative:**
- `BranchManager::ensure_branch` runs `git checkout` before the agent starts; if the agent needs to be on a different branch it must switch itself.
- `cleanup_branch` is not yet called automatically (would require detecting PR merge events, which needs a polling loop or webhook).

## References

- `src/branch.rs` — `BranchManager`, `derive_branch_name`, `current_branch`
- `src/loop_engine.rs` — `branch_manager` field, `single_pr_branch` computation in `run()`
- GitHub issue [#28](https://github.com/jamesbrayton/code-looper/issues/28)
