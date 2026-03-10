use serde::{Deserialize, Serialize};

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
