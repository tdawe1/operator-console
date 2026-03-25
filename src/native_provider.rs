use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result, WrapErr};
use serde_json::{json, Value};

use crate::domain::{
    AccountStats, DecisionSummary, ExchangePanelSnapshot, OpenPositionRow, OtherOpenBetRow,
    RecorderBundleSummary, RecorderEventSummary, RuntimeSummary, TransportCaptureSummary,
    TransportMarkerSummary, VenueId, VenueStatus, VenueSummary, WatchRow, WatchSnapshot,
    WorkerStatus,
};
use crate::exchange_api::execute_trade as execute_exchange_api_trade;
use crate::native_trading::{execute_smarkets_trade, NativeTradingResult};
use crate::provider::{ExchangeProvider, ProviderRequest};
use crate::trading_actions::TradingActionIntent;
use crate::transport::WorkerConfig;

const DEFAULT_RECENT_EVENT_LIMIT: usize = 12;
type BrowserRunner = dyn Fn(&[String]) -> Result<Value> + Send + Sync;
type ApiRunner = dyn Fn(&TradingActionIntent) -> Result<NativeTradingResult> + Send + Sync;

pub struct NativeExchangeProvider {
    config: WorkerConfig,
    selected_venue: VenueId,
    cached_snapshot: Option<ExchangePanelSnapshot>,
    browser_runner: Option<Arc<BrowserRunner>>,
    api_runner: Option<Arc<ApiRunner>>,
}

impl NativeExchangeProvider {
    pub fn new(config: WorkerConfig) -> Self {
        Self {
            config,
            selected_venue: VenueId::Smarkets,
            cached_snapshot: None,
            browser_runner: None,
            api_runner: None,
        }
    }

    pub fn with_browser_runner(config: WorkerConfig, runner: Arc<BrowserRunner>) -> Self {
        Self {
            config,
            selected_venue: VenueId::Smarkets,
            cached_snapshot: None,
            browser_runner: Some(runner),
            api_runner: None,
        }
    }

    pub fn with_api_runner(config: WorkerConfig, runner: Arc<ApiRunner>) -> Self {
        Self {
            config,
            selected_venue: VenueId::Smarkets,
            cached_snapshot: None,
            browser_runner: None,
            api_runner: Some(runner),
        }
    }

    fn load_selected_snapshot(&self, refresh_kind: &str) -> Result<ExchangePanelSnapshot> {
        match self.selected_venue {
            VenueId::Smarkets => self.load_smarkets_snapshot(refresh_kind),
            venue => Ok(self.build_unavailable_live_venue_snapshot(venue)),
        }
    }

    fn load_smarkets_snapshot(&self, refresh_kind: &str) -> Result<ExchangePanelSnapshot> {
        let source = self
            .load_snapshot_source()
            .wrap_err("native provider could not load a Smarkets snapshot")?;
        let mut snapshot = source.snapshot;
        let updated_at = source.updated_at;

        snapshot.selected_venue = Some(VenueId::Smarkets);
        if snapshot.venues.is_empty() {
            snapshot.venues = vec![VenueSummary {
                id: VenueId::Smarkets,
                label: String::from("Smarkets"),
                status: venue_status_from_worker(snapshot.worker.status),
                detail: if snapshot.worker.detail.trim().is_empty() {
                    snapshot.status_line.clone()
                } else {
                    snapshot.worker.detail.clone()
                },
                event_count: snapshot
                    .watch
                    .as_ref()
                    .map(|watch| watch.watch_count)
                    .unwrap_or(snapshot.open_positions.len()),
                market_count: snapshot
                    .watch
                    .as_ref()
                    .map(|watch| watch.watches.len())
                    .unwrap_or_default(),
            }];
        }
        if snapshot.status_line.trim().is_empty() {
            snapshot.status_line = if snapshot.worker.detail.trim().is_empty() {
                String::from("Loaded native Smarkets snapshot.")
            } else {
                snapshot.worker.detail.clone()
            };
        }

        if let Some(runtime) = snapshot.runtime.as_mut() {
            runtime.refresh_kind = String::from(refresh_kind);
            if runtime.source.trim().is_empty() {
                runtime.source = String::from("native-provider");
            }
            if runtime.updated_at.trim().is_empty() {
                runtime.updated_at = updated_at.clone();
            }
        } else {
            snapshot.runtime = Some(RuntimeSummary {
                updated_at: updated_at.clone(),
                source: String::from("native-provider"),
                refresh_kind: String::from(refresh_kind),
                worker_reconnect_count: 0,
                decision_count: snapshot.decisions.len(),
                watcher_iteration: None,
                stale: false,
            });
        }

        if let Some(run_dir) = self.config.run_dir.as_deref() {
            if snapshot.recorder_bundle.is_none() {
                snapshot.recorder_bundle = Some(build_recorder_bundle_summary(run_dir));
            }
            if snapshot.recorder_events.is_empty() {
                snapshot.recorder_events = load_recent_recorder_events(run_dir);
            }
            if snapshot.transport_summary.is_none() {
                snapshot.transport_summary = Some(build_transport_summary(run_dir));
            }
            if snapshot.transport_events.is_empty() {
                snapshot.transport_events = load_recent_transport_events(run_dir);
            }
        }

        Ok(snapshot)
    }

