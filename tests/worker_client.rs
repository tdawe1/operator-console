use std::path::PathBuf;
use std::{env, fs};

use operator_console::domain::VenueId;
use operator_console::transport::WorkerConfig;
use operator_console::worker_client::WorkerRequest;
use serde_json::Value;

#[test]
fn worker_request_serializes_load_dashboard() {
    let positions_payload_path = "/tmp/open-positions.json";
    let run_dir = "/tmp/smarkets-run";
    let account_payload_path = "/tmp/account.json";
    let open_bets_payload_path = "/tmp/open-bets.json";
    let agent_browser_session = "helium-copy";
    let request = serde_json::to_string(&WorkerRequest::LoadDashboard {
        config: WorkerConfig {
            positions_payload_path: Some(PathBuf::from(positions_payload_path)),
            run_dir: Some(PathBuf::from(run_dir)),
            account_payload_path: Some(PathBuf::from(account_payload_path)),
            open_bets_payload_path: Some(PathBuf::from(open_bets_payload_path)),
            agent_browser_session: Some(String::from(agent_browser_session)),
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
        },
    })
    .expect("serialize");
    let expected = fixture("load_dashboard_request_template.json")
        .replace("__POSITIONS_PAYLOAD_PATH__", positions_payload_path)
        .replace("__RUN_DIR__", run_dir)
        .replace("__ACCOUNT_PAYLOAD_PATH__", account_payload_path)
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
            agent_browser_session: None,
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
        },
    })
    .expect("serialize");
    let expected = fixture("load_dashboard_request_template.json")
        .replace("\"__POSITIONS_PAYLOAD_PATH__\"", "null")
        .replace("__RUN_DIR__", run_dir)
        .replace("\"__ACCOUNT_PAYLOAD_PATH__\"", "null")
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
fn worker_request_serializes_refresh_stably() {
    let request = serde_json::to_string(&WorkerRequest::Refresh).expect("serialize");

    assert_json_eq(&request, &fixture("refresh_request.json"));
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
