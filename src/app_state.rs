use color_eyre::Result;

use crate::calculator::Input as CalculatorInput;
use crate::trading_actions::{
    TradingActionMode, TradingActionSeed, TradingActionSide, TradingTimeInForce,
};

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
    Positions,
    Markets,
    Live,
    Props,
    Intel,
    Matcher,
    Stats,
    Alerts,
    Calculator,
    Recorder,
}

impl TradingSection {
    pub const ALL: [Self; 10] = [
        Self::Positions,
        Self::Markets,
        Self::Live,
        Self::Props,
        Self::Intel,
        Self::Matcher,
        Self::Stats,
        Self::Alerts,
        Self::Calculator,
        Self::Recorder,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Positions => "Positions",
            Self::Markets => "Markets",
            Self::Live => "Live",
            Self::Props => "Props",
            Self::Intel => "Intel",
            Self::Matcher => "Matcher",
            Self::Stats => "Stats",
            Self::Alerts => "Alerts",
            Self::Calculator => "Calculator",
            Self::Recorder => "Recorder",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelView {
    Markets,
    Arbitrages,
    PlusEv,
    Event,
    Drops,
    Value,
}

impl IntelView {
    pub const ALL: [Self; 6] = [
        Self::Markets,
        Self::Arbitrages,
        Self::PlusEv,
        Self::Event,
        Self::Drops,
        Self::Value,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Markets => "Markets",
            Self::Arbitrages => "Arbitrages",
            Self::PlusEv => "Plus EV",
            Self::Event => "Event",
            Self::Drops => "Drops",
            Self::Value => "Value",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelSource {
    OddsEntry,
    FairOdds,
}

impl IntelSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::OddsEntry => "OddsEntry",
            Self::FairOdds => "FairOdds",
        }
    }
}

#[derive(Debug, Clone)]
pub struct IntelSourceStatus {
    pub source: IntelSource,
    pub health: String,
    pub freshness: String,
    pub transport: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct IntelRow {
    pub id: String,
    pub source: IntelSource,
    pub event: String,
    pub competition: String,
    pub market: String,
    pub selection: String,
    pub bookmaker: String,
    pub exchange: String,
    pub back_odds: f64,
    pub lay_odds: Option<f64>,
    pub fair_odds: Option<f64>,
    pub edge_pct: Option<f64>,
    pub arb_pct: Option<f64>,
    pub liquidity: Option<f64>,
    pub status: String,
    pub updated_at: String,
    pub route: String,
    pub deep_link_url: String,
    pub note: String,
}

impl IntelRow {
    pub fn can_seed_calculator(&self) -> bool {
        self.lay_odds.is_some()
    }

    pub fn can_open_action(&self) -> bool {
        self.lay_odds.is_some()
            && (!self.route.trim().is_empty() || !self.deep_link_url.trim().is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatcherView {
    Odds,
    Horse,
    Acca,
}

impl MatcherView {
    pub const ALL: [Self; 3] = [Self::Odds, Self::Horse, Self::Acca];

    pub fn label(self) -> &'static str {
        match self {
            Self::Odds => "Odds",
            Self::Horse => "Horse",
            Self::Acca => "Acca",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculatorTool {
    Basic,
    Arb,
    EachWay,
    Acca,
    Ev,
    ExtraPlace,
}

impl CalculatorTool {
    pub const ALL: [Self; 6] = [
        Self::Basic,
        Self::Arb,
        Self::EachWay,
        Self::Acca,
        Self::Ev,
        Self::ExtraPlace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Arb => "Arb",
            Self::EachWay => "EW",
            Self::Acca => "Acca",
            Self::Ev => "EV",
            Self::ExtraPlace => "XPlace",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionsFocus {
    Active,
    Historical,
}

impl PositionsFocus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Historical => "Historical",
        }
    }
}

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

    pub fn display_value(self, state: &CalculatorState) -> String {
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
    pub editing: bool,
    pub buffer: String,
    pub replace_on_input: bool,
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
    pub fn selected_field(&self) -> CalculatorField {
        self.selected_field
    }

    pub fn select_next_field(&mut self) {
        self.selected_field = next_from(self.selected_field, &CalculatorField::ALL);
    }

    pub fn select_previous_field(&mut self) {
        self.selected_field = previous_from(self.selected_field, &CalculatorField::ALL);
    }
}

#[derive(Debug, Clone)]
#[derive(Default)]
pub struct CalculatorState {
    pub input: CalculatorInput,
    pub editor: CalculatorEditorState,
    pub source: Option<CalculatorSourceContext>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradingActionField {
    Mode,
    Side,
    TimeInForce,
    Stake,
    Execute,
}

impl TradingActionField {
    pub const ALL: [Self; 5] = [
        Self::Mode,
        Self::Side,
        Self::TimeInForce,
        Self::Stake,
        Self::Execute,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mode => "Mode",
            Self::Side => "Side",
            Self::TimeInForce => "Order",
            Self::Stake => "Stake",
            Self::Execute => "Execute",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TradingActionOverlayState {
    pub seed: TradingActionSeed,
    pub selected_field: TradingActionField,
    pub mode: TradingActionMode,
    pub side: TradingActionSide,
    pub time_in_force: TradingTimeInForce,
    pub risk_report: crate::trading_actions::TradingRiskReport,
    pub editing: bool,
    pub buffer: String,
    pub replace_on_input: bool,
}

impl TradingActionOverlayState {
    pub fn new(
        seed: TradingActionSeed,
        risk_report: crate::trading_actions::TradingRiskReport,
    ) -> Self {
        Self {
            mode: TradingActionMode::Review,
            side: seed.default_side,
            time_in_force: seed.default_time_in_force(),
            risk_report,
            buffer: seed.default_stake_label(),
            seed,
            selected_field: TradingActionField::Stake,
            editing: false,
            replace_on_input: true,
        }
    }

    pub fn selected_price(&self) -> Option<f64> {
        self.seed.price_for_side(self.side)
    }

    pub fn parsed_stake(&self) -> Result<f64> {
        let parsed = self
            .buffer
            .trim()
            .parse::<f64>()
            .map_err(|_| color_eyre::eyre::eyre!("Stake must be numeric."))?;
        if parsed <= 0.0 {
            return Err(color_eyre::eyre::eyre!("Stake must be greater than zero."));
        }
        Ok(parsed)
    }

    pub fn selected_field(&self) -> TradingActionField {
        self.selected_field
    }

    pub fn select_next_field(&mut self) {
        self.selected_field = next_from(self.selected_field, &TradingActionField::ALL);
    }

    pub fn select_previous_field(&mut self) {
        self.selected_field = previous_from(self.selected_field, &TradingActionField::ALL);
    }

    pub fn can_cycle_side(&self) -> bool {
        self.seed.buy_price.is_some() && self.seed.sell_price.is_some()
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
