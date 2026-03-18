use std::collections::BTreeMap;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

use crate::domain::ExchangePanelSnapshot;

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    table_state: &mut TableState,
) {
    let selected_position = selected_position(snapshot, table_state);
    let layout = Layout::vertical([
        Constraint::Length(8),
        Constraint::Min(10),
        Constraint::Length(10),
    ])
    .split(area);
    let lower = Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(layout[2]);

    render_summary(frame, layout[0], snapshot, selected_position);
    render_stateful_table(
        frame,
        layout[1],
        &format!("Open Market Positions ({})", snapshot.open_positions.len()),
        vec![
            Constraint::Percentage(28),
            Constraint::Percentage(24),
            Constraint::Length(14),
            Constraint::Length(11),
            Constraint::Length(8),
        ],
        position_rows(snapshot),
        empty_row("No open positions are loaded.", 5),
        table_state,
    );
    render_table(
        frame,
        lower[0],
        &format!(
            "Watch Groups ({})",
            snapshot
                .watch
                .as_ref()
                .map(|watch| watch.watch_count)
                .unwrap_or(0)
        ),
        vec![
            Constraint::Percentage(28),
            Constraint::Percentage(24),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(8),
        ],
        watch_rows(snapshot),
        empty_row("No grouped watch rows are loaded.", 6),
    );
    render_table(
        frame,
        lower[1],
        &format!("Market Coverage ({})", market_group_count(snapshot)),
        vec![
            Constraint::Percentage(38),
            Constraint::Length(10),
            Constraint::Length(9),
            Constraint::Length(8),
        ],
        market_rows(snapshot),
        empty_row("No market coverage is loaded.", 4),
    );
}

fn render_summary(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    selected_position: Option<&crate::domain::OpenPositionRow>,
) {
    let mut lines = vec![
        Line::from(vec![
            metric_span("Positions", accent_cyan()),
            Span::raw(snapshot.open_positions.len().to_string()),
            Span::raw("   "),
            metric_span("Watch groups", accent_blue()),
            Span::raw(
                snapshot
                    .watch
                    .as_ref()
                    .map(|watch| watch.watch_count.to_string())
                    .unwrap_or_else(|| String::from("0")),
            ),
            Span::raw("   "),
            metric_span("In-play", accent_red()),
            Span::raw(
                snapshot
                    .open_positions
                    .iter()
                    .filter(|row| row.is_in_play)
                    .count()
                    .to_string(),
            ),
            Span::raw("   "),
            metric_span("Tradable", accent_green()),
            Span::raw(
                snapshot
                    .open_positions
                    .iter()
                    .filter(|row| row.can_trade_out)
                    .count()
                    .to_string(),
            ),
        ]),
        Line::from(vec![
            metric_span("Suspended", accent_gold()),
            Span::raw(
                snapshot
                    .open_positions
                    .iter()
                    .filter(|row| effective_market_status(row) == "suspended")
                    .count()
                    .to_string(),
            ),
            Span::raw("   "),
            metric_span("Total P/L", accent_pink()),
            pnl_span(
                snapshot
                    .open_positions
                    .iter()
                    .map(|row| row.pnl_amount)
                    .sum(),
            ),
            Span::raw("   "),
            metric_span("Source", accent_gold()),
            Span::raw(
                snapshot
                    .runtime
                    .as_ref()
                    .map(|runtime| runtime.source.clone())
                    .unwrap_or_else(|| String::from("snapshot")),
            ),
        ]),
    ];

    if let Some(row) = selected_position {
        lines.push(Line::from(vec![
            metric_span("Selected", accent_blue()),
            Span::raw(format!("{} | {}", event_label(row), row.contract)),
            Span::raw("   "),
            metric_span("Phase", accent_gold()),
            Span::raw(phase_label(row)),
            Span::raw("   "),
            metric_span("Trade", accent_green()),
            Span::raw(trade_label_text(row)),
        ]));
        lines.push(Line::from(vec![
            metric_span("Marked", accent_pink()),
            Span::raw(format!(
                "current {:.2} | pnl {:+.2} | liability {:.2}",
                row.current_value, row.pnl_amount, row.liability
            )),
        ]));
    }

    let body = Paragraph::new(lines)
        .block(section_block("Market Summary", accent_blue()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn position_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    snapshot
        .open_positions
        .iter()
        .take(10)
        .map(|row| {
            Row::new(vec![
                Cell::from(event_label(row)),
                Cell::from(row.contract.clone()),
                Cell::from(phase_label(row)),
                trade_cell(row),
                pnl_cell(row.pnl_amount),
            ])
        })
        .collect()
}

fn watch_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    let Some(watch) = &snapshot.watch else {
        return Vec::new();
    };

    watch
        .watches
        .iter()
        .take(10)
        .map(|row| {
            Row::new(vec![
                Cell::from(row.contract.clone()),
                Cell::from(row.market.clone()),
                pnl_cell(row.current_pnl_amount),
                Cell::from(
                    row.current_back_odds
                        .map(|value| format!("{value:.2}"))
                        .unwrap_or_else(|| String::from("-")),
                ),
                Cell::from(format!("{:.2}", row.profit_take_back_odds)),
                Cell::from(format!("{:.2}", row.stop_loss_back_odds)),
            ])
        })
        .collect()
}

fn market_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    let mut groups: BTreeMap<String, (usize, usize, f64)> = BTreeMap::new();
    for row in &snapshot.open_positions {
        let entry = groups.entry(row.market.clone()).or_insert((0, 0, 0.0));
        entry.0 += 1;
        if row.is_in_play {
            entry.1 += 1;
        }
        entry.2 += row.pnl_amount;
    }

    groups
        .into_iter()
        .take(10)
        .map(|(market, (positions, in_play, pnl))| {
            Row::new(vec![
                Cell::from(market),
                Cell::from(positions.to_string()),
                Cell::from(in_play.to_string()),
                pnl_cell(pnl),
            ])
        })
        .collect()
}

