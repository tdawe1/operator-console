use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    #[serde(default = "default_disabled_venues")]
    pub disabled_venues: String,
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
            command: default_bet_recorder_command(),
            run_dir: default_run_dir(),
            session: String::from("helium-copy"),
            companion_legs_path: None,
            profile_path: default_profile_path(),
            disabled_venues: default_disabled_venues(),
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

const RECORDER_TRANSIENT_FILES: [&str; 5] = [
    "watcher-state.json",
    "events.jsonl",
    "metadata.json",
    "transport.jsonl",
    "watcher.log",
];
const RECORDER_SCREENSHOTS_DIR: &str = "screenshots";
const WATCHER_STARTUP_GRACE_PERIOD: Duration = Duration::from_millis(50);
const WATCHER_SPAWN_RETRY_DELAY: Duration = Duration::from_millis(25);
const WATCHER_SPAWN_MAX_ATTEMPTS: usize = 5;

fn config_root() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn default_profile_path() -> Option<PathBuf> {
    Some(config_root().join("smarkets-automation").join("profile"))
}

fn default_disabled_venues() -> String {
    String::from("bet365")
}

fn default_run_dir() -> PathBuf {
    config_root()
        .join("sabi")
        .join("runs")
        .join("smarkets-watcher")
}

pub fn default_bet_recorder_root() -> PathBuf {
    env::var_os("SABI_BET_RECORDER_ROOT")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(discover_bet_recorder_root)
        .unwrap_or_else(|| PathBuf::from("bet-recorder"))
}

pub fn default_bet_recorder_command() -> PathBuf {
    default_bet_recorder_root().join("bin").join("bet-recorder")
}

pub fn default_bet_recorder_python() -> PathBuf {
    default_bet_recorder_root()
        .join(".venv")
        .join("bin")
        .join("python")
}

fn discover_bet_recorder_root() -> Option<PathBuf> {
    env::current_dir()
        .ok()
        .and_then(|path| discover_bet_recorder_root_from(&path))
        .or_else(|| {
            env::current_exe()
                .ok()
                .and_then(|path| discover_bet_recorder_root_from(&path))
        })
}

fn discover_bet_recorder_root_from(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if is_bet_recorder_root(ancestor) {
            return Some(ancestor.to_path_buf());
        }

        let sibling = ancestor.join("bet-recorder");
        if is_bet_recorder_root(&sibling) {
            return Some(sibling);
        }
    }

    None
}

fn is_bet_recorder_root(path: &Path) -> bool {
    path.join("src").join("bet_recorder").is_dir() && path.join("pyproject.toml").is_file()
}

pub fn default_config_path() -> PathBuf {
    if let Some(path) = env::var_os("SABI_RECORDER_CONFIG_PATH") {
        return PathBuf::from(path);
    }

    config_root().join("sabi").join("recorder.json")
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
    write_private_file(path, &(serde_json::to_string_pretty(config)? + "\n"))?;
    Ok(format!("Saved recorder config to {}.", path.display()))
}

fn ensure_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_private_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents.as_bytes())?;
    file.flush()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderField {
    Command,
    RunDir,
    Session,
    CompanionLegsPath,
    Autostart,
    DisabledVenues,
    IntervalSeconds,
    CommissionRate,
    TargetProfit,
    StopLoss,
    HardMarginCallProfitFloor,
    WarnOnlyDefault,
    ProfilePath,
}

impl RecorderField {
    pub const ALL: [Self; 13] = [
        Self::Command,
        Self::RunDir,
        Self::Session,
        Self::CompanionLegsPath,
        Self::Autostart,
        Self::DisabledVenues,
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
            Self::DisabledVenues => "Disabled Venues",
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
            Self::DisabledVenues => config.disabled_venues.clone(),
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
            Self::DisabledVenues => config.disabled_venues = String::from(value),
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
            Self::Command => vec![default_bet_recorder_command().display().to_string()],
            Self::RunDir => vec![
                default_run_dir().display().to_string(),
                config_root()
                    .join("sabi")
                    .join("runs")
                    .join("smarkets-live")
                    .display()
                    .to_string(),
            ],
            Self::Session => vec![String::from("helium-copy"), String::from("default")],
            Self::CompanionLegsPath => vec![
                String::new(),
                default_run_dir()
                    .join("companion-legs.json")
                    .display()
                    .to_string(),
            ],
            Self::Autostart => vec![String::from("false"), String::from("true")],
            Self::DisabledVenues => vec![
                String::from("bet365"),
                String::new(),
                String::from("bet365,betano"),
            ],
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
                default_profile_path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Default)]
