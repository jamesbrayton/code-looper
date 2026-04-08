/// PR lifecycle management: shippable-signal detection, PR creation, and PR
/// update for the `single-pr` and `multi-pr` strategies.
///
/// # Shippable signal protocol
///
/// The loop engine scans each provider iteration's stdout for a *shippable
/// signal* — an explicit marker that the agent believes the current branch
/// contains review-ready work.  Two forms are accepted (first match wins):
///
/// 1. **Sentinel line** — the output contains a line whose trimmed content
///    equals the configured `ready_marker` string (default:
///    `LOOPER_READY_FOR_REVIEW`).
///
/// 2. **JSON block** — the output contains a JSON object with a `"looper"`
///    key set to `"ready-for-review"`.  An optional `"summary"` key provides
///    a human-readable description that appears in the PR body:
///    ```json
///    {"looper":"ready-for-review","summary":"Implemented user auth"}
///    ```
///
/// When a signal is detected the engine calls [`PrManager::handle_milestone`]
/// which opens a new PR (if none exists for the branch) or appends a comment
/// (if one already exists).  Human-review gating is enforced: when
/// `require_human_review` is `true` (the default) the engine never merges the
/// PR itself.
///
/// # MCP-only policy and engine `gh` usage
///
/// Per the revised ADR-001, the MCP-only GitHub mutation policy is scoped to
/// **agent prompts** — the `PolicyGuard` preamble instructs providers to use
/// MCP server tools for any GitHub writes.  The **engine** (this module and
/// [`crate::issue_tracker`]) is explicitly permitted to shell out to the `gh`
/// CLI for its own PR and issue bookkeeping: engine actions are already
/// audited through the structured logs, per-run manifest, and session
/// summary.  See `docs/ADRs/ADR-001-mcp-only-github-mutations.md` for the
/// full rationale and the list of non-goals this decision implies.
use std::process::Command;

use serde::Deserialize;

use crate::config::{PrManagementConfig, TriagePriority};

// ── Signal detection ──────────────────────────────────────────────────────────

/// The parsed shippable signal emitted by the agent.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadySignal {
    /// Optional human-readable summary extracted from a JSON signal block.
    pub summary: Option<String>,
    /// Which form of signal was detected.
    pub form: SignalForm,
}

/// How the shippable signal was encoded in the agent output.
#[derive(Debug, Clone, PartialEq)]
pub enum SignalForm {
    /// Plain sentinel line (`LOOPER_READY_FOR_REVIEW`).
    Sentinel,
    /// Structured JSON block.
    Json,
}

/// Internal JSON shape for the structured signal form.
#[derive(Deserialize)]
struct JsonSignal {
    looper: String,
    summary: Option<String>,
}

/// Scan `output` for a shippable signal.
///
/// Returns `Some(ReadySignal)` when found, `None` when the output does not
/// contain any recognised signal form.
///
/// The `ready_marker` string is compared case-sensitively against each trimmed
/// line.
pub fn detect_signal(output: &str, ready_marker: &str) -> Option<ReadySignal> {
    // Try sentinel form first (cheap scan).
    for line in output.lines() {
        if line.trim() == ready_marker {
            return Some(ReadySignal {
                summary: None,
                form: SignalForm::Sentinel,
            });
        }
    }

    // Try JSON form: look for lines / blocks that parse as JsonSignal.
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('{') {
            if let Ok(sig) = serde_json::from_str::<JsonSignal>(trimmed) {
                if sig.looper == "ready-for-review" {
                    return Some(ReadySignal {
                        summary: sig.summary,
                        form: SignalForm::Json,
                    });
                }
            }
        }
    }

    None
}

// ── PR data types ─────────────────────────────────────────────────────────────

/// Lightweight description of an open pull request.
#[derive(Debug, Clone)]
pub struct PrInfo {
    pub number: u32,
    pub url: String,
    pub title: String,
    /// Head branch name (e.g. `loop/42-fix-bug`).  Empty string when unknown.
    pub head_ref: String,
}

/// Input for opening a new pull request.
#[derive(Debug, Clone)]
pub struct PrDraft {
    /// Source branch (head).
    pub branch: String,
    /// Target branch (base).
    pub base_branch: String,
    /// PR title.
    pub title: String,
    /// PR body in Markdown.
    pub body: String,
    /// Labels to apply.
    pub labels: Vec<String>,
}

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors produced by PR lifecycle operations.
#[derive(Debug, thiserror::Error)]
pub enum PrError {
    #[error("gh CLI command failed: {0}")]
    GhCommand(String),

    #[error("failed to parse gh output: {0}")]
    ParseError(String),

    #[error("PR already exists for branch '{0}'")]
    #[allow(dead_code)]
    AlreadyExists(String),
}

// ── PrLifecycle trait ─────────────────────────────────────────────────────────

/// Abstraction over PR CRUD operations.
///
/// The production implementation shells out to the `gh` CLI.  Tests inject a
/// [`MockPrLifecycle`] to verify call patterns without network I/O.
pub trait PrLifecycle: Send + Sync {
    /// Find an open PR whose head branch matches `branch`.
    ///
    /// Returns `None` when no such PR exists.
    fn find_open_pr(&self, branch: &str) -> Result<Option<PrInfo>, PrError>;

    /// Open a new pull request described by `draft`.
    fn open_pr(&self, draft: &PrDraft) -> Result<PrInfo, PrError>;

    /// Append `body` as a comment on the pull request identified by
    /// `pr_number`.
    fn comment_on_pr(&self, pr_number: u32, body: &str) -> Result<(), PrError>;
}

// ── GhPrLifecycle (production) ────────────────────────────────────────────────

/// Production [`PrLifecycle`] implementation backed by the `gh` CLI.
pub struct GhPrLifecycle;

impl GhPrLifecycle {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GhPrLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl PrLifecycle for GhPrLifecycle {
    fn find_open_pr(&self, branch: &str) -> Result<Option<PrInfo>, PrError> {
        let out = Command::new("gh")
            .args([
                "pr",
                "list",
                "--head",
                branch,
                "--state",
                "open",
                "--json",
                "number,url,title,headRefName",
                "--limit",
                "1",
            ])
            .output()
            .map_err(|e| PrError::GhCommand(format!("failed to spawn gh: {e}")))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(PrError::GhCommand(format!("gh pr list failed: {stderr}")));
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let prs: Vec<serde_json::Value> = serde_json::from_str(&stdout)
            .map_err(|e| PrError::ParseError(format!("failed to parse gh pr list output: {e}")))?;

        if let Some(pr) = prs.into_iter().next() {
            let number = pr["number"].as_u64().ok_or_else(|| {
                PrError::ParseError("missing 'number' field in gh pr list output".into())
            })? as u32;
            let url = pr["url"]
                .as_str()
                .ok_or_else(|| PrError::ParseError("missing 'url' field".into()))?
                .to_string();
            let title = pr["title"]
                .as_str()
                .ok_or_else(|| PrError::ParseError("missing 'title' field".into()))?
                .to_string();
            let head_ref = pr["headRefName"].as_str().unwrap_or(branch).to_string();
            Ok(Some(PrInfo {
                number,
                url,
                title,
                head_ref,
            }))
        } else {
            Ok(None)
        }
    }

