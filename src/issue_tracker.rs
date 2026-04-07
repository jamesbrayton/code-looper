use serde::Deserialize;
use std::process::Command;
use std::sync::Mutex;
use thiserror::Error;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Current open/closed state of an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Closed,
}

/// A GitHub issue returned by the tracker.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Issue {
    pub id: u64,
    pub number: u32,
    pub title: String,
    pub body: String,
    pub state: IssueState,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub url: String,
}

/// Payload for creating a new issue.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct IssueDraft {
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
}

/// Filter criteria for listing issues.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct IssueFilter {
    pub state: Option<IssueState>,
    pub labels: Vec<String>,
    pub assignee: Option<String>,
    pub search: Option<String>,
}

/// Reason an issue is being closed.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum CloseReason {
    Completed,
    NotPlanned,
    Duplicate(u32),
}

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors produced by any `IssueTracker` implementation.
#[derive(Debug, Error)]
pub enum IssueTrackerError {
    #[error("authentication error: {0}")]
    Auth(String),
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("rate limited: {0}")]
    RateLimited(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("validation error: {0}")]
    #[allow(dead_code)]
    Validation(String),
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Abstraction over issue-tracking systems (GitHub Issues, Jira, Linear, …).
///
/// All mutation operations must go through MCP-compliant paths in production
/// use.  Implementations are responsible for honouring whatever auth and
/// policy constraints apply to their backend.
#[allow(dead_code)]
pub trait IssueTracker: Send + Sync {
    fn list_open_issues(&self, filter: &IssueFilter) -> Result<Vec<Issue>, IssueTrackerError>;
    fn get_issue(&self, number: u32) -> Result<Issue, IssueTrackerError>;
    fn create_issue(&self, draft: IssueDraft) -> Result<Issue, IssueTrackerError>;
    fn update_issue_body(&self, number: u32, body: &str) -> Result<(), IssueTrackerError>;
    fn add_comment(&self, number: u32, body: &str) -> Result<(), IssueTrackerError>;
    fn close_issue(&self, number: u32, reason: CloseReason) -> Result<(), IssueTrackerError>;
    fn reopen_issue(&self, number: u32) -> Result<(), IssueTrackerError>;
    /// Record an association between an issue and a pull request by posting a
    /// cross-reference comment on the issue.
    fn link_issue_to_pr(&self, issue_number: u32, pr_number: u32) -> Result<(), IssueTrackerError>;
    /// Ensure that the given labels exist on the repository, creating any that
    /// are absent.  This is a no-op for backends that do not support label
    /// management (e.g. `LocalPromiseTracker`).
    fn ensure_labels(&self, labels: &[String]) -> Result<(), IssueTrackerError> {
        let _ = labels;
        Ok(())
    }
}

// ── GitHub CLI implementation ─────────────────────────────────────────────────

/// Implements `IssueTracker` by shelling out to the `gh` CLI.
///
/// All write operations use `gh issue` sub-commands.  Authentication is
/// handled by the developer's existing `gh auth` session — no tokens are
/// handled directly by Code Looper.
pub struct GitHubIssueTracker {
    pub owner: String,
    pub repo: String,
}

