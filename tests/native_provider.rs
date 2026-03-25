use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use color_eyre::Result;
use serde_json::{json, Value};

use operator_console::domain::{VenueId, WorkerStatus};
use operator_console::native_provider::{HybridExchangeProvider, NativeExchangeProvider};
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use operator_console::trading_actions::{
    TradingActionIntent, TradingActionKind, TradingActionMode, TradingActionSide,
    TradingActionSource, TradingActionSourceContext, TradingExecutionPolicy, TradingRiskReport,
    TradingTimeInForce,
};
use operator_console::transport::WorkerConfig;

struct ErrorProvider;

impl ExchangeProvider for ErrorProvider {
    fn handle(
        &mut self,
        _request: ProviderRequest,
    ) -> color_eyre::Result<operator_console::domain::ExchangePanelSnapshot> {
        Err(color_eyre::eyre::eyre!("fallback should not be used"))
    }
}

struct FallbackProvider;

impl ExchangeProvider for FallbackProvider {
    fn handle(
        &mut self,
        request: ProviderRequest,
    ) -> color_eyre::Result<operator_console::domain::ExchangePanelSnapshot> {
        Ok(operator_console::domain::ExchangePanelSnapshot {
            status_line: format!("fallback handled {request:?}"),
            ..Default::default()
        })
    }
}

#[test]
fn native_provider_loads_watcher_state_and_run_bundle_summaries() {
    let run_dir = temp_run_dir("native-provider");
    fs::write(
        run_dir.join("watcher-state.json"),
        r#"{
          "worker": { "name": "bet-recorder", "status": "ready", "detail": "watcher ready" },
          "status_line": "Watcher ready from disk.",
          "watch": { "position_count": 2, "watch_count": 1, "commission_rate": 0.0, "target_profit": 1.0, "stop_loss": 1.0, "watches": [] },
          "open_positions": [
            {
              "event": "Arsenal v Everton",
              "event_status": "12'",
              "event_url": "https://smarkets.com/event/1",
              "contract": "Arsenal",
              "market": "Match Odds",
              "price": 2.2,
              "stake": 5.0,
              "liability": 5.0,
              "current_value": 5.5,
              "pnl_amount": 0.5,
              "current_back_odds": 2.0,
              "current_implied_probability": 0.5,
              "current_implied_percentage": 50.0,
              "current_buy_odds": 2.0,
              "current_buy_implied_probability": 0.5,
              "current_sell_odds": null,
              "current_sell_implied_probability": null,
              "current_score": "1-0",
              "current_score_home": 1,
              "current_score_away": 0,
              "live_clock": "12:00",
              "can_trade_out": true
            }
          ],
          "decisions": [
            {
              "contract": "Arsenal",
              "market": "Match Odds",
              "status": "take_profit_ready",
              "reason": "current_back_odds",
              "current_pnl_amount": 0.5,
              "current_back_odds": 2.0,
              "profit_take_back_odds": 1.9,
              "stop_loss_back_odds": 2.5
            }
          ],
          "updated_at": "2026-03-24T10:00:00Z"
        }"#,
    )
    .expect("watcher state");
    fs::write(
        run_dir.join("events.jsonl"),
        concat!(
            "{\"captured_at\":\"2026-03-24T10:00:00Z\",\"kind\":\"positions_snapshot\",\"source\":\"smarkets_exchange\",\"page\":\"open_positions\",\"summary\":\"positions refreshed\",\"url\":\"https://smarkets.com/open-positions\"}\n",
            "{\"captured_at\":\"2026-03-24T10:01:00Z\",\"kind\":\"watch_plan\",\"source\":\"smarkets_exchange\",\"summary\":\"watch plan refreshed\"}\n"
        ),
    )
    .expect("events");
    fs::write(
        run_dir.join("transport.jsonl"),
        "{\"captured_at\":\"2026-03-24T10:01:01Z\",\"kind\":\"interaction_marker\",\"action\":\"place_bet\",\"phase\":\"response\",\"request_id\":\"req-1\",\"reference_id\":\"bet-1\",\"summary\":\"response place_bet req-1 bet-1\",\"detail\":\"loaded in review mode\"}\n",
    )
    .expect("transport");

    let mut provider = NativeExchangeProvider::new(WorkerConfig {
        positions_payload_path: None,
        run_dir: Some(run_dir.clone()),
        account_payload_path: None,
        open_bets_payload_path: None,
        companion_legs_path: None,
        agent_browser_session: Some(String::from("helium-copy")),
        commission_rate: 0.0,
        target_profit: 1.0,
        stop_loss: 1.0,
        hard_margin_call_profit_floor: None,
        warn_only_default: true,
    });

    let snapshot = provider
        .handle(ProviderRequest::LoadDashboard)
        .expect("native snapshot");

    assert_eq!(snapshot.selected_venue, Some(VenueId::Smarkets));
    assert_eq!(snapshot.worker.status, WorkerStatus::Ready);
    assert_eq!(snapshot.open_positions.len(), 1);
    assert_eq!(snapshot.runtime.expect("runtime").refresh_kind, "bootstrap");
    assert_eq!(
        snapshot
            .recorder_bundle
            .expect("bundle")
            .latest_watch_plan_at,
        "2026-03-24T10:01:00Z"
    );
    assert_eq!(snapshot.transport_events.len(), 1);
}

