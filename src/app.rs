use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::backend::Backend;
use ratatui::widgets::ListState;
use ratatui::{Frame, Terminal};

use crate::domain::{ExchangePanelSnapshot, VenueId};
use crate::provider::{ExchangeProvider, ProviderRequest};
use crate::recorder::{ProcessRecorderSupervisor, RecorderConfig, RecorderStatus, RecorderSupervisor};
use crate::stub_provider::StubExchangeProvider;
use crate::transport::WorkerConfig;
use crate::ui;
use crate::worker_client::{BetRecorderWorkerClient, WorkerClientExchangeProvider};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Dashboard,
    Exchanges,
    Recorder,
}

type ProviderFactory = dyn Fn(&RecorderConfig) -> Box<dyn ExchangeProvider>;
type StubFactory = dyn Fn() -> Box<dyn ExchangeProvider>;

pub struct App {
    provider: Box<dyn ExchangeProvider>,
    make_stub_provider: Box<StubFactory>,
    make_recorder_provider: Box<ProviderFactory>,
    recorder_supervisor: Box<dyn RecorderSupervisor>,
    recorder_config: RecorderConfig,
    recorder_status: RecorderStatus,
    snapshot: ExchangePanelSnapshot,
    active_panel: Panel,
    exchange_list_state: ListState,
    running: bool,
    status_message: String,
}

impl Default for App {
    fn default() -> Self {
        let stub_factory = || Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider>;
        let provider = stub_factory();
        let recorder_config = RecorderConfig::default();
        let make_recorder_provider = default_recorder_provider_factory();
        Self::with_dependencies(
            provider,
            Box::new(stub_factory),
            make_recorder_provider,
            Box::new(ProcessRecorderSupervisor::default()),
            recorder_config,
        )
        .expect("default stub provider should load dashboard")
    }
}