    fn load_snapshot_source(&self) -> Result<SnapshotSource> {
        if let Some(run_dir) = self.config.run_dir.as_deref() {
            let watcher_state_path = run_dir.join("watcher-state.json");
            if watcher_state_path.exists() {
                return load_snapshot_source_from_path(&watcher_state_path);
            }
        }

        if let Some(path) = self.config.positions_payload_path.as_deref() {
            if let Ok(source) = load_snapshot_source_from_path(path) {
                return Ok(source);
            }
            return load_snapshot_source_from_positions_payload(path, &self.config);
        }

        Err(eyre!(
            "native provider needs run_dir/watcher-state.json or a snapshot-shaped positions_payload_path"
        ))
    }

    fn build_unavailable_live_venue_snapshot(&self, venue: VenueId) -> ExchangePanelSnapshot {
        if let Some(cached) = self.cached_snapshot.as_ref() {
            if cached.selected_venue == Some(venue) {
                return cached.clone();
            }
        }

        ExchangePanelSnapshot {
            worker: crate::domain::WorkerSummary {
                name: String::from("native-provider"),
                status: WorkerStatus::Idle,
                detail: format!(
                    "{} direct CDP ingestion is not ported into Rust yet.",
                    venue.as_str()
                ),
            },
            venues: vec![
                VenueSummary {
                    id: VenueId::Smarkets,
                    label: String::from("Smarkets"),
                    status: VenueStatus::Ready,
                    detail: String::from("native snapshot"),
                    event_count: 0,
                    market_count: 0,
                },
                VenueSummary {
                    id: venue,
                    label: display_label(venue),
                    status: VenueStatus::Planned,
                    detail: String::from("Rust CDP port pending"),
                    event_count: 0,
                    market_count: 0,
                },
            ],
            selected_venue: Some(venue),
            status_line: format!(
                "{} is not available in the Rust-native provider yet.",
                display_label(venue)
            ),
            runtime: Some(RuntimeSummary {
                updated_at: String::new(),
                source: String::from("native-provider"),
                refresh_kind: String::from("cached"),
                worker_reconnect_count: 0,
                decision_count: 0,
                watcher_iteration: None,
                stale: false,
            }),
            ..ExchangePanelSnapshot::default()
        }
    }

    fn execute_trading_action(
        &mut self,
        intent: &TradingActionIntent,
    ) -> Result<NativeTradingResult> {
        if intent.venue == VenueId::Smarkets {
            return execute_smarkets_trade(
                intent,
                self.config.agent_browser_session.clone(),
                self.config.run_dir.as_deref(),
                self.browser_runner.clone(),
            );
        }
        if let Some(runner) = self.api_runner.as_ref() {
            return runner(intent);
        }
        let result = execute_exchange_api_trade(intent)?;
        Ok(NativeTradingResult {
            detail: result.detail,
            action_status: if intent.mode == crate::trading_actions::TradingActionMode::Review {
                String::from("review_ready")
            } else {
                String::from("submitted")
            },
        })
    }
}

