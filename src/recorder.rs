use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use color_eyre::eyre::{eyre, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecorderStatus {
    Disabled,
    Running,
    Stopped,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderConfig {
    pub command: PathBuf,
    pub run_dir: PathBuf,
    pub session: String,
    pub interval_seconds: u64,
    pub commission_rate: String,
    pub target_profit: String,
    pub stop_loss: String,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            command: PathBuf::from("/home/thomas/projects/sabi/bet-recorder/bin/bet-recorder"),
            run_dir: PathBuf::from("/tmp/sabi-smarkets-watcher"),
            session: String::from("helium-copy"),
            interval_seconds: 5,
            commission_rate: String::from("0"),
            target_profit: String::from("1"),
            stop_loss: String::from("1"),
        }
    }
}

pub trait RecorderSupervisor {
    fn start(&mut self, config: &RecorderConfig) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn poll_status(&mut self) -> RecorderStatus;
}

pub struct ProcessRecorderSupervisor {
    child: Option<Child>,
}

impl Default for ProcessRecorderSupervisor {
    fn default() -> Self {
        Self { child: None }
    }
}

impl RecorderSupervisor for ProcessRecorderSupervisor {
    fn start(&mut self, config: &RecorderConfig) -> Result<()> {
        if matches!(self.poll_status(), RecorderStatus::Running) {
            return Ok(());
        }

        let child = Command::new(&config.command)
            .arg("watch-smarkets-session")
            .arg("--run-dir")
            .arg(&config.run_dir)
            .arg("--session")
            .arg(&config.session)
            .arg("--interval-seconds")
            .arg(config.interval_seconds.to_string())
            .arg("--commission-rate")
            .arg(&config.commission_rate)
            .arg("--target-profit")
            .arg(&config.target_profit)
            .arg("--stop-loss")
            .arg(&config.stop_loss)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| eyre!("failed to start recorder watcher: {error}"))?;

        self.child = Some(child);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            child.kill()?;
            let _ = child.wait();
        }
        Ok(())
    }

    fn poll_status(&mut self) -> RecorderStatus {
        let Some(child) = self.child.as_mut() else {
            return RecorderStatus::Disabled;
        };

        match child.try_wait() {
            Ok(None) => RecorderStatus::Running,
            Ok(Some(_)) => {
                self.child = None;
                RecorderStatus::Stopped
            }
            Err(_) => {
                self.child = None;
                RecorderStatus::Error
            }
        }
    }
}