fn market_group_count(snapshot: &ExchangePanelSnapshot) -> usize {
    snapshot
        .open_positions
        .iter()
        .map(|row| row.market.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

fn event_label(row: &crate::domain::OpenPositionRow) -> String {
    if row.event.is_empty() {
        String::from("-")
    } else {
        row.event.clone()
    }
}

fn phase_label(row: &crate::domain::OpenPositionRow) -> String {
    if row.event_status.is_empty() {
        if row.is_in_play {
            return String::from("Live");
        }
        return String::from("-");
    }
    row.event_status
        .split('|')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("-")
        .to_string()
}

fn effective_market_status(row: &crate::domain::OpenPositionRow) -> &'static str {
    match row.market_status.as_str() {
        "tradable" => "tradable",
        "suspended" => "suspended",
        "pre_event" => "pre_event",
        "settled" => "settled",
        _ if row.can_trade_out => "tradable",
        _ if row.is_in_play => "suspended",
        _ => "unavailable",
    }
}

fn trade_cell(row: &crate::domain::OpenPositionRow) -> Cell<'static> {
    let label = trade_label_text(row);
    let color = match effective_market_status(row) {
        "tradable" => accent_green(),
        "suspended" => accent_red(),
        "pre_event" => accent_gold(),
        "settled" => accent_pink(),
        _ => muted_text(),
    };
    Cell::from(label).style(Style::default().fg(color))
}

fn trade_label_text(row: &crate::domain::OpenPositionRow) -> &'static str {
    match effective_market_status(row) {
        "tradable" => "tradable",
        "suspended" => "suspended",
        "pre_event" => "pre-match",
        "settled" => "settled",
        _ if row.status == "Order filled" => "active",
        _ => "unknown",
    }
}

fn render_table(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    widths: Vec<Constraint>,
    rows: Vec<Row<'static>>,
    empty: Row<'static>,
) {
    let rows = if rows.is_empty() { vec![empty] } else { rows };
    let table = Table::new(rows, widths)
        .header(
            Row::new(table_header(title))
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(accent_cyan())
                        .add_modifier(Modifier::BOLD),
                )
                .bottom_margin(1),
        )
        .block(section_block(title, accent_blue()))
        .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_stateful_table(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    widths: Vec<Constraint>,
    rows: Vec<Row<'static>>,
    empty: Row<'static>,
    table_state: &mut TableState,
) {
    let rows = if rows.is_empty() { vec![empty] } else { rows };
    let table = Table::new(rows, widths)
        .header(
            Row::new(table_header(title))
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(accent_cyan())
                        .add_modifier(Modifier::BOLD),
                )
                .bottom_margin(1),
        )
        .block(section_block(title, accent_blue()))
        .column_spacing(1)
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(28, 39, 52))
                .fg(Color::Rgb(255, 255, 255))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(table, area, table_state);
}

fn table_header(title: &str) -> Vec<&'static str> {
    match title {
        heading if heading.starts_with("Open Market Positions") => {
            vec!["Event", "Position", "Phase", "Trade", "P/L"]
        }
        heading if heading.starts_with("Watch Groups") => {
            vec!["Contract", "Market", "P/L", "Back", "Profit", "Stop"]
        }
        heading if heading.starts_with("Market Coverage") => {
            vec!["Market", "Positions", "In-play", "P/L"]
        }
        _ => vec!["Data"],
    }
}

fn empty_row(message: &str, columns: usize) -> Row<'static> {
    let mut cells = vec![Cell::from(message.to_string())];
    for _ in 1..columns {
        cells.push(Cell::from(String::new()));
    }
    Row::new(cells).style(Style::default().fg(muted_text()))
}

fn pnl_cell(value: f64) -> Cell<'static> {
    let color = if value > 0.0 {
        accent_green()
    } else if value < 0.0 {
        accent_red()
    } else {
        accent_gold()
    };
    Cell::from(format!("{value:+.2}")).style(Style::default().fg(color))
}

fn pnl_span(value: f64) -> Span<'static> {
    let color = if value > 0.0 {
        accent_green()
    } else if value < 0.0 {
        accent_red()
    } else {
        accent_gold()
    };
    Span::styled(
        format!("{value:+.2}"),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn metric_span(label: &'static str, color: Color) -> Span<'static> {
    Span::styled(
        format!("{label}: "),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn section_block(title: &str, accent: Color) -> Block<'_> {
    Block::default()
        .title(Span::styled(
            title.to_string(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(
            Style::default()
                .bg(Color::Rgb(16, 22, 30))
                .fg(Color::Rgb(234, 240, 246)),
        )
        .border_style(Style::default().fg(Color::Rgb(74, 88, 104)))
}

fn muted_text() -> Color {
    Color::Rgb(152, 166, 181)
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

fn accent_red() -> Color {
    Color::Rgb(248, 113, 113)
}

fn selected_position<'a>(
    snapshot: &'a ExchangePanelSnapshot,
    table_state: &TableState,
) -> Option<&'a crate::domain::OpenPositionRow> {
    table_state
        .selected()
        .and_then(|index| snapshot.open_positions.get(index))
        .or_else(|| snapshot.open_positions.first())
}