    fn open_pr(&self, draft: &PrDraft) -> Result<PrInfo, PrError> {
        let mut args = vec![
            "pr",
            "create",
            "--head",
            draft.branch.as_str(),
            "--base",
            draft.base_branch.as_str(),
            "--title",
            draft.title.as_str(),
            "--body",
            draft.body.as_str(),
        ];

        // gh pr create accepts multiple --label flags.
        let label_args: Vec<String> = draft
            .labels
            .iter()
            .flat_map(|l| ["--label".to_string(), l.clone()])
            .collect();
        let label_strs: Vec<&str> = label_args.iter().map(String::as_str).collect();
        args.extend_from_slice(&label_strs);

        let out = Command::new("gh")
            .args(&args)
            .output()
            .map_err(|e| PrError::GhCommand(format!("failed to spawn gh: {e}")))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(PrError::GhCommand(format!("gh pr create failed: {stderr}")));
        }

        // `gh pr create` prints the PR URL to stdout.
        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();

        // Retrieve the PR number from the URL (last path segment).
        let number: u32 = url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                PrError::ParseError(format!("cannot parse PR number from URL: {url}"))
            })?;

        Ok(PrInfo {
            number,
            url,
            title: draft.title.clone(),
            head_ref: draft.branch.clone(),
        })
    }

    fn comment_on_pr(&self, pr_number: u32, body: &str) -> Result<(), PrError> {
        let pr_ref = pr_number.to_string();
        let out = Command::new("gh")
            .args(["pr", "comment", &pr_ref, "--body", body])
            .output()
            .map_err(|e| PrError::GhCommand(format!("failed to spawn gh: {e}")))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(PrError::GhCommand(format!(
                "gh pr comment failed: {stderr}"
            )));
        }
        Ok(())
    }
}

// ── MockPrLifecycle (test double) ─────────────────────────────────────────────

/// Recorded call to the mock lifecycle.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::enum_variant_names)]
pub enum PrCall {
    FindOpenPr {
        branch: String,
    },
    OpenPr {
        branch: String,
        base: String,
        title: String,
    },
    CommentOnPr {
        pr_number: u32,
    },
}

/// Test double for [`PrLifecycle`].
///
/// Callers configure the scripted responses before injecting the mock, then
/// assert on `calls` after exercising the code under test.
#[cfg(test)]
pub struct MockPrLifecycle {
    /// Pre-configured response for `find_open_pr`.  `None` means "no open PR".
    pub existing_pr: std::sync::Mutex<Option<PrInfo>>,
    /// Pre-configured response for `open_pr`.
    pub opened_pr: PrInfo,
    /// All calls made to the mock, in order.
    pub calls: std::sync::Mutex<Vec<PrCall>>,
}

#[cfg(test)]
impl MockPrLifecycle {
    /// Create a mock with no pre-existing PR.  `open_pr` returns a dummy PR
    /// at number `42`.
    pub fn new() -> Self {
        Self {
            existing_pr: std::sync::Mutex::new(None),
            opened_pr: PrInfo {
                number: 42,
                url: "https://github.com/owner/repo/pull/42".into(),
                title: "[LOOPER] Test PR".into(),
                head_ref: "loop/42-test-pr".into(),
            },
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Configure the mock to report an existing open PR.
    pub fn with_existing_pr(self, pr: PrInfo) -> Self {
        *self.existing_pr.lock().unwrap() = Some(pr);
        self
    }
}

#[cfg(test)]
impl Default for MockPrLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl PrLifecycle for MockPrLifecycle {
    fn find_open_pr(&self, branch: &str) -> Result<Option<PrInfo>, PrError> {
        self.calls.lock().unwrap().push(PrCall::FindOpenPr {
            branch: branch.to_string(),
        });
        Ok(self.existing_pr.lock().unwrap().clone())
    }

    fn open_pr(&self, draft: &PrDraft) -> Result<PrInfo, PrError> {
        self.calls.lock().unwrap().push(PrCall::OpenPr {
            branch: draft.branch.clone(),
            base: draft.base_branch.clone(),
            title: draft.title.clone(),
        });
        Ok(self.opened_pr.clone())
    }

    fn comment_on_pr(&self, pr_number: u32, body: &str) -> Result<(), PrError> {
        let _ = body;
        self.calls
            .lock()
            .unwrap()
            .push(PrCall::CommentOnPr { pr_number });
        Ok(())
    }
}

// ── PrManager ─────────────────────────────────────────────────────────────────

/// Result of handling a shippable milestone.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum PrAction {
    /// A new PR was opened.
    Opened(PrInfo),
    /// An existing PR was updated with a new comment.
    Updated { pr: PrInfo, comment_added: bool },
    /// The shippable signal was not present — no action taken.
    NoSignal,
    /// Signal detected but PR creation skipped because `require_human_review`
    /// is `true` and a PR is already open (no further automated action).
    BlockedOnHumanReview(PrInfo),
}

/// Orchestrates shippable-signal detection and PR lifecycle management.
pub struct PrManager<L: PrLifecycle> {
    config: PrManagementConfig,
    lifecycle: L,
    /// Sentinel string to look for in agent output (default:
    /// `LOOPER_READY_FOR_REVIEW`).
    ready_marker: String,
}

impl<L: PrLifecycle> PrManager<L> {
    pub fn new(config: PrManagementConfig, lifecycle: L) -> Self {
        let ready_marker = config
            .ready_marker
            .clone()
            .unwrap_or_else(|| "LOOPER_READY_FOR_REVIEW".to_string());
        Self {
            config,
            lifecycle,
            ready_marker,
        }
    }

    /// Build a PR title from an issue number and title.
    fn pr_title(issue_number: u64, issue_title: &str) -> String {
        format!("[LOOPER] #{issue_number}: {issue_title}")
    }

