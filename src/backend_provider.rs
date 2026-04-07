use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result, WrapErr};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::Deserialize;
use serde::Serialize;

use operator_console::domain::{ExchangePanelSnapshot, VenueId, WorkerStatus, WorkerSummary};
use operator_console::exchange_api::MatchbookAccountState;
use operator_console::provider::{ExchangeProvider, ProviderRequest, ProviderSnapshot};

const SABISABI_BASE_URL_ENV: &str = "SABISABI_BASE_URL";
const DEFAULT_SABISABI_BASE_URL: &str = "http://127.0.0.1:4080";

#[derive(Debug, Clone)]
pub struct BackendExchangeProvider {
    client: Client,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum OperatorSnapshotAction {
    LoadDashboard,
    SelectVenue,
    RefreshCached,
    RefreshLive,
    CashOutTrackedBet,
    ExecuteTradingAction,
    LoadHorseMatcher,
}

#[derive(Debug, Clone, Serialize, Default)]
struct OperatorSnapshotControlRequest {
    action: OperatorSnapshotAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    venue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bet_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    intent: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<serde_json::Value>,
}

impl Default for OperatorSnapshotAction {
    fn default() -> Self {
        Self::LoadDashboard
    }
}

impl BackendExchangeProvider {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_millis(750))
            .timeout(Duration::from_secs(8))
            .build()
            .wrap_err("failed to build backend provider HTTP client")?;
        Ok(Self { client })
    }

    fn load_via_backend(&self, request: ProviderRequest) -> Result<ProviderSnapshot> {
        let base_url = sabisabi_base_url();
        let (path, response) = if matches!(request, ProviderRequest::LoadDashboard) {
            let path = "/api/v1/query/operator/snapshot";
            let response = self
                .client
                .get(format!("{}{}", base_url.trim_end_matches('/'), path))
                .send()
                .wrap_err_with(|| format!("request failed for {path}"))?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                let fallback_path = "/api/v1/control/operator/snapshot";
                let fallback_response = self
                    .client
                    .post(format!(
                        "{}{}",
                        base_url.trim_end_matches('/'),
                        fallback_path
                    ))
                    .json(&OperatorSnapshotControlRequest::default())
                    .send()
                    .wrap_err_with(|| format!("request failed for {fallback_path}"))?;
                (fallback_path, fallback_response)
            } else {
                (path, response)
            }
        } else {
            let control = map_request(request)?;
            let path = "/api/v1/control/operator/snapshot";
            let mut request = self
                .client
                .post(format!("{}{}", base_url.trim_end_matches('/'), path))
                .json(&control);
            if let Some(token) = sabisabi_control_token() {
                request = request.bearer_auth(token);
            }
            let response = request
                .send()
                .wrap_err_with(|| format!("request failed for {path}"))?;
            (path, response)
        };
        decode_snapshot_response(path, response)
    }
}

impl ExchangeProvider for BackendExchangeProvider {
    fn handle(&mut self, request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        self.handle_with_metadata(request)
            .map(|snapshot| snapshot.snapshot)
    }

    fn handle_with_metadata(&mut self, request: ProviderRequest) -> Result<ProviderSnapshot> {
        match self.load_via_backend(request.clone()) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) if should_fallback_to_unavailable_snapshot(&request) => Ok(
                ProviderSnapshot::from_snapshot(unavailable_snapshot(&error.to_string())),
            ),
            Err(error) => Err(error),
        }
    }
}

fn should_fallback_to_unavailable_snapshot(request: &ProviderRequest) -> bool {
    matches!(request, ProviderRequest::LoadDashboard)
}

#[derive(Debug, Deserialize)]
struct BackendOperatorSnapshotResponse {
    snapshot: ExchangePanelSnapshot,
    #[serde(default)]
    matchbook_account_state: Option<MatchbookAccountState>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BackendOperatorSnapshotPayload {
    Envelope(BackendOperatorSnapshotResponse),
    Snapshot(ExchangePanelSnapshot),
}

fn decode_snapshot_response(
    path: &str,
    response: reqwest::blocking::Response,
) -> Result<ProviderSnapshot> {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(snapshot_request_http_error(path, status, &body));
    }

    let payload = serde_json::from_str::<BackendOperatorSnapshotPayload>(&body)
        .wrap_err_with(|| format!("failed to decode backend operator snapshot from {path}"))?;
    Ok(match payload {
        BackendOperatorSnapshotPayload::Envelope(payload) => ProviderSnapshot {
            snapshot: payload.snapshot,
            matchbook_account_state: payload.matchbook_account_state,
        },
        BackendOperatorSnapshotPayload::Snapshot(snapshot) => ProviderSnapshot {
            snapshot,
            matchbook_account_state: None,
        },
    })
}

fn snapshot_request_http_error(path: &str, status: StatusCode, body: &str) -> color_eyre::Report {
    if status == StatusCode::UNAUTHORIZED && path == "/api/v1/control/operator/snapshot" {
        return eyre!(
            "HTTP 401 during operator snapshot control request: missing or invalid SABISABI_CONTROL_TOKEN"
        );
    }

    eyre!(
        "HTTP {} during operator snapshot request: {}",
        status.as_u16(),
        truncate(body, 200)
    )
}

