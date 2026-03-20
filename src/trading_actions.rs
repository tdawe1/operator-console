use color_eyre::eyre::{eyre, Result};
use serde::{Deserialize, Serialize};

use crate::domain::{ExchangePanelSnapshot, VenueId, VenueStatus, WorkerStatus};

const FILL_OR_KILL_TIMEOUT_MS: u64 = 1_500;
const PRICE_GUARD_EXACT: f64 = 0.0;
const SPREAD_WARN_THRESHOLD: f64 = 0.08;
const SPREAD_BLOCK_THRESHOLD: f64 = 0.18;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingActionKind {
    PlaceBet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingActionSource {
    OddsMatcher,
    HorseMatcher,
    Positions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingActionMode {
    Review,
    Confirm,
}

impl TradingActionMode {
    pub const ALL: [Self; 2] = [Self::Review, Self::Confirm];

    pub fn label(self) -> &'static str {
        match self {
            Self::Review => "Review",
            Self::Confirm => "Confirm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingActionSide {
    Buy,
    Sell,
}

impl TradingActionSide {
    pub const ALL: [Self; 2] = [Self::Buy, Self::Sell];

    pub fn label(self) -> &'static str {
        match self {
            Self::Buy => "Buy",
            Self::Sell => "Sell",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingTimeInForce {
    GoodTilCancel,
    FillOrKill,
}

impl TradingTimeInForce {
    pub const ALL: [Self; 2] = [Self::GoodTilCancel, Self::FillOrKill];

    pub fn label(self) -> &'static str {
        match self {
            Self::GoodTilCancel => "GTC",
            Self::FillOrKill => "Fill/Kill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingRiskSeverity {
    Info,
    Warning,
    Block,
}

impl TradingRiskSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warn",
            Self::Block => "block",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingRiskScope {
    Review,
    Submit,
}

impl TradingRiskScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::Submit => "submit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradingRiskCheck {
    pub code: String,
    pub severity: TradingRiskSeverity,
    pub scope: TradingRiskScope,
    pub summary: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradingRiskReport {
    pub summary: String,
    #[serde(default)]
    pub checks: Vec<TradingRiskCheck>,
    pub warning_count: usize,
    pub blocking_review_count: usize,
    pub blocking_submit_count: usize,
    pub reduce_only: bool,
}

impl Default for TradingRiskReport {
    fn default() -> Self {
        Self {
            summary: String::from("No risk assessment is available."),
            checks: Vec::new(),
            warning_count: 0,
            blocking_review_count: 0,
            blocking_submit_count: 0,
            reduce_only: false,
        }
    }
}

impl TradingRiskReport {
    pub fn allows_mode(&self, mode: TradingActionMode) -> bool {
        match mode {
            TradingActionMode::Review => self.blocking_review_count == 0,
            TradingActionMode::Confirm => self.blocking_submit_count == 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradingExecutionPolicy {
    pub time_in_force: TradingTimeInForce,
    pub cancel_unmatched_after_ms: u64,
    pub require_full_fill: bool,
    pub max_price_drift: f64,
}

impl TradingExecutionPolicy {
    pub fn new(time_in_force: TradingTimeInForce) -> Self {
        match time_in_force {
            TradingTimeInForce::GoodTilCancel => Self {
                time_in_force,
                cancel_unmatched_after_ms: 0,
                require_full_fill: false,
                max_price_drift: PRICE_GUARD_EXACT,
            },
            TradingTimeInForce::FillOrKill => Self {
                time_in_force,
                cancel_unmatched_after_ms: FILL_OR_KILL_TIMEOUT_MS,
                require_full_fill: true,
                max_price_drift: PRICE_GUARD_EXACT,
            },
        }
    }
}

impl Default for TradingExecutionPolicy {
    fn default() -> Self {
        Self::new(TradingTimeInForce::GoodTilCancel)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TradingActionSourceContext {
    #[serde(default)]
    pub is_in_play: bool,
    #[serde(default)]
    pub event_status: String,
    #[serde(default)]
    pub market_status: String,
    #[serde(default)]
    pub live_clock: String,
    #[serde(default)]
    pub can_trade_out: bool,
    #[serde(default)]
    pub current_pnl_amount: Option<f64>,
    #[serde(default)]
    pub baseline_stake: Option<f64>,
    #[serde(default)]
    pub baseline_liability: Option<f64>,
    #[serde(default)]
    pub baseline_price: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradingActionIntent {
    pub action_kind: TradingActionKind,
    pub source: TradingActionSource,
    pub venue: VenueId,
    pub mode: TradingActionMode,
    pub side: TradingActionSide,
    pub request_id: String,
    pub source_ref: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub stake: f64,
    pub expected_price: f64,
    pub event_url: Option<String>,
    pub deep_link_url: Option<String>,
    pub betslip_market_id: Option<String>,
    pub betslip_selection_id: Option<String>,
    pub execution_policy: TradingExecutionPolicy,
    pub risk_report: TradingRiskReport,
    #[serde(default)]
    pub source_context: TradingActionSourceContext,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TradingActionSeed {
    pub source: TradingActionSource,
    pub venue: VenueId,
    pub source_ref: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub event_url: Option<String>,
    pub deep_link_url: Option<String>,
    pub betslip_market_id: Option<String>,
    pub betslip_selection_id: Option<String>,
    pub buy_price: Option<f64>,
    pub sell_price: Option<f64>,
    pub default_side: TradingActionSide,
    pub default_stake: Option<f64>,
    pub source_context: TradingActionSourceContext,
    pub notes: Vec<String>,
}

impl TradingActionSeed {
    pub fn supports_side(&self, side: TradingActionSide) -> bool {
        self.price_for_side(side).is_some()
    }

    pub fn price_for_side(&self, side: TradingActionSide) -> Option<f64> {
        match side {
            TradingActionSide::Buy => self.buy_price,
            TradingActionSide::Sell => self.sell_price,
        }
    }

    pub fn default_stake_label(&self) -> String {
        self.default_stake
            .map(format_decimal)
            .unwrap_or_else(|| String::from("10.00"))
    }

    pub fn default_time_in_force(&self) -> TradingTimeInForce {
        if self.source_context.is_in_play {
            TradingTimeInForce::FillOrKill
        } else {
            TradingTimeInForce::GoodTilCancel
        }
    }

    pub fn evaluate(
        &self,
        snapshot: &ExchangePanelSnapshot,
        side: TradingActionSide,
        mode: TradingActionMode,
        stake: f64,
        time_in_force: TradingTimeInForce,
    ) -> Result<TradingActionIntent> {
        if stake <= 0.0 {
            return Err(eyre!("Stake must be greater than zero."));
        }
        let expected_price = self.price_for_side(side).ok_or_else(|| {
            eyre!(
                "{} does not expose a {} price.",
                self.venue.as_str(),
                side.label()
            )
        })?;
        if expected_price <= 1.0 {
            return Err(eyre!("The selected quote is not a valid decimal price."));
        }
        if self.event_url.as_deref().unwrap_or_default().is_empty()
            && self.deep_link_url.as_deref().unwrap_or_default().is_empty()
        {
            return Err(eyre!(
                "This action requires an event URL or deep link before execution."
            ));
        }

        let execution_policy = TradingExecutionPolicy::new(time_in_force);
        let risk_report = build_risk_report(
            snapshot,
            self,
            side,
            mode,
            stake,
            expected_price,
            &execution_policy,
        );

        Ok(TradingActionIntent {
            action_kind: TradingActionKind::PlaceBet,
            source: self.source,
            venue: self.venue,
            mode,
            side,
            request_id: String::new(),
            source_ref: self.source_ref.clone(),
            event_name: self.event_name.clone(),
            market_name: self.market_name.clone(),
            selection_name: self.selection_name.clone(),
            stake,
            expected_price,
            event_url: self.event_url.clone(),
            deep_link_url: self.deep_link_url.clone(),
            betslip_market_id: self.betslip_market_id.clone(),
            betslip_selection_id: self.betslip_selection_id.clone(),
            execution_policy,
            risk_report,
            source_context: self.source_context.clone(),
            notes: self.notes.clone(),
        })
    }

    pub fn build_intent(
        &self,
        snapshot: &ExchangePanelSnapshot,
        request_id: String,
        side: TradingActionSide,
        mode: TradingActionMode,
        stake: f64,
        time_in_force: TradingTimeInForce,
    ) -> Result<TradingActionIntent> {
        let mut intent = self.evaluate(snapshot, side, mode, stake, time_in_force)?;
        if !intent.risk_report.allows_mode(mode) {
            return Err(eyre!(intent.risk_report.summary.clone()));
        }
        intent.request_id = request_id;
        Ok(intent)
    }
}

pub fn format_decimal(value: f64) -> String {
    format!("{value:.2}")
}

fn build_risk_report(
    snapshot: &ExchangePanelSnapshot,
    seed: &TradingActionSeed,
    side: TradingActionSide,
    mode: TradingActionMode,
    stake: f64,
    expected_price: f64,
    execution_policy: &TradingExecutionPolicy,
) -> TradingRiskReport {
    let mut checks = Vec::new();
    let reduce_only =
        seed.source == TradingActionSource::Positions && side == TradingActionSide::Buy;
    let required_balance = required_balance(side, stake, expected_price);

    if snapshot.worker.status == WorkerStatus::Error {
        checks.push(block(
            "worker_error",
            TradingRiskScope::Review,
            "Worker is not healthy.",
            format!(
                "The worker reports an error and should not be trusted for execution: {}",
                snapshot.worker.detail
            ),
        ));
    } else if snapshot.worker.status == WorkerStatus::Busy {
        checks.push(warn(
            "worker_busy",
            TradingRiskScope::Submit,
            "Worker is already busy.",
            "Submitting another latency-sensitive action through a busy worker increases drift risk.",
        ));
    }

    if let Some(runtime) = snapshot.runtime.as_ref() {
        if runtime.stale {
            checks.push(block(
                "stale_runtime",
                TradingRiskScope::Submit,
                "Live runtime context is stale.",
                format!(
                    "The latest watcher snapshot is stale (updated_at={}, source={}).",
                    runtime.updated_at, runtime.source
                ),
            ));
        }
    } else {
        checks.push(warn(
            "missing_runtime",
            TradingRiskScope::Submit,
            "Runtime freshness is unknown.",
            "No runtime summary is present, so the action cannot prove the quotes are fresh.",
        ));
    }

    if let Some(account_stats) = snapshot.account_stats.as_ref() {
        if required_balance > account_stats.available_balance + 0.01 {
            checks.push(block(
                "insufficient_balance",
                TradingRiskScope::Submit,
                "Available balance is too low.",
                format!(
                    "Required {:.2} {} but only {:.2} {} is available.",
                    required_balance,
                    account_stats.currency,
                    account_stats.available_balance,
                    account_stats.currency
                ),
            ));
        } else {
            checks.push(info(
                "balance_headroom",
                TradingRiskScope::Submit,
                "Balance headroom is available.",
                format!(
                    "Required {:.2} {} against {:.2} {} available.",
                    required_balance,
                    account_stats.currency,
                    account_stats.available_balance,
                    account_stats.currency
                ),
            ));
        }
    } else {
        checks.push(warn(
            "missing_account_state",
            TradingRiskScope::Submit,
            "Account balance is unknown.",
            "No account snapshot is present, so bankroll checks cannot be completed before submission.",
        ));
    }

    let normalized_market_status = seed.source_context.market_status.to_ascii_lowercase();
    if normalized_market_status.contains("suspend")
        || normalized_market_status.contains("closed")
        || normalized_market_status.contains("settled")
    {
        checks.push(block(
            "market_not_tradable",
            TradingRiskScope::Submit,
            "Market is not tradable.",
            format!(
                "The selected market reports status {:?}.",
                seed.source_context.market_status
            ),
        ));
    } else if !normalized_market_status.is_empty()
        && !normalized_market_status.contains("tradable")
        && !normalized_market_status.contains("open")
    {
        checks.push(warn(
            "market_status_unclear",
            TradingRiskScope::Submit,
            "Market status is ambiguous.",
            format!(
                "The selected market reports status {:?} instead of a clear tradable state.",
                seed.source_context.market_status
            ),
        ));
    }

    if execution_policy.time_in_force == TradingTimeInForce::FillOrKill
        && !seed.source_context.is_in_play
    {
        checks.push(warn(
            "fok_out_of_play",
            TradingRiskScope::Submit,
            "Fill-or-kill is armed outside live play.",
            "Synthetic fill-or-kill is usually for live scalping; outside live play it may add avoidable churn.",
        ));
    }

    if let (Some(buy), Some(sell)) = (seed.buy_price, seed.sell_price) {
        let midpoint = (buy + sell) / 2.0;
        let spread_pct = if midpoint > 0.0 {
            (sell - buy).abs() / midpoint
        } else {
            0.0
        };
        if spread_pct >= SPREAD_BLOCK_THRESHOLD
            && execution_policy.time_in_force == TradingTimeInForce::FillOrKill
        {
            checks.push(block(
                "wide_spread_block",
                TradingRiskScope::Submit,
                "Spread is too wide for synthetic fill-or-kill.",
                format!(
                    "The buy/sell spread is {:.1}% (buy {}, sell {}).",
                    spread_pct * 100.0,
                    format_decimal(buy),
                    format_decimal(sell)
                ),
            ));
        } else if spread_pct >= SPREAD_WARN_THRESHOLD {
            checks.push(warn(
                "wide_spread_warn",
                TradingRiskScope::Submit,
                "Spread is wide.",
                format!(
                    "The buy/sell spread is {:.1}% (buy {}, sell {}).",
                    spread_pct * 100.0,
                    format_decimal(buy),
                    format_decimal(sell)
                ),
            ));
        }
    } else {
        checks.push(warn(
            "one_sided_quote",
            TradingRiskScope::Submit,
            "Only one side of the market is visible.",
            "The action is being priced from a one-sided quote, so spread and cross-side sanity checks are limited.",
        ));
    }

    if seed.source == TradingActionSource::Positions {
        if reduce_only {
            checks.push(info(
                "reduce_only",
                TradingRiskScope::Submit,
                "Order reduces an existing position.",
                "Buying from the positions board is treated as exposure reduction, not fresh directional exposure.",
            ));
        } else {
            checks.push(warn(
                "adds_to_position",
                TradingRiskScope::Submit,
                "Order adds exposure to an existing position.",
                "Selling again from the positions board increases live exchange exposure.",
            ));
        }
    }

    if snapshot
        .selected_venue
        .is_some_and(|selected| selected != seed.venue)
    {
        checks.push(warn(
            "venue_mismatch",
            TradingRiskScope::Review,
            "The UI is focused on a different venue.",
            format!(
                "The selected venue is {:?}, while this action targets {}.",
                snapshot.selected_venue,
                seed.venue.as_str()
            ),
        ));
    }

    if let Some(summary) = snapshot.venues.iter().find(|venue| venue.id == seed.venue) {
        if summary.status == VenueStatus::Error {
            checks.push(block(
                "venue_error",
                TradingRiskScope::Review,
                "Venue status is unhealthy.",
                format!("{} reports an error: {}", summary.label, summary.detail),
            ));
        }
    }

    if mode == TradingActionMode::Review {
        checks.retain(|check| {
            check.severity != TradingRiskSeverity::Block || check.scope == TradingRiskScope::Review
        });
    }

    summarize_report(checks, reduce_only)
}

fn summarize_report(checks: Vec<TradingRiskCheck>, reduce_only: bool) -> TradingRiskReport {
    let warning_count = checks
        .iter()
        .filter(|check| check.severity == TradingRiskSeverity::Warning)
        .count();
    let blocking_review_count = checks
        .iter()
        .filter(|check| {
            check.severity == TradingRiskSeverity::Block && check.scope == TradingRiskScope::Review
        })
        .count();
    let blocking_submit_count = checks
        .iter()
        .filter(|check| check.severity == TradingRiskSeverity::Block)
        .count();

    let summary = if blocking_submit_count > 0 {
        format!(
            "Submit blocked by {blocking_submit_count} risk check(s); {warning_count} warning(s) remain."
        )
    } else if blocking_review_count > 0 {
        format!(
            "Review blocked by {blocking_review_count} risk check(s); {warning_count} warning(s) remain."
        )
    } else if warning_count > 0 {
        format!("Ready with {warning_count} warning(s).")
    } else if reduce_only {
        String::from("Ready; action is classified as reduce-only.")
    } else {
        String::from("Ready; no blocking risk checks are active.")
    };

    TradingRiskReport {
        summary,
        checks,
        warning_count,
        blocking_review_count,
        blocking_submit_count,
        reduce_only,
    }
}

fn required_balance(side: TradingActionSide, stake: f64, expected_price: f64) -> f64 {
    match side {
        TradingActionSide::Buy => stake,
        TradingActionSide::Sell => stake * (expected_price - 1.0),
    }
}

fn info(
    code: &str,
    scope: TradingRiskScope,
    summary: &str,
    detail: impl Into<String>,
) -> TradingRiskCheck {
    TradingRiskCheck {
        code: String::from(code),
        severity: TradingRiskSeverity::Info,
        scope,
        summary: String::from(summary),
        detail: detail.into(),
    }
}

fn warn(
    code: &str,
    scope: TradingRiskScope,
    summary: &str,
    detail: impl Into<String>,
) -> TradingRiskCheck {
    TradingRiskCheck {
        code: String::from(code),
        severity: TradingRiskSeverity::Warning,
        scope,
        summary: String::from(summary),
        detail: detail.into(),
    }
}

fn block(
    code: &str,
    scope: TradingRiskScope,
    summary: &str,
    detail: impl Into<String>,
) -> TradingRiskCheck {
    TradingRiskCheck {
        code: String::from(code),
        severity: TradingRiskSeverity::Block,
        scope,
        summary: String::from(summary),
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        AccountStats, ExchangePanelSnapshot, ExitPolicySummary, OpenPositionRow, VenueSummary,
        WorkerSummary,
    };

    fn sample_snapshot() -> ExchangePanelSnapshot {
        ExchangePanelSnapshot {
            worker: WorkerSummary {
                name: String::from("bet-recorder"),
                status: WorkerStatus::Ready,
                detail: String::from("healthy"),
            },
            venues: vec![VenueSummary {
                id: VenueId::Smarkets,
                label: String::from("Smarkets"),
                status: VenueStatus::Ready,
                detail: String::from("connected"),
                event_count: 1,
                market_count: 1,
            }],
            selected_venue: Some(VenueId::Smarkets),
            status_line: String::from("ready"),
            runtime: Some(crate::domain::RuntimeSummary {
                updated_at: String::from("2026-03-19T10:00:00Z"),
                source: String::from("watcher-state"),
                refresh_kind: String::from("live_capture"),
                worker_reconnect_count: 0,
                decision_count: 1,
                watcher_iteration: Some(4),
                stale: false,
            }),
            account_stats: Some(AccountStats {
                available_balance: 250.0,
                exposure: 20.0,
                unrealized_pnl: 4.0,
                cumulative_pnl: None,
                cumulative_pnl_label: String::new(),
                currency: String::from("GBP"),
            }),
            open_positions: vec![OpenPositionRow {
                event: String::from("Arsenal v Everton"),
                event_status: String::from("27'"),
                event_url: String::from("https://smarkets.com/event/arsenal-everton"),
                contract: String::from("Draw"),
                market: String::from("Full-time result"),
                status: String::from("Order filled"),
                market_status: String::from("tradable"),
                is_in_play: true,
                price: 3.35,
                stake: 10.0,
                liability: 23.5,
                current_value: 8.0,
                pnl_amount: 1.2,
                current_back_odds: Some(5.0),
                current_implied_probability: Some(0.2),
                current_implied_percentage: Some(20.0),
                current_buy_odds: Some(5.0),
                current_buy_implied_probability: Some(0.2),
                current_sell_odds: Some(5.2),
                current_sell_implied_probability: Some(1.0 / 5.2),
                current_score: String::new(),
                current_score_home: None,
                current_score_away: None,
                live_clock: String::from("27'"),
                can_trade_out: true,
            }],
            historical_positions: Vec::new(),
            exit_policy: ExitPolicySummary::default(),
            ..ExchangePanelSnapshot::default()
        }
    }

    fn sample_seed() -> TradingActionSeed {
        TradingActionSeed {
            source: TradingActionSource::Positions,
            venue: VenueId::Smarkets,
            source_ref: String::from("bet-001"),
            event_name: String::from("Arsenal v Everton"),
            market_name: String::from("Full-time result"),
            selection_name: String::from("Draw"),
            event_url: Some(String::from("https://smarkets.com/event/arsenal-everton")),
            deep_link_url: None,
            betslip_market_id: None,
            betslip_selection_id: None,
            buy_price: Some(5.0),
            sell_price: Some(5.2),
            default_side: TradingActionSide::Buy,
            default_stake: Some(10.0),
            source_context: TradingActionSourceContext {
                is_in_play: true,
                event_status: String::from("27'"),
                market_status: String::from("tradable"),
                live_clock: String::from("27'"),
                can_trade_out: true,
                current_pnl_amount: Some(1.2),
                baseline_stake: Some(10.0),
                baseline_liability: Some(23.5),
                baseline_price: Some(3.35),
            },
            notes: vec![String::from("positions")],
        }
    }

    #[test]
    fn in_play_seed_defaults_to_fill_or_kill() {
        assert_eq!(
            sample_seed().default_time_in_force(),
            TradingTimeInForce::FillOrKill
        );
    }

    #[test]
    fn stale_runtime_blocks_submit() {
        let mut snapshot = sample_snapshot();
        snapshot.runtime.as_mut().expect("runtime").stale = true;

        let intent = sample_seed()
            .evaluate(
                &snapshot,
                TradingActionSide::Buy,
                TradingActionMode::Confirm,
                10.0,
                TradingTimeInForce::FillOrKill,
            )
            .expect("intent should evaluate");

        assert!(!intent.risk_report.allows_mode(TradingActionMode::Confirm));
        assert!(intent.risk_report.summary.contains("Submit blocked"));
    }

    #[test]
    fn insufficient_balance_blocks_sell_submit() {
        let mut snapshot = sample_snapshot();
        snapshot
            .account_stats
            .as_mut()
            .expect("stats")
            .available_balance = 5.0;

        let intent = sample_seed()
            .evaluate(
                &snapshot,
                TradingActionSide::Sell,
                TradingActionMode::Confirm,
                10.0,
                TradingTimeInForce::FillOrKill,
            )
            .expect("intent should evaluate");

        assert!(!intent.risk_report.allows_mode(TradingActionMode::Confirm));
        assert!(intent
            .risk_report
            .checks
            .iter()
            .any(|check| check.code == "insufficient_balance"));
    }
}
