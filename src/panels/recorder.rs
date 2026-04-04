use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::domain::{RecorderEventSummary, VenueStatus};
use crate::recorder::RecorderField;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let layout = Layout::vertical([
        Constraint::Length(11),
        Constraint::Min(14),
        Constraint::Length(8),
    ])
    .split(area);
    let top = Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(layout[0]);
    let middle = Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(layout[1]);
    let right = Layout::vertical([Constraint::Length(10), Constraint::Min(7)]).split(middle[1]);
    let bottom = Layout::horizontal([Constraint::Percentage(54), Constraint::Percentage(46)])
        .split(layout[2]);

    render_status(frame, top[0], app);
    render_pipeline(frame, top[1], app);
    render_config_table(frame, middle[0], app);
    render_field_detail(frame, right[0], app);
    render_storage(frame, right[1], app);
    render_evidence(frame, bottom[0], app);
    render_policy(frame, bottom[1], app);
}

fn render_status(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let snapshot = app.snapshot();
    let selected_venue = snapshot
        .selected_venue
        .map(|venue| venue.as_str().to_string())
        .unwrap_or_else(|| String::from("none"));
    let latest_sync = latest_bookmaker_history_sync_summary(app);
    let latest_event = latest_recorder_event_summary(app);
    let rows = vec![
        key_value_row(
            "󰑓 Recorder",
            format!("{:?}", app.recorder_status()),
            accent_blue(),
        ),
        key_value_row(
            "󰐹 Lifecycle",
            app.recorder_lifecycle_state().to_string(),
            lifecycle_color(app.recorder_lifecycle_state()),
        ),
        key_value_row(
            "󰒋 Worker",
            format!("{:?}", snapshot.worker.status),
            worker_color(snapshot.worker.status),
        ),
        key_value_row(
            "󰄬 Snapshot",
            app.recorder_snapshot_freshness().to_string(),
            snapshot_freshness_color(app.recorder_snapshot_freshness()),
        ),
        key_value_row(
            "󰞋 Refresh",
            app.recorder_snapshot_mode().to_string(),
            accent_gold(),
        ),
        key_value_row("󱙺 Sync", latest_sync.0, latest_sync.1),
        key_value_row("󰀵 Venue", selected_venue, accent_cyan()),
        key_value_row("󰁨 Event", latest_event.0, latest_event.1),
    ];
    let table = Table::new(rows, [Constraint::Length(18), Constraint::Min(10)])
        .block(section_block("󰑓 Recorder Status", accent_blue()))
        .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_pipeline(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let snapshot = app.snapshot();
    let connected_venues = snapshot
        .venues
        .iter()
        .filter(|venue| matches!(venue.status, VenueStatus::Connected | VenueStatus::Ready))
        .count();
    let watch_rows = snapshot
        .watch
        .as_ref()
        .map(|watch| watch.watch_count)
        .unwrap_or(0);
    let rows = vec![
        pipeline_row(
            "󰄨 Venues ready",
            format!("{connected_venues}/{}", snapshot.venues.len()),
        ),
        pipeline_row(
            "󰞇 Exchange positions",
            snapshot.open_positions.len().to_string(),
        ),
        pipeline_row(
            "󰇚 Sportsbook bets",
            snapshot.other_open_bets.len().to_string(),
        ),
        pipeline_row("󰋼 Tracked bets", snapshot.tracked_bets.len().to_string()),
        pipeline_row("󰍵 Decisions", snapshot.decisions.len().to_string()),
        pipeline_row("󰄦 Watch rows", watch_rows.to_string()),
    ];
    let table = Table::new(rows, [Constraint::Length(24), Constraint::Length(10)])
        .header(Row::new(vec![
            Cell::from(Span::styled(
                "Stage",
                Style::default()
                    .fg(accent_green())
                    .add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Count",
                Style::default()
                    .fg(accent_green())
                    .add_modifier(Modifier::BOLD),
            )),
        ]))
        .block(section_block("󰇚 Capture Pipeline", accent_green()))
        .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_config_table(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let config = app.recorder_config();
    let selected_field = app.recorder_selected_field();
    let rows = RecorderField::ALL.into_iter().map(|field| {
        let mut value = field.display_value(config);
        if value.trim().is_empty() {
            value = String::from("<none>");
        }
        if field == selected_field && app.recorder_is_editing() {
            value = format!("{}_", app.recorder_edit_buffer().unwrap_or(value.as_str()));
        }

        let marker = if field == selected_field {
            if app.recorder_is_editing() {
                "󰏫"
            } else {
                "󰄬"
            }
        } else {
            "  "
        };

        Row::new(vec![
            Cell::from(format!("{marker} {}", field.label())),
            Cell::from(value),
        ])
        .style(if field == selected_field {
            Style::default()
                .fg(selected_text())
                .bg(selected_background())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(text_color())
        })
    });

    let table = Table::new(rows, [Constraint::Length(20), Constraint::Min(10)])
        .header(
            Row::new(vec!["Field", "Value"]).style(
                Style::default()
                    .fg(accent_cyan())
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(section_block("󰢻 Recorder Config", accent_cyan()))
        .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_field_detail(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let field = app.recorder_selected_field();
    let snapshot = app.snapshot();
    let current_value = if app.recorder_is_editing() {
        app.recorder_edit_buffer()
            .map(String::from)
            .unwrap_or_else(|| field.display_value(app.recorder_config()))
    } else {
        field.display_value(app.recorder_config())
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("󰏬 field ", Style::default().fg(muted_text())),
            Span::styled(
                field.label(),
                Style::default()
                    .fg(text_color())
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("󰞋 mode ", Style::default().fg(muted_text())),
            Span::raw(if app.recorder_is_editing() {
                "editing buffer"
            } else {
                "selection"
            }),
        ]),
        Line::from(vec![
            Span::styled("󰈈 current ", Style::default().fg(muted_text())),
            Span::raw(if current_value.trim().is_empty() {
                "<none>"
            } else {
                current_value.as_str()
            }),
        ]),
        Line::styled("󱂬 status", Style::default().fg(accent_blue())),
        Line::raw(truncate_line(app.status_message(), 76)),
    ];
    if snapshot.worker.detail != app.status_message() {
        lines.push(Line::styled(
            "󰒋 worker",
            Style::default().fg(accent_green()),
        ));
        lines.push(Line::raw(truncate_line(&snapshot.worker.detail, 76)));
    }
    if let Some(event) = bookmaker_history_sync_events(app).first() {
        lines.push(Line::styled(
            "󱙺 latest sync",
            Style::default().fg(accent_green()),
        ));
        lines.push(Line::raw(format_bookmaker_history_sync_event(event)));
    }
    if let Some(event) = snapshot
        .recorder_events
        .iter()
        .rev()
        .find(|event| event.kind != "bookmaker_history_sync")
    {
        lines.push(Line::styled(
            "󰁨 latest event",
            Style::default().fg(accent_red()),
        ));
        lines.push(Line::raw(format_recorder_evidence_event(event)));
    }
    lines.push(Line::raw("PgUp/PgDn scroll this pane"));

    let body = Paragraph::new(lines)
        .block(section_block("󰞋 Field Detail", accent_gold()))
        .scroll((app.status_scroll(), 0))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_storage(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let config = app.recorder_config();
    let mut lines = vec![
        key_value_line(
            "󰉋 Run dir",
            config.run_dir.display().to_string(),
            text_color(),
        ),
        key_value_line(
            "󰆍 Command",
            config.command.display().to_string(),
            text_color(),
        ),
        key_value_line("󰌘 Session", config.session.clone(), accent_cyan()),
        key_value_line(
            "󰙩 Profile",
            config
                .profile_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| String::from("<none>")),
            muted_text(),
        ),
        key_value_line(
            "󰎟 Companion",
            config
                .companion_legs_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| String::from("<none>")),
            muted_text(),
        ),
    ];
    let history_events = bookmaker_history_sync_events(app);
    if history_events.is_empty() {
        lines.push(key_value_line(
            "󱙺 Sync",
            String::from("No bookmaker history sync attempts captured yet."),
            muted_text(),
        ));
    } else {
        for event in history_events {
            lines.push(key_value_line(
                "󱙺 Sync",
                format_bookmaker_history_sync_event(event),
                accent_green(),
            ));
        }
    }

    let body = Paragraph::new(lines)
        .block(section_block("󰒓 Storage, Inputs & Sync", accent_pink()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_evidence(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let snapshot = app.snapshot();
    let mut lines = Vec::new();
    if let Some(bundle) = snapshot.recorder_bundle.as_ref() {
        lines.push(Line::raw(format!(
            "Bundle: {} | Events: {}",
            bundle.run_dir, bundle.event_count
        )));
        if !bundle.latest_event_at.trim().is_empty() || !bundle.latest_event_kind.trim().is_empty()
        {
            lines.push(Line::raw(format!(
                "Latest: {} {}",
                bundle.latest_event_kind, bundle.latest_event_at
            )));
        }
        if !bundle.latest_event_summary.trim().is_empty() {
            lines.push(Line::raw(format!(
                "Summary: {}",
                truncate_line(bundle.latest_event_summary.trim(), 96)
            )));
        }
    } else {
        lines.push(Line::raw(
            "No recorder bundle is attached to this snapshot.",
        ));
    }

    let recent_events: Vec<_> = snapshot.recorder_events.iter().rev().take(2).collect();
    if recent_events.is_empty() {
        lines.push(Line::raw("No normalized recorder events are available."));
    } else {
        for event in recent_events {
            lines.push(Line::raw(format_recorder_evidence_event(event)));
        }
    }

    let body = Paragraph::new(lines)
        .block(section_block("󰁨 Recorder Evidence", accent_red()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_policy(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let config = app.recorder_config();
    let rows = vec![
        key_value_row(
            "󰔟 Autostart",
            config.autostart.to_string(),
            bool_color(config.autostart),
        ),
        key_value_row(
            "󰔟 Interval",
            format!("{}s", config.interval_seconds),
            accent_cyan(),
        ),
        key_value_row(
            "󰔟 Warn only",
            config.warn_only_default.to_string(),
            bool_color(config.warn_only_default),
        ),
        key_value_row(
            "󰔟 Commission",
            config.commission_rate.to_string(),
            accent_gold(),
        ),
        key_value_row("󰔟 Target", config.target_profit.to_string(), accent_green()),
        key_value_row("󰔟 Stop", config.stop_loss.to_string(), accent_red()),
        key_value_row(
            "󰔟 Hard floor",
            if config.hard_margin_call_profit_floor.trim().is_empty() {
                String::from("<none>")
            } else {
                config.hard_margin_call_profit_floor.clone()
            },
            muted_text(),
        ),
    ];
    let table = Table::new(rows, [Constraint::Length(16), Constraint::Min(10)])
        .block(section_block("󰔟 Policy", accent_gold()))
        .column_spacing(1);
    frame.render_widget(table, area);
}

fn section_block(title: &'static str, color: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            format!(" {} ", title),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .border_style(Style::default().fg(border_color()))
}

fn pipeline_row(label: &'static str, value: String) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(label, Style::default().fg(muted_text()))),
        Cell::from(Span::styled(
            value,
            Style::default()
                .fg(text_color())
                .add_modifier(Modifier::BOLD),
        )),
    ])
}

fn key_value_row(label: &'static str, value: String, color: Color) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(
            label,
            Style::default()
                .fg(muted_text())
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(value, Style::default().fg(color))),
    ])
}

fn key_value_line(label: &'static str, value: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label} "),
            Style::default()
                .fg(muted_text())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(value, Style::default().fg(color)),
    ])
}

fn bookmaker_history_sync_events(app: &App) -> Vec<&RecorderEventSummary> {
    app.snapshot()
        .recorder_events
        .iter()
        .rev()
        .filter(|event| event.kind == "bookmaker_history_sync")
        .take(3)
        .collect()
}

fn format_bookmaker_history_sync_event(event: &RecorderEventSummary) -> String {
    let venue = first_non_empty(&[event.action.as_str(), event.source.as_str(), "unknown"]);
    let status = first_non_empty(&[event.status.as_str(), "unknown"]);
    let headline = if event.summary.trim().is_empty() {
        format!("{venue} [{status}]")
    } else {
        format!("{venue} [{status}] {}", event.summary.trim())
    };
    if event.detail.trim().is_empty() {
        truncate_line(&headline, 120)
    } else {
        truncate_line(&format!("{headline} | {}", event.detail.trim()), 120)
    }
}

fn latest_bookmaker_history_sync_summary(app: &App) -> (String, Color) {
    if let Some(event) = bookmaker_history_sync_events(app).first() {
        let color = match event.status.as_str() {
            "success" => accent_green(),
            "error" | "failed" => accent_red(),
            _ => accent_gold(),
        };
        (
            truncate_line(&format_bookmaker_history_sync_event(event), 48),
            color,
        )
    } else {
        (String::from("<none>"), muted_text())
    }
}

fn latest_recorder_event_summary(app: &App) -> (String, Color) {
    if let Some(event) = app
        .snapshot()
        .recorder_events
        .iter()
        .rev()
        .find(|event| event.kind != "bookmaker_history_sync")
    {
        (
            truncate_line(&format_recorder_evidence_event(event), 48),
            accent_red(),
        )
    } else if let Some(detail) = app.last_recorder_start_failure() {
        (truncate_line(detail, 48), accent_red())
    } else {
        (String::from("<none>"), muted_text())
    }
}

fn format_recorder_evidence_event(event: &RecorderEventSummary) -> String {
    let prefix = if event.captured_at.trim().is_empty() {
        event.kind.trim().to_string()
    } else {
        format!("{} {}", event.captured_at.trim(), event.kind.trim())
    };
    let mut line = if event.summary.trim().is_empty() {
        prefix
    } else {
        format!("{prefix} | {}", event.summary.trim())
    };
    if !event.detail.trim().is_empty() {
        line.push_str(&format!(" | {}", event.detail.trim()));
    }
    truncate_line(&line, 120)
}

fn first_non_empty<'a>(values: &[&'a str]) -> &'a str {
    values
        .iter()
        .copied()
        .find(|value| !value.trim().is_empty())
        .unwrap_or("")
}