#[test]
fn native_provider_parses_raw_positions_payload_without_python() {
    let payload_path = std::env::temp_dir().join(format!(
        "native-positions-{}.json",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::write(
        &payload_path,
        r#"{
          "page": "open_positions",
          "body_text": "Available balance £120.45 Exposure £41.63 Unrealized P/L -£0.49 Open Bets Back Arsenal Full-time result 2.12 £5.00 Open Back Both Teams To Score Bet Builder 1.74 £3.50 Open Lazio vs Sassuolo Sell 1 - 1 Correct score 7.2 £2.55 £15.81 £18.36 £2.46 -£0.09 (3.53%) Order filled Trade out Sell 1 - 1 Correct score 7.2 £0.41 £2.53 £2.94 £0.32 -£0.09 (22.05%) Order filled Trade out Sell Draw Full-time result 3.35 £9.91 £23.29 £33.20 £9.60 -£0.31 (3.13%) Order filled Trade out",
          "inputs": {},
          "visible_actions": ["Trade out"]
        }"#,
    )
    .expect("payload");

    let mut provider = NativeExchangeProvider::new(WorkerConfig {
        positions_payload_path: Some(payload_path),
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
    });

    let snapshot = provider
        .handle(ProviderRequest::LoadDashboard)
        .expect("native parsed snapshot");

    assert_eq!(snapshot.open_positions.len(), 3);
    assert_eq!(snapshot.other_open_bets.len(), 2);
    assert_eq!(
        snapshot
            .account_stats
            .as_ref()
            .expect("account stats")
            .available_balance,
        120.45
    );
    assert_eq!(snapshot.watch.as_ref().expect("watch").watch_count, 2);
}

#[test]
fn hybrid_provider_falls_back_when_native_cannot_execute_action() {
    let mut provider = HybridExchangeProvider::new(
        Box::new(NativeExchangeProvider::new(WorkerConfig {
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
        })),
        Box::new(FallbackProvider),
    );

    let snapshot = provider
        .handle(ProviderRequest::CashOutTrackedBet {
            bet_id: String::from("bet-1"),
        })
        .expect("fallback snapshot");

    assert!(snapshot.status_line.contains("fallback handled"));
}

#[test]
fn hybrid_provider_uses_native_snapshot_when_available() -> Result<()> {
    let run_dir = temp_run_dir("hybrid-native");
    fs::write(
        run_dir.join("watcher-state.json"),
        r#"{
          "worker": { "name": "bet-recorder", "status": "ready", "detail": "native ready" },
          "status_line": "native watcher state"
        }"#,
    )?;

    let mut provider = HybridExchangeProvider::new(
        Box::new(NativeExchangeProvider::new(WorkerConfig {
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
        })),
        Box::new(ErrorProvider),
    );

    let snapshot = provider.handle(ProviderRequest::LoadDashboard)?;
    assert_eq!(snapshot.status_line, "native watcher state");
    Ok(())
}