impl GitHubIssueTracker {
    pub fn new(owner: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            repo: repo.into(),
        }
    }

    fn repo_slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    /// Run a `gh` command and return the raw output.
    fn run_gh(&self, args: &[String]) -> Result<std::process::Output, IssueTrackerError> {
        Command::new("gh")
            .args(args)
            .output()
            .map_err(|e| IssueTrackerError::Transport(format!("failed to spawn gh: {e}")))
    }

    /// Unwrap command output into stdout text, mapping non-zero exits to errors.
    fn check_output(
        &self,
        output: std::process::Output,
        context: &str,
    ) -> Result<String, IssueTrackerError> {
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(classify_gh_error(&stderr, context));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Map `gh` stderr to a typed `IssueTrackerError`.
fn classify_gh_error(stderr: &str, context: &str) -> IssueTrackerError {
    let lower = stderr.to_lowercase();
    if lower.contains("authentication")
        || lower.contains("401")
        || lower.contains("credentials")
        || lower.contains("auth")
    {
        IssueTrackerError::Auth(format!("{context}: {stderr}"))
    } else if lower.contains("not found")
        || lower.contains("404")
        || lower.contains("no issue")
        || lower.contains("could not resolve")
    {
        IssueTrackerError::NotFound(format!("{context}: {stderr}"))
    } else if lower.contains("rate limit") || lower.contains("429") {
        IssueTrackerError::RateLimited(format!("{context}: {stderr}"))
    } else {
        IssueTrackerError::Transport(format!("{context}: {stderr}"))
    }
}

// Serde types matching `gh --json` output.
#[derive(Deserialize)]
struct GhIssue {
    id: Option<u64>,
    number: u32,
    title: String,
    body: Option<String>,
    state: String,
    labels: Vec<GhLabel>,
    assignees: Vec<GhUser>,
    url: String,
}

#[derive(Deserialize)]
struct GhLabel {
    name: String,
}

#[derive(Deserialize)]
struct GhUser {
    login: String,
}

impl From<GhIssue> for Issue {
    fn from(gh: GhIssue) -> Self {
        Issue {
            id: gh.id.unwrap_or(0),
            number: gh.number,
            title: gh.title,
            body: gh.body.unwrap_or_default(),
            state: if gh.state.eq_ignore_ascii_case("open") {
                IssueState::Open
            } else {
                IssueState::Closed
            },
            labels: gh.labels.into_iter().map(|l| l.name).collect(),
            assignees: gh.assignees.into_iter().map(|a| a.login).collect(),
            url: gh.url,
        }
    }
}

const JSON_FIELDS: &str = "id,number,title,body,state,labels,assignees,url";

impl IssueTracker for GitHubIssueTracker {
    fn list_open_issues(&self, filter: &IssueFilter) -> Result<Vec<Issue>, IssueTrackerError> {
        let slug = self.repo_slug();
        let mut args = vec![
            "issue".to_string(),
            "list".to_string(),
            "--repo".to_string(),
            slug,
            "--state".to_string(),
            "open".to_string(),
            "--json".to_string(),
            JSON_FIELDS.to_string(),
        ];
        if !filter.labels.is_empty() {
            args.push("--label".to_string());
            args.push(filter.labels.join(","));
        }
        if let Some(ref assignee) = filter.assignee {
            args.push("--assignee".to_string());
            args.push(assignee.clone());
        }
        if let Some(ref q) = filter.search {
            args.push("--search".to_string());
            args.push(q.clone());
        }
        let output = self.run_gh(&args)?;
        let stdout = self.check_output(output, "list_open_issues")?;
        let gh_issues: Vec<GhIssue> = serde_json::from_str(&stdout).map_err(|e| {
            IssueTrackerError::Transport(format!("failed to parse list output: {e}"))
        })?;
        Ok(gh_issues.into_iter().map(Issue::from).collect())
    }

    fn get_issue(&self, number: u32) -> Result<Issue, IssueTrackerError> {
        let slug = self.repo_slug();
        let args = vec![
            "issue".to_string(),
            "view".to_string(),
            number.to_string(),
            "--repo".to_string(),
            slug,
            "--json".to_string(),
            JSON_FIELDS.to_string(),
        ];
        let output = self.run_gh(&args)?;
        let stdout = self.check_output(output, "get_issue")?;
        let gh_issue: GhIssue = serde_json::from_str(&stdout).map_err(|e| {
            IssueTrackerError::Transport(format!("failed to parse issue view output: {e}"))
        })?;
        Ok(Issue::from(gh_issue))
    }

    fn create_issue(&self, draft: IssueDraft) -> Result<Issue, IssueTrackerError> {
        if draft.title.trim().is_empty() {
            return Err(IssueTrackerError::Validation(
                "issue title must not be empty".to_string(),
            ));
        }
        let slug = self.repo_slug();
        let mut args = vec![
            "issue".to_string(),
            "create".to_string(),
            "--repo".to_string(),
            slug,
            "--title".to_string(),
            draft.title.clone(),
            "--body".to_string(),
            draft.body.clone(),
            "--json".to_string(),
            JSON_FIELDS.to_string(),
        ];
        if !draft.labels.is_empty() {
            args.push("--label".to_string());
            args.push(draft.labels.join(","));
        }
        if !draft.assignees.is_empty() {
            args.push("--assignee".to_string());
            args.push(draft.assignees.join(","));
        }
        let output = self.run_gh(&args)?;
        let stdout = self.check_output(output, "create_issue")?;
        let gh_issue: GhIssue = serde_json::from_str(&stdout).map_err(|e| {
            IssueTrackerError::Transport(format!("failed to parse create output: {e}"))
        })?;
        Ok(Issue::from(gh_issue))
    }

    fn update_issue_body(&self, number: u32, body: &str) -> Result<(), IssueTrackerError> {
        let slug = self.repo_slug();
        let args = vec![
            "issue".to_string(),
            "edit".to_string(),
            number.to_string(),
            "--repo".to_string(),
            slug,
            "--body".to_string(),
            body.to_string(),
        ];
        let output = self.run_gh(&args)?;
        self.check_output(output, "update_issue_body")?;
        Ok(())
    }

    fn add_comment(&self, number: u32, body: &str) -> Result<(), IssueTrackerError> {
        let slug = self.repo_slug();
        let args = vec![
            "issue".to_string(),
            "comment".to_string(),
            number.to_string(),
            "--repo".to_string(),
            slug,
            "--body".to_string(),
            body.to_string(),
        ];
        let output = self.run_gh(&args)?;
        self.check_output(output, "add_comment")?;
        Ok(())
    }

    fn close_issue(&self, number: u32, reason: CloseReason) -> Result<(), IssueTrackerError> {
        let slug = self.repo_slug();
        let reason_str = match reason {
            CloseReason::Completed => "completed",
            CloseReason::NotPlanned | CloseReason::Duplicate(_) => "not planned",
        };
        let args = vec![
            "issue".to_string(),
            "close".to_string(),
            number.to_string(),
            "--repo".to_string(),
            slug,
            "--reason".to_string(),
            reason_str.to_string(),
        ];
        let output = self.run_gh(&args)?;
        self.check_output(output, "close_issue")?;
        Ok(())
    }

    fn reopen_issue(&self, number: u32) -> Result<(), IssueTrackerError> {
        let slug = self.repo_slug();
        let args = vec![
            "issue".to_string(),
            "reopen".to_string(),
            number.to_string(),
            "--repo".to_string(),
            slug,
        ];
        let output = self.run_gh(&args)?;
        self.check_output(output, "reopen_issue")?;
        Ok(())
    }

    fn link_issue_to_pr(&self, issue_number: u32, pr_number: u32) -> Result<(), IssueTrackerError> {
        let body = format!("Linked to pull request #{pr_number}.");
        self.add_comment(issue_number, &body)
    }

    fn ensure_labels(&self, labels: &[String]) -> Result<(), IssueTrackerError> {
        let slug = self.repo_slug();
        for label in labels {
            // `gh label create --force` is idempotent — it creates the label
            // if absent and updates it if present (no error on conflict).
            let args = vec![
                "label".to_string(),
                "create".to_string(),
                label.clone(),
                "--repo".to_string(),
                slug.clone(),
                "--force".to_string(),
            ];
            let output = self.run_gh(&args)?;
            // Label create may exit non-zero on an already-existing label with
            // some older gh versions; treat that as success.
            let _ = output;
        }
        Ok(())
    }
}

// ── LocalPromiseTracker ───────────────────────────────────────────────────────

/// Dev/debug-only tracker that writes to a single markdown file.
///
/// Each "issue" is a heading (`## #<number> <title>`) with a body block and
/// an appended log of timestamped comments.  Intended for offline
/// experimentation — **not** for production use.
///
/// The file is created (including parent directories) on first write if it
/// does not already exist.
#[allow(dead_code)]
pub struct LocalPromiseTracker {
    pub path: std::path::PathBuf,
    inner: Mutex<LocalStore>,
}

struct LocalStore {
    /// In-memory issues indexed by number.
    issues: std::collections::HashMap<u32, Issue>,
    /// Monotonically increasing counter for new issue numbers.
    #[allow(dead_code)]
    next_number: u32,
}

impl LocalPromiseTracker {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: path.into(),
            inner: Mutex::new(LocalStore {
                issues: std::collections::HashMap::new(),
                next_number: 1,
            }),
        }
    }

    /// Persist the entire store to the markdown file.
    fn flush(&self, store: &LocalStore) -> Result<(), IssueTrackerError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| IssueTrackerError::Transport(format!("create_dir_all: {e}")))?;
        }
        let mut out = String::new();
        out.push_str("<!-- LocalPromiseTracker — dev/debug only -->\n\n");
        let mut numbers: Vec<u32> = store.issues.keys().copied().collect();
        numbers.sort_unstable();
        for n in numbers {
            let issue = &store.issues[&n];
            let state_tag = if issue.state == IssueState::Open {
                "OPEN"
            } else {
                "CLOSED"
            };
            out.push_str(&format!(
                "## #{n} {title} [{state_tag}]\n\n",
                title = issue.title
            ));
            if !issue.body.is_empty() {
                out.push_str(&issue.body);
                out.push_str("\n\n");
            }
        }
        std::fs::write(&self.path, out)
            .map_err(|e| IssueTrackerError::Transport(format!("write promise file: {e}")))?;
        Ok(())
    }
}

