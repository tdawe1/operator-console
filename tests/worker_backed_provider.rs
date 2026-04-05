use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use color_eyre::Result;

use operator_console::domain::{ExchangePanelSnapshot, ExitPolicySummary, VenueId};
use operator_console::horse_matcher::HorseMatcherQuery;
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use operator_console::trading_actions::{
    TradingActionIntent, TradingActionKind, TradingActionMode, TradingActionSide,
    TradingActionSource, TradingActionSourceContext, TradingExecutionPolicy, TradingRiskReport,
    TradingTimeInForce,
};
use operator_console::transport::WorkerConfig;
use operator_console::worker_client::{
    WorkerClient, WorkerClientExchangeProvider, WorkerRequest, WorkerResponse,
};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn sample_exchange_snapshot() -> ExchangePanelSnapshot {
    let fixture = r#"{
      "worker": {
        "name": "bet-recorder",
        "status": "ready",
        "detail": "Loaded richer snapshot"
      },
      "venues": [
        {
          "id": "smarkets",
          "label": "Smarkets",
          "status": "ready",
          "detail": "Richer snapshot loaded",
          "event_count": 3,
          "market_count": 2
        }
      ],
      "selected_venue": "smarkets",
      "events": [],
      "markets": [],
      "preflight": null,
      "status_line": "Loaded richer snapshot",
      "runtime": {
        "updated_at": "2026-03-11T12:05:00Z",
        "source": "watcher-state",
        "decision_count": 2,
        "watcher_iteration": 7,
        "stale": false
      },
      "account_stats": {
        "available_balance": 120.45,
        "exposure": 41.63,
        "unrealized_pnl": -0.49,
        "currency": "GBP"
      },
      "open_positions": [
        {
          "event": "West Ham vs Man City",
          "event_status": "27'|Premier League",
          "event_url": "https://smarkets.com/football/england-premier-league/2026/03/14/20-00/west-ham-vs-manchester-city/44919693/",
          "contract": "Draw",
          "market": "Full-time result",
          "price": 3.35,
          "stake": 9.91,
          "liability": 23.29,
          "current_value": 9.60,
          "pnl_amount": -0.31,
          "current_back_odds": 2.80,
          "current_implied_probability": 0.3571428571,
          "current_implied_percentage": 35.71428571,
          "current_score": "0-0",
          "current_score_home": 0,
          "current_score_away": 0,
          "can_trade_out": true
        }
      ],
      "historical_positions": [],
      "other_open_bets": [
        {
          "label": "Arsenal",
          "market": "Full-time result",
          "side": "back",
          "odds": 2.12,
          "stake": 5.00,
          "status": "Open"
        }
      ],
      "decisions": [],
      "watch": {
        "position_count": 3,
        "watch_count": 2,
        "commission_rate": 0.0,
        "target_profit": 1.0,
        "stop_loss": 1.0,
        "watches": []
      },
      "tracked_bets": [],
      "exit_policy": {
        "target_profit": 1.0,
        "stop_loss": 1.0,
        "hard_margin_call_profit_floor": null,
        "warn_only_default": true
      }
    }"#;

    serde_json::from_str(fixture).expect("sample exchange snapshot")
}

#[test]
fn worker_backed_provider_maps_bet_recorder_snapshot() {
    let client = FixtureWorkerClient {
        last_request: Arc::new(Mutex::new(None)),
        snapshot: sample_exchange_snapshot(),
    };
    let mut provider = WorkerClientExchangeProvider::new(
        client,
        WorkerConfig {
            positions_payload_path: Some(fixture_path("smarkets-open-positions.json")),
            run_dir: None,
            account_payload_path: None,
            open_bets_payload_path: None,
            companion_legs_path: None,
            agent_browser_session: None,
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
            hard_margin_call_profit_floor: None,
            warn_only_default: true,
        },
    );

    let snapshot = provider
        .handle(ProviderRequest::LoadDashboard)
        .expect("exchange snapshot");

    assert_eq!(snapshot.selected_venue, Some(VenueId::Smarkets));
    assert_eq!(
        snapshot
            .account_stats
            .as_ref()
            .expect("account stats")
            .currency,
        "GBP"
    );
    assert_eq!(snapshot.open_positions.len(), 1);
    assert_eq!(snapshot.other_open_bets.len(), 1);
    assert_eq!(
        snapshot.watch.as_ref().expect("watch snapshot").watch_count,
        2
    );
    assert_eq!(snapshot.status_line, "Loaded richer snapshot");
}