#[test]
fn native_provider_executes_review_action_in_rust_and_records_markers() -> Result<()> {
    let run_dir = temp_run_dir("native-trade-review");
    fs::write(
        run_dir.join("watcher-state.json"),
        r#"{
          "worker": { "name": "bet-recorder", "status": "ready", "detail": "native ready" },
          "status_line": "native watcher state"
        }"#,
    )?;
    fs::write(run_dir.join("transport.jsonl"), "")?;

    let commands = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let runner = build_agent_browser_runner(commands.clone());
    let mut provider = NativeExchangeProvider::with_browser_runner(
        WorkerConfig {
            positions_payload_path: None,
            run_dir: Some(run_dir.clone()),
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
        runner,
    );

    let snapshot = provider.handle(ProviderRequest::ExecuteTradingAction {
        intent: sample_trading_intent(TradingActionMode::Review, TradingTimeInForce::GoodTilCancel),
    })?;

    assert_eq!(snapshot.worker.status, WorkerStatus::Ready);
    assert!(snapshot.status_line.contains("loaded in review mode"));
    assert_eq!(snapshot.selected_venue, Some(VenueId::Smarkets));

    let events = fs::read_to_string(run_dir.join("events.jsonl"))?;
    assert!(events.contains("\"kind\":\"operator_interaction\""));
    assert!(events.contains("\"status\":\"request:requested\""));
    assert!(events.contains("\"status\":\"response:review_ready\""));

    let transport = fs::read_to_string(run_dir.join("transport.jsonl"))?;
    assert!(transport.contains("\"phase\":\"request\""));
    assert!(transport.contains("\"phase\":\"response\""));

    let recorded = commands.lock().expect("commands");
    assert!(recorded
        .iter()
        .any(|command| has_browser_command(command, "open")));
    assert!(recorded
        .iter()
        .any(|command| has_browser_command(command, "eval")));
    assert!(!recorded.iter().any(|command| {
        has_browser_command(command, "eval")
            && command
                .last()
                .is_some_and(|script| script.contains("Place bet"))
    }));
    Ok(())
}

#[test]
fn native_provider_executes_confirm_action_in_rust() -> Result<()> {
    let run_dir = temp_run_dir("native-trade-confirm");
    fs::write(
        run_dir.join("watcher-state.json"),
        r#"{
          "worker": { "name": "bet-recorder", "status": "ready", "detail": "native ready" },
          "status_line": "native watcher state"
        }"#,
    )?;
    fs::write(run_dir.join("transport.jsonl"), "")?;

    let commands = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let runner = build_agent_browser_runner(commands.clone());
    let mut provider = NativeExchangeProvider::with_browser_runner(
        WorkerConfig {
            positions_payload_path: None,
            run_dir: Some(run_dir.clone()),
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
        runner,
    );

    let snapshot = provider.handle(ProviderRequest::ExecuteTradingAction {
        intent: sample_trading_intent(TradingActionMode::Confirm, TradingTimeInForce::FillOrKill),
    })?;

    assert_eq!(snapshot.worker.status, WorkerStatus::Ready);
    assert!(snapshot.status_line.contains("submitted"));

    let transport = fs::read_to_string(run_dir.join("transport.jsonl"))?;
    assert!(transport.contains("\"status\":\"submitted_fill_or_kill\""));

    let recorded = commands.lock().expect("commands");
    assert!(recorded.iter().any(|command| {
        has_browser_command(command, "eval")
            && command
                .last()
                .is_some_and(|script| script.contains("Place bet"))
    }));
    Ok(())
}

