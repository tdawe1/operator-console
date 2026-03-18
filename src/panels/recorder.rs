use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::domain::VenueStatus;
use crate::recorder::RecorderField;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let layout = Layout::vertical([
        Constraint::Length(7),
        Constraint::Min(14),
        Constraint::Length(9),
    ])
    .split(area);
    let top = Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(layout[0]);
    let middle = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(layout[1]);
    let right = Layout::vertical([Constraint::Length(8), Constraint::Min(6)]).split(middle[1]);

    render_status(frame, top[0], app);
    render_pipeline(frame, top[1], app);
    render_config_table(frame, middle[0], app);
    render_field_detail(frame, right[0], app);
    render_storage(frame, right[1], app);
    render_runbook(frame, layout[2], app);
}

fn render_status(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let snapshot = app.snapshot();
    let selected_venue = snapshot
        .selected_venue
        .map(|venue| venue.as_str().to_string())
        .unwrap_or_else(|| String::from("none"));
    let body = Paragraph::new(vec![
        Line::raw(format!("Recorder: {:?}", app.recorder_status())),
        Line::raw(format!("Worker: {:?} | Selected venue: {selected_venue}", snapshot.worker.status)),
        Line::raw(format!("Status line: {}", snapshot.status_line)),
        Line::raw(format!(
            "Edit mode: {} | Selected field: {}",
            if app.recorder_is_editing() {
                "editing"
            } else {
                "navigation"
            },
            app.recorder_selected_field().label(),
        )),
        Line::raw(format!("Config note: {}", app.recorder_config_note())),
    ])
    .block(section_block("Recorder Status", accent_blue()))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_pipeline(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let snapshot = app.snapshot();
    let connected_venues = snapshot
        .venues
        .iter()
        .filter(|venue| matches!(venue.status, VenueStatus::Connected | VenueStatus::Ready))
        .count();
    let body = Paragraph::new(vec![
        Line::raw(format!(
            "Venues ready: {connected_venues}/{} | Exchange positions: {} | Sportsbook bets: {}",
            snapshot.venues.len(),
            snapshot.open_positions.len(),
            snapshot.other_open_bets.len(),
        )),
        Line::raw(format!(
            "Tracked bets: {} | Watch rows: {} | Decisions: {}",
            snapshot.tracked_bets.len(),
            snapshot
                .watch
                .as_ref()
                .map(|watch| watch.watch_count)
                .unwrap_or(0),
            snapshot.decisions.len(),
        )),
        Line::raw("Positions come from the recorder watcher."),
        Line::raw("Sportsbook tabs are probed from the shared browser session."),
        Line::raw("Use this panel to own the capture config, not just start or stop it."),
    ])
    .block(section_block("Capture Pipeline", accent_green()))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
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
                "*"
            } else {
                ">"
            }
        } else {
            " "
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
        .block(section_block("Recorder Config", accent_cyan()))
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
        Line::raw(format!("Field: {}", field.label())),
        Line::raw(format!(
            "Mode: {}",
            if app.recorder_is_editing() {
                "editing buffer"
            } else {
                "selection"
            }
        )),
        Line::raw(format!(
            "Current: {}",
            if current_value.trim().is_empty() {
                "<none>"
            } else {
                current_value.as_str()
            }
        )),
        Line::raw(format!("Suggestions: {}", suggestions.join(" | "))),
    ])
    .block(section_block("Field Detail", accent_gold()))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_storage(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let config = app.recorder_config();
    let body = Paragraph::new(vec![
        Line::raw(format!("Config file: {}", app.recorder_config_path().display())),
        Line::raw(format!("Run dir: {}", config.run_dir.display())),
        Line::raw(format!("Command: {}", config.command.display())),
        Line::raw(format!("Session: {}", config.session)),
        Line::raw(format!(
            "Profile: {}",
            config
                .profile_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| String::from("<none>"))
        )),
        Line::raw(format!(
            "Companion legs: {}",
            config
                .companion_legs_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| String::from("<none>"))
        )),
    ])
    .block(section_block("Storage & Inputs", accent_pink()))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_runbook(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let config = app.recorder_config();
    let body = Paragraph::new(vec![
        Line::raw("j/k move field | Enter edit/apply | Esc cancel | [/] cycle suggestions"),
        Line::raw("s start recorder | x stop recorder | u reload config | D defaults | r refresh"),
        Line::raw(format!(
            "Autostart {} | Interval {}s | Warn only {}",
            config.autostart, config.interval_seconds, config.warn_only_default
        )),
        Line::raw(format!(
            "Commission {} | Target {} | Stop {} | Hard floor {}",
            config.commission_rate,
            config.target_profit,
            config.stop_loss,
            if config.hard_margin_call_profit_floor.trim().is_empty() {
                "<none>"
            } else {
                config.hard_margin_call_profit_floor.as_str()
            }
        )),
        Line::raw("Start and stop are global. You can trigger them from any panel."),
    ])
    .block(section_block("Recorder Runbook", accent_red()))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn section_block(title: &'static str, color: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .border_style(Style::default().fg(border_color()))
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
