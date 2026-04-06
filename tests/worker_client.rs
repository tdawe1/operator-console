use std::path::PathBuf;
use std::{env, fs};

use operator_console::domain::VenueId;
use operator_console::horse_matcher::{HorseMatcherMode, HorseMatcherQuery};
use operator_console::trading_actions::{
    TradingActionIntent, TradingActionKind, TradingActionMode, TradingActionSide,
    TradingActionSource, TradingActionSourceContext, TradingExecutionPolicy, TradingRiskReport,
    TradingTimeInForce,
};
use operator_console::transport::WorkerConfig;
use operator_console::worker_client::{WorkerRequest, WorkerResponse};
use serde_json::Value;

#[test]
fn worker_request_serializes_load_dashboard() {
    let positions_payload_path = "/tmp/open-positions.json";
    let run_dir = "/tmp/smarkets-run";
    let account_payload_path = "/tmp/account.json";
    let open_bets_payload_path = "/tmp/open-bets.json";
    let companion_legs_path = "/tmp/companion-legs.json";
    let agent_browser_session = "helium-copy";
    let request = serde_json::to_string(&WorkerRequest::LoadDashboard {
        config: WorkerConfig {
            positions_payload_path: Some(PathBuf::from(positions_payload_path)),
            run_dir: Some(PathBuf::from(run_dir)),
            account_payload_path: Some(PathBuf::from(account_payload_path)),
            open_bets_payload_path: Some(PathBuf::from(open_bets_payload_path)),
            companion_legs_path: Some(PathBuf::from(companion_legs_path)),
            agent_browser_session: Some(String::from(agent_browser_session)),
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
            hard_margin_call_profit_floor: None,
            warn_only_default: true,
        },
    })
    .expect("serialize");
    let expected = fixture("load_dashboard_request_template.json")
        .replace("__POSITIONS_PAYLOAD_PATH__", positions_payload_path)
        .replace("__RUN_DIR__", run_dir)
        .replace("__ACCOUNT_PAYLOAD_PATH__", account_payload_path)
        .replace("__COMPANION_LEGS_PATH__", companion_legs_path)
        .replace("__AGENT_BROWSER_SESSION__", agent_browser_session)
        .replace("__OPEN_BETS_PAYLOAD_PATH__", open_bets_payload_path);

    assert_json_eq(&request, &expected);
}

