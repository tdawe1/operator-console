use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use color_eyre::Result;
use operator_console::app::{App, Panel, TradingSection};
use operator_console::domain::{ExchangePanelSnapshot, OpenPositionRow};
use operator_console::owls::{self, OwlsLiveScoreEvent};
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use operator_console::recorder::{RecorderConfig, RecorderStatus, RecorderSupervisor};
use operator_console::resource_state::{ResourcePhase, ResourceState};

#[test]
fn resource_state_expires_loading_to_stale_without_dropping_last_good() {
    let mut state = ResourceState::ready(String::from("payload"));
    state.begin_refresh(Instant::now() - Duration::from_secs(60));

    state.expire_if_overdue(Duration::from_secs(5), "timeout");

    assert_eq!(state.phase(), ResourcePhase::Stale);
    assert_eq!(state.last_good().map(String::as_str), Some("payload"));
}

struct StaticProvider {
    snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for StaticProvider {
    fn handle(&mut self, _request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        Ok(self.snapshot.clone())
    }
}

struct RunningSupervisor;

impl RecorderSupervisor for RunningSupervisor {
    fn start(&mut self, _config: &RecorderConfig) -> Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    fn poll_status(&mut self) -> RecorderStatus {
        RecorderStatus::Running
    }
}

struct HangsAfterBootstrapProvider {
    snapshot: ExchangePanelSnapshot,
    calls: Arc<AtomicUsize>,
}

impl ExchangeProvider for HangsAfterBootstrapProvider {
    fn handle(&mut self, _request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            return Ok(self.snapshot.clone());
        }

        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }
}

#[test]
fn stuck_owls_sync_expires_and_keeps_live_context_targets() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut app = test_app_with_live_snapshot(temp_dir.path().join("recorder.json"));
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Live);
    app.set_owls_dashboard_for_test(sample_ready_owls_dashboard());
    app.mark_owls_sync_in_flight_for_test(Instant::now() - Duration::from_secs(60));

    app.poll_owls_dashboard_for_test();

    assert_eq!(app.owls_status_for_test(), "stale");
    assert!(app
        .owls_dashboard()
        .endpoints
        .iter()
        .any(|endpoint| !endpoint.live_scores.is_empty()));
}

#[test]
fn stuck_provider_request_expires_and_allows_next_refresh() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut app = test_app_with_snapshot(temp_dir.path().join("recorder.json"));
    app.mark_provider_in_flight_for_test(Instant::now() - Duration::from_secs(60));

    app.poll_recorder_for_test();

    assert_eq!(app.provider_status_for_test(), "stale");
}

#[test]
fn provider_watchdog_expiry_allows_follow_up_refresh_to_complete() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let calls = Arc::new(AtomicUsize::new(0));
    let provider_calls = Arc::clone(&calls);
    let mut app = App::with_dependencies_and_storage(
        Box::new(HangsAfterBootstrapProvider {
            snapshot: sample_snapshot(),
            calls: provider_calls,
        }),
        Box::new(|| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot(),
            }) as Box<dyn ExchangeProvider + Send>
        }),
        Box::new(|_| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot(),
            }) as Box<dyn ExchangeProvider + Send>
        }),
        Box::new(RunningSupervisor),
        RecorderConfig::default(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.mark_provider_in_flight_for_test(Instant::now() - Duration::from_secs(60));
    app.poll_recorder_for_test();
    assert_eq!(app.provider_status_for_test(), "stale");

    app.refresh().expect("refresh");
    assert!(app.wait_for_async_idle(Duration::from_millis(500)));
    assert_eq!(app.provider_status_for_test(), "ready");
    assert!(calls.load(Ordering::SeqCst) >= 1);
}

fn test_app_with_snapshot(recorder_path: PathBuf) -> App {
    App::with_dependencies_and_storage(
        Box::new(StaticProvider {
            snapshot: sample_snapshot(),
        }),
        Box::new(|| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot(),
            }) as Box<dyn ExchangeProvider + Send>
        }),
        Box::new(|_| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot(),
            }) as Box<dyn ExchangeProvider + Send>
        }),
        Box::new(RunningSupervisor),
        RecorderConfig::default(),
        recorder_path,
        String::from("test"),
    )
    .expect("app")
}

fn test_app_with_live_snapshot(recorder_path: PathBuf) -> App {
    App::with_dependencies_and_storage(
        Box::new(StaticProvider {
            snapshot: sample_live_snapshot(),
        }),
        Box::new(|| {
            Box::new(StaticProvider {
                snapshot: sample_live_snapshot(),
            }) as Box<dyn ExchangeProvider + Send>
        }),
        Box::new(|_| {
            Box::new(StaticProvider {
                snapshot: sample_live_snapshot(),
            }) as Box<dyn ExchangeProvider + Send>
        }),
        Box::new(RunningSupervisor),
        RecorderConfig::default(),
        recorder_path,
        String::from("test"),
    )
    .expect("app")
}

fn sample_snapshot() -> ExchangePanelSnapshot {
    ExchangePanelSnapshot::default()
}

fn sample_live_snapshot() -> ExchangePanelSnapshot {
    let mut snapshot = ExchangePanelSnapshot::default();
    snapshot.open_positions.push(OpenPositionRow {
        event: String::from("Arsenal v Everton"),
        event_status: String::from("Not started"),
        event_url: String::new(),
        contract: String::from("Arsenal"),
        market: String::from("Match Odds"),
        status: String::from("open"),
        market_status: String::from("open"),
        is_in_play: false,
        price: 2.0,
        stake: 10.0,
        liability: 10.0,
        current_value: 0.0,
        pnl_amount: 0.0,
        overall_pnl_known: true,
        current_back_odds: Some(2.0),
        current_implied_probability: Some(0.5),
        current_implied_percentage: Some(50.0),
        current_buy_odds: Some(2.0),
        current_buy_implied_probability: Some(0.5),
        current_sell_odds: None,
        current_sell_implied_probability: None,
        current_score: String::new(),
        current_score_home: None,
        current_score_away: None,
        live_clock: String::new(),
        can_trade_out: true,
    });
    snapshot
}

fn sample_ready_owls_dashboard() -> owls::OwlsDashboard {
    let mut dashboard = owls::dashboard_for_sport("soccer");
    if let Some(endpoint) = dashboard.endpoints.first_mut() {
        endpoint.status = String::from("ready");
        endpoint.live_scores = vec![OwlsLiveScoreEvent {
            sport: String::from("soccer"),
            event_id: String::from("event-1"),
            name: String::from("Arsenal v Everton"),
            home_team: String::from("Arsenal"),
            away_team: String::from("Everton"),
            home_score: Some(1),
            away_score: Some(0),
            status_state: String::from("inplay"),
            status_detail: String::from("45'"),
            display_clock: String::from("45:00"),
            source_match_id: String::from("source-1"),
            last_updated: String::from("2026-03-25T12:00:00Z"),
            stats: Vec::new(),
            incidents: Vec::new(),
            player_ratings: Vec::new(),
        }];
    }
    dashboard
}
