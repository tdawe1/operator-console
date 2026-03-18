use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use color_eyre::eyre::{eyre, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecorderStatus {
    Disabled,
    Running,
    Stopped,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecorderConfig {
    pub command: PathBuf,
    pub run_dir: PathBuf,
    pub session: String,
    pub companion_legs_path: Option<PathBuf>,
    #[serde(default = "default_profile_path")]
    pub profile_path: Option<PathBuf>,
    pub autostart: bool,
    pub interval_seconds: u64,
    pub commission_rate: String,
    pub target_profit: String,
    pub stop_loss: String,
    pub hard_margin_call_profit_floor: String,
    pub warn_only_default: bool,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            command: PathBuf::from("/home/thomas/projects/sabi/bet-recorder/bin/bet-recorder"),
            run_dir: PathBuf::from("/tmp/sabi-smarkets-watcher"),
            session: String::from("helium-copy"),
            companion_legs_path: None,
            profile_path: default_profile_path(),
            autostart: false,
            interval_seconds: 5,
            commission_rate: String::from("0"),
            target_profit: String::from("1"),
            stop_loss: String::from("1"),
            hard_margin_call_profit_floor: String::new(),
            warn_only_default: true,
        }
    }
}

fn default_profile_path() -> Option<PathBuf> {
    Some(PathBuf::from(
        "/home/thomas/.config/smarkets-automation/profile",
    ))
}

pub fn default_config_path() -> PathBuf {
    if let Some(path) = env::var_os("SABI_RECORDER_CONFIG_PATH") {
        return PathBuf::from(path);
    }

    let config_root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));

    config_root.join("sabi").join("recorder.json")
}

pub fn load_recorder_config_or_default(path: &Path) -> Result<(RecorderConfig, String)> {
    if !path.exists() {
        return Ok((
            RecorderConfig::default(),
            String::from("Using default recorder config."),
        ));
    }

    let content = fs::read_to_string(path)?;
    let config = serde_json::from_str::<RecorderConfig>(&content)?;
    Ok((
        config,
        format!("Loaded recorder config from {}.", path.display()),
    ))
}

