use std::env;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{bail, eyre, Result};

use operator_console::app::{App, TradingSection};
use operator_console::domain::{ExchangePanelSnapshot, VenueId, WorkerStatus, WorkerSummary};
use operator_console::provider::ExchangeProvider;
use operator_console::recorder::{
    default_bet_recorder_command, ProcessRecorderSupervisor, RecorderConfig,
};
use operator_console::transport::WorkerConfig;
use operator_console::worker_client::{BetRecorderWorkerClient, WorkerClientExchangeProvider};

struct StaticProvider {
    snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for StaticProvider {
    fn handle(
        &mut self,
        _request: operator_console::provider::ProviderRequest,
    ) -> Result<ExchangePanelSnapshot> {
        Ok(self.snapshot.clone())
    }
}

#[test]
#[ignore = "requires SABI_OPERATOR_LIVE=1 and a valid live recorder/browser session"]
fn live_operator_smoke_exercises_real_recorder_flow() -> Result<()> {
    if env::var("SABI_OPERATOR_LIVE").as_deref() != Ok("1") {
        bail!("set SABI_OPERATOR_LIVE=1 to run the live operator smoke test");
    }

    let temp_dir = tempfile::tempdir()?;
    let stub_snapshot = stub_snapshot("Live recorder smoke not started.");
    let recorder_config = live_recorder_config(temp_dir.path().join("run"));

    let mut app = App::with_dependencies_and_storage(
        Box::new(StaticProvider {
            snapshot: stub_snapshot.clone(),
        }),
        Box::new({
            let stub_snapshot = stub_snapshot.clone();
            move || {
                Box::new(StaticProvider {
                    snapshot: stub_snapshot.clone(),
                }) as Box<dyn ExchangeProvider>
            }
        }),
        Box::new(|config| {
            Box::new(WorkerClientExchangeProvider::new(
                BetRecorderWorkerClient::new_command(config.command.clone()),
                WorkerConfig {
                    positions_payload_path: None,
                    run_dir: Some(config.run_dir.clone()),
                    account_payload_path: None,
                    open_bets_payload_path: None,
                    companion_legs_path: config.companion_legs_path.clone(),
                    agent_browser_session: Some(config.session.clone()),
                    commission_rate: config.commission_rate.parse::<f64>().unwrap_or(0.0),
                    target_profit: config.target_profit.parse::<f64>().unwrap_or(1.0),
                    stop_loss: config.stop_loss.parse::<f64>().unwrap_or(1.0),
                    hard_margin_call_profit_floor: None,
                    warn_only_default: config.warn_only_default,
                },
            )) as Box<dyn ExchangeProvider>
        }),
        Box::new(ProcessRecorderSupervisor::default()),
        recorder_config,
        temp_dir.path().join("recorder.json"),
        String::from("live smoke"),
    )?;

    let result = (|| -> Result<()> {
        app.start_recorder()?;
        wait_for_first_snapshot(&mut app, live_timeout())?;

        assert_eq!(app.recorder_lifecycle_state(), "running");
        assert_eq!(app.recorder_snapshot_freshness(), "fresh");
        assert!(app.last_successful_snapshot_at().is_some());
        assert_recorder_evidence(&app)?;

        app.refresh()?;
        assert_eq!(app.recorder_snapshot_mode(), "cached");
        assert_recorder_evidence(&app)?;

        app.refresh_live()?;
        assert_eq!(app.recorder_snapshot_mode(), "live");
        assert!(!app.status_message().trim().is_empty());
        assert_recorder_evidence(&app)?;

        app.set_trading_section(TradingSection::Positions);
        select_first_non_smarkets_venue(&mut app)?;
        let selected_venue = app
            .selected_venue()
            .ok_or_else(|| eyre!("live operator smoke lost selected venue"))?;

        app.refresh()?;
        assert_eq!(app.selected_venue(), Some(selected_venue));
        assert!(!app.status_message().trim().is_empty());
        assert_recorder_evidence(&app)?;

        app.refresh_live()?;
        assert_eq!(app.selected_venue(), Some(selected_venue));
        assert!(!app.status_message().trim().is_empty());
        assert_recorder_evidence(&app)?;

        Ok(())
    })();

    let _ = app.stop_recorder();
    result
}

fn live_recorder_config(run_dir: PathBuf) -> RecorderConfig {
    RecorderConfig {
        command: env::var_os("SABI_OPERATOR_LIVE_COMMAND")
            .map(PathBuf::from)
            .unwrap_or_else(default_bet_recorder_command),
        run_dir,
        session: env::var("SABI_OPERATOR_LIVE_SESSION")
            .unwrap_or_else(|_| String::from("helium-copy")),
        companion_legs_path: env::var_os("SABI_OPERATOR_LIVE_COMPANION_LEGS_PATH")
            .map(PathBuf::from),
        profile_path: env::var_os("SABI_OPERATOR_LIVE_PROFILE_PATH").map(PathBuf::from),
        disabled_venues: String::from("bet365"),
        autostart: false,
        interval_seconds: env::var("SABI_OPERATOR_LIVE_INTERVAL_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(2),
        commission_rate: String::from("0"),
        target_profit: String::from("1"),
        stop_loss: String::from("1"),
        hard_margin_call_profit_floor: String::new(),
        warn_only_default: true,
    }
}

fn wait_for_first_snapshot(app: &mut App, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if app.recorder_lifecycle_state() == "running"
            && app.last_successful_snapshot_at().is_some()
        {
            return Ok(());
        }
        let _ = app.refresh();
        thread::sleep(Duration::from_secs(1));
    }

    Err(eyre!(
        "timed out waiting for first live recorder snapshot: {}",
        app.status_message()
    ))
}

fn select_first_non_smarkets_venue(app: &mut App) -> Result<()> {
    let venue_count = app.snapshot().venues.len();
    if venue_count < 2 {
        bail!("live operator smoke expected at least one non-Smarkets venue summary");
    }

    for _ in 0..venue_count {
        app.select_next_exchange_row();
        if app
            .selected_venue()
            .is_some_and(|venue| venue != VenueId::Smarkets)
        {
            return Ok(());
        }
    }

    Err(eyre!(
        "failed to select a non-Smarkets venue from the live snapshot"
    ))
}

fn live_timeout() -> Duration {
    env::var("SABI_OPERATOR_LIVE_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(90))
}

fn assert_recorder_evidence(app: &App) -> Result<()> {
    let bundle = app
        .snapshot()
        .recorder_bundle
        .as_ref()
        .ok_or_else(|| eyre!("live recorder snapshot did not include bundle provenance"))?;
    if bundle.run_dir.trim().is_empty() {
        bail!("live recorder bundle provenance did not include run_dir");
    }
    if bundle.event_count == 0 {
        bail!("live recorder bundle provenance reported zero events");
    }
    if app.snapshot().recorder_events.is_empty() {
        bail!("live recorder snapshot did not include normalized recorder events");
    }
    Ok(())
}

fn stub_snapshot(status_line: &str) -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("operator-console"),
            status: WorkerStatus::Idle,
            detail: String::from(status_line),
        },
        status_line: String::from(status_line),
        ..ExchangePanelSnapshot::empty()
    }
}