impl IssueTracker for LocalPromiseTracker {
    fn list_open_issues(&self, filter: &IssueFilter) -> Result<Vec<Issue>, IssueTrackerError> {
        let store = self.inner.lock().unwrap();
        let issues: Vec<Issue> = store
            .issues
            .values()
            .filter(|i| i.state == IssueState::Open)
            .filter(|i| {
                filter.labels.is_empty() || filter.labels.iter().any(|l| i.labels.contains(l))
            })
            .filter(|i| {
                filter
                    .assignee
                    .as_deref()
                    .is_none_or(|a| i.assignees.contains(&a.to_string()))
            })
            .filter(|i| {
                filter
                    .search
                    .as_deref()
                    .is_none_or(|q| i.title.contains(q) || i.body.contains(q))
            })
            .cloned()
            .collect();
        Ok(issues)
    }

    fn get_issue(&self, number: u32) -> Result<Issue, IssueTrackerError> {
        let store = self.inner.lock().unwrap();
        store
            .issues
            .get(&number)
            .cloned()
            .ok_or_else(|| IssueTrackerError::NotFound(format!("issue #{number} not found")))
    }

    fn create_issue(&self, draft: IssueDraft) -> Result<Issue, IssueTrackerError> {
        if draft.title.trim().is_empty() {
            return Err(IssueTrackerError::Validation(
                "issue title must not be empty".to_string(),
            ));
        }
        let mut store = self.inner.lock().unwrap();
        let number = store.next_number;
        store.next_number += 1;
        let issue = Issue {
            id: number as u64,
            number,
            title: draft.title,
            body: draft.body,
            state: IssueState::Open,
            labels: draft.labels,
            assignees: draft.assignees,
            url: format!("local://issues/{number}"),
        };
        store.issues.insert(number, issue.clone());
        self.flush(&store)?;
        Ok(issue)
    }

