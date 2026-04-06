mod backend_provider;

use std::env;
use std::io::{self, stdout};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use backend_provider::BackendExchangeProvider;
use color_eyre::eyre::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use operator_console::app::App;
use operator_console::domain::{ExchangePanelSnapshot, VenueId, WorkerStatus, WorkerSummary};
use operator_console::native_provider::{HybridExchangeProvider, NativeExchangeProvider};
use operator_console::recorder::{
    default_bet_recorder_command, default_bet_recorder_python, default_bet_recorder_root,
};
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use operator_console::theme::{self, Name as ThemeName};
use operator_console::tracing_setup::init_tracing;
use operator_console::transport::WorkerConfig;
use operator_console::worker_client::{BetRecorderWorkerClient, WorkerClientExchangeProvider};

fn main() -> Result<()> {
    color_eyre::install()?;
    let _ = init_tracing();
    maybe_autostart_local_sabisabi_backend()?;

    let options = match CliOptions::parse(env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            println!("{message}");
            return Ok(());
        }
    };

    theme::set_theme(options.theme);

    let provider: Box<dyn operator_console::provider::ExchangeProvider + Send> =
        match options.launch_mode {
            LaunchMode::Stub => default_configured_provider(),
            LaunchMode::BetRecorder(config) => {
                let BetRecorderLaunchConfig {
                    positions_payload_path,
                    run_dir,
                    account_payload_path,
                    open_bets_payload_path,
                    agent_browser_session,
                    bet_recorder_command,
                    python_executable,
                    bet_recorder_root,
                    commission_rate,
                    target_profit,
                    stop_loss,
                } = *config;
                let worker_config = WorkerConfig {
                    positions_payload_path,
                    run_dir,
                    account_payload_path,
                    open_bets_payload_path,
                    companion_legs_path: None,
                    agent_browser_session,
                    commission_rate,
                    target_profit,
                    stop_loss,
                    hard_margin_call_profit_floor: None,
                    warn_only_default: true,
                };
                Box::new(HybridExchangeProvider::new(
                    Box::new(NativeExchangeProvider::new(worker_config.clone())),
                    Box::new(WorkerClientExchangeProvider::new(
                        if bet_recorder_command.exists() {
                            BetRecorderWorkerClient::new_command(bet_recorder_command)
                        } else {
                            BetRecorderWorkerClient::new(python_executable, bet_recorder_root)
                        },
                        worker_config,
                    )),
                ))
            }
        };

    let mut app = App::from_provider_with_base_factory(provider, Box::new(default_configured_provider))?;
    enable_mouse_capture()?;
    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal);
    ratatui::restore();
    disable_mouse_capture()?;
    result
}

fn enable_mouse_capture() -> io::Result<()> {
    execute!(stdout(), EnableMouseCapture)
}

fn disable_mouse_capture() -> io::Result<()> {
    execute!(stdout(), DisableMouseCapture)
}