impl ExchangeProvider for NativeExchangeProvider {
    fn handle(&mut self, request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        let snapshot = match request {
            ProviderRequest::LoadDashboard => self.load_selected_snapshot("bootstrap")?,
            ProviderRequest::RefreshCached => self.load_selected_snapshot("cached")?,
            ProviderRequest::RefreshLive => self.load_selected_snapshot("live_capture")?,
            ProviderRequest::SelectVenue(venue) => {
                self.selected_venue = venue;
                self.load_selected_snapshot(if venue == VenueId::Smarkets {
                    "cached"
                } else {
                    "live_capture"
                })?
            }
            ProviderRequest::CashOutTrackedBet { bet_id } => {
                let mut snapshot = self.load_selected_snapshot("cached")?;
                let result =
                    handle_native_cash_out(&mut snapshot, &bet_id, self.config.run_dir.as_deref());
                if let Err(error) = result {
                    let detail = error.to_string();
                    snapshot.worker.status = WorkerStatus::Error;
                    snapshot.worker.detail = detail.clone();
                    snapshot.status_line = detail;
                }
                self.cached_snapshot = Some(snapshot.clone());
                return Ok(snapshot);
            }
            ProviderRequest::ExecuteTradingAction { intent } => {
                let result = self.execute_trading_action(&intent)?;
                let mut snapshot = self.load_smarkets_snapshot("live_capture")?;
                snapshot.worker.status = WorkerStatus::Ready;
                snapshot.worker.detail = result.detail.clone();
                snapshot.status_line = result.detail;
                snapshot.selected_venue = Some(VenueId::Smarkets);
                self.cached_snapshot = Some(snapshot.clone());
                return Ok(snapshot);
            }
            ProviderRequest::LoadHorseMatcher { .. } => {
                return Err(eyre!(
                    "native provider does not load horse matcher feeds yet"
                ));
            }
        };

        self.cached_snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }
}

pub struct HybridExchangeProvider {
    primary: Box<dyn ExchangeProvider>,
    fallback: Box<dyn ExchangeProvider>,
}

impl HybridExchangeProvider {
    pub fn new(primary: Box<dyn ExchangeProvider>, fallback: Box<dyn ExchangeProvider>) -> Self {
        Self { primary, fallback }
    }
}

impl ExchangeProvider for HybridExchangeProvider {
    fn handle(&mut self, request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        match self.primary.handle(request.clone()) {
            Ok(snapshot) => Ok(snapshot),
            Err(_) => self.fallback.handle(request),
        }
    }
}

#[derive(Debug)]
struct SnapshotSource {
    snapshot: ExchangePanelSnapshot,
    updated_at: String,
}

