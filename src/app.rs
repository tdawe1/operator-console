use chrono::Local;
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::widgets::{ListState, TableState};
use ratatui::{Frame, Terminal};
use reqwest::blocking::Client;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

use crate::alerts::{
    self, load_alert_config_or_default, save_alert_config, AlertConfig, AlertEditorState,
    AlertField, NotificationEntry, NotificationLevel,
};
pub use crate::app_state::{
    BackendExecutionOverlayState, CalculatorEditorState, CalculatorField, CalculatorSourceContext,
    CalculatorState, CalculatorTool, IntelRow, IntelSource, IntelSourceStatus, IntelView,
    MatcherView, ObservabilitySection, OddsMatcherFocus, Panel, PositionsFocus, TradingActionField,
    TradingActionOverlayState, TradingSection,
};
use crate::calculator::{self, BetType, Mode as CalculatorMode};
use crate::domain::{
    ExchangePanelSnapshot, ExternalLiveEventRow, ExternalLiveIncidentRow, ExternalLiveStatRow,
    ExternalPlayerRatingRow, ExternalQuoteRow, OpenPositionRow, TrackedBetRow, TrackedLeg, VenueId,
    WorkerStatus,
};
use crate::exchange_api::{
    MatchbookAccountState, MatchbookBetRow, MatchbookOfferRow, MatchbookPositionRow,
};
use crate::execution_backend;
use crate::horse_matcher::{self, HorseMatcherEditorState, HorseMatcherField, HorseMatcherQuery};
use crate::manual_positions::{
    self, load_entries_or_default, save_entries, ManualPositionEntry, ManualPositionField,
    ManualPositionOverlayState,
};
use crate::market_intel::{self, MarketIntelDashboard, MarketOpportunityRow};
use crate::market_normalization::{
    event_matches, market_matches, normalize_key, selection_matches,
    selection_matches_with_context, text_matches,
};
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
use crate::resource_state::{ResourcePhase, ResourceState};
use crate::runtime::{AppRuntimeChannels, AppRuntimeHost};
use crate::snapshot_projection::project_snapshot;
use crate::stub_provider::StubExchangeProvider;
use crate::trading_actions::{
    format_decimal, TradingActionMode, TradingActionSeed, TradingActionSide, TradingActionSource,
    TradingActionSourceContext, TradingTimeInForce,
};
use crate::transport::WorkerConfig;
use crate::ui;
use crate::wm::{NavDirection, PaneId};
use crate::worker_client::{BetRecorderWorkerClient, WorkerClientExchangeProvider};

type ProviderFactory = dyn Fn(&RecorderConfig) -> Box<dyn ExchangeProvider + Send>;
type StubFactory = dyn Fn() -> Box<dyn ExchangeProvider + Send>;

pub(crate) struct ProviderJob {
    pub(crate) request: ProviderRequest,
    pub(crate) failure_context: String,
    pub(crate) event_message: Option<String>,
}

pub(crate) struct ProviderResult {
    pub(crate) request: ProviderRequest,
    pub(crate) result: std::result::Result<ExchangePanelSnapshot, String>,
    pub(crate) failure_context: String,
    pub(crate) event_message: Option<String>,
}

pub(crate) struct OwlsSyncJob {
    pub(crate) dashboard: OwlsDashboard,
    pub(crate) reason: OwlsSyncReason,
    pub(crate) focused: Option<OwlsEndpointId>,
}

pub(crate) struct OwlsSyncResult {
    pub(crate) outcome: owls::OwlsSyncOutcome,
    pub(crate) reason: OwlsSyncReason,
}

#[derive(Clone, Copy)]
pub(crate) enum MatchbookSyncReason {
    Manual,
    Background,
}

impl MatchbookSyncReason {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Background => "monitor",
        }
    }
}

pub(crate) struct MatchbookSyncJob {
    pub(crate) reason: MatchbookSyncReason,
}

pub(crate) struct MatchbookSyncResult {
    pub(crate) state: std::result::Result<MatchbookAccountState, String>,
    pub(crate) reason: MatchbookSyncReason,
}

pub struct PositionsRenderState<'a> {
    pub snapshot: &'a ExchangePanelSnapshot,
    pub owls_dashboard: &'a OwlsDashboard,
    pub matchbook_account_state: Option<&'a MatchbookAccountState>,
    pub open_table_state: &'a mut TableState,
    pub historical_table_state: &'a mut TableState,
    pub positions_focus: PositionsFocus,
    pub show_live_view_overlay: bool,
    pub status_message: &'a str,
    pub status_scroll: u16,
}

pub(crate) struct OddsMatcherJob {
    pub(crate) query: GetBestMatchesVariables,
}

pub(crate) struct OddsMatcherResult {
    pub(crate) result: std::result::Result<Vec<OddsMatcherRow>, String>,
}

#[derive(Clone, Copy)]
pub(crate) enum MarketIntelSyncReason {
    Manual,
    Background,
}

impl MarketIntelSyncReason {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Background => "monitor",
        }
    }
}

pub(crate) struct MarketIntelJob {
    pub(crate) reason: MarketIntelSyncReason,
    pub(crate) sport_key: Option<String>,
}

pub(crate) struct MarketIntelResult {
    pub(crate) dashboard: std::result::Result<MarketIntelDashboard, String>,
    pub(crate) reason: MarketIntelSyncReason,
}

#[derive(Clone, Copy)]
enum MouseTargetKind {
    Workspace(usize),
    Pane(PaneId),
    PaneMinimize(PaneId),
    PaneToggleMaximize(PaneId),
    TradingSection(TradingSection),
    IntelView(IntelView),
    MatcherView(MatcherView),
    CalculatorTool(CalculatorTool),
    MinimizedPane(PaneId),
    MinimizeActivePane,
    ToggleMaximize,
}

#[derive(Clone, Copy)]
struct MouseTarget {
    rect: Rect,
    kind: MouseTargetKind,
}

pub struct App {
    runtime_host: AppRuntimeHost,
    provider_tx: tokio::sync::mpsc::UnboundedSender<ProviderJob>,
    provider_rx: tokio::sync::mpsc::UnboundedReceiver<ProviderResult>,
    provider_in_flight: bool,
    provider_resource_state: ResourceState<ExchangePanelSnapshot>,
    provider_pending_job: Option<ProviderJob>,
    #[cfg(debug_assertions)]
    provider_in_flight_started_at_for_test: Option<Instant>,
    make_stub_provider: Box<StubFactory>,
    make_recorder_provider: Box<ProviderFactory>,
    recorder_supervisor: Box<dyn RecorderSupervisor>,
    recorder_config: RecorderConfig,
    recorder_config_path: std::path::PathBuf,
    recorder_config_note: String,
    recorder_editor: RecorderEditorState,
    alerts_config: AlertConfig,
    alerts_config_path: std::path::PathBuf,
    alerts_config_note: String,
    alerts_editor: AlertEditorState,
    manual_positions: Vec<ManualPositionEntry>,
    manual_positions_path: PathBuf,
    manual_positions_note: String,
    manual_position_overlay: Option<ManualPositionOverlayState>,
    alert_last_sent_at: HashMap<&'static str, Instant>,
    notifications: VecDeque<NotificationEntry>,
    notifications_overlay_visible: bool,
    recorder_status: RecorderStatus,
    calculator: CalculatorState,
    calculator_tool: CalculatorTool,
    oddsmatcher_tx: Sender<OddsMatcherJob>,
    oddsmatcher_rx: Receiver<OddsMatcherResult>,
    oddsmatcher_in_flight: bool,
    oddsmatcher_pending_query: Option<GetBestMatchesVariables>,
    market_intel_tx: Sender<MarketIntelJob>,
    market_intel_rx: Receiver<MarketIntelResult>,
    market_intel_in_flight: bool,
    market_intel_resource_state: ResourceState<MarketIntelDashboard>,
    market_intel_pending_reason: Option<MarketIntelSyncReason>,
    last_market_intel_dispatch_at: Option<Instant>,
    owls_sync_tx: tokio::sync::mpsc::UnboundedSender<OwlsSyncJob>,
    owls_sync_rx: tokio::sync::mpsc::UnboundedReceiver<OwlsSyncResult>,
    owls_sync_in_flight: bool,
    owls_resource_state: ResourceState<OwlsDashboard>,
    owls_sync_pending_reason: Option<OwlsSyncReason>,
    last_owls_sync_dispatch_at: Option<Instant>,
    matchbook_sync_tx: tokio::sync::mpsc::UnboundedSender<MatchbookSyncJob>,
    matchbook_sync_rx: tokio::sync::mpsc::UnboundedReceiver<MatchbookSyncResult>,
    matchbook_sync_in_flight: bool,
    matchbook_resource_state: ResourceState<MatchbookAccountState>,
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
    intel_view: IntelView,
    intel_table_state: TableState,
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
    last_event_at_label: Option<String>,
    mouse_targets: Vec<MouseTarget>,
    running: bool,
    status_message: String,
    status_scroll: u16,
    last_problem_console_message: Option<String>,
    recorder_startup_alerts_pending: bool,
    recorder_startup_alerts_muted_until: Option<Instant>,
    pub wm: crate::wm::WindowManager,
}

