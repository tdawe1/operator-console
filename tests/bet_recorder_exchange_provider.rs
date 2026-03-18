use std::path::PathBuf;

use operator_console::bet_recorder_provider::BetRecorderExchangeProvider;
use operator_console::domain::VenueId;
use operator_console::provider::{ExchangeProvider, WatchRequest};

fn project_root() -> PathBuf {
    PathBuf::from("/home/thomas/projects/sabi/bet-recorder")
}

#[test]
fn exchange_provider_maps_watch_snapshot_into_exchange_panel_snapshot() {
    let watch_request = WatchRequest {
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
    };
    let mut provider = BetRecorderExchangeProvider::new(
        PathBuf::from("/home/thomas/projects/sabi/bet-recorder/.venv/bin/python"),
        project_root(),
        watch_request,
    );

    let snapshot = provider
        .handle(operator_console::provider::ProviderRequest::LoadDashboard)
        .expect("exchange snapshot");

    assert_eq!(snapshot.selected_venue, Some(VenueId::Smarkets));
    assert_eq!(snapshot.venues.len(), 1);
    assert_eq!(snapshot.venues[0].event_count, 2);
    assert!(snapshot.status_line.contains("bet-recorder"));
    assert_eq!(
        snapshot.watch.as_ref().expect("watch snapshot").watch_count,
        2
    );
}
