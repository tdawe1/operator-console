use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::widgets::{ListState, TableState};
use ratatui::{Frame, Terminal};
use reqwest::blocking::Client;

pub use crate::app_state::{
    CalculatorEditorState, CalculatorField, CalculatorSourceContext, CalculatorState,
    CalculatorTool, MatcherView, ObservabilitySection, OddsMatcherFocus, Panel, PositionsFocus,
    TradingActionField, TradingActionOverlayState, TradingSection,
};
use crate::calculator::{self, BetType, Mode as CalculatorMode};
use crate::domain::{
    ExchangePanelSnapshot, OpenPositionRow, TrackedBetRow, TrackedLeg, VenueId, WorkerStatus,
};
use crate::exchange_api::{load_matchbook_account_state, MatchbookAccountState};
use crate::horse_matcher::{self, HorseMatcherEditorState, HorseMatcherField, HorseMatcherQuery};
use crate::native_provider::{HybridExchangeProvider, NativeExchangeProvider};
use crate::oddsmatcher::{
    self, GetBestMatchesVariables, OddsMatcherEditorState, OddsMatcherField, OddsMatcherRow,
};
use crate::owls::{
    self, OwlsDashboard, OwlsEndpointGroup, OwlsEndpointId, OwlsEndpointSummary, OwlsSyncReason,
    SUPPORTED_SPORTS,
};
use crate::panels::trading_positions::{
    active_position_row_count, next_actionable_cash_out_bet_id, selected_active_position_seed,
};
use crate::provider::{ExchangeProvider, ProviderRequest};
use crate::recorder::{
    default_config_path, load_recorder_config_or_default, save_recorder_config,
    ProcessRecorderSupervisor, RecorderConfig, RecorderEditorState, RecorderField, RecorderStatus,
    RecorderSupervisor,
};
use crate::stub_provider::StubExchangeProvider;
use crate::trading_actions::{
    format_decimal, TradingActionMode, TradingActionSeed, TradingActionSide, TradingActionSource,
    TradingActionSourceContext, TradingTimeInForce,
};
use crate::transport::WorkerConfig;
use crate::ui;
use crate::worker_client::{BetRecorderWorkerClient, WorkerClientExchangeProvider};

type ProviderFactory = dyn Fn(&RecorderConfig) -> Box<dyn ExchangeProvider + Send>;
type StubFactory = dyn Fn() -> Box<dyn ExchangeProvider + Send>;

struct ProviderJob {
    request: ProviderRequest,
    failure_context: String,
    event_message: Option<String>,
}

struct ProviderResult {
    request: ProviderRequest,
    result: std::result::Result<ExchangePanelSnapshot, String>,
    failure_context: String,
    event_message: Option<String>,
}

struct OwlsSyncJob {
    dashboard: OwlsDashboard,
    reason: OwlsSyncReason,
    focused: Option<OwlsEndpointId>,
}

struct OwlsSyncResult {
    outcome: owls::OwlsSyncOutcome,
    reason: OwlsSyncReason,
}

#[derive(Clone, Copy)]
enum MatchbookSyncReason {
    Manual,
    Background,
}

impl MatchbookSyncReason {
    fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Background => "monitor",
        }
    }
}

struct MatchbookSyncJob {
    reason: MatchbookSyncReason,
}

struct MatchbookSyncResult {
    state: std::result::Result<MatchbookAccountState, String>,
    reason: MatchbookSyncReason,
}

struct OddsMatcherJob {
    query: GetBestMatchesVariables,
}

struct OddsMatcherResult {
    result: std::result::Result<Vec<OddsMatcherRow>, String>,
}

#[derive(Clone, Copy)]
enum MouseTargetKind {
    TradingSection(TradingSection),
    MatcherView(MatcherView),
    CalculatorTool(CalculatorTool),
}

#[derive(Clone, Copy)]
struct MouseTarget {
    rect: Rect,
    kind: MouseTargetKind,
}

pub struct App {
    provider_tx: Sender<ProviderJob>,
    provider_rx: Receiver<ProviderResult>,
    provider_in_flight: bool,
    provider_pending_job: Option<ProviderJob>,
    make_stub_provider: Box<StubFactory>,
    make_recorder_provider: Box<ProviderFactory>,
    recorder_supervisor: Box<dyn RecorderSupervisor>,
    recorder_config: RecorderConfig,
    recorder_config_path: std::path::PathBuf,
    recorder_config_note: String,
    recorder_editor: RecorderEditorState,
    recorder_status: RecorderStatus,
    calculator: CalculatorState,
    calculator_tool: CalculatorTool,
    oddsmatcher_tx: Sender<OddsMatcherJob>,
    oddsmatcher_rx: Receiver<OddsMatcherResult>,
    oddsmatcher_in_flight: bool,
    oddsmatcher_pending_query: Option<GetBestMatchesVariables>,
    owls_sync_tx: Sender<OwlsSyncJob>,
    owls_sync_rx: Receiver<OwlsSyncResult>,
    owls_sync_in_flight: bool,
    owls_sync_pending_reason: Option<OwlsSyncReason>,
    last_owls_sync_dispatch_at: Option<Instant>,
    matchbook_sync_tx: Sender<MatchbookSyncJob>,
    matchbook_sync_rx: Receiver<MatchbookSyncResult>,
    matchbook_sync_in_flight: bool,
    matchbook_sync_pending_reason: Option<MatchbookSyncReason>,
    last_matchbook_sync_dispatch_at: Option<Instant>,
    matchbook_account_state: Option<MatchbookAccountState>,
    owls_dashboard: OwlsDashboard,
    owls_endpoint_table_state: TableState,
    oddsmatcher_query_path: PathBuf,
    oddsmatcher_query_note: String,
    oddsmatcher_query: GetBestMatchesVariables,
    oddsmatcher_editor: OddsMatcherEditorState,
    oddsmatcher_focus: OddsMatcherFocus,
    oddsmatcher_rows: Vec<OddsMatcherRow>,
    oddsmatcher_table_state: TableState,
    horse_matcher_query_path: PathBuf,
    horse_matcher_query_note: String,
    horse_matcher_query: HorseMatcherQuery,
    horse_matcher_editor: HorseMatcherEditorState,
    horse_matcher_focus: OddsMatcherFocus,
    horse_matcher_rows: Vec<OddsMatcherRow>,
    horse_matcher_snapshot: Option<crate::domain::HorseMatcherSnapshot>,
    horse_matcher_table_state: TableState,
    matcher_view: MatcherView,
    snapshot: ExchangePanelSnapshot,
    active_panel: Panel,
    trading_section: TradingSection,
    observability_section: ObservabilitySection,
    exchange_list_state: ListState,
    open_position_table_state: TableState,
    historical_position_table_state: TableState,
    positions_focus: PositionsFocus,
    live_view_overlay_visible: bool,
    markets_overlay_visible: bool,
    keymap_overlay_visible: bool,
    trading_action_overlay: Option<TradingActionOverlayState>,
    last_recorder_refresh_at: Option<Instant>,
    last_successful_snapshot_at: Option<String>,
    last_recorder_start_failure: Option<String>,
    event_history: VecDeque<String>,
    mouse_targets: Vec<MouseTarget>,
    running: bool,
    status_message: String,
    status_scroll: u16,
}

const MAX_EVENT_HISTORY: usize = 25;
const RECORDER_REFRESH_INTERVAL_IDLE: Duration = Duration::from_secs(5);
const RECORDER_REFRESH_INTERVAL_ACTIVE: Duration = Duration::from_secs(2);
const RECORDER_REFRESH_INTERVAL_BOOTSTRAP: Duration = Duration::from_secs(1);
const OWLS_SYNC_DISPATCH_INTERVAL: Duration = Duration::from_secs(1);
const MATCHBOOK_SYNC_DISPATCH_INTERVAL: Duration = Duration::from_secs(4);

impl Default for App {
    fn default() -> Self {
        let stub_factory =
            || Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>;
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
    pub fn from_provider<P: ExchangeProvider + Send + 'static>(provider: P) -> Result<Self> {
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
        provider: Box<dyn ExchangeProvider + Send>,
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
        provider: Box<dyn ExchangeProvider + Send>,
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
        provider: Box<dyn ExchangeProvider + Send>,
        make_stub_provider: Box<StubFactory>,
        make_recorder_provider: Box<ProviderFactory>,
        recorder_supervisor: Box<dyn RecorderSupervisor>,
        recorder_config: RecorderConfig,
        recorder_config_path: std::path::PathBuf,
        recorder_config_note: String,
        oddsmatcher_query_path: PathBuf,
    ) -> Result<Self> {
        Self::with_dependencies_and_storage_matcher_paths(
            provider,
            make_stub_provider,
            make_recorder_provider,
            recorder_supervisor,
            recorder_config,
            recorder_config_path,
            recorder_config_note,
            oddsmatcher_query_path,
            horse_matcher::default_query_path(),
        )
    }