fn map_request(request: ProviderRequest) -> Result<OperatorSnapshotControlRequest> {
    Ok(match request {
        ProviderRequest::LoadDashboard => OperatorSnapshotControlRequest {
            action: OperatorSnapshotAction::LoadDashboard,
            ..OperatorSnapshotControlRequest::default()
        },
        ProviderRequest::SelectVenue(venue) => OperatorSnapshotControlRequest {
            action: OperatorSnapshotAction::SelectVenue,
            venue: Some(venue.as_str().to_string()),
            ..OperatorSnapshotControlRequest::default()
        },
        ProviderRequest::RefreshCached => OperatorSnapshotControlRequest {
            action: OperatorSnapshotAction::RefreshCached,
            ..OperatorSnapshotControlRequest::default()
        },
        ProviderRequest::RefreshLive => OperatorSnapshotControlRequest {
            action: OperatorSnapshotAction::RefreshLive,
            ..OperatorSnapshotControlRequest::default()
        },
        ProviderRequest::CashOutTrackedBet { bet_id } => OperatorSnapshotControlRequest {
            action: OperatorSnapshotAction::CashOutTrackedBet,
            bet_id: Some(bet_id),
            ..OperatorSnapshotControlRequest::default()
        },
        ProviderRequest::ExecuteTradingAction { intent } => OperatorSnapshotControlRequest {
            action: OperatorSnapshotAction::ExecuteTradingAction,
            intent: Some(serde_json::to_value(*intent)?),
            ..OperatorSnapshotControlRequest::default()
        },
        ProviderRequest::LoadHorseMatcher { query } => OperatorSnapshotControlRequest {
            action: OperatorSnapshotAction::LoadHorseMatcher,
            query: Some(serde_json::to_value(*query)?),
            ..OperatorSnapshotControlRequest::default()
        },
    })
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

fn unavailable_snapshot(detail: &str) -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("sabisabi"),
            status: WorkerStatus::Error,
            detail: detail.to_string(),
        },
        selected_venue: Some(VenueId::Smarkets),
        status_line: format!("Backend unavailable: {detail}"),
        ..ExchangePanelSnapshot::default()
    }
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
    // Search from current working directory first.
    if let Ok(current_dir) = env::current_dir() {
        for ancestor in current_dir.ancestors() {
            paths.push(ancestor.join(".env.local"));
            paths.push(ancestor.join(".env"));
        }
    }
    // Also search upward from the executable location since the console may be launched
    // from outside the repo while still living inside the workspace target tree.
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
    use super::{
        map_request, should_fallback_to_unavailable_snapshot, snapshot_request_http_error,
        BackendExchangeProvider,
    };
    use reqwest::StatusCode;
    use operator_console::domain::VenueId;
    use operator_console::horse_matcher::HorseMatcherQuery;
    use operator_console::provider::ProviderRequest;
    use operator_console::trading_actions::{
        TradingActionIntent, TradingActionKind, TradingActionMode, TradingActionSide,
        TradingActionSource, TradingExecutionPolicy, TradingRiskReport,
    };

    #[test]
    fn backend_provider_client_builds() {
        BackendExchangeProvider::new().expect("backend provider");
    }

    #[test]
    fn select_venue_maps_to_backend_control_payload() {
        let payload = map_request(ProviderRequest::SelectVenue(VenueId::Betway)).expect("payload");
        assert_eq!(payload.venue.as_deref(), Some("betway"));
    }

    #[test]
    fn execute_action_maps_to_json_payload() {
        let payload = map_request(ProviderRequest::ExecuteTradingAction {
            intent: Box::new(TradingActionIntent {
                action_kind: TradingActionKind::PlaceBet,
                source: TradingActionSource::Positions,
                venue: VenueId::Matchbook,
                mode: TradingActionMode::Review,
                side: TradingActionSide::Buy,
                request_id: String::from("req-1"),
                source_ref: String::from("row-1"),
                event_name: String::from("Arsenal vs Everton"),
                market_name: String::from("Match Odds"),
                selection_name: String::from("Arsenal"),
                stake: 10.0,
                expected_price: 2.1,
                event_url: None,
                deep_link_url: Some(String::from("https://matchbook.example/market")),
                betslip_event_id: None,
                betslip_market_id: None,
                betslip_selection_id: None,
                execution_policy: TradingExecutionPolicy::default(),
                risk_report: TradingRiskReport::default(),
                source_context: Default::default(),
                notes: Vec::new(),
            }),
        })
        .expect("payload");
        assert!(payload.intent.is_some());
    }

    #[test]
    fn horse_matcher_maps_to_json_payload() {
        let payload = map_request(ProviderRequest::LoadHorseMatcher {
            query: Box::new(HorseMatcherQuery::default()),
        })
        .expect("payload");
        assert!(payload.query.is_some());
    }

    #[test]
    fn only_dashboard_bootstrap_falls_back_to_unavailable_snapshot() {
        assert!(should_fallback_to_unavailable_snapshot(
            &ProviderRequest::LoadDashboard
        ));
        assert!(!should_fallback_to_unavailable_snapshot(
            &ProviderRequest::RefreshCached
        ));
        assert!(!should_fallback_to_unavailable_snapshot(
            &ProviderRequest::RefreshLive
        ));
        assert!(!should_fallback_to_unavailable_snapshot(
            &ProviderRequest::SelectVenue(VenueId::Betway)
        ));
    }

    #[test]
    fn unauthorized_control_error_mentions_control_token() {
        let error = snapshot_request_http_error(
            "/api/v1/control/operator/snapshot",
            StatusCode::UNAUTHORIZED,
            "",
        );

        assert!(
            error
                .to_string()
                .contains("missing or invalid SABISABI_CONTROL_TOKEN")
        );
    }
}