pub struct RecorderEditorState {
    pub selected_field_index: usize,
    pub editing: bool,
    pub buffer: String,
    pub replace_on_input: bool,
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

#[derive(Default)]
pub struct ProcessRecorderSupervisor {
    child: Option<Child>,
    attached_pid: Option<u32>,
    attached_run_dir: Option<PathBuf>,
}


impl RecorderSupervisor for ProcessRecorderSupervisor {
    fn start(&mut self, config: &RecorderConfig) -> Result<()> {
        if matches!(self.poll_status(), RecorderStatus::Running) {
            return Ok(());
        }
        if let Some(pid) = detect_external_watcher(&config.run_dir) {
            self.child = None;
            self.attached_pid = Some(pid);
            self.attached_run_dir = Some(config.run_dir.clone());
            return Ok(());
        }

        let mut backup = stage_run_dir_for_startup(&config.run_dir)?;
        let log_path = config.run_dir.join("watcher.log");

        match spawn_recorder_watcher(config, &log_path) {
            Ok(child) => {
                backup.discard()?;
                self.child = Some(child);
                self.attached_pid = None;
                self.attached_run_dir = None;
                Ok(())
            }
            Err(error) => {
                let restore_error = backup.restore();
                if let Err(restore_error) = restore_error {
                    return Err(error.wrap_err(format!(
                        "failed to restore recorder run dir after startup error: {restore_error}"
                    )));
                }
                Err(error)
            }
        }
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            child.kill()?;
            let _ = child.wait();
        }
        self.attached_pid = None;
        self.attached_run_dir = None;
        Ok(())
    }

    fn poll_status(&mut self) -> RecorderStatus {
        if let Some(child) = self.child.as_mut() {
            return match child.try_wait() {
                Ok(None) => RecorderStatus::Running,
                Ok(Some(_)) => {
                    self.child = None;
                    RecorderStatus::Stopped
                }
                Err(_) => {
                    self.child = None;
                    RecorderStatus::Error
                }
            };
        }

        match (&self.attached_run_dir, self.attached_pid) {
            (Some(run_dir), Some(pid)) if external_watcher_matches(run_dir, pid) => {
                RecorderStatus::Running
            }
            _ => {
                self.attached_pid = None;
                self.attached_run_dir = None;
                RecorderStatus::Disabled
            }
        }
    }
}

fn detect_external_watcher(run_dir: &Path) -> Option<u32> {
    let pid_path = run_dir.join("watcher.pid");
    let recorded_pid = fs::read_to_string(pid_path)
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    for attempt in 0..WATCHER_SPAWN_MAX_ATTEMPTS {
        if external_watcher_matches(run_dir, recorded_pid) {
            return Some(recorded_pid);
        }
        if attempt + 1 < WATCHER_SPAWN_MAX_ATTEMPTS {
            thread::sleep(WATCHER_SPAWN_RETRY_DELAY);
        }
    }
    None
}

fn external_watcher_matches(run_dir: &Path, pid: u32) -> bool {
    if !Path::new(&format!("/proc/{pid}")).exists() {
        return false;
    }
    let cmdline_path = format!("/proc/{pid}/cmdline");
    let Ok(cmdline) = fs::read(cmdline_path) else {
        return false;
    };
    let command = String::from_utf8_lossy(&cmdline).replace('\0', " ");
    command.contains("watch-smarkets-session") && command.contains(&run_dir.display().to_string())
}

fn spawn_recorder_watcher(config: &RecorderConfig, log_path: &Path) -> Result<Child> {
    let log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
        .map_err(|error| {
            eyre!(
                "failed to open recorder watcher log {}: {error}",
                log_path.display()
            )
        })?;
    let stdout = log_file
        .try_clone()
        .map_err(|error| eyre!("failed to clone recorder watcher log handle: {error}"))?;

    let mut command = Command::new(&config.command);
    command
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
        .stderr(Stdio::from(log_file));

    let mut child = spawn_recorder_command_with_retry(&mut command)?;

    thread::sleep(WATCHER_STARTUP_GRACE_PERIOD);
    match child
        .try_wait()
        .map_err(|error| eyre!("failed to inspect recorder watcher startup: {error}"))?
    {
        None => Ok(child),
        Some(status) => {
            let detail = fs::read_to_string(log_path)
                .ok()
                .map(|content| content.trim().to_string())
                .filter(|content| !content.is_empty())
                .unwrap_or_else(|| String::from("watcher log was empty"));
            Err(eyre!(
                "recorder watcher exited immediately with status {status}: {detail}"
            ))
        }
    }
}