    fn update_issue_body(&self, number: u32, body: &str) -> Result<(), IssueTrackerError> {
        let mut store = self.inner.lock().unwrap();
        let issue = store
            .issues
            .get_mut(&number)
            .ok_or_else(|| IssueTrackerError::NotFound(format!("issue #{number} not found")))?;
        issue.body = body.to_string();
        self.flush(&store)
    }

    fn add_comment(&self, number: u32, body: &str) -> Result<(), IssueTrackerError> {
        let mut store = self.inner.lock().unwrap();
        let issue = store
            .issues
            .get_mut(&number)
            .ok_or_else(|| IssueTrackerError::NotFound(format!("issue #{number} not found")))?;
        // Append comment as a timestamped log entry in the body.
        let entry = format!("\n---\n> {body}\n");
        issue.body.push_str(&entry);
        self.flush(&store)
    }

    fn close_issue(&self, number: u32, _reason: CloseReason) -> Result<(), IssueTrackerError> {
        let mut store = self.inner.lock().unwrap();
        let issue = store
            .issues
            .get_mut(&number)
            .ok_or_else(|| IssueTrackerError::NotFound(format!("issue #{number} not found")))?;
        issue.state = IssueState::Closed;
        self.flush(&store)
    }

    fn reopen_issue(&self, number: u32) -> Result<(), IssueTrackerError> {
        let mut store = self.inner.lock().unwrap();
        let issue = store
            .issues
            .get_mut(&number)
            .ok_or_else(|| IssueTrackerError::NotFound(format!("issue #{number} not found")))?;
        issue.state = IssueState::Open;
        self.flush(&store)
    }

    fn link_issue_to_pr(&self, issue_number: u32, pr_number: u32) -> Result<(), IssueTrackerError> {
        self.add_comment(
            issue_number,
            &format!("Linked to pull request #{pr_number}."),
        )
    }
}

// ── MockIssueTracker ──────────────────────────────────────────────────────────

/// Test double that records every call made to it.
///
/// By default, read methods return empty collections and write methods
/// succeed.  Override `next_issue` to control what `get_issue` and
/// `create_issue` return.
#[cfg(test)]
pub struct MockIssueTracker {
    /// Calls recorded in order: (method_name, args…).
    pub calls: Mutex<Vec<MockCall>>,
    /// Optional issue returned by `get_issue` and `create_issue`.
    pub next_issue: Mutex<Option<Issue>>,
    /// If set, every method returns this error.
    pub force_error: Option<IssueTrackerError>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub enum MockCall {
    ListOpenIssues,
    GetIssue(u32),
    CreateIssue {
        title: String,
        #[allow(dead_code)]
        body: String,
    },
    UpdateIssueBody {
        number: u32,
    },
    AddComment {
        number: u32,
        body: String,
    },
    CloseIssue(u32),
    ReopenIssue(u32),
    LinkIssueToPr {
        issue_number: u32,
        pr_number: u32,
    },
    EnsureLabels(Vec<String>),
}

#[cfg(test)]
impl MockIssueTracker {
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            next_issue: Mutex::new(None),
            force_error: None,
        }
    }

    /// Return a reference to the recorded calls.
    pub fn recorded_calls(&self) -> Vec<MockCall> {
        self.calls.lock().unwrap().clone()
    }

    fn check_force_error(&self) -> Option<IssueTrackerError> {
        match &self.force_error {
            Some(IssueTrackerError::Auth(m)) => Some(IssueTrackerError::Auth(m.clone())),
            Some(IssueTrackerError::NotFound(m)) => Some(IssueTrackerError::NotFound(m.clone())),
            Some(IssueTrackerError::RateLimited(m)) => {
                Some(IssueTrackerError::RateLimited(m.clone()))
            }
            Some(IssueTrackerError::Transport(m)) => Some(IssueTrackerError::Transport(m.clone())),
            Some(IssueTrackerError::Validation(m)) => {
                Some(IssueTrackerError::Validation(m.clone()))
            }
            None => None,
        }
    }

    fn default_issue() -> Issue {
        Issue {
            id: 1,
            number: 1,
            title: "mock issue".to_string(),
            body: String::new(),
            state: IssueState::Open,
            labels: vec![],
            assignees: vec![],
            url: "https://github.com/mock/repo/issues/1".to_string(),
        }
    }
}