const MAX_EVENT_HISTORY: usize = 25;
const RECORDER_REFRESH_INTERVAL_IDLE: Duration = Duration::from_secs(5);
const RECORDER_REFRESH_INTERVAL_ACTIVE: Duration = Duration::from_secs(2);
const RECORDER_REFRESH_INTERVAL_BOOTSTRAP: Duration = Duration::from_secs(1);
const OWLS_SYNC_DISPATCH_INTERVAL: Duration = Duration::from_secs(1);
const MATCHBOOK_SYNC_DISPATCH_INTERVAL: Duration = Duration::from_secs(4);
const MARKET_INTEL_SYNC_DISPATCH_INTERVAL: Duration = Duration::from_secs(20);
const RECORDER_STARTUP_ALERT_MUTE: Duration = Duration::from_secs(15);
const MAX_NOTIFICATIONS: usize = 50;
const RESOURCE_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(30);

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
        let mut app = Self::with_dependencies_and_storage(
            provider,
            Box::new(stub_factory),
            make_recorder_provider,
            Box::new(ProcessRecorderSupervisor::default()),
            recorder_config,
            recorder_config_path,
            recorder_config_note,
        )
        .expect("default stub provider should load dashboard");
        app.load_alerts_from_disk(alerts::default_config_path())
            .expect("default alerts config should load");
        app
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
        let mut app = Self::with_dependencies_and_storage(
            Box::new(provider),
            Box::new(|| Box::new(StubExchangeProvider::default())),
            default_recorder_provider_factory(),
            Box::new(ProcessRecorderSupervisor::default()),
            recorder_config,
            recorder_config_path,
            recorder_config_note,
        )?;
        app.load_alerts_from_disk(alerts::default_config_path())?;
        Ok(app)
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
        let manual_positions_path = recorder_config_path
            .parent()
            .map(|parent| parent.join("manual_positions.json"))
            .unwrap_or_else(manual_positions::default_config_path);
        let (manual_positions, manual_positions_note) =
            load_entries_or_default(&manual_positions_path).unwrap_or_else(|error| {
                (
                    Vec::new(),
                    format!("Manual positions load failed; using empty defaults: {error}"),
                )
            });
        let runtime_host = AppRuntimeHost::new()?;
        let snapshot = normalize_snapshot(
            provider.handle(ProviderRequest::LoadDashboard)?,
            &recorder_config.disabled_venues,
            &manual_positions,
        );
        let last_successful_snapshot_at = runtime_updated_at(&snapshot).map(str::to_string);
        let status_message = snapshot.status_line.clone();
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
        let owls_client = default_owls_sync_async_client();
        let runtime =
            AppRuntimeChannels::start_all(&runtime_host, provider, oddsmatcher_client, owls_client);

        let mut app = Self {
            runtime_host,
            provider_tx: runtime.provider_tx,
            provider_rx: runtime.provider_rx,
            provider_in_flight: false,
            provider_resource_state: ResourceState::ready(snapshot.clone()),
            provider_pending_job: None,
            #[cfg(debug_assertions)]
            provider_in_flight_started_at_for_test: None,
            make_stub_provider,
            make_recorder_provider,
            recorder_supervisor,
            recorder_config,
            recorder_config_path,
            recorder_config_note,
            recorder_editor: RecorderEditorState::default(),
            alerts_config: AlertConfig::default(),
            alerts_config_path: alerts::default_config_path(),
            alerts_config_note: String::from("Using in-memory alerts config."),
            alerts_editor: AlertEditorState::default(),
            manual_positions,
            manual_positions_path,
            manual_positions_note,
            manual_position_overlay: None,
            alert_last_sent_at: HashMap::new(),
            notifications: VecDeque::with_capacity(MAX_NOTIFICATIONS),
            notifications_overlay_visible: false,
            recorder_status: RecorderStatus::Disabled,
            calculator: CalculatorState::default(),
            calculator_tool: CalculatorTool::Basic,
            oddsmatcher_tx: runtime.oddsmatcher_tx,
            oddsmatcher_rx: runtime.oddsmatcher_rx,
            oddsmatcher_in_flight: false,
            oddsmatcher_pending_query: None,
            market_intel_tx: runtime.market_intel_tx,
            market_intel_rx: runtime.market_intel_rx,
            market_intel_in_flight: false,
            market_intel_resource_state: ResourceState::idle(),
            market_intel_pending_reason: None,
            last_market_intel_dispatch_at: None,
            owls_sync_tx: runtime.owls_sync_tx,
            owls_sync_rx: runtime.owls_sync_rx,
            owls_sync_in_flight: false,
            owls_resource_state: ResourceState::idle(),
            owls_sync_pending_reason: None,
            last_owls_sync_dispatch_at: None,
            matchbook_sync_tx: runtime.matchbook_sync_tx,
            matchbook_sync_rx: runtime.matchbook_sync_rx,
            matchbook_sync_in_flight: false,
            matchbook_resource_state: ResourceState::idle(),
            matchbook_sync_pending_reason: None,
            last_matchbook_sync_dispatch_at: None,
            matchbook_account_state: None,
            owls_dashboard: { OwlsDashboard::default() },
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
            intel_view: IntelView::Markets,
            intel_table_state: {
                let mut state = TableState::default();
                state.select(Some(0));
                state
            },
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
            last_event_at_label: None,
            mouse_targets: Vec::new(),
            running: true,
            status_message,
            status_scroll: 0,
            last_problem_console_message: None,
            recorder_startup_alerts_pending: true,
            recorder_startup_alerts_muted_until: None,
            wm: crate::wm::WindowManager::default(),
        };
        app.sync_workspace_context();
        app.refresh_snapshot_enrichment();
        app.request_market_intel_sync(MarketIntelSyncReason::Background);
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

    pub fn market_intel_dashboard(&self) -> Option<&MarketIntelDashboard> {
        self.market_intel_resource_state.last_good()
    }

    pub fn market_intel_phase(&self) -> &str {
        self.market_intel_resource_state.phase().as_str()
    }

    pub fn market_intel_last_error(&self) -> Option<&str> {
        self.market_intel_resource_state.last_error()
    }

    fn refresh_snapshot_enrichment(&mut self) {
        let previous_snapshot = self.snapshot.clone();
        let base_snapshot = self
            .provider_resource_state
            .last_good()
            .cloned()
            .unwrap_or_else(|| self.snapshot.clone());
        let owls_dashboard = self
            .owls_resource_state
            .last_good()
            .cloned()
            .unwrap_or_else(|| self.owls_dashboard.clone());
        let matchbook_account_state = self
            .matchbook_resource_state
            .last_good()
            .cloned()
            .or_else(|| self.matchbook_account_state.clone());
        let market_intel_dashboard = self.market_intel_resource_state.last_good().cloned();
        let mut projected_snapshot = project_snapshot(
            &base_snapshot,
            &owls_dashboard,
            matchbook_account_state.as_ref(),
            market_intel_dashboard.as_ref(),
        );
        if self.provider_resource_state.phase() == ResourcePhase::Error {
            projected_snapshot.status_line = previous_snapshot.status_line.clone();
            projected_snapshot.worker = previous_snapshot.worker.clone();
            if let Some(selected_venue) = previous_snapshot.selected_venue {
                if let Some(previous_venue) = previous_snapshot
                    .venues
                    .iter()
                    .find(|venue| venue.id == selected_venue)
                {
                    if let Some(projected_venue) = projected_snapshot
                        .venues
                        .iter_mut()
                        .find(|venue| venue.id == selected_venue)
                    {
                        projected_venue.status = previous_venue.status;
                        projected_venue.detail = previous_venue.detail.clone();
                    }
                }
            }
        }
        self.snapshot = projected_snapshot;
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

    pub fn intel_view(&self) -> IntelView {
        self.intel_view
    }

    pub fn intel_rows(&self) -> Vec<IntelRow> {
        intel_rows_for_view(self.intel_view, self.market_intel_dashboard())
    }

    pub fn selected_intel_row(&self) -> Option<IntelRow> {
        let rows = self.intel_rows();
        self.intel_table_state
            .selected()
            .and_then(|index| rows.get(index).cloned())
            .or_else(|| rows.first().cloned())
    }

    pub fn intel_table_state(&mut self) -> &mut TableState {
        &mut self.intel_table_state
    }

    pub fn intel_source_statuses(&self) -> Vec<IntelSourceStatus> {
        intel_source_statuses_for_view(
            self.intel_view,
            self.market_intel_dashboard(),
            self.market_intel_phase(),
            self.market_intel_last_error(),
        )
    }

    pub fn intel_ready_sources(&self) -> usize {
        self.intel_source_statuses()
            .into_iter()
            .filter(|status| status.health == "ready" || status.health == "fixture")
            .count()
    }

    pub fn intel_freshness_label(&self) -> String {
        if self.market_intel_dashboard().is_none() {
            return self.market_intel_phase().to_string();
        }

        let has_fixture = self
            .intel_source_statuses()
            .into_iter()
            .any(|status| status.freshness == "fixture");
        if has_fixture {
            String::from("fixture")
        } else {
            String::from("live")
        }
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
            self.notifications_overlay_visible = false;
            self.trading_action_overlay = None;
        }
    }

    pub fn active_trading_section(&self) -> TradingSection {
        self.trading_section
    }

    fn apply_trading_section_state(&mut self, section: TradingSection) {
        self.set_active_panel(Panel::Trading);
        self.trading_section = section;
        if section != TradingSection::Positions {
            self.live_view_overlay_visible = false;
        }
        if section == TradingSection::Alerts {
            self.mark_notifications_read();
        }
        if !self.is_owls_context() {
            self.markets_overlay_visible = false;
        }
        if !matches!(
            section,
            TradingSection::Positions | TradingSection::Matcher | TradingSection::Intel
        ) {
            self.trading_action_overlay = None;
        }
        if self.is_owls_context() {
            self.align_owls_selection_for_section();
            self.request_owls_sync(OwlsSyncReason::Background);
        }
        if section == TradingSection::Accounts {
            self.seed_selected_exchange_row_from_snapshot();
        }
        if section == TradingSection::Intel {
            self.clamp_selected_intel_row();
            self.request_market_intel_sync(MarketIntelSyncReason::Background);
        }
    }

    pub fn set_trading_section(&mut self, section: TradingSection) {
        self.apply_trading_section_state(section);
        let pane = pane_for_trading_section(section);
        if let Some(workspace_index) = self.wm.workspace_index_for_pane(pane) {
            self.wm.switch_workspace(workspace_index);
            self.wm.focus_pane(pane);
            self.wm.maximized_pane = self
                .wm
                .current_workspace()
                .is_minimized(pane)
                .then_some(pane);
        }
    }

    pub fn active_observability_section(&self) -> ObservabilitySection {
        self.observability_section
    }

    pub fn active_pane(&self) -> Option<PaneId> {
        self.wm.active_pane
    }

    pub fn help_text(&self) -> &'static str {
        "? keymap | n alerts | q quit | o observability | alt+1-3 workspaces | ctrl+left/right sections | h/j/k/l panes | arrows nav inside pane | tab rotate pane/tool | r refresh cache | R recapture live\nenter edit/open | p place action | a manual entry | esc cancel | [/] cycle sport or suggestions | u reload | D defaults | s start recorder | x stop recorder | c cash out | v live view | b cycle type | m toggle mode"
    }

    pub fn live_view_overlay_visible(&self) -> bool {
        self.live_view_overlay_visible
    }

    pub fn keymap_overlay_visible(&self) -> bool {
        self.keymap_overlay_visible
    }

    pub fn notifications_overlay_visible(&self) -> bool {
        self.notifications_overlay_visible
    }

    pub fn markets_overlay_visible(&self) -> bool {
        self.markets_overlay_visible
    }

    pub fn alerts_config(&self) -> &AlertConfig {
        &self.alerts_config
    }

    pub fn alerts_config_note(&self) -> &str {
        &self.alerts_config_note
    }

    pub fn alerts_selected_field(&self) -> AlertField {
        self.alerts_editor.selected_field()
    }

    pub fn alerts_is_editing(&self) -> bool {
        self.alerts_editor.editing
    }

    pub fn alerts_edit_buffer(&self) -> Option<&str> {
        if self.alerts_editor.editing {
            Some(self.alerts_editor.buffer.as_str())
        } else {
            None
        }
    }

    pub fn notifications(&self) -> &VecDeque<NotificationEntry> {
        &self.notifications
    }

    pub fn unread_notification_count(&self) -> usize {
        self.notifications
            .iter()
            .filter(|entry| entry.unread)
            .count()
    }

    pub fn latest_problem_notification(&self) -> Option<&NotificationEntry> {
        self.notifications.iter().rev().find(|entry| {
            matches!(
                entry.level,
                NotificationLevel::Warning | NotificationLevel::Critical
            )
        })
    }

    pub fn problem_notifications(&self) -> Vec<&NotificationEntry> {
        self.notifications
            .iter()
            .rev()
            .filter(|entry| {
                matches!(
                    entry.level,
                    NotificationLevel::Warning | NotificationLevel::Critical
                )
            })
            .collect()
    }

    pub fn current_minimized_panes(&self) -> &[PaneId] {
        &self.wm.current_workspace().minimized
    }

    pub fn trading_action_overlay(&self) -> Option<&TradingActionOverlayState> {
        self.trading_action_overlay.as_ref()
    }

    pub fn manual_position_overlay(&self) -> Option<&ManualPositionOverlayState> {
        self.manual_position_overlay.as_ref()
    }

    pub fn manual_positions_note(&self) -> &str {
        &self.manual_positions_note
    }

    pub fn toggle_live_view_overlay(&mut self) {
        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::Positions
        {
            self.live_view_overlay_visible = !self.live_view_overlay_visible;
            if self.live_view_overlay_visible {
                self.request_matchbook_sync(MatchbookSyncReason::Manual);
            }
        }
    }

    pub fn toggle_keymap_overlay(&mut self) {
        self.keymap_overlay_visible = !self.keymap_overlay_visible;
    }

    pub fn toggle_notifications_overlay(&mut self) {
        self.notifications_overlay_visible = !self.notifications_overlay_visible;
        if self.notifications_overlay_visible {
            self.mark_notifications_read();
        }
    }

    pub fn toggle_markets_overlay(&mut self) {
        if self.active_panel == Panel::Trading && self.is_owls_context() {
            self.markets_overlay_visible = !self.markets_overlay_visible;
        }
    }

    pub fn cycle_intel_view(&mut self, forward: bool) {
        let current = self.intel_view;
        let all = &IntelView::ALL;
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
        self.intel_view = all[next];
        self.clamp_selected_intel_row();
        self.status_message = format!("Intel view set to {}.", self.intel_view.label());
        self.status_scroll = 0;
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
        self.owls_resource_state
            .finish_ok(self.owls_dashboard.clone());
        self.align_owls_selection_for_section();
        self.markets_overlay_visible = false;
        self.request_owls_sync(OwlsSyncReason::Manual);
        // Sport changes should immediately requery the current operator slice instead of
        // waiting for the background interval or forcing a full backend refresh.
        self.request_market_intel_sync(MarketIntelSyncReason::Manual);
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
            self.drain_market_intel_results();
            self.drain_owls_sync_results();
            self.drain_matchbook_sync_results();
            if !self.provider_resource_state.is_loading()
                && !self.oddsmatcher_in_flight
                && !self.market_intel_resource_state.is_loading()
                && !self.owls_resource_state.is_loading()
                && !self.matchbook_resource_state.is_loading()
            {
                return true;
            }
            if started.elapsed() >= timeout {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[cfg(debug_assertions)]
    pub fn poll_recorder_for_test(&mut self) {
        self.poll_recorder();
    }

    #[cfg(debug_assertions)]
    pub fn poll_owls_dashboard_for_test(&mut self) {
        self.poll_owls_dashboard();
    }

    #[cfg(debug_assertions)]
    pub fn poll_matchbook_account_for_test(&mut self) {
        self.poll_matchbook_account();
    }

    #[cfg(debug_assertions)]
    pub fn poll_market_intel_for_test(&mut self) {
        self.poll_market_intel();
    }

    #[cfg(debug_assertions)]
    pub fn set_matchbook_state_for_test(&mut self, state: MatchbookAccountState) {
        self.matchbook_account_state = Some(state);
        if let Some(current) = self.matchbook_account_state.clone() {
            self.matchbook_resource_state.finish_ok(current);
        }
        self.refresh_snapshot_enrichment();
    }

    #[cfg(debug_assertions)]
    pub fn mark_matchbook_sync_in_flight_for_test(&mut self, started_at: Instant) {
        self.matchbook_sync_in_flight = true;
        self.last_matchbook_sync_dispatch_at = Some(started_at);
        self.matchbook_resource_state.begin_refresh(started_at);
    }

    #[cfg(debug_assertions)]
    pub fn matchbook_sync_in_flight_for_test(&self) -> bool {
        self.matchbook_resource_state.is_loading()
    }

    #[cfg(debug_assertions)]
    pub fn matchbook_status_for_test(&self) -> &'static str {
        self.matchbook_resource_state.phase().as_str()
    }

    #[cfg(debug_assertions)]
    pub fn set_owls_dashboard_for_test(&mut self, dashboard: OwlsDashboard) {
        self.owls_dashboard = dashboard;
        self.owls_resource_state
            .finish_ok(self.owls_dashboard.clone());
        self.refresh_snapshot_enrichment();
    }

    #[cfg(debug_assertions)]
    pub fn set_market_intel_dashboard_for_test(&mut self, dashboard: MarketIntelDashboard) {
        self.market_intel_resource_state.finish_ok(dashboard);
        self.clamp_selected_intel_row();
        self.refresh_snapshot_enrichment();
    }

    #[cfg(debug_assertions)]
    pub fn mark_owls_sync_in_flight_for_test(&mut self, started_at: Instant) {
        self.owls_sync_in_flight = true;
        self.last_owls_sync_dispatch_at = Some(started_at);
        self.owls_resource_state.begin_refresh(started_at);
    }

    #[cfg(debug_assertions)]
    pub fn owls_sync_in_flight_for_test(&self) -> bool {
        self.owls_resource_state.is_loading()
    }

    #[cfg(debug_assertions)]
    pub fn owls_status_for_test(&self) -> &'static str {
        self.owls_resource_state.phase().as_str()
    }

    #[cfg(debug_assertions)]
    pub fn mark_provider_in_flight_for_test(&mut self, started_at: Instant) {
        self.provider_in_flight = true;
        self.provider_pending_job = None;
        self.provider_in_flight_started_at_for_test = Some(started_at);
        self.provider_resource_state.begin_refresh(started_at);
    }

    #[cfg(debug_assertions)]
    pub fn provider_in_flight_for_test(&self) -> bool {
        self.provider_resource_state.is_loading()
    }

    #[cfg(debug_assertions)]
    pub fn provider_status_for_test(&self) -> &'static str {
        self.provider_resource_state.phase().as_str()
    }

    #[cfg(debug_assertions)]
    pub fn provider_pending_debug_label(&self) -> Option<String> {
        self.provider_pending_job
            .as_ref()
            .map(|job| format!("{:?}", job.request))
    }

    pub fn status_scroll(&self) -> u16 {
        self.status_scroll
    }

    pub fn clear_mouse_targets(&mut self) {
        self.mouse_targets.clear();
    }

    pub fn register_workspace_target(&mut self, rect: Rect, index: usize) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::Workspace(index),
        });
    }

    pub fn register_pane_target(&mut self, rect: Rect, pane: PaneId) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::Pane(pane),
        });
    }

    pub fn register_pane_minimize_target(&mut self, rect: Rect, pane: PaneId) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::PaneMinimize(pane),
        });
    }

    pub fn register_pane_toggle_maximize_target(&mut self, rect: Rect, pane: PaneId) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::PaneToggleMaximize(pane),
        });
    }

    pub fn register_trading_section_target(&mut self, rect: Rect, section: TradingSection) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::TradingSection(section),
        });
    }

    pub fn register_intel_view_target(&mut self, rect: Rect, view: IntelView) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::IntelView(view),
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

    pub fn register_minimized_pane_target(&mut self, rect: Rect, pane: PaneId) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::MinimizedPane(pane),
        });
    }

    pub fn register_minimize_active_pane_target(&mut self, rect: Rect) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::MinimizeActivePane,
        });
    }

    pub fn register_toggle_maximize_target(&mut self, rect: Rect) {
        self.mouse_targets.push(MouseTarget {
            rect,
            kind: MouseTargetKind::ToggleMaximize,
        });
    }

    pub fn can_minimize_active_pane(&self) -> bool {
        self.wm
            .active_pane
            .map(|pane| self.wm.can_minimize_pane(pane))
            .unwrap_or(false)
    }

    pub fn can_minimize_pane(&self, pane: PaneId) -> bool {
        self.wm.can_minimize_pane(pane)
    }

    pub fn minimize_active_pane(&mut self) -> bool {
        let Some(pane) = self.wm.active_pane else {
            return false;
        };

        if self.wm.minimize_pane(pane) {
            self.status_message = format!("Minimized {} to strip.", pane.title());
            self.status_scroll = 0;
            true
        } else {
            self.status_message = format!("{} cannot be minimized here.", pane.title());
            self.status_scroll = 0;
            false
        }
    }

    fn toggle_maximize_with_status(&mut self) {
        self.wm.toggle_maximize();
        if self.wm.maximized_pane.is_some() {
            self.status_message = "Pane maximized (click restore or Alt+f)".to_string();
        } else {
            self.status_message = "Pane restored".to_string();
        }
        self.status_scroll = 0;
    }

    fn toggle_pane_maximize_with_status(&mut self, pane: PaneId) {
        self.wm.active_pane = Some(pane);
        if self.wm.maximized_pane == Some(pane) {
            self.wm.maximized_pane = None;
            self.status_message = format!("{} restored.", pane.title());
        } else {
            self.wm.maximized_pane = Some(pane);
            self.apply_pane_context(pane);
            self.status_message = format!("{} maximized.", pane.title());
        }
        self.status_scroll = 0;
    }

    fn minimize_pane_with_status(&mut self, pane: PaneId) {
        self.wm.active_pane = Some(pane);
        if self.wm.minimize_pane(pane) {
            self.apply_pane_context(self.wm.active_pane.unwrap_or(pane));
            self.status_message = format!("Minimized {} to strip.", pane.title());
        } else {
            self.status_message = format!("{} cannot be minimized here.", pane.title());
        }
        self.status_scroll = 0;
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

    pub fn positions_render_state(&mut self) -> PositionsRenderState<'_> {
        let Self {
            snapshot,
            owls_dashboard,
            matchbook_account_state,
            open_position_table_state,
            historical_position_table_state,
            positions_focus,
            live_view_overlay_visible,
            status_message,
            status_scroll,
            ..
        } = self;
        PositionsRenderState {
            snapshot,
            owls_dashboard,
            matchbook_account_state: matchbook_account_state.as_ref(),
            open_table_state: open_position_table_state,
            historical_table_state: historical_position_table_state,
            positions_focus: *positions_focus,
            show_live_view_overlay: *live_view_overlay_visible,
            status_message,
            status_scroll: *status_scroll,
        }
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

    pub fn last_event_at_label(&self) -> Option<&str> {
        self.last_event_at_label.as_deref()
    }

    pub fn top_bar_ticker_parts(&self) -> (&'static str, String) {
        top_bar_ticker_parts(&self.snapshot, &self.owls_dashboard)
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
            betslip_event_id: Some(row.event_id.clone()),
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
        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::Intel {
            return self.refresh_intel(false);
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
        if self.active_panel == Panel::Trading && self.trading_section == TradingSection::Intel {
            return self.refresh_intel(true);
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

    fn refresh_intel(&mut self, live: bool) -> Result<()> {
        let reason = if live {
            MarketIntelSyncReason::Manual
        } else {
            MarketIntelSyncReason::Manual
        };
        self.request_market_intel_sync(reason);
        self.status_message = format!("Intel {} refresh queued.", self.intel_view.label());
        self.status_scroll = 0;
        self.record_event(format!(
            "Intel {} refresh queued.",
            if live { "live" } else { "cached" }
        ));
        Ok(())
    }

    fn queue_provider_request(&mut self, job: ProviderJob) {
        debug!(request = ?job.request, in_flight = self.provider_in_flight, "queue provider request");
        self.drain_provider_results();
        if self.provider_resource_state.is_loading() {
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
        debug!(request = ?request, "dispatch provider request");
        match self.provider_tx.send(job) {
            Ok(()) => {
                self.provider_in_flight = true;
                self.provider_resource_state.begin_refresh_now();
                #[cfg(debug_assertions)]
                {
                    self.provider_in_flight_started_at_for_test = Some(Instant::now());
                }
                self.status_message = provider_queue_message(&request);
                self.status_scroll = 0;
            }
            Err(error) => {
                warn!(request = ?request, error = %error, "provider worker unavailable");
                self.status_message = format!("Provider worker unavailable: {error}");
                self.status_scroll = 0;
                self.record_event("Provider worker unavailable.");
            }
        }
    }

    fn restart_provider_worker(&mut self, provider: Box<dyn ExchangeProvider + Send>) {
        info!("restart provider worker");
        let pending_job = self.provider_pending_job.take();
        let _ = self
            .provider_resource_state
            .expire_if_overdue(Duration::ZERO, "provider worker restarted");
        let (provider_tx, provider_rx) =
            AppRuntimeChannels::start_provider(&self.runtime_host, provider);
        self.provider_tx = provider_tx;
        self.provider_rx = provider_rx;
        self.provider_in_flight = false;
        #[cfg(debug_assertions)]
        {
            self.provider_in_flight_started_at_for_test = None;
        }
        if let Some(job) = pending_job {
            self.dispatch_provider_request(job);
        }
    }

    fn restart_owls_sync_worker(&mut self) {
        info!("restart owls worker");
        let (owls_sync_tx, owls_sync_rx) =
            AppRuntimeChannels::start_owls(&self.runtime_host, default_owls_sync_async_client());
        self.owls_sync_tx = owls_sync_tx;
        self.owls_sync_rx = owls_sync_rx;
        self.owls_sync_in_flight = false;
    }

    fn restart_matchbook_sync_worker(&mut self) {
        info!("restart matchbook worker");
        let (matchbook_sync_tx, matchbook_sync_rx) =
            AppRuntimeChannels::start_matchbook(&self.runtime_host);
        self.matchbook_sync_tx = matchbook_sync_tx;
        self.matchbook_sync_rx = matchbook_sync_rx;
        self.matchbook_sync_in_flight = false;
    }

    fn restart_market_intel_worker(&mut self) {
        info!("restart market intel worker");
        let (market_intel_tx, market_intel_rx) =
            AppRuntimeChannels::start_market_intel(&self.runtime_host);
        self.market_intel_tx = market_intel_tx;
        self.market_intel_rx = market_intel_rx;
        self.market_intel_in_flight = false;
    }

    fn current_provider_for_watchdog(&self) -> Box<dyn ExchangeProvider + Send> {
        if self.recorder_status == RecorderStatus::Running {
            (self.make_recorder_provider)(&self.recorder_config)
        } else {
            (self.make_stub_provider)()
        }
    }

    fn drain_provider_results(&mut self) {
        let mut latest_result = None;
        while let Ok(result) = self.provider_rx.try_recv() {
            latest_result = Some(result);
        }
        let Some(result) = latest_result else {
            return;
        };

        debug!(request = ?result.request, success = result.result.is_ok(), "drain provider result");
        self.provider_in_flight = false;
        #[cfg(debug_assertions)]
        {
            self.provider_in_flight_started_at_for_test = None;
        }
        match result.result {
            Ok(snapshot) => {
                self.provider_resource_state.finish_ok(snapshot.clone());
                self.apply_provider_snapshot_result(result.request, snapshot, result.event_message)
            }
            Err(error) => {
                self.provider_resource_state.finish_error(error.clone());
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
        let placed_bet_detail = match &request {
            ProviderRequest::ExecuteTradingAction { intent }
                if self.alerts_config.bet_placed && intent.mode == TradingActionMode::Confirm =>
            {
                Some(format!(
                    "{} {} @ {:.2} stake {:.2}",
                    intent.venue.as_str(),
                    intent.selection_name,
                    intent.expected_price,
                    intent.stake
                ))
            }
            _ => None,
        };
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

        if let Some(detail) = placed_bet_detail {
            self.emit_alert("bet_placed", NotificationLevel::Info, "Bet placed", detail);
        }

        if let Some(message) = event_message {
            self.record_event(message);
        }
    }

    fn request_matchbook_sync(&mut self, reason: MatchbookSyncReason) {
        debug!(
            reason = reason.label(),
            in_flight = self.matchbook_sync_in_flight,
            "request matchbook sync"
        );
        self.drain_matchbook_sync_results();
        if self.matchbook_resource_state.is_loading() {
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
                self.matchbook_resource_state.begin_refresh_now();
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

        debug!(
            reason = result.reason.label(),
            success = result.state.is_ok(),
            "drain matchbook sync result"
        );
        self.matchbook_sync_in_flight = false;
        match result.state {
            Ok(state) => {
                self.matchbook_resource_state.finish_ok(state.clone());
                let first_load = self.matchbook_account_state.is_none();
                self.matchbook_account_state = Some(state.clone());
                self.refresh_snapshot_enrichment();
                if matches!(result.reason, MatchbookSyncReason::Manual) || first_load {
                    self.status_message = state.status_line.clone();
                    self.status_scroll = 0;
                    self.record_event(format!("Matchbook {} sync applied.", result.reason.label()));
                }
            }
            Err(error) => {
                self.matchbook_resource_state.finish_error(error.clone());
                if matches!(result.reason, MatchbookSyncReason::Manual) {
                    self.status_message = format!("Matchbook sync failed: {error}");
                    self.status_scroll = 0;
                }
                self.record_event(format!("Matchbook sync failed: {error}"));
                if self.alerts_config.matchbook_failures {
                    self.emit_alert(
                        "matchbook_failures",
                        NotificationLevel::Warning,
                        "Matchbook sync failed",
                        error,
                    );
                }
            }
        }

        if let Some(reason) = self.matchbook_sync_pending_reason.take() {
            self.dispatch_matchbook_sync(reason);
        }
    }

    fn request_market_intel_sync(&mut self, reason: MarketIntelSyncReason) {
        debug!(
            reason = reason.label(),
            in_flight = self.market_intel_in_flight,
            "request market intel sync"
        );
        self.drain_market_intel_results();
        if self.market_intel_resource_state.is_loading() {
            self.market_intel_pending_reason =
                Some(match (self.market_intel_pending_reason, reason) {
                    (Some(MarketIntelSyncReason::Manual), _)
                    | (_, MarketIntelSyncReason::Manual) => MarketIntelSyncReason::Manual,
                    _ => MarketIntelSyncReason::Background,
                });
            return;
        }
        self.dispatch_market_intel_sync(reason);
    }

    fn dispatch_market_intel_sync(&mut self, reason: MarketIntelSyncReason) {
        match self.market_intel_tx.send(MarketIntelJob {
            reason,
            sport_key: self.current_market_intel_sport_key(),
        }) {
            Ok(()) => {
                self.market_intel_in_flight = true;
                self.market_intel_resource_state.begin_refresh_now();
                self.last_market_intel_dispatch_at = Some(Instant::now());
            }
            Err(error) => {
                self.status_message = format!("Market intel worker unavailable: {error}");
                self.status_scroll = 0;
                self.record_event("Market intel worker unavailable.");
            }
        }
    }

    fn current_market_intel_sport_key(&self) -> Option<String> {
        let sport = self.owls_dashboard.sport.trim();
        if !sport.is_empty() {
            return Some(sport.to_ascii_lowercase());
        }

        inferred_owls_sport(&self.snapshot).map(str::to_string)
    }

    fn drain_market_intel_results(&mut self) {
        let mut latest_result = None;
        while let Ok(result) = self.market_intel_rx.try_recv() {
            latest_result = Some(result);
        }
        let Some(result) = latest_result else {
            return;
        };

        debug!(
            reason = result.reason.label(),
            success = result.dashboard.is_ok(),
            "drain market intel result"
        );
        self.market_intel_in_flight = false;
        match result.dashboard {
            Ok(dashboard) => {
                self.market_intel_resource_state
                    .finish_ok(dashboard.clone());
                self.refresh_snapshot_enrichment();
                if matches!(result.reason, MarketIntelSyncReason::Manual) {
                    self.status_message = dashboard.status_line.clone();
                    self.status_scroll = 0;
                    self.record_event(format!(
                        "Market intel {} sync applied.",
                        result.reason.label()
                    ));
                }
                self.clamp_selected_intel_row();
            }
            Err(error) => {
                self.market_intel_resource_state.finish_error(error.clone());
                if matches!(result.reason, MarketIntelSyncReason::Manual) {
                    self.status_message = format!("Market intel sync failed: {error}");
                    self.status_scroll = 0;
                }
                self.record_event(format!("Market intel sync failed: {error}"));
            }
        }

        if let Some(reason) = self.market_intel_pending_reason.take() {
            self.dispatch_market_intel_sync(reason);
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

    fn open_manual_position_overlay_from_positions(&mut self) {
        if self.active_panel != Panel::Trading
            || self.trading_section != TradingSection::Positions
            || self.positions_focus != PositionsFocus::Active
        {
            return;
        }
        let Some(selected_index) = self.selected_open_position_row() else {
            self.status_message = String::from("No active position is selected.");
            return;
        };
        let Some(open_position) = self.snapshot.open_positions.get(selected_index) else {
            self.status_message = String::from("The selected position is out of range.");
            return;
        };

        let mut draft = self
            .manual_positions
            .iter()
            .find(|entry| {
                normalize_manual_key(&entry.event) == normalize_manual_key(&open_position.event)
                    && normalize_manual_key(&entry.selection)
                        == normalize_manual_key(&open_position.contract)
            })
            .cloned()
            .unwrap_or_default();
        if draft.event.trim().is_empty() {
            draft.event = open_position.event.clone();
        }
        if draft.market.trim().is_empty() {
            draft.market = open_position.market.clone();
        }
        if draft.selection.trim().is_empty() {
            draft.selection = open_position.contract.clone();
        }
        self.manual_position_overlay = Some(ManualPositionOverlayState::new(draft));
        self.status_message = String::from("Manual position editor opened.");
    }

    fn save_manual_position_overlay(&mut self) -> Result<()> {
        let Some(overlay) = self.manual_position_overlay.as_ref() else {
            return Ok(());
        };
        let entry = overlay.draft.clone();
        if entry.event.trim().is_empty()
            || entry.market.trim().is_empty()
            || entry.selection.trim().is_empty()
            || entry.venue.trim().is_empty()
            || entry.odds <= 0.0
            || entry.stake <= 0.0
        {
            return Err(color_eyre::eyre::eyre!(
                "Manual entry requires event, market, selection, venue, odds, and stake."
            ));
        }

        let key = entry.display_key();
        if let Some(existing) = self
            .manual_positions
            .iter_mut()
            .find(|existing| existing.display_key() == key)
        {
            *existing = entry;
        } else {
            self.manual_positions.push(entry);
        }
        self.manual_positions_note =
            save_entries(&self.manual_positions_path, &self.manual_positions)?;
        self.snapshot = normalize_snapshot(
            self.snapshot.clone(),
            &self.recorder_config.disabled_venues,
            &self.manual_positions,
        );
        self.refresh_snapshot_enrichment();
        self.manual_position_overlay = None;
        self.status_message = String::from("Manual position saved.");
        self.status_scroll = 0;
        Ok(())
    }

    fn is_manual_position_overlay_active(&self) -> bool {
        self.manual_position_overlay.is_some()
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
        self.recorder_startup_alerts_pending = true;
        self.recorder_startup_alerts_muted_until =
            Some(Instant::now() + RECORDER_STARTUP_ALERT_MUTE);
        self.queue_provider_request(ProviderJob {
            request: ProviderRequest::LoadDashboard,
            failure_context: String::from("Recorder dashboard load failed"),
            event_message: Some(String::from("Recorder dashboard loaded.")),
        });
        self.status_message = String::from("Recorder started; waiting for first snapshot.");
        self.status_scroll = 0;
        self.set_trading_section(TradingSection::Positions);
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
        self.recorder_startup_alerts_pending = false;
        self.recorder_startup_alerts_muted_until = None;
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

    fn dismiss_top_overlay(&mut self) -> bool {
        if self.keymap_overlay_visible {
            self.keymap_overlay_visible = false;
            return true;
        }
        if self.notifications_overlay_visible {
            self.notifications_overlay_visible = false;
            return true;
        }
        if self.markets_overlay_visible {
            self.markets_overlay_visible = false;
            return true;
        }
        if self.live_view_overlay_visible {
            self.live_view_overlay_visible = false;
            return true;
        }
        if self.manual_position_overlay.is_some() {
            self.manual_position_overlay = None;
            return true;
        }
        if self.is_trading_action_overlay_active() {
            self.close_trading_action_overlay("Cancelled trading action.");
            return true;
        }
        false
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
                self.focus_trading_section(next_from(self.trading_section, &TradingSection::ALL))
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
        if !matches!(
            self.trading_section,
            TradingSection::Positions | TradingSection::Matcher | TradingSection::Intel
        ) {
            self.trading_action_overlay = None;
        }
    }

    pub fn previous_section(&mut self) {
        match self.active_panel {
            Panel::Trading => self
                .focus_trading_section(previous_from(self.trading_section, &TradingSection::ALL)),
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
        if !matches!(
            self.trading_section,
            TradingSection::Positions | TradingSection::Matcher | TradingSection::Intel
        ) {
            self.trading_action_overlay = None;
        }
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        <B as ratatui::backend::Backend>::Error: std::marker::Send + std::marker::Sync + 'static,
    {
        use crossterm::event::KeyModifiers;
        while self.running {
            self.poll_recorder();
            self.drain_provider_results();
            self.drain_oddsmatcher_results();
            self.poll_market_intel();
            self.poll_owls_dashboard();
            self.poll_matchbook_account();
            self.sync_problem_console_from_status();
            terminal.draw(|frame| self.render(frame))?;

            if event::poll(Duration::from_millis(250))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        if key.modifiers.contains(KeyModifiers::ALT) {
                            self.handle_alt_key(key.code);
                        } else if key.modifiers.contains(KeyModifiers::CONTROL) {
                            self.handle_ctrl_key(key.code);
                        } else {
                            self.handle_key_code(key.code);
                        }
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
                self.owls_resource_state.begin_refresh_now();
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
        debug!(
            reason = reason.label(),
            in_flight = self.owls_sync_in_flight,
            "request owls sync"
        );
        self.drain_owls_sync_results();
        if self.owls_resource_state.is_loading() {
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

        debug!(
            reason = result.reason.label(),
            checked = result.outcome.checked_count,
            changed = result.outcome.changed_count,
            "drain owls sync result"
        );
        self.owls_sync_in_flight = false;
        let outcome = result.outcome;
        let selected_sport = self.owls_dashboard.sport.clone();
        if outcome.dashboard.sport != selected_sport {
            self.owls_resource_state
                .finish_ok(self.owls_dashboard.clone());
            self.record_event(format!(
                "Discarded stale Owls {} sync for {} while {} is selected.",
                result.reason.label(),
                outcome.dashboard.sport,
                selected_sport
            ));
            if let Some(reason) = self.owls_sync_pending_reason.take() {
                self.dispatch_owls_sync(reason);
            }
            return;
        }
        let previous_dashboard = self.owls_dashboard.clone();
        let previous_error_count = self
            .owls_dashboard
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.status == "error")
            .count();
        self.owls_dashboard = outcome.dashboard;
        self.owls_resource_state
            .finish_ok(self.owls_dashboard.clone());
        self.clamp_selected_owls_endpoint();
        self.refresh_snapshot_enrichment();

        if matches!(result.reason, OwlsSyncReason::Manual) {
            self.status_message = self.owls_dashboard.status_line.clone();
            self.status_scroll = 0;
        }
        if outcome.changed && matches!(result.reason, OwlsSyncReason::Manual) {
            self.record_event(format!(
                "Owls {} sync applied {} changes after {} checks.",
                result.reason.label(),
                outcome.changed_count,
                outcome.checked_count
            ));
        }
        let current_error_count = self
            .owls_dashboard
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.status == "error")
            .count();
        if self.alerts_config.owls_errors && current_error_count > previous_error_count {
            let detail = self
                .owls_dashboard
                .endpoints
                .iter()
                .find(|endpoint| endpoint.status == "error")
                .map(|endpoint| format!("{} {}", endpoint.label, endpoint.detail))
                .unwrap_or_else(|| format!("{current_error_count} Owls endpoints errored."));
            self.emit_alert(
                "owls_errors",
                NotificationLevel::Warning,
                "Owls endpoint error",
                detail,
            );
        }
        if self.alerts_config.opportunity_detected {
            if let Some(detail) = live_sharp_opportunity_alert_detail(
                &self.snapshot,
                &previous_dashboard,
                &self.owls_dashboard,
                self.alerts_config.opportunity_threshold_pct,
            ) {
                self.emit_alert(
                    "opportunity_detected",
                    NotificationLevel::Warning,
                    "Opportunity detected",
                    detail,
                );
            }
        }
        if self.alerts_config.watched_movement {
            if let Some(detail) = sharp_watch_movement_alert_detail(
                &self.snapshot,
                &previous_dashboard,
                &self.owls_dashboard,
                self.alerts_config.watched_movement_threshold_pct,
            ) {
                self.emit_alert(
                    "watched_movement",
                    NotificationLevel::Warning,
                    "Watched result moved",
                    detail,
                );
            }
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
        if self.is_manual_position_overlay_active() {
            match key_code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.manual_position_overlay = None;
                    self.status_message = String::from("Manual position editor closed.");
                    return;
                }
                KeyCode::Backspace => {
                    if let Some(overlay) = self.manual_position_overlay.as_mut() {
                        if overlay.editing {
                            overlay.backspace();
                        }
                    }
                    return;
                }
                KeyCode::Enter => {
                    let Some(overlay) = self.manual_position_overlay.as_mut() else {
                        return;
                    };
                    if overlay.editing {
                        if let Err(error) = overlay.apply_edit() {
                            self.status_message = format!("Manual position input error: {error}");
                        }
                    } else if overlay.selected_field() == ManualPositionField::Save {
                        if let Err(error) = self.save_manual_position_overlay() {
                            self.status_message = format!("Manual position save failed: {error}");
                        }
                    } else {
                        overlay.begin_edit();
                    }
                    return;
                }
                KeyCode::Up => {
                    if let Some(overlay) = self.manual_position_overlay.as_mut() {
                        if !overlay.editing {
                            overlay.select_previous_field();
                        }
                    }
                    return;
                }
                KeyCode::Down => {
                    if let Some(overlay) = self.manual_position_overlay.as_mut() {
                        if !overlay.editing {
                            overlay.select_next_field();
                        }
                    }
                    return;
                }
                KeyCode::Char(character) => {
                    if let Some(overlay) = self.manual_position_overlay.as_mut() {
                        if overlay.editing && !character.is_control() {
                            overlay.push_char(character);
                        }
                    }
                    return;
                }
                _ => return,
            }
        }

        if self.is_trading_action_overlay_active() {
            match key_code {
                KeyCode::Esc | KeyCode::Char('q') => {
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
                KeyCode::Left | KeyCode::Char('[') => {
                    if let Err(error) = self.trading_action_shift(false) {
                        self.status_message = format!("Trading action failed: {error}");
                    }
                    return;
                }
                KeyCode::Right | KeyCode::Char(']') => {
                    if let Err(error) = self.trading_action_shift(true) {
                        self.status_message = format!("Trading action failed: {error}");
                    }
                    return;
                }
                KeyCode::Up => {
                    if let Some(overlay) = self.trading_action_overlay.as_mut() {
                        overlay.select_previous_field();
                        self.status_message = format!(
                            "Trading action field set to {}.",
                            overlay.selected_field().label()
                        );
                    }
                    return;
                }
                KeyCode::Down => {
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

        if self.is_alerts_editing_context() {
            match key_code {
                KeyCode::Esc => {
                    self.cancel_alerts_edit();
                    return;
                }
                KeyCode::Enter => {
                    if let Err(error) = self.apply_alerts_edit() {
                        self.status_message = format!("Alerts config error: {error}");
                    }
                    return;
                }
                KeyCode::Backspace => {
                    self.alerts_backspace();
                    return;
                }
                KeyCode::Char(character) => {
                    self.alerts_push_char(character);
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

        if self.active_panel == Panel::Trading
            && self.trading_section == TradingSection::Matcher
            && key_code == KeyCode::Tab
        {
            self.cycle_matcher_view(true);
            return;
        }

        if self.active_panel == Panel::Trading
            && self.trading_section == TradingSection::Intel
            && key_code == KeyCode::Tab
        {
            self.cycle_intel_view(true);
            return;
        }

        if self.is_calculator_context() && key_code == KeyCode::Tab {
            self.cycle_calculator_tool(true);
            return;
        }

        match key_code {
            KeyCode::Char('?') => self.toggle_keymap_overlay(),
            KeyCode::Char('n') => self.toggle_notifications_overlay(),
            KeyCode::Char('q') => {
                if !self.dismiss_top_overlay() {
                    self.request_quit();
                }
            }
            KeyCode::Esc => {
                if !self.dismiss_top_overlay() {
                    self.request_quit();
                }
            }
            KeyCode::Char('o') => self.toggle_observability_panel(),
            KeyCode::Char('h') => self.navigate_pane(NavDirection::Left),
            KeyCode::Char('j') => self.navigate_pane(NavDirection::Down),
            KeyCode::Char('k') => self.navigate_pane(NavDirection::Up),
            KeyCode::Char('l') => self.navigate_pane(NavDirection::Right),
            KeyCode::Enter => {
                if self.is_oddsmatcher_filters_context() {
                    self.begin_oddsmatcher_edit();
                } else if self.is_oddsmatcher_results_context() {
                    self.load_calculator_from_selected_oddsmatcher();
                } else if self.is_horse_matcher_filters_context() {
                    self.begin_horse_matcher_edit();
                } else if self.is_horse_matcher_results_context() {
                    self.load_calculator_from_selected_horse_matcher();
                } else if self.is_intel_context() {
                    self.load_calculator_from_selected_intel();
                } else if self.active_panel == Panel::Trading && self.is_owls_context() {
                    if let Some(endpoint) = self.selected_owls_endpoint() {
                        self.status_message = format!(
                            "{} {} [{}] {}",
                            endpoint.method, endpoint.path, endpoint.status, endpoint.description
                        );
                        self.markets_overlay_visible = true;
                    }
                } else if self.active_panel == Panel::Trading
                    && self.trading_section == TradingSection::Accounts
                {
                    self.sync_selected_venue();
                } else if self.active_panel == Panel::Trading
                    && self.trading_section == TradingSection::Positions
                    && self.positions_focus == PositionsFocus::Active
                {
                    self.open_trading_action_overlay_from_positions();
                } else if self.is_alerts_context() {
                    self.begin_alerts_edit();
                } else if self.is_recorder_context() {
                    self.begin_recorder_edit();
                } else if self.is_calculator_context() {
                    self.begin_calculator_edit();
                }
            }
            KeyCode::Char('a') => {
                if self.active_panel == Panel::Trading
                    && self.trading_section == TradingSection::Positions
                    && self.positions_focus == PositionsFocus::Active
                {
                    self.open_manual_position_overlay_from_positions();
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
                } else if self.is_intel_context() {
                    self.open_trading_action_overlay_from_intel();
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
                } else if self.is_alerts_context() {
                    if let Err(error) = self.cycle_alerts_suggestion(false) {
                        self.status_message = format!("Alerts suggestion failed: {error}");
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
                } else if self.is_alerts_context() {
                    if let Err(error) = self.cycle_alerts_suggestion(true) {
                        self.status_message = format!("Alerts suggestion failed: {error}");
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
                } else if self.is_alerts_context() {
                    if let Err(error) = self.reload_alerts_config() {
                        self.status_message = format!("Alerts reload failed: {error}");
                    }
                } else {
                    self.status_message =
                        String::from("Open Trading > Recorder or Alerts to reload config.");
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
                } else if self.is_alerts_context() {
                    if let Err(error) = self.reset_alerts_config() {
                        self.status_message = format!("Alerts reset failed: {error}");
                    }
                } else {
                    self.status_message =
                        String::from("Open Trading > Recorder or Alerts to reset config.");
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
            KeyCode::Down => match (self.active_panel, self.trading_section) {
                (Panel::Trading, TradingSection::Accounts) => self.select_next_exchange_row(),
                (Panel::Trading, TradingSection::Positions) => self.select_next_positions_row(),
                (Panel::Trading, TradingSection::Markets)
                | (Panel::Trading, TradingSection::Live)
                | (Panel::Trading, TradingSection::Props) => self.select_next_owls_endpoint(),
                (Panel::Trading, TradingSection::Intel) => self.select_next_intel_row(),
                (Panel::Trading, TradingSection::Matcher) => self.select_next_matcher_row(),
                (Panel::Trading, TradingSection::Alerts) => self.alerts_editor.select_next_field(),
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
                (Panel::Trading, TradingSection::Positions) => self.select_previous_positions_row(),
                (Panel::Trading, TradingSection::Markets)
                | (Panel::Trading, TradingSection::Live)
                | (Panel::Trading, TradingSection::Props) => self.select_previous_owls_endpoint(),
                (Panel::Trading, TradingSection::Intel) => self.select_previous_intel_row(),
                (Panel::Trading, TradingSection::Matcher) => self.select_previous_matcher_row(),
                (Panel::Trading, TradingSection::Alerts) => {
                    self.alerts_editor.select_previous_field()
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

    fn handle_ctrl_key(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Left => self.previous_section(),
            KeyCode::Right => self.next_section(),
            _ => {}
        }
    }

    fn handle_alt_key(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char('1') => {
                self.switch_workspace_with_status(0);
            }
            KeyCode::Char('2') => {
                if self.wm.workspaces.len() > 1 {
                    self.switch_workspace_with_status(1);
                }
            }
            KeyCode::Char('3') => {
                if self.wm.workspaces.len() > 2 {
                    self.switch_workspace_with_status(2);
                }
            }
            KeyCode::Char('f') => {
                self.toggle_maximize_with_status();
            }
            KeyCode::Up => {
                self.navigate_pane(NavDirection::Up);
            }
            KeyCode::Down => {
                self.navigate_pane(NavDirection::Down);
            }
            KeyCode::Left => {
                self.navigate_pane(NavDirection::Left);
            }
            KeyCode::Right => {
                self.navigate_pane(NavDirection::Right);
            }
            _ => {}
        }
    }

    fn switch_workspace_with_status(&mut self, index: usize) {
        if let Some(pane) = self.wm.switch_workspace(index) {
            self.apply_pane_context(pane);
            self.status_message = format!(
                "Switched to {} / {}",
                self.wm.current_workspace().name,
                pane.title()
            );
        }
    }

    fn navigate_pane(&mut self, direction: NavDirection) {
        if let Some(next) = self.wm.focus_neighbor(direction) {
            self.apply_pane_context(next);
            self.status_message = format!("Focused {}", next.title());
        }
    }

    fn sync_workspace_context(&mut self) {
        if let Some(pane) = self.wm.active_pane {
            self.apply_pane_context(pane);
        }
    }

    fn focus_trading_section(&mut self, section: TradingSection) {
        let pane = pane_for_trading_section(section);
        if let Some(workspace_index) = self.wm.workspace_index_for_pane(pane) {
            self.wm.switch_workspace(workspace_index);
            self.wm.focus_pane(pane);
        }

        self.apply_pane_context(pane);
    }

    fn apply_pane_context(&mut self, pane: PaneId) {
        match pane {
            PaneId::Observability => {
                self.set_active_panel(Panel::Observability);
            }
            PaneId::History => {
                self.positions_focus = PositionsFocus::Historical;
                self.apply_trading_section_state(TradingSection::Positions);
            }
            PaneId::Positions => {
                self.positions_focus = PositionsFocus::Active;
                self.apply_trading_section_state(TradingSection::Positions);
            }
            _ => {
                self.set_active_panel(Panel::Trading);
                self.set_trading_section(trading_section_for_pane(pane));
            }
        }
    }

    fn is_recorder_context(&self) -> bool {
        self.active_panel == Panel::Trading && self.trading_section == TradingSection::Recorder
    }

    fn is_alerts_context(&self) -> bool {
        self.active_panel == Panel::Trading && self.trading_section == TradingSection::Alerts
    }

    fn supports_status_scroll(&self) -> bool {
        self.active_panel == Panel::Trading
            && matches!(
                self.trading_section,
                TradingSection::Positions | TradingSection::Recorder
            )
    }

    fn is_alerts_editing_context(&self) -> bool {
        self.is_alerts_context() && self.alerts_editor.editing
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

    fn is_intel_context(&self) -> bool {
        self.active_panel == Panel::Trading && self.trading_section == TradingSection::Intel
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

    fn seed_selected_exchange_row_from_snapshot(&mut self) {
        if self.snapshot.venues.is_empty() {
            self.exchange_list_state.select(None);
            return;
        }

        if self
            .exchange_list_state
            .selected()
            .is_some_and(|index| index < self.snapshot.venues.len())
        {
            return;
        }

        let selected_index = self
            .snapshot
            .selected_venue
            .and_then(|selected_venue| {
                self.snapshot
                    .venues
                    .iter()
                    .position(|venue| venue.id == selected_venue)
            })
            .unwrap_or(0);
        self.exchange_list_state.select(Some(selected_index));
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

    fn clamp_selected_intel_row(&mut self) {
        let row_count = self.intel_rows().len();
        if row_count == 0 {
            self.intel_table_state.select(None);
            return;
        }

        match self.intel_table_state.selected() {
            Some(index) if index < row_count => {}
            _ => self.intel_table_state.select(Some(0)),
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

    pub fn select_next_intel_row(&mut self) {
        let row_count = self.intel_rows().len();
        if row_count == 0 {
            self.intel_table_state.select(None);
            return;
        }

        let next_index = match self.intel_table_state.selected() {
            Some(index) if index + 1 < row_count => index + 1,
            Some(index) => index,
            None => 0,
        };

        self.intel_table_state.select(Some(next_index));
    }

    pub fn select_previous_intel_row(&mut self) {
        if self.intel_rows().is_empty() {
            self.intel_table_state.select(None);
            return;
        }

        let previous_index = match self.intel_table_state.selected() {
            Some(index) if index > 0 => index - 1,
            Some(index) => index,
            None => 0,
        };

        self.intel_table_state.select(Some(previous_index));
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
                self.wm.focus_pane(PaneId::Positions);
                if self.open_position_table_state.selected().is_none()
                    && active_position_row_count(&self.snapshot) > 0
                {
                    self.open_position_table_state.select(Some(0));
                }
            }
            PositionsFocus::Historical => {
                self.wm.focus_pane(PaneId::History);
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

    fn preferred_owls_endpoint_id(&self) -> OwlsEndpointId {
        match self.trading_section {
            TradingSection::Live => {
                if self.owls_dashboard.sport == "soccer" {
                    OwlsEndpointId::ScoresSport
                } else {
                    OwlsEndpointId::Realtime
                }
            }
            TradingSection::Props => OwlsEndpointId::Props,
            TradingSection::Markets => {
                if self.owls_dashboard.sport == "soccer" {
                    OwlsEndpointId::ScoresSport
                } else {
                    OwlsEndpointId::Odds
                }
            }
            _ => OwlsEndpointId::Odds,
        }
    }

    fn align_owls_selection_for_section(&mut self) {
        let preferred_id = self.preferred_owls_endpoint_id();
        let visible = self.visible_owls_endpoints();
        if let Some(index) = visible
            .iter()
            .position(|endpoint| endpoint.id == preferred_id)
        {
            self.owls_endpoint_table_state.select(Some(index));
            return;
        }

        self.clamp_selected_owls_endpoint();
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
                if self.alerts_config.recorder_failures {
                    self.emit_alert(
                        "recorder_failures",
                        if next_status == RecorderStatus::Error {
                            NotificationLevel::Critical
                        } else {
                            NotificationLevel::Warning
                        },
                        "Recorder degraded",
                        format!("{previous_status:?} -> {next_status:?}"),
                    );
                }
            }
        }

        self.expire_stuck_provider_request_placeholder();

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
        self.expire_stuck_owls_sync_placeholder();
        let should_sync = self.active_panel == Panel::Trading
            && (self.is_owls_context()
                || (self.trading_section == TradingSection::Positions
                    && self.live_view_overlay_visible));
        if !should_sync {
            return;
        }
        if self.owls_resource_state.is_loading() {
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

    fn poll_market_intel(&mut self) {
        self.drain_market_intel_results();
        self.expire_stuck_market_intel_placeholder();
        if !self.should_poll_market_intel() || self.market_intel_resource_state.is_loading() {
            return;
        }
        if self
            .last_market_intel_dispatch_at
            .is_some_and(|last| last.elapsed() < MARKET_INTEL_SYNC_DISPATCH_INTERVAL)
        {
            return;
        }
        self.dispatch_market_intel_sync(MarketIntelSyncReason::Background);
    }

    fn should_poll_market_intel(&self) -> bool {
        self.active_panel == Panel::Trading && self.trading_section == TradingSection::Intel
    }

    fn poll_matchbook_account(&mut self) {
        self.drain_matchbook_sync_results();
        self.expire_stuck_matchbook_sync_placeholder();
        let should_sync = self.active_panel == Panel::Trading
            && (self.trading_section == TradingSection::Stats
                || (self.trading_section == TradingSection::Positions
                    && self.live_view_overlay_visible)
                || self
                    .trading_action_overlay
                    .as_ref()
                    .is_some_and(|overlay| overlay.seed.venue == VenueId::Matchbook));
        if !should_sync || self.matchbook_resource_state.is_loading() {
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
                        MouseTargetKind::Workspace(index) => {
                            self.switch_workspace_with_status(index)
                        }
                        MouseTargetKind::Pane(pane) => {
                            if self.wm.active_pane == Some(pane) {
                                let emphasized = self.wm.toggle_pane_emphasis(pane);
                                self.apply_pane_context(pane);
                                self.status_message = if emphasized {
                                    format!("Expanded {}.", pane.title())
                                } else {
                                    format!("Reset {} sizing.", pane.title())
                                };
                            } else {
                                self.wm.focus_pane(pane);
                                self.apply_pane_context(pane);
                                self.status_message = format!("Focused {}.", pane.title());
                            }
                            self.status_scroll = 0;
                        }
                        MouseTargetKind::PaneMinimize(pane) => {
                            self.minimize_pane_with_status(pane);
                        }
                        MouseTargetKind::PaneToggleMaximize(pane) => {
                            self.toggle_pane_maximize_with_status(pane);
                        }
                        MouseTargetKind::TradingSection(section) => {
                            self.set_trading_section(section)
                        }
                        MouseTargetKind::IntelView(view) => {
                            self.intel_view = view;
                            self.clamp_selected_intel_row();
                        }
                        MouseTargetKind::MatcherView(view) => self.matcher_view = view,
                        MouseTargetKind::CalculatorTool(tool) => self.calculator_tool = tool,
                        MouseTargetKind::MinimizedPane(pane) => {
                            if self.wm.restore_minimized_pane(pane) {
                                self.apply_pane_context(pane);
                                self.status_message = format!("Restored {} pane.", pane.title());
                                self.status_scroll = 0;
                            }
                        }
                        MouseTargetKind::MinimizeActivePane => {
                            self.minimize_active_pane();
                        }
                        MouseTargetKind::ToggleMaximize => {
                            self.toggle_maximize_with_status();
                        }
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
        let summary = context.to_string();
        let message = format!("{context}: {detail}");
        self.status_message = message.clone();
        self.status_scroll = 0;
        self.last_problem_console_message = Some(message.clone());
        self.snapshot.status_line = message.clone();
        self.snapshot.worker.status = crate::domain::WorkerStatus::Error;
        self.snapshot.worker.detail = truncate_console_message(&message, 56);
        self.record_event(message.clone());

        if let Some(venue_id) = selected_venue {
            if let Some(venue) = self
                .snapshot
                .venues
                .iter_mut()
                .find(|venue| venue.id == venue_id)
            {
                venue.status = crate::domain::VenueStatus::Error;
                venue.detail = truncate_console_message(&summary, 40);
            }
        }

        if self.alerts_config.provider_errors {
            self.emit_alert(
                "provider_errors",
                NotificationLevel::Critical,
                "Provider error",
                format!("{context}: {detail}"),
            );
        }
    }

    fn replace_snapshot(&mut self, snapshot: ExchangePanelSnapshot) {
        let previous_snapshot = self.snapshot.clone();
        let had_successful_snapshot = self.last_successful_snapshot_at.is_some();
        let previous_reconnect_count = self.worker_reconnect_count();
        let previous_decision_counts = decision_status_counts(&previous_snapshot);
        let previous_actionable_decisions = actionable_decision_count(&previous_snapshot);
        let previous_tracked_bets = previous_snapshot.tracked_bets.len();
        let previous_exit_recommendations =
            actionable_exit_recommendation_count(&previous_snapshot);
        let previous_stale = previous_snapshot
            .runtime
            .as_ref()
            .map(|runtime| runtime.stale)
            .unwrap_or(false);
        let normalized_snapshot = normalize_snapshot(
            preserve_cached_snapshot_state(&previous_snapshot, snapshot),
            &self.recorder_config.disabled_venues,
            &self.manual_positions,
        );
        self.provider_resource_state
            .finish_ok(normalized_snapshot.clone());
        self.snapshot = normalized_snapshot;
        self.maybe_align_owls_sport_with_snapshot();
        self.refresh_snapshot_enrichment();
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
        let current_actionable_decisions = actionable_decision_count(&self.snapshot);
        if self.alerts_config.decision_queue
            && current_actionable_decisions > previous_actionable_decisions
        {
            self.emit_alert(
                "decision_queue",
                NotificationLevel::Info,
                "Decision queue moved",
                format!(
                    "{} actionable decisions ready.",
                    current_actionable_decisions
                ),
            );
        }
        let current_tracked_bets = self.snapshot.tracked_bets.len();
        if self.alerts_config.tracked_bets && current_tracked_bets > previous_tracked_bets {
            self.emit_alert(
                "tracked_bets",
                NotificationLevel::Info,
                "Tracked bets increased",
                format!("{current_tracked_bets} tracked bets now loaded."),
            );
        }
        if self.alerts_config.bet_settled {
            if let Some(detail) =
                first_settled_transition_detail(&previous_snapshot, &self.snapshot)
            {
                self.emit_alert(
                    "bet_settled",
                    NotificationLevel::Info,
                    "Bet settled",
                    detail,
                );
            }
        }
        let current_exit_recommendations = actionable_exit_recommendation_count(&self.snapshot);
        if self.alerts_config.exit_recommendations
            && current_exit_recommendations > previous_exit_recommendations
        {
            let detail = self
                .snapshot
                .exit_recommendations
                .iter()
                .find(|recommendation| recommendation.action != "hold")
                .map(|recommendation| {
                    format!(
                        "{} {} {}",
                        recommendation.bet_id, recommendation.action, recommendation.reason
                    )
                })
                .unwrap_or_else(|| {
                    format!("{current_exit_recommendations} actionable exits available.")
                });
            self.emit_alert(
                "exit_recommendations",
                NotificationLevel::Warning,
                "Exit recommendation",
                detail,
            );
        }
        let current_stale = self
            .snapshot
            .runtime
            .as_ref()
            .map(|runtime| runtime.stale)
            .unwrap_or(false);
        if self.alerts_config.snapshot_stale && current_stale && !previous_stale {
            self.emit_alert(
                "snapshot_stale",
                NotificationLevel::Warning,
                "Snapshot turned stale",
                self.snapshot.status_line.clone(),
            );
        }
        if self.recorder_startup_alerts_pending {
            self.recorder_startup_alerts_pending = false;
            self.recorder_startup_alerts_muted_until = None;
            self.record_event("Recorder startup alert mute cleared after first snapshot.");
        }
        if !had_successful_snapshot
            || previous_stale != current_stale
            || self.status_message.is_empty()
        {
            self.status_message = self.snapshot.status_line.clone();
            self.status_scroll = 0;
        }
        self.clamp_selected_exchange_row();
        if self.trading_section == TradingSection::Accounts {
            self.seed_selected_exchange_row_from_snapshot();
        }
        self.clamp_selected_open_position_row();
        self.clamp_selected_intel_row();
        self.clamp_selected_oddsmatcher_row();
        self.clamp_selected_horse_matcher_row();
        if active_position_row_count(&self.snapshot) == 0 {
            self.live_view_overlay_visible = false;
        }
        self.trading_action_overlay = None;
    }

    fn maybe_align_owls_sport_with_snapshot(&mut self) {
        let Some(inferred_sport) = inferred_owls_sport(&self.snapshot) else {
            return;
        };
        if inferred_sport == self.owls_dashboard.sport || self.owls_dashboard.sport != "nba" {
            return;
        }
        self.owls_dashboard = owls::dashboard_for_sport(inferred_sport);
        self.owls_resource_state
            .finish_ok(self.owls_dashboard.clone());
        self.align_owls_selection_for_section();
        self.request_market_intel_sync(MarketIntelSyncReason::Background);
        if self.is_owls_context()
            || (self.trading_section == TradingSection::Positions && self.live_view_overlay_visible)
        {
            self.request_owls_sync(OwlsSyncReason::Manual);
        }
        self.record_event(format!(
            "Owls sport auto-set to {inferred_sport} from snapshot context."
        ));
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
        self.last_event_at_label = Some(Local::now().format("%H:%M:%S").to_string());
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

    fn expire_stuck_provider_request_placeholder(&mut self) {
        if !self.provider_resource_state.expire_if_overdue(
            RESOURCE_WATCHDOG_TIMEOUT,
            "provider request watchdog expired",
        ) {
            return;
        }

        self.provider_in_flight = false;
        self.last_recorder_refresh_at = Some(Instant::now());
        #[cfg(debug_assertions)]
        {
            self.provider_in_flight_started_at_for_test = None;
        }
        self.restart_provider_worker(self.current_provider_for_watchdog());
        self.record_event("Provider refresh timed out; marking state stale.");
    }

    fn expire_stuck_owls_sync_placeholder(&mut self) {
        if !self
            .owls_resource_state
            .expire_if_overdue(RESOURCE_WATCHDOG_TIMEOUT, "owls sync watchdog expired")
        {
            return;
        }

        self.owls_sync_in_flight = false;
        self.last_owls_sync_dispatch_at = Some(Instant::now());
        self.restart_owls_sync_worker();
        if let Some(last_good) = self.owls_resource_state.last_good().cloned() {
            self.owls_dashboard = last_good;
            self.refresh_snapshot_enrichment();
        }
        self.record_event("Owls sync timed out; marking state stale.");
    }

    fn expire_stuck_matchbook_sync_placeholder(&mut self) {
        if !self
            .matchbook_resource_state
            .expire_if_overdue(RESOURCE_WATCHDOG_TIMEOUT, "matchbook sync watchdog expired")
        {
            return;
        }

        self.matchbook_sync_in_flight = false;
        self.last_matchbook_sync_dispatch_at = Some(Instant::now());
        self.restart_matchbook_sync_worker();
        self.record_event("Matchbook sync timed out; marking state stale.");
    }

    fn expire_stuck_market_intel_placeholder(&mut self) {
        if !self.market_intel_resource_state.expire_if_overdue(
            RESOURCE_WATCHDOG_TIMEOUT,
            "market intel sync watchdog expired",
        ) {
            return;
        }

        self.market_intel_in_flight = false;
        self.last_market_intel_dispatch_at = Some(Instant::now());
        self.restart_market_intel_worker();
        self.record_event("Market intel sync timed out; marking state stale.");
    }

    fn load_alerts_from_disk(&mut self, path: PathBuf) -> Result<()> {
        let (config, note) = load_alert_config_or_default(&path)?;
        self.alerts_config = config;
        self.alerts_config_path = path;
        self.alerts_config_note = note;
        Ok(())
    }

    fn begin_alerts_edit(&mut self) {
        let field = self.alerts_editor.selected_field();
        self.alerts_editor.buffer = field.display_value(&self.alerts_config);
        self.alerts_editor.editing = true;
        self.alerts_editor.replace_on_input = true;
        self.status_message = format!("Editing alerts {}.", field.label());
        self.status_scroll = 0;
    }

    fn apply_alerts_edit(&mut self) -> Result<()> {
        let field = self.alerts_editor.selected_field();
        let value = self.alerts_editor.buffer.clone();
        field.apply_value(&mut self.alerts_config, &value)?;
        self.alerts_editor.editing = false;
        self.alerts_editor.buffer.clear();
        self.alerts_editor.replace_on_input = false;
        self.persist_alerts_config()?;
        self.status_message = format!("Updated alerts {}.", field.label());
        self.status_scroll = 0;
        self.record_event(format!("Updated alerts {}.", field.label()));
        Ok(())
    }

    fn cancel_alerts_edit(&mut self) {
        self.alerts_editor.editing = false;
        self.alerts_editor.buffer.clear();
        self.alerts_editor.replace_on_input = false;
        self.status_message = String::from("Cancelled alerts edit.");
    }

    fn alerts_push_char(&mut self, character: char) {
        if self.alerts_editor.replace_on_input {
            self.alerts_editor.buffer.clear();
            self.alerts_editor.replace_on_input = false;
        }
        self.alerts_editor.buffer.push(character);
    }

    fn alerts_backspace(&mut self) {
        if self.alerts_editor.replace_on_input {
            self.alerts_editor.buffer.clear();
            self.alerts_editor.replace_on_input = false;
            return;
        }
        self.alerts_editor.buffer.pop();
    }

    fn cycle_alerts_suggestion(&mut self, forward: bool) -> Result<()> {
        let field = self.alerts_editor.selected_field();
        let suggestions = field.suggestions();
        if suggestions.is_empty() {
            return Ok(());
        }
        let current_value = field.display_value(&self.alerts_config);
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
        field.apply_value(&mut self.alerts_config, suggestions[next_index])?;
        self.persist_alerts_config()?;
        self.status_message = format!("Applied alerts suggestion for {}.", field.label());
        self.status_scroll = 0;
        self.record_event(format!("Applied alerts suggestion for {}.", field.label()));
        Ok(())
    }

    fn reload_alerts_config(&mut self) -> Result<()> {
        let (config, note) = load_alert_config_or_default(&self.alerts_config_path)?;
        self.alerts_config = config;
        self.alerts_config_note = note;
        self.alerts_editor = AlertEditorState::default();
        self.status_message = String::from("Reloaded alerts config from disk.");
        self.status_scroll = 0;
        Ok(())
    }

    fn reset_alerts_config(&mut self) -> Result<()> {
        self.alerts_config = AlertConfig::default();
        self.alerts_editor = AlertEditorState::default();
        self.persist_alerts_config()?;
        self.status_message = String::from("Reset alerts config to defaults.");
        self.status_scroll = 0;
        Ok(())
    }

    fn persist_alerts_config(&mut self) -> Result<()> {
        self.alerts_config_note = save_alert_config(&self.alerts_config_path, &self.alerts_config)?;
        Ok(())
    }

    fn mark_notifications_read(&mut self) {
        for entry in &mut self.notifications {
            entry.unread = false;
        }
    }

    fn emit_alert(
        &mut self,
        rule_key: &'static str,
        level: NotificationLevel,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) {
        if !self.alerts_config.enabled {
            return;
        }
        if self.should_suppress_alert_for_recorder_startup(rule_key) {
            return;
        }
        if self.alerts_config.cooldown_seconds > 0
            && self.alert_last_sent_at.get(rule_key).is_some_and(|last| {
                last.elapsed() < Duration::from_secs(self.alerts_config.cooldown_seconds)
            })
        {
            return;
        }
        self.alert_last_sent_at.insert(rule_key, Instant::now());

        let title = title.into();
        let detail = detail.into();
        let entry = NotificationEntry {
            created_at: current_time_label(),
            rule_key: String::from(rule_key),
            level,
            title: title.clone(),
            detail: detail.clone(),
            unread: true,
        };
        if self.notifications.len() == MAX_NOTIFICATIONS {
            self.notifications.pop_front();
        }
        self.notifications.push_back(entry.clone());
        self.record_event(format!("Alert {} {}: {}", level.label(), title, detail));
        if !self.notifications_overlay_visible {
            self.status_message = format!("{title}: {detail}");
            self.status_scroll = 0;
        }
        if self.alerts_config.desktop_notifications || self.alerts_config.sound_effects {
            dispatch_notification_delivery(entry, &self.alerts_config);
        }
    }

    fn should_suppress_alert_for_recorder_startup(&mut self, rule_key: &str) -> bool {
        let Some(muted_until) = self.recorder_startup_alerts_muted_until else {
            return false;
        };
        if Instant::now() >= muted_until {
            self.recorder_startup_alerts_muted_until = None;
            self.recorder_startup_alerts_pending = false;
            return false;
        }
        if !self.recorder_startup_alerts_pending {
            return false;
        }
        matches!(
            rule_key,
            "provider_errors"
                | "matchbook_failures"
                | "snapshot_stale"
                | "decision_queue"
                | "tracked_bets"
                | "exit_recommendations"
                | "opportunity_detected"
                | "watched_movement"
                | "owls_errors"
        )
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
        overlay.clear_backend_result();
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
        if overlay.seed.source == TradingActionSource::MarketIntel {
            return self.execute_market_intel_action_via_backend(&overlay, stake);
        }
        if matches!(overlay.seed.venue, VenueId::Matchbook | VenueId::Betfair) {
            return self.execute_overlay_action_via_backend(&overlay, stake);
        }
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
            request: ProviderRequest::ExecuteTradingAction {
                intent: Box::new(intent),
            },
            failure_context: String::from("Trading action failed"),
            event_message: Some(String::from("Trading action completed.")),
        });
        Ok(())
    }

    fn execute_market_intel_action_via_backend(
        &mut self,
        overlay: &TradingActionOverlayState,
        stake: f64,
    ) -> Result<()> {
        let match_id = overlay.seed.source_ref.trim();
        if match_id.is_empty() {
            return Err(color_eyre::eyre::eyre!(
                "The selected Intel row is missing its backend match identifier."
            ));
        }

        if overlay.mode == TradingActionMode::Review {
            let response = execution_backend::review_execution(match_id, stake)?;
            if let Some(current) = self.trading_action_overlay.as_mut() {
                current.backend_gateway = Some(BackendExecutionOverlayState {
                    gateway: response.gateway.kind,
                    mode: response.gateway.mode,
                    detail: response.gateway.detail,
                    last_status: Some(response.review.status.clone()),
                    last_detail: Some(response.review.detail.clone()),
                    executable: Some(response.review.executable),
                    accepted: None,
                });
            }
            self.status_message = format!(
                "Execution review {}: {}",
                response.review.status, response.review.detail
            );
            self.status_scroll = 0;
            self.record_event(format!("Execution review {}.", response.review.status));
            return Ok(());
        }

        let response = execution_backend::submit_execution(match_id, stake)?;
        if let Some(current) = self.trading_action_overlay.as_mut() {
            current.backend_gateway = Some(BackendExecutionOverlayState {
                gateway: response.gateway.kind,
                mode: response.gateway.mode,
                detail: response.gateway.detail,
                last_status: Some(response.result.status.clone()),
                last_detail: Some(response.result.detail.clone()),
                executable: None,
                accepted: Some(response.result.accepted),
            });
        }
        self.status_message = format!(
            "Execution submit {}: {}",
            response.result.status, response.result.detail
        );
        self.status_scroll = 0;
        self.record_event(format!("Execution submit {}.", response.result.status));
        Ok(())
    }

    fn execute_overlay_action_via_backend(
        &mut self,
        overlay: &TradingActionOverlayState,
        stake: f64,
    ) -> Result<()> {
        let price = overlay.seed.price_for_side(overlay.side).ok_or_else(|| {
            color_eyre::eyre::eyre!("No executable price is available for this side.")
        })?;
        let request = execution_backend::AdhocExecutionRequest {
            venue: overlay.seed.venue.as_str().to_string(),
            side: match overlay.side {
                TradingActionSide::Buy => String::from("back"),
                TradingActionSide::Sell => String::from("lay"),
            },
            event_name: overlay.seed.event_name.clone(),
            market_name: overlay.seed.market_name.clone(),
            selection_name: overlay.seed.selection_name.clone(),
            stake,
            price,
            event_url: overlay.seed.event_url.clone(),
            deep_link_url: overlay.seed.deep_link_url.clone(),
            event_ref: overlay.seed.betslip_event_id.clone(),
            market_ref: overlay.seed.betslip_market_id.clone(),
            selection_ref: overlay.seed.betslip_selection_id.clone(),
        };

        if overlay.mode == TradingActionMode::Review {
            let response = execution_backend::review_adhoc_execution(&request)?;
            if let Some(current) = self.trading_action_overlay.as_mut() {
                current.backend_gateway = Some(BackendExecutionOverlayState {
                    gateway: response.gateway.kind,
                    mode: response.gateway.mode,
                    detail: response.gateway.detail,
                    last_status: Some(response.review.status.clone()),
                    last_detail: Some(response.review.detail.clone()),
                    executable: Some(response.review.executable),
                    accepted: None,
                });
            }
            self.status_message = format!(
                "Execution review {}: {}",
                response.review.status, response.review.detail
            );
            self.status_scroll = 0;
            self.record_event(format!("Execution review {}.", response.review.status));
            return Ok(());
        }

        let response = execution_backend::submit_adhoc_execution(&request)?;
        if let Some(current) = self.trading_action_overlay.as_mut() {
            current.backend_gateway = Some(BackendExecutionOverlayState {
                gateway: response.gateway.kind,
                mode: response.gateway.mode,
                detail: response.gateway.detail,
                last_status: Some(response.result.status.clone()),
                last_detail: Some(response.result.detail.clone()),
                executable: None,
                accepted: Some(response.result.accepted),
            });
        }
        self.status_message = format!(
            "Execution submit {}: {}",
            response.result.status, response.result.detail
        );
        self.status_scroll = 0;
        self.record_event(format!("Execution submit {}.", response.result.status));
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

    fn load_calculator_from_selected_intel(&mut self) {
        let Some(row) = self.selected_intel_row() else {
            self.status_message = String::from("No Intel row is selected.");
            return;
        };

        let Some(lay_odds) = row.lay_odds else {
            self.status_message = String::from(
                "The selected Intel row has no lay quote, so the calculator was not loaded.",
            );
            return;
        };

        self.calculator.input.back_odds = row.back_odds;
        self.calculator.input.lay_odds = lay_odds;
        self.calculator.source = Some(CalculatorSourceContext {
            event_name: row.event.clone(),
            selection_name: row.selection.clone(),
            competition_name: row.competition.clone(),
            rating: row.edge_pct.unwrap_or(row.arb_pct.unwrap_or(0.0)),
            bookmaker_name: row.bookmaker.clone(),
            exchange_name: row.exchange.clone(),
        });
        self.set_trading_section(TradingSection::Calculator);
        self.status_message = format!(
            "Loaded calculator from Intel {}: {} @ {:.2} / {:.2}.",
            row.source.label(),
            row.selection,
            row.back_odds,
            lay_odds
        );
    }

    fn open_trading_action_overlay_from_intel(&mut self) {
        let Some(row) = self.selected_intel_row() else {
            self.status_message = String::from("No Intel row is selected.");
            return;
        };
        if !row.can_open_action() {
            self.status_message =
                String::from("The selected Intel row does not expose an executable quote.");
            return;
        }

        let (seed, backend_gateway) =
            match self.build_market_intel_overlay_seed(&row).or_else(|error| {
                warn!("market-intel execution plan unavailable, falling back to row seed: {error}");
                self.build_market_intel_overlay_seed_legacy(&row)
                    .map(|seed| (seed, None))
            }) {
                Ok(result) => result,
                Err(error) => {
                    self.status_message = error.to_string();
                    self.status_scroll = 0;
                    return;
                }
            };

        self.open_trading_action_overlay(seed);
        if let (Some(gateway), Some(overlay)) =
            (backend_gateway, self.trading_action_overlay.as_mut())
        {
            overlay.backend_gateway = Some(gateway);
        }
    }

    fn build_market_intel_overlay_seed(
        &self,
        row: &IntelRow,
    ) -> Result<(TradingActionSeed, Option<BackendExecutionOverlayState>)> {
        let plan = execution_backend::fetch_execution_plan(&row.id)?;
        let action = plan.plan.primary.clone();
        let venue = exchange_venue_from_bookmaker("", &action.venue).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "The backend execution venue '{}' is not mapped in operator-console.",
                action.venue
            )
        })?;
        let primary_mapping = plan
            .opportunity
            .venue_mappings
            .iter()
            .find(|mapping| mapping.venue.eq_ignore_ascii_case(&action.venue))
            .cloned();
        let (buy_price, sell_price, default_side) =
            backend_action_prices(&action.side, action.price);
        let mut seed = TradingActionSeed {
            source: TradingActionSource::MarketIntel,
            venue,
            source_ref: row.id.clone(),
            event_name: plan.opportunity.event_name.clone(),
            market_name: plan.opportunity.market_name.clone(),
            selection_name: plan.opportunity.selection_name.clone(),
            event_url: primary_mapping
                .as_ref()
                .map(|mapping| mapping.event_url.clone())
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some(row.route.clone()).filter(|value| !value.trim().is_empty())),
            deep_link_url: Some(action.deep_link_url.clone())
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    primary_mapping
                        .as_ref()
                        .map(|mapping| mapping.deep_link_url.clone())
                        .filter(|value| !value.trim().is_empty())
                })
                .or_else(|| {
                    Some(row.deep_link_url.clone()).filter(|value| !value.trim().is_empty())
                }),
            betslip_event_id: primary_mapping
                .as_ref()
                .map(|mapping| mapping.event_ref.clone()),
            betslip_market_id: primary_mapping
                .as_ref()
                .map(|mapping| mapping.market_ref.clone()),
            betslip_selection_id: primary_mapping
                .as_ref()
                .map(|mapping| mapping.selection_ref.clone()),
            buy_price,
            sell_price,
            default_side,
            default_stake: action
                .stake_hint
                .or(plan.opportunity.stake_hint)
                .or(row.liquidity.map(|liquidity| liquidity.min(25.0)))
                .or(Some(10.0)),
            source_context: TradingActionSourceContext {
                market_status: row.status.clone(),
                ..TradingActionSourceContext::default()
            },
            notes: vec![
                format!("intel_source:{}", row.source.key()),
                format!(
                    "canonical_selection:{}",
                    plan.opportunity.canonical.selection.id
                ),
                format!("gateway:{}:{}", plan.gateway.kind, plan.gateway.mode),
                format!("bookmaker:{}", row.bookmaker),
                plan.gateway.detail.clone(),
                row.note.clone(),
            ],
        };
        seed.notes.retain(|note| !note.trim().is_empty());

        Ok((
            seed,
            Some(BackendExecutionOverlayState {
                gateway: plan.gateway.kind,
                mode: plan.gateway.mode,
                detail: plan.gateway.detail,
                last_status: None,
                last_detail: None,
                executable: None,
                accepted: None,
            }),
        ))
    }

    fn build_market_intel_overlay_seed_legacy(&self, row: &IntelRow) -> Result<TradingActionSeed> {
        let venue = exchange_venue_from_bookmaker("", &row.exchange).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "The selected Intel row does not map to a supported exchange venue."
            )
        })?;
        Ok(TradingActionSeed {
            source: TradingActionSource::MarketIntel,
            venue,
            source_ref: row.id.clone(),
            event_name: row.event.clone(),
            market_name: row.market.clone(),
            selection_name: row.selection.clone(),
            event_url: Some(row.route.clone()).filter(|value| !value.trim().is_empty()),
            deep_link_url: Some(row.deep_link_url.clone()).filter(|value| !value.trim().is_empty()),
            betslip_event_id: None,
            betslip_market_id: None,
            betslip_selection_id: None,
            buy_price: None,
            sell_price: row.lay_odds,
            default_side: TradingActionSide::Sell,
            default_stake: row
                .liquidity
                .map(|liquidity| liquidity.min(25.0))
                .or(Some(10.0)),
            source_context: TradingActionSourceContext {
                market_status: row.status.clone(),
                ..TradingActionSourceContext::default()
            },
            notes: vec![
                format!("intel_source:{}", row.source.key()),
                String::from("gateway:fallback:legacy"),
                format!("bookmaker:{}", row.bookmaker),
                row.note.clone(),
            ],
        })
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
        self.set_trading_section(TradingSection::Calculator);
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
                query: Box::new(self.horse_matcher_query.clone()),
            },
            failure_context: String::from("Horse Matcher refresh failed"),
            event_message: Some(String::from("Horse Matcher refresh completed.")),
        });
        Ok(())
    }

    fn toggle_observability_panel(&mut self) {
        match self.active_panel {
            Panel::Trading => {
                if let Some(index) = self.wm.workspace_index_for_pane(PaneId::Observability) {
                    self.wm.switch_workspace(index);
                    self.wm.focus_pane(PaneId::Observability);
                }
                self.apply_pane_context(PaneId::Observability);
            }
            Panel::Observability => {
                self.focus_trading_section(self.trading_section);
            }
        }
    }

    fn sync_problem_console_from_status(&mut self) {
        let status = self.status_message.trim();
        if status.is_empty() || !looks_like_problem_message(status) {
            return;
        }

        if self.last_problem_console_message.as_deref() == Some(status) {
            return;
        }

        if self.latest_problem_notification().is_some_and(|entry| {
            entry.title == status
                || entry.detail == status
                || entry.detail.starts_with(status)
                || status.starts_with(&entry.title)
        }) {
            self.last_problem_console_message = Some(status.to_string());
            return;
        }

        let entry = NotificationEntry {
            created_at: current_time_label(),
            rule_key: String::from("error_console_status"),
            level: infer_problem_level(status),
            title: truncate_console_message(status, 32),
            detail: status.to_string(),
            unread: true,
        };
        if self.notifications.len() == MAX_NOTIFICATIONS {
            self.notifications.pop_front();
        }
        self.notifications.push_back(entry);
        self.last_problem_console_message = Some(status.to_string());
    }
}

fn truncate_console_message(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn looks_like_problem_message(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    normalized.contains("failed")
        || normalized.contains("error")
        || normalized.contains("unavailable")
        || normalized.contains("timed out")
        || normalized.contains("expired")
}

fn infer_problem_level(value: &str) -> NotificationLevel {
    let normalized = value.to_ascii_lowercase();
    if normalized.contains("unavailable")
        || normalized.contains("timed out")
        || normalized.contains("expired")
    {
        NotificationLevel::Critical
    } else {
        NotificationLevel::Warning
    }
}

fn pane_for_trading_section(section: TradingSection) -> PaneId {
    match section {
        TradingSection::Positions => PaneId::Positions,
        TradingSection::Accounts => PaneId::Accounts,
        TradingSection::Markets => PaneId::Markets,
        TradingSection::Live => PaneId::Live,
        TradingSection::Props => PaneId::Props,
        TradingSection::Intel => PaneId::Intel,
        TradingSection::Matcher => PaneId::Matcher,
        TradingSection::Stats => PaneId::Stats,
        TradingSection::Alerts => PaneId::Alerts,
        TradingSection::Calculator => PaneId::Calculator,
        TradingSection::Recorder => PaneId::Recorder,
    }
}

fn trading_section_for_pane(pane: PaneId) -> TradingSection {
    match pane {
        PaneId::Positions => TradingSection::Positions,
        PaneId::Accounts => TradingSection::Accounts,
        PaneId::History => TradingSection::Positions,
        PaneId::Markets => TradingSection::Markets,
        PaneId::Live => TradingSection::Live,
        PaneId::Props => TradingSection::Props,
        PaneId::Chart => TradingSection::Markets,
        PaneId::Intel => TradingSection::Intel,
        PaneId::Matcher => TradingSection::Matcher,
        PaneId::Stats => TradingSection::Stats,
        PaneId::Alerts => TradingSection::Alerts,
        PaneId::Calculator => TradingSection::Calculator,
        PaneId::Recorder => TradingSection::Recorder,
        PaneId::Observability => TradingSection::Positions,
    }
}

#[derive(Clone)]
struct SnapshotMarketTarget {
    event: String,
    market: String,
    selection: String,
    event_url: String,
}

pub(crate) fn populate_snapshot_enrichment(
    snapshot: &mut ExchangePanelSnapshot,
    owls_dashboard: &OwlsDashboard,
    matchbook_account_state: Option<&MatchbookAccountState>,
    market_intel_dashboard: Option<&MarketIntelDashboard>,
) {
    snapshot.external_quotes = build_external_quote_rows(
        snapshot,
        owls_dashboard,
        matchbook_account_state,
        market_intel_dashboard,
    );
    snapshot.external_live_events = build_external_live_event_rows(snapshot, owls_dashboard);
    apply_external_live_context(snapshot);
}

fn build_external_quote_rows(
    snapshot: &ExchangePanelSnapshot,
    owls_dashboard: &OwlsDashboard,
    matchbook_account_state: Option<&MatchbookAccountState>,
    market_intel_dashboard: Option<&MarketIntelDashboard>,
) -> Vec<ExternalQuoteRow> {
    let mut rows = Vec::new();
    let mut seen = HashSet::new();

    for open_position in &snapshot.open_positions {
        if let Some(price) = open_position.current_back_odds {
            push_external_quote_row(
                &mut rows,
                &mut seen,
                ExternalQuoteRow {
                    provider: String::from("snapshot"),
                    venue: String::from("smarkets"),
                    event: open_position.event.clone(),
                    market: open_position.market.clone(),
                    selection: open_position.contract.clone(),
                    side: String::from("back"),
                    event_url: open_position.event_url.clone(),
                    deep_link_url: String::new(),
                    event_id: String::new(),
                    market_id: String::new(),
                    selection_id: String::new(),
                    price: Some(price),
                    liquidity: None,
                    is_sharp: false,
                    updated_at: snapshot
                        .runtime
                        .as_ref()
                        .map(|runtime| runtime.updated_at.clone())
                        .unwrap_or_default(),
                    status: if open_position.can_trade_out {
                        String::from("live")
                    } else {
                        String::from("snapshot")
                    },
                },
            );
        }
    }

    let targets = snapshot_market_targets(snapshot);
    for target in &targets {
        for quote in owls::matching_market_quotes(
            owls_dashboard,
            &target.event,
            &target.market,
            &target.selection,
        ) {
            let book = quote.book.trim().to_string();
            push_external_quote_row(
                &mut rows,
                &mut seen,
                ExternalQuoteRow {
                    provider: String::from("owls"),
                    venue: if book.is_empty() {
                        String::from("unknown")
                    } else {
                        book.clone()
                    },
                    event: target.event.clone(),
                    market: target.market.clone(),
                    selection: target.selection.clone(),
                    side: String::from("back"),
                    event_url: target.event_url.clone(),
                    deep_link_url: quote.event_link.clone(),
                    event_id: String::new(),
                    market_id: String::new(),
                    selection_id: String::new(),
                    price: quote.decimal_price,
                    liquidity: quote.limit_amount,
                    is_sharp: normalize_key(&book) == "pinnacle",
                    updated_at: owls_dashboard.refreshed_at.clone(),
                    status: if quote.suspended {
                        String::from("suspended")
                    } else {
                        String::from("ready")
                    },
                },
            );
        }
    }

    if let Some(state) = matchbook_account_state {
        for target in &targets {
            for offer in state
                .current_offers
                .iter()
                .filter(|offer| matchbook_offer_matches_target(offer, target))
            {
                push_external_quote_row(
                    &mut rows,
                    &mut seen,
                    ExternalQuoteRow {
                        provider: String::from("matchbook_api"),
                        venue: String::from("matchbook"),
                        event: target.event.clone(),
                        market: target.market.clone(),
                        selection: target.selection.clone(),
                        side: offer.side.clone(),
                        event_url: target.event_url.clone(),
                        deep_link_url: String::new(),
                        event_id: offer.event_id.clone(),
                        market_id: offer.market_id.clone(),
                        selection_id: offer.runner_id.clone(),
                        price: offer.odds,
                        liquidity: offer.remaining_stake.or(offer.stake),
                        is_sharp: false,
                        updated_at: String::new(),
                        status: offer.status.clone(),
                    },
                );
            }
            for bet in state
                .current_bets
                .iter()
                .filter(|bet| matchbook_bet_matches_target(bet, target))
            {
                push_external_quote_row(
                    &mut rows,
                    &mut seen,
                    ExternalQuoteRow {
                        provider: String::from("matchbook_api"),
                        venue: String::from("matchbook"),
                        event: target.event.clone(),
                        market: target.market.clone(),
                        selection: target.selection.clone(),
                        side: bet.side.clone(),
                        event_url: target.event_url.clone(),
                        deep_link_url: String::new(),
                        event_id: bet.event_id.clone(),
                        market_id: bet.market_id.clone(),
                        selection_id: bet.runner_id.clone(),
                        price: bet.odds,
                        liquidity: bet.stake,
                        is_sharp: false,
                        updated_at: String::new(),
                        status: bet.status.clone(),
                    },
                );
            }
            for position in state
                .positions
                .iter()
                .filter(|position| matchbook_position_matches_target(position, target))
            {
                push_external_quote_row(
                    &mut rows,
                    &mut seen,
                    ExternalQuoteRow {
                        provider: String::from("matchbook_api"),
                        venue: String::from("matchbook"),
                        event: target.event.clone(),
                        market: target.market.clone(),
                        selection: target.selection.clone(),
                        side: String::new(),
                        event_url: target.event_url.clone(),
                        deep_link_url: String::new(),
                        event_id: position.event_id.clone(),
                        market_id: position.market_id.clone(),
                        selection_id: position.runner_id.clone(),
                        price: None,
                        liquidity: position.exposure.map(f64::abs),
                        is_sharp: false,
                        updated_at: String::new(),
                        status: String::from("position"),
                    },
                );
            }
        }
    }

    if let Some(dashboard) = market_intel_dashboard {
        for quote in market_intel::project_external_quote_rows(snapshot, dashboard) {
            push_external_quote_row(&mut rows, &mut seen, quote);
        }
    }

    rows
}

fn build_external_live_event_rows(
    snapshot: &ExchangePanelSnapshot,
    owls_dashboard: &OwlsDashboard,
) -> Vec<ExternalLiveEventRow> {
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for target in snapshot_market_targets(snapshot) {
        let Some(live_event) = owls::find_live_score(owls_dashboard, &target.event).or_else(|| {
            owls_dashboard
                .endpoints
                .iter()
                .flat_map(|endpoint| endpoint.live_scores.iter())
                .find(|score| event_matches(&score.name, &target.event))
                .cloned()
        }) else {
            continue;
        };
        let key = normalize_key(&target.event);
        if !seen.insert(key) {
            continue;
        }
        rows.push(ExternalLiveEventRow {
            provider: String::from("owls"),
            sport: live_event.sport.clone(),
            event: target.event,
            event_id: live_event.event_id.clone(),
            source_match_id: live_event.source_match_id.clone(),
            home_team: live_event.home_team.clone(),
            away_team: live_event.away_team.clone(),
            home_score: live_event.home_score,
            away_score: live_event.away_score,
            status_state: live_event.status_state.clone(),
            status_detail: live_event.status_detail.clone(),
            display_clock: live_event.display_clock.clone(),
            last_updated: live_event.last_updated.clone(),
            stats: live_event
                .stats
                .iter()
                .map(|stat| ExternalLiveStatRow {
                    key: stat.key.clone(),
                    label: stat.label.clone(),
                    home_value: stat.home_value.clone(),
                    away_value: stat.away_value.clone(),
                })
                .collect(),
            incidents: live_event
                .incidents
                .iter()
                .map(|incident| ExternalLiveIncidentRow {
                    minute: incident.minute,
                    incident_type: incident.incident_type.clone(),
                    team_side: incident.team_side.clone(),
                    player_name: incident.player_name.clone(),
                    detail: incident.detail.clone(),
                })
                .collect(),
            player_ratings: live_event
                .player_ratings
                .iter()
                .map(|player| ExternalPlayerRatingRow {
                    player_name: player.player_name.clone(),
                    team_side: player.team_side.clone(),
                    rating: player.rating,
                })
                .collect(),
        });
    }
    rows
}

fn apply_external_live_context(snapshot: &mut ExchangePanelSnapshot) {
    for open_position in &mut snapshot.open_positions {
        let Some(live_event) = snapshot
            .external_live_events
            .iter()
            .find(|live_event| event_matches(&live_event.event, &open_position.event))
        else {
            continue;
        };
        open_position.current_score_home = live_event.home_score;
        open_position.current_score_away = live_event.away_score;
        if let (Some(away), Some(home)) = (live_event.away_score, live_event.home_score) {
            open_position.current_score = format!("{away}-{home}");
        }
        if !live_event.display_clock.trim().is_empty() {
            open_position.live_clock = live_event.display_clock.clone();
        } else if !live_event.status_detail.trim().is_empty() {
            open_position.live_clock = live_event.status_detail.clone();
        }
        if !live_event.status_detail.trim().is_empty() {
            open_position.event_status = live_event.status_detail.clone();
        } else if !live_event.status_state.trim().is_empty() {
            open_position.event_status = live_event.status_state.clone();
        }
        open_position.is_in_play = matches!(
            normalize_key(&live_event.status_state).as_str(),
            "in" | "live" | "inplay"
        );
    }
}

fn snapshot_market_targets(snapshot: &ExchangePanelSnapshot) -> Vec<SnapshotMarketTarget> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for open_position in &snapshot.open_positions {
        push_snapshot_target(
            &mut targets,
            &mut seen,
            &open_position.event,
            &open_position.market,
            &open_position.contract,
            &open_position.event_url,
        );
    }
    for tracked_bet in &snapshot.tracked_bets {
        if tracked_bet_is_closed(tracked_bet) {
            continue;
        }
        push_snapshot_target(
            &mut targets,
            &mut seen,
            &tracked_bet.event,
            &tracked_bet.market,
            &tracked_bet.selection,
            "",
        );
    }
    for sportsbook_bet in &snapshot.other_open_bets {
        push_snapshot_target(
            &mut targets,
            &mut seen,
            &sportsbook_bet.event,
            &sportsbook_bet.market,
            &sportsbook_bet.label,
            "",
        );
    }
    targets
}

fn push_snapshot_target(
    targets: &mut Vec<SnapshotMarketTarget>,
    seen: &mut HashSet<String>,
    event: &str,
    market: &str,
    selection: &str,
    event_url: &str,
) {
    if event.trim().is_empty() || market.trim().is_empty() || selection.trim().is_empty() {
        return;
    }
    let key = format!(
        "{}|{}|{}",
        normalize_key(event),
        normalize_key(market),
        normalize_key(selection)
    );
    if !seen.insert(key) {
        return;
    }
    targets.push(SnapshotMarketTarget {
        event: event.to_string(),
        market: market.to_string(),
        selection: selection.to_string(),
        event_url: event_url.to_string(),
    });
}

fn push_external_quote_row(
    rows: &mut Vec<ExternalQuoteRow>,
    seen: &mut HashSet<String>,
    row: ExternalQuoteRow,
) {
    let key = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        normalize_key(&row.provider),
        normalize_key(&row.venue),
        normalize_key(&row.event),
        normalize_key(&row.market),
        normalize_key(&row.selection),
        normalize_key(&row.side),
        row.price.unwrap_or_default(),
        row.event_id.as_str(),
        row.selection_id.as_str()
    );
    if seen.insert(key) {
        rows.push(row);
    }
}

fn matchbook_offer_matches_target(
    offer: &MatchbookOfferRow,
    target: &SnapshotMarketTarget,
) -> bool {
    event_matches(&offer.event_name, &target.event)
        && market_matches(&offer.market_name, &target.market)
        && selection_matches_with_context(
            &offer.selection_name,
            &offer.event_name,
            &offer.market_name,
            &target.selection,
            &target.event,
            &target.market,
        )
}

fn matchbook_bet_matches_target(bet: &MatchbookBetRow, target: &SnapshotMarketTarget) -> bool {
    event_matches(&bet.event_name, &target.event)
        && market_matches(&bet.market_name, &target.market)
        && selection_matches_with_context(
            &bet.selection_name,
            &bet.event_name,
            &bet.market_name,
            &target.selection,
            &target.event,
            &target.market,
        )
}

fn matchbook_position_matches_target(
    position: &MatchbookPositionRow,
    target: &SnapshotMarketTarget,
) -> bool {
    event_matches(&position.event_name, &target.event)
        && market_matches(&position.market_name, &target.market)
        && selection_matches_with_context(
            &position.selection_name,
            &position.event_name,
            &position.market_name,
            &target.selection,
            &target.event,
            &target.market,
        )
}

fn normalize_snapshot(
    mut snapshot: ExchangePanelSnapshot,
    disabled_venues: &str,
    manual_positions: &[ManualPositionEntry],
) -> ExchangePanelSnapshot {
    snapshot
        .venues
        .retain(|venue| venue_enabled(venue.id, disabled_venues));
    snapshot
        .other_open_bets
        .retain(|bet| !venue_name_disabled(&bet.venue, disabled_venues));
    merge_manual_positions(&mut snapshot, manual_positions, disabled_venues);
    if snapshot
        .selected_venue
        .is_some_and(|venue| !venue_enabled(venue, disabled_venues))
    {
        snapshot.selected_venue = snapshot.venues.first().map(|venue| venue.id);
    }
    snapshot.historical_positions = merge_historical_positions(&snapshot);
    snapshot
}

fn merge_manual_positions(
    snapshot: &mut ExchangePanelSnapshot,
    manual_positions: &[ManualPositionEntry],
    disabled_venues: &str,
) {
    for entry in manual_positions {
        if entry.event.trim().is_empty()
            || entry.market.trim().is_empty()
            || entry.selection.trim().is_empty()
            || entry.venue.trim().is_empty()
            || entry.odds <= 0.0
            || entry.stake <= 0.0
            || venue_name_disabled(&entry.venue, disabled_venues)
        {
            continue;
        }

        let already_present = snapshot.other_open_bets.iter().any(|bet| {
            normalize_manual_key(&bet.event) == normalize_manual_key(&entry.event)
                && normalize_manual_key(&bet.market) == normalize_manual_key(&entry.market)
                && normalize_manual_key(&bet.label) == normalize_manual_key(&entry.selection)
                && normalize_manual_key(&bet.venue) == normalize_manual_key(&entry.venue)
        });
        if already_present {
            continue;
        }

        snapshot.other_open_bets.push(entry.to_other_open_bet());
    }
}

fn normalize_manual_key(value: &str) -> String {
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

fn tracked_bet_key(tracked_bet: &TrackedBetRow) -> String {
    if !tracked_bet.bet_id.trim().is_empty() {
        tracked_bet.bet_id.clone()
    } else {
        format!(
            "{}|{}|{}|{}",
            tracked_bet.event, tracked_bet.market, tracked_bet.selection, tracked_bet.placed_at
        )
    }
}

fn first_settled_transition_detail(
    previous: &ExchangePanelSnapshot,
    current: &ExchangePanelSnapshot,
) -> Option<String> {
    let previous_by_key = previous
        .tracked_bets
        .iter()
        .map(|tracked_bet| (tracked_bet_key(tracked_bet), tracked_bet))
        .collect::<HashMap<_, _>>();

    current.tracked_bets.iter().find_map(|tracked_bet| {
        if !tracked_bet_is_closed(tracked_bet) {
            return None;
        }
        let key = tracked_bet_key(tracked_bet);
        let was_closed = previous_by_key
            .get(&key)
            .map(|previous_tracked_bet| tracked_bet_is_closed(previous_tracked_bet))
            .unwrap_or(false);
        if was_closed {
            return None;
        }
        let pnl = tracked_bet
            .realised_pnl_gbp
            .map(|value| format!(" pnl {value:+.2}"))
            .unwrap_or_default();
        Some(format!(
            "{} {} {}{}",
            tracked_bet
                .platform
                .clone()
                .if_empty_then(|| String::from("bet")),
            tracked_bet.selection,
            tracked_bet.status,
            pnl
        ))
    })
}

fn live_sharp_opportunity_alert_detail(
    snapshot: &ExchangePanelSnapshot,
    previous_dashboard: &OwlsDashboard,
    current_dashboard: &OwlsDashboard,
    threshold_pct: f64,
) -> Option<String> {
    let previous_best =
        best_live_sharp_edge(snapshot, previous_dashboard).map(|candidate| candidate.edge_pct);
    let candidate = best_live_sharp_edge(snapshot, current_dashboard)?;
    if candidate.edge_pct < threshold_pct
        || previous_best.unwrap_or(f64::NEG_INFINITY) >= threshold_pct
    {
        return None;
    }
    Some(format!(
        "{} {} @ {:.2} vs pinnacle {:.2} ({:+.2}%)",
        candidate.book,
        candidate.selection,
        candidate.offered_odds,
        candidate.sharp_odds,
        candidate.edge_pct
    ))
}

fn sharp_watch_movement_alert_detail(
    snapshot: &ExchangePanelSnapshot,
    previous_dashboard: &OwlsDashboard,
    current_dashboard: &OwlsDashboard,
    threshold_pct: f64,
) -> Option<String> {
    let watch = snapshot.watch.as_ref()?;
    watch.watches.iter().find_map(|watch_row| {
        let event = watch_row_event_name(snapshot, watch_row)?;
        let previous = find_owls_sharp_price(
            previous_dashboard,
            &event,
            &watch_row.market,
            &watch_row.contract,
        )?;
        let current = find_owls_sharp_price(
            current_dashboard,
            &event,
            &watch_row.market,
            &watch_row.contract,
        )?;
        if previous <= 1.0 || current <= 1.0 {
            return None;
        }
        let pct_move = ((current - previous) / previous).abs() * 100.0;
        if pct_move < threshold_pct {
            return None;
        }
        let direction = if current > previous { "up" } else { "down" };
        Some(format!(
            "{} {:.2}->{:.2} ({pct_move:.2}% {direction})",
            watch_row.contract, previous, current
        ))
    })
}

#[derive(Debug, Clone)]
struct SharpEdgeCandidate {
    book: String,
    selection: String,
    offered_odds: f64,
    sharp_odds: f64,
    edge_pct: f64,
}

fn best_live_sharp_edge(
    snapshot: &ExchangePanelSnapshot,
    dashboard: &OwlsDashboard,
) -> Option<SharpEdgeCandidate> {
    let other_bets = snapshot.other_open_bets.iter().filter_map(|bet| {
        let sharp_odds = find_owls_sharp_price(dashboard, &bet.event, &bet.market, &bet.label)?;
        if bet.odds <= 1.0 || sharp_odds <= 1.0 {
            return None;
        }
        let edge_pct = ((bet.odds / sharp_odds) - 1.0) * 100.0;
        Some(SharpEdgeCandidate {
            book: bet.venue.clone(),
            selection: bet.label.clone(),
            offered_odds: bet.odds,
            sharp_odds,
            edge_pct,
        })
    });

    let tracked = snapshot
        .tracked_bets
        .iter()
        .filter(|tracked_bet| !tracked_bet_is_closed(tracked_bet))
        .filter_map(|tracked_bet| {
            let offered_odds = tracked_bet.back_price.or_else(|| {
                tracked_bet
                    .legs
                    .iter()
                    .find(|leg| is_back_leg(leg))
                    .map(|leg| leg.odds)
            })?;
            let sharp_odds = find_owls_sharp_price(
                dashboard,
                &tracked_bet.event,
                &tracked_bet.market,
                &tracked_bet.selection,
            )?;
            if offered_odds <= 1.0 || sharp_odds <= 1.0 {
                return None;
            }
            let edge_pct = ((offered_odds / sharp_odds) - 1.0) * 100.0;
            Some(SharpEdgeCandidate {
                book: tracked_bet
                    .platform
                    .clone()
                    .if_empty_then(|| String::from("tracked")),
                selection: tracked_bet.selection.clone(),
                offered_odds,
                sharp_odds,
                edge_pct,
            })
        });

    other_bets.chain(tracked).max_by(|left, right| {
        left.edge_pct
            .partial_cmp(&right.edge_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn watch_row_event_name(
    snapshot: &ExchangePanelSnapshot,
    watch_row: &crate::domain::WatchRow,
) -> Option<String> {
    snapshot
        .open_positions
        .iter()
        .find(|row| {
            text_matches(&row.contract, &watch_row.contract)
                && market_matches(&row.market, &watch_row.market)
        })
        .map(|row| row.event.clone())
        .or_else(|| {
            snapshot
                .tracked_bets
                .iter()
                .find(|tracked_bet| {
                    selection_matches(&tracked_bet.selection, &watch_row.contract)
                        && market_matches(&tracked_bet.market, &watch_row.market)
                })
                .map(|tracked_bet| tracked_bet.event.clone())
        })
        .or_else(|| {
            snapshot
                .other_open_bets
                .iter()
                .find(|bet| {
                    selection_matches(&bet.label, &watch_row.contract)
                        && market_matches(&bet.market, &watch_row.market)
                })
                .map(|bet| bet.event.clone())
        })
}

fn top_bar_ticker_parts(
    snapshot: &ExchangePanelSnapshot,
    owls_dashboard: &OwlsDashboard,
) -> (&'static str, String) {
    let watched = dedupe_ticker_items(
        snapshot
            .watch
            .as_ref()
            .into_iter()
            .flat_map(|watch| watch.watches.iter())
            .filter_map(|watch_row| watch_row_event_name(snapshot, watch_row)),
        3,
    );
    if !watched.is_empty() {
        return ("watch", watched.join("  •  "));
    }

    let live_soccer = dedupe_ticker_items(
        owls_dashboard
            .endpoints
            .iter()
            .flat_map(|endpoint| endpoint.live_scores.iter())
            .filter(|event| event.sport.trim().eq_ignore_ascii_case("soccer"))
            .map(|event| {
                if event.name.trim().is_empty() {
                    format!("{} vs {}", event.away_team.trim(), event.home_team.trim())
                } else {
                    event.name.clone()
                }
            }),
        3,
    );
    if !live_soccer.is_empty() {
        return ("live", live_soccer.join("  •  "));
    }

    let live = dedupe_ticker_items(
        snapshot
            .open_positions
            .iter()
            .filter(|row| is_live_ticker_position(row))
            .map(|row| row.event.clone()),
        3,
    );
    if !live.is_empty() {
        return ("live", live.join("  •  "));
    }

    let next = dedupe_ticker_items(snapshot.events.iter().map(|event| event.label.clone()), 3);
    if !next.is_empty() {
        return ("next", next.join("  •  "));
    }

    ("watch", String::from("clear"))
}

fn dedupe_ticker_items(items: impl IntoIterator<Item = String>, limit: usize) -> Vec<String> {
    let mut unique = Vec::new();
    let mut seen = HashSet::new();

    for item in items {
        let label = item.trim();
        if label.is_empty() {
            continue;
        }
        let key = normalize_manual_key(label);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        unique.push(label.to_string());
        if unique.len() >= limit {
            break;
        }
    }

    unique
}

fn is_live_ticker_position(row: &OpenPositionRow) -> bool {
    row.is_in_play
        || row.market_status.eq_ignore_ascii_case("live")
        || !row.current_score.trim().is_empty()
        || !row.live_clock.trim().is_empty()
}

fn find_owls_sharp_price(
    dashboard: &OwlsDashboard,
    event: &str,
    market: &str,
    selection: &str,
) -> Option<f64> {
    owls::find_pinnacle_quote(dashboard, event, market, selection)
        .and_then(|quote| quote.decimal_price)
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

fn actionable_decision_count(snapshot: &ExchangePanelSnapshot) -> usize {
    snapshot
        .decisions
        .iter()
        .filter(|decision| !decision.status.eq_ignore_ascii_case("hold"))
        .count()
}

fn actionable_exit_recommendation_count(snapshot: &ExchangePanelSnapshot) -> usize {
    snapshot
        .exit_recommendations
        .iter()
        .filter(|recommendation| !recommendation.action.eq_ignore_ascii_case("hold"))
        .count()
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

fn current_time_label() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let seconds_of_day = seconds % 86_400;
    let hours = seconds_of_day / 3_600;
    let minutes = (seconds_of_day % 3_600) / 60;
    let secs = seconds_of_day % 60;
    format!("{hours:02}:{minutes:02}:{secs:02}")
}

fn dispatch_notification_delivery(entry: NotificationEntry, config: &AlertConfig) {
    let desktop_notifications = config.desktop_notifications;
    let sound_effects = config.sound_effects;
    thread::spawn(move || {
        let sound_name = alert_sound_name(&entry.rule_key, entry.level);
        if desktop_notifications {
            let _ = std::process::Command::new("notify-send")
                .arg("-u")
                .arg(entry.level.notify_send_urgency())
                .arg("-h")
                .arg(format!("string:sound-name:{sound_name}"))
                .arg(&entry.title)
                .arg(&entry.detail)
                .status();
        }
        if sound_effects {
            let sound_status = std::process::Command::new("canberra-gtk-play")
                .arg("-i")
                .arg(sound_name)
                .status();
            if sound_status.is_err() {
                let _ = std::io::stderr().write_all(b"\x07");
                let _ = std::io::stderr().flush();
            }
        }
    });
}

fn alert_sound_name(rule_key: &str, level: NotificationLevel) -> &'static str {
    match rule_key {
        "bet_placed" => "complete",
        "bet_settled" => "message",
        "recorder_failures" => "dialog-error",
        "provider_errors" => "network-error",
        "matchbook_failures" => "dialog-warning",
        "snapshot_stale" => "dialog-warning",
        "exit_recommendations" => "message-new-instant",
        "decision_queue" => "message",
        "tracked_bets" => "complete",
        "opportunity_detected" => "message-new-instant",
        "watched_movement" => "dialog-warning",
        "owls_errors" => "dialog-warning",
        _ => match level {
            NotificationLevel::Info => "message",
            NotificationLevel::Warning => "dialog-warning",
            NotificationLevel::Critical => "dialog-error",
        },
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

fn backend_action_prices(
    side: &str,
    price: Option<f64>,
) -> (Option<f64>, Option<f64>, TradingActionSide) {
    if side.eq_ignore_ascii_case("lay")
        || side.eq_ignore_ascii_case("sell")
        || side.eq_ignore_ascii_case("lose")
    {
        (None, price, TradingActionSide::Sell)
    } else {
        (price, None, TradingActionSide::Buy)
    }
}

fn intel_source_statuses_for_view(
    view: IntelView,
    dashboard: Option<&MarketIntelDashboard>,
    phase: &str,
    last_error: Option<&str>,
) -> Vec<IntelSourceStatus> {
    let Some(dashboard) = dashboard else {
        let health = match phase {
            "error" => "error",
            "stale" => "stale",
            "loading" => "loading",
            _ => "idle",
        };
        let freshness = match phase {
            "error" | "stale" => "stale",
            "loading" => "pending",
            _ => "idle",
        };
        let detail = last_error
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| match phase {
                "error" => format!("{} dashboard failed to load.", view.label()),
                "stale" => format!("{} dashboard refresh went stale.", view.label()),
                "loading" => format!("{} dashboard is still loading.", view.label()),
                _ => format!("{} dashboard has not been requested yet.", view.label()),
            });

        return vec![
            IntelSourceStatus {
                source: IntelSource::OddsEntry,
                health: String::from(health),
                freshness: String::from(freshness),
                transport: String::from("worker"),
                detail: detail.clone(),
            },
            IntelSourceStatus {
                source: IntelSource::FairOdds,
                health: String::from(health),
                freshness: String::from(freshness),
                transport: String::from("worker"),
                detail: detail.clone(),
            },
            IntelSourceStatus {
                source: IntelSource::OddsApi,
                health: String::from(health),
                freshness: String::from(freshness),
                transport: String::from("worker"),
                detail,
            },
        ];
    };

    dashboard
        .sources
        .iter()
        .map(|status| IntelSourceStatus {
            source: intel_source_from_market_intel(status.source.clone()),
            health: match (status.mode, status.status) {
                (crate::market_intel::SourceLoadMode::Fixture, _) => String::from("fixture"),
                (_, value) => value.as_str().to_string(),
            },
            freshness: if status.refreshed_at.trim().is_empty() {
                status.mode.as_str().to_string()
            } else {
                status.refreshed_at.clone()
            },
            transport: match status.mode {
                crate::market_intel::SourceLoadMode::Live => String::from("REST"),
                crate::market_intel::SourceLoadMode::Fixture => String::from("fixture"),
            },
            detail: status.detail.clone(),
        })
        .collect()
}

fn intel_rows_for_view(view: IntelView, dashboard: Option<&MarketIntelDashboard>) -> Vec<IntelRow> {
    let Some(dashboard) = dashboard else {
        return Vec::new();
    };

    match view {
        IntelView::Markets => dashboard
            .markets
            .iter()
            .map(intel_row_from_opportunity)
            .collect(),
        IntelView::Arbitrages => dashboard
            .arbitrages
            .iter()
            .map(intel_row_from_opportunity)
            .collect(),
        IntelView::PlusEv => dashboard
            .plus_ev
            .iter()
            .map(intel_row_from_opportunity)
            .collect(),
        IntelView::Event => dashboard
            .event_detail
            .as_ref()
            .map(intel_rows_from_event_detail)
            .unwrap_or_default(),
        IntelView::Drops => dashboard
            .drops
            .iter()
            .map(intel_row_from_opportunity)
            .collect(),
        IntelView::Value => dashboard
            .value
            .iter()
            .map(intel_row_from_opportunity)
            .collect(),
    }
}

fn intel_source_from_market_intel(source: crate::market_intel::MarketIntelSourceId) -> IntelSource {
    match source.key() {
        "fair_odds" => IntelSource::FairOdds,
        "odds_api" => IntelSource::OddsApi,
        _ => IntelSource::OddsEntry,
    }
}

fn intel_rows_from_event_detail(detail: &crate::market_intel::MarketEventDetail) -> Vec<IntelRow> {
    detail
        .quotes
        .iter()
        .enumerate()
        .map(|(index, quote)| IntelRow {
            id: format!("event:{}:{}", detail.event_id, index),
            source: intel_source_from_market_intel(detail.source.clone()),
            event: detail.event_name.clone(),
            competition: if detail.sport.trim().is_empty() {
                intel_source_from_market_intel(detail.source.clone())
                    .label()
                    .to_string()
            } else {
                detail.sport.clone()
            },
            market: quote.market_name.clone(),
            selection: quote.selection_name.clone(),
            bookmaker: quote.venue.clone(),
            exchange: quote.venue.clone(),
            back_odds: quote.price.unwrap_or_default(),
            lay_odds: quote.fair_price,
            fair_odds: quote.fair_price,
            edge_pct: None,
            arb_pct: None,
            liquidity: quote.liquidity,
            status: if quote.is_live {
                String::from("live")
            } else {
                String::from("ready")
            },
            updated_at: quote.updated_at.clone(),
            route: quote.event_url.clone(),
            deep_link_url: quote.deep_link_url.clone(),
            note: quote.notes.join(" | "),
        })
        .collect()
}

fn intel_row_from_opportunity(row: &MarketOpportunityRow) -> IntelRow {
    let primary = row.primary_quote();
    let secondary = row.secondary_quote();
    let back_odds = primary
        .and_then(|quote| quote.price)
        .or(row.price)
        .unwrap_or_default();

    IntelRow {
        id: row.id.clone(),
        source: intel_source_from_market_intel(row.source.clone()),
        event: row.event_name.clone(),
        competition: if row.competition_name.trim().is_empty() {
            row.sport.clone()
        } else {
            row.competition_name.clone()
        },
        market: row.market_name.clone(),
        selection: if row.selection_name.trim().is_empty() {
            primary
                .map(|quote| quote.selection_name.clone())
                .unwrap_or_default()
        } else {
            row.selection_name.clone()
        },
        bookmaker: primary
            .map(|quote| quote.venue.clone())
            .unwrap_or_else(|| row.venue.clone()),
        exchange: secondary
            .map(|quote| quote.venue.clone())
            .unwrap_or_else(|| row.secondary_venue.clone()),
        back_odds,
        lay_odds: secondary
            .and_then(|quote| quote.price)
            .or(row.secondary_price),
        fair_odds: primary
            .and_then(|quote| quote.fair_price)
            .or(row.fair_price),
        edge_pct: row.edge_percent,
        arb_pct: row.arbitrage_margin,
        liquidity: primary.and_then(|quote| quote.liquidity).or(row.liquidity),
        status: if row.is_live {
            String::from("live")
        } else if primary.is_some() {
            String::from("ready")
        } else {
            String::from("idle")
        },
        updated_at: row.updated_at.clone(),
        route: primary
            .map(|quote| quote.event_url.clone())
            .unwrap_or_else(|| row.event_url.clone()),
        deep_link_url: primary
            .map(|quote| quote.deep_link_url.clone())
            .unwrap_or_else(|| row.deep_link_url.clone()),
        note: row.notes.join(" | "),
    }
}

fn new_trading_action_request_id(source: TradingActionSource) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{source:?}-{millis}").to_lowercase()
}

pub(crate) fn start_oddsmatcher_worker(
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

pub(crate) fn start_market_intel_worker() -> (Sender<MarketIntelJob>, Receiver<MarketIntelResult>) {
    let (job_tx, job_rx) = mpsc::channel::<MarketIntelJob>();
    let (result_tx, result_rx) = mpsc::channel::<MarketIntelResult>();
    thread::spawn(move || {
        while let Ok(job) = job_rx.recv() {
            let dashboard = market_intel::load_dashboard_with_options(
                matches!(job.reason, MarketIntelSyncReason::Manual),
                job.sport_key.as_deref(),
            )
            .map_err(|error| error.to_string());
            if result_tx
                .send(MarketIntelResult {
                    dashboard,
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

fn default_owls_sync_async_client() -> reqwest::Client {
    owls::build_async_client().unwrap_or_else(|_| reqwest::Client::new())
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

#[cfg(test)]
fn matchbook_error_has_status(error: &color_eyre::Report, status_code: u16) -> bool {
    error.to_string().contains(&format!(" {status_code}:"))
}

fn inferred_owls_sport(snapshot: &ExchangePanelSnapshot) -> Option<&'static str> {
    snapshot
        .tracked_bets
        .iter()
        .find_map(|tracked_bet| infer_owls_sport_from_key(&tracked_bet.sport_key))
        .or_else(|| {
            snapshot
                .open_positions
                .iter()
                .find_map(infer_owls_sport_from_open_position)
        })
}

fn infer_owls_sport_from_key(value: &str) -> Option<&'static str> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.starts_with("soccer") || normalized.contains("premierleague") {
        Some("soccer")
    } else if normalized.contains("wnba") {
        Some("wnba")
    } else if normalized.contains("nba") {
        Some("nba")
    } else if normalized.contains("ncaab") {
        Some("ncaab")
    } else if normalized.contains("ncaaf") {
        Some("ncaaf")
    } else if normalized.contains("nfl") {
        Some("nfl")
    } else if normalized.contains("nhl") {
        Some("nhl")
    } else if normalized.contains("mlb") {
        Some("mlb")
    } else if normalized.contains("mma") {
        Some("mma")
    } else if normalized.contains("tennis") {
        Some("tennis")
    } else if normalized.contains("cs2") {
        Some("cs2")
    } else {
        None
    }
}

fn infer_owls_sport_from_open_position(row: &OpenPositionRow) -> Option<&'static str> {
    let event_url = row.event_url.trim().to_ascii_lowercase();
    if event_url.contains("/football/") || event_url.contains("/soccer/") {
        Some("soccer")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Arc as Rc, Mutex, Mutex as RefCell};
    use std::time::{Duration, Instant};

    use crate::domain::{
        EventCandidateSummary, ExchangePanelSnapshot, OpenPositionRow, RuntimeSummary,
        TrackedBetRow, VenueId, VenueStatus, VenueSummary, WatchRow, WatchSnapshot, WorkerStatus,
        WorkerSummary,
    };
    use crate::exchange_api::{MatchbookAccountState, MatchbookOfferRow};
    use crate::manual_positions::ManualPositionEntry;
    use crate::owls::{
        self, OwlsDashboard, OwlsEndpointId, OwlsLiveIncident, OwlsLiveScoreEvent, OwlsLiveStat,
        OwlsMarketQuote, OwlsPlayerRating, OwlsPreviewRow, OwlsSyncReason,
    };
    use crate::provider::{ExchangeProvider, ProviderRequest};
    use crate::recorder::{RecorderConfig, RecorderStatus, RecorderSupervisor};
    use crate::resource_state::ResourcePhase;
    use crate::stub_provider::StubExchangeProvider;
    use crate::trading_actions::{
        TradingActionIntent, TradingActionKind, TradingActionMode, TradingActionSide,
        TradingActionSource, TradingActionSourceContext, TradingExecutionPolicy, TradingRiskReport,
        TradingTimeInForce,
    };
    use crossterm::event::KeyCode;

    use super::{
        populate_snapshot_enrichment, App, MatchbookSyncJob, MatchbookSyncReason,
        MatchbookSyncResult, NotificationLevel, OwlsSyncJob, OwlsSyncResult, Panel, ProviderJob,
        ProviderResult, TradingSection, MAX_EVENT_HISTORY,
    };

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
    fn intel_source_statuses_surface_error_without_dashboard_payload() {
        let statuses = super::intel_source_statuses_for_view(
            crate::app_state::IntelView::Markets,
            None,
            "error",
            Some("backend unavailable"),
        );

        assert_eq!(statuses.len(), 3);
        assert!(statuses.iter().all(|status| status.health == "error"));
        assert!(statuses
            .iter()
            .all(|status| status.detail.contains("backend unavailable")));
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

        let normalized = super::normalize_snapshot(snapshot, "bet365", &[]);

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
    fn normalize_snapshot_merges_manual_positions_into_other_open_bets() {
        let snapshot = sample_snapshot("Initial dashboard");
        let manual_positions = vec![ManualPositionEntry {
            event: String::from("Malta v Luxembourg"),
            market: String::from("Match Betting"),
            selection: String::from("X"),
            venue: String::from("betway"),
            odds: 3.2,
            stake: 50.0,
            ..ManualPositionEntry::default()
        }];

        let normalized = super::normalize_snapshot(snapshot, "", &manual_positions);

        assert!(normalized.other_open_bets.iter().any(|bet| {
            bet.venue.eq_ignore_ascii_case("betway")
                && bet.event == "Malta v Luxembourg"
                && bet.label == "X"
                && (bet.odds - 3.2).abs() < 0.001
                && (bet.stake - 50.0).abs() < 0.001
        }));
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

    #[test]
    fn notifications_overlay_marks_alerts_read() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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
        app.alerts_config.desktop_notifications = false;
        app.alerts_config.sound_effects = false;

        app.emit_alert(
            "tracked_bets",
            NotificationLevel::Info,
            "Tracked bets increased",
            "2 tracked bets now loaded.",
        );

        assert_eq!(app.unread_notification_count(), 1);
        assert!(!app.notifications_overlay_visible());

        app.toggle_notifications_overlay();

        assert!(app.notifications_overlay_visible());
        assert_eq!(app.unread_notification_count(), 0);
    }

    #[test]
    fn provider_errors_raise_notifications() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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
        app.alerts_config.desktop_notifications = false;
        app.alerts_config.sound_effects = false;

        app.record_provider_error(
            "Refresh failed",
            "worker timed out",
            Some(VenueId::Smarkets),
        );

        assert_eq!(app.notifications.len(), 1);
        assert_eq!(app.notifications[0].rule_key, "provider_errors");
        assert_eq!(app.notifications[0].level, NotificationLevel::Critical);
    }

    #[test]
    fn replace_snapshot_alerts_when_tracked_bets_increase() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: Arc::new(Mutex::new(0)),
                live_refresh_count: Arc::new(Mutex::new(0)),
                load_snapshot: sample_snapshot("Initial dashboard"),
                cached_refresh_snapshot: sample_snapshot("Initial dashboard"),
                live_refresh_snapshot: sample_snapshot("Initial dashboard"),
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
        app.alerts_config.desktop_notifications = false;
        app.alerts_config.sound_effects = false;
        let mut next = sample_snapshot("updated");
        next.tracked_bets = vec![TrackedBetRow {
            bet_id: String::from("bet-1"),
            event: String::from("Arsenal vs Everton"),
            selection: String::from("Draw"),
            ..TrackedBetRow::default()
        }];

        app.replace_snapshot(next);

        assert_eq!(app.notifications.len(), 1);
        assert_eq!(app.notifications[0].rule_key, "tracked_bets");
        assert!(app.notifications[0].title.contains("Tracked bets"));
    }

    #[test]
    fn recorder_startup_suppresses_bootstrap_tracked_bet_alerts() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut startup_snapshot = sample_snapshot("Recorder dashboard");
        startup_snapshot.tracked_bets = vec![TrackedBetRow {
            bet_id: String::from("bet-1"),
            event: String::from("Arsenal vs Everton"),
            selection: String::from("Draw"),
            ..TrackedBetRow::default()
        }];
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
            Box::new(|| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(move |_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: startup_snapshot.clone(),
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
        app.alerts_config.desktop_notifications = false;
        app.alerts_config.sound_effects = false;

        app.start_recorder().expect("start recorder");
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));

        assert!(app.notifications.is_empty());
        assert!(!app.recorder_startup_alerts_pending);
        assert!(app.recorder_startup_alerts_muted_until.is_none());
    }

    #[test]
    fn tracked_bet_alerts_resume_after_recorder_startup_snapshot() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut startup_snapshot = sample_snapshot("Recorder dashboard");
        startup_snapshot.tracked_bets = vec![TrackedBetRow {
            bet_id: String::from("bet-1"),
            event: String::from("Arsenal vs Everton"),
            selection: String::from("Draw"),
            ..TrackedBetRow::default()
        }];
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
            Box::new(|| {
                Box::new(StubExchangeProvider::default()) as Box<dyn ExchangeProvider + Send>
            }),
            Box::new(move |_| {
                Box::new(RefreshingProvider {
                    cached_refresh_count: Rc::new(RefCell::new(0)),
                    live_refresh_count: Rc::new(RefCell::new(0)),
                    load_snapshot: startup_snapshot.clone(),
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
        app.alerts_config.desktop_notifications = false;
        app.alerts_config.sound_effects = false;

        app.start_recorder().expect("start recorder");
        assert!(app.wait_for_async_idle(Duration::from_millis(200)));
        assert!(app.notifications.is_empty());

        let mut next = sample_snapshot("updated");
        next.tracked_bets = vec![
            TrackedBetRow {
                bet_id: String::from("bet-1"),
                event: String::from("Arsenal vs Everton"),
                selection: String::from("Draw"),
                ..TrackedBetRow::default()
            },
            TrackedBetRow {
                bet_id: String::from("bet-2"),
                event: String::from("Chelsea vs Liverpool"),
                selection: String::from("Chelsea"),
                ..TrackedBetRow::default()
            },
        ];

        app.replace_snapshot(next);

        assert_eq!(app.notifications.len(), 1);
        assert_eq!(app.notifications[0].rule_key, "tracked_bets");
    }

    #[test]
    fn confirmed_trade_execution_alerts_bet_placed() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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
        app.alerts_config.desktop_notifications = false;
        app.alerts_config.sound_effects = false;

        app.apply_provider_snapshot_result(
            ProviderRequest::ExecuteTradingAction {
                intent: Box::new(sample_trading_action_intent(TradingActionMode::Confirm)),
            },
            sample_snapshot("submitted"),
            None,
        );

        assert_eq!(
            app.notifications.back().expect("notification").rule_key,
            "bet_placed"
        );
    }

    #[test]
    fn replace_snapshot_alerts_when_bet_settles() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut initial = sample_snapshot("initial");
        initial.tracked_bets = vec![TrackedBetRow {
            bet_id: String::from("bet-1"),
            platform: String::from("matchbook"),
            event: String::from("Arsenal vs Everton"),
            market: String::from("Match Odds"),
            selection: String::from("Draw"),
            status: String::from("open"),
            ..TrackedBetRow::default()
        }];
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: Rc::new(RefCell::new(0)),
                live_refresh_count: Rc::new(RefCell::new(0)),
                load_snapshot: initial,
                cached_refresh_snapshot: sample_snapshot("cached"),
                live_refresh_snapshot: sample_snapshot("live"),
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
        app.alerts_config.desktop_notifications = false;
        app.alerts_config.sound_effects = false;
        let mut next = sample_snapshot("settled");
        next.tracked_bets = vec![TrackedBetRow {
            bet_id: String::from("bet-1"),
            platform: String::from("matchbook"),
            event: String::from("Arsenal vs Everton"),
            market: String::from("Match Odds"),
            selection: String::from("Draw"),
            status: String::from("won"),
            settled_at: String::from("2026-03-25T20:00:00Z"),
            realised_pnl_gbp: Some(12.4),
            ..TrackedBetRow::default()
        }];

        app.replace_snapshot(next);

        assert_eq!(
            app.notifications.back().expect("notification").rule_key,
            "bet_settled"
        );
    }

    #[test]
    fn live_sharp_opportunity_helper_detects_crossing_threshold() {
        let mut snapshot = sample_snapshot("opportunity");
        snapshot.other_open_bets = vec![crate::domain::OtherOpenBetRow {
            venue: String::from("bet365"),
            event: String::from("Chelsea vs Liverpool"),
            label: String::from("Chelsea"),
            market: String::from("Match Odds"),
            side: String::from("back"),
            odds: 2.2,
            stake: 10.0,
            status: String::from("open"),
            funding_kind: String::from("cash"),
            current_cashout_value: None,
            supports_cash_out: false,
        }];

        let previous = sample_owls_dashboard_with_quote("Chelsea vs Liverpool", "Chelsea", 2.18);
        let current = sample_owls_dashboard_with_quote("Chelsea vs Liverpool", "Chelsea", 2.00);

        let detail =
            super::live_sharp_opportunity_alert_detail(&snapshot, &previous, &current, 3.0)
                .expect("sharp opportunity should alert");

        assert!(detail.contains("bet365"));
        assert!(detail.contains("Chelsea"));
        assert!(detail.contains("pinnacle"));
    }

    #[test]
    fn sharp_watch_movement_helper_uses_owls_prices() {
        let mut snapshot = sample_snapshot("watch moved");
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("Arsenal vs Everton"),
            event_status: String::new(),
            event_url: String::new(),
            contract: String::from("Draw"),
            market: String::from("Full-time result"),
            status: String::from("open"),
            market_status: String::from("open"),
            is_in_play: true,
            price: 3.2,
            stake: 10.0,
            liability: 10.0,
            current_value: 0.0,
            pnl_amount: 0.0,
            overall_pnl_known: false,
            current_back_odds: Some(3.2),
            current_implied_probability: Some(1.0 / 3.2),
            current_implied_percentage: Some(100.0 / 3.2),
            current_buy_odds: Some(3.2),
            current_buy_implied_probability: Some(1.0 / 3.2),
            current_sell_odds: None,
            current_sell_implied_probability: None,
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::new(),
            can_trade_out: true,
        }];
        snapshot.watch = Some(WatchSnapshot {
            position_count: 1,
            watch_count: 1,
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
            watches: vec![WatchRow {
                contract: String::from("Draw"),
                market: String::from("Full-time result"),
                position_count: 1,
                can_trade_out: true,
                total_stake: 10.0,
                total_liability: 10.0,
                current_pnl_amount: 0.0,
                current_back_odds: Some(3.0),
                average_entry_lay_odds: 3.4,
                entry_implied_probability: 1.0 / 3.4,
                profit_take_back_odds: 2.8,
                profit_take_implied_probability: 1.0 / 2.8,
                stop_loss_back_odds: 3.7,
                stop_loss_implied_probability: 1.0 / 3.7,
            }],
        });

        let previous = sample_owls_dashboard_with_quote("Arsenal vs Everton", "Draw", 3.00);
        let current = sample_owls_dashboard_with_quote("Arsenal vs Everton", "Draw", 3.30);

        let detail = super::sharp_watch_movement_alert_detail(&snapshot, &previous, &current, 5.0)
            .expect("watch movement should alert");

        assert!(detail.contains("Draw"));
        assert!(detail.contains("3.00->3.30"));
    }

    #[test]
    fn stale_owls_result_does_not_reset_selected_sport() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        app.owls_sync_rx = rx;
        app.owls_sync_in_flight = true;
        app.owls_dashboard = owls::dashboard_for_sport("nfl");

        tx.send(OwlsSyncResult {
            outcome: owls::OwlsSyncOutcome {
                dashboard: owls::dashboard_for_sport("nba"),
                checked_count: 3,
                changed_count: 1,
                changed: true,
            },
            reason: OwlsSyncReason::Background,
        })
        .expect("send stale result");

        app.drain_owls_sync_results();

        assert_eq!(app.owls_dashboard.sport, "nfl");
    }

    #[test]
    fn top_bar_ticker_prefers_watched_events() {
        let mut app = App::default();
        let mut snapshot = sample_snapshot("ticker");
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("Arsenal vs Everton"),
            event_status: String::new(),
            event_url: String::new(),
            contract: String::from("Draw"),
            market: String::from("Full-time result"),
            status: String::from("open"),
            market_status: String::from("ready"),
            is_in_play: false,
            price: 3.2,
            stake: 10.0,
            liability: 22.0,
            current_value: 10.0,
            pnl_amount: 0.0,
            overall_pnl_known: true,
            current_back_odds: Some(3.0),
            current_implied_probability: Some(1.0 / 3.0),
            current_implied_percentage: Some(100.0 / 3.0),
            current_buy_odds: Some(3.0),
            current_buy_implied_probability: Some(1.0 / 3.0),
            current_sell_odds: Some(3.1),
            current_sell_implied_probability: Some(1.0 / 3.1),
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::new(),
            can_trade_out: true,
        }];
        snapshot.watch = Some(WatchSnapshot {
            position_count: 1,
            watch_count: 1,
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
            watches: vec![WatchRow {
                contract: String::from("Draw"),
                market: String::from("Full-time result"),
                position_count: 1,
                can_trade_out: true,
                total_stake: 10.0,
                total_liability: 10.0,
                current_pnl_amount: 0.0,
                current_back_odds: Some(3.0),
                average_entry_lay_odds: 3.4,
                entry_implied_probability: 1.0 / 3.4,
                profit_take_back_odds: 2.8,
                profit_take_implied_probability: 1.0 / 2.8,
                stop_loss_back_odds: 3.7,
                stop_loss_implied_probability: 1.0 / 3.7,
            }],
        });
        app.replace_snapshot(snapshot);

        let (kind, body) = app.top_bar_ticker_parts();
        assert_eq!(kind, "watch");
        assert_eq!(body, "Arsenal vs Everton");
    }

    #[test]
    fn top_bar_ticker_falls_back_to_live_events() {
        let mut app = App::default();
        let mut snapshot = sample_snapshot("ticker");
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("Malta vs Luxembourg"),
            event_status: String::new(),
            event_url: String::new(),
            contract: String::from("Draw"),
            market: String::from("Full-time result"),
            status: String::from("open"),
            market_status: String::from("live"),
            is_in_play: false,
            price: 3.3,
            stake: 10.0,
            liability: 23.0,
            current_value: 10.0,
            pnl_amount: 0.0,
            overall_pnl_known: true,
            current_back_odds: Some(3.2),
            current_implied_probability: Some(1.0 / 3.2),
            current_implied_percentage: Some(100.0 / 3.2),
            current_buy_odds: Some(3.2),
            current_buy_implied_probability: Some(1.0 / 3.2),
            current_sell_odds: Some(3.3),
            current_sell_implied_probability: Some(1.0 / 3.3),
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::from("72"),
            can_trade_out: true,
        }];
        app.replace_snapshot(snapshot);

        let (kind, body) = app.top_bar_ticker_parts();
        assert_eq!(kind, "live");
        assert_eq!(body, "Malta vs Luxembourg");
    }

    #[test]
    fn top_bar_ticker_falls_back_to_upcoming_events() {
        let mut app = App::default();
        let mut snapshot = sample_snapshot("ticker");
        snapshot.events = vec![EventCandidateSummary {
            id: String::from("event-1"),
            label: String::from("Liverpool vs City"),
            competition: String::from("Premier League"),
            start_time: String::from("2026-03-22T17:30:00Z"),
            url: String::from("https://example.com/liverpool-city"),
        }];
        app.replace_snapshot(snapshot);

        let (kind, body) = app.top_bar_ticker_parts();
        assert_eq!(kind, "next");
        assert_eq!(body, "Liverpool vs City");
    }

    #[test]
    fn background_owls_sync_does_not_replace_operator_feed_status() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let initial_event_count = app.recent_events().len();
        app.owls_sync_rx = rx;
        app.owls_sync_in_flight = true;
        app.status_message = String::from("Pinned operator message.");

        let mut dashboard = owls::dashboard_for_sport("nba");
        dashboard.status_line = String::from("Owls background refresh completed.");
        tx.send(OwlsSyncResult {
            outcome: owls::OwlsSyncOutcome {
                dashboard,
                checked_count: 3,
                changed_count: 1,
                changed: true,
            },
            reason: OwlsSyncReason::Background,
        })
        .expect("send background result");

        app.drain_owls_sync_results();

        assert_eq!(app.status_message(), "Pinned operator message.");
        assert_eq!(app.recent_events().len(), initial_event_count);
    }

    #[test]
    fn background_snapshot_preserves_operator_feed_status() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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

        app.status_message = String::from("Pinned operator message.");
        app.last_successful_snapshot_at = Some(String::from("2026-03-19T10:00:00Z"));

        app.replace_snapshot(sample_runtime_snapshot(
            "Auto refreshed dashboard",
            "2026-03-19T10:00:01Z",
            false,
            "cached",
        ));

        assert_eq!(app.status_message(), "Pinned operator message.");
    }

    #[test]
    fn provider_watchdog_restart_replaces_stuck_worker_channel() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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

        let (dead_job_tx, mut dead_job_rx) = tokio::sync::mpsc::unbounded_channel::<ProviderJob>();
        let (_dead_result_tx, dead_result_rx) =
            tokio::sync::mpsc::unbounded_channel::<ProviderResult>();
        app.provider_tx = dead_job_tx;
        app.provider_rx = dead_result_rx;
        app.provider_in_flight = true;
        app.provider_resource_state
            .begin_refresh(Instant::now() - Duration::from_secs(60));

        app.expire_stuck_provider_request_placeholder();
        app.queue_provider_request(ProviderJob {
            request: ProviderRequest::RefreshCached,
            failure_context: String::from("test"),
            event_message: None,
        });

        assert!(dead_job_rx.try_recv().is_err());
    }

    #[test]
    fn provider_watchdog_restart_preserves_pending_job() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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

        app.provider_in_flight = true;
        app.provider_resource_state
            .begin_refresh(Instant::now() - Duration::from_secs(60));
        app.provider_pending_job = Some(ProviderJob {
            request: ProviderRequest::RefreshLive,
            failure_context: String::from("test"),
            event_message: None,
        });

        app.expire_stuck_provider_request_placeholder();
        assert!(app.provider_pending_job.is_none());
        assert!(app.provider_resource_state.is_loading());

        app.wait_for_async_idle(Duration::from_millis(200));

        assert!(app.provider_pending_job.is_none());
        assert_eq!(app.provider_resource_state.phase(), ResourcePhase::Ready);
    }

    #[test]
    fn owls_watchdog_restart_replaces_stuck_worker_channel() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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

        let (dead_job_tx, mut dead_job_rx) = tokio::sync::mpsc::unbounded_channel::<OwlsSyncJob>();
        let (_dead_result_tx, dead_result_rx) =
            tokio::sync::mpsc::unbounded_channel::<OwlsSyncResult>();
        app.owls_sync_tx = dead_job_tx;
        app.owls_sync_rx = dead_result_rx;
        app.owls_sync_in_flight = true;
        app.owls_resource_state
            .begin_refresh(Instant::now() - Duration::from_secs(60));

        app.expire_stuck_owls_sync_placeholder();
        app.request_owls_sync(OwlsSyncReason::Manual);

        assert!(dead_job_rx.try_recv().is_err());
    }

    #[test]
    fn matchbook_watchdog_restart_replaces_stuck_worker_channel() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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

        let (dead_job_tx, mut dead_job_rx) =
            tokio::sync::mpsc::unbounded_channel::<MatchbookSyncJob>();
        let (_dead_result_tx, dead_result_rx) =
            tokio::sync::mpsc::unbounded_channel::<MatchbookSyncResult>();
        app.matchbook_sync_tx = dead_job_tx;
        app.matchbook_sync_rx = dead_result_rx;
        app.matchbook_sync_in_flight = true;
        app.matchbook_resource_state
            .begin_refresh(Instant::now() - Duration::from_secs(60));

        app.expire_stuck_matchbook_sync_placeholder();
        app.request_matchbook_sync(MatchbookSyncReason::Manual);

        assert!(dead_job_rx.try_recv().is_err());
    }

    #[test]
    fn matchbook_sync_error_preserves_last_good_account_state() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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

        let prior_state = MatchbookAccountState {
            status_line: String::from("good"),
            ..MatchbookAccountState::default()
        };
        app.matchbook_account_state = Some(prior_state.clone());
        app.matchbook_resource_state.finish_ok(prior_state);
        app.matchbook_sync_in_flight = true;
        app.matchbook_sync_tx = tokio::sync::mpsc::unbounded_channel::<MatchbookSyncJob>().0;
        let (result_tx, result_rx) = tokio::sync::mpsc::unbounded_channel::<MatchbookSyncResult>();
        app.matchbook_sync_rx = result_rx;

        result_tx
            .send(MatchbookSyncResult {
                state: Err(String::from("current offers failed")),
                reason: MatchbookSyncReason::Manual,
            })
            .expect("send matchbook error");

        app.drain_matchbook_sync_results();

        assert_eq!(
            app.matchbook_account_state
                .as_ref()
                .map(|state| state.status_line.as_str()),
            Some("good")
        );
        assert_eq!(
            app.matchbook_resource_state.last_good(),
            app.matchbook_account_state.as_ref()
        );
    }

    #[test]
    fn replace_snapshot_auto_switches_default_owls_sport_for_soccer_positions() {
        let mut app = App::default();
        assert_eq!(app.owls_sport(), "nba");

        let mut snapshot = sample_snapshot("soccer snapshot");
        snapshot.tracked_bets = vec![TrackedBetRow {
            sport_key: String::from("soccer_epl"),
            ..TrackedBetRow::default()
        }];

        app.replace_snapshot(snapshot);

        assert_eq!(app.owls_sport(), "soccer");
    }

    #[test]
    fn snapshot_enrichment_projects_owls_quotes_and_live_scores() {
        let mut snapshot = sample_snapshot("snapshot");
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("Malta vs Luxembourg"),
            event_status: String::new(),
            event_url: String::from("https://example.com/malta-luxembourg"),
            contract: String::from("Draw"),
            market: String::from("Full-time result"),
            status: String::from("open"),
            market_status: String::from("live"),
            is_in_play: true,
            price: 3.35,
            stake: 25.0,
            liability: 58.75,
            current_value: 25.0,
            pnl_amount: 0.0,
            overall_pnl_known: true,
            current_back_odds: Some(3.35),
            current_implied_probability: Some(1.0 / 3.35),
            current_implied_percentage: Some(100.0 / 3.35),
            current_buy_odds: Some(3.35),
            current_buy_implied_probability: Some(1.0 / 3.35),
            current_sell_odds: Some(3.4),
            current_sell_implied_probability: Some(1.0 / 3.4),
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::new(),
            can_trade_out: true,
        }];

        let mut dashboard = OwlsDashboard::default();
        dashboard.sport = String::from("soccer");
        if let Some(endpoint) = dashboard
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == OwlsEndpointId::Realtime)
        {
            endpoint.status = String::from("ready");
            endpoint.quotes = vec![
                OwlsMarketQuote {
                    book: String::from("matchbook"),
                    event: String::from("Luxembourg @ Malta"),
                    selection: String::from("Draw"),
                    market_key: String::from("h2h"),
                    decimal_price: Some(3.25),
                    limit_amount: Some(120.0),
                    ..OwlsMarketQuote::default()
                },
                OwlsMarketQuote {
                    book: String::from("pinnacle"),
                    event: String::from("Luxembourg @ Malta"),
                    selection: String::from("Draw"),
                    market_key: String::from("h2h"),
                    decimal_price: Some(3.10),
                    ..OwlsMarketQuote::default()
                },
            ];
        }
        if let Some(endpoint) = dashboard
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == OwlsEndpointId::ScoresSport)
        {
            endpoint.status = String::from("ready");
            endpoint.live_scores = vec![OwlsLiveScoreEvent {
                sport: String::from("soccer"),
                event_id: String::from("soccer:Malta@Luxembourg-20260325"),
                name: String::from("Malta at Luxembourg"),
                home_team: String::from("Luxembourg"),
                away_team: String::from("Malta"),
                home_score: Some(1),
                away_score: Some(2),
                status_state: String::from("in"),
                status_detail: String::from("72'"),
                display_clock: String::from("72"),
                source_match_id: String::from("owls-1"),
                last_updated: String::from("2026-03-25T11:12:13Z"),
                stats: vec![OwlsLiveStat {
                    key: String::from("expectedGoals"),
                    label: String::from("xG"),
                    home_value: String::from("0.8"),
                    away_value: String::from("1.7"),
                }],
                incidents: vec![OwlsLiveIncident {
                    minute: Some(58),
                    incident_type: String::from("goal"),
                    team_side: String::from("away"),
                    player_name: String::from("Attard"),
                    detail: String::new(),
                }],
                player_ratings: vec![OwlsPlayerRating {
                    player_name: String::from("Teuma"),
                    team_side: String::from("away"),
                    rating: Some(8.4),
                }],
            }];
        }

        populate_snapshot_enrichment(&mut snapshot, &dashboard, None, None);

        assert!(snapshot.external_quotes.iter().any(|quote| {
            quote.provider == "owls"
                && quote.venue == "matchbook"
                && quote.price == Some(3.25)
                && quote.liquidity == Some(120.0)
        }));
        assert!(snapshot.external_quotes.iter().any(|quote| {
            quote.is_sharp && quote.venue == "pinnacle" && quote.price == Some(3.10)
        }));
        assert_eq!(snapshot.external_live_events.len(), 1);
        assert_eq!(snapshot.external_live_events[0].display_clock, "72");
        assert_eq!(snapshot.external_live_events[0].stats.len(), 1);
        assert_eq!(snapshot.open_positions[0].current_score, "2-1");
        assert_eq!(snapshot.open_positions[0].live_clock, "72");
    }

    #[test]
    fn refresh_snapshot_enrichment_uses_last_good_resources_when_stale() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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

        let mut snapshot = sample_snapshot("snapshot");
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("Arsenal v Everton"),
            event_status: String::new(),
            event_url: String::new(),
            contract: String::from("Arsenal"),
            market: String::from("Match Odds"),
            status: String::from("open"),
            market_status: String::from("live"),
            is_in_play: false,
            price: 2.8,
            stake: 10.0,
            liability: 18.0,
            current_value: 10.0,
            pnl_amount: 0.0,
            overall_pnl_known: true,
            current_back_odds: Some(2.4),
            current_implied_probability: Some(1.0 / 2.4),
            current_implied_percentage: Some(100.0 / 2.4),
            current_buy_odds: Some(2.42),
            current_buy_implied_probability: Some(1.0 / 2.42),
            current_sell_odds: Some(2.46),
            current_sell_implied_probability: Some(1.0 / 2.46),
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::new(),
            can_trade_out: true,
        }];
        app.replace_snapshot(snapshot);

        let mut dashboard = sample_owls_dashboard_with_quote("Arsenal v Everton", "Arsenal", 2.30);
        dashboard.sport = String::from("soccer");
        if let Some(endpoint) = dashboard
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == OwlsEndpointId::ScoresSport)
        {
            endpoint.status = String::from("ready");
            endpoint.live_scores = vec![OwlsLiveScoreEvent {
                sport: String::from("soccer"),
                event_id: String::from("evt-1"),
                name: String::from("Arsenal v Everton"),
                home_team: String::from("Arsenal"),
                away_team: String::from("Everton"),
                home_score: Some(1),
                away_score: Some(0),
                status_state: String::from("inplay"),
                status_detail: String::from("45'"),
                display_clock: String::from("45:00"),
                source_match_id: String::from("owls-1"),
                last_updated: String::from("2026-03-25T12:00:00Z"),
                stats: Vec::new(),
                incidents: Vec::new(),
                player_ratings: Vec::new(),
            }];
        }
        app.set_owls_dashboard_for_test(dashboard);
        app.set_matchbook_state_for_test(MatchbookAccountState {
            current_offers: vec![MatchbookOfferRow {
                event_name: String::from("Arsenal v Everton"),
                market_name: String::from("Match Odds"),
                selection_name: String::from("Arsenal"),
                side: String::from("lay"),
                status: String::from("open"),
                odds: Some(2.28),
                remaining_stake: Some(50.0),
                ..MatchbookOfferRow::default()
            }],
            ..MatchbookAccountState::default()
        });

        app.mark_owls_sync_in_flight_for_test(Instant::now() - Duration::from_secs(60));
        app.poll_owls_dashboard_for_test();
        app.mark_matchbook_sync_in_flight_for_test(Instant::now() - Duration::from_secs(60));
        app.poll_matchbook_account_for_test();

        assert_eq!(app.owls_resource_state.phase(), ResourcePhase::Stale);
        assert_eq!(app.matchbook_resource_state.phase(), ResourcePhase::Stale);
        assert!(app
            .snapshot
            .external_quotes
            .iter()
            .any(|quote| { quote.provider == "owls" && quote.price == Some(2.30) }));
        assert!(app
            .snapshot
            .external_quotes
            .iter()
            .any(|quote| { quote.provider == "matchbook_api" && quote.price == Some(2.28) }));
        assert_eq!(app.snapshot.external_live_events.len(), 1);
        assert_eq!(app.snapshot.open_positions[0].live_clock, "45:00");
    }

    #[test]
    fn accounts_section_seeds_selected_exchange_row_from_snapshot() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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

        let mut snapshot = sample_snapshot("snapshot");
        snapshot.venues = vec![
            VenueSummary {
                id: VenueId::Smarkets,
                label: String::from("smarkets"),
                status: VenueStatus::Connected,
                detail: String::from("watcher"),
                event_count: 2,
                market_count: 4,
            },
            VenueSummary {
                id: VenueId::Betway,
                label: String::from("betway"),
                status: VenueStatus::Connected,
                detail: String::from("live"),
                event_count: 3,
                market_count: 6,
            },
        ];
        snapshot.selected_venue = Some(VenueId::Betway);
        app.exchange_list_state.select(None);
        app.replace_snapshot(snapshot);

        app.set_trading_section(TradingSection::Accounts);

        assert_eq!(app.selected_exchange_row(), Some(1));
        assert_eq!(app.selected_venue(), Some(VenueId::Betway));
    }

    #[test]
    fn soccer_live_section_prefers_scores_endpoint() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::with_dependencies_and_storage(
            Box::new(StubExchangeProvider::default()),
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

        app.set_owls_dashboard_for_test(owls::dashboard_for_sport("soccer"));
        app.set_trading_section(TradingSection::Live);

        assert_eq!(
            app.selected_owls_endpoint_id(),
            Some(OwlsEndpointId::ScoresSport)
        );
    }

    #[test]
    fn matchbook_error_status_detection_matches_embedded_http_codes() {
        let rate_limit =
            color_eyre::eyre::eyre!("Matchbook session login failed with 429: slow down");
        let unauthorized = color_eyre::eyre::eyre!("Matchbook account failed with 401: expired");

        assert!(super::matchbook_error_has_status(&rate_limit, 429));
        assert!(!super::matchbook_error_has_status(&rate_limit, 401));
        assert!(super::matchbook_error_has_status(&unauthorized, 401));
    }

    #[test]
    fn replace_snapshot_alerts_when_watched_odds_move_sharply() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut initial = sample_snapshot("initial");
        initial.tracked_bets = Vec::new();
        let mut app = App::with_dependencies_and_storage(
            Box::new(RefreshingProvider {
                cached_refresh_count: Rc::new(RefCell::new(0)),
                live_refresh_count: Rc::new(RefCell::new(0)),
                load_snapshot: initial,
                cached_refresh_snapshot: sample_snapshot("cached"),
                live_refresh_snapshot: sample_snapshot("live"),
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
        app.alerts_config.desktop_notifications = false;
        app.alerts_config.sound_effects = false;
        app.alerts_config.watched_movement_threshold_pct = 5.0;

        assert_eq!(app.alerts_config.watched_movement_threshold_pct, 5.0);
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
            external_quotes: Vec::new(),
            external_live_events: Vec::new(),
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

    fn sample_trading_action_intent(mode: TradingActionMode) -> TradingActionIntent {
        TradingActionIntent {
            action_kind: TradingActionKind::PlaceBet,
            source: TradingActionSource::OddsMatcher,
            venue: VenueId::Matchbook,
            mode,
            side: TradingActionSide::Buy,
            request_id: String::from("req-1"),
            source_ref: String::from("row-1"),
            event_name: String::from("Chelsea vs Liverpool"),
            market_name: String::from("Match Odds"),
            selection_name: String::from("Chelsea"),
            stake: 10.0,
            expected_price: 2.2,
            event_url: Some(String::from("https://example.com/event")),
            deep_link_url: Some(String::from("https://example.com/deep")),
            betslip_event_id: Some(String::from("evt-1")),
            betslip_market_id: Some(String::from("mkt-1")),
            betslip_selection_id: Some(String::from("sel-1")),
            execution_policy: TradingExecutionPolicy::new(TradingTimeInForce::FillOrKill),
            risk_report: TradingRiskReport::default(),
            source_context: TradingActionSourceContext::default(),
            notes: Vec::new(),
        }
    }

    fn sample_owls_dashboard_with_quote(event: &str, selection: &str, price: f64) -> OwlsDashboard {
        let mut dashboard = OwlsDashboard::default();
        if let Some(endpoint) = dashboard
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == OwlsEndpointId::Realtime)
        {
            endpoint.status = String::from("ready");
            endpoint.preview = vec![OwlsPreviewRow {
                label: String::from(event),
                detail: String::from("pinnacle"),
                metric: format!("{selection} {price:.2}"),
            }];
            endpoint.quotes = vec![OwlsMarketQuote {
                book: String::from("pinnacle"),
                event: String::from(event),
                selection: String::from(selection),
                market_key: String::from("h2h"),
                decimal_price: Some(price),
                ..OwlsMarketQuote::default()
            }];
        }
        dashboard
    }
}
