use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crossterm::event::KeyCode;
use operator_console::app::{App, Panel, TradingSection};
use operator_console::domain::{ExchangePanelSnapshot, WorkerStatus, WorkerSummary};
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use operator_console::recorder::{
    save_recorder_config, RecorderConfig, RecorderField, RecorderStatus, RecorderSupervisor,
};

struct StaticProvider {
    snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for StaticProvider {
    fn handle(&mut self, _request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot> {
        Ok(self.snapshot.clone())
    }
}

struct FakeSupervisor {
    started: Rc<RefCell<Vec<RecorderConfig>>>,
    stopped: Rc<RefCell<u32>>,
    running: bool,
}

impl RecorderSupervisor for FakeSupervisor {
    fn start(&mut self, config: &RecorderConfig) -> color_eyre::Result<()> {
        self.started.borrow_mut().push(config.clone());
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> color_eyre::Result<()> {
        *self.stopped.borrow_mut() += 1;
        self.running = false;
        Ok(())
    }

    fn poll_status(&mut self) -> RecorderStatus {
        if self.running {
            RecorderStatus::Running
        } else {
            RecorderStatus::Disabled
        }
    }
}

#[test]
fn recorder_start_and_stop_are_controllable_from_app() {
    let started = Rc::new(RefCell::new(Vec::new()));
    let stopped = Rc::new(RefCell::new(0));
    let stub_snapshot = sample_snapshot("Stub dashboard");
    let recorder_snapshot = sample_snapshot("Recorder dashboard");
    let recorder_config = RecorderConfig {
        command: PathBuf::from("/tmp/bet-recorder"),
        run_dir: PathBuf::from("/tmp/sabi-smarkets-watcher"),
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
    };

    let temp_dir = tempfile::tempdir().expect("tempdir");
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
        Box::new({
            let recorder_snapshot = recorder_snapshot.clone();
            move |_| {
                Box::new(StaticProvider {
                    snapshot: recorder_snapshot.clone(),
                }) as Box<dyn ExchangeProvider>
            }
        }),
        Box::new(FakeSupervisor {
            started: started.clone(),
            stopped: stopped.clone(),
            running: false,
        }),
        recorder_config.clone(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Recorder);
    app.start_recorder().expect("start recorder");
    assert_eq!(app.recorder_status(), &RecorderStatus::Running);
    assert_eq!(app.snapshot().status_line, "Recorder dashboard");
    assert_eq!(started.borrow().len(), 1);
    assert_eq!(started.borrow()[0], recorder_config);

    app.stop_recorder().expect("stop recorder");
    assert_eq!(app.recorder_status(), &RecorderStatus::Disabled);
    assert_eq!(app.snapshot().status_line, "Stub dashboard");
    assert_eq!(*stopped.borrow(), 1);
}

#[test]
fn recorder_config_is_editable_before_starting() {
    let started = Rc::new(RefCell::new(Vec::new()));
    let stopped = Rc::new(RefCell::new(0));
    let stub_snapshot = sample_snapshot("Stub dashboard");
    let recorder_snapshot = sample_snapshot("Recorder dashboard");

    let temp_dir = tempfile::tempdir().expect("tempdir");
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
        Box::new({
            let recorder_snapshot = recorder_snapshot.clone();
            move |_| {
                Box::new(StaticProvider {
                    snapshot: recorder_snapshot.clone(),
                }) as Box<dyn ExchangeProvider>
            }
        }),
        Box::new(FakeSupervisor {
            started: started.clone(),
            stopped: stopped.clone(),
            running: false,
        }),
        RecorderConfig::default(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Recorder);
    app.handle_key(KeyCode::Down);
    assert_eq!(app.recorder_selected_field(), RecorderField::RunDir);

    app.handle_key(KeyCode::Enter);
    assert!(app.recorder_is_editing());
    app.handle_key(KeyCode::Char('/'));
    app.handle_key(KeyCode::Char('t'));
    app.handle_key(KeyCode::Char('m'));
    app.handle_key(KeyCode::Char('p'));
    app.handle_key(KeyCode::Char('/'));
    app.handle_key(KeyCode::Char('l'));
    app.handle_key(KeyCode::Char('i'));
    app.handle_key(KeyCode::Char('v'));
    app.handle_key(KeyCode::Char('e'));
    app.handle_key(KeyCode::Enter);

    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    assert_eq!(
        app.recorder_selected_field(),
        RecorderField::IntervalSeconds
    );
    app.handle_key(KeyCode::Enter);
    app.handle_key(KeyCode::Char('1'));
    app.handle_key(KeyCode::Char('0'));
    app.handle_key(KeyCode::Enter);

    assert_eq!(app.recorder_config().run_dir, PathBuf::from("/tmp/live"));
    assert_eq!(app.recorder_config().interval_seconds, 10);

    app.start_recorder().expect("start recorder");
    assert_eq!(started.borrow().len(), 1);
    assert_eq!(started.borrow()[0].run_dir, PathBuf::from("/tmp/live"));
    assert_eq!(started.borrow()[0].interval_seconds, 10);
}

#[test]
fn recorder_config_can_reload_from_disk_and_reset_to_defaults() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config_path = temp_dir.path().join("recorder.json");
    save_recorder_config(
        &config_path,
        &RecorderConfig {
            command: PathBuf::from("/tmp/custom-bet-recorder"),
            run_dir: PathBuf::from("/tmp/saved-run"),
            session: String::from("saved-session"),
            companion_legs_path: Some(PathBuf::from("/tmp/saved-run/companion-legs.json")),
            profile_path: Some(PathBuf::from("/tmp/saved-profile")),
            disabled_venues: String::from("bet365"),
            autostart: true,
            interval_seconds: 9,
            commission_rate: String::from("0"),
            target_profit: String::from("3"),
            stop_loss: String::from("2"),
            hard_margin_call_profit_floor: String::from("5"),
            warn_only_default: false,
        },
    )
    .expect("save seed config");

    let mut app = App::with_dependencies_and_storage(
        Box::new(StaticProvider {
            snapshot: sample_snapshot("Stub dashboard"),
        }),
        Box::new(|| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Stub dashboard"),
            })
        }),
        Box::new(|_| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Recorder dashboard"),
            })
        }),
        Box::new(FakeSupervisor {
            started: Rc::new(RefCell::new(Vec::new())),
            stopped: Rc::new(RefCell::new(0)),
            running: false,
        }),
        RecorderConfig::default(),
        config_path.clone(),
        String::from("test"),
    )
    .expect("app");

    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Recorder);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Enter);
    app.handle_key(KeyCode::Char('/'));
    app.handle_key(KeyCode::Char('t'));
    app.handle_key(KeyCode::Char('m'));
    app.handle_key(KeyCode::Char('p'));
    app.handle_key(KeyCode::Char('/'));
    app.handle_key(KeyCode::Char('e'));
    app.handle_key(KeyCode::Char('d'));
    app.handle_key(KeyCode::Char('i'));
    app.handle_key(KeyCode::Char('t'));
    app.handle_key(KeyCode::Enter);
    assert_eq!(app.recorder_config().run_dir, PathBuf::from("/tmp/edit"));

    save_recorder_config(
        &config_path,
        &RecorderConfig {
            command: PathBuf::from("/tmp/custom-bet-recorder"),
            run_dir: PathBuf::from("/tmp/reloaded-run"),
            session: String::from("reloaded-session"),
            companion_legs_path: Some(PathBuf::from("/tmp/reloaded-run/companion-legs.json")),
            profile_path: Some(PathBuf::from("/tmp/reloaded-profile")),
            disabled_venues: String::from("bet365"),
            autostart: true,
            interval_seconds: 11,
            commission_rate: String::from("0"),
            target_profit: String::from("4"),
            stop_loss: String::from("2"),
            hard_margin_call_profit_floor: String::from("6"),
            warn_only_default: false,
        },
    )
    .expect("overwrite saved config");

    app.handle_key(KeyCode::Char('u'));
    assert_eq!(
        app.recorder_config().run_dir,
        PathBuf::from("/tmp/reloaded-run")
    );
    assert_eq!(app.recorder_config().session, "reloaded-session");
    assert!(app.recorder_config().autostart);
    assert_eq!(app.recorder_config().interval_seconds, 11);

    app.handle_key(KeyCode::Char('D'));
    assert_eq!(app.recorder_config(), &RecorderConfig::default());
}