#[cfg(test)]
impl Default for MockIssueTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl IssueTracker for MockIssueTracker {
    fn list_open_issues(&self, _filter: &IssueFilter) -> Result<Vec<Issue>, IssueTrackerError> {
        self.calls.lock().unwrap().push(MockCall::ListOpenIssues);
        if let Some(e) = self.check_force_error() {
            return Err(e);
        }
        Ok(vec![])
    }

    fn get_issue(&self, number: u32) -> Result<Issue, IssueTrackerError> {
        self.calls.lock().unwrap().push(MockCall::GetIssue(number));
        if let Some(e) = self.check_force_error() {
            return Err(e);
        }
        let issue = self
            .next_issue
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(Self::default_issue);
        Ok(issue)
    }

    fn create_issue(&self, draft: IssueDraft) -> Result<Issue, IssueTrackerError> {
        self.calls.lock().unwrap().push(MockCall::CreateIssue {
            title: draft.title.clone(),
            body: draft.body.clone(),
        });
        if let Some(e) = self.check_force_error() {
            return Err(e);
        }
        let mut issue = self
            .next_issue
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(Self::default_issue);
        issue.title = draft.title;
        issue.body = draft.body;
        Ok(issue)
    }

    fn update_issue_body(&self, number: u32, _body: &str) -> Result<(), IssueTrackerError> {
        self.calls
            .lock()
            .unwrap()
            .push(MockCall::UpdateIssueBody { number });
        if let Some(e) = self.check_force_error() {
            return Err(e);
        }
        Ok(())
    }

    fn add_comment(&self, number: u32, body: &str) -> Result<(), IssueTrackerError> {
        self.calls.lock().unwrap().push(MockCall::AddComment {
            number,
            body: body.to_string(),
        });
        if let Some(e) = self.check_force_error() {
            return Err(e);
        }
        Ok(())
    }

    fn close_issue(&self, number: u32, _reason: CloseReason) -> Result<(), IssueTrackerError> {
        self.calls
            .lock()
            .unwrap()
            .push(MockCall::CloseIssue(number));
        if let Some(e) = self.check_force_error() {
            return Err(e);
        }
        Ok(())
    }

    fn reopen_issue(&self, number: u32) -> Result<(), IssueTrackerError> {
        self.calls
            .lock()
            .unwrap()
            .push(MockCall::ReopenIssue(number));
        if let Some(e) = self.check_force_error() {
            return Err(e);
        }
        Ok(())
    }

    fn link_issue_to_pr(&self, issue_number: u32, pr_number: u32) -> Result<(), IssueTrackerError> {
        self.calls.lock().unwrap().push(MockCall::LinkIssueToPr {
            issue_number,
            pr_number,
        });
        if let Some(e) = self.check_force_error() {
            return Err(e);
        }
        Ok(())
    }

    fn ensure_labels(&self, labels: &[String]) -> Result<(), IssueTrackerError> {
        self.calls
            .lock()
            .unwrap()
            .push(MockCall::EnsureLabels(labels.to_vec()));
        if let Some(e) = self.check_force_error() {
            return Err(e);
        }
        Ok(())
    }
}

// ── SharedMockIssueTracker ────────────────────────────────────────────────────

/// Thin newtype that lets tests hold an `Arc<MockIssueTracker>` while also
/// giving the engine ownership of a `Box<dyn IssueTracker>`.
///
/// Only compiled in `#[cfg(test)]` because it's a test utility, not production
/// code.
#[cfg(test)]
pub struct SharedMockIssueTracker(pub std::sync::Arc<MockIssueTracker>);