impl App {
    pub fn from_provider<P: ExchangeProvider + 'static>(provider: P) -> Result<Self> {
        let recorder_config = RecorderConfig::default();
        Self::with_dependencies(
            Box::new(provider),
            Box::new(|| Box::new(StubExchangeProvider::default())),
            default_recorder_provider_factory(),
            Box::new(ProcessRecorderSupervisor::default()),
            recorder_config,
        )
    }

    pub fn with_dependencies(
        mut provider: Box<dyn ExchangeProvider>,
        make_stub_provider: Box<StubFactory>,
        make_recorder_provider: Box<ProviderFactory>,
        recorder_supervisor: Box<dyn RecorderSupervisor>,
        recorder_config: RecorderConfig,
    ) -> Result<Self> {
        let snapshot = provider.handle(ProviderRequest::LoadDashboard)?;
        Ok(Self {
            provider,
            make_stub_provider,
            make_recorder_provider,
            recorder_supervisor,
            recorder_config,
            recorder_status: RecorderStatus::Disabled,
            status_message: snapshot.status_line.clone(),
            snapshot,
            active_panel: Panel::Dashboard,
            exchange_list_state: ListState::default(),
            running: true,
        })
    }

    pub fn snapshot(&self) -> &ExchangePanelSnapshot {
        &self.snapshot
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn active_panel(&self) -> Panel {
        self.active_panel
    }

    pub fn set_active_panel(&mut self, panel: Panel) {
        self.active_panel = panel;
    }

    pub fn help_text(&self) -> &'static str {
        "q quit | tab switch panel | j/k move | r refresh | s start recorder | x stop recorder"
    }

    pub fn selected_exchange_row(&self) -> Option<usize> {
        self.exchange_list_state.selected()
    }

    pub fn exchange_list_state(&mut self) -> &mut ListState {
        &mut self.exchange_list_state
    }

    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub fn recorder_config(&self) -> &RecorderConfig {
        &self.recorder_config
    }

    pub fn recorder_status(&self) -> &RecorderStatus {
        &self.recorder_status
    }

    pub fn refresh(&mut self) -> Result<()> {
        match self.provider.handle(ProviderRequest::Refresh) {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.status_message = self.snapshot.status_line.clone();
                self.clamp_selected_exchange_row();
                Ok(())
            }
            Err(error) => {
                self.record_provider_error(
                    "Refresh failed",
                    &error.to_string(),
                    self.selected_venue(),
                );
                Err(error)
            }
        }
    }

    pub fn start_recorder(&mut self) -> Result<()> {
        self.recorder_supervisor.start(&self.recorder_config)?;
        self.recorder_status = self.recorder_supervisor.poll_status();
        self.provider = (self.make_recorder_provider)(&self.recorder_config);
        self.snapshot = self.provider.handle(ProviderRequest::LoadDashboard)?;
        self.status_message = String::from("Recorder enabled from TUI.");
        self.active_panel = Panel::Exchanges;
        Ok(())
    }

    pub fn stop_recorder(&mut self) -> Result<()> {
        self.recorder_supervisor.stop()?;
        self.recorder_status = RecorderStatus::Disabled;
        self.provider = (self.make_stub_provider)();
        self.snapshot = self.provider.handle(ProviderRequest::LoadDashboard)?;
        self.status_message = String::from("Recorder disabled from TUI.");
        Ok(())
    }

    pub fn next_panel(&mut self) {
        self.active_panel = match self.active_panel {
            Panel::Dashboard => Panel::Exchanges,
            Panel::Exchanges => Panel::Recorder,
            Panel::Recorder => Panel::Dashboard,
        };
    }

    pub fn previous_panel(&mut self) {
        self.active_panel = match self.active_panel {
            Panel::Dashboard => Panel::Recorder,
            Panel::Exchanges => Panel::Dashboard,
            Panel::Recorder => Panel::Exchanges,
        };
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        while self.running {
            self.poll_recorder();
            terminal.draw(|frame| self.render(frame))?;

            if event::poll(Duration::from_millis(250))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key_code(key.code)
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        ui::render(frame, self);
    }

    fn handle_key_code(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char('q') | KeyCode::Esc => self.running = false,
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.next_panel(),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.previous_panel(),
            KeyCode::Char('r') => {
                if let Err(error) = self.refresh() {
                    self.status_message = format!("Refresh failed: {error}");
                }
            }
            KeyCode::Char('s') => {
                if self.active_panel == Panel::Recorder {
                    if let Err(error) = self.start_recorder() {
                        self.status_message = format!("Recorder start failed: {error}");
                        self.recorder_status = RecorderStatus::Error;
                    }
                }
            }
            KeyCode::Char('x') => {
                if self.active_panel == Panel::Recorder {
                    if let Err(error) = self.stop_recorder() {
                        self.status_message = format!("Recorder stop failed: {error}");
                        self.recorder_status = RecorderStatus::Error;
                    }
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.active_panel == Panel::Exchanges {
                    self.select_next_exchange_row();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.active_panel == Panel::Exchanges {
                    self.select_previous_exchange_row();
                }
            }
            _ => {}
        }
    }

    fn clamp_selected_exchange_row(&mut self) {
        match self.exchange_list_state.selected() {
            Some(index) if index >= self.snapshot.venues.len() => {
                self.exchange_list_state.select(None);
            }
            _ => {}
        }
    }

    pub fn select_next_exchange_row(&mut self) {
        if self.snapshot.venues.is_empty() {
            self.exchange_list_state.select(None);
            return;
        }

        let next_index = match self.exchange_list_state.selected() {
            Some(index) if index + 1 < self.snapshot.venues.len() => index + 1,
            Some(index) => index,
            None => 0,
        };

        self.exchange_list_state.select(Some(next_index));
        self.sync_selected_venue();
    }

    pub fn select_previous_exchange_row(&mut self) {
        if self.snapshot.venues.is_empty() {
            self.exchange_list_state.select(None);
            return;
        }

        let previous_index = match self.exchange_list_state.selected() {
            Some(index) if index > 0 => index - 1,
            Some(index) => index,
            None => 0,
        };

        self.exchange_list_state.select(Some(previous_index));
        self.sync_selected_venue();
    }

    fn sync_selected_venue(&mut self) {
        let Some(selected_index) = self.exchange_list_state.selected() else {
            return;
        };
        let Some(venue) = self.snapshot.venues.get(selected_index) else {
            return;
        };

        self.snapshot.selected_venue = Some(venue.id);

        match self.provider.handle(ProviderRequest::SelectVenue(venue.id)) {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.status_message = self.snapshot.status_line.clone();
            }
            Err(error) => {
                self.record_provider_error("Venue sync failed", &error.to_string(), Some(venue.id));
            }
        }
    }

    pub fn selected_venue(&self) -> Option<VenueId> {
        self.exchange_list_state
            .selected()
            .and_then(|index| self.snapshot.venues.get(index).map(|venue| venue.id))
            .or(self.snapshot.selected_venue)
    }

    fn poll_recorder(&mut self) {
        let next_status = self.recorder_supervisor.poll_status();
        if next_status != self.recorder_status {
            self.recorder_status = next_status.clone();
            if matches!(next_status, RecorderStatus::Stopped | RecorderStatus::Error) {
                self.status_message = format!("Recorder status changed to {next_status:?}.");
            }
        }
    }

    fn record_provider_error(
        &mut self,
        context: &str,
        detail: &str,
        selected_venue: Option<VenueId>,
    ) {
        let message = format!("{context}: {detail}");
        self.status_message = message.clone();
        self.snapshot.status_line = message.clone();
        self.snapshot.worker.status = crate::domain::WorkerStatus::Error;
        self.snapshot.worker.detail = message.clone();

        if let Some(venue_id) = selected_venue {
            if let Some(venue) = self
                .snapshot
                .venues
                .iter_mut()
                .find(|venue| venue.id == venue_id)
            {
                venue.status = crate::domain::VenueStatus::Error;
                venue.detail = message;
            }
        }
    }
}

fn default_recorder_provider_factory() -> Box<ProviderFactory> {
    Box::new(|config: &RecorderConfig| {
        Box::new(WorkerClientExchangeProvider::new(
            BetRecorderWorkerClient::new_command(config.command.clone()),
            WorkerConfig {
                positions_payload_path: None,
                run_dir: Some(config.run_dir.clone()),
                account_payload_path: None,
                open_bets_payload_path: None,
                agent_browser_session: None,
                commission_rate: config.commission_rate.parse::<f64>().unwrap_or(0.0),
                target_profit: config.target_profit.parse::<f64>().unwrap_or(1.0),
                stop_loss: config.stop_loss.parse::<f64>().unwrap_or(1.0),
            },
        )) as Box<dyn ExchangeProvider>
    })
}
