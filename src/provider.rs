use std::path::PathBuf;

use crate::domain::WatchSnapshot;

#[derive(Debug, Clone, PartialEq)]
pub struct WatchRequest {
    pub payload_path: PathBuf,
    pub commission_rate: f64,
    pub target_profit: f64,
    pub stop_loss: f64,
}

pub trait WatchProvider {
    fn load_watch_snapshot(&mut self, request: &WatchRequest) -> color_eyre::Result<WatchSnapshot>;
}
