use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Padding, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::domain::VenueStatus;
use crate::recorder::RecorderField;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let layout = Layout::vertical([
        Constraint::Length(9),
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
    render_runbook(frame, bottom[0], app);
    render_policy(frame, bottom[1], app);
}

fn render_status(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let snapshot = app.snapshot();
    let selected_venue = snapshot
        .selected_venue
        .map(|venue| venue.as_str().to_string())
        .unwrap_or_else(|| String::from("none"));
    let rows = vec![
        key_value_row(
            "󰑓 Recorder",
            format!("{:?}", app.recorder_status()),
            accent_blue(),
        ),
        key_value_row(
            "󰒋 Worker",
            format!("{:?}", snapshot.worker.status),
            worker_color(snapshot.worker.status),
        ),
        key_value_row("󰀵 Venue", selected_venue, accent_cyan()),
        key_value_row(
            "󰞋 Mode",
            if app.recorder_is_editing() {
                String::from("editing buffer")
            } else {
                String::from("field navigation")
            },
            accent_gold(),
        ),
        key_value_row(
            "󰏬 Selected",
            app.recorder_selected_field().label().to_string(),
            text_color(),
        ),
        key_value_row(
            "󱂬 Note",
            app.recorder_config_note().to_string(),
            muted_text(),
        ),
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
                .fg(Color::Black)
                .bg(accent_cyan())
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
    let mut suggestions = field.suggestions();
    if suggestions.is_empty() {
        suggestions.push(String::from("<none>"));
    }
    let current_value = if app.recorder_is_editing() {
        app.recorder_edit_buffer()
            .map(String::from)
            .unwrap_or_else(|| field.display_value(app.recorder_config()))
    } else {
        field.display_value(app.recorder_config())
    };
    let body = Paragraph::new(vec![
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
        Line::styled("󰘵 suggestions", Style::default().fg(accent_gold())),
        Line::raw(format!(
            "• {}",
            suggestions.first().map(String::as_str).unwrap_or("<none>")
        )),
        Line::raw(format!(
            "• {}",
            suggestions.get(1).map(String::as_str).unwrap_or("<none>")
        )),
        Line::raw(format!(
            "• {}",
            suggestions.get(2).map(String::as_str).unwrap_or("<none>")
        )),
    ])
    .block(section_block("󰞋 Field Detail", accent_gold()))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_storage(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let config = app.recorder_config();
    let rows = vec![
        key_value_row(
            "󰈔 Config file",
            app.recorder_config_path().display().to_string(),
            text_color(),
        ),
        key_value_row(
            "󰉋 Run dir",
            config.run_dir.display().to_string(),
            text_color(),
        ),
        key_value_row(
            "󰆍 Command",
            config.command.display().to_string(),
            text_color(),
        ),
        key_value_row("󰌘 Session", config.session.clone(), accent_cyan()),
        key_value_row(
            "󰙩 Profile",
            config
                .profile_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| String::from("<none>")),
            muted_text(),
        ),
        key_value_row(
            "󰎟 Companion",
            config
                .companion_legs_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| String::from("<none>")),
            muted_text(),
        ),
    ];
    let table = Table::new(rows, [Constraint::Length(18), Constraint::Min(10)])
        .block(section_block("󰒓 Storage & Inputs", accent_pink()))
        .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_runbook(frame: &mut Frame<'_>, area: Rect, _app: &App) {
    let rows = vec![
        action_row("󰮳 Navigate", "j/k move field"),
        action_row("󰌑 Edit", "Enter apply • Esc cancel • [/] cycle suggestions"),
        action_row("󰑓 Control", "s start recorder • x stop recorder"),
        action_row("󰑓 Config", "u reload config • D defaults • r refresh"),
    ];
    let table = Table::new(rows, [Constraint::Length(14), Constraint::Min(10)])
        .block(section_block("󰌑 Recorder Runbook", accent_red()))
        .column_spacing(1);
    frame.render_widget(table, area);
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
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .padding(Padding::horizontal(1))
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

fn action_row(label: &'static str, value: &'static str) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(
            label,
            Style::default()
                .fg(muted_text())
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(value, Style::default().fg(text_color()))),
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

fn panel_background() -> Color {
    Color::Rgb(16, 22, 30)
}

fn border_color() -> Color {
    Color::Rgb(48, 64, 86)
}

fn text_color() -> Color {
    Color::Rgb(234, 240, 246)
}

fn muted_text() -> Color {
    Color::Rgb(148, 163, 184)
}

fn accent_blue() -> Color {
    Color::Rgb(104, 179, 255)
}

fn accent_cyan() -> Color {
    Color::Rgb(110, 231, 255)
}

fn accent_green() -> Color {
    Color::Rgb(90, 214, 154)
}

fn accent_gold() -> Color {
    Color::Rgb(245, 196, 89)
}

fn accent_pink() -> Color {
    Color::Rgb(255, 122, 162)
}

fn accent_red() -> Color {
    Color::Rgb(255, 107, 107)
}
