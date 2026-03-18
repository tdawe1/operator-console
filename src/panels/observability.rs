use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, ObservabilitySection};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, section: ObservabilitySection) {
    let layout = Layout::vertical([Constraint::Length(5), Constraint::Min(10)]).split(area);
    let lower = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(layout[1]);

    render_block(
        frame,
        layout[0],
        "Observability Summary",
        summary_lines(app, section),
    );
    let (left_title, left_rows, right_title, right_rows) = section_blocks(app, section);
    render_block(frame, lower[0], left_title, left_rows);
    render_block(frame, lower[1], right_title, right_rows);
}

fn summary_lines(app: &App, section: ObservabilitySection) -> Vec<Line<'static>> {
    let runtime = app.snapshot().runtime.as_ref();
    vec![
        Line::raw(format!("Section: {}", section.label())),
        Line::raw(format!(
            "Worker: {} [{:?}] | Recorder: {:?}",
            app.snapshot().worker.name,
            app.snapshot().worker.status,
            app.recorder_status()
        )),
        Line::raw(format!(
            "Updated: {} | Source: {}",
            runtime
                .map(|summary| summary.updated_at.as_str())
                .unwrap_or("unknown"),
            runtime
                .map(|summary| summary.source.as_str())
                .unwrap_or("snapshot")
        )),
    ]
}

fn section_blocks(
    app: &App,
    section: ObservabilitySection,
) -> (
    &'static str,
    Vec<Line<'static>>,
    &'static str,
    Vec<Line<'static>>,
) {
    match section {
        ObservabilitySection::Workers => (
            "Worker Detail",
            worker_lines(app),
            "Venue Status",
            venue_lines(app),
        ),
        ObservabilitySection::Watchers => (
            "Watcher Runtime",
            watcher_lines(app),
            "Watch Coverage",
            watch_coverage_lines(app),
        ),
        ObservabilitySection::Configs => (
            "Recorder Config",
            config_lines(app),
            "Current Paths",
            path_lines(app),
        ),
        ObservabilitySection::Logs => (
            "Recent Status",
            log_lines(app),
            "Operator Notes",
            operator_note_lines(),
        ),
        ObservabilitySection::Health => (
            "Health Summary",
            health_lines(app),
            "Recommended Actions",
            recommended_action_lines(app),
        ),
    }
}

fn worker_lines(app: &App) -> Vec<Line<'static>> {
    vec![
        Line::raw(format!("Name: {}", app.snapshot().worker.name)),
        Line::raw(format!("Status: {:?}", app.snapshot().worker.status)),
        Line::raw(format!("Detail: {}", app.snapshot().worker.detail)),
        Line::raw(format!("Status line: {}", app.snapshot().status_line)),
    ]
}

fn venue_lines(app: &App) -> Vec<Line<'static>> {
    if app.snapshot().venues.is_empty() {
        return vec![Line::raw("No venues are loaded.")];
    }

    app.snapshot()
        .venues
        .iter()
        .map(|venue| {
            Line::raw(format!(
                "{} [{}] {:?} | events {} | markets {}",
                venue.label,
                venue.id.as_str(),
                venue.status,
                venue.event_count,
                venue.market_count
            ))
        })
        .collect()
}

fn watcher_lines(app: &App) -> Vec<Line<'static>> {
    let Some(runtime) = app.snapshot().runtime.as_ref() else {
        return vec![
            Line::raw("No runtime summary is present."),
            Line::raw("Watcher state will appear here once recorder-backed data is loaded."),
        ];
    };

    vec![
        Line::raw(format!("Updated at: {}", runtime.updated_at)),
        Line::raw(format!("Source: {}", runtime.source)),
        Line::raw(format!("Decisions: {}", runtime.decision_count)),
        Line::raw(format!(
            "Iteration: {}",
            runtime
                .watcher_iteration
                .map(|value| value.to_string())
                .unwrap_or_else(|| String::from("-"))
        )),
        Line::raw(format!(
            "Stale: {}",
            if runtime.stale { "yes" } else { "no" }
        )),
        decision_preview_line(app),
    ]
}

fn watch_coverage_lines(app: &App) -> Vec<Line<'static>> {
    let watch_count = app
        .snapshot()
        .watch
        .as_ref()
        .map(|watch| watch.watch_count)
        .unwrap_or(0);

    vec![
        Line::raw(format!("Grouped watch rows: {}", watch_count)),
        Line::raw(format!(
            "Decision queue: {}",
            app.snapshot().decisions.len()
        )),
        Line::raw(format!(
            "Open positions: {}",
            app.snapshot().open_positions.len()
        )),
        Line::raw(format!(
            "Other open bets: {}",
            app.snapshot().other_open_bets.len()
        )),
        Line::raw(format!("Recorder status: {:?}", app.recorder_status())),
    ]
}

fn decision_preview_line(app: &App) -> Line<'static> {
    let ready = app
        .snapshot()
        .decisions
        .iter()
        .filter(|decision| {
            decision.status == "take_profit_ready" || decision.status == "stop_loss_ready"
        })
        .count();
    let holds = app
        .snapshot()
        .decisions
        .iter()
        .filter(|decision| decision.status == "hold")
        .count();
    let monitor_only = app
        .snapshot()
        .decisions
        .iter()
        .filter(|decision| decision.status == "monitor_only")
        .count();

    Line::raw(format!(
        "Ready: {} | Hold: {} | Monitor only: {}",
        ready, holds, monitor_only
    ))
}

