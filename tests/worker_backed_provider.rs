use std::path::PathBuf;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use operator_console::domain::VenueId;
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use operator_console::transport::WorkerConfig;
use operator_console::worker_client::{BetRecorderWorkerClient, WorkerClientExchangeProvider};

fn project_root() -> PathBuf {
    PathBuf::from("/home/thomas/projects/sabi/bet-recorder")
}

#[test]
fn worker_backed_provider_maps_bet_recorder_snapshot() {
    let client = BetRecorderWorkerClient::new(
        PathBuf::from("/home/thomas/projects/sabi/bet-recorder/.venv/bin/python"),
        project_root(),
    );
    let mut provider = WorkerClientExchangeProvider::new(
        client,
        WorkerConfig {
            positions_payload_path: Some(PathBuf::from(
                "/home/thomas/projects/sabi/console/operator-console/fixtures/smarkets-open-positions.json",
            )),
            run_dir: None,
            account_payload_path: None,
            open_bets_payload_path: None,
            agent_browser_session: None,
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
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
    assert_eq!(snapshot.open_positions.len(), 3);
    assert_eq!(snapshot.other_open_bets.len(), 2);
    assert_eq!(
        snapshot.watch.as_ref().expect("watch snapshot").watch_count,
        2
    );
    assert!(snapshot.status_line.contains("bet-recorder"));
}

#[test]
fn worker_backed_provider_loads_latest_positions_snapshot_from_run_dir() {
    let run_dir = temp_run_dir("operator-console-run-dir");
    fs::write(
        run_dir.join("events.jsonl"),
        concat!(
            "{\"captured_at\":\"2026-03-11T11:00:00Z\",\"source\":\"smarkets_exchange\",\"kind\":\"positions_snapshot\",\"page\":\"open_positions\",\"url\":\"https://smarkets.com/open-positions\",\"document_title\":\"Open positions\",\"body_text\":\"Available balance £120.45 Exposure £41.63 Unrealized P/L -£0.49 Open Bets Back Arsenal Full-time result 2.12 £5.00 Open Lazio vs Sassuolo Sell Draw Full-time result 3.35 £9.91 £23.29 £33.20 £9.60 -£0.31 (3.13%) Order filled Trade out\",\"interactive_snapshot\":[],\"links\":[],\"inputs\":{},\"visible_actions\":[\"Trade out\"],\"resource_hosts\":[\"smarkets.com\"],\"local_storage_keys\":[],\"screenshot_path\":null,\"notes\":[]}\n",
            "{\"captured_at\":\"2026-03-11T11:05:00Z\",\"source\":\"smarkets_exchange\",\"kind\":\"positions_snapshot\",\"page\":\"open_positions\",\"url\":\"https://smarkets.com/open-positions\",\"document_title\":\"Open positions\",\"body_text\":\"Available balance £150.00 Exposure £23.29 Unrealized P/L £2.10 Open Bets Back Arsenal Full-time result 2.12 £5.00 Open Back Both Teams To Score Bet Builder 1.74 £3.50 Open Lazio vs Sassuolo Sell 1 - 1 Correct score 7.2 £2.55 £15.81 £18.36 £2.46 -£0.09 (3.53%) Order filled Trade out Sell Draw Full-time result 3.35 £9.91 £23.29 £33.20 £9.60 -£0.31 (3.13%) Order filled Trade out\",\"interactive_snapshot\":[],\"links\":[],\"inputs\":{},\"visible_actions\":[\"Trade out\"],\"resource_hosts\":[\"smarkets.com\"],\"local_storage_keys\":[],\"screenshot_path\":null,\"notes\":[]}\n"
        ),
    )
    .expect("events");

    let client = BetRecorderWorkerClient::new(
        PathBuf::from("/home/thomas/projects/sabi/bet-recorder/.venv/bin/python"),
        project_root(),
    );
    let mut provider = WorkerClientExchangeProvider::new(
        client,
        WorkerConfig {
            positions_payload_path: None,
            run_dir: Some(run_dir),
            account_payload_path: None,
            open_bets_payload_path: None,
            agent_browser_session: None,
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
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
        150.0
    );
    assert_eq!(snapshot.open_positions.len(), 2);
    assert_eq!(snapshot.other_open_bets.len(), 2);
    assert_eq!(
        snapshot.watch.as_ref().expect("watch snapshot").watch_count,
        2
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