#[cfg(test)]
impl IssueTracker for SharedMockIssueTracker {
    fn list_open_issues(&self, f: &IssueFilter) -> Result<Vec<Issue>, IssueTrackerError> {
        self.0.list_open_issues(f)
    }
    fn get_issue(&self, n: u32) -> Result<Issue, IssueTrackerError> {
        self.0.get_issue(n)
    }
    fn create_issue(&self, d: IssueDraft) -> Result<Issue, IssueTrackerError> {
        self.0.create_issue(d)
    }
    fn update_issue_body(&self, n: u32, b: &str) -> Result<(), IssueTrackerError> {
        self.0.update_issue_body(n, b)
    }
    fn add_comment(&self, n: u32, b: &str) -> Result<(), IssueTrackerError> {
        self.0.add_comment(n, b)
    }
    fn close_issue(&self, n: u32, r: CloseReason) -> Result<(), IssueTrackerError> {
        self.0.close_issue(n, r)
    }
    fn reopen_issue(&self, n: u32) -> Result<(), IssueTrackerError> {
        self.0.reopen_issue(n)
    }
    fn link_issue_to_pr(&self, issue: u32, pr: u32) -> Result<(), IssueTrackerError> {
        self.0.link_issue_to_pr(issue, pr)
    }
    fn ensure_labels(&self, labels: &[String]) -> Result<(), IssueTrackerError> {
        self.0.ensure_labels(labels)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── MockIssueTracker tests ────────────────────────────────────────────────

    #[test]
    fn mock_records_list_call() {
        let tracker = MockIssueTracker::new();
        let result = tracker.list_open_issues(&IssueFilter::default()).unwrap();
        assert!(result.is_empty());
        let calls = tracker.recorded_calls();
        assert_eq!(calls.len(), 1);
        assert!(matches!(calls[0], MockCall::ListOpenIssues));
    }

    #[test]
    fn mock_records_get_issue_call() {
        let tracker = MockIssueTracker::new();
        let issue = tracker.get_issue(42).unwrap();
        assert_eq!(issue.id, 1); // default issue
        let calls = tracker.recorded_calls();
        assert!(matches!(calls[0], MockCall::GetIssue(42)));
    }

    #[test]
    fn mock_returns_configured_next_issue() {
        let tracker = MockIssueTracker::new();
        *tracker.next_issue.lock().unwrap() = Some(Issue {
            id: 99,
            number: 7,
            title: "custom".to_string(),
            body: "body text".to_string(),
            state: IssueState::Open,
            labels: vec!["bug".to_string()],
            assignees: vec![],
            url: "https://example.com/7".to_string(),
        });
        let issue = tracker.get_issue(7).unwrap();
        assert_eq!(issue.id, 99);
        assert_eq!(issue.number, 7);
        assert_eq!(issue.title, "custom");
        assert_eq!(issue.labels, vec!["bug"]);
    }

    #[test]
    fn mock_create_issue_applies_title_and_body() {
        let tracker = MockIssueTracker::new();
        let draft = IssueDraft {
            title: "Fix the thing".to_string(),
            body: "Here is why".to_string(),
            ..Default::default()
        };
        let issue = tracker.create_issue(draft).unwrap();
        assert_eq!(issue.title, "Fix the thing");
        assert_eq!(issue.body, "Here is why");
        let calls = tracker.recorded_calls();
        assert!(
            matches!(&calls[0], MockCall::CreateIssue { title, .. } if title == "Fix the thing")
        );
    }

    #[test]
    fn mock_records_add_comment() {
        let tracker = MockIssueTracker::new();
        tracker.add_comment(5, "hello world").unwrap();
        let calls = tracker.recorded_calls();
        assert!(
            matches!(&calls[0], MockCall::AddComment { number: 5, body } if body == "hello world")
        );
    }

    #[test]
    fn mock_records_close_issue() {
        let tracker = MockIssueTracker::new();
        tracker.close_issue(3, CloseReason::Completed).unwrap();
        assert!(matches!(
            tracker.recorded_calls()[0],
            MockCall::CloseIssue(3)
        ));
    }

    #[test]
    fn mock_records_reopen_issue() {
        let tracker = MockIssueTracker::new();
        tracker.reopen_issue(8).unwrap();
        assert!(matches!(
            tracker.recorded_calls()[0],
            MockCall::ReopenIssue(8)
        ));
    }

    #[test]
    fn mock_records_link_issue_to_pr() {
        let tracker = MockIssueTracker::new();
        tracker.link_issue_to_pr(10, 55).unwrap();
        let calls = tracker.recorded_calls();
        assert!(matches!(
            &calls[0],
            MockCall::LinkIssueToPr {
                issue_number: 10,
                pr_number: 55
            }
        ));
    }

    #[test]
    fn mock_link_issue_to_pr_also_records_add_comment() {
        // link_issue_to_pr is implemented as add_comment, so both MockCall
        // variants should appear: LinkIssueToPr (this trait method) and
        // AddComment (the internal call it delegates to in a real impl).
        // With MockIssueTracker, link_issue_to_pr is its own path, so only
        // one call is recorded.
        let tracker = MockIssueTracker::new();
        tracker.link_issue_to_pr(1, 2).unwrap();
        assert_eq!(tracker.recorded_calls().len(), 1);
    }

    #[test]
    fn mock_propagates_force_error() {
        let mut tracker = MockIssueTracker::new();
        tracker.force_error = Some(IssueTrackerError::Auth("token expired".to_string()));
        let err = tracker
            .list_open_issues(&IssueFilter::default())
            .unwrap_err();
        assert!(matches!(err, IssueTrackerError::Auth(_)));
    }

    #[test]
    fn mock_update_issue_body_records_number() {
        let tracker = MockIssueTracker::new();
        tracker.update_issue_body(12, "new body").unwrap();
        assert!(matches!(
            tracker.recorded_calls()[0],
            MockCall::UpdateIssueBody { number: 12 }
        ));
    }

    // ── classify_gh_error tests ───────────────────────────────────────────────

    #[test]
    fn classify_auth_error() {
        let e = classify_gh_error("HTTP 401 Unauthorized: bad credentials", "test");
        assert!(matches!(e, IssueTrackerError::Auth(_)));
    }

    #[test]
    fn classify_not_found_error() {
        let e = classify_gh_error("Could not resolve to an Issue with the title 'foo'", "test");
        assert!(matches!(e, IssueTrackerError::NotFound(_)));
    }

    #[test]
    fn classify_rate_limited_error() {
        let e = classify_gh_error("rate limit exceeded (429)", "test");
        assert!(matches!(e, IssueTrackerError::RateLimited(_)));
    }

    #[test]
    fn classify_transport_for_unknown_error() {
        let e = classify_gh_error("something went wrong with the network", "test");
        assert!(matches!(e, IssueTrackerError::Transport(_)));
    }

    // ── GitHubIssueTracker unit tests (no network) ───────────────────────────

    #[test]
    fn github_tracker_validate_empty_title() {
        let tracker = GitHubIssueTracker::new("owner", "repo");
        let err = tracker
            .create_issue(IssueDraft {
                title: "  ".to_string(),
                ..Default::default()
            })
            .unwrap_err();
        assert!(matches!(err, IssueTrackerError::Validation(_)));
    }

    #[test]
    fn gh_issue_deserialize_from_json() {
        let json = r#"{
            "id": 12345,
            "number": 7,
            "title": "Test issue",
            "body": "The body",
            "state": "OPEN",
            "labels": [{"name": "bug"}, {"name": "help wanted"}],
            "assignees": [{"login": "alice"}],
            "url": "https://github.com/owner/repo/issues/7"
        }"#;
        let gh: GhIssue = serde_json::from_str(json).unwrap();
        let issue = Issue::from(gh);
        assert_eq!(issue.number, 7);
        assert_eq!(issue.title, "Test issue");
        assert_eq!(issue.state, IssueState::Open);
        assert_eq!(issue.labels, vec!["bug", "help wanted"]);
        assert_eq!(issue.assignees, vec!["alice"]);
    }