fn load_snapshot_source_from_path(path: &Path) -> Result<SnapshotSource> {
    let content = fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read snapshot source {}", path.display()))?;
    let raw: Value =
        serde_json::from_str(&content).wrap_err("failed to decode snapshot source JSON")?;
    if raw.get("page").is_some() && raw.get("body_text").is_some() && raw.get("worker").is_none() {
        return Err(eyre!(
            "raw page payload is not a snapshot-shaped JSON document"
        ));
    }
    let snapshot = serde_json::from_str::<ExchangePanelSnapshot>(&content)
        .wrap_err("failed to deserialize exchange panel snapshot")?;
    let updated_at = raw
        .get("updated_at")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            raw.get("runtime")
                .and_then(|runtime| runtime.get("updated_at"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    Ok(SnapshotSource {
        snapshot,
        updated_at,
    })
}

fn load_snapshot_source_from_positions_payload(
    path: &Path,
    config: &WorkerConfig,
) -> Result<SnapshotSource> {
    let content = fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read positions payload {}", path.display()))?;
    let payload: Value =
        serde_json::from_str(&content).wrap_err("failed to decode positions payload")?;
    let page = payload
        .get("page")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if page != "open_positions" {
        return Err(eyre!("positions payload must be an open_positions capture"));
    }
    let body_text = payload
        .get("body_text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let snapshot = build_snapshot_from_positions_body(body_text, config);
    Ok(SnapshotSource {
        snapshot,
        updated_at: payload
            .get("captured_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn build_recorder_bundle_summary(run_dir: &Path) -> RecorderBundleSummary {
    let events = read_jsonl(run_dir.join("events.jsonl"));
    let latest_event = events.last();
    let latest_positions_at = events
        .iter()
        .rev()
        .find(|value| value.get("kind").and_then(Value::as_str) == Some("positions_snapshot"))
        .and_then(|value| value.get("captured_at"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let latest_watch_plan_at = events
        .iter()
        .rev()
        .find(|value| {
            value
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.contains("watch"))
        })
        .and_then(|value| value.get("captured_at"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    RecorderBundleSummary {
        run_dir: run_dir.display().to_string(),
        event_count: events.len(),
        latest_event_at: latest_event
            .and_then(|value| value.get("captured_at"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        latest_event_kind: latest_event
            .and_then(|value| value.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        latest_event_summary: latest_event
            .and_then(|value| value.get("summary"))
            .and_then(Value::as_str)
            .or_else(|| {
                latest_event
                    .and_then(|value| value.get("action"))
                    .and_then(Value::as_str)
            })
            .unwrap_or_default()
            .to_string(),
        latest_positions_at,
        latest_watch_plan_at,
    }
}

fn load_recent_recorder_events(run_dir: &Path) -> Vec<RecorderEventSummary> {
    take_last(
        read_jsonl(run_dir.join("events.jsonl")),
        DEFAULT_RECENT_EVENT_LIMIT,
    )
    .into_iter()
    .map(|value| RecorderEventSummary {
        captured_at: str_field(&value, "captured_at"),
        kind: str_field(&value, "kind"),
        source: str_field(&value, "source"),
        page: str_field(&value, "page"),
        action: str_field(&value, "action"),
        status: str_field(&value, "status"),
        request_id: str_field(&value, "request_id"),
        reference_id: str_field(&value, "reference_id"),
        summary: str_field(&value, "summary"),
        detail: first_present_string(&value, &["detail", "url", "document_title"]),
    })
    .collect()
}

fn build_transport_summary(run_dir: &Path) -> TransportCaptureSummary {
    let transport_path = run_dir.join("transport.jsonl");
    let markers = read_jsonl(&transport_path);
    let latest_marker = markers.last();

    TransportCaptureSummary {
        transport_path: transport_path.display().to_string(),
        marker_count: markers.len(),
        latest_marker_at: latest_marker
            .and_then(|value| value.get("captured_at"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        latest_marker_action: latest_marker
            .and_then(|value| value.get("action"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        latest_marker_phase: latest_marker
            .and_then(|value| value.get("phase"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        latest_marker_summary: latest_marker
            .and_then(|value| value.get("summary"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

fn handle_native_cash_out(
    snapshot: &mut ExchangePanelSnapshot,
    bet_id: &str,
    run_dir: Option<&Path>,
) -> Result<()> {
    record_cash_out_marker(
        run_dir,
        bet_id,
        "request",
        "requested",
        &format!("Cash out requested for {bet_id}."),
    )?;

    let tracked_bet = snapshot
        .tracked_bets
        .iter()
        .find(|tracked_bet| tracked_bet.bet_id == bet_id)
        .ok_or_else(|| eyre!("Tracked bet not found: {bet_id}"))?;
    let recommendation = snapshot
        .exit_recommendations
        .iter()
        .find(|recommendation| recommendation.bet_id == bet_id)
        .ok_or_else(|| eyre!("No exit recommendation is available for tracked bet: {bet_id}"))?;
    if recommendation.cash_out_venue.as_deref() != Some("smarkets") {
        return Err(eyre!("Tracked bet {bet_id} is not actionable on Smarkets"));
    }
    if recommendation.action != "cash_out" {
        return Err(eyre!(
            "Tracked bet {bet_id} is not currently marked for cash out"
        ));
    }

    let detail = format!(
        "Cash out requested for {}, but native Smarkets cash-out is not implemented yet.",
        tracked_bet.bet_id
    );
    snapshot.worker.status = WorkerStatus::Error;
    snapshot.worker.detail = detail.clone();
    snapshot.status_line = detail.clone();
    record_cash_out_marker(run_dir, bet_id, "response", "not_implemented", &detail)?;
    Ok(())
}

fn record_cash_out_marker(
    run_dir: Option<&Path>,
    bet_id: &str,
    phase: &str,
    status: &str,
    detail: &str,
) -> Result<()> {
    let Some(run_dir) = run_dir else {
        return Ok(());
    };
    fs::create_dir_all(run_dir)?;
    append_jsonl(
        &run_dir.join("events.jsonl"),
        json!({
            "captured_at": now_iso(),
            "source": "operator_console",
            "kind": "operator_interaction",
            "page": "worker_request",
            "action": "cash_out",
            "status": format!("{phase}:{status}"),
            "detail": detail,
            "reference_id": bet_id,
            "metadata": {
                "bet_id": bet_id,
            }
        }),
    )?;
    let transport_path = run_dir.join("transport.jsonl");
    if transport_path.exists() {
        append_jsonl(
            &transport_path,
            json!({
                "captured_at": now_iso(),
                "kind": "interaction_marker",
                "action": "cash_out",
                "phase": phase,
                "detail": detail,
                "reference_id": bet_id,
                "metadata": {
                    "bet_id": bet_id,
                    "status": status,
                }
            }),
        )?;
    }
    Ok(())
}

fn load_recent_transport_events(run_dir: &Path) -> Vec<TransportMarkerSummary> {
    take_last(
        read_jsonl(run_dir.join("transport.jsonl")),
        DEFAULT_RECENT_EVENT_LIMIT,
    )
    .into_iter()
    .map(|value| TransportMarkerSummary {
        captured_at: str_field(&value, "captured_at"),
        kind: str_field(&value, "kind"),
        action: str_field(&value, "action"),
        phase: str_field(&value, "phase"),
        request_id: str_field(&value, "request_id"),
        reference_id: str_field(&value, "reference_id"),
        summary: str_field(&value, "summary"),
        detail: first_present_string(&value, &["detail", "message"]),
    })
    .collect()
}

fn read_jsonl(path: impl AsRef<Path>) -> Vec<Value> {
    let path = path.as_ref();
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

fn append_jsonl(path: &Path, value: Value) -> Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(serde_json::to_string(&value)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn now_iso() -> String {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let output = Command::new("date")
        .arg("-u")
        .arg("-d")
        .arg(format!("@{now}"))
        .arg("+%Y-%m-%dT%H:%M:%SZ")
        .output();
    match output {
        Ok(value) if value.status.success() => {
            String::from_utf8_lossy(&value.stdout).trim().to_string()
        }
        _ => String::from("1970-01-01T00:00:00Z"),
    }
}

fn take_last(values: Vec<Value>, limit: usize) -> Vec<Value> {
    if values.len() <= limit {
        return values;
    }
    values[values.len() - limit..].to_vec()
}

fn str_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn first_present_string(value: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(found) = value.get(*key).and_then(Value::as_str) {
            if !found.trim().is_empty() {
                return found.to_string();
            }
        }
    }
    String::new()
}

fn venue_status_from_worker(status: WorkerStatus) -> VenueStatus {
    match status {
        WorkerStatus::Ready => VenueStatus::Ready,
        WorkerStatus::Busy => VenueStatus::Connected,
        WorkerStatus::Idle => VenueStatus::Planned,
        WorkerStatus::Error => VenueStatus::Error,
    }
}

fn display_label(venue: VenueId) -> String {
    match venue {
        VenueId::Bet365 => String::from("Bet365"),
        VenueId::Betfair => String::from("Betfair"),
        VenueId::Betway => String::from("Betway"),
        VenueId::Betuk => String::from("BetUK"),
        VenueId::Betfred => String::from("Betfred"),
        VenueId::Betmgm => String::from("BetMGM"),
        other => {
            let raw = other.as_str();
            let mut chars = raw.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        }
    }
}

fn build_snapshot_from_positions_body(
    body_text: &str,
    config: &WorkerConfig,
) -> ExchangePanelSnapshot {
    let account_stats = extract_account_stats(body_text);
    let other_open_bets = extract_open_bets(body_text);
    let open_positions = extract_sell_positions(body_text);
    let watch = build_watch_snapshot(
        &open_positions,
        config.commission_rate,
        config.target_profit,
        config.stop_loss,
    );
    let decisions = build_decisions(&watch);

    ExchangePanelSnapshot {
        worker: crate::domain::WorkerSummary {
            name: String::from("native-provider"),
            status: WorkerStatus::Ready,
            detail: format!(
                "Parsed {} watch groups and {} open positions in Rust.",
                watch.watch_count,
                open_positions.len()
            ),
        },
        venues: vec![VenueSummary {
            id: VenueId::Smarkets,
            label: String::from("Smarkets"),
            status: VenueStatus::Ready,
            detail: format!(
                "{} grouped watches across {} markets",
                watch.watch_count,
                watch.watches.len()
            ),
            event_count: watch.watch_count,
            market_count: watch.watches.len(),
        }],
        selected_venue: Some(VenueId::Smarkets),
        events: watch
            .watches
            .iter()
            .map(|row| crate::domain::EventCandidateSummary {
                id: format!("{}::{}", row.market, row.contract),
                label: row.contract.clone(),
                competition: row.market.clone(),
                start_time: format!(
                    "profit {:.2} stop {:.2}",
                    row.profit_take_back_odds, row.stop_loss_back_odds
                ),
                url: String::new(),
            })
            .collect(),
        markets: watch
            .watches
            .iter()
            .map(|row| crate::domain::MarketSummary {
                name: row.market.clone(),
                contract_count: row.position_count,
            })
            .collect(),
        status_line: format!(
            "Loaded {} Smarkets watch groups from native parsing.",
            watch.watch_count
        ),
        account_stats,
        open_positions,
        other_open_bets,
        decisions,
        watch: Some(watch.clone()),
        exit_policy: crate::domain::ExitPolicySummary {
            target_profit: config.target_profit,
            stop_loss: config.stop_loss,
            hard_margin_call_profit_floor: config.hard_margin_call_profit_floor,
            warn_only_default: config.warn_only_default,
        },
        runtime: Some(RuntimeSummary {
            updated_at: String::new(),
            source: String::from("native-positions-payload"),
            refresh_kind: String::new(),
            worker_reconnect_count: 0,
            decision_count: 0,
            watcher_iteration: None,
            stale: false,
        }),
        ..ExchangePanelSnapshot::default()
    }
}

fn extract_account_stats(body_text: &str) -> Option<AccountStats> {
    let available_balance = amount_after(body_text, "Available balance")?;
    let exposure = amount_after(body_text, "Exposure")?;
    let unrealized_pnl = signed_amount_after(body_text, "Unrealized P/L")?;
    Some(AccountStats {
        available_balance,
        exposure,
        unrealized_pnl,
        cumulative_pnl: None,
        cumulative_pnl_label: String::new(),
        currency: String::from("GBP"),
    })
}

fn extract_open_bets(body_text: &str) -> Vec<OtherOpenBetRow> {
    let Some(open_bets_index) = body_text.find("Open Bets") else {
        return Vec::new();
    };
    let first_sell_index = body_text[open_bets_index..]
        .find("Sell ")
        .map(|offset| open_bets_index + offset)
        .unwrap_or(body_text.len());
    let section = &body_text[open_bets_index..first_sell_index];
    let tokens = tokenize(section);
    let mut rows = Vec::new();
    let mut index = 0usize;

    while index < tokens.len() {
        if tokens[index] != "Back" {
            index += 1;
            continue;
        }
        let Some((market, market_len, market_start)) =
            find_market_phrase_tokens(&tokens, index + 1)
        else {
            break;
        };
        if market_start + market_len + 2 >= tokens.len() {
            break;
        }
        let label = tokens[index + 1..market_start].join(" ");
        let odds = parse_number_token(tokens[market_start + market_len]).unwrap_or(0.0);
        let stake = parse_money_token(tokens[market_start + market_len + 1]).unwrap_or(0.0);
        let status = tokens[market_start + market_len + 2].to_string();
        rows.push(OtherOpenBetRow {
            venue: String::new(),
            event: String::new(),
            label,
            market: market.to_string(),
            side: String::from("back"),
            odds,
            stake,
            status,
            funding_kind: String::new(),
            current_cashout_value: None,
            supports_cash_out: false,
        });
        index = market_start + market_len + 3;
    }

    rows
}

fn extract_sell_positions(body_text: &str) -> Vec<OpenPositionRow> {
    let mut rows = Vec::new();
    let mut search_from = 0usize;
    let mut last_event = String::new();

    while let Some(relative_start) = body_text[search_from..].find("Sell ") {
        let start = search_from + relative_start;
        let next_start = body_text[start + 5..]
            .find("Sell ")
            .map(|offset| start + 5 + offset)
            .unwrap_or(body_text.len());
        let prefix = body_text[search_from..start].trim();
        if prefix.contains(" vs ") {
            last_event = prefix.to_string();
        }
        let segment = body_text[start..next_start].trim();
        if let Some(row) = parse_sell_segment(segment, &last_event) {
            rows.push(row);
        }
        search_from = next_start;
    }

    rows
}

fn parse_sell_segment(segment: &str, event: &str) -> Option<OpenPositionRow> {
    let tokens = tokenize(segment);
    if tokens.first().copied() != Some("Sell") {
        return None;
    }
    let (market, market_len, market_start) = find_market_phrase_tokens(&tokens, 1)?;
    if market_start + market_len + 6 >= tokens.len() {
        return None;
    }

    let contract = tokens[1..market_start].join(" ");
    let price = parse_number_token(tokens[market_start + market_len])?;
    let stake = parse_money_token(tokens[market_start + market_len + 1])?;
    let liability = parse_money_token(tokens[market_start + market_len + 2])?;
    let current_value = parse_signed_money_token(tokens[market_start + market_len + 4])?;
    let pnl_amount = parse_signed_money_token(tokens[market_start + market_len + 5])?;
    let current_back_odds = segment
        .split("Trade out Back ")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .and_then(parse_number_token);
    let can_trade_out = segment.contains("Trade out");

    Some(OpenPositionRow {
        event: event.to_string(),
        event_status: String::new(),
        event_url: String::new(),
        contract,
        market: market.to_string(),
        status: String::from("matched"),
        market_status: if can_trade_out {
            String::from("open")
        } else {
            String::new()
        },
        is_in_play: false,
        price,
        stake,
        liability,
        current_value,
        pnl_amount,
        overall_pnl_known: true,
        current_back_odds,
        current_implied_probability: current_back_odds.map(|odds| 1.0 / odds),
        current_implied_percentage: current_back_odds.map(|odds| 100.0 / odds),
        current_buy_odds: current_back_odds,
        current_buy_implied_probability: current_back_odds.map(|odds| 1.0 / odds),
        current_sell_odds: None,
        current_sell_implied_probability: None,
        current_score: String::new(),
        current_score_home: None,
        current_score_away: None,
        live_clock: String::new(),
        can_trade_out,
    })
}

fn build_watch_snapshot(
    positions: &[OpenPositionRow],
    commission_rate: f64,
    target_profit: f64,
    stop_loss: f64,
) -> WatchSnapshot {
    let mut grouped: BTreeMap<(String, String), Vec<&OpenPositionRow>> = BTreeMap::new();
    for position in positions {
        grouped
            .entry((position.market.clone(), position.contract.clone()))
            .or_default()
            .push(position);
    }

    let watches = grouped
        .into_iter()
        .filter_map(|((market, contract), rows)| {
            let total_stake: f64 = rows.iter().map(|row| row.stake).sum();
            if total_stake <= 0.0 {
                return None;
            }
            let total_liability: f64 = rows.iter().map(|row| row.liability).sum();
            let weighted_entry_odds: f64 =
                rows.iter().map(|row| row.stake * row.price).sum::<f64>() / total_stake;
            let current_back_odds = weighted_current_back_odds(&rows);
            let profit_take_back_odds = exit_odds_for_target_profit(
                weighted_entry_odds,
                total_stake,
                commission_rate,
                target_profit,
            );
            let stop_loss_back_odds = exit_odds_for_target_profit(
                weighted_entry_odds,
                total_stake,
                commission_rate,
                -stop_loss,
            );
            Some(WatchRow {
                contract,
                market,
                position_count: rows.len(),
                can_trade_out: rows.iter().any(|row| row.can_trade_out),
                total_stake,
                total_liability,
                current_pnl_amount: rows.iter().map(|row| row.pnl_amount).sum(),
                current_back_odds,
                average_entry_lay_odds: weighted_entry_odds,
                entry_implied_probability: 1.0 / weighted_entry_odds,
                profit_take_back_odds,
                profit_take_implied_probability: 1.0 / profit_take_back_odds,
                stop_loss_back_odds,
                stop_loss_implied_probability: 1.0 / stop_loss_back_odds,
            })
        })
        .collect::<Vec<_>>();

    WatchSnapshot {
        position_count: positions.len(),
        watch_count: watches.len(),
        commission_rate,
        target_profit,
        stop_loss,
        watches,
    }
}

fn weighted_current_back_odds(rows: &[&OpenPositionRow]) -> Option<f64> {
    let positions = rows
        .iter()
        .filter_map(|row| row.current_back_odds.map(|odds| (row.stake, odds)))
        .collect::<Vec<_>>();
    if positions.is_empty() {
        return None;
    }
    let total_stake = positions.iter().map(|(stake, _)| *stake).sum::<f64>();
    Some(
        positions
            .iter()
            .map(|(stake, odds)| stake * odds)
            .sum::<f64>()
            / total_stake,
    )
}

fn build_decisions(watch: &WatchSnapshot) -> Vec<DecisionSummary> {
    watch
        .watches
        .iter()
        .filter_map(|row| {
            let current = row.current_back_odds?;
            let (status, reason) = if current >= row.profit_take_back_odds {
                ("take_profit_ready", "current_back_odds")
            } else if current <= row.stop_loss_back_odds {
                ("stop_loss_ready", "current_back_odds")
            } else {
                return None;
            };
            Some(DecisionSummary {
                contract: row.contract.clone(),
                market: row.market.clone(),
                status: String::from(status),
                reason: String::from(reason),
                current_pnl_amount: row.current_pnl_amount,
                current_back_odds: row.current_back_odds,
                profit_take_back_odds: row.profit_take_back_odds,
                stop_loss_back_odds: row.stop_loss_back_odds,
            })
        })
        .collect()
}

fn tokenize(value: &str) -> Vec<&str> {
    value.split_whitespace().collect()
}

fn find_market_phrase_tokens<'a>(
    tokens: &'a [&'a str],
    start: usize,
) -> Option<(&'static str, usize, usize)> {
    const MARKETS: [(&str, [&str; 4], usize); 4] = [
        (
            "Winner (including overtime)",
            ["Winner", "(including", "overtime)", ""],
            3,
        ),
        ("Full-time result", ["Full-time", "result", "", ""], 2),
        ("Correct score", ["Correct", "score", "", ""], 2),
        ("Bet Builder", ["Bet", "Builder", "", ""], 2),
    ];

    for index in start..tokens.len() {
        for (label, pattern, len) in MARKETS {
            if index + len > tokens.len() {
                continue;
            }
            if tokens[index..index + len]
                .iter()
                .zip(pattern.iter())
                .take(len)
                .all(|(left, right)| left == right)
            {
                return Some((label, len, index));
            }
        }
    }
    None
}

fn parse_number_token(token: &str) -> Option<f64> {
    token.trim().trim_matches(',').parse::<f64>().ok()
}

fn parse_money_token(token: &str) -> Option<f64> {
    token
        .trim()
        .trim_start_matches('£')
        .replace(',', "")
        .parse::<f64>()
        .ok()
}

fn parse_signed_money_token(token: &str) -> Option<f64> {
    let negative = token.trim().starts_with("-£");
    let amount = token
        .trim()
        .trim_start_matches("-£")
        .trim_start_matches('£')
        .replace(',', "")
        .parse::<f64>()
        .ok()?;
    Some(if negative { -amount } else { amount })
}

fn amount_after(text: &str, label: &str) -> Option<f64> {
    let start = text.find(label)?;
    let tail = &text[start + label.len()..];
    let token = tail.split_whitespace().find(|value| value.contains('£'))?;
    parse_money_token(token)
}

fn signed_amount_after(text: &str, label: &str) -> Option<f64> {
    let start = text.find(label)?;
    let tail = &text[start + label.len()..];
    let token = tail.split_whitespace().find(|value| value.contains('£'))?;
    parse_signed_money_token(token)
}

fn exit_odds_for_target_profit(
    entry_lay_odds: f64,
    lay_stake: f64,
    commission_rate: f64,
    target_profit: f64,
) -> f64 {
    let effective_commission = normalize_commission_rate(commission_rate);
    let denominator = (lay_stake * (1.0 - effective_commission)) - target_profit;
    if denominator <= 0.0 {
        return entry_lay_odds;
    }
    (lay_stake * (entry_lay_odds - effective_commission)) / denominator
}

fn normalize_commission_rate(value: f64) -> f64 {
    if value > 1.0 {
        value / 100.0
    } else {
        value
    }
}