#[test]
fn worker_request_serializes_load_dashboard_with_run_dir_only() {
    let run_dir = "/tmp/smarkets-run";
    let request = serde_json::to_string(&WorkerRequest::LoadDashboard {
        config: WorkerConfig {
            positions_payload_path: None,
            run_dir: Some(PathBuf::from(run_dir)),
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
    })
    .expect("serialize");
    let expected = fixture("load_dashboard_request_template.json")
        .replace("\"__POSITIONS_PAYLOAD_PATH__\"", "null")
        .replace("__RUN_DIR__", run_dir)
        .replace("\"__ACCOUNT_PAYLOAD_PATH__\"", "null")
        .replace("\"__COMPANION_LEGS_PATH__\"", "null")
        .replace("\"__AGENT_BROWSER_SESSION__\"", "null")
        .replace("\"__OPEN_BETS_PAYLOAD_PATH__\"", "null");

    assert_json_eq(&request, &expected);
}

#[test]
fn worker_request_serializes_select_venue_stably() {
    let request = serde_json::to_string(&WorkerRequest::SelectVenue {
        venue: VenueId::Smarkets,
    })
    .expect("serialize");

    assert_json_eq(&request, &fixture("select_venue_request.json"));
}

#[test]
fn worker_request_serializes_bet365_select_venue() {
    let request = serde_json::to_string(&WorkerRequest::SelectVenue {
        venue: VenueId::Bet365,
    })
    .expect("serialize");

    assert_json_eq(&request, r#"{"SelectVenue":{"venue":"bet365"}}"#);
}

#[test]
fn worker_request_serializes_refresh_cached_stably() {
    let request = serde_json::to_string(&WorkerRequest::RefreshCached).expect("serialize");

    assert_json_eq(&request, r#""RefreshCached""#);
}

#[test]
fn worker_request_serializes_refresh_live_stably() {
    let request = serde_json::to_string(&WorkerRequest::RefreshLive).expect("serialize");

    assert_json_eq(&request, r#""RefreshLive""#);
}

#[test]
fn worker_request_serializes_cash_out_tracked_bet() {
    let request = serde_json::to_string(&WorkerRequest::CashOutTrackedBet {
        bet_id: String::from("bet-001"),
    })
    .expect("serialize");

    assert_json_eq(&request, r#"{"CashOutTrackedBet":{"bet_id":"bet-001"}}"#);
}

#[test]
fn worker_request_serializes_execute_trading_action() {
    let request = serde_json::to_string(&WorkerRequest::ExecuteTradingAction {
        intent: Box::new(TradingActionIntent {
            action_kind: TradingActionKind::PlaceBet,
            source: TradingActionSource::Positions,
            venue: VenueId::Smarkets,
            mode: TradingActionMode::Review,
            side: TradingActionSide::Buy,
            request_id: String::from("positions-123"),
            source_ref: String::from("bet-001"),
            event_name: String::from("Arsenal v Everton"),
            market_name: String::from("Match Odds"),
            selection_name: String::from("Arsenal"),
            stake: 10.0,
            expected_price: 2.34,
            event_url: Some(String::from("https://smarkets.com/event/1")),
            deep_link_url: None,
            betslip_event_id: None,
            betslip_market_id: None,
            betslip_selection_id: None,
            execution_policy: TradingExecutionPolicy::new(TradingTimeInForce::FillOrKill),
            risk_report: TradingRiskReport {
                summary: String::from("Ready with 1 warning(s)."),
                checks: Vec::new(),
                warning_count: 1,
                blocking_review_count: 0,
                blocking_submit_count: 0,
                reduce_only: true,
            },
            source_context: TradingActionSourceContext {
                is_in_play: true,
                event_status: String::from("27'"),
                market_status: String::from("tradable"),
                live_clock: String::from("27'"),
                can_trade_out: true,
                current_pnl_amount: Some(1.0),
                baseline_stake: Some(9.91),
                baseline_liability: Some(23.29),
                baseline_price: Some(3.35),
            },
            notes: vec![String::from("positions")],
        }),
    })
    .expect("serialize");

    assert_json_eq(
        &request,
        r#"{
          "ExecuteTradingAction": {
            "intent": {
              "action_kind": "place_bet",
              "source": "positions",
              "venue": "smarkets",
              "mode": "review",
              "side": "buy",
              "request_id": "positions-123",
              "source_ref": "bet-001",
              "event_name": "Arsenal v Everton",
              "market_name": "Match Odds",
              "selection_name": "Arsenal",
              "stake": 10.0,
              "expected_price": 2.34,
              "event_url": "https://smarkets.com/event/1",
              "deep_link_url": null,
              "betslip_event_id": null,
              "betslip_market_id": null,
              "betslip_selection_id": null,
              "execution_policy": {
                "time_in_force": "fill_or_kill",
                "cancel_unmatched_after_ms": 1500,
                "require_full_fill": true,
                "max_price_drift": 0.0
              },
              "risk_report": {
                "summary": "Ready with 1 warning(s).",
                "checks": [],
                "warning_count": 1,
                "blocking_review_count": 0,
                "blocking_submit_count": 0,
                "reduce_only": true
              },
              "source_context": {
                "is_in_play": true,
                "event_status": "27'",
                "market_status": "tradable",
                "live_clock": "27'",
                "can_trade_out": true,
                "current_pnl_amount": 1.0,
                "baseline_stake": 9.91,
                "baseline_liability": 23.29,
                "baseline_price": 3.35
              },
              "notes": ["positions"]
            }
          }
        }"#,
    );
}

#[test]
fn worker_request_serializes_load_horse_matcher() {
    let request = serde_json::to_string(&WorkerRequest::LoadHorseMatcher {
        query: Box::new(HorseMatcherQuery {
            mode: HorseMatcherMode::RacesPerOffer,
            bookmakers: vec![String::from("betfred"), String::from("coral")],
            exchanges: vec![String::from("smarkets"), String::from("betdaq")],
            rating_type: String::from("rating"),
            min_rating: Some(String::from("95")),
            min_odds: Some(String::from("3.0")),
            search: vec![String::from("Cheltenham 15:20")],
            limit: 25,
            date_from: Some(String::from("2026-03-19")),
            date_to: Some(String::from("2026-03-20")),
            offers: vec![],
            offer_types: vec![],
        }),
    })
    .expect("serialize");

    assert_json_eq(
        &request,
        r#"{
          "LoadHorseMatcher": {
            "query": {
              "mode": "races_per_offer",
              "bookmakers": ["betfred", "coral"],
              "exchanges": ["smarkets", "betdaq"],
              "rating_type": "rating",
              "min_rating": "95",
              "min_odds": "3.0",
              "search": ["Cheltenham 15:20"],
              "limit": 25,
              "date_from": "2026-03-19",
              "date_to": "2026-03-20",
              "offers": [],
              "offer_types": []
            }
          }
        }"#,
    );
}

#[test]
fn worker_response_deserializes_ledger_sections() {
    let response: WorkerResponse = serde_json::from_str(
        r#"{
          "snapshot": {
            "worker": {
              "name": "bet-recorder",
              "status": "ready",
              "detail": "Loaded ledger snapshot"
            },
            "venues": [],
            "selected_venue": "smarkets",
            "events": [],
            "markets": [],
            "preflight": null,
            "status_line": "Loaded ledger snapshot",
            "runtime": null,
            "account_stats": null,
            "open_positions": [],
            "other_open_bets": [],
            "decisions": [],
            "watch": null,
            "tracked_bets": [
              {
                "bet_id": "bet-001",
                "group_id": "group-arsenal-everton",
                "platform": "bet365",
                "exchange": "smarkets",
                "sport_key": "soccer_epl",
                "sport_name": "Premier League",
                "bet_type": "single",
                "market_family": "match_odds",
                "back_price": 2.12,
                "lay_price": 3.35,
                "expected_ev": {
                  "gbp": 0.42,
                  "pct": 0.21,
                  "method": "fair_price",
                  "source": "local_formula",
                  "status": "calculated"
                },
                "event": "Arsenal v Everton",
                "market": "Full-time result",
                "selection": "Draw",
                "status": "open",
                "legs": []
              }
            ],
            "exit_policy": {
              "target_profit": 1.0,
              "stop_loss": 1.0,
              "hard_margin_call_profit_floor": null,
              "warn_only_default": true
            },
            "exit_recommendations": [
              {
                "bet_id": "bet-001",
                "action": "warn",
                "reason": "target not reached",
                "worst_case_pnl": 0.82,
                "cash_out_venue": "smarkets"
              }
            ]
          }
        }"#,
    )
    .expect("response should parse");

    assert_eq!(response.snapshot.tracked_bets.len(), 1);
    assert_eq!(response.snapshot.tracked_bets[0].platform, "bet365");
    assert_eq!(
        response.snapshot.tracked_bets[0].exchange.as_deref(),
        Some("smarkets")
    );
    assert_eq!(
        response.snapshot.tracked_bets[0].expected_ev.gbp,
        Some(0.42)
    );
    assert_eq!(response.snapshot.exit_recommendations.len(), 1);
    assert!(response.snapshot.exit_policy.warn_only_default);
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("worker")
        .join(name);
    fs::read_to_string(path).expect("fixture")
}

fn assert_json_eq(actual: &str, expected: &str) {
    let actual_json: Value = serde_json::from_str(actual).expect("actual json");
    let expected_json: Value = serde_json::from_str(expected).expect("expected json");
    assert_eq!(actual_json, expected_json);
}