#[test]
fn recorder_config_can_cycle_field_suggestions() {
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
        Box::new(|_| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Recorder dashboard"),
            }) as Box<dyn ExchangeProvider>
        }),
        Box::new(FakeSupervisor {
            started: Rc::new(RefCell::new(Vec::new())),
            stopped: Rc::new(RefCell::new(0)),
            running: false,
        }),
        RecorderConfig::default(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Recorder);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    assert_eq!(
        app.recorder_selected_field(),
        RecorderField::IntervalSeconds
    );

    app.handle_key(KeyCode::Char(']'));
    assert_eq!(app.recorder_config().interval_seconds, 10);

    app.handle_key(KeyCode::Char(']'));
    assert_eq!(app.recorder_config().interval_seconds, 15);

    app.handle_key(KeyCode::Char('['));
    assert_eq!(app.recorder_config().interval_seconds, 10);
}

#[test]
fn recorder_config_edit_restarts_running_recorder() {
    let started = Rc::new(RefCell::new(Vec::new()));
    let stopped = Rc::new(RefCell::new(0));
    let stub_snapshot = sample_snapshot("Stub dashboard");
    let recorder_snapshot = sample_snapshot("Recorder dashboard");

    let temp_dir = tempfile::tempdir().expect("tempdir");
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
        Box::new({
            let recorder_snapshot = recorder_snapshot.clone();
            move |_| {
                Box::new(StaticProvider {
                    snapshot: recorder_snapshot.clone(),
                }) as Box<dyn ExchangeProvider>
            }
        }),
        Box::new(FakeSupervisor {
            started: started.clone(),
            stopped: stopped.clone(),
            running: false,
        }),
        RecorderConfig::default(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Recorder);
    app.start_recorder().expect("start recorder");
    assert_eq!(started.borrow().len(), 1);

    app.set_trading_section(TradingSection::Recorder);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    assert_eq!(
        app.recorder_selected_field(),
        RecorderField::IntervalSeconds
    );
    app.handle_key(KeyCode::Enter);
    app.handle_key(KeyCode::Char('1'));
    app.handle_key(KeyCode::Char('0'));
    app.handle_key(KeyCode::Enter);

    assert_eq!(app.recorder_config().interval_seconds, 10);
    assert_eq!(started.borrow().len(), 2);
    assert_eq!(started.borrow()[1].interval_seconds, 10);
    assert_eq!(*stopped.borrow(), 1);
    assert_eq!(app.recorder_status(), &RecorderStatus::Running);
    assert!(app.status_message().contains("Restarted recorder"));
}

