use std::path::PathBuf;

use operator_console::bet_recorder_provider::BetRecorderProvider;
use operator_console::provider::{WatchProvider, WatchRequest};

fn project_root() -> PathBuf {
    PathBuf::from("/home/thomas/projects/sabi/bet-recorder")
}

#[test]
fn provider_loads_watch_snapshot_from_bet_recorder_cli() {
    let mut provider = BetRecorderProvider::new(
        PathBuf::from("/home/thomas/projects/sabi/bet-recorder/.venv/bin/python"),
        project_root(),
    );

    let snapshot = provider
        .load_watch_snapshot(&WatchRequest {
            positions_payload_path: Some(PathBuf::from(
                "/home/thomas/projects/sabi/console/operator-console/fixtures/smarkets-open-positions.json",
            )),
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
        })
        .expect("provider should return watch snapshot");

    assert_eq!(snapshot.watch_count, 2);
    assert_eq!(snapshot.watches[0].contract, "1 - 1");
    assert!((snapshot.watches[0].profit_take_back_odds - 10.87).abs() < 0.01);
    assert!((snapshot.watches[1].stop_loss_back_odds - 3.04).abs() < 0.01);
}
