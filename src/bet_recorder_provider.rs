use std::path::PathBuf;
use std::process::Command;

use color_eyre::eyre::{eyre, Context, Result};

use crate::domain::{
    EventCandidateSummary, ExchangePanelSnapshot, MarketSummary, VenueId, VenueStatus,
    VenueSummary, WatchSnapshot, WorkerStatus, WorkerSummary,
};
use crate::provider::{ExchangeProvider, ProviderRequest, WatchProvider, WatchRequest};

#[derive(Debug, Clone)]
enum BetRecorderCommand {
    Direct {
        executable: PathBuf,
    },
    PythonModule {
        python_executable: PathBuf,
        bet_recorder_root: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub struct BetRecorderProvider {
    command: BetRecorderCommand,
}

impl BetRecorderProvider {
    pub fn new(python_executable: PathBuf, bet_recorder_root: PathBuf) -> Self {
        Self {
            command: BetRecorderCommand::PythonModule {
                python_executable,
                bet_recorder_root,
            },
        }
    }

    pub fn new_command(executable: PathBuf) -> Self {
        Self {
            command: BetRecorderCommand::Direct { executable },
        }
    }
}

impl WatchProvider for BetRecorderProvider {
    fn load_watch_snapshot(&mut self, request: &WatchRequest) -> Result<WatchSnapshot> {
        let positions_payload_path = request
            .positions_payload_path
            .as_ref()
            .ok_or_else(|| eyre!("bet-recorder watch provider requires positions_payload_path"))?;
        let mut command = self.base_command();
        let output = command
            .arg("watch-open-positions")
            .arg("--payload-path")
            .arg(positions_payload_path)
            .arg("--commission-rate")
            .arg(request.commission_rate.to_string())
            .arg("--target-profit")
            .arg(request.target_profit.to_string())
            .arg("--stop-loss")
            .arg(request.stop_loss.to_string())
            .output()
            .wrap_err("failed to execute bet-recorder watcher subprocess")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(eyre!(
                "bet-recorder watcher subprocess failed: {}",
                stderr.trim()
            ));
        }

        serde_json::from_slice::<WatchSnapshot>(&output.stdout)
            .wrap_err("failed to decode bet-recorder watcher snapshot")
    }
}

impl BetRecorderProvider {
    fn base_command(&self) -> Command {
        match &self.command {
            BetRecorderCommand::Direct { executable } => Command::new(executable),
            BetRecorderCommand::PythonModule {
                python_executable,
                bet_recorder_root,
            } => {
                let mut command = Command::new(python_executable);
                command
                    .current_dir(bet_recorder_root)
                    .env("PYTHONPATH", bet_recorder_root.join("src"))
                    .arg("-m")
                    .arg("bet_recorder");
                command
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct BetRecorderExchangeProvider {
    watch_provider: BetRecorderProvider,
    watch_request: WatchRequest,
}

impl BetRecorderExchangeProvider {
    pub fn new(
        python_executable: PathBuf,
        bet_recorder_root: PathBuf,
        watch_request: WatchRequest,
    ) -> Self {
        Self {
            watch_provider: BetRecorderProvider::new(python_executable, bet_recorder_root),
            watch_request,
        }
    }

    fn load_snapshot(&mut self) -> Result<ExchangePanelSnapshot> {
        let watch = self
            .watch_provider
            .load_watch_snapshot(&self.watch_request)?;
        Ok(map_watch_snapshot(&watch))
    }
}

impl ExchangeProvider for BetRecorderExchangeProvider {
    fn handle(&mut self, request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        match request {
            ProviderRequest::LoadDashboard | ProviderRequest::Refresh => self.load_snapshot(),
            ProviderRequest::SelectVenue(VenueId::Smarkets) => self.load_snapshot(),
            ProviderRequest::CashOutTrackedBet { .. } => Err(eyre!(
                "bet-recorder watch provider does not implement cash out actions"
            )),
            ProviderRequest::SelectVenue(venue) => Err(eyre!(
                "bet-recorder provider does not support {}",
                venue.as_str()
            )),
        }
    }
}

pub(crate) fn map_watch_snapshot(watch: &WatchSnapshot) -> ExchangePanelSnapshot {
    let unique_market_count = watch
        .watches
        .iter()
        .map(|row| row.market.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("bet-recorder"),
            status: WorkerStatus::Ready,
            detail: format!(
                "Loaded {} watch groups from {} positions.",
                watch.watch_count, watch.position_count
            ),
        },
        venues: vec![VenueSummary {
            id: VenueId::Smarkets,
            label: String::from("Smarkets"),
            status: VenueStatus::Ready,
            detail: format!(
                "{} grouped watches across {} markets",
                watch.watch_count, unique_market_count
            ),
            event_count: watch.watch_count,
            market_count: unique_market_count,
        }],
        selected_venue: Some(VenueId::Smarkets),
        events: watch
            .watches
            .iter()
            .map(|row| EventCandidateSummary {
                id: format!("{}::{}", row.market, row.contract),
                label: row.contract.clone(),
                competition: row.market.clone(),
                start_time: format!(
                    "profit {:.2} stop {:.2}",
                    row.profit_take_back_odds, row.stop_loss_back_odds
                ),
                url: String::new(),
            })
            .collect(),
        markets: watch
            .watches
            .iter()
            .map(|row| MarketSummary {
                name: row.market.clone(),
                contract_count: row.position_count,
            })
            .collect(),
        preflight: None,
        status_line: format!(
            "Loaded {} Smarkets watch groups from bet-recorder.",
            watch.watch_count
        ),
        runtime: None,
        account_stats: None,
        open_positions: Vec::new(),
        historical_positions: Vec::new(),
        other_open_bets: Vec::new(),
        decisions: Vec::new(),
        watch: Some(watch.clone()),
        tracked_bets: Vec::new(),
        exit_policy: Default::default(),
        exit_recommendations: Vec::new(),
    }
}