fn spawn_recorder_command_with_retry(command: &mut Command) -> Result<Child> {
    let mut attempts = 0;
    loop {
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(error)
                if is_text_file_busy_error(&error) && attempts + 1 < WATCHER_SPAWN_MAX_ATTEMPTS =>
            {
                attempts += 1;
                thread::sleep(WATCHER_SPAWN_RETRY_DELAY);
            }
            Err(error) => return Err(eyre!("failed to start recorder watcher: {error}")),
        }
    }
}

fn is_text_file_busy_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(26))
}

fn stage_run_dir_for_startup(run_dir: &Path) -> Result<RunDirBackup> {
    let mut backup = RunDirBackup::new(run_dir.to_path_buf());
    let result = (|| -> Result<()> {
        ensure_private_dir(run_dir)?;

        for relative_path in RECORDER_TRANSIENT_FILES {
            backup.move_entry(relative_path)?;
        }
        backup.move_entry(RECORDER_SCREENSHOTS_DIR)?;

        ensure_private_dir(&run_dir.join(RECORDER_SCREENSHOTS_DIR))?;
        Ok(())
    })();

    if let Err(error) = result {
        let _ = backup.restore();
        return Err(error);
    }

    Ok(backup)
}

struct RunDirBackup {
    run_dir: PathBuf,
    backup_dir: Option<PathBuf>,
}

impl RunDirBackup {
    fn new(run_dir: PathBuf) -> Self {
        Self {
            run_dir,
            backup_dir: None,
        }
    }

    fn move_entry(&mut self, relative_path: &str) -> Result<()> {
        let source = self.run_dir.join(relative_path);
        if !source.exists() {
            return Ok(());
        }

        let Some(backup_dir) = self.ensure_backup_dir()? else {
            return Ok(());
        };
        let destination = backup_dir.join(relative_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&source, &destination)?;
        Ok(())
    }

    fn restore(&mut self) -> Result<()> {
        let Some(backup_dir) = self.backup_dir.take() else {
            return Ok(());
        };

        for relative_path in RECORDER_TRANSIENT_FILES {
            remove_entry_if_present(&self.run_dir.join(relative_path))?;
        }
        remove_entry_if_present(&self.run_dir.join(RECORDER_SCREENSHOTS_DIR))?;

        for entry in fs::read_dir(&backup_dir)? {
            let entry = entry?;
            fs::rename(entry.path(), self.run_dir.join(entry.file_name()))?;
        }
        fs::remove_dir_all(backup_dir)?;
        Ok(())
    }

    fn discard(&mut self) -> Result<()> {
        let Some(backup_dir) = self.backup_dir.take() else {
            return Ok(());
        };
        fs::remove_dir_all(backup_dir)?;
        Ok(())
    }

    fn ensure_backup_dir(&mut self) -> Result<Option<&Path>> {
        if self.backup_dir.is_none() {
            let parent = self
                .run_dir
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.run_dir.clone());
            let file_stem = self
                .run_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("run-dir");
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let backup_dir = parent.join(format!(
                ".{file_stem}.startup-backup-{}-{timestamp}",
                std::process::id()
            ));
            fs::create_dir_all(&backup_dir)?;
            self.backup_dir = Some(backup_dir);
        }

        Ok(self.backup_dir.as_deref())
    }
}

fn remove_entry_if_present(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        default_bet_recorder_command, discover_bet_recorder_root_from, is_bet_recorder_root,
    };
    use std::fs;

    #[test]
    fn discover_bet_recorder_root_from_workspace_tree() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let workspace_root = temp_dir.path().join("sabi");
        let bet_recorder_root = workspace_root.join("bet-recorder");
        let console_root = workspace_root.join("console").join("operator-console");

        fs::create_dir_all(bet_recorder_root.join("src").join("bet_recorder")).expect("mkdir");
        fs::write(
            bet_recorder_root.join("pyproject.toml"),
            "[project]\nname='bet-recorder'\n",
        )
        .expect("pyproject");
        fs::create_dir_all(console_root.join("target").join("debug")).expect("console tree");

        let discovered =
            discover_bet_recorder_root_from(&console_root.join("target").join("debug"))
                .expect("discover root");

        assert_eq!(discovered, bet_recorder_root);
        assert!(is_bet_recorder_root(&discovered));
        assert_eq!(
            default_bet_recorder_command().file_name().unwrap(),
            "bet-recorder"
        );
    }
}
