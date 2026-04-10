use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result, WrapErr};
use reqwest::blocking::Client;
use serde::Deserialize;
use urlencoding::encode;

const SABISABI_BASE_URL_ENV: &str = "SABISABI_BASE_URL";
const DEFAULT_SABISABI_BASE_URL: &str = "http://127.0.0.1:4080";

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ExecutionPlanEnvelope {
    pub matched_at: String,
    pub opportunity: ExecutionOpportunity,
    pub gateway: ExecutionGatewayInfo,
    pub plan: ExecutionPlan,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ExecutionOpportunity {
    pub id: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub stake_hint: Option<f64>,
    pub canonical: ExecutionCanonicalRefs,
    #[serde(default)]
    pub venue_mappings: Vec<VenueSelectionMapping>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ExecutionCanonicalRefs {
    pub event: ExecutionRef,
    pub market: ExecutionRef,
    pub selection: ExecutionRef,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ExecutionRef {
    pub id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct VenueSelectionMapping {
    pub venue: String,
    pub event_ref: String,
    pub market_ref: String,
    pub selection_ref: String,
    pub event_url: String,
    pub deep_link_url: String,
    pub side: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ExecutionGatewayInfo {
    pub kind: String,
    pub mode: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ExecutionPlan {
    pub primary: ExecutionAction,
    pub secondary: Option<ExecutionAction>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ExecutionAction {
    pub venue: String,
    pub selection_name: String,
    pub side: String,
    pub price: Option<f64>,
    pub stake_hint: Option<f64>,
    pub deep_link_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ExecutionReviewResponse {
    pub gateway: ExecutionGatewayInfo,
    pub review: GatewayReview,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ExecutionSubmitResponse {
    pub gateway: ExecutionGatewayInfo,
    pub result: GatewaySubmitResult,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct AdhocExecutionRequest {
    pub venue: String,
    pub side: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub stake: f64,
    pub price: f64,
    pub event_url: Option<String>,
    pub deep_link_url: Option<String>,
    pub event_ref: Option<String>,
    pub market_ref: Option<String>,
    pub selection_ref: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct GatewayReview {
    pub status: String,
    pub detail: String,
    pub executable: bool,
    pub stake: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct GatewaySubmitResult {
    pub status: String,
    pub detail: String,
    pub accepted: bool,
    pub stake: f64,
    #[serde(default)]
    pub venue_order_refs: Vec<String>,
}

pub fn fetch_execution_plan(match_id: &str) -> Result<ExecutionPlanEnvelope> {
    fetch_execution_plan_from_base_url(&sabisabi_base_url(), match_id)
}

fn fetch_execution_plan_from_base_url(
    base_url: &str,
    match_id: &str,
) -> Result<ExecutionPlanEnvelope> {
    let client = build_backend_client()?;
    let path = format!("/api/v1/query/execution/plan/{}", encode(match_id));
    let response = client
        .get(format!("{}{}", base_url.trim_end_matches('/'), path))
        .send()
        .wrap_err_with(|| format!("request failed for {path}"))?;
    decode_json_response(response, "execution plan")
}

pub fn review_execution(match_id: &str, stake: f64) -> Result<ExecutionReviewResponse> {
    let client = build_backend_client()?;
    let path = "/api/v1/execution/review";
    let mut request = client
        .post(format!(
            "{}{}",
            sabisabi_base_url().trim_end_matches('/'),
            path
        ))
        .json(&serde_json::json!({"match_id": match_id, "stake": stake}));
    if let Some(token) = sabisabi_control_token() {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .wrap_err_with(|| format!("request failed for {path}"))?;
    decode_json_response(response, "execution review")
}

pub fn submit_execution(match_id: &str, stake: f64) -> Result<ExecutionSubmitResponse> {
    let client = build_backend_client()?;
    let path = "/api/v1/execution/submit";
    let mut request = client
        .post(format!(
            "{}{}",
            sabisabi_base_url().trim_end_matches('/'),
            path
        ))
        .json(&serde_json::json!({"match_id": match_id, "stake": stake}));
    if let Some(token) = sabisabi_control_token() {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .wrap_err_with(|| format!("request failed for {path}"))?;
    decode_json_response(response, "execution submit")
}

pub fn review_adhoc_execution(request: &AdhocExecutionRequest) -> Result<ExecutionReviewResponse> {
    let client = build_backend_client()?;
    let path = "/api/v1/execution/ad-hoc/review";
    let mut http_request = client
        .post(format!(
            "{}{}",
            sabisabi_base_url().trim_end_matches('/'),
            path
        ))
        .json(request);
    if let Some(token) = sabisabi_control_token() {
        http_request = http_request.bearer_auth(token);
    }
    let response = http_request
        .send()
        .wrap_err_with(|| format!("request failed for {path}"))?;
    decode_json_response(response, "ad hoc execution review")
}

pub fn submit_adhoc_execution(request: &AdhocExecutionRequest) -> Result<ExecutionSubmitResponse> {
    let client = build_backend_client()?;
    let path = "/api/v1/execution/ad-hoc/submit";
    let mut http_request = client
        .post(format!(
            "{}{}",
            sabisabi_base_url().trim_end_matches('/'),
            path
        ))
        .json(request);
    if let Some(token) = sabisabi_control_token() {
        http_request = http_request.bearer_auth(token);
    }
    let response = http_request
        .send()
        .wrap_err_with(|| format!("request failed for {path}"))?;
    decode_json_response(response, "ad hoc execution submit")
}

#[derive(Debug)]
pub struct ExecutionHttpError {
    pub status: u16,
    pub label: String,
    pub body: String,
}

impl std::fmt::Display for ExecutionHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {} during {}: {}", self.status, self.label, self.body)
    }
}

impl std::error::Error for ExecutionHttpError {}

pub fn is_not_found_error(error: &color_eyre::eyre::Report) -> bool {
    if let Some(http_error) = error.downcast_ref::<ExecutionHttpError>() {
        http_error.status == 404
    } else {
        false
    }
}

fn build_backend_client() -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_millis(750))
        .timeout(Duration::from_secs(8))
        .build()
        .wrap_err("failed to build backend client")
}

fn sabisabi_base_url() -> String {
    env::var(SABISABI_BASE_URL_ENV).unwrap_or_else(|_| String::from(DEFAULT_SABISABI_BASE_URL))
}

fn sabisabi_control_token() -> Option<String> {
    env::var("SABISABI_CONTROL_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| dotenv_value_from_paths("SABISABI_CONTROL_TOKEN", &dotenv_candidates()))
}

fn decode_json_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::blocking::Response,
    label: &str,
) -> Result<T> {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(ExecutionHttpError {
            status: status.as_u16(),
            label: label.to_string(),
            body: truncate(&body, 200),
        }
        .into());
    }
    serde_json::from_str(&body).wrap_err_with(|| format!("failed to decode {label} response"))
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn dotenv_value_from_paths(name: &str, paths: &[PathBuf]) -> Option<String> {
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if key.trim() != name {
                continue;
            }
            let parsed = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !parsed.is_empty() {
                return Some(parsed);
            }
        }
    }
    None
}

fn dotenv_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(current_dir) = env::current_dir() {
        for ancestor in current_dir.ancestors() {
            paths.push(ancestor.join(".env.local"));
            paths.push(ancestor.join(".env"));
        }
    }
    if let Ok(executable) = env::current_exe() {
        if let Some(executable_dir) = executable.parent() {
            for ancestor in executable_dir.ancestors() {
                paths.push(ancestor.join(".env.local"));
                paths.push(ancestor.join(".env"));
            }
        }
    }
    if let Some(home) = env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        paths.push(home_path.join(".env.local"));
        paths.push(home_path.join(".env"));
    }
    paths.sort();
    paths.dedup();
    paths
}

#[cfg(test)]
mod tests {
    use super::fetch_execution_plan_from_base_url;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn fetch_execution_plan_decodes_backend_shape() {
        let body = serde_json::json!({
            "matched_at": "2026-04-05T12:00:00Z",
            "gateway": {"kind": "matchbook", "mode": "stub", "detail": "stub"},
            "plan": {
                "primary": {
                    "venue": "matchbook",
                    "selection_name": "Arsenal",
                    "side": "back",
                    "price": 2.1,
                    "stake_hint": 10.0,
                    "deep_link_url": "https://matchbook.example/market"
                },
                "secondary": null,
                "notes": []
            },
            "opportunity": {
                "id": "arb-1",
                "event_name": "Arsenal vs Everton",
                "market_name": "Full-time result",
                "selection_name": "Arsenal",
                "stake_hint": 10.0,
                "canonical": {
                    "event": {"id": "event-1"},
                    "market": {"id": "market-1"},
                    "selection": {"id": "selection-1"}
                },
                "venue_mappings": [{
                    "venue": "matchbook",
                    "event_ref": "event-1",
                    "market_ref": "market-1",
                    "selection_ref": "selection-1",
                    "event_url": "https://matchbook.example/event",
                    "deep_link_url": "https://matchbook.example/market",
                    "side": "back"
                }]
            }
        })
        .to_string();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind server");
        let address = listener.local_addr().expect("server addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0_u8; 2048];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(), body
            );
            stream.write_all(response.as_bytes()).expect("write");
        });

        let plan = fetch_execution_plan_from_base_url(&format!("http://{address}"), "arb-1")
            .expect("plan");

        assert_eq!(plan.gateway.kind, "matchbook");
        assert_eq!(plan.plan.primary.venue, "matchbook");
        assert_eq!(
            plan.opportunity.venue_mappings[0].selection_ref,
            "selection-1"
        );

        server.join().expect("server join");
    }

    #[test]
    fn fetch_execution_plan_percent_encodes_match_id_path_segment() {
        let body = serde_json::json!({
            "matched_at": "2026-04-05T12:00:00Z",
            "gateway": {"kind": "matchbook", "mode": "stub", "detail": "stub"},
            "plan": {"primary": {"venue": "matchbook", "selection_name": "Arsenal", "side": "back", "price": 2.1, "stake_hint": 10.0, "deep_link_url": "https://matchbook.example/market"}, "secondary": null, "notes": []},
            "opportunity": {"id": "arb-1", "event_name": "Arsenal vs Everton", "market_name": "Full-time result", "selection_name": "Arsenal", "stake_hint": 10.0, "canonical": {"event": {"id": "event-1"}, "market": {"id": "market-1"}, "selection": {"id": "selection-1"}}, "venue_mappings": []}
        })
        .to_string();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind server");
        let address = listener.local_addr().expect("server addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0_u8; 2048];
            let read = stream.read(&mut buffer).expect("read request");
            let request = String::from_utf8_lossy(&buffer[..read]);
            assert!(
                request.starts_with(
                    "GET /api/v1/query/execution/plan/match%2Fid%20with%20spaces HTTP/1.1"
                ),
                "unexpected request line: {request}"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(), body
            );
            stream.write_all(response.as_bytes()).expect("write");
        });

        let plan = fetch_execution_plan_from_base_url(
            &format!("http://{address}"),
            "match/id with spaces",
        )
        .expect("plan");

        assert_eq!(plan.opportunity.id, "arb-1");

        server.join().expect("server join");
    }
}
