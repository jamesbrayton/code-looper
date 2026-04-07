use crate::config::{LoopConfig, Provider as ProviderKind};
use crate::provider::build_adapter;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Instant;
use tracing::{info, warn, error};

// ── Request / response types ──────────────────────────────────────────────────

/// A JSON-lines request from a service client.
///
/// Wire format: `{"cmd": "<name>", ...fields}` as a single newline-terminated
/// JSON object.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "lowercase")]
pub enum ServiceRequest {
    /// Execute one provider iteration with the given prompt.
    ///
    /// `provider` overrides the service-level default for this single request.
    Run {
        prompt: String,
        #[serde(default)]
        provider: Option<ProviderKind>,
    },
    /// Return service uptime and run statistics.
    Status,
    /// Gracefully stop the service after the current connection closes.
    Shutdown,
}

/// A JSON-lines response returned to the client for each request.
#[derive(Debug, Serialize)]
pub struct ServiceResponse {
    /// `true` when the request completed without error.
    pub ok: bool,
    /// Request-specific payload (present on success).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Human-readable error description (present on failure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ServiceResponse {
    /// Build a successful response with an attached payload.
    pub fn success(data: serde_json::Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    /// Build an error response with a description.
    pub fn failure(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

// ── Service state ─────────────────────────────────────────────────────────────

/// Accumulated statistics for the lifetime of one service session.
struct ServiceState {
    started_at: Instant,
    run_count: u64,
    success_count: u64,
    failure_count: u64,
}

impl ServiceState {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            run_count: 0,
            success_count: 0,
            failure_count: 0,
        }
    }

    fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

// ── ServiceMode ───────────────────────────────────────────────────────────────

/// Embedded service mode: accepts JSON-lines requests over a local TCP socket.
///
/// Each connection is processed sequentially.  The service shuts down cleanly
/// when a `shutdown` command is received or when the process receives SIGINT.
///
/// # Protocol
///
/// Each request is a single newline-terminated JSON object.  The server
/// responds with a single newline-terminated JSON object per request.
///
/// ```text
/// Client → {"cmd":"run","prompt":"fix the tests"}
/// Server ← {"ok":true,"data":{"exit_code":0,"stdout":"...","duration_ms":1200}}
///
/// Client → {"cmd":"status"}
/// Server ← {"ok":true,"data":{"uptime_secs":42,"run_count":1,...}}
///
/// Client → {"cmd":"shutdown"}
/// Server ← {"ok":true,"data":{"message":"shutting down"}}
/// ```
pub struct ServiceMode {
    config: LoopConfig,
    bind_addr: String,
    port: u16,
}

impl ServiceMode {
    /// Create a new service mode from an existing resolved config and binding
    /// parameters.  `config.provider` is used as the default provider for `run`
    /// requests that omit the `provider` field.
    pub fn new(config: LoopConfig, bind_addr: String, port: u16) -> Self {
        Self {
            config,
            bind_addr,
            port,
        }
    }

    /// Start the TCP listener and process connections until a `shutdown` command
    /// is received or the process is terminated.
    pub fn run(&self) -> anyhow::Result<()> {
        let addr = format!("{}:{}", self.bind_addr, self.port);
        let listener = TcpListener::bind(&addr)?;
        info!(addr = %addr, "Code Looper service listening");
        println!("Code Looper service listening on {addr}");
        println!("Send JSON-lines requests (one per line).  Ctrl-C to stop.");

        let mut state = ServiceState::new();

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let peer = stream
                        .peer_addr()
                        .map(|a| a.to_string())
                        .unwrap_or_default();
                    info!(peer = %peer, "Client connected");
                    match self.handle_connection(stream, &mut state) {
                        Ok(true) => {
                            info!("Shutdown requested by client — stopping service");
                            break;
                        }
                        Ok(false) => {}
                        Err(e) => warn!(error = %e, "Connection error"),
                    }
                }
                Err(e) => warn!(error = %e, "Accept error"),
            }
        }

