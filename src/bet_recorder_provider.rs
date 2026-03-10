use std::path::PathBuf;
use std::process::Command;

use color_eyre::eyre::{eyre, Context, Result};

use crate::domain::WatchSnapshot;
use crate::provider::{WatchProvider, WatchRequest};

#[derive(Debug, Clone)]
pub struct BetRecorderProvider {
    python_executable: PathBuf,
    bet_recorder_root: PathBuf,
}

impl BetRecorderProvider {
    pub fn new(python_executable: PathBuf, bet_recorder_root: PathBuf) -> Self {
        Self {
            python_executable,
            bet_recorder_root,
        }
    }
}

impl WatchProvider for BetRecorderProvider {
    fn load_watch_snapshot(&mut self, request: &WatchRequest) -> Result<WatchSnapshot> {
        let mut command = self.base_command();
        let output = command
            .current_dir(&self.bet_recorder_root)
            .env("PYTHONPATH", self.bet_recorder_root.join("src"))
            .arg("watch-open-positions")
            .arg("--payload-path")
            .arg(&request.payload_path)
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
        let mut command = Command::new(&self.python_executable);
        command
            .arg("-c")
            .arg("from bet_recorder.cli import main; main()");
        command
    }
}
