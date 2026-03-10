use std::env;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use operator_console::app::App;
use operator_console::bet_recorder_provider::BetRecorderProvider;
use operator_console::provider::WatchRequest;

const DEFAULT_BET_RECORDER_ROOT: &str = "/home/thomas/projects/sabi/bet-recorder";
const DEFAULT_PYTHON: &str = "/home/thomas/projects/sabi/bet-recorder/.venv/bin/python";

struct CliOptions {
    payload_path: PathBuf,
    python_executable: PathBuf,
    bet_recorder_root: PathBuf,
    commission_rate: f64,
    target_profit: f64,
    stop_loss: f64,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let options = match CliOptions::parse(env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            println!("{message}");
            return Ok(());
        }
    };

    let request = WatchRequest {
        payload_path: options.payload_path,
        commission_rate: options.commission_rate,
        target_profit: options.target_profit,
        stop_loss: options.stop_loss,
    };

    let provider = BetRecorderProvider::new(options.python_executable, options.bet_recorder_root);
    let mut app = App::new(request, provider);
    app.refresh()?;

    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}

impl CliOptions {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut payload_path = None;
        let mut python_executable = PathBuf::from(DEFAULT_PYTHON);
        let mut bet_recorder_root = PathBuf::from(DEFAULT_BET_RECORDER_ROOT);
        let mut commission_rate = 0.0;
        let mut target_profit = 1.0;
        let mut stop_loss = 1.0;

        let mut iter = args.into_iter();
        while let Some(argument) = iter.next() {
            match argument.as_str() {
                "--help" | "-h" => return Err(Self::help_text()),
                "--payload-path" => {
                    payload_path = Some(PathBuf::from(Self::next_value(
                        &mut iter,
                        "--payload-path",
                    )?))
                }
                "--python" => {
                    python_executable = PathBuf::from(Self::next_value(&mut iter, "--python")?)
                }
                "--bet-recorder-root" => {
                    bet_recorder_root =
                        PathBuf::from(Self::next_value(&mut iter, "--bet-recorder-root")?)
                }
                "--commission-rate" => {
                    commission_rate = Self::next_value(&mut iter, "--commission-rate")?
                        .parse::<f64>()
                        .map_err(|_| Self::help_text())?;
                }
                "--target-profit" => {
                    target_profit = Self::next_value(&mut iter, "--target-profit")?
                        .parse::<f64>()
                        .map_err(|_| Self::help_text())?;
                }
                "--stop-loss" => {
                    stop_loss = Self::next_value(&mut iter, "--stop-loss")?
                        .parse::<f64>()
                        .map_err(|_| Self::help_text())?;
                }
                _ => return Err(Self::help_text()),
            }
        }

        let payload_path = payload_path.ok_or_else(Self::help_text)?;

        Ok(Self {
            payload_path,
            python_executable,
            bet_recorder_root,
            commission_rate,
            target_profit,
            stop_loss,
        })
    }

    fn next_value(
        iter: &mut impl Iterator<Item = String>,
        option_name: &str,
    ) -> Result<String, String> {
        iter.next()
            .ok_or_else(|| format!("{option_name} requires a value\n\n{}", Self::help_text()))
    }

    fn help_text() -> String {
        [
            "operator-console",
            "",
            "Usage:",
            "  operator-console --payload-path <path> [options]",
            "",
            "Options:",
            "  --payload-path <path>         Smarkets open_positions payload JSON",
            "  --python <path>               Python executable for bet-recorder",
            "  --bet-recorder-root <path>    bet-recorder checkout root",
            "  --commission-rate <value>     Commission rate passed to watch-open-positions",
            "  --target-profit <value>       Profit target passed to watch-open-positions",
            "  --stop-loss <value>           Stop-loss passed to watch-open-positions",
            "  -h, --help                    Show this help",
        ]
        .join("\n")
    }
}
