use std::path::PathBuf;
use std::{env, fs};

use operator_console::domain::VenueId;
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
fn worker_request_serializes_refresh_stably() {
    let request = serde_json::to_string(&WorkerRequest::Refresh).expect("serialize");

    assert_json_eq(&request, &fixture("refresh_request.json"));
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