#[test]
fn recorder_provider_swap_clamps_stale_exchange_selection() {
    let started = Rc::new(RefCell::new(Vec::new()));
    let stopped = Rc::new(RefCell::new(0));
    let stub_snapshot = snapshot_with_venues("Stub dashboard", 4);
    let recorder_snapshot = snapshot_with_venues("Recorder dashboard", 1);

    let temp_dir = tempfile::tempdir().expect("tempdir");
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
        Box::new({
            let recorder_snapshot = recorder_snapshot.clone();
            move |_| {
                Box::new(StaticProvider {
                    snapshot: recorder_snapshot.clone(),
                }) as Box<dyn ExchangeProvider>
            }
        }),
        Box::new(FakeSupervisor {
            started: started.clone(),
            stopped: stopped.clone(),
            running: false,
        }),
        RecorderConfig::default(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Positions);
    app.select_next_exchange_row();
    app.select_next_exchange_row();
    app.select_next_exchange_row();
    app.select_next_exchange_row();
    assert_eq!(app.selected_exchange_row(), Some(3));

    app.set_trading_section(TradingSection::Recorder);
    app.start_recorder().expect("start recorder");

    assert_eq!(app.selected_exchange_row(), None);

    app.set_trading_section(TradingSection::Positions);
    app.select_next_exchange_row();
    assert_eq!(app.selected_exchange_row(), Some(0));
}

#[test]
fn recorder_start_is_global_and_switches_into_trading_positions() {
    let started = Rc::new(RefCell::new(Vec::new()));
    let stopped = Rc::new(RefCell::new(0));
    let stub_snapshot = sample_snapshot("Stub dashboard");
    let recorder_snapshot = sample_snapshot("Recorder dashboard");

    let temp_dir = tempfile::tempdir().expect("tempdir");
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
        Box::new({
            let recorder_snapshot = recorder_snapshot.clone();
            move |_| {
                Box::new(StaticProvider {
                    snapshot: recorder_snapshot.clone(),
                }) as Box<dyn ExchangeProvider>
            }
        }),
        Box::new(FakeSupervisor {
            started: started.clone(),
            stopped: stopped.clone(),
            running: false,
        }),
        RecorderConfig::default(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.set_active_panel(Panel::Observability);
    app.handle_key(KeyCode::Char('s'));

    assert_eq!(app.recorder_status(), &RecorderStatus::Running);
    assert_eq!(app.active_panel(), Panel::Trading);
    assert_eq!(app.active_trading_section(), TradingSection::Positions);
    assert_eq!(started.borrow().len(), 1);
}

#[test]
fn quitting_app_stops_a_running_recorder() {
    let started = Rc::new(RefCell::new(Vec::new()));
    let stopped = Rc::new(RefCell::new(0));
    let stub_snapshot = sample_snapshot("Stub dashboard");
    let recorder_snapshot = sample_snapshot("Recorder dashboard");

    let temp_dir = tempfile::tempdir().expect("tempdir");
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
        Box::new({
            let recorder_snapshot = recorder_snapshot.clone();
            move |_| {
                Box::new(StaticProvider {
                    snapshot: recorder_snapshot.clone(),
                }) as Box<dyn ExchangeProvider>
            }
        }),
        Box::new(FakeSupervisor {
            started: started.clone(),
            stopped: stopped.clone(),
            running: false,
        }),
        RecorderConfig::default(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.start_recorder().expect("start recorder");
    assert_eq!(app.recorder_status(), &RecorderStatus::Running);

    app.handle_key(KeyCode::Char('q'));

    assert_eq!(*stopped.borrow(), 1);
    assert_eq!(app.recorder_status(), &RecorderStatus::Disabled);
    assert!(!app.is_running());
}

#[test]
fn recorder_autostart_field_is_editable_and_persisted() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config_path = temp_dir.path().join("recorder.json");
    let mut app = App::with_dependencies_and_storage(
        Box::new(StaticProvider {
            snapshot: sample_snapshot("Stub dashboard"),
        }),
        Box::new(|| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Stub dashboard"),
            }) as Box<dyn ExchangeProvider>
        }),
        Box::new(|_| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Recorder dashboard"),
            }) as Box<dyn ExchangeProvider>
        }),
        Box::new(FakeSupervisor {
            started: Rc::new(RefCell::new(Vec::new())),
            stopped: Rc::new(RefCell::new(0)),
            running: false,
        }),
        RecorderConfig::default(),
        config_path.clone(),
        String::from("test"),
    )
    .expect("app");

    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Recorder);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Down);
    assert_eq!(app.recorder_selected_field(), RecorderField::Autostart);

    app.handle_key(KeyCode::Char(']'));

    assert!(app.recorder_config().autostart);

    let (loaded, _) = operator_console::recorder::load_recorder_config_or_default(&config_path)
        .expect("load config");
    assert!(loaded.autostart);
}