#[test]
fn native_provider_routes_matchbook_actions_to_api_runner() -> Result<()> {
    let run_dir = temp_run_dir("native-trade-matchbook");
    fs::write(
        run_dir.join("watcher-state.json"),
        r#"{
          "worker": { "name": "bet-recorder", "status": "ready", "detail": "native ready" },
          "status_line": "native watcher state"
        }"#,
    )?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_clone = seen.clone();
    let runner = Arc::new(move |intent: &TradingActionIntent| -> Result<_> {
        seen_clone
            .lock()
            .expect("seen")
            .push((intent.venue, intent.selection_name.clone()));
        Ok(operator_console::native_trading::NativeTradingResult {
            detail: String::from("Matchbook review ready: Arsenal @ 2.16"),
            action_status: String::from("review_ready"),
        })
    });
    let mut provider = NativeExchangeProvider::with_api_runner(
        WorkerConfig {
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
        },
        runner,
    );

    let snapshot = provider.handle(ProviderRequest::ExecuteTradingAction {
        intent: sample_api_trading_intent(),
    })?;

    assert!(snapshot.status_line.contains("Matchbook review ready"));
    let recorded = seen.lock().expect("seen");
    assert_eq!(
        recorded.as_slice(),
        &[(VenueId::Matchbook, String::from("Arsenal"))]
    );
    Ok(())
}

#[test]
fn native_provider_handles_cash_out_request_without_python() -> Result<()> {
    let run_dir = temp_run_dir("native-cashout");
    fs::write(
        run_dir.join("watcher-state.json"),
        r#"{
          "worker": { "name": "bet-recorder", "status": "ready", "detail": "native ready" },
          "status_line": "native watcher state",
          "tracked_bets": [
            {
              "bet_id": "bet-1",
              "group_id": "group-1",
              "event": "Arsenal v Everton",
              "market": "Match Odds",
              "selection": "Arsenal",
              "status": "open",
              "stake_gbp": 12.0,
              "legs": []
            }
          ],
          "exit_recommendations": [
            {
              "bet_id": "bet-1",
              "action": "cash_out",
              "reason": "take_profit",
              "worst_case_pnl": 1.2,
              "cash_out_venue": "smarkets"
            }
          ]
        }"#,
    )?;
    fs::write(run_dir.join("transport.jsonl"), "")?;

    let mut provider = NativeExchangeProvider::new(WorkerConfig {
        positions_payload_path: None,
        run_dir: Some(run_dir.clone()),
        account_payload_path: None,
        open_bets_payload_path: None,
        companion_legs_path: None,
        agent_browser_session: Some(String::from("helium-copy")),
        commission_rate: 0.0,
        target_profit: 1.0,
        stop_loss: 1.0,
        hard_margin_call_profit_floor: None,
        warn_only_default: true,
    });

    let snapshot = provider.handle(ProviderRequest::CashOutTrackedBet {
        bet_id: String::from("bet-1"),
    })?;

    assert_eq!(snapshot.worker.status, WorkerStatus::Error);
    assert!(snapshot
        .status_line
        .contains("cash-out is not implemented yet"));

    let events = fs::read_to_string(run_dir.join("events.jsonl"))?;
    assert!(events.contains("\"action\":\"cash_out\""));
    assert!(events.contains("\"status\":\"request:requested\""));
    assert!(events.contains("\"status\":\"response:not_implemented\""));

    let transport = fs::read_to_string(run_dir.join("transport.jsonl"))?;
    assert!(transport.contains("\"action\":\"cash_out\""));
    assert!(transport.contains("\"phase\":\"response\""));
    assert!(transport.contains("\"status\":\"not_implemented\""));
    Ok(())
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

