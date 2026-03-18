use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use reqwest::blocking::Client;
use ratatui::backend::Backend;
use ratatui::widgets::{ListState, TableState};
use ratatui::{Frame, Terminal};

use crate::calculator::{self, BetType, Input as CalculatorInput, Mode as CalculatorMode};
use crate::domain::{ExchangePanelSnapshot, VenueId};
use crate::oddsmatcher::{
    self, GetBestMatchesVariables, OddsMatcherEditorState, OddsMatcherField, OddsMatcherRow,
};
use crate::provider::{ExchangeProvider, ProviderRequest};
use crate::recorder::{
    default_config_path, load_recorder_config_or_default, save_recorder_config,
    ProcessRecorderSupervisor, RecorderConfig, RecorderEditorState, RecorderField, RecorderStatus,
    RecorderSupervisor,
};
use crate::stub_provider::StubExchangeProvider;
use crate::transport::WorkerConfig;
use crate::ui;
use crate::worker_client::{BetRecorderWorkerClient, WorkerClientExchangeProvider};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Trading,
    Observability,
}

impl Panel {
    pub const ALL: [Self; 2] = [Self::Trading, Self::Observability];

    pub fn label(self) -> &'static str {
        match self {
            Self::Trading => "Trading",
            Self::Observability => "Observability",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradingSection {
    Accounts,
    Positions,
    Markets,
    OddsMatcher,
    Stats,
    Calculator,
    Recorder,
}

impl TradingSection {
    pub const ALL: [Self; 7] = [
        Self::Accounts,
        Self::Positions,
        Self::Markets,
        Self::OddsMatcher,
        Self::Stats,
        Self::Calculator,
        Self::Recorder,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Accounts => "Accounts",
            Self::Positions => "Positions",
            Self::Markets => "Markets",
            Self::OddsMatcher => "OddsMatcher",
            Self::Stats => "Stats",
            Self::Calculator => "Calculator",
            Self::Recorder => "Recorder",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservabilitySection {
    Workers,
    Watchers,
    Configs,
    Logs,
    Health,
}

impl ObservabilitySection {
    pub const ALL: [Self; 5] = [
        Self::Workers,
        Self::Watchers,
        Self::Configs,
        Self::Logs,
        Self::Health,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Workers => "Workers",
            Self::Watchers => "Watchers",
            Self::Configs => "Configs",
            Self::Logs => "Logs",
            Self::Health => "Health",
        }
    }
}

type ProviderFactory = dyn Fn(&RecorderConfig) -> Box<dyn ExchangeProvider>;
type StubFactory = dyn Fn() -> Box<dyn ExchangeProvider>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculatorField {
    BackStake,
    BackOdds,
    LayOdds,
    BackCommission,
    LayCommission,
    RiskFreeAward,
    RiskFreeRetention,
    PartLayStakeOne,
    PartLayOddsOne,
    PartLayStakeTwo,
    PartLayOddsTwo,
}

impl CalculatorField {
    pub const ALL: [Self; 11] = [
        Self::BackStake,
        Self::BackOdds,
        Self::LayOdds,
        Self::BackCommission,
        Self::LayCommission,
        Self::RiskFreeAward,
        Self::RiskFreeRetention,
        Self::PartLayStakeOne,
        Self::PartLayOddsOne,
        Self::PartLayStakeTwo,
        Self::PartLayOddsTwo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::BackStake => "Back Stake",
            Self::BackOdds => "Back Odds",
            Self::LayOdds => "Lay Odds",
            Self::BackCommission => "Bookie Comm %",
            Self::LayCommission => "Lay Comm %",
            Self::RiskFreeAward => "Risk-Free Award",
            Self::RiskFreeRetention => "Retention %",
            Self::PartLayStakeOne => "Part Lay 1 Stake",
            Self::PartLayOddsOne => "Part Lay 1 Odds",
            Self::PartLayStakeTwo => "Part Lay 2 Stake",
            Self::PartLayOddsTwo => "Part Lay 2 Odds",
        }
    }

    fn display_value(self, state: &CalculatorState) -> String {
        match self {
            Self::BackStake => format!("{:.2}", state.input.back_stake),
            Self::BackOdds => format!("{:.2}", state.input.back_odds),
            Self::LayOdds => format!("{:.2}", state.input.lay_odds),
            Self::BackCommission => format!("{:.2}", state.input.back_commission_pct),
            Self::LayCommission => format!("{:.2}", state.input.lay_commission_pct),
            Self::RiskFreeAward => format!("{:.2}", state.input.risk_free_award),
            Self::RiskFreeRetention => format!("{:.2}", state.input.risk_free_retention_pct),
            Self::PartLayStakeOne => format!("{:.2}", state.input.part_lays[0].stake),
            Self::PartLayOddsOne => format!("{:.2}", state.input.part_lays[0].odds),
            Self::PartLayStakeTwo => format!("{:.2}", state.input.part_lays[1].stake),
            Self::PartLayOddsTwo => format!("{:.2}", state.input.part_lays[1].odds),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CalculatorEditorState {
    selected_field: CalculatorField,
    editing: bool,
    buffer: String,
    replace_on_input: bool,
}

impl Default for CalculatorEditorState {
    fn default() -> Self {
        Self {
            selected_field: CalculatorField::BackStake,
            editing: false,
            buffer: String::new(),
            replace_on_input: false,
        }
    }
}

impl CalculatorEditorState {
    fn selected_field(&self) -> CalculatorField {
        self.selected_field
    }

    fn select_next_field(&mut self) {
        self.selected_field = next_from(self.selected_field, &CalculatorField::ALL);
    }

    fn select_previous_field(&mut self) {
        self.selected_field = previous_from(self.selected_field, &CalculatorField::ALL);
    }
}

#[derive(Debug, Clone)]
pub struct CalculatorState {
    input: CalculatorInput,
    editor: CalculatorEditorState,
    source: Option<CalculatorSourceContext>,
}

impl Default for CalculatorState {
    fn default() -> Self {
        Self {
            input: CalculatorInput::default(),
            editor: CalculatorEditorState::default(),
            source: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CalculatorSourceContext {
    pub event_name: String,
    pub selection_name: String,
    pub competition_name: String,
    pub rating: f64,
    pub bookmaker_name: String,
    pub exchange_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OddsMatcherFocus {
    Filters,
    Results,
}

pub struct App {
    provider: Box<dyn ExchangeProvider>,
    make_stub_provider: Box<StubFactory>,
    make_recorder_provider: Box<ProviderFactory>,
    recorder_supervisor: Box<dyn RecorderSupervisor>,
    recorder_config: RecorderConfig,
    recorder_config_path: std::path::PathBuf,
    recorder_config_note: String,
    recorder_editor: RecorderEditorState,
    recorder_status: RecorderStatus,
    calculator: CalculatorState,
    oddsmatcher_client: Client,
    oddsmatcher_query_path: PathBuf,
    oddsmatcher_query_note: String,
    oddsmatcher_query: GetBestMatchesVariables,
    oddsmatcher_editor: OddsMatcherEditorState,
    oddsmatcher_focus: OddsMatcherFocus,
    oddsmatcher_rows: Vec<OddsMatcherRow>,
    oddsmatcher_table_state: TableState,
    snapshot: ExchangePanelSnapshot,
    active_panel: Panel,
    trading_section: TradingSection,
    observability_section: ObservabilitySection,
    exchange_list_state: ListState,
    open_position_table_state: TableState,
    last_recorder_refresh_at: Option<Instant>,
    running: bool,
    status_message: String,
}

impl Default for App {
    fn default() -> Self {
        let stub_factory =
            || Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider>;
        let provider = stub_factory();
        let recorder_config_path = default_config_path();
        let (recorder_config, recorder_config_note) =
            load_recorder_config_or_default(&recorder_config_path).unwrap_or_else(|error| {
                (
                    RecorderConfig::default(),
                    format!("Recorder config load failed; using defaults: {error}"),
                )
            });
        let make_recorder_provider = default_recorder_provider_factory();
        Self::with_dependencies_and_storage(
            provider,
            Box::new(stub_factory),
            make_recorder_provider,
            Box::new(ProcessRecorderSupervisor::default()),
            recorder_config,
            recorder_config_path,
            recorder_config_note,
        )
        .expect("default stub provider should load dashboard")
    }
}

impl App {
    pub fn from_provider<P: ExchangeProvider + 'static>(provider: P) -> Result<Self> {
        let recorder_config_path = default_config_path();
        let (recorder_config, recorder_config_note) =
            load_recorder_config_or_default(&recorder_config_path).unwrap_or_else(|error| {
                (
                    RecorderConfig::default(),
                    format!("Recorder config load failed; using defaults: {error}"),
                )
            });
        Self::with_dependencies_and_storage(
            Box::new(provider),
            Box::new(|| Box::new(StubExchangeProvider::default())),
            default_recorder_provider_factory(),
            Box::new(ProcessRecorderSupervisor::default()),
            recorder_config,
            recorder_config_path,
            recorder_config_note,
        )
    }

    pub fn with_dependencies(
        provider: Box<dyn ExchangeProvider>,
        make_stub_provider: Box<StubFactory>,
        make_recorder_provider: Box<ProviderFactory>,
        recorder_supervisor: Box<dyn RecorderSupervisor>,
        recorder_config: RecorderConfig,
    ) -> Result<Self> {
        Self::with_dependencies_and_storage(
            provider,
            make_stub_provider,
            make_recorder_provider,
            recorder_supervisor,
            recorder_config,
            default_config_path(),
            String::from("Using in-memory recorder config."),
        )
    }

    pub fn with_dependencies_and_storage(
        provider: Box<dyn ExchangeProvider>,
        make_stub_provider: Box<StubFactory>,
        make_recorder_provider: Box<ProviderFactory>,
        recorder_supervisor: Box<dyn RecorderSupervisor>,
        recorder_config: RecorderConfig,
        recorder_config_path: std::path::PathBuf,
        recorder_config_note: String,
    ) -> Result<Self> {
        Self::with_dependencies_and_storage_paths(
            provider,
            make_stub_provider,
            make_recorder_provider,
            recorder_supervisor,
            recorder_config,
            recorder_config_path,
            recorder_config_note,
            oddsmatcher::default_query_path(),
        )
    }

    pub fn with_dependencies_and_storage_paths(
        mut provider: Box<dyn ExchangeProvider>,
        make_stub_provider: Box<StubFactory>,
        make_recorder_provider: Box<ProviderFactory>,
        recorder_supervisor: Box<dyn RecorderSupervisor>,
        recorder_config: RecorderConfig,
        recorder_config_path: std::path::PathBuf,
        recorder_config_note: String,
        oddsmatcher_query_path: PathBuf,
    ) -> Result<Self> {
        let snapshot = provider.handle(ProviderRequest::LoadDashboard)?;
        let status_message = snapshot.status_line.clone();
        let (oddsmatcher_query, oddsmatcher_query_note) =
            oddsmatcher::load_query_or_default(&oddsmatcher_query_path).unwrap_or_else(|error| {
                (
                    GetBestMatchesVariables::default(),
                    format!("OddsMatcher config load failed; using defaults: {error}"),
                )
            });
        let mut open_position_table_state = TableState::default();
        if !snapshot.open_positions.is_empty() {
            open_position_table_state.select(Some(0));
        }

        Ok(Self {
            provider,
            make_stub_provider,
            make_recorder_provider,
            recorder_supervisor,
            recorder_config,
            recorder_config_path,
            recorder_config_note,
            recorder_editor: RecorderEditorState::default(),
            recorder_status: RecorderStatus::Disabled,
            calculator: CalculatorState::default(),
            oddsmatcher_client: Client::new(),
            oddsmatcher_query_path,
            oddsmatcher_query_note,
            oddsmatcher_query,
            oddsmatcher_editor: OddsMatcherEditorState::default(),
            oddsmatcher_focus: OddsMatcherFocus::Results,
            oddsmatcher_rows: Vec::new(),
            oddsmatcher_table_state: TableState::default(),
            snapshot,
            active_panel: Panel::Trading,
            trading_section: TradingSection::Accounts,
            observability_section: ObservabilitySection::Workers,
            exchange_list_state: ListState::default(),
            open_position_table_state,
            last_recorder_refresh_at: None,
            running: true,
            status_message,
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

    pub fn active_trading_section(&self) -> TradingSection {
        self.trading_section
    }

    pub fn set_trading_section(&mut self, section: TradingSection) {
        self.trading_section = section;
    }

    pub fn active_observability_section(&self) -> ObservabilitySection {
        self.observability_section
    }

    pub fn help_text(&self) -> &'static str {
        "q quit | o observability | h/l sections | arrows nav | r refresh\nenter edit | esc cancel | [/] cycle suggestions | u reload | D defaults\ns start recorder | x stop recorder | c cash out | b cycle type | m toggle mode"
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

    pub fn selected_open_position_row(&self) -> Option<usize> {
        self.open_position_table_state.selected()
    }

    pub fn open_position_table_state(&mut self) -> &mut TableState {
        &mut self.open_position_table_state
    }

    pub fn oddsmatcher_rows(&self) -> &[OddsMatcherRow] {
        &self.oddsmatcher_rows
    }

    pub fn oddsmatcher_query(&self) -> &GetBestMatchesVariables {
        &self.oddsmatcher_query
    }

    pub fn oddsmatcher_query_note(&self) -> &str {
        &self.oddsmatcher_query_note
    }

    pub fn oddsmatcher_selected_field(&self) -> OddsMatcherField {
        self.oddsmatcher_editor.selected_field()
    }

    pub fn oddsmatcher_focus(&self) -> OddsMatcherFocus {
        self.oddsmatcher_focus
    }

    pub fn oddsmatcher_is_editing(&self) -> bool {
        self.oddsmatcher_editor.editing
    }

    pub fn oddsmatcher_edit_buffer(&self) -> Option<&str> {
        self.oddsmatcher_editor
            .editing
            .then_some(self.oddsmatcher_editor.buffer.as_str())
    }

    pub fn oddsmatcher_field_rows(&self) -> Vec<(OddsMatcherField, String, bool)> {
        OddsMatcherField::ALL
            .into_iter()
            .map(|field| {
                let value = if self.oddsmatcher_editor.editing
                    && self.oddsmatcher_editor.selected_field() == field
                {
                    self.oddsmatcher_editor.buffer.clone()
                } else {
                    field.display_value(&self.oddsmatcher_query)
                };
                (field, value, self.oddsmatcher_editor.selected_field() == field)
            })
            .collect()
    }

    pub fn selected_oddsmatcher_row(&self) -> Option<&OddsMatcherRow> {
        self.oddsmatcher_table_state
            .selected()
            .and_then(|index| self.oddsmatcher_rows.get(index))
    }

    pub fn oddsmatcher_table_state(&mut self) -> &mut TableState {
        &mut self.oddsmatcher_table_state
    }

    pub fn recorder_config(&self) -> &RecorderConfig {
        &self.recorder_config
    }

    pub fn recorder_status(&self) -> &RecorderStatus {
        &self.recorder_status
    }

    pub fn recorder_config_path(&self) -> &Path {
        &self.recorder_config_path
    }

    pub fn recorder_config_note(&self) -> &str {
        &self.recorder_config_note
    }

    pub fn recorder_selected_field(&self) -> RecorderField {
        self.recorder_editor.selected_field()
    }

    pub fn recorder_is_editing(&self) -> bool {
        self.recorder_editor.editing
    }

    pub fn recorder_edit_buffer(&self) -> Option<&str> {
        self.recorder_editor
            .editing
            .then_some(self.recorder_editor.buffer.as_str())
    }

    pub fn calculator_state(&self) -> &CalculatorState {
        &self.calculator
    }

    pub fn calculator_source(&self) -> Option<&CalculatorSourceContext> {
        self.calculator.source.as_ref()
    }

    pub fn calculator_selected_field(&self) -> CalculatorField {
        self.calculator.editor.selected_field()
    }

    pub fn calculator_is_editing(&self) -> bool {
        self.calculator.editor.editing
    }

    pub fn calculator_edit_buffer(&self) -> Option<&str> {
        self.calculator
            .editor
            .editing
            .then_some(self.calculator.editor.buffer.as_str())
    }

    pub fn calculator_output(&self) -> Result<calculator::Output, String> {
        calculator::calculate(&self.calculator.input)
    }

    pub fn calculator_bet_type(&self) -> BetType {
        self.calculator.input.bet_type
    }

    pub fn calculator_mode(&self) -> CalculatorMode {
        self.calculator.input.mode
    }

    pub fn calculator_back_odds(&self) -> f64 {
        self.calculator.input.back_odds
    }

    pub fn calculator_lay_odds(&self) -> f64 {
        self.calculator.input.lay_odds
    }

    pub fn calculator_field_rows(&self) -> Vec<(CalculatorField, String, bool)> {
        CalculatorField::ALL
            .into_iter()
            .map(|field| {
                let value = if self.calculator.editor.editing
                    && self.calculator.editor.selected_field() == field
                {
                    self.calculator.editor.buffer.clone()
                } else {
                    field.display_value(&self.calculator)
                };
                (
                    field,
                    value,
                    self.calculator.editor.selected_field() == field,
                )
            })
            .collect()
    }

    pub fn refresh(&mut self) -> Result<()> {
        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::OddsMatcher
        {
            return self.refresh_oddsmatcher();
        }
        self.refresh_provider_snapshot()
    }

    pub fn replace_oddsmatcher_rows(&mut self, rows: Vec<OddsMatcherRow>, status_message: String) {
        self.oddsmatcher_rows = rows;
        self.clamp_selected_oddsmatcher_row();
        self.status_message = status_message;
    }

    fn refresh_provider_snapshot(&mut self) -> Result<()> {
        match self.provider.handle(ProviderRequest::Refresh) {
            Ok(snapshot) => {
                self.replace_snapshot(snapshot);
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

    pub fn cash_out_next_actionable_bet(&mut self) -> Result<()> {
        let actionable_bet_id = self
            .snapshot
            .exit_recommendations
            .iter()
            .find(|recommendation| recommendation.action == "cash_out")
            .map(|recommendation| recommendation.bet_id.clone())
            .ok_or_else(|| {
                color_eyre::eyre::eyre!("No tracked bet is currently marked for cash out.")
            })?;
        let snapshot = self.provider.handle(ProviderRequest::CashOutTrackedBet {
            bet_id: actionable_bet_id,
        })?;
        self.replace_snapshot(snapshot);
        Ok(())
    }

    pub fn start_recorder(&mut self) -> Result<()> {
        self.persist_recorder_config()?;
        self.recorder_supervisor.start(&self.recorder_config)?;
        self.recorder_status = self.recorder_supervisor.poll_status();
        self.provider = (self.make_recorder_provider)(&self.recorder_config);
        let snapshot = self.load_recorder_dashboard_with_retry()?;
        self.replace_snapshot(snapshot);
        self.last_recorder_refresh_at = Some(Instant::now());
        self.active_panel = Panel::Trading;
        self.trading_section = TradingSection::Positions;
        Ok(())
    }

    pub fn autostart_recorder_if_enabled(&mut self) -> Result<()> {
        if self.recorder_config.autostart && self.recorder_status != RecorderStatus::Running {
            self.start_recorder()?;
        }
        Ok(())
    }

    pub fn stop_recorder(&mut self) -> Result<()> {
        self.recorder_supervisor.stop()?;
        self.recorder_status = RecorderStatus::Disabled;
        self.last_recorder_refresh_at = None;
        self.provider = (self.make_stub_provider)();
        let snapshot = self.provider.handle(ProviderRequest::LoadDashboard)?;
        self.replace_snapshot(snapshot);
        Ok(())
    }

    pub fn reload_recorder_config(&mut self) -> Result<()> {
        let (config, note) = load_recorder_config_or_default(&self.recorder_config_path)?;
        self.recorder_config = config;
        self.recorder_config_note = note;
        self.recorder_editor = RecorderEditorState::default();
        self.apply_recorder_change("Reloaded recorder config from disk.")
    }

    pub fn reset_recorder_config(&mut self) -> Result<()> {
        self.recorder_config = RecorderConfig::default();
        self.recorder_editor = RecorderEditorState::default();
        self.apply_recorder_change("Reset recorder config to defaults.")
    }

    pub fn reload_oddsmatcher_query(&mut self) -> Result<()> {
        let (query, note) = oddsmatcher::load_query_or_default(&self.oddsmatcher_query_path)?;
        self.oddsmatcher_query = query;
        self.oddsmatcher_query_note = note;
        self.oddsmatcher_editor = OddsMatcherEditorState::default();
        self.oddsmatcher_rows.clear();
        self.oddsmatcher_table_state.select(None);
        self.status_message = String::from("Reloaded OddsMatcher config from disk.");
        Ok(())
    }

    pub fn reset_oddsmatcher_query(&mut self) -> Result<()> {
        self.oddsmatcher_query = GetBestMatchesVariables::default();
        self.oddsmatcher_editor = OddsMatcherEditorState::default();
        self.oddsmatcher_rows.clear();
        self.oddsmatcher_table_state.select(None);
        self.persist_oddsmatcher_query()?;
        self.status_message = String::from("Reset OddsMatcher config to defaults.");
        Ok(())
    }

    pub fn next_panel(&mut self) {
        self.toggle_observability_panel();
    }

    pub fn previous_panel(&mut self) {
        self.toggle_observability_panel();
    }

    pub fn next_section(&mut self) {
        match self.active_panel {
            Panel::Trading => {
                self.trading_section = next_from(self.trading_section, &TradingSection::ALL)
            }
            Panel::Observability => {
                self.observability_section =
                    next_from(self.observability_section, &ObservabilitySection::ALL)
            }
        }
    }

    pub fn previous_section(&mut self) {
        match self.active_panel {
            Panel::Trading => {
                self.trading_section = previous_from(self.trading_section, &TradingSection::ALL)
            }
            Panel::Observability => {
                self.observability_section =
                    previous_from(self.observability_section, &ObservabilitySection::ALL)
            }
        }
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

    pub fn handle_key(&mut self, key_code: KeyCode) {
        if self.is_oddsmatcher_editing_context() {
            match key_code {
                KeyCode::Esc => {
                    self.cancel_oddsmatcher_edit();
                    return;
                }
                KeyCode::Enter => {
                    if let Err(error) = self.apply_oddsmatcher_edit() {
                        self.status_message = format!("OddsMatcher filter error: {error}");
                    }
                    return;
                }
                KeyCode::Backspace => {
                    self.oddsmatcher_backspace();
                    return;
                }
                KeyCode::Char(character) => {
                    self.oddsmatcher_push_char(character);
                    return;
                }
                _ => return,
            }
        }

        if self.is_calculator_editing_context() {
            match key_code {
                KeyCode::Esc => {
                    self.cancel_calculator_edit();
                    return;
                }
                KeyCode::Enter => {
                    if let Err(error) = self.apply_calculator_edit() {
                        self.status_message = format!("Calculator input error: {error}");
                    }
                    return;
                }
                KeyCode::Backspace => {
                    self.calculator_backspace();
                    return;
                }
                KeyCode::Char(character) => {
                    if matches!(character, '0'..='9' | '.' | '-') {
                        self.calculator_push_char(character);
                    }
                    return;
                }
                _ => return,
            }
        }

        if self.is_recorder_editing_context() {
            match key_code {
                KeyCode::Esc => {
                    self.cancel_recorder_edit();
                    return;
                }
                KeyCode::Enter => {
                    if let Err(error) = self.apply_recorder_edit() {
                        self.status_message = format!("Recorder config error: {error}");
                    }
                    return;
                }
                KeyCode::Backspace => {
                    self.recorder_backspace();
                    return;
                }
                KeyCode::Char(character) => {
                    self.recorder_push_char(character);
                    return;
                }
                _ => return,
            }
        }

        if self.is_oddsmatcher_context() {
            match key_code {
                KeyCode::Left => {
                    self.focus_oddsmatcher_filters();
                    return;
                }
                KeyCode::Right => {
                    self.focus_oddsmatcher_results();
                    return;
                }
                _ => {}
            }
        }

        match key_code {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Esc => self.running = false,
            KeyCode::Char('o') => self.toggle_observability_panel(),
            KeyCode::Right | KeyCode::Char('l') => self.next_section(),
            KeyCode::Left | KeyCode::Char('h') => self.previous_section(),
            KeyCode::Enter => {
                if self.is_oddsmatcher_filters_context() {
                    self.begin_oddsmatcher_edit();
                } else if self.is_oddsmatcher_results_context() {
                    self.load_calculator_from_selected_oddsmatcher();
                } else if self.is_recorder_context() {
                    self.begin_recorder_edit();
                } else if self.is_calculator_context() {
                    self.begin_calculator_edit();
                }
            }
            KeyCode::Char('b') => {
                if self.is_calculator_context() {
                    self.cycle_calculator_bet_type();
                }
            }
            KeyCode::Char('m') => {
                if self.is_calculator_context() {
                    self.toggle_calculator_mode();
                }
            }
            KeyCode::Char('r') => {
                if let Err(error) = self.refresh() {
                    self.status_message = format!("Refresh failed: {error}");
                }
            }
            KeyCode::Char('c') => {
                if self.active_panel == Panel::Trading
                    && self.trading_section == TradingSection::Positions
                {
                    if let Err(error) = self.cash_out_next_actionable_bet() {
                        self.status_message = format!("Cash out failed: {error}");
                    }
                } else {
                    self.status_message =
                        String::from("Open Trading > Positions to request a tracked-bet cash out.");
                }
            }
            KeyCode::Char('[') => {
                if self.is_oddsmatcher_filters_context() {
                    if let Err(error) = self.cycle_oddsmatcher_suggestion(false) {
                        self.status_message = format!("OddsMatcher suggestion failed: {error}");
                    }
                } else if self.is_recorder_context() {
                    if let Err(error) = self.cycle_recorder_suggestion(false) {
                        self.status_message = format!("Recorder suggestion failed: {error}");
                    }
                }
            }
            KeyCode::Char(']') => {
                if self.is_oddsmatcher_filters_context() {
                    if let Err(error) = self.cycle_oddsmatcher_suggestion(true) {
                        self.status_message = format!("OddsMatcher suggestion failed: {error}");
                    }
                } else if self.is_recorder_context() {
                    if let Err(error) = self.cycle_recorder_suggestion(true) {
                        self.status_message = format!("Recorder suggestion failed: {error}");
                    }
                }
            }
            KeyCode::Char('u') => {
                if self.is_oddsmatcher_context() {
                    if let Err(error) = self.reload_oddsmatcher_query() {
                        self.status_message = format!("OddsMatcher reload failed: {error}");
                    }
                } else if self.is_recorder_context() {
                    if let Err(error) = self.reload_recorder_config() {
                        self.status_message = format!("Recorder reload failed: {error}");
                    }
                } else {
                    self.status_message =
                        String::from("Open Trading > Recorder to reload recorder config.");
                }
            }
            KeyCode::Char('D') => {
                if self.is_oddsmatcher_context() {
                    if let Err(error) = self.reset_oddsmatcher_query() {
                        self.status_message = format!("OddsMatcher reset failed: {error}");
                    }
                } else if self.is_recorder_context() {
                    if let Err(error) = self.reset_recorder_config() {
                        self.status_message = format!("Recorder reset failed: {error}");
                    }
                } else {
                    self.status_message =
                        String::from("Open Trading > Recorder to reset recorder config.");
                }
            }
            KeyCode::Char('s') => {
                if let Err(error) = self.start_recorder() {
                    self.status_message = format!("Recorder start failed: {error}");
                    self.recorder_status = RecorderStatus::Error;
                }
            }
            KeyCode::Char('x') => {
                if let Err(error) = self.stop_recorder() {
                    self.status_message = format!("Recorder stop failed: {error}");
                    self.recorder_status = RecorderStatus::Error;
                }
            }
            KeyCode::Down => match (self.active_panel, self.trading_section) {
                (Panel::Trading, TradingSection::Accounts) => self.select_next_exchange_row(),
                (Panel::Trading, TradingSection::Positions | TradingSection::Markets) => {
                    self.select_next_open_position_row()
                }
                (Panel::Trading, TradingSection::OddsMatcher) => {
                    if self.oddsmatcher_focus == OddsMatcherFocus::Filters {
                        self.oddsmatcher_editor.select_next_field();
                    } else {
                        self.select_next_oddsmatcher_row();
                    }
                }
                (Panel::Trading, TradingSection::Calculator) => {
                    self.calculator.editor.select_next_field()
                }
                (Panel::Trading, TradingSection::Recorder) => {
                    self.recorder_editor.select_next_field()
                }
                _ => {}
            },
            KeyCode::Up => match (self.active_panel, self.trading_section) {
                (Panel::Trading, TradingSection::Accounts) => self.select_previous_exchange_row(),
                (Panel::Trading, TradingSection::Positions | TradingSection::Markets) => {
                    self.select_previous_open_position_row()
                }
                (Panel::Trading, TradingSection::OddsMatcher) => {
                    if self.oddsmatcher_focus == OddsMatcherFocus::Filters {
                        self.oddsmatcher_editor.select_previous_field();
                    } else {
                        self.select_previous_oddsmatcher_row();
                    }
                }
                (Panel::Trading, TradingSection::Calculator) => {
                    self.calculator.editor.select_previous_field()
                }
                (Panel::Trading, TradingSection::Recorder) => {
                    self.recorder_editor.select_previous_field()
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn handle_key_code(&mut self, key_code: KeyCode) {
        self.handle_key(key_code);
    }

    fn is_recorder_context(&self) -> bool {
        self.active_panel == Panel::Trading && self.trading_section == TradingSection::Recorder
    }

    fn is_oddsmatcher_context(&self) -> bool {
        self.active_panel == Panel::Trading && self.trading_section == TradingSection::OddsMatcher
    }

    fn is_oddsmatcher_filters_context(&self) -> bool {
        self.is_oddsmatcher_context() && self.oddsmatcher_focus == OddsMatcherFocus::Filters
    }

    fn is_oddsmatcher_results_context(&self) -> bool {
        self.is_oddsmatcher_context() && self.oddsmatcher_focus == OddsMatcherFocus::Results
    }

    fn is_oddsmatcher_editing_context(&self) -> bool {
        self.is_oddsmatcher_filters_context() && self.oddsmatcher_editor.editing
    }

    fn is_recorder_editing_context(&self) -> bool {
        self.is_recorder_context() && self.recorder_editor.editing
    }

    fn is_calculator_context(&self) -> bool {
        self.active_panel == Panel::Trading && self.trading_section == TradingSection::Calculator
    }

    fn is_calculator_editing_context(&self) -> bool {
        self.is_calculator_context() && self.calculator.editor.editing
    }

    fn clamp_selected_exchange_row(&mut self) {
        match self.exchange_list_state.selected() {
            Some(index) if index >= self.snapshot.venues.len() => {
                self.exchange_list_state.select(None);
            }
            _ => {}
        }
    }

    fn clamp_selected_open_position_row(&mut self) {
        if self.snapshot.open_positions.is_empty() {
            self.open_position_table_state.select(None);
            return;
        }

        match self.open_position_table_state.selected() {
            Some(index) if index < self.snapshot.open_positions.len() => {}
            _ => self.open_position_table_state.select(Some(0)),
        }
    }

    fn clamp_selected_oddsmatcher_row(&mut self) {
        if self.oddsmatcher_rows.is_empty() {
            self.oddsmatcher_table_state.select(None);
            return;
        }

        match self.oddsmatcher_table_state.selected() {
            Some(index) if index < self.oddsmatcher_rows.len() => {}
            _ => self.oddsmatcher_table_state.select(Some(0)),
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

    pub fn select_next_open_position_row(&mut self) {
        if self.snapshot.open_positions.is_empty() {
            self.open_position_table_state.select(None);
            return;
        }

        let next_index = match self.open_position_table_state.selected() {
            Some(index) if index + 1 < self.snapshot.open_positions.len() => index + 1,
            Some(index) => index,
            None => 0,
        };

        self.open_position_table_state.select(Some(next_index));
    }

    pub fn select_previous_open_position_row(&mut self) {
        if self.snapshot.open_positions.is_empty() {
            self.open_position_table_state.select(None);
            return;
        }

        let previous_index = match self.open_position_table_state.selected() {
            Some(index) if index > 0 => index - 1,
            Some(index) => index,
            None => 0,
        };

        self.open_position_table_state.select(Some(previous_index));
    }

    pub fn select_next_oddsmatcher_row(&mut self) {
        if self.oddsmatcher_rows.is_empty() {
            self.oddsmatcher_table_state.select(None);
            return;
        }

        let next_index = match self.oddsmatcher_table_state.selected() {
            Some(index) if index + 1 < self.oddsmatcher_rows.len() => index + 1,
            Some(index) => index,
            None => 0,
        };

        self.oddsmatcher_table_state.select(Some(next_index));
    }

    pub fn select_previous_oddsmatcher_row(&mut self) {
        if self.oddsmatcher_rows.is_empty() {
            self.oddsmatcher_table_state.select(None);
            return;
        }

        let previous_index = match self.oddsmatcher_table_state.selected() {
            Some(index) if index > 0 => index - 1,
            Some(index) => index,
            None => 0,
        };

        self.oddsmatcher_table_state.select(Some(previous_index));
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
                self.last_recorder_refresh_at = None;
            }
        }

        if self.recorder_status == RecorderStatus::Running && self.recorder_refresh_due() {
            self.last_recorder_refresh_at = Some(Instant::now());
            let _ = self.refresh_provider_snapshot();
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

    fn replace_snapshot(&mut self, snapshot: ExchangePanelSnapshot) {
        self.snapshot = snapshot;
        self.status_message = self.snapshot.status_line.clone();
        self.clamp_selected_exchange_row();
        self.clamp_selected_open_position_row();
        self.clamp_selected_oddsmatcher_row();
    }

    fn load_recorder_dashboard_with_retry(&mut self) -> Result<ExchangePanelSnapshot> {
        let start = Instant::now();
        let timeout = self.recorder_startup_timeout();
        let retry_delay = Duration::from_millis(100);
        let log_path = self.recorder_log_path();
        loop {
            let last_error = match self.provider.handle(ProviderRequest::LoadDashboard) {
                Ok(snapshot) => return Ok(snapshot),
                Err(error) => error.to_string(),
            };

            self.recorder_status = self.recorder_supervisor.poll_status();
            if matches!(
                self.recorder_status,
                RecorderStatus::Stopped | RecorderStatus::Error
            ) {
                return Err(color_eyre::eyre::eyre!(
                    "recorder watcher stopped before first snapshot: {last_error}. See {}",
                    log_path.display()
                ));
            }

            if start.elapsed() >= timeout {
                return Err(color_eyre::eyre::eyre!(
                    "timed out waiting for recorder snapshot: {last_error}. See {}",
                    log_path.display()
                ));
            }

            thread::sleep(retry_delay);
        }
    }

    fn recorder_startup_timeout(&self) -> Duration {
        Duration::from_secs(self.recorder_config.interval_seconds.max(1) + 5)
    }

    fn recorder_log_path(&self) -> std::path::PathBuf {
        self.recorder_config.run_dir.join("watcher.log")
    }

    fn recorder_refresh_due(&self) -> bool {
        let refresh_interval = Duration::from_secs(1);
        self.last_recorder_refresh_at
            .is_none_or(|last| last.elapsed() >= refresh_interval)
    }

    fn begin_recorder_edit(&mut self) {
        let field = self.recorder_editor.selected_field();
        self.recorder_editor.buffer = field.display_value(&self.recorder_config);
        self.recorder_editor.editing = true;
        self.recorder_editor.replace_on_input = true;
        self.status_message = format!("Editing recorder {}.", field.label());
    }

    fn apply_recorder_edit(&mut self) -> Result<()> {
        let field = self.recorder_editor.selected_field();
        let value = self.recorder_editor.buffer.clone();
        field.apply_value(&mut self.recorder_config, &value)?;
        self.recorder_editor.editing = false;
        self.recorder_editor.buffer.clear();
        self.recorder_editor.replace_on_input = false;
        self.apply_recorder_change(&format!("Updated recorder {}.", field.label()))
    }

    fn cancel_recorder_edit(&mut self) {
        self.recorder_editor.editing = false;
        self.recorder_editor.buffer.clear();
        self.recorder_editor.replace_on_input = false;
        self.status_message = String::from("Cancelled recorder edit.");
    }

    fn recorder_push_char(&mut self, character: char) {
        if self.recorder_editor.replace_on_input {
            self.recorder_editor.buffer.clear();
            self.recorder_editor.replace_on_input = false;
        }
        self.recorder_editor.buffer.push(character);
    }

    fn recorder_backspace(&mut self) {
        if self.recorder_editor.replace_on_input {
            self.recorder_editor.buffer.clear();
            self.recorder_editor.replace_on_input = false;
            return;
        }
        self.recorder_editor.buffer.pop();
    }

    fn cycle_recorder_suggestion(&mut self, forward: bool) -> Result<()> {
        let field = self.recorder_editor.selected_field();
        let suggestions = field.suggestions();
        if suggestions.is_empty() {
            return Ok(());
        }

        let current_value = field.display_value(&self.recorder_config);
        let current_index = suggestions.iter().position(|value| value == &current_value);
        let next_index = match (current_index, forward) {
            (Some(index), true) => (index + 1) % suggestions.len(),
            (Some(index), false) => {
                if index == 0 {
                    suggestions.len() - 1
                } else {
                    index - 1
                }
            }
            (None, _) => 0,
        };

        field.apply_value(&mut self.recorder_config, &suggestions[next_index])?;
        self.apply_recorder_change(&format!(
            "Applied recorder suggestion for {}.",
            field.label()
        ))
    }

    fn persist_recorder_config(&mut self) -> Result<()> {
        self.recorder_config_note =
            save_recorder_config(&self.recorder_config_path, &self.recorder_config)?;
        Ok(())
    }

    fn apply_recorder_change(&mut self, message: &str) -> Result<()> {
        self.persist_recorder_config()?;

        if self.recorder_status == RecorderStatus::Running {
            self.recorder_supervisor.stop()?;
            self.recorder_supervisor.start(&self.recorder_config)?;
            self.recorder_status = self.recorder_supervisor.poll_status();
            self.provider = (self.make_recorder_provider)(&self.recorder_config);
            let snapshot = self.load_recorder_dashboard_with_retry()?;
            self.replace_snapshot(snapshot);
            self.last_recorder_refresh_at = Some(Instant::now());
            self.status_message = format!("{message} Restarted recorder to apply the change.");
            return Ok(());
        }

        self.status_message = String::from(message);
        Ok(())
    }

    fn begin_calculator_edit(&mut self) {
        let field = self.calculator.editor.selected_field();
        self.calculator.editor.buffer = field.display_value(&self.calculator);
        self.calculator.editor.editing = true;
        self.calculator.editor.replace_on_input = true;
        self.status_message = format!("Editing calculator {}.", field.label());
    }

    fn apply_calculator_edit(&mut self) -> Result<()> {
        let field = self.calculator.editor.selected_field();
        let value = self.calculator.editor.buffer.clone();
        let parsed = value
            .parse::<f64>()
            .map_err(|_| color_eyre::eyre::eyre!("{} must be numeric.", field.label()))?;
        match field {
            CalculatorField::BackStake => self.calculator.input.back_stake = parsed,
            CalculatorField::BackOdds => self.calculator.input.back_odds = parsed,
            CalculatorField::LayOdds => self.calculator.input.lay_odds = parsed,
            CalculatorField::BackCommission => self.calculator.input.back_commission_pct = parsed,
            CalculatorField::LayCommission => self.calculator.input.lay_commission_pct = parsed,
            CalculatorField::RiskFreeAward => self.calculator.input.risk_free_award = parsed,
            CalculatorField::RiskFreeRetention => {
                self.calculator.input.risk_free_retention_pct = parsed
            }
            CalculatorField::PartLayStakeOne => self.calculator.input.part_lays[0].stake = parsed,
            CalculatorField::PartLayOddsOne => self.calculator.input.part_lays[0].odds = parsed,
            CalculatorField::PartLayStakeTwo => self.calculator.input.part_lays[1].stake = parsed,
            CalculatorField::PartLayOddsTwo => self.calculator.input.part_lays[1].odds = parsed,
        }
        self.calculator.editor.editing = false;
        self.calculator.editor.buffer.clear();
        self.calculator.editor.replace_on_input = false;
        self.status_message = format!("Updated calculator {}.", field.label());
        Ok(())
    }

    fn cancel_calculator_edit(&mut self) {
        self.calculator.editor.editing = false;
        self.calculator.editor.buffer.clear();
        self.calculator.editor.replace_on_input = false;
        self.status_message = String::from("Cancelled calculator edit.");
    }

    fn calculator_push_char(&mut self, character: char) {
        if self.calculator.editor.replace_on_input {
            self.calculator.editor.buffer.clear();
            self.calculator.editor.replace_on_input = false;
        }
        self.calculator.editor.buffer.push(character);
    }

    fn calculator_backspace(&mut self) {
        if self.calculator.editor.replace_on_input {
            self.calculator.editor.buffer.clear();
            self.calculator.editor.replace_on_input = false;
            return;
        }
        self.calculator.editor.buffer.pop();
    }

    fn cycle_calculator_bet_type(&mut self) {
        let current = self.calculator.input.bet_type;
        let index = BetType::ALL
            .iter()
            .position(|candidate| candidate == &current)
            .unwrap_or(0);
        self.calculator.input.bet_type = BetType::ALL[(index + 1) % BetType::ALL.len()];
        self.status_message = format!(
            "Calculator bet type set to {}.",
            self.calculator.input.bet_type.label()
        );
    }

    fn toggle_calculator_mode(&mut self) {
        self.calculator.input.mode.toggle();
        self.status_message = format!(
            "Calculator mode set to {}.",
            self.calculator.input.mode.label()
        );
    }

    fn focus_oddsmatcher_filters(&mut self) {
        self.oddsmatcher_focus = OddsMatcherFocus::Filters;
        self.status_message = String::from("OddsMatcher focus set to filters.");
    }

    fn focus_oddsmatcher_results(&mut self) {
        self.oddsmatcher_focus = OddsMatcherFocus::Results;
        self.status_message = String::from("OddsMatcher focus set to results.");
    }

    fn begin_oddsmatcher_edit(&mut self) {
        let field = self.oddsmatcher_editor.selected_field();
        self.oddsmatcher_editor.buffer = field.display_value(&self.oddsmatcher_query);
        self.oddsmatcher_editor.editing = true;
        self.oddsmatcher_editor.replace_on_input = true;
        self.status_message = format!("Editing OddsMatcher {}.", field.label());
    }

    fn apply_oddsmatcher_edit(&mut self) -> Result<()> {
        let field = self.oddsmatcher_editor.selected_field();
        let value = self.oddsmatcher_editor.buffer.clone();
        field.apply_value(&mut self.oddsmatcher_query, &value)?;
        self.oddsmatcher_editor.editing = false;
        self.oddsmatcher_editor.buffer.clear();
        self.oddsmatcher_editor.replace_on_input = false;
        self.oddsmatcher_rows.clear();
        self.oddsmatcher_table_state.select(None);
        self.persist_oddsmatcher_query()?;
        self.status_message = format!(
            "Updated OddsMatcher {} and saved config. Press r to refresh.",
            field.label()
        );
        Ok(())
    }

    fn cancel_oddsmatcher_edit(&mut self) {
        self.oddsmatcher_editor.editing = false;
        self.oddsmatcher_editor.buffer.clear();
        self.oddsmatcher_editor.replace_on_input = false;
        self.status_message = String::from("Cancelled OddsMatcher edit.");
    }

    fn oddsmatcher_push_char(&mut self, character: char) {
        if self.oddsmatcher_editor.replace_on_input {
            self.oddsmatcher_editor.buffer.clear();
            self.oddsmatcher_editor.replace_on_input = false;
        }
        self.oddsmatcher_editor.buffer.push(character);
    }

    fn oddsmatcher_backspace(&mut self) {
        if self.oddsmatcher_editor.replace_on_input {
            self.oddsmatcher_editor.buffer.clear();
            self.oddsmatcher_editor.replace_on_input = false;
            return;
        }
        self.oddsmatcher_editor.buffer.pop();
    }

    fn cycle_oddsmatcher_suggestion(&mut self, forward: bool) -> Result<()> {
        let field = self.oddsmatcher_editor.selected_field();
        let suggestions = field.suggestions();
        if suggestions.is_empty() {
            return Ok(());
        }

        let current_value = field.display_value(&self.oddsmatcher_query);
        let current_index = suggestions.iter().position(|value| value == &current_value);
        let next_index = match (current_index, forward) {
            (Some(index), true) => (index + 1) % suggestions.len(),
            (Some(index), false) => {
                if index == 0 {
                    suggestions.len() - 1
                } else {
                    index - 1
                }
            }
            (None, _) => 0,
        };

        field.apply_value(&mut self.oddsmatcher_query, &suggestions[next_index])?;
        self.oddsmatcher_rows.clear();
        self.oddsmatcher_table_state.select(None);
        self.persist_oddsmatcher_query()?;
        self.status_message = format!("Applied OddsMatcher suggestion for {}.", field.label());
        Ok(())
    }

    fn persist_oddsmatcher_query(&mut self) -> Result<()> {
        self.oddsmatcher_query_note =
            oddsmatcher::save_query(&self.oddsmatcher_query_path, &self.oddsmatcher_query)?;
        Ok(())
    }

    fn load_calculator_from_selected_oddsmatcher(&mut self) {
        let Some(row) = self.selected_oddsmatcher_row().cloned() else {
            self.status_message = String::from("No OddsMatcher row is selected.");
            return;
        };

        self.calculator.input.back_odds = row.back.odds;
        self.calculator.input.lay_odds = row.lay.odds;
        self.calculator.source = Some(CalculatorSourceContext {
            event_name: row.event_name.clone(),
            selection_name: row.selection_name.clone(),
            competition_name: row.event_group.display_name.clone(),
            rating: row.rating,
            bookmaker_name: row.back.bookmaker.display_name.clone(),
            exchange_name: row.lay.bookmaker.display_name.clone(),
        });
        self.trading_section = TradingSection::Calculator;
        self.status_message = format!(
            "Loaded calculator from OddsMatcher row: {} @ {:.2} / {:.2}.",
            row.selection_name, row.back.odds, row.lay.odds
        );
    }

    fn refresh_oddsmatcher(&mut self) -> Result<()> {
        let rows = oddsmatcher::fetch_best_matches(&self.oddsmatcher_client, &self.oddsmatcher_query)
            .map_err(|error| {
                self.status_message = format!("OddsMatcher refresh failed: {error}");
                error
            })?;
        let row_count = rows.len();
        self.replace_oddsmatcher_rows(rows, format!("Loaded {row_count} live OddsMatcher row(s)."));
        Ok(())
    }

    fn toggle_observability_panel(&mut self) {
        self.active_panel = match self.active_panel {
            Panel::Trading => Panel::Observability,
            Panel::Observability => Panel::Trading,
        };
    }
}

fn next_from<T: Copy + PartialEq>(value: T, all: &[T]) -> T {
    let index = all
        .iter()
        .position(|candidate| candidate == &value)
        .unwrap_or(0);
    all[(index + 1) % all.len()]
}

fn previous_from<T: Copy + PartialEq>(value: T, all: &[T]) -> T {
    let index = all
        .iter()
        .position(|candidate| candidate == &value)
        .unwrap_or(0);
    if index == 0 {
        all[all.len() - 1]
    } else {
        all[index - 1]
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
                companion_legs_path: config.companion_legs_path.clone(),
                agent_browser_session: None,
                commission_rate: config.commission_rate.parse::<f64>().unwrap_or(0.0),
                target_profit: config.target_profit.parse::<f64>().unwrap_or(1.0),
                stop_loss: config.stop_loss.parse::<f64>().unwrap_or(1.0),
                hard_margin_call_profit_floor: parse_optional_f64(
                    &config.hard_margin_call_profit_floor,
                ),
                warn_only_default: config.warn_only_default,
            },
        )) as Box<dyn ExchangeProvider>
    })
}

fn parse_optional_f64(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        trimmed.parse::<f64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use crate::domain::{
        ExchangePanelSnapshot, VenueId, VenueStatus, VenueSummary, WorkerStatus, WorkerSummary,
    };
    use crate::provider::{ExchangeProvider, ProviderRequest};
    use crate::recorder::{RecorderConfig, RecorderStatus, RecorderSupervisor};

    use super::{App, Panel, TradingSection};

    struct RefreshingProvider {
        refresh_count: Rc<RefCell<usize>>,
        load_snapshot: ExchangePanelSnapshot,
        refresh_snapshot: ExchangePanelSnapshot,
    }

    impl ExchangeProvider for RefreshingProvider {
        fn handle(
            &mut self,
            request: ProviderRequest,
        ) -> color_eyre::Result<ExchangePanelSnapshot> {
            match request {
                ProviderRequest::LoadDashboard => Ok(self.load_snapshot.clone()),
                ProviderRequest::Refresh => {
                    *self.refresh_count.borrow_mut() += 1;
                    Ok(self.refresh_snapshot.clone())
                }
                ProviderRequest::SelectVenue(_) | ProviderRequest::CashOutTrackedBet { .. } => {
                    Ok(self.refresh_snapshot.clone())
                }
            }
        }
    }

    struct RunningSupervisor;

    struct DisabledSupervisor;

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

    impl RecorderSupervisor for DisabledSupervisor {
        fn start(&mut self, _config: &RecorderConfig) -> color_eyre::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> color_eyre::Result<()> {
            Ok(())
        }

        fn poll_status(&mut self) -> RecorderStatus {
            RecorderStatus::Disabled
        }
    }

    #[test]
    fn poll_recorder_refreshes_running_recorder_automatically() {
        let refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                refresh_count: refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                refresh_snapshot: sample_snapshot("Auto refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider>
            }),
            Box::new(RunningSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.recorder_status = RecorderStatus::Running;
        app.last_recorder_refresh_at = Some(Instant::now() - Duration::from_secs(2));

        app.poll_recorder();

        assert_eq!(app.snapshot().status_line, "Auto refreshed dashboard");
        assert_eq!(*refresh_count.borrow(), 1);
    }

    #[test]
    fn poll_recorder_skips_auto_refresh_when_not_running() {
        let refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                refresh_count: refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                refresh_snapshot: sample_snapshot("Auto refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider>
            }),
            Box::new(DisabledSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.recorder_status = RecorderStatus::Disabled;
        app.last_recorder_refresh_at = Some(Instant::now() - Duration::from_secs(2));

        app.poll_recorder();

        assert_eq!(app.snapshot().status_line, "Initial dashboard");
        assert_eq!(*refresh_count.borrow(), 0);
    }

    #[test]
    fn poll_recorder_skips_auto_refresh_before_interval_elapses() {
        let refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                refresh_count: refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                refresh_snapshot: sample_snapshot("Auto refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider>
            }),
            Box::new(RunningSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.recorder_status = RecorderStatus::Running;
        app.last_recorder_refresh_at = Some(Instant::now());

        app.poll_recorder();

        assert_eq!(app.snapshot().status_line, "Initial dashboard");
        assert_eq!(*refresh_count.borrow(), 0);
    }

    #[test]
    fn poll_recorder_keeps_provider_refresh_running_inside_oddsmatcher() {
        let refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                refresh_count: refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                refresh_snapshot: sample_snapshot("Auto refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider>
            }),
            Box::new(RunningSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.set_active_panel(Panel::Trading);
        app.set_trading_section(TradingSection::OddsMatcher);
        app.recorder_status = RecorderStatus::Running;
        app.last_recorder_refresh_at = Some(Instant::now() - Duration::from_secs(2));

        app.poll_recorder();

        assert_eq!(app.snapshot().status_line, "Auto refreshed dashboard");
        assert!(app.oddsmatcher_rows().is_empty());
        assert_eq!(*refresh_count.borrow(), 1);
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
            other_open_bets: Vec::new(),
            decisions: Vec::new(),
            watch: None,
            tracked_bets: Vec::new(),
            exit_policy: Default::default(),
            exit_recommendations: Vec::new(),
        }
    }
}