pub fn save_recorder_config(path: &Path, config: &RecorderConfig) -> Result<String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(config)? + "\n")?;
    Ok(format!("Saved recorder config to {}.", path.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderField {
    Command,
    RunDir,
    Session,
    CompanionLegsPath,
    Autostart,
    IntervalSeconds,
    CommissionRate,
    TargetProfit,
    StopLoss,
    HardMarginCallProfitFloor,
    WarnOnlyDefault,
    ProfilePath,
}

impl RecorderField {
    pub const ALL: [Self; 12] = [
        Self::Command,
        Self::RunDir,
        Self::Session,
        Self::CompanionLegsPath,
        Self::Autostart,
        Self::IntervalSeconds,
        Self::CommissionRate,
        Self::TargetProfit,
        Self::StopLoss,
        Self::HardMarginCallProfitFloor,
        Self::WarnOnlyDefault,
        Self::ProfilePath,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Command => "Command",
            Self::RunDir => "Run Dir",
            Self::Session => "Session",
            Self::CompanionLegsPath => "Companion Legs",
            Self::Autostart => "Autostart",
            Self::IntervalSeconds => "Interval",
            Self::CommissionRate => "Commission",
            Self::TargetProfit => "Target Profit",
            Self::StopLoss => "Stop Loss",
            Self::HardMarginCallProfitFloor => "Hard Profit Floor",
            Self::WarnOnlyDefault => "Warn Only",
            Self::ProfilePath => "Profile Path",
        }
    }

    pub fn display_value(self, config: &RecorderConfig) -> String {
        match self {
            Self::Command => config.command.display().to_string(),
            Self::RunDir => config.run_dir.display().to_string(),
            Self::Session => config.session.clone(),
            Self::CompanionLegsPath => config
                .companion_legs_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            Self::Autostart => config.autostart.to_string(),
            Self::IntervalSeconds => config.interval_seconds.to_string(),
            Self::CommissionRate => config.commission_rate.clone(),
            Self::TargetProfit => config.target_profit.clone(),
            Self::StopLoss => config.stop_loss.clone(),
            Self::HardMarginCallProfitFloor => config.hard_margin_call_profit_floor.clone(),
            Self::WarnOnlyDefault => config.warn_only_default.to_string(),
            Self::ProfilePath => config
                .profile_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        }
    }

    pub fn apply_value(self, config: &mut RecorderConfig, value: &str) -> Result<()> {
        match self {
            Self::Command => config.command = PathBuf::from(value),
            Self::RunDir => config.run_dir = PathBuf::from(value),
            Self::Session => config.session = String::from(value),
            Self::CompanionLegsPath => {
                let trimmed = value.trim();
                config.companion_legs_path = if trimmed.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(trimmed))
                };
            }
            Self::ProfilePath => {
                let trimmed = value.trim();
                config.profile_path = if trimmed.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(trimmed))
                };
            }
            Self::Autostart => {
                config.autostart = value
                    .parse::<bool>()
                    .map_err(|_| eyre!("autostart must be true or false"))?;
            }
            Self::IntervalSeconds => {
                config.interval_seconds = value
                    .parse::<u64>()
                    .map_err(|_| eyre!("interval must be a positive integer"))?;
            }
            Self::CommissionRate => config.commission_rate = String::from(value),
            Self::TargetProfit => config.target_profit = String::from(value),
            Self::StopLoss => config.stop_loss = String::from(value),
            Self::HardMarginCallProfitFloor => {
                config.hard_margin_call_profit_floor = String::from(value);
            }
            Self::WarnOnlyDefault => {
                config.warn_only_default = value
                    .parse::<bool>()
                    .map_err(|_| eyre!("warn_only_default must be true or false"))?;
            }
        }
        Ok(())
    }

    pub fn suggestions(self) -> Vec<String> {
        match self {
            Self::Command => vec![String::from(
                "/home/thomas/projects/sabi/bet-recorder/bin/bet-recorder",
            )],
            Self::RunDir => vec![
                String::from("/tmp/sabi-smarkets-watcher"),
                String::from("/tmp/sabi-live-smarkets"),
            ],
            Self::Session => vec![String::from("helium-copy"), String::from("default")],
            Self::CompanionLegsPath => vec![
                String::new(),
                String::from("/tmp/sabi-smarkets-watcher/companion-legs.json"),
            ],
            Self::Autostart => vec![String::from("false"), String::from("true")],
            Self::IntervalSeconds => {
                vec![String::from("5"), String::from("10"), String::from("15")]
            }
            Self::CommissionRate => vec![String::from("0")],
            Self::TargetProfit => vec![String::from("1"), String::from("2"), String::from("3")],
            Self::StopLoss => vec![String::from("1"), String::from("2"), String::from("3")],
            Self::HardMarginCallProfitFloor => vec![
                String::new(),
                String::from("0"),
                String::from("1"),
                String::from("2"),
            ],
            Self::WarnOnlyDefault => vec![String::from("true"), String::from("false")],
            Self::ProfilePath => vec![
                String::new(),
                String::from("/home/thomas/.config/smarkets-automation/profile"),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderEditorState {
    pub selected_field_index: usize,
    pub editing: bool,
    pub buffer: String,
    pub replace_on_input: bool,
}

impl Default for RecorderEditorState {
    fn default() -> Self {
        Self {
            selected_field_index: 0,
            editing: false,
            buffer: String::new(),
            replace_on_input: false,
        }
    }
}

impl RecorderEditorState {
    pub fn selected_field(&self) -> RecorderField {
        RecorderField::ALL[self.selected_field_index]
    }

    pub fn select_next_field(&mut self) {
        self.selected_field_index = (self.selected_field_index + 1) % RecorderField::ALL.len();
    }

    pub fn select_previous_field(&mut self) {
        self.selected_field_index = if self.selected_field_index == 0 {
            RecorderField::ALL.len() - 1
        } else {
            self.selected_field_index - 1
        };
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

        reset_run_dir(&config.run_dir)?;
        let log_path = config.run_dir.join("watcher.log");
        let log_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
            .map_err(|error| {
                eyre!(
                    "failed to open recorder watcher log {}: {error}",
                    log_path.display()
                )
            })?;
        let stdout = log_file
            .try_clone()
            .map_err(|error| eyre!("failed to clone recorder watcher log handle: {error}"))?;

        let child = Command::new(&config.command)
            .arg("watch-smarkets-session")
            .arg("--run-dir")
            .arg(&config.run_dir)
            .arg("--session")
            .arg(&config.session)
            .args(
                config
                    .profile_path
                    .as_ref()
                    .map(|path| vec![String::from("--profile-path"), path.display().to_string()])
                    .unwrap_or_default(),
            )
            .arg("--interval-seconds")
            .arg(config.interval_seconds.to_string())
            .arg("--commission-rate")
            .arg(&config.commission_rate)
            .arg("--target-profit")
            .arg(&config.target_profit)
            .arg("--stop-loss")
            .arg(&config.stop_loss)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(log_file))
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

fn reset_run_dir(run_dir: &Path) -> Result<()> {
    fs::create_dir_all(run_dir)?;

    for relative_path in [
        "watcher-state.json",
        "events.jsonl",
        "metadata.json",
        "transport.jsonl",
    ] {
        let path = run_dir.join(relative_path);
        if path.exists() {
            fs::remove_file(&path)?;
        }
    }

    let screenshots_dir = run_dir.join("screenshots");
    if screenshots_dir.exists() {
        fs::remove_dir_all(&screenshots_dir)?;
    }
    fs::create_dir_all(&screenshots_dir)?;

    Ok(())
}