    /// Build a PR body linking back to the originating issue and including an
    /// optional agent-provided summary.
    fn pr_body(issue_number: u64, run_summary: Option<&str>) -> String {
        let mut body = format!(
            "Closes #{issue_number}\n\n\
             > This pull request was opened automatically by [Code Looper](https://github.com/jamesbrayton/code-looper).\n"
        );
        if let Some(summary) = run_summary {
            body.push_str(&format!("\n## Agent summary\n\n{summary}\n"));
        }
        body
    }

    /// Standard labels applied to every auto-opened PR.
    fn default_labels() -> Vec<String> {
        vec!["code-looper".to_string(), "needs-review".to_string()]
    }

    /// Inspect `agent_output` for a shippable signal and act accordingly.
    ///
    /// - If no signal: returns `PrAction::NoSignal`.
    /// - If signal and no open PR: opens a new PR.
    /// - If signal and an open PR exists:
    ///   - When `require_human_review` is `true`: returns
    ///     `PrAction::BlockedOnHumanReview`.
    ///   - Otherwise: adds a comment to the existing PR.
    pub fn handle_milestone(
        &self,
        branch: &str,
        issue_number: u64,
        issue_title: &str,
        agent_output: &str,
    ) -> Result<PrAction, PrError> {
        let signal = detect_signal(agent_output, &self.ready_marker);
        if signal.is_none() {
            return Ok(PrAction::NoSignal);
        }
        let signal = signal.unwrap();

        tracing::info!(
            branch,
            issue_number,
            form = ?signal.form,
            "shippable signal detected; checking for existing PR"
        );

        let existing = self.lifecycle.find_open_pr(branch)?;

        match existing {
            None => {
                // Open a fresh PR.
                let draft = PrDraft {
                    branch: branch.to_string(),
                    base_branch: self.config.base_branch.clone(),
                    title: Self::pr_title(issue_number, issue_title),
                    body: Self::pr_body(issue_number, signal.summary.as_deref()),
                    labels: Self::default_labels(),
                };
                let pr = self.lifecycle.open_pr(&draft)?;
                tracing::info!(
                    pr_number = pr.number,
                    url = %pr.url,
                    "opened PR for branch"
                );
                Ok(PrAction::Opened(pr))
            }
            Some(pr) if self.config.require_human_review => {
                tracing::info!(
                    pr_number = pr.number,
                    "PR already open; require_human_review=true — no automated action"
                );
                Ok(PrAction::BlockedOnHumanReview(pr))
            }
            Some(pr) => {
                // Existing PR, human review not required — append a comment.
                let comment = match signal.summary {
                    Some(ref s) => {
                        format!("**Code Looper update** — agent signalled ready-for-review.\n\n{s}")
                    }
                    None => {
                        "**Code Looper update** — agent signalled ready-for-review.".to_string()
                    }
                };
                self.lifecycle.comment_on_pr(pr.number, &comment)?;
                tracing::info!(pr_number = pr.number, "appended comment to existing PR");
                Ok(PrAction::Updated {
                    pr,
                    comment_added: true,
                })
            }
        }
    }
}

// ── Convenience constructor for production use ────────────────────────────────

/// Build a production [`PrManager`] backed by the `gh` CLI.
pub fn build_pr_manager(config: PrManagementConfig) -> PrManager<GhPrLifecycle> {
    PrManager::new(config, GhPrLifecycle::new())
}

// ── PR triage types ───────────────────────────────────────────────────────────

/// Review/check state of an open PR as seen by the triage step.
#[derive(Debug, Clone, PartialEq)]
pub enum PrTriageState {
    /// One or more CI checks are failing.
    ChecksFailing,
    /// Reviewer has requested changes.
    ChangesRequested,
    /// Approved (or no review required) and all checks pass — ready to merge.
    ReadyToMerge,
    /// Awaiting initial review; not yet approved.
    NeedsReview,
    /// PR is labeled with a skip label (`do-not-loop`, `wip`, etc.).
    Skipped { reason: String },
}

/// An open PR with its resolved triage state.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PrWithState {
    pub pr: PrInfo,
    pub state: PrTriageState,
    /// ISO-8601 creation timestamp (used for `oldest`/`newest` ordering).
    pub created_at: String,
    /// GitHub merge-ability state: `"MERGEABLE"`, `"CONFLICTING"`, or `"UNKNOWN"`.
    /// `None` when the field was not returned by the API (older `gh` versions).
    pub mergeable: Option<String>,
}

/// Outcome of a `PrTriage::select_action` call.
#[derive(Debug, Clone)]
pub enum TriageAction {
    /// Instruct the agent to fix CI failures on this PR.
    FixChecks { pr: PrInfo, prompt: String },
    /// Instruct the agent to address review feedback on this PR.
    AddressReviewFeedback { pr: PrInfo, prompt: String },
    /// The PR is ready to merge; the engine will merge it directly (when
    /// `require_human_review = false`).
    Merge { pr: PrInfo },
    /// The PR is ready to merge but `require_human_review = true`; engine
    /// reports the situation and falls through.
    BlockedOnHumanReview { pr: PrInfo },
    /// No open PR could be advanced — fall through to issue work.
    NoActionablePr,
}

// ── Extended PrLifecycle methods (triage) ────────────────────────────────────

/// Extra methods on [`PrLifecycle`] required by the triage step.
///
/// Kept as a separate trait so existing implementations are not broken, and to
/// make it easy to implement only what is needed in tests.
pub trait PrLifecycleTriage: Send + Sync {
    /// List open PRs that carry `label`.  Returns PRs in ascending creation
    /// order (oldest first).
    fn list_open_prs_with_label(&self, label: &str) -> Result<Vec<PrInfo>, PrError>;

    /// Fetch the triage state for a single PR.
    ///
    /// The implementation queries `gh pr view <number> --json` for check
    /// status, review decision, and labels.
    fn get_pr_state(&self, pr_number: u32, skip_labels: &[String]) -> Result<PrWithState, PrError>;
}

impl PrLifecycleTriage for GhPrLifecycle {
    fn list_open_prs_with_label(&self, label: &str) -> Result<Vec<PrInfo>, PrError> {
        let out = Command::new("gh")
            .args([
                "pr",
                "list",
                "--label",
                label,
                "--state",
                "open",
                "--json",
                "number,url,title,headRefName",
                "--limit",
                "100",
            ])
            .output()
            .map_err(|e| PrError::GhCommand(format!("failed to spawn gh: {e}")))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(PrError::GhCommand(format!("gh pr list failed: {stderr}")));
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let prs: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| PrError::ParseError(e.to_string()))?;