#[test]
fn recorder_can_autostart_from_saved_config() {
    let started = Rc::new(RefCell::new(Vec::new()));
    let stopped = Rc::new(RefCell::new(0));
    let stub_snapshot = sample_snapshot("Stub dashboard");
    let recorder_snapshot = sample_snapshot("Recorder dashboard");

    let temp_dir = tempfile::tempdir().expect("tempdir");
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
        Box::new({
            let recorder_snapshot = recorder_snapshot.clone();
            move |_| {
                Box::new(StaticProvider {
                    snapshot: recorder_snapshot.clone(),
                }) as Box<dyn ExchangeProvider>
            }
        }),
        Box::new(FakeSupervisor {
            started: started.clone(),
            stopped: stopped.clone(),
            running: false,
        }),
        RecorderConfig {
            autostart: true,
            ..RecorderConfig::default()
        },
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.autostart_recorder_if_enabled()
        .expect("autostart recorder");

    assert_eq!(app.recorder_status(), &RecorderStatus::Running);
    assert_eq!(app.snapshot().status_line, "Recorder dashboard");
    assert_eq!(app.active_panel(), Panel::Trading);
    assert_eq!(app.active_trading_section(), TradingSection::Positions);
    assert_eq!(started.borrow().len(), 1);
    assert_eq!(*stopped.borrow(), 0);
}

#[test]
fn recorder_reload_outside_recorder_panel_reports_guidance() {
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
        Box::new(|_| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Recorder dashboard"),
            }) as Box<dyn ExchangeProvider>
        }),
        Box::new(FakeSupervisor {
            started: Rc::new(RefCell::new(Vec::new())),
            stopped: Rc::new(RefCell::new(0)),
            running: false,
        }),
        RecorderConfig::default(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
    )
    .expect("app");

    app.set_active_panel(Panel::Observability);
    app.handle_key(KeyCode::Char('u'));

    assert!(app
        .status_message()
        .contains("Open Trading > Recorder to reload recorder config."));
}

fn sample_snapshot(status_line: &str) -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("provider"),
            status: WorkerStatus::Ready,
            detail: String::from("ok"),
        },
        status_line: String::from(status_line),
        ..ExchangePanelSnapshot::empty()
    }
}

fn snapshot_with_venues(status_line: &str, count: usize) -> ExchangePanelSnapshot {
    let mut snapshot = sample_snapshot(status_line);
    snapshot.venues = (0..count)
        .map(|index| operator_console::domain::VenueSummary {
            id: match index {
                0 => operator_console::domain::VenueId::Smarkets,
                1 => operator_console::domain::VenueId::Betfair,
                2 => operator_console::domain::VenueId::Matchbook,
                _ => operator_console::domain::VenueId::Betdaq,
            },
            label: format!("Venue {index}"),
            status: operator_console::domain::VenueStatus::Connected,
            detail: String::from("ready"),
            event_count: 1,
            market_count: 1,
        })
        .collect();
    snapshot.selected_venue = snapshot.venues.first().map(|venue| venue.id);
    snapshot
}