#[test]
fn worker_backed_provider_loads_latest_positions_snapshot_from_run_dir() {
    let run_dir = temp_run_dir("operator-console-run-dir");
    let last_request = Arc::new(Mutex::new(None));
    let client = FixtureWorkerClient {
        last_request: last_request.clone(),
        snapshot: sample_exchange_snapshot(),
    };
    let mut provider = WorkerClientExchangeProvider::new(
        client,
        WorkerConfig {
            positions_payload_path: None,
            run_dir: Some(run_dir.clone()),
            account_payload_path: None,
            open_bets_payload_path: None,
            companion_legs_path: None,
            agent_browser_session: None,
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
            hard_margin_call_profit_floor: None,
            warn_only_default: true,
        },
    );

    let snapshot = provider
        .handle(ProviderRequest::LoadDashboard)
        .expect("exchange snapshot");

    assert_eq!(snapshot.selected_venue, Some(VenueId::Smarkets));
    assert_eq!(
        snapshot
            .account_stats
            .as_ref()
            .expect("account stats")
            .available_balance,
        120.45
    );
    assert_eq!(snapshot.open_positions.len(), 1);
    assert_eq!(snapshot.other_open_bets.len(), 1);
    assert_eq!(
        snapshot.watch.as_ref().expect("watch snapshot").watch_count,
        2
    );
    assert_eq!(
        *last_request.lock().expect("lock"),
        Some(WorkerRequest::LoadDashboard {
            config: WorkerConfig {
                positions_payload_path: None,
                run_dir: Some(run_dir),
                account_payload_path: None,
                open_bets_payload_path: None,
                companion_legs_path: None,
                agent_browser_session: None,
                commission_rate: 0.0,
                target_profit: 1.0,
                stop_loss: 1.0,
                hard_margin_call_profit_floor: None,
                warn_only_default: true,
            }
        })
    );
}

fn temp_run_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
    fs::create_dir_all(&path).expect("mkdir");
    path
}

struct RecordingWorkerClient {
    last_request: Arc<Mutex<Option<WorkerRequest>>>,
}

impl WorkerClient for RecordingWorkerClient {
    fn send(&mut self, request: WorkerRequest) -> Result<WorkerResponse> {
        *self.last_request.lock().expect("lock") = Some(request);
        Ok(WorkerResponse {
            snapshot: ExchangePanelSnapshot {
                exit_policy: ExitPolicySummary::default(),
                ..ExchangePanelSnapshot::default()
            },
            request_error: None,
        })
    }
}

struct FixtureWorkerClient {
    last_request: Arc<Mutex<Option<WorkerRequest>>>,
    snapshot: ExchangePanelSnapshot,
}

impl WorkerClient for FixtureWorkerClient {
    fn send(&mut self, request: WorkerRequest) -> Result<WorkerResponse> {
        *self.last_request.lock().expect("lock") = Some(request);
        Ok(WorkerResponse {
            snapshot: self.snapshot.clone(),
            request_error: None,
        })
    }
}

