use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

use crate::app::{App, OddsMatcherFocus};
use crate::oddsmatcher::OddsMatcherRow;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let layout = Layout::horizontal([
        Constraint::Length(38),
        Constraint::Min(48),
        Constraint::Length(36),
    ])
    .split(area);
    let rows = app.oddsmatcher_rows().to_vec();

    render_filters(frame, layout[0], app);
    render_table(frame, layout[1], &rows, app.oddsmatcher_table_state());
    render_details(frame, layout[2], app.selected_oddsmatcher_row(), app.status_message());
}

fn render_filters(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let header = vec![
        Line::from(vec![
            metric("Rows", accent_blue()),
            Span::raw(app.oddsmatcher_rows().len().to_string()),
            Span::raw("  "),
            metric("Focus", accent_green()),
            Span::raw(match app.oddsmatcher_focus() {
                OddsMatcherFocus::Filters => "filters",
                OddsMatcherFocus::Results => "results",
            }),
        ]),
        Line::from(vec![
            metric("Editing", accent_cyan()),
            Span::raw(if app.oddsmatcher_is_editing() {
                "yes"
            } else {
                "no"
            }),
        ]),
        Line::raw("left/right change focus, enter edits, [/] suggestions, r refresh"),
        Line::raw(app.oddsmatcher_query_note().to_string()),
        Line::raw(""),
    ];

    let field_lines = app
        .oddsmatcher_field_rows()
        .into_iter()
        .map(|(field, value, selected)| {
            let prefix = if selected { ">" } else { " " };
            let value = if selected && app.oddsmatcher_is_editing() {
                format!("{}_", app.oddsmatcher_edit_buffer().unwrap_or(value.as_str()))
            } else if value.is_empty() {
                String::from("-")
            } else {
                value
            };

            let style = if selected && app.oddsmatcher_focus() == OddsMatcherFocus::Filters {
                Style::default()
                    .fg(Color::Black)
                    .bg(accent_gold())
                    .add_modifier(Modifier::BOLD)
            } else if selected {
                Style::default().fg(accent_gold())
            } else {
                Style::default().fg(text_color())
            };

            Line::from(vec![
                Span::styled(format!("{prefix} {:<16}", field.label()), style),
                Span::styled(value, style),
            ])
        })
        .collect::<Vec<_>>();

    let body = Paragraph::new(header.into_iter().chain(field_lines).collect::<Vec<_>>())
        .block(section_block("Filters", accent_blue()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_table(
    frame: &mut Frame<'_>,
    area: Rect,
    rows: &[OddsMatcherRow],
    table_state: &mut TableState,
) {
    let header = Row::new(vec![
        Cell::from("Event"),
        Cell::from("Selection"),
        Cell::from("Back"),
        Cell::from("Lay"),
        Cell::from("Rating"),
        Cell::from("Avail."),
    ])
    .style(
        Style::default()
            .fg(Color::Black)
            .bg(accent_cyan())
            .add_modifier(Modifier::BOLD),
    );

    let body_rows = rows.iter().map(|row| {
        Row::new(vec![
            Cell::from(row.event_name.clone()),
            Cell::from(row.selection_name.clone()),
            Cell::from(format!("{:.2}", row.back.odds)),
            Cell::from(format!("{:.2}", row.lay.odds)),
            Cell::from(format!("{:.2}", row.rating)),
            Cell::from(row.availability_label()),
        ])
    });

    let table = Table::new(
        body_rows,
        [
            Constraint::Percentage(38),
            Constraint::Percentage(22),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(section_block("Live Matches", accent_green()))
    .row_highlight_style(
        Style::default()
            .bg(accent_blue())
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol(if rows.is_empty() { "  " } else { ">>" });

    frame.render_stateful_widget(table, area, table_state);
}

fn render_details(
    frame: &mut Frame<'_>,
    area: Rect,
    row: Option<&OddsMatcherRow>,
    status_message: &str,
) {
    let lines = if let Some(row) = row {
        vec![
            Line::raw(format!("Event: {}", row.event_name)),
            Line::raw(format!("Selection: {}", row.selection_name)),
            Line::raw(format!("Sport: {}", row.sport.display_name)),
            Line::raw(format!("Market: {}", row.market_name)),
            Line::raw(format!("Start: {}", row.start_at)),
            Line::raw(format!(
                "Bookie: {} @ {:.2}",
                row.back.bookmaker.display_name, row.back.odds
            )),
            Line::raw(format!(
                "Exchange: {} @ {:.2}",
                row.lay.bookmaker.display_name, row.lay.odds
            )),
            Line::raw(format!("Liquidity: {}", row.availability_label())),
            Line::raw(format!("Rating: {:.2}", row.rating)),
            Line::raw(format!(
                "SNR: {}",
                row.snr
                    .map(|value| format!("{value:.2}%"))
                    .unwrap_or_else(|| String::from("-"))
            )),
            Line::raw(format!(
                "Back Link: {}",
                row.back.deep_link.as_deref().unwrap_or("-")
            )),
            Line::raw(format!(
                "Lay Link: {}",
                row.lay.deep_link.as_deref().unwrap_or("-")
            )),
            Line::raw(format!(
                "Betslip: {}",
                row.lay
                    .bet_slip
                    .as_ref()
                    .map(|bet_slip| format!("{}/{}", bet_slip.market_id, bet_slip.selection_id))
                    .unwrap_or_else(|| String::from("-"))
            )),
            Line::raw(""),
            Line::raw(status_message.to_string()),
        ]
    } else {
        vec![
            Line::raw("No live OddsMatcher rows loaded."),
            Line::raw("Press r to fetch with the saved filter set."),
            Line::raw("Use left/right to move between filters and results."),
            Line::raw(""),
            Line::raw(status_message.to_string()),
        ]
    };

    let body = Paragraph::new(lines)
        .block(section_block("Selection", accent_pink()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn metric(label: &'static str, accent: Color) -> Span<'static> {
    Span::styled(
        format!("{label}: "),
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )
}

fn section_block(title: &'static str, accent: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .border_style(Style::default().fg(border_color()))
}

fn panel_background() -> Color {
    Color::Rgb(16, 22, 30)
}

fn text_color() -> Color {
    Color::Rgb(234, 240, 246)
}

fn border_color() -> Color {
    Color::Rgb(74, 88, 104)
}

fn accent_blue() -> Color {
    Color::Rgb(109, 180, 255)
}

fn accent_cyan() -> Color {
    Color::Rgb(94, 234, 212)
}

fn accent_green() -> Color {
    Color::Rgb(134, 239, 172)
}

fn accent_gold() -> Color {
    Color::Rgb(248, 208, 119)
}

fn accent_pink() -> Color {
    Color::Rgb(244, 143, 177)
}
