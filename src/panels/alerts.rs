use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::alerts::{AlertField, NotificationLevel};
use crate::app::App;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let layout = Layout::vertical([
        Constraint::Length(8),
        Constraint::Min(12),
        Constraint::Length(8),
    ])
    .split(area);
    let middle = Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(layout[1]);

    render_summary(frame, layout[0], app);
    render_config_table(frame, middle[0], app);
    render_recent_notifications(frame, middle[1], app);
    render_field_detail(frame, layout[2], app);
}

fn render_summary(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let config = app.alerts_config();
    let recent = app.notifications().len();
    let unread = app.unread_notification_count();
    let latest = app
        .notifications()
        .back()
        .map(|entry| format!("{} {}", entry.created_at, entry.title))
        .unwrap_or_else(|| String::from("No notifications yet."));
    let lines = vec![
        Line::from(vec![
            summary_item("state", bool_label(config.enabled), accent(config.enabled)),
            Span::raw("  "),
            summary_item(
                "desktop",
                bool_label(config.desktop_notifications),
                accent(config.desktop_notifications),
            ),
            Span::raw("  "),
            summary_item(
                "sound",
                bool_label(config.sound_effects),
                accent(config.sound_effects),
            ),
            Span::raw("  "),
            summary_item("unread", unread.to_string(), accent_blue()),
            Span::raw("  "),
            summary_item("recent", recent.to_string(), accent_gold()),
        ]),
        Line::from(vec![
            Span::styled("config ", Style::default().fg(muted_text())),
            Span::raw(truncate_line(app.alerts_config_note(), 90)),
        ]),
        Line::from(vec![
            Span::styled("latest ", Style::default().fg(muted_text())),
            Span::raw(truncate_line(&latest, 90)),
        ]),
        Line::from(vec![
            Span::styled("keys ", Style::default().fg(muted_text())),
            Span::raw("enter edit • [/] suggestion • u reload • D defaults • n inbox"),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(section_block("󰀨 Alerts", accent_blue()))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_config_table(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let config = app.alerts_config();
    let selected = app.alerts_selected_field();
    let rows = AlertField::ALL.into_iter().map(|field| {
        let marker = if field == selected {
            if app.alerts_is_editing() {
                "󰏫"
            } else {
                "󰄬"
            }
        } else {
            "  "
        };
        let mut value = field.display_value(config);
        if field == selected && app.alerts_is_editing() {
            value = format!("{}_", app.alerts_edit_buffer().unwrap_or(value.as_str()));
        }
        Row::new(vec![
            Cell::from(format!("{marker} {}", field.label())),
            Cell::from(value),
        ])
        .style(if field == selected {
            Style::default()
                .fg(selected_text())
                .bg(selected_background())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(text_color())
        })
    });

    frame.render_widget(
        Table::new(rows, [Constraint::Length(24), Constraint::Min(8)])
            .header(
                Row::new(vec!["Rule", "Value"]).style(
                    Style::default()
                        .fg(accent_cyan())
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .block(section_block("󰒓 Alert Rules", accent_cyan()))
            .column_spacing(1),
        area,
    );
}

fn render_recent_notifications(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = app.notifications().iter().rev().take(10).map(|entry| {
        let level_style = match entry.level {
            NotificationLevel::Info => Style::default().fg(accent_blue()),
            NotificationLevel::Warning => Style::default().fg(accent_gold()),
            NotificationLevel::Critical => Style::default().fg(accent_red()),
        };
        Row::new(vec![
            Cell::from(entry.created_at.clone()),
            Cell::from(Span::styled(
                entry.level.label(),
                level_style.add_modifier(Modifier::BOLD),
            )),
            Cell::from(truncate_line(&entry.title, 26)),
            Cell::from(truncate_line(&entry.detail, 46)),
        ])
    });

    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(11),
                Constraint::Length(6),
                Constraint::Length(28),
                Constraint::Min(18),
            ],
        )
        .header(
            Row::new(vec!["At", "Lvl", "Title", "Detail"]).style(
                Style::default()
                    .fg(accent_green())
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(section_block("󰎟 Recent Notifications", accent_green()))
        .column_spacing(1),
        area,
    );
}

fn render_field_detail(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let field = app.alerts_selected_field();
    let current = if app.alerts_is_editing() {
        app.alerts_edit_buffer()
            .map(String::from)
            .unwrap_or_else(|| field.display_value(app.alerts_config()))
    } else {
        field.display_value(app.alerts_config())
    };
    let suggestions = field.suggestions().join(", ");
    let lines = vec![
        Line::from(vec![
            Span::styled("field ", Style::default().fg(muted_text())),
            Span::styled(
                field.label(),
                Style::default()
                    .fg(text_color())
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("current ", Style::default().fg(muted_text())),
            Span::raw(current),
            Span::raw("  "),
            Span::styled("mode ", Style::default().fg(muted_text())),
            Span::raw(if app.alerts_is_editing() {
                "editing"
            } else {
                "selection"
            }),
        ]),
        Line::from(vec![
            Span::styled("about ", Style::default().fg(muted_text())),
            Span::raw(field.summary()),
        ]),
        Line::from(vec![
            Span::styled("suggest ", Style::default().fg(muted_text())),
            Span::raw(suggestions),
        ]),
        Line::from(vec![
            Span::styled("status ", Style::default().fg(muted_text())),
            Span::raw(truncate_line(app.status_message(), 92)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(section_block("󰏬 Alert Detail", accent_gold()))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn summary_item<'a>(label: &'static str, value: impl Into<String>, color: Color) -> Span<'a> {
    Span::styled(
        format!("{label}:{}", value.into()),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn truncate_line(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn bool_label(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

fn accent(value: bool) -> Color {
    if value {
        accent_green()
    } else {
        muted_text()
    }
}

fn section_block(title: &'static str, color: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            format!(" {} ", title),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color()))
        .style(Style::default().bg(panel_background()).fg(text_color()))
}

fn panel_background() -> Color {
    crate::theme::panel_background()
}

fn text_color() -> Color {
    crate::theme::text_color()
}

fn muted_text() -> Color {
    crate::theme::muted_text()
}

fn border_color() -> Color {
    crate::theme::border_color()
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

fn accent_red() -> Color {
    crate::theme::accent_red()
}

fn selected_background() -> Color {
    crate::theme::selected_background()
}

fn selected_text() -> Color {
    crate::theme::selected_text()
}