#[test]
fn worker_backed_provider_maps_cash_out_request() {
    let last_request = Arc::new(Mutex::new(None));
    let client = RecordingWorkerClient {
        last_request: last_request.clone(),
    };
    let mut provider = WorkerClientExchangeProvider::new(
        client,
        WorkerConfig {
            positions_payload_path: None,
            run_dir: None,
            account_payload_path: None,
            open_bets_payload_path: None,
            companion_legs_path: None,
            agent_browser_session: None,
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
            hard_margin_call_profit_floor: None,
            warn_only_default: true,
        },
    );

    provider
        .handle(ProviderRequest::CashOutTrackedBet {
            bet_id: String::from("bet-001"),
        })
        .expect("cash out request should serialize");

    assert_eq!(
        *last_request.lock().expect("lock"),
        Some(WorkerRequest::CashOutTrackedBet {
            bet_id: String::from("bet-001"),
        })
    );
}

#[test]
fn worker_backed_provider_maps_execute_trading_action_request() {
    let last_request = Arc::new(Mutex::new(None));
    let client = RecordingWorkerClient {
        last_request: last_request.clone(),
    };
    let mut provider = WorkerClientExchangeProvider::new(
        client,
        WorkerConfig {
            positions_payload_path: None,
            run_dir: None,
            account_payload_path: None,
            open_bets_payload_path: None,
            companion_legs_path: None,
            agent_browser_session: Some(String::from("helium-copy")),
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
            hard_margin_call_profit_floor: None,
            warn_only_default: true,
        },
    );
    let intent = TradingActionIntent {
        action_kind: TradingActionKind::PlaceBet,
        source: TradingActionSource::OddsMatcher,
        venue: VenueId::Smarkets,
        mode: TradingActionMode::Review,
        side: TradingActionSide::Sell,
        request_id: String::from("oddsmatcher-1"),
        source_ref: String::from("match-1"),
        event_name: String::from("Arsenal v Everton"),
        market_name: String::from("Match Odds"),
        selection_name: String::from("Arsenal"),
        stake: 12.0,
        expected_price: 2.44,
        event_url: None,
        deep_link_url: Some(String::from("https://smarkets.com/betslip/1")),
        betslip_event_id: Some(String::from("event-1")),
        betslip_market_id: Some(String::from("market-1")),
        betslip_selection_id: Some(String::from("selection-1")),
        execution_policy: TradingExecutionPolicy::new(TradingTimeInForce::GoodTilCancel),
        risk_report: TradingRiskReport {
            summary: String::from("Ready; no blocking risk checks are active."),
            checks: Vec::new(),
            warning_count: 0,
            blocking_review_count: 0,
            blocking_submit_count: 0,
            reduce_only: false,
        },
        source_context: TradingActionSourceContext::default(),
        notes: vec![String::from("oddsmatcher")],
    };

    provider
        .handle(ProviderRequest::ExecuteTradingAction {
            intent: Box::new(intent.clone()),
        })
        .expect("execute trading action should serialize");

    assert_eq!(
        *last_request.lock().expect("lock"),
        Some(WorkerRequest::ExecuteTradingAction {
            intent: Box::new(intent),
        })
    );
}

#[test]
fn worker_backed_provider_maps_load_horse_matcher_request() {
    let last_request = Arc::new(Mutex::new(None));
    let client = RecordingWorkerClient {
        last_request: last_request.clone(),
    };
    let mut provider = WorkerClientExchangeProvider::new(
        client,
        WorkerConfig {
            positions_payload_path: None,
            run_dir: None,
            account_payload_path: None,
            open_bets_payload_path: None,
            companion_legs_path: None,
            agent_browser_session: Some(String::from("helium-copy")),
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
            hard_margin_call_profit_floor: None,
            warn_only_default: true,
        },
    );
    let query = HorseMatcherQuery::default();

    provider
        .handle(ProviderRequest::LoadHorseMatcher {
            query: Box::new(query.clone()),
        })
        .expect("load horse matcher should serialize");

    assert_eq!(
        *last_request.lock().expect("lock"),
        Some(WorkerRequest::LoadHorseMatcher {
            query: Box::new(query),
        })
    );
}
