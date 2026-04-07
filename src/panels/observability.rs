use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, ObservabilitySection};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, section: ObservabilitySection) {
    let layout = Layout::vertical([Constraint::Length(6), Constraint::Min(10)]).split(area);
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
            "Worker: {} [{:?}] | Recorder: {}",
            app.snapshot().worker.name,
            app.snapshot().worker.status,
            app.recorder_lifecycle_state()
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
        Line::raw(format!(
            "Refresh mode: {}",
            runtime
                .map(|summary| summary.refresh_kind.as_str())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("unknown")
        )),
        Line::raw(format!(
            "Worker reconnects: {}",
            app.worker_reconnect_count()
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
            "Recorder Evidence",
            recorder_evidence_lines(app),
            "Transport + Operator",
            transport_and_operator_lines(app),
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
        Line::raw(format!("Reconnects: {}", app.worker_reconnect_count())),
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
        Line::raw(format!(
            "Refresh mode: {}",
            if runtime.refresh_kind.trim().is_empty() {
                "unknown"
            } else {
                runtime.refresh_kind.as_str()
            }
        )),
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
        Line::raw(format!(
            "Reconnect count: {}",
            runtime.worker_reconnect_count
        )),
        Line::raw(format!(
            "Bundle events: {}",
            app.snapshot()
                .recorder_bundle
                .as_ref()
                .map(|bundle| bundle.event_count)
                .unwrap_or(0)
        )),
        Line::raw(format!(
            "Transport markers: {}",
            app.snapshot()
                .transport_summary
                .as_ref()
                .map(|summary| summary.marker_count)
                .unwrap_or(0)
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

fn recorder_evidence_lines(app: &App) -> Vec<Line<'static>> {
    let mut rows = Vec::new();

    if let Some(bundle) = app.snapshot().recorder_bundle.as_ref() {
        rows.push(Line::raw(format!("Run dir: {}", bundle.run_dir)));
        rows.push(Line::raw(format!("Event count: {}", bundle.event_count)));
        rows.push(Line::raw(format!(
            "Latest event: {} {}",
            bundle.latest_event_kind, bundle.latest_event_at
        )));
        if !bundle.latest_event_summary.trim().is_empty() {
            rows.push(Line::raw(format!(
                "Latest summary: {}",
                bundle.latest_event_summary
            )));
        }
        if !bundle.latest_positions_at.trim().is_empty() {
            rows.push(Line::raw(format!(
                "Latest positions snapshot: {}",
                bundle.latest_positions_at
            )));
        }
        if !bundle.latest_watch_plan_at.trim().is_empty() {
            rows.push(Line::raw(format!(
                "Latest watch plan: {}",
                bundle.latest_watch_plan_at
            )));
        }
    } else {
        rows.push(Line::raw(
            "No recorder bundle is attached to this snapshot.",
        ));
    }

    if app.snapshot().recorder_events.is_empty() {
        rows.push(Line::raw("No normalized recorder events are available."));
        return rows;
    }

    rows.push(Line::raw(String::new()));
    for event in &app.snapshot().recorder_events {
        let prefix = if event.captured_at.trim().is_empty() {
            event.kind.clone()
        } else {
            format!("{} {}", event.captured_at, event.kind)
        };
        let mut line = format!("- {} | {}", prefix.trim(), event.summary);
        if !event.detail.trim().is_empty() {
            line.push_str(&format!(" | {}", event.detail));
        }
        rows.push(Line::raw(line));
    }

    rows
}

fn transport_and_operator_lines(app: &App) -> Vec<Line<'static>> {
    let mut rows = vec![
        Line::raw(format!(
            "Recorder lifecycle: {}",
            app.recorder_lifecycle_state()
        )),
        Line::raw(format!(
            "Worker reconnect count: {}",
            app.worker_reconnect_count()
        )),
        Line::raw(format!(
            "Last successful snapshot: {}",
            app.last_successful_snapshot_at().unwrap_or("none")
        )),
        Line::raw(format!(
            "Current refresh mode: {}",
            app.recorder_snapshot_mode()
        )),
    ];

    if let Some(detail) = app.last_recorder_start_failure() {
        rows.push(Line::raw(format!("Last startup failure: {detail}")));
    } else {
        rows.push(Line::raw("Last startup failure: <none>"));
    }

    rows.push(Line::raw(format!(
        "Current worker detail: {}",
        app.snapshot().worker.detail
    )));

    rows.push(Line::raw(String::new()));
    if let Some(summary) = app.snapshot().transport_summary.as_ref() {
        rows.push(Line::raw(format!(
            "Transport path: {}",
            summary.transport_path
        )));
        rows.push(Line::raw(format!(
            "Transport markers: {}",
            summary.marker_count
        )));
        rows.push(Line::raw(format!(
            "Latest transport marker: {} {} {}",
            summary.latest_marker_phase, summary.latest_marker_action, summary.latest_marker_at
        )));
        if !summary.latest_marker_summary.trim().is_empty() {
            rows.push(Line::raw(format!(
                "Latest marker summary: {}",
                summary.latest_marker_summary
            )));
        }
    } else {
        rows.push(Line::raw(
            "No transport marker summary is attached to this snapshot.",
        ));
    }

    if app.snapshot().transport_events.is_empty() {
        rows.push(Line::raw("No transport markers are available."));
    } else {
        for event in &app.snapshot().transport_events {
            let prefix = if event.captured_at.trim().is_empty() {
                format!("{} {}", event.phase, event.action)
            } else {
                format!("{} {} {}", event.captured_at, event.phase, event.action)
            };
            let mut line = format!("- {} | {}", prefix.trim(), event.summary);
            if !event.detail.trim().is_empty() {
                line.push_str(&format!(" | {}", event.detail));
            }
            rows.push(Line::raw(line));
        }
    }

    let events = app.recent_events();
    if events.is_empty() {
        rows.push(Line::raw("No operator events recorded yet."));
    } else {
        rows.push(Line::raw(String::new()));
        for event in events {
            rows.push(Line::raw(format!("- {event}")));
        }
    }
    rows
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
        Line::raw(format!(
            "Recorder lifecycle: {}",
            app.recorder_lifecycle_state()
        )),
        Line::raw(format!(
            "Refresh mode: {}",
            runtime
                .map(|summary| summary.refresh_kind.as_str())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("unknown")
        )),
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
    if app.last_recorder_start_failure().is_some() {
        rows.push(Line::raw(
            "Fix the recorder startup failure before relying on auto-refresh.",
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
        .block(
            Block::default()
                .title(Span::styled(
                    format!(" {} ", title),
                    Style::default()
                        .fg(crate::theme::accent_blue())
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::TOP)
                .style(
                    Style::default()
                        .bg(crate::theme::panel_background())
                        .fg(crate::theme::text_color()),
                )
                .border_style(Style::default().fg(crate::theme::border_color())),
        )
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
fn log_text(app: &App) -> Vec<String> {
    recorder_evidence_lines(app)
        .into_iter()
        .chain(transport_and_operator_lines(app))
        .map(|line| line.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{health_text, log_text};
    use crate::app::App;
    use crate::domain::{
        ExchangePanelSnapshot, RecorderBundleSummary, RecorderEventSummary, RuntimeSummary,
        TransportCaptureSummary, TransportMarkerSummary, WorkerStatus, WorkerSummary,
    };
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
                    refresh_kind: String::from("cached"),
                    worker_reconnect_count: 2,
                    decision_count: 4,
                    watcher_iteration: Some(14),
                    stale: true,
                }),
                recorder_bundle: Some(RecorderBundleSummary {
                    run_dir: String::from("/tmp/sabi-run"),
                    event_count: 3,
                    latest_event_at: String::from("2026-03-11T15:00:00Z"),
                    latest_event_kind: String::from("action_snapshot"),
                    latest_event_summary: String::from("place_order Draw -> submitted"),
                    latest_positions_at: String::from("2026-03-11T14:58:00Z"),
                    latest_watch_plan_at: String::from("2026-03-11T14:59:00Z"),
                }),
                recorder_events: vec![RecorderEventSummary {
                    captured_at: String::from("2026-03-11T15:00:00Z"),
                    kind: String::from("action_snapshot"),
                    source: String::from("smarkets_exchange"),
                    page: String::from("betslip"),
                    action: String::new(),
                    status: String::new(),
                    request_id: String::new(),
                    reference_id: String::new(),
                    summary: String::from("place_order Draw -> submitted"),
                    detail: String::from("https://smarkets.com/betslip"),
                }],
                transport_summary: Some(TransportCaptureSummary {
                    transport_path: String::from("/tmp/sabi-run/transport.jsonl"),
                    marker_count: 2,
                    latest_marker_at: String::from("2026-03-11T15:00:01Z"),
                    latest_marker_action: String::from("place_bet"),
                    latest_marker_phase: String::from("response"),
                    latest_marker_summary: String::from("response place_bet req-1 bet-1"),
                }),
                transport_events: vec![TransportMarkerSummary {
                    captured_at: String::from("2026-03-11T15:00:01Z"),
                    kind: String::from("interaction_marker"),
                    action: String::from("place_bet"),
                    phase: String::from("response"),
                    request_id: String::from("req-1"),
                    reference_id: String::from("bet-1"),
                    summary: String::from("response place_bet req-1 bet-1"),
                    detail: String::from("loaded in review mode"),
                }],
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
            .any(|line| line.contains("Refresh mode: cached")));
        assert!(text
            .iter()
            .any(|line| line.contains("Investigate worker failure")));
    }

    #[test]
    fn logs_section_mentions_recent_events_and_restart_history() {
        let app = App::from_provider(FixedProvider).expect("fixed provider should initialize");
        let text = log_text(&app);

        assert!(text.iter().any(|line| line.contains("Event count: 3")));
        assert!(text
            .iter()
            .any(|line| line.contains("place_order Draw -> submitted")));
        assert!(text
            .iter()
            .any(|line| line.contains("Transport markers: 2")));
        assert!(text
            .iter()
            .any(|line| line.contains("response place_bet req-1 bet-1")));
        assert!(text
            .iter()
            .any(|line| line.contains("Worker reconnect count: 2")));
        assert!(text
            .iter()
            .any(|line| line.contains("Last successful snapshot: 2026-03-11T15:00:00Z")));
    }
}