fn truncate_line(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() && max_chars >= 3 {
        format!(
            "{}...",
            truncated.chars().take(max_chars - 3).collect::<String>()
        )
    } else {
        truncated
    }
}

fn bool_color(value: bool) -> Color {
    if value {
        accent_green()
    } else {
        muted_text()
    }
}

fn worker_color(status: crate::domain::WorkerStatus) -> Color {
    match status {
        crate::domain::WorkerStatus::Ready => accent_green(),
        crate::domain::WorkerStatus::Busy => accent_gold(),
        crate::domain::WorkerStatus::Idle => muted_text(),
        crate::domain::WorkerStatus::Error => accent_red(),
    }
}

fn lifecycle_color(state: &str) -> Color {
    match state {
        "running" => accent_green(),
        "stale" | "waiting" | "stopped" => accent_gold(),
        "failed" => accent_red(),
        _ => muted_text(),
    }
}

fn snapshot_freshness_color(state: &str) -> Color {
    match state {
        "fresh" => accent_green(),
        "stale" | "waiting" => accent_gold(),
        _ => muted_text(),
    }
}

fn panel_background() -> Color {
    crate::theme::panel_background()
}

fn border_color() -> Color {
    crate::theme::border_color()
}

fn text_color() -> Color {
    crate::theme::text_color()
}

fn muted_text() -> Color {
    crate::theme::muted_text()
}

fn accent_blue() -> Color {
    crate::theme::accent_blue()
}

fn accent_cyan() -> Color {
    crate::theme::accent_cyan()
}

fn accent_green() -> Color {
    crate::theme::accent_green()
}

fn accent_gold() -> Color {
    crate::theme::accent_gold()
}

fn accent_pink() -> Color {
    crate::theme::accent_pink()
}

fn accent_red() -> Color {
    crate::theme::accent_red()
}

fn selected_background() -> Color {
    crate::theme::selected_background()
}

fn selected_text() -> Color {
    crate::theme::selected_text()
}