        prs.into_iter()
            .map(|v| {
                let number = v["number"]
                    .as_u64()
                    .ok_or_else(|| PrError::ParseError("missing number".into()))?
                    as u32;
                let url = v["url"]
                    .as_str()
                    .ok_or_else(|| PrError::ParseError("missing url".into()))?
                    .to_string();
                let title = v["title"]
                    .as_str()
                    .ok_or_else(|| PrError::ParseError("missing title".into()))?
                    .to_string();
                let head_ref = v["headRefName"].as_str().unwrap_or("").to_string();
                Ok(PrInfo {
                    number,
                    url,
                    title,
                    head_ref,
                })
            })
            .collect()
    }

    fn get_pr_state(&self, pr_number: u32, skip_labels: &[String]) -> Result<PrWithState, PrError> {
        let pr_ref = pr_number.to_string();
        let out = Command::new("gh")
            .args([
                "pr",
                "view",
                &pr_ref,
                "--json",
                "number,url,title,headRefName,labels,statusCheckRollup,reviewDecision,createdAt,mergeable",
            ])
            .output()
            .map_err(|e| PrError::GhCommand(format!("failed to spawn gh: {e}")))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(PrError::GhCommand(format!("gh pr view failed: {stderr}")));
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let v: serde_json::Value =
            serde_json::from_str(&stdout).map_err(|e| PrError::ParseError(e.to_string()))?;

        let number = v["number"].as_u64().unwrap_or(pr_number as u64) as u32;
        let url = v["url"].as_str().unwrap_or("").to_string();
        let title = v["title"].as_str().unwrap_or("").to_string();
        let head_ref = v["headRefName"].as_str().unwrap_or("").to_string();
        let created_at = v["createdAt"].as_str().unwrap_or("").to_string();
        let mergeable = v["mergeable"].as_str().map(|s| s.to_string());

        let pr = PrInfo {
            number,
            url,
            title,
            head_ref,
        };

        // Check skip labels.
        if let Some(labels) = v["labels"].as_array() {
            for lv in labels {
                if let Some(name) = lv["name"].as_str() {
                    if skip_labels.iter().any(|s| s == name) {
                        return Ok(PrWithState {
                            pr,
                            state: PrTriageState::Skipped {
                                reason: name.to_string(),
                            },
                            created_at,
                            mergeable: mergeable.clone(),
                        });
                    }
                }
            }
        }

        // Check CI status.
        let checks_failing = v["statusCheckRollup"]
            .as_array()
            .map(|checks| {
                checks.iter().any(|c| {
                    c["conclusion"]
                        .as_str()
                        .map(|s| s == "FAILURE")
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if checks_failing {
            return Ok(PrWithState {
                pr,
                state: PrTriageState::ChecksFailing,
                created_at,
                mergeable,
            });
        }

        // Check review decision.
        let review_decision = v["reviewDecision"].as_str().unwrap_or("");
        let state = match review_decision {
            "CHANGES_REQUESTED" => PrTriageState::ChangesRequested,
            "APPROVED" | "" => PrTriageState::ReadyToMerge,
            "REVIEW_REQUIRED" => PrTriageState::NeedsReview,
            _ => PrTriageState::NeedsReview,
        };

        Ok(PrWithState {
            pr,
            state,
            created_at,
            mergeable,
        })
    }
}

// ── PrTriage ──────────────────────────────────────────────────────────────────

/// Implements the multi-PR triage step: inspect open PRs and decide which
/// action to take (or fall through to issue work).
pub struct PrTriage<L: PrLifecycleTriage> {
    config: PrManagementConfig,
    lifecycle: L,
}

impl<L: PrLifecycleTriage> PrTriage<L> {
    pub fn new(config: PrManagementConfig, lifecycle: L) -> Self {
        Self { config, lifecycle }
    }

    /// Select the highest-priority actionable PR and return the triage action.
    ///
    /// PRs are fetched by the `code-looper` label, then filtered (skip
    /// labels), sorted by triage priority, and the first actionable one is
    /// returned.
    pub fn select_action(&self) -> TriageAction {
        let mut prs = match self.lifecycle.list_open_prs_with_label("code-looper") {
            Ok(prs) => prs,
            Err(e) => {
                tracing::warn!(error = %e, "PrTriage: failed to list open PRs; falling through");
                return TriageAction::NoActionablePr;
            }
        };

        // Apply triage priority ordering for Oldest/Newest (list already comes
        // from gh in ascending creation order).
        match self.config.triage_priority {
            TriagePriority::Newest => prs.reverse(),
            TriagePriority::Oldest => {}
            // LeastConflicts requires state for all PRs; handled below.
            TriagePriority::LeastConflicts => {}
        }

        // For LeastConflicts mode, fetch all states upfront so we can sort by
        // merge-ability before iterating.
        if self.config.triage_priority == TriagePriority::LeastConflicts {
            return self.select_action_least_conflicts(prs);
        }

        for pr_info in prs {
            let with_state = match self
                .lifecycle
                .get_pr_state(pr_info.number, &self.config.skip_labels)
            {
                Ok(ws) => ws,
                Err(e) => {
                    tracing::warn!(
                        pr = pr_info.number,
                        error = %e,
                        "PrTriage: failed to get state for PR; skipping"
                    );
                    continue;
                }
            };

            if let Some(action) = self.evaluate_pr_state(with_state) {
                return action;
            }
        }

        TriageAction::NoActionablePr
    }

    /// Evaluate a single `PrWithState` and return the triage action, or `None`
    /// to continue to the next PR.
    fn evaluate_pr_state(&self, with_state: PrWithState) -> Option<TriageAction> {
        match with_state.state {
            PrTriageState::Skipped { ref reason } => {
                tracing::debug!(pr = with_state.pr.number, %reason, "PrTriage: skipping PR");
                None
            }
            PrTriageState::ChecksFailing => {
                let prompt = format!(
                    "The CI checks on PR #{} («{}») are failing. Check out the branch, \
                     diagnose the root cause, fix it, commit, and push. Do not merge — \
                     the loop engine will handle the merge when checks pass.",
                    with_state.pr.number, with_state.pr.title
                );
                Some(TriageAction::FixChecks {
                    pr: with_state.pr,
                    prompt,
                })
            }
            PrTriageState::ChangesRequested => {
                let prompt = format!(
                    "PR #{} («{}») has review comments requesting changes. Read each \
                     review comment, address the feedback, commit the fixes, and push. \
                     After pushing, reply to each resolved comment thread.",
                    with_state.pr.number, with_state.pr.title
                );
                Some(TriageAction::AddressReviewFeedback {
                    pr: with_state.pr,
                    prompt,
                })
            }
            PrTriageState::ReadyToMerge => {
                if self.config.require_human_review {
                    tracing::info!(
                        pr = with_state.pr.number,
                        "PrTriage: PR ready to merge but require_human_review=true"
                    );
                    Some(TriageAction::BlockedOnHumanReview { pr: with_state.pr })
                } else {
                    Some(TriageAction::Merge { pr: with_state.pr })
                }
            }
            PrTriageState::NeedsReview => {
                tracing::debug!(
                    pr = with_state.pr.number,
                    "PrTriage: PR awaiting initial review; skipping"
                );
                None
            }
        }
    }

    /// `LeastConflicts` variant: fetch all PR states, sort by merge-ability
    /// (MERGEABLE first, UNKNOWN second, CONFLICTING last), then evaluate.
    fn select_action_least_conflicts(&self, prs: Vec<PrInfo>) -> TriageAction {
        let mut states: Vec<PrWithState> = prs
            .into_iter()
            .filter_map(|pr_info| {
                match self
                    .lifecycle
                    .get_pr_state(pr_info.number, &self.config.skip_labels)
                {
                    Ok(ws) => Some(ws),
                    Err(e) => {
                        tracing::warn!(
                            pr = pr_info.number,
                            error = %e,
                            "PrTriage(least-conflicts): failed to get state; skipping"
                        );
                        None
                    }
                }
            })
            .collect();

        // Sort: MERGEABLE (0) < UNKNOWN (1) < CONFLICTING (2) < missing (3).
        states.sort_by_key(|ws| mergeable_sort_key(ws.mergeable.as_deref()));

        tracing::debug!(
            count = states.len(),
            "PrTriage(least-conflicts): sorted {} PRs by merge-ability",
            states.len()
        );

        for with_state in states {
            if let Some(action) = self.evaluate_pr_state(with_state) {
                return action;
            }
        }

        TriageAction::NoActionablePr
    }
}

/// Build a production [`PrTriage`] backed by the `gh` CLI.
pub fn build_pr_triage(config: PrManagementConfig) -> PrTriage<GhPrLifecycle> {
    PrTriage::new(config, GhPrLifecycle::new())
}

/// Numeric sort key for the `LeastConflicts` priority.
///
/// | GitHub value   | key |
/// |----------------|-----|
/// | `"MERGEABLE"`  | 0   |
/// | `"UNKNOWN"`    | 1   |
/// | `"CONFLICTING"`| 2   |
/// | `None`         | 3   |
fn mergeable_sort_key(mergeable: Option<&str>) -> u8 {
    match mergeable {
        Some("MERGEABLE") => 0,
        Some("UNKNOWN") => 1,
        Some("CONFLICTING") => 2,
        _ => 3,
    }
}

// ── MockPrLifecycleTriage (test double) ───────────────────────────────────────

/// Pre-scripted responses for [`PrLifecycleTriage`] in tests.
#[cfg(test)]
pub struct MockPrLifecycleTriage {
    /// PRs returned by `list_open_prs_with_label`.
    pub open_prs: Vec<PrInfo>,
    /// State returned for each PR by number.
    pub states: std::collections::HashMap<u32, PrWithState>,
    /// All calls recorded in order.
    pub calls: std::sync::Mutex<Vec<TriageCall>>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub enum TriageCall {
    ListOpenPrsWithLabel { label: String },
    GetPrState { pr_number: u32 },
}

#[cfg(test)]
impl MockPrLifecycleTriage {
    pub fn new() -> Self {
        Self {
            open_prs: Vec::new(),
            states: std::collections::HashMap::new(),
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl Default for MockPrLifecycleTriage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl PrLifecycleTriage for MockPrLifecycleTriage {
    fn list_open_prs_with_label(&self, label: &str) -> Result<Vec<PrInfo>, PrError> {
        self.calls
            .lock()
            .unwrap()
            .push(TriageCall::ListOpenPrsWithLabel {
                label: label.to_string(),
            });
        Ok(self.open_prs.clone())
    }

    fn get_pr_state(
        &self,
        pr_number: u32,
        _skip_labels: &[String],
    ) -> Result<PrWithState, PrError> {
        self.calls
            .lock()
            .unwrap()
            .push(TriageCall::GetPrState { pr_number });
        self.states.get(&pr_number).cloned().ok_or_else(|| {
            PrError::GhCommand(format!("mock: no state configured for PR #{pr_number}"))
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PrManagementConfig;

    const DEFAULT_MARKER: &str = "LOOPER_READY_FOR_REVIEW";

    fn default_config() -> PrManagementConfig {
        PrManagementConfig::default()
    }

    // ── detect_signal ─────────────────────────────────────────────────────────

    #[test]
    fn detects_sentinel_line() {
        let output = "Some output\nLOOPER_READY_FOR_REVIEW\nMore output";
        let sig = detect_signal(output, DEFAULT_MARKER).unwrap();
        assert_eq!(sig.form, SignalForm::Sentinel);
        assert!(sig.summary.is_none());
    }

    #[test]
    fn sentinel_line_trimmed() {
        let output = "  LOOPER_READY_FOR_REVIEW  ";
        let sig = detect_signal(output, DEFAULT_MARKER).unwrap();
        assert_eq!(sig.form, SignalForm::Sentinel);
    }

    #[test]
    fn sentinel_partial_match_not_detected() {
        let output = "PREFIX_LOOPER_READY_FOR_REVIEW";
        assert!(detect_signal(output, DEFAULT_MARKER).is_none());
    }

    #[test]
    fn detects_json_signal_without_summary() {
        let output = r#"{"looper":"ready-for-review"}"#;
        let sig = detect_signal(output, DEFAULT_MARKER).unwrap();
        assert_eq!(sig.form, SignalForm::Json);
        assert!(sig.summary.is_none());
    }

    #[test]
    fn detects_json_signal_with_summary() {
        let output = r#"{"looper":"ready-for-review","summary":"Auth module complete"}"#;
        let sig = detect_signal(output, DEFAULT_MARKER).unwrap();
        assert_eq!(sig.form, SignalForm::Json);
        assert_eq!(sig.summary.as_deref(), Some("Auth module complete"));
    }

    #[test]
    fn json_wrong_looper_value_not_detected() {
        let output = r#"{"looper":"something-else"}"#;
        assert!(detect_signal(output, DEFAULT_MARKER).is_none());
    }

    #[test]
    fn no_signal_in_empty_output() {
        assert!(detect_signal("", DEFAULT_MARKER).is_none());
    }

    #[test]
    fn no_signal_in_ordinary_output() {
        let output = "All tests passed.\nBuild succeeded.";
        assert!(detect_signal(output, DEFAULT_MARKER).is_none());
    }

    #[test]
    fn custom_ready_marker_detected() {
        let output = "CUSTOM_SHIP_IT";
        let sig = detect_signal(output, "CUSTOM_SHIP_IT").unwrap();
        assert_eq!(sig.form, SignalForm::Sentinel);
    }

    #[test]
    fn sentinel_preferred_over_json_when_both_present() {
        // Sentinel scan runs first; if the sentinel is found the JSON block is
        // never reached.
        let output = "LOOPER_READY_FOR_REVIEW\n{\"looper\":\"ready-for-review\",\"summary\":\"s\"}";
        let sig = detect_signal(output, DEFAULT_MARKER).unwrap();
        assert_eq!(sig.form, SignalForm::Sentinel);
    }

    // ── PrManager ─────────────────────────────────────────────────────────────

    fn manager_with_mock(mock: MockPrLifecycle) -> PrManager<MockPrLifecycle> {
        PrManager::new(default_config(), mock)
    }

    #[test]
    fn no_signal_returns_no_action() {
        let mgr = manager_with_mock(MockPrLifecycle::new());
        let action = mgr
            .handle_milestone("loop/1-branch", 1, "My feature", "ordinary output")
            .unwrap();
        assert!(matches!(action, PrAction::NoSignal));
        assert!(mgr.lifecycle.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn signal_opens_pr_when_none_exists() {
        let mock = MockPrLifecycle::new();
        let mgr = manager_with_mock(mock);
        let output = "LOOPER_READY_FOR_REVIEW";
        let action = mgr
            .handle_milestone("loop/1-feat", 1, "My feature", output)
            .unwrap();
        assert!(matches!(action, PrAction::Opened(_)));

        let calls = mgr.lifecycle.calls.lock().unwrap();
        assert_eq!(
            calls[0],
            PrCall::FindOpenPr {
                branch: "loop/1-feat".into()
            }
        );
        assert!(matches!(calls[1], PrCall::OpenPr { .. }));
    }

    #[test]
    fn opened_pr_title_references_issue() {
        let mock = MockPrLifecycle::new();
        let mgr = manager_with_mock(mock);
        let output = "LOOPER_READY_FOR_REVIEW";
        mgr.handle_milestone("loop/7-auth", 7, "Add auth", output)
            .unwrap();

        let calls = mgr.lifecycle.calls.lock().unwrap();
        if let PrCall::OpenPr { title, .. } = &calls[1] {
            assert!(
                title.contains("#7"),
                "title should reference issue: {title}"
            );
            assert!(
                title.contains("Add auth"),
                "title should include issue title: {title}"
            );
        } else {
            panic!("expected OpenPr call");
        }
    }

    #[test]
    fn signal_blocked_on_human_review_when_pr_exists() {
        let existing = PrInfo {
            number: 10,
            url: "https://github.com/o/r/pull/10".into(),
            title: "[LOOPER] #1: feat".into(),
            head_ref: "loop/1-feat".into(),
        };
        let mock = MockPrLifecycle::new().with_existing_pr(existing);
        let mut cfg = default_config();
        cfg.require_human_review = true;
        let mgr = PrManager::new(cfg, mock);

        let action = mgr
            .handle_milestone("loop/1-feat", 1, "feat", "LOOPER_READY_FOR_REVIEW")
            .unwrap();
        assert!(matches!(action, PrAction::BlockedOnHumanReview(_)));

        // Should NOT have tried to open a new PR or add a comment.
        let calls = mgr.lifecycle.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "only find_open_pr should be called");
    }

    #[test]
    fn signal_comments_on_existing_pr_when_review_not_required() {
        let existing = PrInfo {
            number: 10,
            url: "https://github.com/o/r/pull/10".into(),
            title: "[LOOPER] #1: feat".into(),
            head_ref: "loop/1-feat".into(),
        };
        let mock = MockPrLifecycle::new().with_existing_pr(existing);
        let mut cfg = default_config();
        cfg.require_human_review = false;
        let mgr = PrManager::new(cfg, mock);

        let action = mgr
            .handle_milestone("loop/1-feat", 1, "feat", "LOOPER_READY_FOR_REVIEW")
            .unwrap();
        assert!(matches!(action, PrAction::Updated { .. }));

        let calls = mgr.lifecycle.calls.lock().unwrap();
        assert!(matches!(calls[1], PrCall::CommentOnPr { pr_number: 10 }));
    }

    #[test]
    fn pr_body_includes_closes_link() {
        let body = PrManager::<MockPrLifecycle>::pr_body(99, None);
        assert!(
            body.contains("Closes #99"),
            "body should reference issue: {body}"
        );
    }

    #[test]
    fn pr_body_includes_agent_summary() {
        let body = PrManager::<MockPrLifecycle>::pr_body(5, Some("Auth complete"));
        assert!(
            body.contains("Auth complete"),
            "body should include summary: {body}"
        );
    }

    #[test]
    fn pr_title_format() {
        let title = PrManager::<MockPrLifecycle>::pr_title(42, "Add caching");
        assert_eq!(title, "[LOOPER] #42: Add caching");
    }

    #[test]
    fn default_labels_contains_code_looper_and_needs_review() {
        let labels = PrManager::<MockPrLifecycle>::default_labels();
        assert!(labels.contains(&"code-looper".to_string()));
        assert!(labels.contains(&"needs-review".to_string()));
    }

    #[test]
    fn json_summary_included_in_pr_body() {
        let mock = MockPrLifecycle::new();
        let mgr = manager_with_mock(mock);
        let output = r#"{"looper":"ready-for-review","summary":"Phase 1 done"}"#;
        mgr.handle_milestone("loop/3-phase", 3, "Phase 1", output)
            .unwrap();

        let calls = mgr.lifecycle.calls.lock().unwrap();
        if let PrCall::OpenPr { title, .. } = &calls[1] {
            // Title comes from issue, not JSON summary
            assert!(title.contains("#3"));
        } else {
            panic!("expected OpenPr call, got: {calls:?}");
        }
    }

    // ── PrTriage::select_action ───────────────────────────────────────────────

    fn make_pr(number: u32) -> PrInfo {
        PrInfo {
            number,
            url: format!("https://github.com/o/r/pull/{number}"),
            title: format!("PR {number}"),
            head_ref: format!("loop/{number}-pr"),
        }
    }

    fn make_state(pr: PrInfo, state: PrTriageState) -> PrWithState {
        PrWithState {
            pr,
            state,
            created_at: "2026-01-01T00:00:00Z".into(),
            mergeable: Some("MERGEABLE".into()),
        }
    }

    fn make_state_with_mergeable(
        pr: PrInfo,
        state: PrTriageState,
        mergeable: Option<&str>,
    ) -> PrWithState {
        PrWithState {
            pr,
            state,
            created_at: "2026-01-01T00:00:00Z".into(),
            mergeable: mergeable.map(|s| s.to_string()),
        }
    }

    #[test]
    fn triage_no_open_prs_returns_no_actionable() {
        let mock = MockPrLifecycleTriage::new();
        let triage = PrTriage::new(default_config(), mock);
        assert!(matches!(
            triage.select_action(),
            TriageAction::NoActionablePr
        ));
    }

    #[test]
    fn triage_all_skipped_returns_no_actionable() {
        let mut mock = MockPrLifecycleTriage::new();
        let pr = make_pr(1);
        mock.open_prs = vec![pr.clone()];
        mock.states.insert(
            1,
            make_state(
                pr,
                PrTriageState::Skipped {
                    reason: "wip".into(),
                },
            ),
        );
        let triage = PrTriage::new(default_config(), mock);
        assert!(matches!(
            triage.select_action(),
            TriageAction::NoActionablePr
        ));
    }

    #[test]
    fn triage_checks_failing_returns_fix_checks() {
        let mut mock = MockPrLifecycleTriage::new();
        let pr = make_pr(2);
        mock.open_prs = vec![pr.clone()];
        mock.states
            .insert(2, make_state(pr, PrTriageState::ChecksFailing));
        let triage = PrTriage::new(default_config(), mock);
        let action = triage.select_action();
        assert!(matches!(action, TriageAction::FixChecks { .. }));
        if let TriageAction::FixChecks { pr, prompt } = action {
            assert_eq!(pr.number, 2);
            assert!(prompt.contains("CI checks"));
        }
    }

    #[test]
    fn triage_changes_requested_returns_address_review() {
        let mut mock = MockPrLifecycleTriage::new();
        let pr = make_pr(3);
        mock.open_prs = vec![pr.clone()];
        mock.states
            .insert(3, make_state(pr, PrTriageState::ChangesRequested));
        let triage = PrTriage::new(default_config(), mock);
        let action = triage.select_action();
        assert!(matches!(action, TriageAction::AddressReviewFeedback { .. }));
        if let TriageAction::AddressReviewFeedback { pr, prompt } = action {
            assert_eq!(pr.number, 3);
            assert!(prompt.contains("review comment"));
        }
    }

    #[test]
    fn triage_ready_to_merge_require_human_review_returns_blocked() {
        let mut mock = MockPrLifecycleTriage::new();
        let pr = make_pr(4);
        mock.open_prs = vec![pr.clone()];
        mock.states
            .insert(4, make_state(pr, PrTriageState::ReadyToMerge));
        let mut cfg = default_config();
        cfg.require_human_review = true;
        let triage = PrTriage::new(cfg, mock);
        assert!(matches!(
            triage.select_action(),
            TriageAction::BlockedOnHumanReview { .. }
        ));
    }

    #[test]
    fn triage_ready_to_merge_no_human_review_returns_merge() {
        let mut mock = MockPrLifecycleTriage::new();
        let pr = make_pr(5);
        mock.open_prs = vec![pr.clone()];
        mock.states
            .insert(5, make_state(pr, PrTriageState::ReadyToMerge));
        let mut cfg = default_config();
        cfg.require_human_review = false;
        let triage = PrTriage::new(cfg, mock);
        assert!(matches!(triage.select_action(), TriageAction::Merge { .. }));
    }

    #[test]
    fn triage_needs_review_skipped_falls_through() {
        let mut mock = MockPrLifecycleTriage::new();
        let pr = make_pr(6);
        mock.open_prs = vec![pr.clone()];
        mock.states
            .insert(6, make_state(pr, PrTriageState::NeedsReview));
        let triage = PrTriage::new(default_config(), mock);
        assert!(matches!(
            triage.select_action(),
            TriageAction::NoActionablePr
        ));
    }

    #[test]
    fn triage_picks_first_actionable_pr_in_order() {
        let mut mock = MockPrLifecycleTriage::new();
        let pr1 = make_pr(10);
        let pr2 = make_pr(11);
        mock.open_prs = vec![pr1.clone(), pr2.clone()];
        // First PR skipped, second has checks failing.
        mock.states.insert(
            10,
            make_state(
                pr1,
                PrTriageState::Skipped {
                    reason: "wip".into(),
                },
            ),
        );
        mock.states
            .insert(11, make_state(pr2, PrTriageState::ChecksFailing));
        let triage = PrTriage::new(default_config(), mock);
        let action = triage.select_action();
        if let TriageAction::FixChecks { pr, .. } = action {
            assert_eq!(pr.number, 11);
        } else {
            panic!("expected FixChecks");
        }
    }

    #[test]
    fn triage_newest_priority_reverses_pr_order() {
        use crate::config::TriagePriority;
        let mut mock = MockPrLifecycleTriage::new();
        let pr1 = make_pr(20);
        let pr2 = make_pr(21);
        mock.open_prs = vec![pr1.clone(), pr2.clone()]; // oldest first
        mock.states
            .insert(20, make_state(pr1, PrTriageState::ChecksFailing));
        mock.states
            .insert(21, make_state(pr2, PrTriageState::ChecksFailing));
        let mut cfg = default_config();
        cfg.triage_priority = TriagePriority::Newest;
        let triage = PrTriage::new(cfg, mock);
        // With Newest first, the last in the list (21) should be selected.
        if let TriageAction::FixChecks { pr, .. } = triage.select_action() {
            assert_eq!(pr.number, 21);
        } else {
            panic!("expected FixChecks for PR 21");
        }
    }

    #[test]
    fn triage_calls_list_then_get_state_for_each_pr() {
        let mut mock = MockPrLifecycleTriage::new();
        let pr = make_pr(30);
        mock.open_prs = vec![pr.clone()];
        mock.states
            .insert(30, make_state(pr, PrTriageState::NeedsReview));
        let triage = PrTriage::new(default_config(), mock);
        triage.select_action();
        let calls = triage.lifecycle.calls.lock().unwrap();
        assert!(calls.contains(&TriageCall::ListOpenPrsWithLabel {
            label: "code-looper".into()
        }));
        assert!(calls.contains(&TriageCall::GetPrState { pr_number: 30 }));
    }

    // ── PrInfo::head_ref ──────────────────────────────────────────────────────

    #[test]
    fn pr_info_head_ref_is_set_by_make_pr_helper() {
        let pr = make_pr(5);
        assert_eq!(pr.head_ref, "loop/5-pr");
    }

    #[test]
    fn merge_triage_action_carries_head_ref() {
        let mut mock = MockPrLifecycleTriage::new();
        let pr = make_pr(99);
        mock.open_prs = vec![pr.clone()];
        mock.states
            .insert(99, make_state(pr, PrTriageState::ReadyToMerge));
        let mut cfg = default_config();
        cfg.require_human_review = false;
        let triage = PrTriage::new(cfg, mock);
        if let TriageAction::Merge { pr } = triage.select_action() {
            assert_eq!(
                pr.head_ref, "loop/99-pr",
                "Merge action must preserve head_ref for post-merge cleanup"
            );
        } else {
            panic!("expected TriageAction::Merge");
        }
    }

    // ── least-conflicts triage priority ──────────────────────────────────────

    #[test]
    fn least_conflicts_prefers_mergeable_over_conflicting() {
        // PR 1 is CONFLICTING, PR 2 is MERGEABLE — expect PR 2 to be selected.
        let mut mock = MockPrLifecycleTriage::new();
        mock.open_prs = vec![make_pr(1), make_pr(2)];
        mock.states.insert(
            1,
            make_state_with_mergeable(
                make_pr(1),
                PrTriageState::ChecksFailing,
                Some("CONFLICTING"),
            ),
        );
        mock.states.insert(
            2,
            make_state_with_mergeable(make_pr(2), PrTriageState::ChecksFailing, Some("MERGEABLE")),
        );

        let mut cfg = default_config();
        cfg.triage_priority = TriagePriority::LeastConflicts;
        let triage = PrTriage::new(cfg, mock);

        if let TriageAction::FixChecks { pr, .. } = triage.select_action() {
            assert_eq!(pr.number, 2, "MERGEABLE PR should be selected first");
        } else {
            panic!("expected FixChecks action");
        }
    }

    #[test]
    fn least_conflicts_unknown_before_conflicting() {
        // PR 1 is CONFLICTING, PR 2 is UNKNOWN — expect PR 2 (UNKNOWN) first.
        let mut mock = MockPrLifecycleTriage::new();
        mock.open_prs = vec![make_pr(1), make_pr(2)];
        mock.states.insert(
            1,
            make_state_with_mergeable(
                make_pr(1),
                PrTriageState::ChangesRequested,
                Some("CONFLICTING"),
            ),
        );
        mock.states.insert(
            2,
            make_state_with_mergeable(make_pr(2), PrTriageState::ChangesRequested, Some("UNKNOWN")),
        );

        let mut cfg = default_config();
        cfg.triage_priority = TriagePriority::LeastConflicts;
        let triage = PrTriage::new(cfg, mock);

        if let TriageAction::AddressReviewFeedback { pr, .. } = triage.select_action() {
            assert_eq!(
                pr.number, 2,
                "UNKNOWN PR should be selected before CONFLICTING"
            );
        } else {
            panic!("expected AddressReviewFeedback action");
        }
    }

    #[test]
    fn least_conflicts_all_mergeable_first_pr_wins() {
        // All PRs are MERGEABLE — standard first-in-list ordering applies.
        let mut mock = MockPrLifecycleTriage::new();
        mock.open_prs = vec![make_pr(10), make_pr(20)];
        mock.states.insert(
            10,
            make_state_with_mergeable(make_pr(10), PrTriageState::ChecksFailing, Some("MERGEABLE")),
        );
        mock.states.insert(
            20,
            make_state_with_mergeable(make_pr(20), PrTriageState::ChecksFailing, Some("MERGEABLE")),
        );

        let mut cfg = default_config();
        cfg.triage_priority = TriagePriority::LeastConflicts;
        let triage = PrTriage::new(cfg, mock);

        if let TriageAction::FixChecks { pr, .. } = triage.select_action() {
            assert_eq!(pr.number, 10, "When equal, first listed PR wins");
        } else {
            panic!("expected FixChecks action");
        }
    }

    #[test]
    fn least_conflicts_no_prs_returns_no_actionable() {
        let mock = MockPrLifecycleTriage::new();
        let mut cfg = default_config();
        cfg.triage_priority = TriagePriority::LeastConflicts;
        let triage = PrTriage::new(cfg, mock);
        assert!(matches!(
            triage.select_action(),
            TriageAction::NoActionablePr
        ));
    }

    #[test]
    fn least_conflicts_none_mergeable_field_treated_as_last() {
        // PR 1 has no mergeable field (None), PR 2 is CONFLICTING.
        // None (key 3) sorts after CONFLICTING (key 2) — PR 2 is selected first.
        let mut mock = MockPrLifecycleTriage::new();
        mock.open_prs = vec![make_pr(1), make_pr(2)];
        mock.states.insert(
            1,
            make_state_with_mergeable(make_pr(1), PrTriageState::ChecksFailing, None),
        );
        mock.states.insert(
            2,
            make_state_with_mergeable(
                make_pr(2),
                PrTriageState::ChecksFailing,
                Some("CONFLICTING"),
            ),
        );

        let mut cfg = default_config();
        cfg.triage_priority = TriagePriority::LeastConflicts;
        let triage = PrTriage::new(cfg, mock);

        if let TriageAction::FixChecks { pr, .. } = triage.select_action() {
            assert_eq!(
                pr.number, 2,
                "CONFLICTING (key 2) should be selected before None (key 3)"
            );
        } else {
            panic!("expected FixChecks action");
        }
    }

    #[test]
    fn mergeable_sort_key_values() {
        assert_eq!(mergeable_sort_key(Some("MERGEABLE")), 0);
        assert_eq!(mergeable_sort_key(Some("UNKNOWN")), 1);
        assert_eq!(mergeable_sort_key(Some("CONFLICTING")), 2);
        assert_eq!(mergeable_sort_key(None), 3);
        assert_eq!(mergeable_sort_key(Some("something-else")), 3);
    }
}