        info!(
            runs = state.run_count,
            successes = state.success_count,
            failures = state.failure_count,
            uptime_secs = state.uptime_secs(),
            "Code Looper service stopped"
        );
        Ok(())
    }

    /// Process all requests from one TCP connection.
    ///
    /// Returns `Ok(true)` if the client sent a `shutdown` command.
    fn handle_connection(
        &self,
        stream: TcpStream,
        state: &mut ServiceState,
    ) -> anyhow::Result<bool> {
        let mut write_stream = stream.try_clone()?;
        let reader = BufReader::new(stream);
        let mut shutdown_requested = false;

        for raw_line in reader.lines() {
            let line = raw_line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let (response, shutdown) = match serde_json::from_str::<ServiceRequest>(line) {
                Ok(req) => self.process_request(req, state),
                Err(e) => (ServiceResponse::failure(format!("parse error: {e}")), false),
            };

            let response_json = serde_json::to_string(&response)?;
            writeln!(write_stream, "{response_json}")?;

            if shutdown {
                shutdown_requested = true;
                break;
            }
        }

        Ok(shutdown_requested)
    }

    /// Dispatch a parsed request to its handler and return (response, shutdown).
    fn process_request(
        &self,
        req: ServiceRequest,
        state: &mut ServiceState,
    ) -> (ServiceResponse, bool) {
        match req {
            ServiceRequest::Run { prompt, provider } => {
                let provider_kind = provider.as_ref().unwrap_or(&self.config.provider);
                let adapter = build_adapter(
                    provider_kind,
                    false,
                    self.config.workspace_dir.clone(),
                    self.config.iteration_timeout_secs,
                    self.config.provider_extra_args.clone(),
                );
                state.run_count += 1;

                match adapter.execute(&prompt) {
                    Ok(result) => {
                        let ok = result.succeeded();
                        let duration_ms = result.duration.as_millis();
                        if ok {
                            state.success_count += 1;
                            info!(
                                provider = adapter.name(),
                                duration_ms,
                                exit_code = result.exit_code,
                                ok = true,
                                "Service run completed"
                            );
                        } else {
                            state.failure_count += 1;
                            warn!(
                                provider = adapter.name(),
                                duration_ms,
                                exit_code = result.exit_code,
                                ok = false,
                                "Service run failed"
                            );
                        }
                        let data = serde_json::json!({
                            "ok": ok,
                            "exit_code": result.exit_code,
                            "stdout": result.stdout,
                            "stderr": result.stderr,
                            "duration_ms": duration_ms,
                        });
                        (ServiceResponse::success(data), false)
                    }
                    Err(e) => {
                        state.failure_count += 1;
                        error!(
                            provider = adapter.name(),
                            error = %e,
                            ok = false,
                            "Service run error"
                        );
                        (ServiceResponse::failure(e.to_string()), false)
                    }
                }
            }

            ServiceRequest::Status => {
                let data = serde_json::json!({
                    "uptime_secs": state.uptime_secs(),
                    "run_count": state.run_count,
                    "success_count": state.success_count,
                    "failure_count": state.failure_count,
                    "provider": self.config.provider.to_string(),
                });
                (ServiceResponse::success(data), false)
            }

            ServiceRequest::Shutdown => {
                let data = serde_json::json!({"message": "shutting down"});
                (ServiceResponse::success(data), true)
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ServiceRequest parsing ────────────────────────────────────────────────

    #[test]
    fn parse_run_request_with_default_provider() {
        let json = r#"{"cmd":"run","prompt":"fix tests"}"#;
        let req: ServiceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            req,
            ServiceRequest::Run {
                prompt: "fix tests".to_string(),
                provider: None
            }
        );
    }

    #[test]
    fn parse_run_request_with_explicit_provider() {
        let json = r#"{"cmd":"run","prompt":"hello","provider":"codex"}"#;
        let req: ServiceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            req,
            ServiceRequest::Run {
                prompt: "hello".to_string(),
                provider: Some(ProviderKind::Codex),
            }
        );
    }

    #[test]
    fn parse_status_request() {
        let json = r#"{"cmd":"status"}"#;
        let req: ServiceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req, ServiceRequest::Status);
    }

    #[test]
    fn parse_shutdown_request() {
        let json = r#"{"cmd":"shutdown"}"#;
        let req: ServiceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req, ServiceRequest::Shutdown);
    }

    #[test]
    fn parse_unknown_command_returns_error() {
        let json = r#"{"cmd":"frobnicate"}"#;
        let result: Result<ServiceRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // ── ServiceResponse serialization ─────────────────────────────────────────

    #[test]
    fn success_response_omits_error_field() {
        let resp = ServiceResponse::success(serde_json::json!({"key": "value"}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"ok\":true"));
        assert!(json.contains("\"key\":\"value\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn failure_response_omits_data_field() {
        let resp = ServiceResponse::failure("something went wrong");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"ok\":false"));
        assert!(json.contains("\"error\":\"something went wrong\""));
        assert!(!json.contains("\"data\""));
    }

    // ── ServiceState ──────────────────────────────────────────────────────────

    #[test]
    fn service_state_initial_counts_are_zero() {
        let state = ServiceState::new();
        assert_eq!(state.run_count, 0);
        assert_eq!(state.success_count, 0);
        assert_eq!(state.failure_count, 0);
    }

    #[test]
    fn service_state_uptime_is_non_negative() {
        let state = ServiceState::new();
        // Uptime immediately after creation should be 0 or very small.
        assert!(state.uptime_secs() < 5);
    }

    // ── process_request (status / shutdown via ServiceMode) ───────────────────

    fn make_service() -> ServiceMode {
        let config = crate::config::LoopConfig {
            provider: ProviderKind::Claude,
            ..Default::default()
        };
        ServiceMode::new(config, "127.0.0.1".to_string(), 7979)
    }

    #[test]
    fn process_status_returns_correct_fields() {
        let service = make_service();
        let mut state = ServiceState::new();
        state.run_count = 3;
        state.success_count = 2;
        state.failure_count = 1;

        let (resp, shutdown) = service.process_request(ServiceRequest::Status, &mut state);
        assert!(!shutdown);
        assert!(resp.ok);
        let data = resp.data.unwrap();
        assert_eq!(data["run_count"], 3);
        assert_eq!(data["success_count"], 2);
        assert_eq!(data["failure_count"], 1);
        assert_eq!(data["provider"], "claude");
    }

    #[test]
    fn process_shutdown_sets_shutdown_flag() {
        let service = make_service();
        let mut state = ServiceState::new();
        let (resp, shutdown) = service.process_request(ServiceRequest::Shutdown, &mut state);
        assert!(shutdown);
        assert!(resp.ok);
        let data = resp.data.unwrap();
        assert_eq!(data["message"], "shutting down");
    }

    #[test]
    fn process_status_increments_uptime_field_exists() {
        let service = make_service();
        let mut state = ServiceState::new();
        let (resp, _) = service.process_request(ServiceRequest::Status, &mut state);
        assert!(resp
            .data
            .as_ref()
            .and_then(|d| d.get("uptime_secs"))
            .is_some());
    }
}
