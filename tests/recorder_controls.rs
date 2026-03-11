use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use operator_console::app::{App, Panel};
use operator_console::domain::{ExchangePanelSnapshot, WorkerStatus, WorkerSummary};
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
        interval_seconds: 5,
        commission_rate: String::from("0"),
        target_profit: String::from("1"),
        stop_loss: String::from("1"),
    };

    let mut app = App::with_dependencies(
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
    )
    .expect("app");

    app.set_active_panel(Panel::Recorder);
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