fn config_lines(app: &App) -> Vec<Line<'static>> {
    let config = app.recorder_config();
    vec![
        Line::raw(format!("Command: {}", config.command.display())),
        Line::raw(format!("Run dir: {}", config.run_dir.display())),
        Line::raw(format!("Session: {}", config.session)),
        Line::raw(format!(
            "Companion legs: {}",
            config
                .companion_legs_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| String::from("<none>"))
        )),
        Line::raw(format!("Interval: {}s", config.interval_seconds)),
        Line::raw(format!(
            "Commission {} | Profit {} | Stop {}",
            config.commission_rate, config.target_profit, config.stop_loss
        )),
        Line::raw(format!(
            "Hard floor {} | Warn only {}",
            if config.hard_margin_call_profit_floor.trim().is_empty() {
                String::from("<none>")
            } else {
                config.hard_margin_call_profit_floor.clone()
            },
            config.warn_only_default
        )),
    ]
}

fn path_lines(app: &App) -> Vec<Line<'static>> {
    vec![
        Line::raw(format!(
            "Recorder config file: {}",
            app.recorder_config_path().display()
        )),
        Line::raw(format!("Config note: {}", app.recorder_config_note())),
        Line::raw(String::from(
            "Recorder values are editable from Trading > Recorder.",
        )),
    ]
}

fn log_lines(app: &App) -> Vec<Line<'static>> {
    let mut rows = vec![
        Line::raw(format!("UI status: {}", app.status_message())),
        Line::raw(format!("Worker detail: {}", app.snapshot().worker.detail)),
        Line::raw(format!("Snapshot status: {}", app.snapshot().status_line)),
    ];

    if let Some(runtime) = app.snapshot().runtime.as_ref() {
        rows.push(Line::raw(format!(
            "Runtime event: updated {} from {}",
            runtime.updated_at, runtime.source
        )));
    }

    rows
}

fn operator_note_lines() -> Vec<Line<'static>> {
    vec![
        Line::raw("This pane is currently summary-driven, not a live log tail."),
        Line::raw("Next step: attach event history, worker restart history, and error feed."),
    ]
}

fn health_lines(app: &App) -> Vec<Line<'static>> {
    let runtime = app.snapshot().runtime.as_ref();
    let worker_health = if matches!(
        app.snapshot().worker.status,
        crate::domain::WorkerStatus::Error
    ) {
        "degraded"
    } else {
        "ok"
    };
    let freshness = if runtime.map(|summary| summary.stale).unwrap_or(false) {
        "stale"
    } else {
        "fresh"
    };

    vec![
        Line::raw(format!("Worker health: {}", worker_health)),
        Line::raw(format!("Snapshot freshness: {}", freshness)),
        Line::raw(format!("Recorder process: {:?}", app.recorder_status())),
        Line::raw(format!(
            "Selected venue: {}",
            app.snapshot()
                .selected_venue
                .map(|venue| venue.as_str().to_string())
                .unwrap_or_else(|| String::from("none"))
        )),
    ]
}

fn recommended_action_lines(app: &App) -> Vec<Line<'static>> {
    let runtime = app.snapshot().runtime.as_ref();
    let mut rows = Vec::new();

    if matches!(
        app.snapshot().worker.status,
        crate::domain::WorkerStatus::Error
    ) {
        rows.push(Line::raw(
            "Investigate worker failure before trusting the trading snapshot.",
        ));
    }
    if runtime.map(|summary| summary.stale).unwrap_or(false) {
        rows.push(Line::raw(
            "Refresh or restart recorder to clear stale data.",
        ));
    }
    if rows.is_empty() {
        rows.push(Line::raw("No immediate operator action required."));
    }

    rows.push(Line::raw(String::new()));
    rows.push(Line::raw("Use Trading > Recorder for lifecycle changes."));
    rows.push(Line::raw(
        "Use Trading > Positions for live exit monitoring.",
    ));
    rows
}

fn render_block(frame: &mut Frame<'_>, area: Rect, title: &str, rows: Vec<Line<'static>>) {
    let body = Paragraph::new(rows)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

#[cfg(test)]
fn health_text(app: &App) -> Vec<String> {
    health_lines(app)
        .into_iter()
        .chain(recommended_action_lines(app))
        .map(|line| line.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::health_text;
    use crate::app::App;
    use crate::domain::{ExchangePanelSnapshot, RuntimeSummary, WorkerStatus, WorkerSummary};
    use crate::provider::{ExchangeProvider, ProviderRequest};
    use color_eyre::Result;

    struct FixedProvider;

    impl ExchangeProvider for FixedProvider {
        fn handle(&mut self, _request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
            Ok(ExchangePanelSnapshot {
                worker: WorkerSummary {
                    name: String::from("bet-recorder"),
                    status: WorkerStatus::Error,
                    detail: String::from("worker exited"),
                },
                runtime: Some(RuntimeSummary {
                    updated_at: String::from("2026-03-11T15:00:00Z"),
                    source: String::from("watcher-state"),
                    decision_count: 4,
                    watcher_iteration: Some(14),
                    stale: true,
                }),
                status_line: String::from("refresh failed"),
                ..ExchangePanelSnapshot::default()
            })
        }
    }

    #[test]
    fn health_section_mentions_worker_and_stale_state() {
        let app = App::from_provider(FixedProvider).expect("fixed provider should initialize");
        let text = health_text(&app);

        assert!(text
            .iter()
            .any(|line| line.contains("Worker health: degraded")));
        assert!(text
            .iter()
            .any(|line| line.contains("Snapshot freshness: stale")));
        assert!(text
            .iter()
            .any(|line| line.contains("Investigate worker failure")));
    }
}