    pub fn with_dependencies_and_storage_matcher_paths(
        mut provider: Box<dyn ExchangeProvider + Send>,
        make_stub_provider: Box<StubFactory>,
        make_recorder_provider: Box<ProviderFactory>,
        recorder_supervisor: Box<dyn RecorderSupervisor>,
        recorder_config: RecorderConfig,
        recorder_config_path: std::path::PathBuf,
        recorder_config_note: String,
        oddsmatcher_query_path: PathBuf,
        horse_matcher_query_path: PathBuf,
    ) -> Result<Self> {
        let snapshot = normalize_snapshot(
            provider.handle(ProviderRequest::LoadDashboard)?,
            &recorder_config.disabled_venues,
        );
        let last_successful_snapshot_at = runtime_updated_at(&snapshot).map(str::to_string);
        let status_message = snapshot.status_line.clone();
        let (oddsmatcher_query, oddsmatcher_query_note) =
            oddsmatcher::load_query_or_default(&oddsmatcher_query_path).unwrap_or_else(|error| {
                (
                    GetBestMatchesVariables::default(),
                    format!("OddsMatcher config load failed; using defaults: {error}"),
                )
            });
        let (horse_matcher_query, horse_matcher_query_note) = horse_matcher::load_query_or_default(
            &horse_matcher_query_path,
        )
        .unwrap_or_else(|error| {
            (
                HorseMatcherQuery::default(),
                format!("Horse Matcher config load failed; using defaults: {error}"),
            )
        });
        let mut open_position_table_state = TableState::default();
        let mut historical_position_table_state = TableState::default();
        let positions_focus = if !snapshot.open_positions.is_empty() {
            open_position_table_state.select(Some(0));
            PositionsFocus::Active
        } else if !snapshot.historical_positions.is_empty() {
            historical_position_table_state.select(Some(0));
            PositionsFocus::Historical
        } else {
            PositionsFocus::Active
        };
        let oddsmatcher_client = oddsmatcher::build_client().unwrap_or_else(|_| {
            Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(12))
                .build()
                .unwrap_or_else(|_| Client::new())
        });
        let owls_client = owls::build_client().unwrap_or_else(|_| {
            Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new())
        });
        let (provider_tx, provider_rx) = start_provider_worker(provider);
        let (oddsmatcher_tx, oddsmatcher_rx) = start_oddsmatcher_worker(oddsmatcher_client);
        let (owls_sync_tx, owls_sync_rx) = start_owls_sync_worker(owls_client.clone());
        let (matchbook_sync_tx, matchbook_sync_rx) = start_matchbook_sync_worker();

        let mut app = Self {
            provider_tx,
            provider_rx,
            provider_in_flight: false,
            provider_pending_job: None,
            make_stub_provider,
            make_recorder_provider,
            recorder_supervisor,
            recorder_config,
            recorder_config_path,
            recorder_config_note,
            recorder_editor: RecorderEditorState::default(),
            recorder_status: RecorderStatus::Disabled,
            calculator: CalculatorState::default(),
            calculator_tool: CalculatorTool::Basic,
            oddsmatcher_tx,
            oddsmatcher_rx,
            oddsmatcher_in_flight: false,
            oddsmatcher_pending_query: None,
            owls_sync_tx,
            owls_sync_rx,
            owls_sync_in_flight: false,
            owls_sync_pending_reason: None,
            last_owls_sync_dispatch_at: None,
            matchbook_sync_tx,
            matchbook_sync_rx,
            matchbook_sync_in_flight: false,
            matchbook_sync_pending_reason: None,
            last_matchbook_sync_dispatch_at: None,
            matchbook_account_state: None,
            owls_dashboard: {
                let dashboard = OwlsDashboard::default();
                dashboard
            },
            owls_endpoint_table_state: {
                let mut state = TableState::default();
                state.select(Some(0));
                state
            },
            oddsmatcher_query_path,
            oddsmatcher_query_note,
            oddsmatcher_query,
            oddsmatcher_editor: OddsMatcherEditorState::default(),
            oddsmatcher_focus: OddsMatcherFocus::Results,
            oddsmatcher_rows: Vec::new(),
            oddsmatcher_table_state: TableState::default(),
            horse_matcher_query_path,
            horse_matcher_query_note,
            horse_matcher_query,
            horse_matcher_editor: HorseMatcherEditorState::default(),
            horse_matcher_focus: OddsMatcherFocus::Results,
            horse_matcher_rows: Vec::new(),
            horse_matcher_snapshot: None,
            horse_matcher_table_state: TableState::default(),
            matcher_view: MatcherView::Odds,
            snapshot,
            active_panel: Panel::Trading,
            trading_section: TradingSection::Positions,
            observability_section: ObservabilitySection::Workers,
            exchange_list_state: ListState::default(),
            open_position_table_state,
            historical_position_table_state,
            positions_focus,
            live_view_overlay_visible: false,
            markets_overlay_visible: false,
            keymap_overlay_visible: false,
            trading_action_overlay: None,
            last_recorder_refresh_at: None,
            last_successful_snapshot_at,
            last_recorder_start_failure: None,
            event_history: VecDeque::with_capacity(MAX_EVENT_HISTORY),
            mouse_targets: Vec::new(),
            running: true,
            status_message,
            status_scroll: 0,
        };
        app.record_event(format!(
            "Loaded initial dashboard from {}.",
            app.snapshot
                .runtime
                .as_ref()
                .map(|runtime| runtime.source.as_str())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("snapshot")
        ));
        Ok(app)
    }

    pub fn snapshot(&self) -> &ExchangePanelSnapshot {
        &self.snapshot
    }

    pub fn owls_dashboard(&self) -> &OwlsDashboard {
        &self.owls_dashboard
    }

    pub fn matchbook_account_state(&self) -> Option<&MatchbookAccountState> {
        self.matchbook_account_state.as_ref()
    }

    pub fn owls_sport(&self) -> &str {
        self.owls_dashboard.sport.as_str()
    }

    pub fn matcher_view(&self) -> MatcherView {
        self.matcher_view
    }

    pub fn calculator_tool(&self) -> CalculatorTool {
        self.calculator_tool
    }

    pub fn visible_owls_endpoints(&self) -> Vec<&OwlsEndpointSummary> {
        let groups = self.visible_owls_groups();
        self.owls_dashboard
            .endpoints
            .iter()
            .filter(|endpoint| groups.contains(&endpoint.group))
            .collect()
    }

    pub fn selected_owls_endpoint(&self) -> Option<&OwlsEndpointSummary> {
        let visible = self.visible_owls_endpoints();
        self.owls_endpoint_table_state
            .selected()
            .and_then(|index| visible.get(index).copied())
            .or_else(|| visible.first().copied())
    }

    fn selected_owls_endpoint_id(&self) -> Option<OwlsEndpointId> {
        self.selected_owls_endpoint().map(|endpoint| endpoint.id)
    }

    pub fn owls_endpoint_table_state(&mut self) -> &mut TableState {
        &mut self.owls_endpoint_table_state
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn active_panel(&self) -> Panel {
        self.active_panel
    }

    pub fn set_active_panel(&mut self, panel: Panel) {
        self.active_panel = panel;
        if panel != Panel::Trading {
            self.live_view_overlay_visible = false;
            self.markets_overlay_visible = false;
            self.trading_action_overlay = None;
        }
    }

    pub fn active_trading_section(&self) -> TradingSection {
        self.trading_section
    }

    pub fn set_trading_section(&mut self, section: TradingSection) {
        self.trading_section = section;
        if section != TradingSection::Positions {
            self.live_view_overlay_visible = false;
        }
        if !self.is_owls_context() {
            self.markets_overlay_visible = false;
        }
        if section != TradingSection::Positions && section != TradingSection::Matcher {
            self.trading_action_overlay = None;
        }
        if self.is_owls_context() {
            self.clamp_selected_owls_endpoint();
        }
    }

    pub fn active_observability_section(&self) -> ObservabilitySection {
        self.observability_section
    }

    pub fn help_text(&self) -> &'static str {
        "? keymap | q quit | o observability | h/l sections | arrows or j/k nav | tab rotate pane/tool | r refresh cache | R recapture live\nenter edit/open | p place action | esc cancel | [/] cycle sport or suggestions | u reload | D defaults | s start recorder | x stop recorder | c cash out | v live view | b cycle type | m toggle mode"
    }

    pub fn live_view_overlay_visible(&self) -> bool {
        self.live_view_overlay_visible
    }

    pub fn keymap_overlay_visible(&self) -> bool {
        self.keymap_overlay_visible
    }

    pub fn markets_overlay_visible(&self) -> bool {
        self.markets_overlay_visible
    }

    pub fn trading_action_overlay(&self) -> Option<&TradingActionOverlayState> {
        self.trading_action_overlay.as_ref()
    }

    pub fn toggle_live_view_overlay(&mut self) {
        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::Positions
        {
            self.live_view_overlay_visible = !self.live_view_overlay_visible;
        }
    }

    pub fn toggle_keymap_overlay(&mut self) {
        self.keymap_overlay_visible = !self.keymap_overlay_visible;
    }

    pub fn toggle_markets_overlay(&mut self) {
        if self.active_panel == Panel::Trading && self.is_owls_context() {
            self.markets_overlay_visible = !self.markets_overlay_visible;
        }
    }

    pub fn cycle_matcher_view(&mut self, forward: bool) {
        let current = self.matcher_view;
        let all = &MatcherView::ALL;
        let index = all
            .iter()
            .position(|candidate| *candidate == current)
            .unwrap_or(0);
        let next = if forward {
            (index + 1) % all.len()
        } else if index == 0 {
            all.len() - 1
        } else {
            index - 1
        };
        self.matcher_view = all[next];
        self.status_message = format!("Matcher view set to {}.", self.matcher_view.label());
        self.status_scroll = 0;
    }

    pub fn cycle_calculator_tool(&mut self, forward: bool) {
        let current = self.calculator_tool;
        let all = &CalculatorTool::ALL;
        let index = all
            .iter()
            .position(|candidate| *candidate == current)
            .unwrap_or(0);
        let next = if forward {
            (index + 1) % all.len()
        } else if index == 0 {
            all.len() - 1
        } else {
            index - 1
        };
        self.calculator_tool = all[next];
        self.status_message = format!("Calculator tool set to {}.", self.calculator_tool.label());
        self.status_scroll = 0;
    }

    pub fn cycle_owls_sport(&mut self, forward: bool) {
        let current_index = SUPPORTED_SPORTS
            .iter()
            .position(|sport| *sport == self.owls_dashboard.sport)
            .unwrap_or(0);
        let next_index = if forward {
            (current_index + 1) % SUPPORTED_SPORTS.len()
        } else if current_index == 0 {
            SUPPORTED_SPORTS.len() - 1
        } else {
            current_index - 1
        };
        let sport = SUPPORTED_SPORTS[next_index];
        self.owls_dashboard = owls::dashboard_for_sport(sport);
        self.clamp_selected_owls_endpoint();
        self.markets_overlay_visible = false;
        self.request_owls_sync(OwlsSyncReason::Manual);
        self.status_message = format!("Owls sport set to {sport}. Sync queued.");
        self.status_scroll = 0;
        self.record_event(format!("Owls sport set to {sport}."));
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

    pub fn wait_for_async_idle(&mut self, timeout: Duration) -> bool {
        let started = Instant::now();
        loop {
            self.drain_provider_results();
            self.drain_oddsmatcher_results();
            self.drain_owls_sync_results();
            self.drain_matchbook_sync_results();
            if !self.provider_in_flight
                && !self.oddsmatcher_in_flight
                && !self.owls_sync_in_flight
                && !self.matchbook_sync_in_flight
            {
                return true;
            }
            if started.elapsed() >= timeout {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn status_scroll(&self) -> u16 {
        self.status_scroll
    }

    pub fn clear_mouse_targets(&mut self) {
        self.mouse_targets.clear();
    }

    pub fn register_trading_section_target(&mut self, rect: Rect, section: TradingSection) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::TradingSection(section),
        });
    }

    pub fn register_matcher_view_target(&mut self, rect: Rect, view: MatcherView) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::MatcherView(view),
        });
    }

    pub fn register_calculator_tool_target(&mut self, rect: Rect, tool: CalculatorTool) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::CalculatorTool(tool),
        });
    }

    pub fn selected_open_position_row(&self) -> Option<usize> {
        self.open_position_table_state.selected()
    }

    pub fn open_position_table_state(&mut self) -> &mut TableState {
        &mut self.open_position_table_state
    }

    pub fn historical_position_table_state(&mut self) -> &mut TableState {
        &mut self.historical_position_table_state
    }

    pub fn position_table_states(&mut self) -> (&mut TableState, &mut TableState) {
        (
            &mut self.open_position_table_state,
            &mut self.historical_position_table_state,
        )
    }

    pub fn selected_historical_position_row(&self) -> Option<usize> {
        self.historical_position_table_state.selected()
    }

    pub fn positions_focus(&self) -> PositionsFocus {
        self.positions_focus
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
                (
                    field,
                    value,
                    self.oddsmatcher_editor.selected_field() == field,
                )
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

    pub fn horse_matcher_rows(&self) -> &[OddsMatcherRow] {
        &self.horse_matcher_rows
    }

    pub fn horse_matcher_query(&self) -> &HorseMatcherQuery {
        &self.horse_matcher_query
    }

    pub fn horse_matcher_query_note(&self) -> &str {
        &self.horse_matcher_query_note
    }

    pub fn horse_matcher_selected_field(&self) -> HorseMatcherField {
        self.horse_matcher_editor.selected_field()
    }

    pub fn horse_matcher_focus(&self) -> OddsMatcherFocus {
        self.horse_matcher_focus
    }

    pub fn horse_matcher_is_editing(&self) -> bool {
        self.horse_matcher_editor.editing
    }

    pub fn horse_matcher_edit_buffer(&self) -> Option<&str> {
        self.horse_matcher_editor
            .editing
            .then_some(self.horse_matcher_editor.buffer.as_str())
    }

    pub fn selected_horse_matcher_row(&self) -> Option<&OddsMatcherRow> {
        self.horse_matcher_table_state
            .selected()
            .and_then(|index| self.horse_matcher_rows.get(index))
    }

    pub fn horse_matcher_table_state(&mut self) -> &mut TableState {
        &mut self.horse_matcher_table_state
    }

    pub fn recorder_config(&self) -> &RecorderConfig {
        &self.recorder_config
    }

    pub fn recorder_status(&self) -> &RecorderStatus {
        &self.recorder_status
    }

    pub fn recorder_lifecycle_state(&self) -> &'static str {
        match self.recorder_status {
            RecorderStatus::Disabled => "disabled",
            RecorderStatus::Stopped => "stopped",
            RecorderStatus::Error => "failed",
            RecorderStatus::Running => {
                if self.last_recorder_start_failure.is_some() {
                    return "failed";
                }
                if self.waiting_for_first_snapshot() {
                    return "waiting";
                }
                if self
                    .snapshot
                    .runtime
                    .as_ref()
                    .is_some_and(|runtime| runtime.stale)
                {
                    return "stale";
                }
                "running"
            }
        }
    }

    pub fn recorder_snapshot_freshness(&self) -> &'static str {
        if self.waiting_for_first_snapshot() {
            return "waiting";
        }
        match self.snapshot.runtime.as_ref() {
            Some(runtime) if runtime.stale => "stale",
            Some(_) => "fresh",
            None => "unknown",
        }
    }

    pub fn recorder_snapshot_mode(&self) -> &'static str {
        match self
            .snapshot
            .runtime
            .as_ref()
            .map(|runtime| runtime.refresh_kind.as_str())
        {
            Some("bootstrap") => "bootstrap",
            Some("cached") => "cached",
            Some("live_capture") => "live",
            _ => "unknown",
        }
    }

    pub fn last_successful_snapshot_at(&self) -> Option<&str> {
        self.last_successful_snapshot_at.as_deref()
    }

    pub fn last_recorder_start_failure(&self) -> Option<&str> {
        self.last_recorder_start_failure.as_deref()
    }

    pub fn recent_events(&self) -> Vec<&str> {
        self.event_history
            .iter()
            .rev()
            .map(String::as_str)
            .collect()
    }

    pub fn worker_reconnect_count(&self) -> usize {
        self.snapshot
            .runtime
            .as_ref()
            .map(|runtime| runtime.worker_reconnect_count)
            .unwrap_or(0)
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

    pub fn open_trading_action_overlay_from_positions(&mut self) {
        if self.positions_focus != PositionsFocus::Active {
            self.status_message =
                String::from("Switch to the active positions pane to open a trading action.");
            return;
        }

        let Some(seed) =
            selected_active_position_seed(&self.snapshot, &self.open_position_table_state)
        else {
            self.status_message = String::from("No active position is selected.");
            return;
        };

        self.open_trading_action_overlay(seed);
    }

    pub fn open_trading_action_overlay_from_oddsmatcher(&mut self) {
        let Some(row) = self.selected_oddsmatcher_row().cloned() else {
            self.status_message = String::from("No OddsMatcher row is selected.");
            return;
        };

        self.open_trading_action_overlay_from_matcher_row(
            row,
            TradingActionSource::OddsMatcher,
            "oddsmatcher",
        );
    }

    pub fn open_trading_action_overlay_from_horse_matcher(&mut self) {
        let Some(row) = self.selected_horse_matcher_row().cloned() else {
            self.status_message = String::from("No Horse Matcher row is selected.");
            return;
        };

        self.open_trading_action_overlay_from_matcher_row(
            row,
            TradingActionSource::HorseMatcher,
            "horse_matcher",
        );
    }

    fn open_trading_action_overlay_from_matcher_row(
        &mut self,
        row: OddsMatcherRow,
        source: TradingActionSource,
        note: &str,
    ) {
        let deep_link_url = row
            .lay
            .deep_link
            .clone()
            .filter(|value| !value.trim().is_empty());
        let default_stake = if self.calculator.source.as_ref().is_some_and(|source| {
            source.selection_name == row.selection_name
                && source.event_name == row.event_name
                && source.exchange_name == row.lay.bookmaker.display_name
        }) {
            Some(self.calculator.input.back_stake)
        } else {
            None
        };
        let venue =
            exchange_venue_from_bookmaker(&row.lay.bookmaker.code, &row.lay.bookmaker.display_name)
                .unwrap_or(VenueId::Smarkets);
        let seed = TradingActionSeed {
            source,
            venue,
            source_ref: row.id.clone(),
            event_name: row.event_name.clone(),
            market_name: row.market_name.clone(),
            selection_name: row.selection_name.clone(),
            event_url: None,
            deep_link_url,
            betslip_market_id: row
                .lay
                .bet_slip
                .as_ref()
                .map(|bet_slip| bet_slip.market_id.clone()),
            betslip_selection_id: row
                .lay
                .bet_slip
                .as_ref()
                .map(|bet_slip| bet_slip.selection_id.clone()),
            buy_price: None,
            sell_price: Some(row.lay.odds),
            default_side: TradingActionSide::Sell,
            default_stake,
            source_context: TradingActionSourceContext::default(),
            notes: vec![
                String::from(note),
                format!("rating:{:.1}", row.rating),
                row.bet_request_id
                    .as_ref()
                    .map(|request_id| format!("bet_request:{request_id}"))
                    .unwrap_or_else(|| String::from("bet_request:missing")),
            ],
        };

        self.open_trading_action_overlay(seed);
    }

    pub fn refresh(&mut self) -> Result<()> {
        if self.active_panel == Panel::Trading && self.is_owls_context() {
            return self.refresh_owls_dashboard();
        }
        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::Matcher {
            return self.refresh_matcher();
        }
        self.refresh_provider_snapshot(
            ProviderRequest::RefreshCached,
            "Refresh failed",
            Some(format!(
                "Manual cached refresh completed for {}.",
                self.selected_venue_label()
            )),
        )?;
        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::Stats {
            self.request_matchbook_sync(MatchbookSyncReason::Manual);
        }
        Ok(())
    }

    pub fn refresh_live(&mut self) -> Result<()> {
        if self.active_panel == Panel::Trading && self.is_owls_context() {
            return self.refresh_owls_dashboard();
        }
        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::Matcher {
            return self.refresh_matcher();
        }
        self.refresh_provider_snapshot(
            ProviderRequest::RefreshLive,
            "Live refresh failed",
            Some(format!(
                "Manual live refresh completed for {}.",
                self.selected_venue_label()
            )),
        )?;
        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::Stats {
            self.request_matchbook_sync(MatchbookSyncReason::Manual);
        }
        Ok(())
    }

    pub fn replace_oddsmatcher_rows(&mut self, rows: Vec<OddsMatcherRow>, status_message: String) {
        self.oddsmatcher_rows = rows;
        self.clamp_selected_oddsmatcher_row();
        self.status_message = status_message;
    }

    pub fn replace_horse_matcher_rows(
        &mut self,
        rows: Vec<OddsMatcherRow>,
        status_message: String,
    ) {
        self.horse_matcher_rows = rows;
        self.clamp_selected_horse_matcher_row();
        self.status_message = status_message;
    }

    fn refresh_provider_snapshot(
        &mut self,
        request: ProviderRequest,
        failure_context: &str,
        event_message: Option<String>,
    ) -> Result<()> {
        self.queue_provider_request(ProviderJob {
            request,
            failure_context: String::from(failure_context),
            event_message,
        });
        Ok(())
    }

    fn refresh_owls_dashboard(&mut self) -> Result<()> {
        self.request_owls_sync(OwlsSyncReason::Manual);
        self.status_message = String::from("Owls sync queued.");
        self.status_scroll = 0;
        self.record_event("Owls manual sync queued.");
        Ok(())
    }

    fn queue_provider_request(&mut self, job: ProviderJob) {
        self.drain_provider_results();
        if self.provider_in_flight {
            self.provider_pending_job = Some(match self.provider_pending_job.take() {
                Some(existing)
                    if provider_job_priority(&existing.request)
                        > provider_job_priority(&job.request) =>
                {
                    existing
                }
                _ => job,
            });
            return;
        }
        self.dispatch_provider_request(job);
    }

    fn dispatch_provider_request(&mut self, job: ProviderJob) {
        let request = job.request.clone();
        match self.provider_tx.send(job) {
            Ok(()) => {
                self.provider_in_flight = true;
                self.status_message = provider_queue_message(&request);
                self.status_scroll = 0;
            }
            Err(error) => {
                self.status_message = format!("Provider worker unavailable: {error}");
                self.status_scroll = 0;
                self.record_event("Provider worker unavailable.");
            }
        }
    }

    fn restart_provider_worker(&mut self, provider: Box<dyn ExchangeProvider + Send>) {
        let (provider_tx, provider_rx) = start_provider_worker(provider);
        self.provider_tx = provider_tx;
        self.provider_rx = provider_rx;
        self.provider_in_flight = false;
        self.provider_pending_job = None;
    }

    fn drain_provider_results(&mut self) {
        let mut latest_result = None;
        while let Ok(result) = self.provider_rx.try_recv() {
            latest_result = Some(result);
        }
        let Some(result) = latest_result else {
            return;
        };

        self.provider_in_flight = false;
        match result.result {
            Ok(snapshot) => {
                self.apply_provider_snapshot_result(result.request, snapshot, result.event_message)
            }
            Err(error) => {
                if matches!(result.request, ProviderRequest::LoadDashboard)
                    && self.recorder_status == RecorderStatus::Running
                    && self.last_successful_snapshot_at.is_none()
                {
                    self.status_message =
                        String::from("Recorder started; waiting for first snapshot.");
                    self.status_scroll = 0;
                    self.record_event(format!("{}: {}", result.failure_context, error));
                } else {
                    self.record_provider_error(
                        &result.failure_context,
                        &error,
                        self.selected_venue(),
                    );
                }
            }
        }

        if let Some(job) = self.provider_pending_job.take() {
            self.dispatch_provider_request(job);
        }
    }

    fn apply_provider_snapshot_result(
        &mut self,
        request: ProviderRequest,
        snapshot: ExchangePanelSnapshot,
        event_message: Option<String>,
    ) {
        match request {
            ProviderRequest::LoadHorseMatcher { query } => {
                let Some(market_snapshot) = snapshot.horse_matcher.clone() else {
                    self.status_message =
                        String::from("Horse Matcher refresh failed: missing market data.");
                    self.status_scroll = 0;
                    return;
                };
                match horse_matcher::build_rows(&market_snapshot, &query) {
                    Ok(rows) => {
                        let row_count = rows.len();
                        self.horse_matcher_snapshot = Some(market_snapshot);
                        self.replace_horse_matcher_rows(
                            rows,
                            format!("Loaded {row_count} internal Horse Matcher row(s)."),
                        );
                    }
                    Err(error) => {
                        self.status_message = format!("Horse Matcher refresh failed: {error}");
                        self.status_scroll = 0;
                    }
                }
            }
            _ => {
                self.replace_snapshot(snapshot);
            }
        }

        if let Some(message) = event_message {
            self.record_event(message);
        }
    }

    fn request_matchbook_sync(&mut self, reason: MatchbookSyncReason) {
        self.drain_matchbook_sync_results();
        if self.matchbook_sync_in_flight {
            self.matchbook_sync_pending_reason =
                Some(match (self.matchbook_sync_pending_reason, reason) {
                    (Some(MatchbookSyncReason::Manual), _) | (_, MatchbookSyncReason::Manual) => {
                        MatchbookSyncReason::Manual
                    }
                    _ => MatchbookSyncReason::Background,
                });
            return;
        }
        self.dispatch_matchbook_sync(reason);
    }

    fn dispatch_matchbook_sync(&mut self, reason: MatchbookSyncReason) {
        match self.matchbook_sync_tx.send(MatchbookSyncJob { reason }) {
            Ok(()) => {
                self.matchbook_sync_in_flight = true;
                self.last_matchbook_sync_dispatch_at = Some(Instant::now());
            }
            Err(error) => {
                self.status_message = format!("Matchbook sync worker unavailable: {error}");
                self.status_scroll = 0;
                self.record_event("Matchbook sync worker unavailable.");
            }
        }
    }

    fn queue_oddsmatcher_refresh(&mut self, query: GetBestMatchesVariables) {
        self.drain_oddsmatcher_results();
        if self.oddsmatcher_in_flight {
            self.oddsmatcher_pending_query = Some(query);
            return;
        }
        self.dispatch_oddsmatcher_refresh(query);
    }

    fn dispatch_oddsmatcher_refresh(&mut self, query: GetBestMatchesVariables) {
        match self.oddsmatcher_tx.send(OddsMatcherJob {
            query: query.clone(),
        }) {
            Ok(()) => {
                self.oddsmatcher_in_flight = true;
                self.status_message = String::from("OddsMatcher refresh queued.");
                self.status_scroll = 0;
            }
            Err(error) => {
                self.status_message = format!("OddsMatcher worker unavailable: {error}");
                self.status_scroll = 0;
            }
        }
    }

    fn drain_oddsmatcher_results(&mut self) {
        let mut latest_result = None;
        while let Ok(result) = self.oddsmatcher_rx.try_recv() {
            latest_result = Some(result);
        }
        let Some(result) = latest_result else {
            return;
        };

        self.oddsmatcher_in_flight = false;
        match result.result {
            Ok(rows) => {
                let row_count = rows.len();
                self.replace_oddsmatcher_rows(
                    rows,
                    format!("Loaded {row_count} live OddsMatcher row(s)."),
                );
            }
            Err(error) => {
                self.status_message = format!("OddsMatcher refresh failed: {error}");
                self.status_scroll = 0;
            }
        }

        if let Some(query) = self.oddsmatcher_pending_query.take() {
            self.dispatch_oddsmatcher_refresh(query);
        }
    }

    fn drain_matchbook_sync_results(&mut self) {
        let mut latest_result = None;
        while let Ok(result) = self.matchbook_sync_rx.try_recv() {
            latest_result = Some(result);
        }
        let Some(result) = latest_result else {
            return;
        };

        self.matchbook_sync_in_flight = false;
        match result.state {
            Ok(state) => {
                let first_load = self.matchbook_account_state.is_none();
                self.matchbook_account_state = Some(state.clone());
                if matches!(result.reason, MatchbookSyncReason::Manual) || first_load {
                    self.status_message = state.status_line.clone();
                    self.status_scroll = 0;
                }
                self.record_event(format!("Matchbook {} sync applied.", result.reason.label()));
            }
            Err(error) => {
                if matches!(result.reason, MatchbookSyncReason::Manual) {
                    self.status_message = format!("Matchbook sync failed: {error}");
                    self.status_scroll = 0;
                }
                self.record_event(format!("Matchbook sync failed: {error}"));
            }
        }

        if let Some(reason) = self.matchbook_sync_pending_reason.take() {
            self.dispatch_matchbook_sync(reason);
        }
    }

    pub fn cash_out_next_actionable_bet(&mut self) -> Result<()> {
        let actionable_bet_id =
            next_actionable_cash_out_bet_id(&self.snapshot).ok_or_else(|| {
                color_eyre::eyre::eyre!("No tracked bet is currently marked for cash out.")
            })?;
        self.queue_provider_request(ProviderJob {
            request: ProviderRequest::CashOutTrackedBet {
                bet_id: actionable_bet_id,
            },
            failure_context: String::from("Cash out failed"),
            event_message: Some(String::from("Cash out request completed.")),
        });
        Ok(())
    }

    fn open_trading_action_overlay(&mut self, seed: TradingActionSeed) {
        if !seed.supports_side(seed.default_side) {
            self.status_message =
                String::from("The selected row does not expose an executable quote.");
            return;
        }
        if seed.event_url.as_deref().unwrap_or_default().is_empty()
            && seed.deep_link_url.as_deref().unwrap_or_default().is_empty()
        {
            self.status_message =
                String::from("The selected row does not expose an execution URL or deep link.");
            return;
        }
        let time_in_force = seed.default_time_in_force();
        let risk_report = match seed.evaluate(
            &self.snapshot,
            seed.default_side,
            TradingActionMode::Review,
            seed.default_stake.unwrap_or(10.0),
            time_in_force,
        ) {
            Ok(intent) => intent.risk_report,
            Err(error) => {
                self.status_message = format!("Trading action unavailable: {error}");
                return;
            }
        };
        self.trading_action_overlay = Some(TradingActionOverlayState::new(seed, risk_report));
        if self
            .trading_action_overlay
            .as_ref()
            .is_some_and(|overlay| overlay.seed.venue == VenueId::Matchbook)
        {
            self.request_matchbook_sync(MatchbookSyncReason::Manual);
        }
        self.status_message = String::from("Trading action overlay opened.");
    }

    pub fn start_recorder(&mut self) -> Result<()> {
        self.record_event("Recorder start requested.");
        self.persist_recorder_config()?;
        self.recorder_supervisor.start(&self.recorder_config)?;
        self.recorder_status = self.recorder_supervisor.poll_status();
        self.last_recorder_start_failure = None;
        self.restart_provider_worker((self.make_recorder_provider)(&self.recorder_config));
        self.exchange_list_state.select(None);
        self.record_event("Recorder process started.");
        self.last_recorder_refresh_at = None;
        self.queue_provider_request(ProviderJob {
            request: ProviderRequest::LoadDashboard,
            failure_context: String::from("Recorder dashboard load failed"),
            event_message: Some(String::from("Recorder dashboard loaded.")),
        });
        self.status_message = String::from("Recorder started; waiting for first snapshot.");
        self.status_scroll = 0;
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
        self.record_event("Recorder stop requested.");
        self.recorder_supervisor.stop()?;
        self.recorder_status = RecorderStatus::Disabled;
        self.last_recorder_refresh_at = None;
        self.last_recorder_start_failure = None;
        self.restart_provider_worker((self.make_stub_provider)());
        self.queue_provider_request(ProviderJob {
            request: ProviderRequest::LoadDashboard,
            failure_context: String::from("Stub dashboard load failed"),
            event_message: Some(String::from("Recorder stopped; restored stub dashboard.")),
        });
        self.record_event("Recorder stopped; restored stub dashboard.");
        Ok(())
    }

    fn request_quit(&mut self) {
        self.recorder_status = self.recorder_supervisor.poll_status();
        if self.recorder_status == RecorderStatus::Running {
            if let Err(error) = self.stop_recorder() {
                self.status_message = format!("Recorder stop failed: {error}");
                self.recorder_status = RecorderStatus::Error;
                self.record_event(format!("Recorder stop failed during quit: {error}"));
                return;
            }
        }
        self.record_event("Quit requested.");
        self.running = false;
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

    pub fn reload_horse_matcher_query(&mut self) -> Result<()> {
        let (query, note) = horse_matcher::load_query_or_default(&self.horse_matcher_query_path)?;
        self.horse_matcher_query = query;
        self.horse_matcher_query_note = note;
        self.horse_matcher_editor = HorseMatcherEditorState::default();
        self.horse_matcher_rows.clear();
        self.horse_matcher_table_state.select(None);
        self.status_message = String::from("Reloaded Horse Matcher config from disk.");
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

    pub fn reset_horse_matcher_query(&mut self) -> Result<()> {
        self.horse_matcher_query = HorseMatcherQuery::default();
        self.horse_matcher_editor = HorseMatcherEditorState::default();
        self.horse_matcher_rows.clear();
        self.horse_matcher_table_state.select(None);
        self.persist_horse_matcher_query()?;
        self.status_message = String::from("Reset Horse Matcher config to defaults.");
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
        if self.active_panel != Panel::Trading || self.trading_section != TradingSection::Positions
        {
            self.live_view_overlay_visible = false;
        }
        if self.active_panel != Panel::Trading || !self.is_owls_context() {
            self.markets_overlay_visible = false;
        }
        if self.trading_section != TradingSection::Positions
            && self.trading_section != TradingSection::Matcher
        {
            self.trading_action_overlay = None;
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
        if self.active_panel != Panel::Trading || self.trading_section != TradingSection::Positions
        {
            self.live_view_overlay_visible = false;
        }
        if self.active_panel != Panel::Trading || !self.is_owls_context() {
            self.markets_overlay_visible = false;
        }
        if self.trading_section != TradingSection::Positions
            && self.trading_section != TradingSection::Matcher
        {
            self.trading_action_overlay = None;
        }
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        while self.running {
            self.poll_recorder();
            self.drain_provider_results();
            self.drain_oddsmatcher_results();
            self.poll_owls_dashboard();
            self.poll_matchbook_account();
            terminal.draw(|frame| self.render(frame))?;

            if event::poll(Duration::from_millis(250))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key_code(key.code)
                    }
                    Event::Mouse(mouse) => self.handle_mouse(mouse.kind, mouse.column, mouse.row),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn dispatch_owls_sync(&mut self, reason: OwlsSyncReason) {
        let job = OwlsSyncJob {
            dashboard: self.owls_dashboard.clone(),
            reason,
            focused: self.selected_owls_endpoint_id(),
        };
        match self.owls_sync_tx.send(job) {
            Ok(()) => {
                self.owls_sync_in_flight = true;
                self.last_owls_sync_dispatch_at = Some(Instant::now());
            }
            Err(error) => {
                self.status_message = format!("Owls sync worker unavailable: {error}");
                self.status_scroll = 0;
                self.record_event("Owls sync worker unavailable.");
            }
        }
    }

    fn request_owls_sync(&mut self, reason: OwlsSyncReason) {
        self.drain_owls_sync_results();
        if self.owls_sync_in_flight {
            self.owls_sync_pending_reason = Some(match (self.owls_sync_pending_reason, reason) {
                (Some(OwlsSyncReason::Manual), _) | (_, OwlsSyncReason::Manual) => {
                    OwlsSyncReason::Manual
                }
                _ => OwlsSyncReason::Background,
            });
            return;
        }
        self.dispatch_owls_sync(reason);
    }

    fn drain_owls_sync_results(&mut self) {
        let mut latest_result = None;
        while let Ok(result) = self.owls_sync_rx.try_recv() {
            latest_result = Some(result);
        }
        let Some(result) = latest_result else {
            return;
        };

        self.owls_sync_in_flight = false;
        let outcome = result.outcome;
        self.owls_dashboard = outcome.dashboard;
        self.clamp_selected_owls_endpoint();

        if outcome.changed || matches!(result.reason, OwlsSyncReason::Manual) {
            self.status_message = self.owls_dashboard.status_line.clone();
            self.status_scroll = 0;
        }
        if outcome.changed {
            self.record_event(format!(
                "Owls {} sync applied {} changes after {} checks.",
                result.reason.label(),
                outcome.changed_count,
                outcome.checked_count
            ));
        }

        if let Some(reason) = self.owls_sync_pending_reason.take() {
            self.dispatch_owls_sync(reason);
        }
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        self.clear_mouse_targets();
        ui::render(frame, self);
    }

    pub fn handle_key(&mut self, key_code: KeyCode) {
        if self.is_trading_action_overlay_active() {
            match key_code {
                KeyCode::Esc => {
                    self.close_trading_action_overlay("Cancelled trading action.");
                    return;
                }
                KeyCode::Backspace => {
                    if self.is_trading_action_overlay_editing() {
                        self.trading_action_backspace();
                    }
                    return;
                }
                KeyCode::Enter => {
                    if self.is_trading_action_overlay_editing() {
                        if let Err(error) = self.apply_trading_action_edit() {
                            self.status_message = format!("Trading action input error: {error}");
                        }
                    } else if let Err(error) = self.activate_trading_action_field() {
                        self.status_message = format!("Trading action failed: {error}");
                    }
                    return;
                }
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('[') => {
                    if let Err(error) = self.trading_action_shift(false) {
                        self.status_message = format!("Trading action failed: {error}");
                    }
                    return;
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(']') => {
                    if let Err(error) = self.trading_action_shift(true) {
                        self.status_message = format!("Trading action failed: {error}");
                    }
                    return;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(overlay) = self.trading_action_overlay.as_mut() {
                        overlay.select_previous_field();
                        self.status_message = format!(
                            "Trading action field set to {}.",
                            overlay.selected_field().label()
                        );
                    }
                    return;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(overlay) = self.trading_action_overlay.as_mut() {
                        overlay.select_next_field();
                        self.status_message = format!(
                            "Trading action field set to {}.",
                            overlay.selected_field().label()
                        );
                    }
                    return;
                }
                KeyCode::Char(character) => {
                    if self.is_trading_action_overlay_editing() {
                        if matches!(character, '0'..='9' | '.') {
                            self.trading_action_push_char(character);
                        }
                        return;
                    }
                    if self
                        .trading_action_overlay
                        .as_ref()
                        .map(|overlay| overlay.selected_field == TradingActionField::Stake)
                        .unwrap_or(false)
                        && matches!(character, '0'..='9' | '.')
                    {
                        self.begin_trading_action_edit();
                        self.trading_action_push_char(character);
                    }
                    return;
                }
                _ => return,
            }
        }

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

        if self.is_horse_matcher_editing_context() {
            match key_code {
                KeyCode::Esc => {
                    self.cancel_horse_matcher_edit();
                    return;
                }
                KeyCode::Enter => {
                    if let Err(error) = self.apply_horse_matcher_edit() {
                        self.status_message = format!("Horse Matcher filter error: {error}");
                    }
                    return;
                }
                KeyCode::Backspace => {
                    self.horse_matcher_backspace();
                    return;
                }
                KeyCode::Char(character) => {
                    self.horse_matcher_push_char(character);
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

        if self.is_horse_matcher_context() {
            match key_code {
                KeyCode::Left => {
                    self.focus_horse_matcher_filters();
                    return;
                }
                KeyCode::Right => {
                    self.focus_horse_matcher_results();
                    return;
                }
                _ => {}
            }
        }

        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::Positions
        {
            if key_code == KeyCode::Tab {
                self.toggle_positions_focus();
                return;
            }
            if key_code == KeyCode::Char('v') {
                self.toggle_live_view_overlay();
                return;
            }
        }

        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::Matcher {
            if key_code == KeyCode::Tab {
                self.cycle_matcher_view(true);
                return;
            }
        }

        if self.is_calculator_context() && key_code == KeyCode::Tab {
            self.cycle_calculator_tool(true);
            return;
        }

        match key_code {
            KeyCode::Char('?') => self.toggle_keymap_overlay(),
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Esc => {
                if self.keymap_overlay_visible {
                    self.keymap_overlay_visible = false;
                } else if self.markets_overlay_visible {
                    self.markets_overlay_visible = false;
                } else if self.live_view_overlay_visible {
                    self.live_view_overlay_visible = false;
                } else {
                    self.request_quit();
                }
            }
            KeyCode::Char('o') => self.toggle_observability_panel(),
            KeyCode::Right | KeyCode::Char('l') => self.next_section(),
            KeyCode::Left | KeyCode::Char('h') => self.previous_section(),
            KeyCode::Enter => {
                if self.is_oddsmatcher_filters_context() {
                    self.begin_oddsmatcher_edit();
                } else if self.is_oddsmatcher_results_context() {
                    self.load_calculator_from_selected_oddsmatcher();
                } else if self.is_horse_matcher_filters_context() {
                    self.begin_horse_matcher_edit();
                } else if self.is_horse_matcher_results_context() {
                    self.load_calculator_from_selected_horse_matcher();
                } else if self.active_panel == Panel::Trading && self.is_owls_context() {
                    if let Some(endpoint) = self.selected_owls_endpoint() {
                        self.status_message = format!(
                            "{} {} [{}] {}",
                            endpoint.method, endpoint.path, endpoint.status, endpoint.description
                        );
                        self.markets_overlay_visible = true;
                    }
                } else if self.active_panel == Panel::Trading
                    && self.trading_section == TradingSection::Positions
                    && self.positions_focus == PositionsFocus::Active
                {
                    self.open_trading_action_overlay_from_positions();
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
            KeyCode::Char('p') => {
                if self.is_oddsmatcher_results_context() {
                    self.open_trading_action_overlay_from_oddsmatcher();
                } else if self.is_horse_matcher_results_context() {
                    self.open_trading_action_overlay_from_horse_matcher();
                } else if self.active_panel == Panel::Trading
                    && self.trading_section == TradingSection::Positions
                    && self.positions_focus == PositionsFocus::Active
                {
                    self.open_trading_action_overlay_from_positions();
                }
            }
            KeyCode::Char('r') => {
                if let Err(error) = self.refresh() {
                    self.status_message = format!("Refresh failed: {error}");
                }
            }
            KeyCode::Char('R') => {
                if let Err(error) = self.refresh_live() {
                    self.status_message = format!("Live refresh failed: {error}");
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
                if self.is_owls_context() {
                    self.cycle_owls_sport(false);
                } else if self.is_oddsmatcher_filters_context() {
                    if let Err(error) = self.cycle_oddsmatcher_suggestion(false) {
                        self.status_message = format!("OddsMatcher suggestion failed: {error}");
                    }
                } else if self.is_horse_matcher_filters_context() {
                    if let Err(error) = self.cycle_horse_matcher_suggestion(false) {
                        self.status_message = format!("Horse Matcher suggestion failed: {error}");
                    }
                } else if self.is_recorder_context() {
                    if let Err(error) = self.cycle_recorder_suggestion(false) {
                        self.status_message = format!("Recorder suggestion failed: {error}");
                    }
                }
            }
            KeyCode::Char(']') => {
                if self.is_owls_context() {
                    self.cycle_owls_sport(true);
                } else if self.is_oddsmatcher_filters_context() {
                    if let Err(error) = self.cycle_oddsmatcher_suggestion(true) {
                        self.status_message = format!("OddsMatcher suggestion failed: {error}");
                    }
                } else if self.is_horse_matcher_filters_context() {
                    if let Err(error) = self.cycle_horse_matcher_suggestion(true) {
                        self.status_message = format!("Horse Matcher suggestion failed: {error}");
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
                } else if self.is_horse_matcher_context() {
                    if let Err(error) = self.reload_horse_matcher_query() {
                        self.status_message = format!("Horse Matcher reload failed: {error}");
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
                } else if self.is_horse_matcher_context() {
                    if let Err(error) = self.reset_horse_matcher_query() {
                        self.status_message = format!("Horse Matcher reset failed: {error}");
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
                    self.last_recorder_start_failure = Some(error.to_string());
                    self.record_event(format!("Recorder start failed: {error}"));
                }
            }
            KeyCode::Char('x') => {
                if let Err(error) = self.stop_recorder() {
                    self.status_message = format!("Recorder stop failed: {error}");
                    self.recorder_status = RecorderStatus::Error;
                    self.record_event(format!("Recorder stop failed: {error}"));
                }
            }
            KeyCode::PageDown => {
                if self.supports_status_scroll() {
                    self.scroll_status_down(4);
                }
            }
            KeyCode::PageUp => {
                if self.supports_status_scroll() {
                    self.scroll_status_up(4);
                }
            }
            KeyCode::Home => {
                if self.supports_status_scroll() {
                    self.status_scroll = 0;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => match (self.active_panel, self.trading_section) {
                (Panel::Trading, TradingSection::Positions) => self.select_next_positions_row(),
                (Panel::Trading, TradingSection::Markets)
                | (Panel::Trading, TradingSection::Live)
                | (Panel::Trading, TradingSection::Props) => self.select_next_owls_endpoint(),
                (Panel::Trading, TradingSection::Matcher) => self.select_next_matcher_row(),
                (Panel::Trading, TradingSection::Calculator) => {
                    self.calculator.editor.select_next_field()
                }
                (Panel::Trading, TradingSection::Recorder) => {
                    self.recorder_editor.select_next_field()
                }
                _ => {}
            },
            KeyCode::Up | KeyCode::Char('k') => match (self.active_panel, self.trading_section) {
                (Panel::Trading, TradingSection::Positions) => self.select_previous_positions_row(),
                (Panel::Trading, TradingSection::Markets)
                | (Panel::Trading, TradingSection::Live)
                | (Panel::Trading, TradingSection::Props) => self.select_previous_owls_endpoint(),
                (Panel::Trading, TradingSection::Matcher) => self.select_previous_matcher_row(),
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

    fn supports_status_scroll(&self) -> bool {
        self.active_panel == Panel::Trading
            && matches!(
                self.trading_section,
                TradingSection::Positions | TradingSection::Recorder
            )
    }

    fn is_trading_action_overlay_active(&self) -> bool {
        self.trading_action_overlay.is_some()
    }

    fn is_trading_action_overlay_editing(&self) -> bool {
        self.trading_action_overlay
            .as_ref()
            .map(|overlay| overlay.editing)
            .unwrap_or(false)
    }

    fn is_oddsmatcher_context(&self) -> bool {
        self.active_panel == Panel::Trading
            && self.trading_section == TradingSection::Matcher
            && self.matcher_view == MatcherView::Odds
    }

    fn is_horse_matcher_context(&self) -> bool {
        self.active_panel == Panel::Trading
            && self.trading_section == TradingSection::Matcher
            && self.matcher_view == MatcherView::Horse
    }

    fn is_owls_context(&self) -> bool {
        self.active_panel == Panel::Trading
            && matches!(
                self.trading_section,
                TradingSection::Markets | TradingSection::Live | TradingSection::Props
            )
    }

    fn visible_owls_groups(&self) -> &'static [OwlsEndpointGroup] {
        match self.trading_section {
            TradingSection::Live => &[
                OwlsEndpointGroup::Realtime,
                OwlsEndpointGroup::Scores,
                OwlsEndpointGroup::Odds,
            ],
            TradingSection::Props => &[OwlsEndpointGroup::Props, OwlsEndpointGroup::History],
            _ => &OwlsEndpointGroup::ALL,
        }
    }

    fn is_oddsmatcher_filters_context(&self) -> bool {
        self.is_oddsmatcher_context() && self.oddsmatcher_focus == OddsMatcherFocus::Filters
    }

    fn is_horse_matcher_filters_context(&self) -> bool {
        self.is_horse_matcher_context() && self.horse_matcher_focus == OddsMatcherFocus::Filters
    }

    fn is_oddsmatcher_results_context(&self) -> bool {
        self.is_oddsmatcher_context() && self.oddsmatcher_focus == OddsMatcherFocus::Results
    }

    fn is_horse_matcher_results_context(&self) -> bool {
        self.is_horse_matcher_context() && self.horse_matcher_focus == OddsMatcherFocus::Results
    }

    fn is_oddsmatcher_editing_context(&self) -> bool {
        self.is_oddsmatcher_filters_context() && self.oddsmatcher_editor.editing
    }

    fn is_horse_matcher_editing_context(&self) -> bool {
        self.is_horse_matcher_filters_context() && self.horse_matcher_editor.editing
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
        let active_row_count = active_position_row_count(&self.snapshot);
        if active_row_count == 0 {
            self.open_position_table_state.select(None);
        } else {
            match self.open_position_table_state.selected() {
                Some(index) if index < active_row_count => {}
                _ => self.open_position_table_state.select(Some(0)),
            }
        }

        if self.snapshot.historical_positions.is_empty() {
            self.historical_position_table_state.select(None);
        } else {
            match self.historical_position_table_state.selected() {
                Some(index) if index < self.snapshot.historical_positions.len() => {}
                _ => self.historical_position_table_state.select(Some(0)),
            }
        }

        if self.positions_focus == PositionsFocus::Active && active_row_count == 0 {
            self.positions_focus = if self.snapshot.historical_positions.is_empty() {
                PositionsFocus::Active
            } else {
                PositionsFocus::Historical
            };
        } else if self.positions_focus == PositionsFocus::Historical
            && self.snapshot.historical_positions.is_empty()
        {
            self.positions_focus = PositionsFocus::Active;
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

    fn clamp_selected_horse_matcher_row(&mut self) {
        if self.horse_matcher_rows.is_empty() {
            self.horse_matcher_table_state.select(None);
            return;
        }

        match self.horse_matcher_table_state.selected() {
            Some(index) if index < self.horse_matcher_rows.len() => {}
            _ => self.horse_matcher_table_state.select(Some(0)),
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
        let active_row_count = active_position_row_count(&self.snapshot);
        if active_row_count == 0 {
            self.open_position_table_state.select(None);
            return;
        }

        let next_index = match self.open_position_table_state.selected() {
            Some(index) if index + 1 < active_row_count => index + 1,
            Some(index) => index,
            None => 0,
        };

        self.open_position_table_state.select(Some(next_index));
    }

    pub fn select_previous_open_position_row(&mut self) {
        if active_position_row_count(&self.snapshot) == 0 {
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

    pub fn select_next_owls_endpoint(&mut self) {
        let visible_count = self.visible_owls_endpoints().len();
        if visible_count == 0 {
            self.owls_endpoint_table_state.select(None);
            return;
        }

        let next_index = match self.owls_endpoint_table_state.selected() {
            Some(index) if index + 1 < visible_count => index + 1,
            Some(index) => index,
            None => 0,
        };

        self.owls_endpoint_table_state.select(Some(next_index));
    }

    pub fn select_previous_owls_endpoint(&mut self) {
        if self.visible_owls_endpoints().is_empty() {
            self.owls_endpoint_table_state.select(None);
            return;
        }

        let previous_index = match self.owls_endpoint_table_state.selected() {
            Some(index) if index > 0 => index - 1,
            Some(index) => index,
            None => 0,
        };

        self.owls_endpoint_table_state.select(Some(previous_index));
    }

    pub fn select_next_positions_row(&mut self) {
        match self.positions_focus {
            PositionsFocus::Active => self.select_next_open_position_row(),
            PositionsFocus::Historical => self.select_next_historical_position_row(),
        }
    }

    pub fn select_previous_positions_row(&mut self) {
        match self.positions_focus {
            PositionsFocus::Active => self.select_previous_open_position_row(),
            PositionsFocus::Historical => self.select_previous_historical_position_row(),
        }
    }

    pub fn select_next_matcher_row(&mut self) {
        match self.matcher_view {
            MatcherView::Odds => {
                if self.oddsmatcher_focus == OddsMatcherFocus::Filters {
                    self.oddsmatcher_editor.select_next_field();
                } else {
                    self.select_next_oddsmatcher_row();
                }
            }
            MatcherView::Horse => {
                if self.horse_matcher_focus == OddsMatcherFocus::Filters {
                    self.horse_matcher_editor.select_next_field();
                } else {
                    self.select_next_horse_matcher_row();
                }
            }
            MatcherView::Acca => {}
        }
    }

    pub fn select_previous_matcher_row(&mut self) {
        match self.matcher_view {
            MatcherView::Odds => {
                if self.oddsmatcher_focus == OddsMatcherFocus::Filters {
                    self.oddsmatcher_editor.select_previous_field();
                } else {
                    self.select_previous_oddsmatcher_row();
                }
            }
            MatcherView::Horse => {
                if self.horse_matcher_focus == OddsMatcherFocus::Filters {
                    self.horse_matcher_editor.select_previous_field();
                } else {
                    self.select_previous_horse_matcher_row();
                }
            }
            MatcherView::Acca => {}
        }
    }

    pub fn toggle_positions_focus(&mut self) {
        self.positions_focus = match self.positions_focus {
            PositionsFocus::Active if !self.snapshot.historical_positions.is_empty() => {
                PositionsFocus::Historical
            }
            PositionsFocus::Historical if active_position_row_count(&self.snapshot) > 0 => {
                PositionsFocus::Active
            }
            other => other,
        };

        match self.positions_focus {
            PositionsFocus::Active => {
                if self.open_position_table_state.selected().is_none()
                    && active_position_row_count(&self.snapshot) > 0
                {
                    self.open_position_table_state.select(Some(0));
                }
            }
            PositionsFocus::Historical => {
                if self.historical_position_table_state.selected().is_none()
                    && !self.snapshot.historical_positions.is_empty()
                {
                    self.historical_position_table_state.select(Some(0));
                }
            }
        }
    }

    fn select_next_historical_position_row(&mut self) {
        if self.snapshot.historical_positions.is_empty() {
            self.historical_position_table_state.select(None);
            return;
        }

        let next_index = match self.historical_position_table_state.selected() {
            Some(index) if index + 1 < self.snapshot.historical_positions.len() => index + 1,
            Some(index) => index,
            None => 0,
        };

        self.historical_position_table_state
            .select(Some(next_index));
    }

    fn select_previous_historical_position_row(&mut self) {
        if self.snapshot.historical_positions.is_empty() {
            self.historical_position_table_state.select(None);
            return;
        }

        let previous_index = match self.historical_position_table_state.selected() {
            Some(index) if index > 0 => index - 1,
            Some(index) => index,
            None => 0,
        };

        self.historical_position_table_state
            .select(Some(previous_index));
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

    pub fn select_next_horse_matcher_row(&mut self) {
        if self.horse_matcher_rows.is_empty() {
            self.horse_matcher_table_state.select(None);
            return;
        }

        let next_index = match self.horse_matcher_table_state.selected() {
            Some(index) if index + 1 < self.horse_matcher_rows.len() => index + 1,
            Some(index) => index,
            None => 0,
        };

        self.horse_matcher_table_state.select(Some(next_index));
    }

    pub fn select_previous_horse_matcher_row(&mut self) {
        if self.horse_matcher_rows.is_empty() {
            self.horse_matcher_table_state.select(None);
            return;
        }

        let previous_index = match self.horse_matcher_table_state.selected() {
            Some(index) if index > 0 => index - 1,
            Some(index) => index,
            None => 0,
        };

        self.horse_matcher_table_state.select(Some(previous_index));
    }

    fn clamp_selected_owls_endpoint(&mut self) {
        let visible_count = self.visible_owls_endpoints().len();
        if visible_count == 0 {
            self.owls_endpoint_table_state.select(None);
            return;
        }

        match self.owls_endpoint_table_state.selected() {
            Some(index) if index < visible_count => {}
            _ => self.owls_endpoint_table_state.select(Some(0)),
        }
    }

    fn sync_selected_venue(&mut self) {
        let Some(selected_index) = self.exchange_list_state.selected() else {
            return;
        };
        let Some(venue) = self.snapshot.venues.get(selected_index) else {
            return;
        };

        self.snapshot.selected_venue = Some(venue.id);
        self.queue_provider_request(ProviderJob {
            request: ProviderRequest::SelectVenue(venue.id),
            failure_context: String::from("Venue sync failed"),
            event_message: Some(format!("Selected venue {}.", venue.id.as_str())),
        });
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
            let previous_status = self.recorder_status.clone();
            self.recorder_status = next_status.clone();
            self.record_event(format!(
                "Recorder status changed from {previous_status:?} to {next_status:?}."
            ));
            if matches!(next_status, RecorderStatus::Stopped | RecorderStatus::Error) {
                self.status_message = format!("Recorder status changed to {next_status:?}.");
                self.status_scroll = 0;
                self.last_recorder_refresh_at = None;
            }
        }

        if self.recorder_status == RecorderStatus::Running && self.recorder_refresh_due() {
            self.last_recorder_refresh_at = Some(Instant::now());
            let request = self.recorder_auto_refresh_request();
            let _ = self.refresh_provider_snapshot(request, "Refresh failed", None);
        }
    }

    fn recorder_auto_refresh_request(&self) -> ProviderRequest {
        match self.selected_venue() {
            Some(VenueId::Smarkets) | None => ProviderRequest::RefreshCached,
            Some(_) => ProviderRequest::RefreshLive,
        }
    }

    fn poll_owls_dashboard(&mut self) {
        self.drain_owls_sync_results();
        if self.active_panel != Panel::Trading || !self.is_owls_context() {
            return;
        }
        if self.owls_sync_in_flight {
            return;
        }
        if self
            .last_owls_sync_dispatch_at
            .is_some_and(|last| last.elapsed() < OWLS_SYNC_DISPATCH_INTERVAL)
        {
            return;
        }
        self.dispatch_owls_sync(OwlsSyncReason::Background);
    }

    fn poll_matchbook_account(&mut self) {
        self.drain_matchbook_sync_results();
        let should_sync = self.active_panel == Panel::Trading
            && (self.trading_section == TradingSection::Stats
                || self
                    .trading_action_overlay
                    .as_ref()
                    .is_some_and(|overlay| overlay.seed.venue == VenueId::Matchbook));
        if !should_sync || self.matchbook_sync_in_flight {
            return;
        }
        if self
            .last_matchbook_sync_dispatch_at
            .is_some_and(|last| last.elapsed() < MATCHBOOK_SYNC_DISPATCH_INTERVAL)
        {
            return;
        }
        self.dispatch_matchbook_sync(MatchbookSyncReason::Background);
    }

    fn handle_mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) {
        match kind {
            MouseEventKind::ScrollDown => self.handle_key(KeyCode::Down),
            MouseEventKind::ScrollUp => self.handle_key(KeyCode::Up),
            MouseEventKind::Down(MouseButton::Left) => {
                let selected_target = self.mouse_targets.iter().find(|target| {
                    column >= target.rect.x
                        && column < target.rect.x.saturating_add(target.rect.width)
                        && row >= target.rect.y
                        && row < target.rect.y.saturating_add(target.rect.height)
                });
                if let Some(target) = selected_target {
                    match target.kind {
                        MouseTargetKind::TradingSection(section) => {
                            self.set_trading_section(section)
                        }
                        MouseTargetKind::MatcherView(view) => self.matcher_view = view,
                        MouseTargetKind::CalculatorTool(tool) => self.calculator_tool = tool,
                    }
                }
            }
            _ => {}
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
        self.status_scroll = 0;
        self.snapshot.status_line = message.clone();
        self.snapshot.worker.status = crate::domain::WorkerStatus::Error;
        self.snapshot.worker.detail = message.clone();
        self.record_event(message.clone());

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
        let had_successful_snapshot = self.last_successful_snapshot_at.is_some();
        let previous_reconnect_count = self.worker_reconnect_count();
        let previous_decision_counts = decision_status_counts(&self.snapshot);
        self.snapshot = normalize_snapshot(
            preserve_cached_snapshot_state(&self.snapshot, snapshot),
            &self.recorder_config.disabled_venues,
        );
        if let Some(updated_at) = runtime_updated_at(&self.snapshot) {
            self.last_successful_snapshot_at = Some(updated_at.to_string());
        }
        let reconnect_count = self.worker_reconnect_count();
        if reconnect_count > previous_reconnect_count {
            self.record_event(format!(
                "Worker session recovered; reconnect count is now {reconnect_count}."
            ));
        }
        if !had_successful_snapshot && self.last_successful_snapshot_at.is_some() {
            self.record_event("Received first successful recorder snapshot.");
        }
        let current_decision_counts = decision_status_counts(&self.snapshot);
        if previous_decision_counts != current_decision_counts {
            self.record_event(format!(
                "Decision queue changed: {}.",
                format_decision_status_counts(&current_decision_counts)
            ));
        }
        self.status_message = self.snapshot.status_line.clone();
        self.status_scroll = 0;
        self.clamp_selected_exchange_row();
        self.clamp_selected_open_position_row();
        self.clamp_selected_oddsmatcher_row();
        self.clamp_selected_horse_matcher_row();
        if active_position_row_count(&self.snapshot) == 0 {
            self.live_view_overlay_visible = false;
        }
        self.trading_action_overlay = None;
    }

    fn recorder_refresh_due(&self) -> bool {
        let refresh_interval = self.recorder_refresh_interval();
        self.last_recorder_refresh_at
            .is_none_or(|last| last.elapsed() >= refresh_interval)
    }

    fn recorder_refresh_interval(&self) -> Duration {
        if self.waiting_for_first_snapshot() {
            return RECORDER_REFRESH_INTERVAL_BOOTSTRAP;
        }
        if active_position_row_count(&self.snapshot) > 0 {
            return RECORDER_REFRESH_INTERVAL_ACTIVE;
        }
        RECORDER_REFRESH_INTERVAL_IDLE
    }

    fn waiting_for_first_snapshot(&self) -> bool {
        self.recorder_status == RecorderStatus::Running
            && (self
                .status_message
                .to_ascii_lowercase()
                .contains("waiting for first snapshot")
                || self.snapshot.worker.status == WorkerStatus::Busy)
    }

    fn record_event(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self
            .event_history
            .back()
            .is_some_and(|last| last == &message)
        {
            return;
        }
        if self.event_history.len() == MAX_EVENT_HISTORY {
            self.event_history.pop_front();
        }
        self.event_history.push_back(message);
    }

    fn selected_venue_label(&self) -> String {
        self.selected_venue()
            .map(|venue| venue.as_str().to_string())
            .unwrap_or_else(|| String::from("current venue"))
    }

    fn scroll_status_down(&mut self, lines: u16) {
        self.status_scroll = self.status_scroll.saturating_add(lines);
    }

    fn scroll_status_up(&mut self, lines: u16) {
        self.status_scroll = self.status_scroll.saturating_sub(lines);
    }

    fn begin_recorder_edit(&mut self) {
        let field = self.recorder_editor.selected_field();
        self.recorder_editor.buffer = field.display_value(&self.recorder_config);
        self.recorder_editor.editing = true;
        self.recorder_editor.replace_on_input = true;
        self.status_message = format!("Editing recorder {}.", field.label());
        self.status_scroll = 0;
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
        self.record_event(message.to_string());

        if self.recorder_status == RecorderStatus::Running {
            self.record_event("Restarting recorder to apply config change.");
            self.recorder_supervisor.stop()?;
            self.recorder_supervisor.start(&self.recorder_config)?;
            self.recorder_status = self.recorder_supervisor.poll_status();
            self.restart_provider_worker((self.make_recorder_provider)(&self.recorder_config));
            self.exchange_list_state.select(None);
            self.last_recorder_refresh_at = None;
            self.status_message = format!("{message} Restarted recorder; dashboard reload queued.");
            self.status_scroll = 0;
            self.queue_provider_request(ProviderJob {
                request: ProviderRequest::LoadDashboard,
                failure_context: String::from("Recorder dashboard load failed"),
                event_message: Some(String::from(
                    "Recorder restart completed after config change.",
                )),
            });
            return Ok(());
        }

        self.status_message = String::from(message);
        self.status_scroll = 0;
        Ok(())
    }

    fn close_trading_action_overlay(&mut self, message: &str) {
        self.trading_action_overlay = None;
        self.status_message = String::from(message);
    }

    fn refresh_trading_action_risk_report(&mut self) -> Result<()> {
        let Some(overlay) = self.trading_action_overlay.as_mut() else {
            return Ok(());
        };
        let stake = overlay.parsed_stake()?;
        let preview = overlay.seed.evaluate(
            &self.snapshot,
            overlay.side,
            overlay.mode,
            stake,
            overlay.time_in_force,
        )?;
        overlay.risk_report = preview.risk_report;
        Ok(())
    }

    fn begin_trading_action_edit(&mut self) {
        let Some(overlay) = self.trading_action_overlay.as_mut() else {
            return;
        };
        if overlay.selected_field != TradingActionField::Stake {
            return;
        }
        overlay.editing = true;
        overlay.replace_on_input = true;
        self.status_message = String::from("Editing trading action stake.");
    }

    fn apply_trading_action_edit(&mut self) -> Result<()> {
        let buffer = {
            let Some(overlay) = self.trading_action_overlay.as_mut() else {
                return Ok(());
            };
            if overlay.selected_field != TradingActionField::Stake {
                return Ok(());
            }
            let parsed = overlay.parsed_stake()?;
            overlay.buffer = format_decimal(parsed);
            overlay.editing = false;
            overlay.replace_on_input = false;
            overlay.buffer.clone()
        };
        self.refresh_trading_action_risk_report()?;
        self.status_message = format!("Trading action stake set to {buffer}.");
        Ok(())
    }

    fn trading_action_push_char(&mut self, character: char) {
        let Some(overlay) = self.trading_action_overlay.as_mut() else {
            return;
        };
        if overlay.replace_on_input {
            overlay.buffer.clear();
            overlay.replace_on_input = false;
        }
        overlay.buffer.push(character);
    }

    fn trading_action_backspace(&mut self) {
        let Some(overlay) = self.trading_action_overlay.as_mut() else {
            return;
        };
        if overlay.replace_on_input {
            overlay.buffer.clear();
            overlay.replace_on_input = false;
            return;
        }
        overlay.buffer.pop();
    }

    fn trading_action_shift(&mut self, forward: bool) -> Result<()> {
        let Some(overlay) = self.trading_action_overlay.as_mut() else {
            return Ok(());
        };

        match overlay.selected_field {
            TradingActionField::Mode => {
                let index = TradingActionMode::ALL
                    .iter()
                    .position(|candidate| candidate == &overlay.mode)
                    .unwrap_or(0);
                let next_index = if forward {
                    (index + 1) % TradingActionMode::ALL.len()
                } else if index == 0 {
                    TradingActionMode::ALL.len() - 1
                } else {
                    index - 1
                };
                overlay.mode = TradingActionMode::ALL[next_index];
                self.status_message =
                    format!("Trading action mode set to {}.", overlay.mode.label());
            }
            TradingActionField::Side => {
                if !overlay.can_cycle_side() {
                    return Err(color_eyre::eyre::eyre!(
                        "The selected source only exposes one executable side."
                    ));
                }
                let sides = TradingActionSide::ALL
                    .into_iter()
                    .filter(|side| overlay.seed.supports_side(*side))
                    .collect::<Vec<_>>();
                let index = sides
                    .iter()
                    .position(|candidate| candidate == &overlay.side)
                    .unwrap_or(0);
                let next_index = if forward {
                    (index + 1) % sides.len()
                } else if index == 0 {
                    sides.len() - 1
                } else {
                    index - 1
                };
                overlay.side = sides[next_index];
                self.status_message =
                    format!("Trading action side set to {}.", overlay.side.label());
            }
            TradingActionField::TimeInForce => {
                let index = TradingTimeInForce::ALL
                    .iter()
                    .position(|candidate| candidate == &overlay.time_in_force)
                    .unwrap_or(0);
                let next_index = if forward {
                    (index + 1) % TradingTimeInForce::ALL.len()
                } else if index == 0 {
                    TradingTimeInForce::ALL.len() - 1
                } else {
                    index - 1
                };
                overlay.time_in_force = TradingTimeInForce::ALL[next_index];
                self.status_message = format!(
                    "Trading action order policy set to {}.",
                    overlay.time_in_force.label()
                );
            }
            TradingActionField::Stake => self.begin_trading_action_edit(),
            TradingActionField::Execute => {
                if forward {
                    self.execute_trading_action()?;
                }
            }
        }
        self.refresh_trading_action_risk_report()?;
        Ok(())
    }

    fn activate_trading_action_field(&mut self) -> Result<()> {
        let Some(selected_field) = self
            .trading_action_overlay
            .as_ref()
            .map(|overlay| overlay.selected_field)
        else {
            return Ok(());
        };
        match selected_field {
            TradingActionField::Mode => self.trading_action_shift(true),
            TradingActionField::Side => self.trading_action_shift(true),
            TradingActionField::TimeInForce => self.trading_action_shift(true),
            TradingActionField::Stake => {
                self.begin_trading_action_edit();
                Ok(())
            }
            TradingActionField::Execute => self.execute_trading_action(),
        }
    }

    fn execute_trading_action(&mut self) -> Result<()> {
        let overlay = self
            .trading_action_overlay
            .clone()
            .ok_or_else(|| color_eyre::eyre::eyre!("Trading action overlay is not open."))?;
        let stake = overlay.parsed_stake()?;
        let request_id = new_trading_action_request_id(overlay.seed.source);
        let intent = overlay.seed.build_intent(
            &self.snapshot,
            request_id.clone(),
            overlay.side,
            overlay.mode,
            stake,
            overlay.time_in_force,
        )?;
        self.queue_provider_request(ProviderJob {
            request: ProviderRequest::ExecuteTradingAction { intent },
            failure_context: String::from("Trading action failed"),
            event_message: Some(String::from("Trading action completed.")),
        });
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

    fn focus_horse_matcher_filters(&mut self) {
        self.horse_matcher_focus = OddsMatcherFocus::Filters;
        self.status_message = String::from("Horse Matcher focus set to filters.");
    }

    fn focus_horse_matcher_results(&mut self) {
        self.horse_matcher_focus = OddsMatcherFocus::Results;
        self.status_message = String::from("Horse Matcher focus set to results.");
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

    fn begin_horse_matcher_edit(&mut self) {
        let field = self.horse_matcher_editor.selected_field();
        self.horse_matcher_editor.buffer = field.display_value(&self.horse_matcher_query);
        self.horse_matcher_editor.editing = true;
        self.horse_matcher_editor.replace_on_input = true;
        self.status_message = format!("Editing Horse Matcher {}.", field.label());
    }

    fn apply_horse_matcher_edit(&mut self) -> Result<()> {
        let field = self.horse_matcher_editor.selected_field();
        let value = self.horse_matcher_editor.buffer.clone();
        field.apply_value(&mut self.horse_matcher_query, &value)?;
        self.horse_matcher_editor.editing = false;
        self.horse_matcher_editor.buffer.clear();
        self.horse_matcher_editor.replace_on_input = false;
        self.horse_matcher_rows.clear();
        self.horse_matcher_table_state.select(None);
        self.persist_horse_matcher_query()?;
        self.status_message = format!(
            "Updated Horse Matcher {} and saved config. Press r to refresh.",
            field.label()
        );
        Ok(())
    }

    fn cancel_horse_matcher_edit(&mut self) {
        self.horse_matcher_editor.editing = false;
        self.horse_matcher_editor.buffer.clear();
        self.horse_matcher_editor.replace_on_input = false;
        self.status_message = String::from("Cancelled Horse Matcher edit.");
    }

    fn horse_matcher_push_char(&mut self, character: char) {
        if self.horse_matcher_editor.replace_on_input {
            self.horse_matcher_editor.buffer.clear();
            self.horse_matcher_editor.replace_on_input = false;
        }
        self.horse_matcher_editor.buffer.push(character);
    }

    fn horse_matcher_backspace(&mut self) {
        if self.horse_matcher_editor.replace_on_input {
            self.horse_matcher_editor.buffer.clear();
            self.horse_matcher_editor.replace_on_input = false;
            return;
        }
        self.horse_matcher_editor.buffer.pop();
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

    fn cycle_horse_matcher_suggestion(&mut self, forward: bool) -> Result<()> {
        let field = self.horse_matcher_editor.selected_field();
        let suggestions = field.suggestions();
        if suggestions.is_empty() {
            return Ok(());
        }

        let current_value = field.display_value(&self.horse_matcher_query);
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

        field.apply_value(&mut self.horse_matcher_query, &suggestions[next_index])?;
        self.horse_matcher_rows.clear();
        self.horse_matcher_table_state.select(None);
        self.persist_horse_matcher_query()?;
        self.status_message = format!("Applied Horse Matcher suggestion for {}.", field.label());
        Ok(())
    }

    fn persist_horse_matcher_query(&mut self) -> Result<()> {
        self.horse_matcher_query_note =
            horse_matcher::save_query(&self.horse_matcher_query_path, &self.horse_matcher_query)?;
        Ok(())
    }

    fn load_calculator_from_selected_oddsmatcher(&mut self) {
        let Some(row) = self.selected_oddsmatcher_row().cloned() else {
            self.status_message = String::from("No OddsMatcher row is selected.");
            return;
        };

        self.load_calculator_from_matcher_row(row, "OddsMatcher");
    }

    fn load_calculator_from_selected_horse_matcher(&mut self) {
        let Some(row) = self.selected_horse_matcher_row().cloned() else {
            self.status_message = String::from("No Horse Matcher row is selected.");
            return;
        };

        self.load_calculator_from_matcher_row(row, "Horse Matcher");
    }

    fn load_calculator_from_matcher_row(&mut self, row: OddsMatcherRow, source_name: &str) {
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
            "Loaded calculator from {source_name}: {} @ {:.2} / {:.2}.",
            row.selection_name, row.back.odds, row.lay.odds
        );
    }

    fn refresh_matcher(&mut self) -> Result<()> {
        match self.matcher_view {
            MatcherView::Odds => self.refresh_oddsmatcher(),
            MatcherView::Horse => self.refresh_horse_matcher(),
            MatcherView::Acca => {
                self.status_message =
                    String::from("Acca Matcher scaffolded; live feed not wired yet.");
                Ok(())
            }
        }
    }

    fn refresh_oddsmatcher(&mut self) -> Result<()> {
        self.queue_oddsmatcher_refresh(self.oddsmatcher_query.clone());
        Ok(())
    }

    fn refresh_horse_matcher(&mut self) -> Result<()> {
        self.queue_provider_request(ProviderJob {
            request: ProviderRequest::LoadHorseMatcher {
                query: self.horse_matcher_query.clone(),
            },
            failure_context: String::from("Horse Matcher refresh failed"),
            event_message: Some(String::from("Horse Matcher refresh completed.")),
        });
        Ok(())
    }

    fn toggle_observability_panel(&mut self) {
        self.active_panel = match self.active_panel {
            Panel::Trading => Panel::Observability,
            Panel::Observability => Panel::Trading,
        };
        if self.active_panel != Panel::Trading || self.trading_section != TradingSection::Positions
        {
            self.live_view_overlay_visible = false;
        }
        if self.active_panel != Panel::Trading
            || (self.trading_section != TradingSection::Positions
                && self.trading_section != TradingSection::Matcher)
        {
            self.trading_action_overlay = None;
        }
    }
}

fn normalize_snapshot(
    mut snapshot: ExchangePanelSnapshot,
    disabled_venues: &str,
) -> ExchangePanelSnapshot {
    snapshot
        .venues
        .retain(|venue| venue_enabled(venue.id, disabled_venues));
    snapshot
        .other_open_bets
        .retain(|bet| !venue_name_disabled(&bet.venue, disabled_venues));
    if snapshot
        .selected_venue
        .is_some_and(|venue| !venue_enabled(venue, disabled_venues))
    {
        snapshot.selected_venue = snapshot.venues.first().map(|venue| venue.id);
    }
    snapshot.historical_positions = merge_historical_positions(&snapshot);
    snapshot
}

fn preserve_cached_snapshot_state(
    previous: &ExchangePanelSnapshot,
    mut next: ExchangePanelSnapshot,
) -> ExchangePanelSnapshot {
    let refresh_kind = next
        .runtime
        .as_ref()
        .map(|runtime| runtime.refresh_kind.as_str())
        .unwrap_or_default();
    if refresh_kind == "cached"
        && next.historical_positions.is_empty()
        && !previous.historical_positions.is_empty()
    {
        next.historical_positions = previous.historical_positions.clone();
    }
    next
}

fn venue_enabled(venue: VenueId, disabled_venues: &str) -> bool {
    !parse_disabled_venues(disabled_venues).contains(venue.as_str())
}

fn venue_name_disabled(value: &str, disabled_venues: &str) -> bool {
    parse_disabled_venues(disabled_venues).contains(&value.trim().to_ascii_lowercase())
}

fn parse_disabled_venues(value: &str) -> HashSet<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn merge_historical_positions(snapshot: &ExchangePanelSnapshot) -> Vec<OpenPositionRow> {
    let mut rows = snapshot.historical_positions.clone();
    let mut seen = rows
        .iter()
        .map(historical_position_key)
        .collect::<HashSet<_>>();

    for tracked_bet in snapshot
        .tracked_bets
        .iter()
        .filter(|tracked_bet| tracked_bet_is_closed(tracked_bet))
    {
        let row = historical_position_from_tracked_bet(tracked_bet);
        if let Some(existing_index) = rows.iter().position(|existing_row| {
            !existing_row.overall_pnl_known
                && historical_position_matches_smarkets_fallback(existing_row, &row)
        }) {
            let previous_key = historical_position_key(&rows[existing_index]);
            seen.remove(&previous_key);
            rows[existing_index] = row.clone();
            seen.insert(historical_position_key(&row));
            continue;
        }
        let row_key = historical_position_key(&row);
        if seen.insert(row_key) {
            rows.push(row);
        }
    }

    rows.sort_by(|left, right| {
        (right.live_clock.as_str(), right.event.as_str())
            .cmp(&(left.live_clock.as_str(), left.event.as_str()))
    });
    rows
}

fn historical_position_key(row: &OpenPositionRow) -> String {
    format!(
        "{}|{}|{}|{}|{:.2}|{:.2}|{:.2}|{}",
        canonical_history_text(&row.event),
        canonical_history_text(&row.market),
        canonical_history_text(&row.contract),
        row.live_clock.trim(),
        row.stake,
        row.price,
        row.pnl_amount,
        row.overall_pnl_known
    )
}

fn historical_position_matches_smarkets_fallback(
    existing_row: &OpenPositionRow,
    tracked_row: &OpenPositionRow,
) -> bool {
    canonical_history_text(&existing_row.event) == canonical_history_text(&tracked_row.event)
        && canonical_history_market(&existing_row.market)
            == canonical_history_market(&tracked_row.market)
        && canonical_history_text(&existing_row.contract)
            == canonical_history_text(&tracked_row.contract)
        && existing_row.live_clock.trim() == tracked_row.live_clock.trim()
}

fn canonical_history_market(value: &str) -> String {
    let normalized = canonical_history_text(value);
    if matches!(
        normalized.as_str(),
        "full time result" | "match odds" | "to win" | "winner"
    ) {
        return String::from("match odds");
    }
    normalized
}

fn canonical_history_text(value: &str) -> String {
    value
        .to_lowercase()
        .replace("vs", "v")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn tracked_bet_is_closed(tracked_bet: &TrackedBetRow) -> bool {
    if !tracked_bet.settled_at.trim().is_empty() {
        return true;
    }

    matches!(
        tracked_bet.status.trim().to_ascii_lowercase().as_str(),
        "settled" | "closed" | "cashedout" | "void" | "lost" | "won"
    )
}

fn historical_position_from_tracked_bet(tracked_bet: &TrackedBetRow) -> OpenPositionRow {
    let price = tracked_bet
        .back_price
        .or(tracked_bet.lay_price)
        .or_else(|| tracked_bet.legs.first().map(|leg| leg.odds))
        .unwrap_or(0.0);
    let stake = tracked_bet
        .stake_gbp
        .or_else(|| {
            tracked_bet
                .legs
                .iter()
                .find(|leg| is_back_leg(leg))
                .map(|leg| leg.stake)
        })
        .or_else(|| tracked_bet.legs.first().map(|leg| leg.stake))
        .unwrap_or(0.0);
    let liability = tracked_bet_liability(tracked_bet).unwrap_or(stake);
    let pnl_amount = tracked_bet
        .realised_pnl_gbp
        .or_else(|| {
            tracked_bet
                .payout_gbp
                .map(|payout| payout - tracked_bet.stake_gbp.unwrap_or(stake))
        })
        .unwrap_or(0.0);
    let current_value = tracked_bet
        .payout_gbp
        .unwrap_or_else(|| (stake + pnl_amount).max(0.0));

    OpenPositionRow {
        event: tracked_bet.event.clone(),
        event_status: String::from("Settled"),
        event_url: String::new(),
        contract: tracked_bet.selection.clone(),
        market: tracked_bet.market.clone(),
        status: if tracked_bet.status.trim().is_empty() {
            String::from("settled")
        } else {
            tracked_bet.status.clone()
        },
        market_status: String::from("settled"),
        is_in_play: false,
        price,
        stake,
        liability,
        current_value,
        pnl_amount,
        overall_pnl_known: true,
        current_back_odds: if price > 0.0 { Some(price) } else { None },
        current_implied_probability: if price > 0.0 { Some(1.0 / price) } else { None },
        current_implied_percentage: if price > 0.0 {
            Some(100.0 / price)
        } else {
            None
        },
        current_buy_odds: if price > 0.0 { Some(price) } else { None },
        current_buy_implied_probability: if price > 0.0 { Some(1.0 / price) } else { None },
        current_sell_odds: None,
        current_sell_implied_probability: None,
        current_score: String::new(),
        current_score_home: None,
        current_score_away: None,
        live_clock: tracked_bet
            .settled_at
            .trim()
            .to_string()
            .if_empty_then(|| tracked_bet.placed_at.clone()),
        can_trade_out: false,
    }
}

fn tracked_bet_liability(tracked_bet: &TrackedBetRow) -> Option<f64> {
    let total = tracked_bet
        .legs
        .iter()
        .map(tracked_leg_liability)
        .sum::<f64>();
    if total > 0.0 {
        Some(total)
    } else {
        None
    }
}

fn tracked_leg_liability(leg: &TrackedLeg) -> f64 {
    if let Some(liability) = leg.liability {
        return liability.abs();
    }
    if is_back_leg(leg) {
        return leg.stake;
    }
    let implied_liability = leg.stake * (leg.odds - 1.0);
    if implied_liability.is_sign_negative() {
        0.0
    } else {
        implied_liability
    }
}

fn is_back_leg(leg: &TrackedLeg) -> bool {
    leg.side.trim().eq_ignore_ascii_case("back")
}

trait StringFallbackExt {
    fn if_empty_then(self, fallback: impl FnOnce() -> String) -> String;
}

impl StringFallbackExt for String {
    fn if_empty_then(self, fallback: impl FnOnce() -> String) -> String {
        if self.is_empty() {
            fallback()
        } else {
            self
        }
    }
}

fn runtime_updated_at(snapshot: &ExchangePanelSnapshot) -> Option<&str> {
    snapshot
        .runtime
        .as_ref()
        .map(|runtime| runtime.updated_at.as_str())
        .filter(|value| !value.trim().is_empty())
}

fn decision_status_counts(snapshot: &ExchangePanelSnapshot) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for decision in &snapshot.decisions {
        *counts.entry(decision.status.clone()).or_insert(0) += 1;
    }
    counts
}

fn format_decision_status_counts(counts: &BTreeMap<String, usize>) -> String {
    if counts.is_empty() {
        return String::from("empty");
    }
    counts
        .iter()
        .map(|(status, count)| format!("{status}={count}"))
        .collect::<Vec<_>>()
        .join(", ")
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
        let worker_config = WorkerConfig {
            positions_payload_path: None,
            run_dir: Some(config.run_dir.clone()),
            account_payload_path: None,
            open_bets_payload_path: None,
            companion_legs_path: config.companion_legs_path.clone(),
            agent_browser_session: Some(config.session.clone()),
            commission_rate: config.commission_rate.parse::<f64>().unwrap_or(0.0),
            target_profit: config.target_profit.parse::<f64>().unwrap_or(1.0),
            stop_loss: config.stop_loss.parse::<f64>().unwrap_or(1.0),
            hard_margin_call_profit_floor: parse_optional_f64(
                &config.hard_margin_call_profit_floor,
            ),
            warn_only_default: config.warn_only_default,
        };
        Box::new(HybridExchangeProvider::new(
            Box::new(NativeExchangeProvider::new(worker_config.clone())),
            Box::new(WorkerClientExchangeProvider::new(
                BetRecorderWorkerClient::new_command(config.command.clone()),
                worker_config,
            )),
        )) as Box<dyn ExchangeProvider + Send>
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

fn exchange_venue_from_bookmaker(code: &str, display_name: &str) -> Option<VenueId> {
    let normalized_code = code.trim().to_lowercase();
    let normalized_name = display_name.trim().to_lowercase();
    if normalized_code.contains("matchbook") || normalized_name.contains("matchbook") {
        Some(VenueId::Matchbook)
    } else if normalized_code.contains("betdaq") || normalized_name.contains("betdaq") {
        Some(VenueId::Betdaq)
    } else if normalized_code.contains("betfair") || normalized_name.contains("betfair") {
        Some(VenueId::Betfair)
    } else if normalized_code.contains("smarkets") || normalized_name.contains("smarkets") {
        Some(VenueId::Smarkets)
    } else {
        None
    }
}

fn new_trading_action_request_id(source: TradingActionSource) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{source:?}-{millis}").to_lowercase()
}

fn start_provider_worker(
    mut provider: Box<dyn ExchangeProvider + Send>,
) -> (Sender<ProviderJob>, Receiver<ProviderResult>) {
    let (job_tx, job_rx) = mpsc::channel::<ProviderJob>();
    let (result_tx, result_rx) = mpsc::channel::<ProviderResult>();
    thread::spawn(move || {
        while let Ok(job) = job_rx.recv() {
            let request = job.request.clone();
            let result = provider
                .handle(request.clone())
                .map_err(|error| error.to_string());
            if result_tx
                .send(ProviderResult {
                    request,
                    result,
                    failure_context: job.failure_context,
                    event_message: job.event_message,
                })
                .is_err()
            {
                break;
            }
        }
    });
    (job_tx, result_rx)
}

fn start_oddsmatcher_worker(
    client: Client,
) -> (Sender<OddsMatcherJob>, Receiver<OddsMatcherResult>) {
    let (job_tx, job_rx) = mpsc::channel::<OddsMatcherJob>();
    let (result_tx, result_rx) = mpsc::channel::<OddsMatcherResult>();
    thread::spawn(move || {
        while let Ok(job) = job_rx.recv() {
            let result = oddsmatcher::fetch_best_matches(&client, &job.query)
                .map_err(|error| error.to_string());
            if result_tx.send(OddsMatcherResult { result }).is_err() {
                break;
            }
        }
    });
    (job_tx, result_rx)
}

fn start_owls_sync_worker(client: Client) -> (Sender<OwlsSyncJob>, Receiver<OwlsSyncResult>) {
    let (job_tx, job_rx) = mpsc::channel::<OwlsSyncJob>();
    let (result_tx, result_rx) = mpsc::channel::<OwlsSyncResult>();
    thread::spawn(move || {
        while let Ok(job) = job_rx.recv() {
            let outcome = owls::sync_dashboard(&client, &job.dashboard, job.reason, job.focused);
            if result_tx
                .send(OwlsSyncResult {
                    outcome,
                    reason: job.reason,
                })
                .is_err()
            {
                break;
            }
        }
    });
    (job_tx, result_rx)
}

fn provider_job_priority(request: &ProviderRequest) -> u8 {
    match request {
        ProviderRequest::ExecuteTradingAction { .. } => 6,
        ProviderRequest::CashOutTrackedBet { .. } => 5,
        ProviderRequest::LoadHorseMatcher { .. } => 4,
        ProviderRequest::SelectVenue(_) => 3,
        ProviderRequest::LoadDashboard => 2,
        ProviderRequest::RefreshLive => 1,
        ProviderRequest::RefreshCached => 0,
    }
}

fn provider_queue_message(request: &ProviderRequest) -> String {
    match request {
        ProviderRequest::LoadDashboard => String::from("Dashboard load queued."),
        ProviderRequest::SelectVenue(venue) => {
            format!("Venue sync queued for {}.", venue.as_str())
        }
        ProviderRequest::RefreshCached => String::from("Cached refresh queued."),
        ProviderRequest::RefreshLive => String::from("Live refresh queued."),
        ProviderRequest::CashOutTrackedBet { bet_id } => {
            format!("Cash out queued for {bet_id}.")
        }
        ProviderRequest::ExecuteTradingAction { intent } => format!(
            "Trading action queued for {} {}.",
            intent.venue.as_str(),
            intent.selection_name
        ),
        ProviderRequest::LoadHorseMatcher { .. } => String::from("Horse Matcher refresh queued."),
    }
}

fn start_matchbook_sync_worker() -> (Sender<MatchbookSyncJob>, Receiver<MatchbookSyncResult>) {
    let (job_tx, job_rx) = mpsc::channel::<MatchbookSyncJob>();
    let (result_tx, result_rx) = mpsc::channel::<MatchbookSyncResult>();
    thread::spawn(move || {
        while let Ok(job) = job_rx.recv() {
            let state = load_matchbook_account_state().map_err(|error| error.to_string());
            if result_tx
                .send(MatchbookSyncResult {
                    state,
                    reason: job.reason,
                })
                .is_err()
            {
                break;
            }
        }
    });
    (job_tx, result_rx)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Arc as Rc, Mutex, Mutex as RefCell};
    use std::time::{Duration, Instant};

    use crate::domain::{
        ExchangePanelSnapshot, OpenPositionRow, RuntimeSummary, TrackedBetRow, VenueId,
        VenueStatus, VenueSummary, WorkerStatus, WorkerSummary,
    };
    use crate::provider::{ExchangeProvider, ProviderRequest};
    use crate::recorder::{RecorderConfig, RecorderStatus, RecorderSupervisor};
    use crate::stub_provider::StubExchangeProvider;
    use crossterm::event::KeyCode;

    use super::{App, Panel, TradingSection, MAX_EVENT_HISTORY};

    struct RefreshingProvider {
        cached_refresh_count: Arc<Mutex<usize>>,
        live_refresh_count: Arc<Mutex<usize>>,
        load_snapshot: ExchangePanelSnapshot,
        cached_refresh_snapshot: ExchangePanelSnapshot,
        live_refresh_snapshot: ExchangePanelSnapshot,
    }

    impl ExchangeProvider for RefreshingProvider {
        fn handle(
            &mut self,
            request: ProviderRequest,
        ) -> color_eyre::Result<ExchangePanelSnapshot> {
            match request {
                ProviderRequest::LoadDashboard => Ok(self.load_snapshot.clone()),
                ProviderRequest::RefreshCached => {
                    *self.cached_refresh_count.lock().expect("lock") += 1;
                    Ok(self.cached_refresh_snapshot.clone())
                }
                ProviderRequest::RefreshLive => {
                    *self.live_refresh_count.lock().expect("lock") += 1;
                    Ok(self.live_refresh_snapshot.clone())
                }
                ProviderRequest::SelectVenue(_)
                | ProviderRequest::CashOutTrackedBet { .. }
                | ProviderRequest::ExecuteTradingAction { .. }
                | ProviderRequest::LoadHorseMatcher { .. } => {
                    Ok(self.cached_refresh_snapshot.clone())
                }
            }
        }
    }

    struct RunningSupervisor;

    struct DisabledSupervisor;

    struct FailingSupervisor;

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

    impl RecorderSupervisor for FailingSupervisor {
        fn start(&mut self, _config: &RecorderConfig) -> color_eyre::Result<()> {
            Err(color_eyre::eyre::eyre!("watcher binary missing"))
        }

        fn stop(&mut self) -> color_eyre::Result<()> {
            Ok(())
        }

        fn poll_status(&mut self) -> RecorderStatus {
            RecorderStatus::Error
        }
    }

    #[test]
    fn poll_recorder_refreshes_running_recorder_automatically() {
        let cached_refresh_count = Rc::new(RefCell::new(0));
        let live_refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: cached_refresh_count.clone(),
                live_refresh_count: live_refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                cached_refresh_snapshot: sample_snapshot("Auto refreshed dashboard"),
                live_refresh_snapshot: sample_snapshot("Live refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                    live_refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                    live_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(RunningSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.recorder_status = RecorderStatus::Running;
        app.last_recorder_refresh_at = Some(Instant::now() - Duration::from_secs(6));

        app.poll_recorder();
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));

        assert_eq!(app.snapshot().status_line, "Auto refreshed dashboard");
        assert_eq!(*cached_refresh_count.lock().expect("lock"), 1);
        assert_eq!(*live_refresh_count.lock().expect("lock"), 0);
    }

    #[test]
    fn poll_recorder_skips_auto_refresh_when_not_running() {
        let cached_refresh_count = Rc::new(RefCell::new(0));
        let live_refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: cached_refresh_count.clone(),
                live_refresh_count: live_refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                cached_refresh_snapshot: sample_snapshot("Auto refreshed dashboard"),
                live_refresh_snapshot: sample_snapshot("Live refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                    live_refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                    live_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(DisabledSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.recorder_status = RecorderStatus::Disabled;
        app.last_recorder_refresh_at = Some(Instant::now() - Duration::from_secs(6));

        app.poll_recorder();

        assert_eq!(app.snapshot().status_line, "Initial dashboard");
        assert_eq!(*cached_refresh_count.lock().expect("lock"), 0);
        assert_eq!(*live_refresh_count.lock().expect("lock"), 0);
    }

    #[test]
    fn poll_recorder_skips_auto_refresh_before_interval_elapses() {
        let cached_refresh_count = Rc::new(RefCell::new(0));
        let live_refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: cached_refresh_count.clone(),
                live_refresh_count: live_refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                cached_refresh_snapshot: sample_snapshot("Auto refreshed dashboard"),
                live_refresh_snapshot: sample_snapshot("Live refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                    live_refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                    live_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
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
        assert_eq!(*cached_refresh_count.lock().expect("lock"), 0);
        assert_eq!(*live_refresh_count.lock().expect("lock"), 0);
    }

    #[test]
    fn poll_recorder_keeps_provider_refresh_running_inside_oddsmatcher() {
        let cached_refresh_count = Rc::new(RefCell::new(0));
        let live_refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: cached_refresh_count.clone(),
                live_refresh_count: live_refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                cached_refresh_snapshot: sample_snapshot("Auto refreshed dashboard"),
                live_refresh_snapshot: sample_snapshot("Live refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                    live_refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                    live_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(RunningSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.set_active_panel(Panel::Trading);
        app.set_trading_section(TradingSection::Matcher);
        app.recorder_status = RecorderStatus::Running;
        app.last_recorder_refresh_at = Some(Instant::now() - Duration::from_secs(6));

        app.poll_recorder();
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));

        assert_eq!(app.snapshot().status_line, "Auto refreshed dashboard");
        assert!(app.oddsmatcher_rows().is_empty());
        assert_eq!(*cached_refresh_count.lock().expect("lock"), 1);
        assert_eq!(*live_refresh_count.lock().expect("lock"), 0);
    }

    #[test]
    fn poll_recorder_uses_live_refresh_for_non_smarkets_selected_venue() {
        let cached_refresh_count = Rc::new(RefCell::new(0));
        let live_refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut load_snapshot = sample_snapshot("Initial dashboard");
        load_snapshot.venues.push(VenueSummary {
            id: VenueId::Betway,
            label: String::from("betway"),
            status: VenueStatus::Connected,
            detail: String::from("live"),
            event_count: 2,
            market_count: 2,
        });
        load_snapshot.selected_venue = Some(VenueId::Betway);

        let cached_refresh_snapshot = sample_runtime_snapshot(
            "Cached smarkets dashboard",
            "2026-03-24T12:00:00Z",
            false,
            "cached",
        );
        let mut live_refresh_snapshot = sample_runtime_snapshot(
            "Live betway dashboard",
            "2026-03-24T12:00:01Z",
            false,
            "live_capture",
        );
        live_refresh_snapshot.selected_venue = Some(VenueId::Betway);
        live_refresh_snapshot.venues = load_snapshot.venues.clone();
        live_refresh_snapshot.other_open_bets = vec![crate::domain::OtherOpenBetRow {
            venue: String::from("betway"),
            event: String::from("Arsenal v Everton"),
            label: String::from("Arsenal"),
            market: String::from("Match Odds"),
            side: String::from("back"),
            odds: 2.4,
            stake: 10.0,
            status: String::from("open"),
            funding_kind: String::new(),
            current_cashout_value: None,
            supports_cash_out: false,
        }];

        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: cached_refresh_count.clone(),
                live_refresh_count: live_refresh_count.clone(),
                load_snapshot,
                cached_refresh_snapshot,
                live_refresh_snapshot,
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                    live_refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                    live_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(RunningSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.recorder_status = RecorderStatus::Running;
        app.last_recorder_refresh_at = Some(Instant::now() - Duration::from_secs(6));

        app.poll_recorder();
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));

        assert_eq!(app.snapshot().status_line, "Live betway dashboard");
        assert_eq!(app.snapshot().selected_venue, Some(VenueId::Betway));
        assert_eq!(app.snapshot().other_open_bets.len(), 1);
        assert_eq!(*cached_refresh_count.lock().expect("lock"), 0);
        assert_eq!(*live_refresh_count.lock().expect("lock"), 1);
    }

    #[test]
    fn normalize_snapshot_filters_disabled_bet365_surface() {
        let mut snapshot = sample_snapshot("Initial dashboard");
        snapshot.venues.push(VenueSummary {
            id: VenueId::Bet365,
            label: String::from("bet365"),
            status: VenueStatus::Connected,
            detail: String::from("blocked"),
            event_count: 3,
            market_count: 2,
        });
        snapshot.selected_venue = Some(VenueId::Bet365);
        snapshot
            .other_open_bets
            .push(crate::domain::OtherOpenBetRow {
                venue: String::from("bet365"),
                event: String::from("Arsenal v Everton"),
                label: String::from("Arsenal"),
                market: String::from("Match Odds"),
                side: String::from("back"),
                odds: 2.4,
                stake: 10.0,
                status: String::from("open"),
                funding_kind: String::new(),
                current_cashout_value: None,
                supports_cash_out: false,
            });

        let normalized = super::normalize_snapshot(snapshot, "bet365");

        assert!(normalized
            .venues
            .iter()
            .all(|venue| venue.id != VenueId::Bet365));
        assert_eq!(normalized.selected_venue, Some(VenueId::Smarkets));
        assert!(normalized
            .other_open_bets
            .iter()
            .all(|bet| !bet.venue.eq_ignore_ascii_case("bet365")));
    }

    #[test]
    fn start_recorder_with_busy_bootstrap_snapshot_triggers_immediate_auto_refresh() {
        let cached_refresh_count = Rc::new(RefCell::new(0));
        let live_refresh_count = Rc::new(RefCell::new(0));
        let provider_cached_refresh_count = cached_refresh_count.clone();
        let provider_live_refresh_count = live_refresh_count.clone();
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut busy_snapshot = sample_snapshot("Recorder started; waiting for first snapshot.");
        busy_snapshot.worker.status = WorkerStatus::Busy;
        busy_snapshot.worker.detail = String::from("Recorder started; waiting for first snapshot.");

        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: cached_refresh_count.clone(),
                live_refresh_count: live_refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                cached_refresh_snapshot: sample_snapshot("Auto refreshed dashboard"),
                live_refresh_snapshot: sample_snapshot("Live refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                    live_refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(move |_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: provider_cached_refresh_count.clone(),
                    live_refresh_count: provider_live_refresh_count.clone(),
                    load_snapshot: busy_snapshot.clone(),
                    cached_refresh_snapshot: sample_snapshot("Auto refreshed dashboard"),
                    live_refresh_snapshot: sample_snapshot("Live refreshed dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(RunningSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.start_recorder().expect("start recorder");
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));

        assert_eq!(app.snapshot().worker.status, WorkerStatus::Busy);
        assert_eq!(app.last_recorder_refresh_at, None);

        app.poll_recorder();
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));

        assert_eq!(app.snapshot().status_line, "Auto refreshed dashboard");
        assert_eq!(app.snapshot().worker.status, WorkerStatus::Ready);
        assert_eq!(*cached_refresh_count.lock().expect("lock"), 1);
        assert_eq!(*live_refresh_count.lock().expect("lock"), 0);
    }

    #[test]
    fn manual_live_refresh_uses_live_request() {
        let cached_refresh_count = Rc::new(RefCell::new(0));
        let live_refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: cached_refresh_count.clone(),
                live_refresh_count: live_refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                cached_refresh_snapshot: sample_snapshot("Cached dashboard"),
                live_refresh_snapshot: sample_snapshot("Live refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                    live_refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                    live_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(RunningSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.refresh_live().expect("live refresh should succeed");
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));

        assert_eq!(app.snapshot().status_line, "Live refreshed dashboard");
        assert_eq!(*cached_refresh_count.lock().expect("lock"), 0);
        assert_eq!(*live_refresh_count.lock().expect("lock"), 1);
    }

    #[test]
    fn recorder_lifecycle_reports_stale_when_runtime_is_stale() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: Rc::new(RefCell::new(0)),
                live_refresh_count: Rc::new(RefCell::new(0)),
                load_snapshot: sample_runtime_snapshot(
                    "Initial dashboard",
                    "2026-03-19T10:00:00Z",
                    true,
                    "cached",
                ),
                cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                live_refresh_snapshot: sample_snapshot("Stub dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                    live_refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                    live_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(RunningSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.recorder_status = RecorderStatus::Running;

        assert_eq!(app.recorder_lifecycle_state(), "stale");
        assert_eq!(app.recorder_snapshot_freshness(), "stale");
        assert_eq!(app.recorder_snapshot_mode(), "cached");
        assert_eq!(
            app.last_successful_snapshot_at(),
            Some("2026-03-19T10:00:00Z")
        );
    }

    #[test]
    fn handle_key_start_failure_tracks_startup_failure_detail() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: Rc::new(RefCell::new(0)),
                live_refresh_count: Rc::new(RefCell::new(0)),
                load_snapshot: sample_snapshot("Initial dashboard"),
                cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                live_refresh_snapshot: sample_snapshot("Stub dashboard"),
            }),
            Box::new(|| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Stub dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                    live_refresh_snapshot: sample_snapshot("Stub dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: sample_snapshot("Recorder dashboard"),
                    cached_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                    live_refresh_snapshot: sample_snapshot("Recorder dashboard"),
                }) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(FailingSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.handle_key(KeyCode::Char('s'));

        assert_eq!(app.recorder_lifecycle_state(), "failed");
        assert!(app
            .last_recorder_start_failure()
            .is_some_and(|detail| detail.contains("watcher binary missing")));
        assert!(app.status_message().contains("Recorder start failed"));
        assert!(app
            .recent_events()
            .iter()
            .any(|event| event.contains("Recorder start failed: watcher binary missing")));
    }

    #[test]
    fn record_event_deduplicates_and_trims_history() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: Rc::new(RefCell::new(0)),
                live_refresh_count: Rc::new(RefCell::new(0)),
                load_snapshot: sample_snapshot("Initial dashboard"),
                cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                live_refresh_snapshot: sample_snapshot("Stub dashboard"),
            }),
            Box::new(|| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(DisabledSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");
        app.event_history.clear();

        app.record_event("duplicate event");
        app.record_event("duplicate event");
        for index in 0..MAX_EVENT_HISTORY {
            app.record_event(format!("event {index}"));
        }

        assert_eq!(app.event_history.len(), MAX_EVENT_HISTORY);
        assert_eq!(
            app.event_history.front().map(String::as_str),
            Some("event 0")
        );
        assert_eq!(
            app.event_history.back().map(String::as_str),
            Some("event 24")
        );
    }

    #[test]
    fn refresh_logs_worker_reconnect_recovery_event() {
        let cached_refresh_count = Rc::new(RefCell::new(0));
        let live_refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: cached_refresh_count.clone(),
                live_refresh_count: live_refresh_count.clone(),
                load_snapshot: sample_runtime_snapshot(
                    "Initial dashboard",
                    "2026-03-19T10:00:00Z",
                    false,
                    "cached",
                ),
                cached_refresh_snapshot: {
                    let mut snapshot = sample_runtime_snapshot(
                        "Recovered dashboard",
                        "2026-03-19T10:00:01Z",
                        false,
                        "cached",
                    );
                    snapshot
                        .runtime
                        .as_mut()
                        .expect("runtime")
                        .worker_reconnect_count = 2;
                    snapshot
                },
                live_refresh_snapshot: sample_snapshot("Live refreshed dashboard"),
            }),
            Box::new(|| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(RunningSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        app.refresh().expect("refresh should succeed");
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));

        assert_eq!(*cached_refresh_count.lock().expect("lock"), 1);
        assert_eq!(*live_refresh_count.lock().expect("lock"), 0);
        assert!(app
            .recent_events()
            .iter()
            .any(|event| event.contains("Worker session recovered; reconnect count is now 2.")));
        assert!(app
            .recent_events()
            .iter()
            .any(|event| event.contains("Manual cached refresh completed for smarkets.")));
    }

    #[test]
    fn initial_snapshot_merges_closed_tracked_bets_into_historical_positions() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut snapshot = sample_snapshot("Initial dashboard");
        snapshot.tracked_bets = vec![TrackedBetRow {
            bet_id: String::from("bet-1"),
            group_id: String::from("group-1"),
            event: String::from("Arsenal vs Spurs"),
            market: String::from("Match Odds"),
            selection: String::from("Draw"),
            status: String::from("settled"),
            placed_at: String::from("2026-03-19T19:00:00Z"),
            settled_at: String::from("2026-03-19T21:55:00Z"),
            stake_gbp: Some(10.0),
            realised_pnl_gbp: Some(4.5),
            back_price: Some(3.2),
            ..TrackedBetRow::default()
        }];
        let app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: Rc::new(RefCell::new(0)),
                live_refresh_count: Rc::new(RefCell::new(0)),
                load_snapshot: snapshot,
                cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                live_refresh_snapshot: sample_snapshot("Stub dashboard"),
            }),
            Box::new(|| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(DisabledSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        assert_eq!(app.snapshot().historical_positions.len(), 1);
        assert_eq!(
            app.snapshot().historical_positions[0].event,
            "Arsenal vs Spurs"
        );
        assert_eq!(app.snapshot().historical_positions[0].contract, "Draw");
        assert_eq!(
            app.snapshot().historical_positions[0].live_clock,
            "2026-03-19T21:55:00Z"
        );
    }

    #[test]
    fn refresh_merges_newly_closed_tracked_bets_into_historical_positions() {
        let cached_refresh_count = Rc::new(RefCell::new(0));
        let live_refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut refreshed_snapshot = sample_snapshot("Refreshed dashboard");
        refreshed_snapshot.tracked_bets = vec![TrackedBetRow {
            bet_id: String::from("bet-2"),
            group_id: String::from("group-2"),
            event: String::from("Chelsea vs Liverpool"),
            market: String::from("Both Teams To Score"),
            selection: String::from("Yes"),
            status: String::from("won"),
            placed_at: String::from("2026-03-20T14:00:00Z"),
            settled_at: String::from("2026-03-20T15:57:00Z"),
            stake_gbp: Some(12.0),
            realised_pnl_gbp: Some(9.6),
            back_price: Some(1.8),
            ..TrackedBetRow::default()
        }];
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: cached_refresh_count.clone(),
                live_refresh_count: live_refresh_count.clone(),
                load_snapshot: sample_snapshot("Initial dashboard"),
                cached_refresh_snapshot: refreshed_snapshot,
                live_refresh_snapshot: sample_snapshot("Live dashboard"),
            }),
            Box::new(|| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(DisabledSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        assert!(app.snapshot().historical_positions.is_empty());

        app.refresh().expect("refresh should succeed");
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));

        assert_eq!(*cached_refresh_count.lock().expect("lock"), 1);
        assert_eq!(*live_refresh_count.lock().expect("lock"), 0);
        assert_eq!(app.snapshot().historical_positions.len(), 1);
        assert_eq!(
            app.snapshot().historical_positions[0].event,
            "Chelsea vs Liverpool"
        );
        assert_eq!(app.snapshot().historical_positions[0].contract, "Yes");
        assert_eq!(
            app.snapshot().historical_positions[0].live_clock,
            "2026-03-20T15:57:00Z"
        );
    }

    #[test]
    fn cached_refresh_preserves_existing_historical_positions_when_history_is_omitted() {
        let cached_refresh_count = Rc::new(RefCell::new(0));
        let live_refresh_count = Rc::new(RefCell::new(0));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut initial_snapshot =
            sample_runtime_snapshot("Initial dashboard", "2026-03-24T12:00:00Z", false, "cached");
        initial_snapshot.historical_positions = vec![OpenPositionRow {
            event: String::from("Arsenal vs Everton"),
            event_status: String::from("Settled"),
            event_url: String::new(),
            contract: String::from("Draw"),
            market: String::from("Full-time result"),
            status: String::from("lost"),
            market_status: String::from("settled"),
            is_in_play: false,
            price: 3.35,
            stake: 9.91,
            liability: 23.29,
            current_value: -23.29,
            pnl_amount: -23.29,
            overall_pnl_known: false,
            current_back_odds: Some(3.35),
            current_implied_probability: Some(1.0 / 3.35),
            current_implied_percentage: Some(100.0 / 3.35),
            current_buy_odds: Some(3.35),
            current_buy_implied_probability: Some(1.0 / 3.35),
            current_sell_odds: None,
            current_sell_implied_probability: None,
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::from("2026-03-22T16:10:26Z"),
            can_trade_out: false,
        }];
        let cached_refresh_snapshot =
            sample_runtime_snapshot("Cached dashboard", "2026-03-24T12:00:05Z", false, "cached");

        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: cached_refresh_count.clone(),
                live_refresh_count: live_refresh_count.clone(),
                load_snapshot: initial_snapshot,
                cached_refresh_snapshot,
                live_refresh_snapshot: sample_snapshot("Live dashboard"),
            }),
            Box::new(|| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(DisabledSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        assert_eq!(app.snapshot().historical_positions.len(), 1);

        app.refresh().expect("refresh should succeed");
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));

        assert_eq!(*cached_refresh_count.lock().expect("lock"), 1);
        assert_eq!(*live_refresh_count.lock().expect("lock"), 0);
        assert_eq!(app.snapshot().historical_positions.len(), 1);
        assert_eq!(
            app.snapshot().historical_positions[0].event,
            "Arsenal vs Everton"
        );
        assert_eq!(app.snapshot().historical_positions[0].contract, "Draw");
    }

    #[test]
    fn tracked_bet_history_replaces_smarkets_only_fallback_row() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut snapshot = sample_snapshot("Initial dashboard");
        snapshot.historical_positions = vec![OpenPositionRow {
            event: String::from("Arsenal vs Everton"),
            event_status: String::from("Settled"),
            event_url: String::new(),
            contract: String::from("Draw"),
            market: String::from("Full-time result"),
            status: String::from("lost"),
            market_status: String::from("settled"),
            is_in_play: false,
            price: 3.35,
            stake: 9.91,
            liability: 23.29,
            current_value: -23.29,
            pnl_amount: -23.29,
            overall_pnl_known: false,
            current_back_odds: Some(3.35),
            current_implied_probability: Some(1.0 / 3.35),
            current_implied_percentage: Some(100.0 / 3.35),
            current_buy_odds: Some(3.35),
            current_buy_implied_probability: Some(1.0 / 3.35),
            current_sell_odds: None,
            current_sell_implied_probability: None,
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::from("2026-03-22T16:10:26Z"),
            can_trade_out: false,
        }];
        snapshot.tracked_bets = vec![TrackedBetRow {
            bet_id: String::from("bet-arsenal-everton-draw"),
            group_id: String::from("group-arsenal-everton-draw"),
            event: String::from("Arsenal vs Everton"),
            market: String::from("Match Odds"),
            selection: String::from("Draw"),
            status: String::from("lost"),
            placed_at: String::from("2026-03-22T15:05:00Z"),
            settled_at: String::from("2026-03-22T16:10:26Z"),
            stake_gbp: Some(10.0),
            realised_pnl_gbp: Some(8.71),
            back_price: Some(4.2),
            lay_price: Some(3.35),
            ..TrackedBetRow::default()
        }];
        let replacement_row =
            super::historical_position_from_tracked_bet(&snapshot.tracked_bets[0]);
        assert!(super::historical_position_matches_smarkets_fallback(
            &snapshot.historical_positions[0],
            &replacement_row
        ));

        let app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: Rc::new(RefCell::new(0)),
                live_refresh_count: Rc::new(RefCell::new(0)),
                load_snapshot: snapshot,
                cached_refresh_snapshot: sample_snapshot("Stub dashboard"),
                live_refresh_snapshot: sample_snapshot("Stub dashboard"),
            }),
            Box::new(|| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(|_| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(DisabledSupervisor),
            RecorderConfig::default(),
            temp_dir.path().join("recorder.json"),
            String::from("test"),
        )
        .expect("app");

        assert_eq!(app.snapshot().historical_positions.len(), 1);
        assert!(app.snapshot().historical_positions[0].overall_pnl_known);
        assert_eq!(app.snapshot().historical_positions[0].pnl_amount, 8.71);
        assert_eq!(app.snapshot().historical_positions[0].stake, 10.0);
        assert_eq!(app.snapshot().historical_positions[0].price, 4.2);
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

    fn sample_runtime_snapshot(
        status_line: &str,
        updated_at: &str,
        stale: bool,
        refresh_kind: &str,
    ) -> ExchangePanelSnapshot {
        let mut snapshot = sample_snapshot(status_line);
        snapshot.runtime = Some(RuntimeSummary {
            updated_at: String::from(updated_at),
            source: String::from("watcher-state"),
            refresh_kind: String::from(refresh_kind),
            worker_reconnect_count: 0,
            decision_count: 1,
            watcher_iteration: Some(4),
            stale,
        });
        snapshot
    }
}
