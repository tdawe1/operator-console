use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use color_eyre::eyre::eyre;
use operator_console::app::App;
use operator_console::domain::{
    ExchangePanelSnapshot, VenueId, VenueStatus, VenueSummary, WorkerStatus, WorkerSummary,
};
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use operator_console::recorder::{RecorderConfig, RecorderStatus, RecorderSupervisor};

struct StaticProvider {
    snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for StaticProvider {
    fn handle(&mut self, _request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot> {
        Ok(self.snapshot.clone())
    }
}

struct FlakyLoadProvider {
    remaining_failures: Rc<RefCell<usize>>,
    load_attempts: Rc<RefCell<usize>>,
    snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for FlakyLoadProvider {
    fn handle(&mut self, request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot> {
        match request {
            ProviderRequest::LoadDashboard => {
                *self.load_attempts.borrow_mut() += 1;
                let mut remaining = self.remaining_failures.borrow_mut();
                if *remaining > 0 {
                    *remaining -= 1;
                    return Err(eyre!("No positions_snapshot event found in run bundle"));
                }
                Ok(self.snapshot.clone())
            }
            ProviderRequest::RefreshCached
            | ProviderRequest::RefreshLive
            | ProviderRequest::SelectVenue(_)
            | ProviderRequest::CashOutTrackedBet { .. }
            | ProviderRequest::ExecuteTradingAction { .. }
            | ProviderRequest::LoadHorseMatcher { .. } => Ok(self.snapshot.clone()),
        }
    }
}

struct RunningSupervisor;

impl RecorderSupervisor for RunningSupervisor {
    fn start(&mut self, _config: &RecorderConfig) -> color_eyre::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> color_eyre::Result<()> {
        Ok(())
    }

    fn poll_status(&mut self) -> RecorderStatus {
        RecorderStatus::Running
    }
}

#[test]
fn start_recorder_returns_immediately_when_first_snapshot_is_not_ready() {
    let load_attempts = Rc::new(RefCell::new(0));
    let remaining_failures = Rc::new(RefCell::new(2));
    let temp_dir = tempfile::tempdir().expect("tempdir");

    let mut app = App::with_dependencies_and_storage(
        Box::new(StaticProvider {
            snapshot: sample_snapshot("Stub dashboard"),
        }),
        Box::new(|| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Stub dashboard"),
            }) as Box<dyn ExchangeProvider>
        }),
        Box::new({
            let load_attempts = load_attempts.clone();
            let remaining_failures = remaining_failures.clone();
            move |_| {
                Box::new(FlakyLoadProvider {
                    remaining_failures: remaining_failures.clone(),
                    load_attempts: load_attempts.clone(),
                    snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider>
            }
        }),
        Box::new(RunningSupervisor),
        RecorderConfig {
            command: PathBuf::from("/tmp/bet-recorder"),
            run_dir: temp_dir.path().join("run"),
            session: String::from("helium-copy"),
            companion_legs_path: None,
            profile_path: Some(PathBuf::from("/tmp/owned-profile")),
            disabled_venues: String::from("bet365"),
            autostart: false,
            interval_seconds: 5,
            commission_rate: String::from("0"),
            target_profit: String::from("1"),
            stop_loss: String::from("1"),
            hard_margin_call_profit_floor: String::new(),
            warn_only_default: true,
        },
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.start_recorder().expect("start recorder");

    assert!(app.status_message().contains("waiting for first snapshot"));
    assert_eq!(app.recorder_lifecycle_state(), "waiting");
    assert_eq!(app.last_successful_snapshot_at(), None);
    assert_eq!(*load_attempts.borrow(), 1);

    app.refresh().expect("refresh");

    assert_eq!(*load_attempts.borrow(), 1);
    assert_eq!(app.snapshot().status_line, "Recorder dashboard");
    assert_eq!(app.recorder_lifecycle_state(), "running");
}

fn sample_snapshot(status_line: &str) -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("bet-recorder"),
            status: WorkerStatus::Ready,
            detail: String::from("ready"),
        },
        venues: vec![VenueSummary {
            id: VenueId::Smarkets,
            label: String::from("Smarkets"),
            status: VenueStatus::Connected,
            detail: String::from("ready"),
            event_count: 1,
            market_count: 1,
        }],
        selected_venue: Some(VenueId::Smarkets),
        events: Vec::new(),
        markets: Vec::new(),
        preflight: None,
        status_line: String::from(status_line),
        runtime: None,
        account_stats: None,
        open_positions: Vec::new(),
        historical_positions: Vec::new(),
        ledger_pnl_summary: Default::default(),
        other_open_bets: Vec::new(),
        decisions: Vec::new(),
        watch: None,
        recorder_bundle: None,
        recorder_events: Vec::new(),
        transport_summary: None,
        transport_events: Vec::new(),
        tracked_bets: Vec::new(),
        exit_policy: Default::default(),
        exit_recommendations: Vec::new(),
        horse_matcher: None,
    }
}