fn maybe_autostart_local_sabisabi_backend() -> Result<()> {
    let base_url =
        env::var("SABISABI_BASE_URL").unwrap_or_else(|_| String::from("http://127.0.0.1:4080"));
    if !should_autostart_local_sabisabi_backend(&base_url) || sabisabi_is_healthy(&base_url) {
        return Ok(());
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .ok_or_else(|| color_eyre::eyre::eyre!("failed to resolve sabi repo root"))?
        .to_path_buf();
    let sabisabi_dir = repo_root.join("sabisabi");
    let sabisabi_bin = sabisabi_dir.join("target/debug/sabisabi");

    let build_status = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(sabisabi_dir.join("Cargo.toml"))
        .status()?;
    if !build_status.success() {
        return Err(color_eyre::eyre::eyre!("failed to build sabisabi backend"));
    }

    Command::new(&sabisabi_bin)
        .current_dir(&sabisabi_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    for _ in 0..20 {
        if sabisabi_is_healthy(&base_url) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }

    Err(color_eyre::eyre::eyre!(
        "sabisabi backend did not become healthy at {base_url}"
    ))
}

fn is_local_default_sabisabi(base_url: &str) -> bool {
    matches!(
        base_url.trim_end_matches('/'),
        "http://127.0.0.1:4080" | "http://localhost:4080"
    )
}

fn should_autostart_local_sabisabi_backend(base_url: &str) -> bool {
    should_autostart_local_sabisabi_backend_with_flag(
        base_url,
        autostart_sabisabi_backend_enabled(),
    )
}

fn should_autostart_local_sabisabi_backend_with_flag(
    base_url: &str,
    autostart_enabled: bool,
) -> bool {
    is_local_default_sabisabi(base_url) && autostart_enabled
}

fn autostart_sabisabi_backend_enabled() -> bool {
    env::var("OPERATOR_CONSOLE_AUTOSTART_SABISABI")
        .ok()
        .as_deref()
        .is_some_and(flag_is_enabled)
}

fn flag_is_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn sabisabi_is_healthy(base_url: &str) -> bool {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_millis(500))
        .build()
        .ok()
        .and_then(|client| {
            client
                .get(format!("{}/health", base_url.trim_end_matches('/')))
                .send()
                .ok()
        })
        .is_some_and(|response| response.status().is_success())
}

#[cfg(test)]
mod tests {
    use super::{flag_is_enabled, should_autostart_local_sabisabi_backend_with_flag};

    #[test]
    fn autostart_flag_recognizes_enabled_values() {
        for value in ["1", "true", "TRUE", " yes ", "On"] {
            assert!(
                flag_is_enabled(value),
                "expected {value:?} to enable autostart"
            );
        }
    }

    #[test]
    fn autostart_flag_rejects_disabled_values() {
        for value in ["", "0", "false", "off", "no", "disabled"] {
            assert!(
                !flag_is_enabled(value),
                "expected {value:?} to disable autostart"
            );
        }
    }

    #[test]
    fn autostart_only_targets_local_default_backend() {
        assert!(should_autostart_local_sabisabi_backend_with_flag(
            "http://127.0.0.1:4080",
            true
        ));
        assert!(should_autostart_local_sabisabi_backend_with_flag(
            "http://localhost:4080",
            true
        ));
        assert!(!should_autostart_local_sabisabi_backend_with_flag(
            "https://sabisabi.internal",
            true
        ));
        assert!(!should_autostart_local_sabisabi_backend_with_flag(
            "http://127.0.0.1:9999",
            true
        ));
        assert!(!should_autostart_local_sabisabi_backend_with_flag(
            "http://127.0.0.1:4080",
            false
        ));
    }
}

fn help_text() -> String {
    format!(
        "operator-console\n\nUsage:\n  operator-console [options]\n\nOptions:\n  --theme <name>                           Select UI theme (default: {})\n  --list-themes                            Print available theme names\n  --bet-recorder-payload-path <path>       Load positions from a captured payload\n  --bet-recorder-run-dir <path>            Load the latest exchange snapshot from a bet-recorder run bundle\n  --bet-recorder-account-path <path>       Optional account stats payload\n  --bet-recorder-open-bets-path <path>     Optional open bets payload\n  --bet-recorder-session <name>            agent-browser session to capture before refresh\n  --bet-recorder-command <path>            bet-recorder executable to run\n  --bet-recorder-python <path>             Python executable override for bet-recorder\n  --bet-recorder-root <path>               bet-recorder checkout root override\n  --commission-rate <value>                Exchange commission rate for worker calculations\n  --target-profit <value>                  Target profit for worker calculations\n  --stop-loss <value>                      Stop-loss for worker calculations\n  -h, --help                               Show this help\n\nRun `--list-themes` to see the available theme names.\n",
        theme::default_theme().slug(),
    )
}

fn default_configured_provider() -> Box<dyn operator_console::provider::ExchangeProvider + Send> {
    match BackendExchangeProvider::new() {
        Ok(provider) => Box::new(provider),
        Err(error) => Box::new(BackendUnavailableProvider::new(error.to_string())),
    }
}

#[derive(Debug, Clone)]
struct BackendUnavailableProvider {
    detail: String,
}

impl BackendUnavailableProvider {
    fn new(detail: String) -> Self {
        Self { detail }
    }

    fn snapshot(&self) -> ExchangePanelSnapshot {
        ExchangePanelSnapshot {
            worker: WorkerSummary {
                name: String::from("sabisabi"),
                status: WorkerStatus::Error,
                detail: self.detail.clone(),
            },
            selected_venue: Some(VenueId::Smarkets),
            status_line: format!("Backend unavailable: {}", self.detail),
            ..ExchangePanelSnapshot::default()
        }
    }
}

impl ExchangeProvider for BackendUnavailableProvider {
    fn handle(&mut self, _request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        Ok(self.snapshot())
    }
}

fn list_themes_text() -> String {
    let mut lines = vec![String::from("Available themes:")];
    for theme in ThemeName::all() {
        lines.push(format!("  {:<20} {}", theme.slug(), theme.display_name()));
    }
    lines.join("\n")
}

struct CliOptions {
    launch_mode: LaunchMode,
    theme: ThemeName,
}

struct BetRecorderLaunchConfig {
    positions_payload_path: Option<PathBuf>,
    run_dir: Option<PathBuf>,
    account_payload_path: Option<PathBuf>,
    open_bets_payload_path: Option<PathBuf>,
    agent_browser_session: Option<String>,
    bet_recorder_command: PathBuf,
    python_executable: PathBuf,
    bet_recorder_root: PathBuf,
    commission_rate: f64,
    target_profit: f64,
    stop_loss: f64,
}

enum LaunchMode {
    Stub,
    BetRecorder(Box<BetRecorderLaunchConfig>),
}

impl CliOptions {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<CliOptions, String> {
        let mut positions_payload_path = None;
        let mut run_dir = None;
        let mut account_payload_path = None;
        let mut open_bets_payload_path = None;
        let mut agent_browser_session = None;
        let mut bet_recorder_command = default_bet_recorder_command();
        let mut python_executable = default_bet_recorder_python();
        let mut bet_recorder_root = default_bet_recorder_root();
        let mut commission_rate = 0.0;
        let mut target_profit = 1.0;
        let mut stop_loss = 1.0;
        let mut theme = theme::default_theme();

        let mut iter = args.into_iter();
        while let Some(argument) = iter.next() {
            match argument.as_str() {
                "-h" | "--help" => return Err(help_text()),
                "--list-themes" => return Err(list_themes_text()),
                "--theme" => {
                    theme = parse_theme(&mut iter)?;
                }
                "--bet-recorder-payload-path" => {
                    positions_payload_path = Some(PathBuf::from(next_value(
                        &mut iter,
                        "--bet-recorder-payload-path",
                    )?))
                }
                "--bet-recorder-run-dir" => {
                    run_dir = Some(PathBuf::from(next_value(
                        &mut iter,
                        "--bet-recorder-run-dir",
                    )?))
                }
                "--bet-recorder-account-path" => {
                    account_payload_path = Some(PathBuf::from(next_value(
                        &mut iter,
                        "--bet-recorder-account-path",
                    )?))
                }
                "--bet-recorder-open-bets-path" => {
                    open_bets_payload_path = Some(PathBuf::from(next_value(
                        &mut iter,
                        "--bet-recorder-open-bets-path",
                    )?))
                }
                "--bet-recorder-session" => {
                    agent_browser_session = Some(next_value(&mut iter, "--bet-recorder-session")?)
                }
                "--bet-recorder-command" => {
                    bet_recorder_command =
                        PathBuf::from(next_value(&mut iter, "--bet-recorder-command")?)
                }
                "--bet-recorder-python" => {
                    python_executable =
                        PathBuf::from(next_value(&mut iter, "--bet-recorder-python")?)
                }
                "--bet-recorder-root" => {
                    bet_recorder_root = PathBuf::from(next_value(&mut iter, "--bet-recorder-root")?)
                }
                "--commission-rate" => {
                    commission_rate = parse_f64(&mut iter, "--commission-rate")?;
                }
                "--target-profit" => {
                    target_profit = parse_f64(&mut iter, "--target-profit")?;
                }
                "--stop-loss" => {
                    stop_loss = parse_f64(&mut iter, "--stop-loss")?;
                }
                _ => return Err(help_text()),
            }
        }

        let launch_mode = match (positions_payload_path, run_dir) {
            (Some(positions_payload_path), run_dir) => {
                LaunchMode::BetRecorder(Box::new(BetRecorderLaunchConfig {
                    positions_payload_path: Some(positions_payload_path),
                    run_dir,
                    account_payload_path,
                    open_bets_payload_path,
                    agent_browser_session,
                    bet_recorder_command,
                    python_executable,
                    bet_recorder_root,
                    commission_rate,
                    target_profit,
                    stop_loss,
                }))
            }
            (None, Some(run_dir)) => LaunchMode::BetRecorder(Box::new(BetRecorderLaunchConfig {
                positions_payload_path: None,
                run_dir: Some(run_dir),
                account_payload_path,
                open_bets_payload_path,
                agent_browser_session,
                bet_recorder_command,
                python_executable,
                bet_recorder_root,
                commission_rate,
                target_profit,
                stop_loss,
            })),
            (None, None) => LaunchMode::Stub,
        };

        Ok(CliOptions { launch_mode, theme })
    }
}

fn next_value(
    iter: &mut impl Iterator<Item = String>,
    option_name: &str,
) -> Result<String, String> {
    iter.next()
        .ok_or_else(|| format!("{option_name} requires a value\n\n{}", help_text()))
}

fn parse_f64(iter: &mut impl Iterator<Item = String>, option_name: &str) -> Result<f64, String> {
    next_value(iter, option_name)?
        .parse::<f64>()
        .map_err(|_| help_text())
}

fn parse_theme(iter: &mut impl Iterator<Item = String>) -> Result<ThemeName, String> {
    let value = next_value(iter, "--theme")?;
    value
        .parse::<ThemeName>()
        .map_err(|error| format!("{error}\n\n{}\n\n{}", help_text(), list_themes_text(),))
}
