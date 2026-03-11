use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::{ExchangePanelSnapshot, VenueId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerConfig {
    pub positions_payload_path: Option<PathBuf>,
    pub run_dir: Option<PathBuf>,
    pub account_payload_path: Option<PathBuf>,
    pub open_bets_payload_path: Option<PathBuf>,
    pub agent_browser_session: Option<String>,
    pub commission_rate: f64,
    pub target_profit: f64,
    pub stop_loss: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerRequestEnvelope {
    LoadDashboard { config: WorkerConfig },
    SelectVenue { venue: VenueId },
    Refresh,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerResponseEnvelope {
    pub snapshot: ExchangePanelSnapshot,
}
