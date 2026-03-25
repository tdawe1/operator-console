use std::env;
use std::io::{self, stdout};
use std::path::PathBuf;

use color_eyre::eyre::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use operator_console::app::App;
use operator_console::native_provider::{HybridExchangeProvider, NativeExchangeProvider};
use operator_console::recorder::{
    default_bet_recorder_command, default_bet_recorder_python, default_bet_recorder_root,
};
use operator_console::stub_provider::StubExchangeProvider;
use operator_console::transport::WorkerConfig;
use operator_console::worker_client::{BetRecorderWorkerClient, WorkerClientExchangeProvider};

fn main() -> Result<()> {
    color_eyre::install()?;

    let launch_mode = match LaunchMode::parse(env::args().skip(1)) {
        Ok(mode) => mode,
        Err(message) => {
            println!("{message}");
            return Ok(());
        }
    };

    let should_autostart = matches!(launch_mode, LaunchMode::Stub);
    let provider: Box<dyn operator_console::provider::ExchangeProvider + Send> = match launch_mode {
        LaunchMode::Stub => Box::new(StubExchangeProvider::default()),
        LaunchMode::BetRecorder {
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
        } => {
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

    let mut app = App::from_provider(provider)?;
    if should_autostart {
        app.autostart_recorder_if_enabled()?;
    }
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

fn help_text() -> &'static str {
    "operator-console\n\nUsage:\n  operator-console [options]\n\nOptions:\n  --bet-recorder-payload-path <path>       Load positions from a captured payload\n  --bet-recorder-run-dir <path>            Load the latest exchange snapshot from a bet-recorder run bundle\n  --bet-recorder-account-path <path>       Optional account stats payload\n  --bet-recorder-open-bets-path <path>     Optional open bets payload\n  --bet-recorder-session <name>            agent-browser session to capture before refresh\n  --bet-recorder-command <path>            bet-recorder executable to run\n  --bet-recorder-python <path>             Python executable override for bet-recorder\n  --bet-recorder-root <path>               bet-recorder checkout root override\n  --commission-rate <value>                Exchange commission rate for worker calculations\n  --target-profit <value>                  Target profit for worker calculations\n  --stop-loss <value>                      Stop-loss for worker calculations\n  -h, --help                               Show this help\n"
}

enum LaunchMode {
    Stub,
    BetRecorder {
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
    },
}

impl LaunchMode {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
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

        let mut iter = args.into_iter();
        while let Some(argument) = iter.next() {
            match argument.as_str() {
                "-h" | "--help" => return Err(help_text().to_string()),
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
                _ => return Err(help_text().to_string()),
            }
        }

        Ok(match (positions_payload_path, run_dir) {
            (Some(positions_payload_path), run_dir) => LaunchMode::BetRecorder {
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
            },
            (None, Some(run_dir)) => LaunchMode::BetRecorder {
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
            },
            (None, None) => LaunchMode::Stub,
        })
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
        .map_err(|_| help_text().to_string())
}