fn sample_trading_intent(
    mode: TradingActionMode,
    time_in_force: TradingTimeInForce,
) -> TradingActionIntent {
    TradingActionIntent {
        action_kind: TradingActionKind::PlaceBet,
        source: TradingActionSource::OddsMatcher,
        venue: VenueId::Smarkets,
        mode,
        side: TradingActionSide::Sell,
        request_id: format!("req-{}", format!("{mode:?}").to_lowercase()),
        source_ref: String::from("match-1"),
        event_name: String::from("Arsenal v Everton"),
        market_name: String::from("Match Odds"),
        selection_name: String::from("Arsenal"),
        stake: 12.0,
        expected_price: 2.44,
        event_url: None,
        deep_link_url: Some(String::from("https://smarkets.com/event/1/betslip")),
        betslip_market_id: Some(String::from("market-1")),
        betslip_selection_id: Some(String::from("selection-1")),
        execution_policy: TradingExecutionPolicy::new(time_in_force),
        risk_report: TradingRiskReport {
            summary: String::from("Ready; no blocking risk checks are active."),
            checks: Vec::new(),
            warning_count: 0,
            blocking_review_count: 0,
            blocking_submit_count: 0,
            reduce_only: false,
        },
        source_context: TradingActionSourceContext::default(),
        notes: vec![String::from("native test")],
    }
}

fn sample_api_trading_intent() -> TradingActionIntent {
    TradingActionIntent {
        venue: VenueId::Matchbook,
        expected_price: 2.16,
        deep_link_url: Some(String::from("https://www.matchbook.com/events/1")),
        betslip_selection_id: Some(String::from("runner-7")),
        notes: vec![String::from("runner_id:runner-7")],
        ..sample_trading_intent(TradingActionMode::Review, TradingTimeInForce::GoodTilCancel)
    }
}

fn build_agent_browser_runner(
    commands: Arc<Mutex<Vec<Vec<String>>>>,
) -> Arc<dyn Fn(&[String]) -> Result<Value> + Send + Sync> {
    Arc::new(move |command: &[String]| -> Result<Value> {
        commands.lock().expect("commands").push(command.to_vec());
        let verb = browser_verb(command);
        let payload = match verb {
            "open" | "wait" => json!({
                "success": true,
                "data": {}
            }),
            "eval" => {
                let script = command.last().map(String::as_str).unwrap_or_default();
                if script.contains("return !!contract && !!side && !!stake") {
                    json!({ "success": true, "data": { "result": true } })
                } else if script.contains("input instanceof HTMLInputElement") {
                    json!({ "success": true, "data": { "result": true } })
                } else if script.contains("descriptor.set.call(element, value)") {
                    json!({ "success": true, "data": { "result": { "valueLength": 5 } } })
                } else if script.contains("contractText:") {
                    json!({
                        "success": true,
                        "data": {
                            "result": {
                                "contractText": "Arsenal",
                                "sideText": "Sell",
                                "stakeValue": "12.00",
                                "priceValue": "2.44",
                                "bodyText": "Current price 2.44"
                            }
                        }
                    })
                } else if script.contains("Place bet") {
                    json!({ "success": true, "data": { "result": true } })
                } else {
                    return Err(color_eyre::eyre::eyre!(
                        "unexpected agent-browser eval script: {script}"
                    ));
                }
            }
            other => {
                return Err(color_eyre::eyre::eyre!(
                    "unexpected agent-browser verb: {other}"
                ));
            }
        };
        Ok(payload)
    })
}

fn browser_verb(command: &[String]) -> &str {
    command
        .iter()
        .position(|part| part == "--json")
        .and_then(|index| command.get(index + 1))
        .map(String::as_str)
        .unwrap_or_default()
}

fn has_browser_command(command: &[String], expected_verb: &str) -> bool {
    browser_verb(command) == expected_verb
}