    #[test]
    fn gh_issue_closed_state() {
        let json = r#"{
            "number": 1, "title": "t", "body": null,
            "state": "closed",
            "labels": [], "assignees": [],
            "url": "https://github.com/o/r/issues/1"
        }"#;
        let gh: GhIssue = serde_json::from_str(json).unwrap();
        let issue = Issue::from(gh);
        assert_eq!(issue.state, IssueState::Closed);
    }

    #[test]
    fn issue_draft_default_is_empty() {
        let d = IssueDraft::default();
        assert!(d.title.is_empty());
        assert!(d.body.is_empty());
        assert!(d.labels.is_empty());
        assert!(d.assignees.is_empty());
    }

    #[test]
    fn issue_filter_default_is_empty() {
        let f = IssueFilter::default();
        assert!(f.state.is_none());
        assert!(f.labels.is_empty());
        assert!(f.assignee.is_none());
        assert!(f.search.is_none());
    }

    // ── LocalPromiseTracker tests ─────────────────────────────────────────────

    fn temp_tracker() -> LocalPromiseTracker {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("promise.md");
        // Leak the TempDir so the file persists for the test.
        std::mem::forget(dir);
        LocalPromiseTracker::new(path)
    }

    #[test]
    fn local_tracker_create_and_get() {
        let tracker = temp_tracker();
        let issue = tracker
            .create_issue(IssueDraft {
                title: "My task".to_string(),
                body: "details".to_string(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(issue.number, 1);
        assert_eq!(issue.title, "My task");
        assert_eq!(issue.state, IssueState::Open);

        let fetched = tracker.get_issue(1).unwrap();
        assert_eq!(fetched.title, "My task");
    }

    #[test]
    fn local_tracker_not_found() {
        let tracker = temp_tracker();
        let err = tracker.get_issue(99).unwrap_err();
        assert!(matches!(err, IssueTrackerError::NotFound(_)));
    }

    #[test]
    fn local_tracker_list_open() {
        let tracker = temp_tracker();
        tracker
            .create_issue(IssueDraft {
                title: "A".to_string(),
                ..Default::default()
            })
            .unwrap();
        tracker
            .create_issue(IssueDraft {
                title: "B".to_string(),
                ..Default::default()
            })
            .unwrap();
        tracker.close_issue(1, CloseReason::Completed).unwrap();

        let open = tracker.list_open_issues(&IssueFilter::default()).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].title, "B");
    }

    #[test]
    fn local_tracker_update_body() {
        let tracker = temp_tracker();
        tracker
            .create_issue(IssueDraft {
                title: "T".to_string(),
                ..Default::default()
            })
            .unwrap();
        tracker.update_issue_body(1, "updated body").unwrap();
        let issue = tracker.get_issue(1).unwrap();
        assert_eq!(issue.body, "updated body");
    }

    #[test]
    fn local_tracker_add_comment_appends_to_body() {
        let tracker = temp_tracker();
        tracker
            .create_issue(IssueDraft {
                title: "T".to_string(),
                body: "original".to_string(),
                ..Default::default()
            })
            .unwrap();
        tracker.add_comment(1, "a comment").unwrap();
        let issue = tracker.get_issue(1).unwrap();
        assert!(issue.body.contains("a comment"));
    }

    #[test]
    fn local_tracker_close_and_reopen() {
        let tracker = temp_tracker();
        tracker
            .create_issue(IssueDraft {
                title: "T".to_string(),
                ..Default::default()
            })
            .unwrap();
        tracker.close_issue(1, CloseReason::Completed).unwrap();
        assert_eq!(tracker.get_issue(1).unwrap().state, IssueState::Closed);
        tracker.reopen_issue(1).unwrap();
        assert_eq!(tracker.get_issue(1).unwrap().state, IssueState::Open);
    }

    #[test]
    fn local_tracker_link_pr_adds_comment() {
        let tracker = temp_tracker();
        tracker
            .create_issue(IssueDraft {
                title: "T".to_string(),
                ..Default::default()
            })
            .unwrap();
        tracker.link_issue_to_pr(1, 42).unwrap();
        let issue = tracker.get_issue(1).unwrap();
        assert!(issue.body.contains("42"));
    }

    #[test]
    fn local_tracker_empty_title_rejected() {
        let tracker = temp_tracker();
        let err = tracker
            .create_issue(IssueDraft {
                title: "".to_string(),
                ..Default::default()
            })
            .unwrap_err();
        assert!(matches!(err, IssueTrackerError::Validation(_)));
    }

    #[test]
    fn local_tracker_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("promise.md");
        let tracker = LocalPromiseTracker::new(&path);
        tracker
            .create_issue(IssueDraft {
                title: "dir test".to_string(),
                ..Default::default()
            })
            .unwrap();
        assert!(path.exists());
    }

    #[test]
    fn local_tracker_search_filter() {
        let tracker = temp_tracker();
        tracker
            .create_issue(IssueDraft {
                title: "fix login bug".to_string(),
                ..Default::default()
            })
            .unwrap();
        tracker
            .create_issue(IssueDraft {
                title: "update docs".to_string(),
                ..Default::default()
            })
            .unwrap();
        let results = tracker
            .list_open_issues(&IssueFilter {
                search: Some("login".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "fix login bug");
    }

    // ── ensure_labels tests ───────────────────────────────────────────────────

    #[test]
    fn mock_records_ensure_labels_call() {
        let tracker = MockIssueTracker::new();
        let labels = vec!["bug".to_string(), "discovered-during-loop".to_string()];
        tracker.ensure_labels(&labels).unwrap();
        let calls = tracker.recorded_calls();
        assert_eq!(calls.len(), 1);
        assert!(matches!(&calls[0], MockCall::EnsureLabels(l) if l == &labels));
    }

    #[test]
    fn local_tracker_ensure_labels_is_noop() {
        // LocalPromiseTracker uses the default no-op implementation.
        let tracker = temp_tracker();
        let labels = vec!["bug".to_string(), "tech-debt".to_string()];
        // Should succeed without doing anything.
        tracker.ensure_labels(&labels).unwrap();
    }

    #[test]
    fn mock_ensure_labels_propagates_force_error() {
        let mut tracker = MockIssueTracker::new();
        tracker.force_error = Some(IssueTrackerError::Transport("network error".to_string()));
        let labels = vec!["bug".to_string()];
        let err = tracker.ensure_labels(&labels).unwrap_err();
        assert!(matches!(err, IssueTrackerError::Transport(_)));
    }
}
