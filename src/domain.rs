use serde::Deserializer;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VenueId {
    Smarkets,
    Bet365,
    Betfair,
    Betfred,
    Matchbook,
    Betdaq,
    Betway,
    Betuk,
}

impl VenueId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Smarkets => "smarkets",
            Self::Bet365 => "bet365",
            Self::Betfair => "betfair",
            Self::Betfred => "betfred",
            Self::Matchbook => "matchbook",
            Self::Betdaq => "betdaq",
            Self::Betway => "betway",
            Self::Betuk => "betuk",
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
    #[serde(default, deserialize_with = "null_string_as_default")]
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
    #[serde(default, deserialize_with = "null_string_as_default")]
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
    #[serde(default, deserialize_with = "null_string_as_default")]
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
    #[serde(
        default = "default_currency",
        deserialize_with = "null_string_or_default_currency"
    )]
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenPositionRow {
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub event: String,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub event_status: String,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub event_url: String,
    pub contract: String,
    pub market: String,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub status: String,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub market_status: String,
    #[serde(default)]
    pub is_in_play: bool,
    pub price: f64,
    pub stake: f64,
    pub liability: f64,
    pub current_value: f64,
    pub pnl_amount: f64,
    pub current_back_odds: Option<f64>,
    pub current_implied_probability: Option<f64>,
    pub current_implied_percentage: Option<f64>,
    pub current_buy_odds: Option<f64>,
    pub current_buy_implied_probability: Option<f64>,
    pub current_sell_odds: Option<f64>,
    pub current_sell_implied_probability: Option<f64>,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub current_score: String,
    pub current_score_home: Option<i64>,
    pub current_score_away: Option<i64>,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub live_clock: String,
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
pub struct ValueMetric {
    pub gbp: Option<f64>,
    pub pct: Option<f64>,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BetActivityRow {
    #[serde(default)]
    pub occurred_at: String,
    #[serde(default)]
    pub activity_type: String,
    pub amount_gbp: Option<f64>,
    pub balance_after_gbp: Option<f64>,
    #[serde(default)]
    pub source_file: String,
    #[serde(default)]
    pub raw_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackedLeg {
    pub venue: String,
    pub outcome: String,
    pub side: String,
    pub odds: f64,
    pub stake: f64,
    pub status: String,
    #[serde(default)]
    pub market: String,
    #[serde(default)]
    pub market_family: String,
    pub line: Option<f64>,
    pub liability: Option<f64>,
    pub commission_rate: Option<f64>,
    pub exchange: Option<String>,
    #[serde(default)]
    pub placed_at: String,
    #[serde(default)]
    pub settled_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackedBetRow {
    pub bet_id: String,
    pub group_id: String,
    pub event: String,
    pub market: String,
    pub selection: String,
    pub status: String,
    #[serde(default)]
    pub placed_at: String,
    #[serde(default)]
    pub settled_at: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub platform_kind: String,
    pub exchange: Option<String>,
    #[serde(default)]
    pub sport_key: String,
    #[serde(default)]
    pub sport_name: String,
    #[serde(default)]
    pub bet_type: String,
    #[serde(default)]
    pub market_family: String,
    pub selection_line: Option<f64>,
    #[serde(default = "default_currency")]
    pub currency: String,
    pub stake_gbp: Option<f64>,
    pub potential_returns_gbp: Option<f64>,
    pub payout_gbp: Option<f64>,
    pub realised_pnl_gbp: Option<f64>,
    pub back_price: Option<f64>,
    pub lay_price: Option<f64>,
    pub spread: Option<f64>,
    #[serde(default)]
    pub expected_ev: ValueMetric,
    #[serde(default)]
    pub realised_ev: ValueMetric,
    #[serde(default)]
    pub activities: Vec<BetActivityRow>,
    #[serde(default)]
    pub parse_confidence: String,
    #[serde(default)]
    pub notes: String,
    pub legs: Vec<TrackedLeg>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitPolicySummary {
    pub target_profit: f64,
    pub stop_loss: f64,
    pub hard_margin_call_profit_floor: Option<f64>,
    pub warn_only_default: bool,
}

impl Default for ExitPolicySummary {
    fn default() -> Self {
        Self {
            target_profit: 0.0,
            stop_loss: 0.0,
            hard_margin_call_profit_floor: None,
            warn_only_default: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExitRecommendation {
    pub bet_id: String,
    pub action: String,
    pub reason: String,
    pub worst_case_pnl: f64,
    pub cash_out_venue: Option<String>,
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
    pub runtime: Option<RuntimeSummary>,
    pub account_stats: Option<AccountStats>,
    pub open_positions: Vec<OpenPositionRow>,
    pub historical_positions: Vec<OpenPositionRow>,
    pub other_open_bets: Vec<OtherOpenBetRow>,
    pub decisions: Vec<DecisionSummary>,
    pub watch: Option<WatchSnapshot>,
    pub tracked_bets: Vec<TrackedBetRow>,
    pub exit_policy: ExitPolicySummary,
    pub exit_recommendations: Vec<ExitRecommendation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSummary {
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub updated_at: String,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub source: String,
    pub decision_count: usize,
    pub watcher_iteration: Option<usize>,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionSummary {
    pub contract: String,
    pub market: String,
    pub status: String,
    pub reason: String,
    pub current_pnl_amount: f64,
    pub current_back_odds: Option<f64>,
    pub profit_take_back_odds: f64,
    pub stop_loss_back_odds: f64,
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
    pub current_back_odds: Option<f64>,
    pub average_entry_lay_odds: f64,
    pub entry_implied_probability: f64,
    pub profit_take_back_odds: f64,
    pub profit_take_implied_probability: f64,
    pub stop_loss_back_odds: f64,
    pub stop_loss_implied_probability: f64,
}

fn default_currency() -> String {
    String::from("GBP")
}

fn null_string_as_default<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn null_string_or_default_currency<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_else(default_currency))
}
