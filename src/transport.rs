use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::{ExchangePanelSnapshot, VenueId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerConfig {
    pub positions_payload_path: Option<PathBuf>,
    pub run_dir: Option<PathBuf>,
    pub account_payload_path: Option<PathBuf>,
    pub open_bets_payload_path: Option<PathBuf>,
    pub companion_legs_path: Option<PathBuf>,
    pub agent_browser_session: Option<String>,
    pub commission_rate: f64,
    pub target_profit: f64,
    pub stop_loss: f64,
    pub hard_margin_call_profit_floor: Option<f64>,
    pub warn_only_default: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerRequestEnvelope {
    LoadDashboard { config: WorkerConfig },
    SelectVenue { venue: VenueId },
    Refresh,
    CashOutTrackedBet { bet_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerResponseEnvelope {
    pub snapshot: ExchangePanelSnapshot,
    #[serde(default)]
    pub request_error: Option<String>,
}
