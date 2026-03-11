use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VenueId {
    Smarkets,
    Betfair,
    Matchbook,
    Betdaq,
}

impl VenueId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Smarkets => "smarkets",
            Self::Betfair => "betfair",
            Self::Matchbook => "matchbook",
            Self::Betdaq => "betdaq",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VenueStatus {
    Connected,
    Ready,
    #[default]
    Planned,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerStatus {
    Ready,
    Busy,
    #[default]
    Idle,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSummary {
    pub name: String,
    pub status: WorkerStatus,
    pub detail: String,
}

impl Default for WorkerSummary {
    fn default() -> Self {
        Self {
            name: String::from("exchange-browser-worker"),
            status: WorkerStatus::Idle,
            detail: String::from("Worker not connected"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueSummary {
    pub id: VenueId,
    pub label: String,
    pub status: VenueStatus,
    pub detail: String,
    pub event_count: usize,
    pub market_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventCandidateSummary {
    pub id: String,
    pub label: String,
    pub competition: String,
    pub start_time: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketSummary {
    pub name: String,
    pub contract_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreflightSummary {
    pub venue: VenueId,
    pub event: String,
    pub market: String,
    pub contract: String,
    pub side: String,
    pub stake: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountStats {
    pub available_balance: f64,
    pub exposure: f64,
    pub unrealized_pnl: f64,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenPositionRow {
    pub contract: String,
    pub market: String,
    pub price: f64,
    pub stake: f64,
    pub liability: f64,
    pub current_value: f64,
    pub pnl_amount: f64,
    pub can_trade_out: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OtherOpenBetRow {
    pub label: String,
    pub market: String,
    pub side: String,
    pub odds: f64,
    pub stake: f64,
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExchangePanelSnapshot {
    pub worker: WorkerSummary,
    pub venues: Vec<VenueSummary>,
    pub selected_venue: Option<VenueId>,
    pub events: Vec<EventCandidateSummary>,
    pub markets: Vec<MarketSummary>,
    pub preflight: Option<PreflightSummary>,
    pub status_line: String,
    pub account_stats: Option<AccountStats>,
    pub open_positions: Vec<OpenPositionRow>,
    pub other_open_bets: Vec<OtherOpenBetRow>,
    pub watch: Option<WatchSnapshot>,
}

impl ExchangePanelSnapshot {
    pub fn empty() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WatchSnapshot {
    pub position_count: usize,
    pub watch_count: usize,
    pub commission_rate: f64,
    pub target_profit: f64,
    pub stop_loss: f64,
    pub watches: Vec<WatchRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WatchRow {
    pub contract: String,
    pub market: String,
    pub position_count: usize,
    pub can_trade_out: bool,
    pub total_stake: f64,
    pub total_liability: f64,
    pub current_pnl_amount: f64,
    pub average_entry_lay_odds: f64,
    pub entry_implied_probability: f64,
    pub profit_take_back_odds: f64,
    pub profit_take_implied_probability: f64,
    pub stop_loss_back_odds: f64,
    pub stop_loss_implied_probability: f64,
}
