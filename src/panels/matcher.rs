use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::Frame;

use crate::app::{App, OddsMatcherFocus};
use crate::app_state::MatcherView;
use crate::oddsmatcher::OddsMatcherRow;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(10)]).split(area);
    let titles = MatcherView::ALL.map(MatcherView::label);
    let selected = MatcherView::ALL
        .iter()
        .position(|view| *view == app.matcher_view())
        .unwrap_or(0);

    let tabs = Tabs::new(titles.to_vec())
        .select(selected)
        .block(section_block("Matcher", accent_blue()))
        .style(Style::default().fg(muted_text()).bg(panel_background()))
        .highlight_style(
            Style::default()
                .fg(selected_text())
                .bg(selected_background())
                .add_modifier(Modifier::BOLD),
        )
        .divider("│");
    frame.render_widget(tabs, layout[0]);
    register_tab_targets(layout[0], &titles)
        .into_iter()
        .enumerate()
        .for_each(|(index, rect)| app.register_matcher_view_target(rect, MatcherView::ALL[index]));

    if layout[1].width < 72 {
        render_compact(frame, layout[1], app);
        return;
    }

    match app.matcher_view() {
        MatcherView::Odds => crate::panels::oddsmatcher::render(frame, layout[1], app),
        MatcherView::Horse => crate::panels::horse_matcher::render(frame, layout[1], app),
        MatcherView::Acca => {
            let body = Paragraph::new(
                "Acca Matcher is scaffolded. The merged matcher shell is live, but acca ranking and execution wiring still need API-backed leg aggregation.",
            )
            .block(section_block("Acca Matcher", accent_gold()))
            .wrap(Wrap { trim: true });
            frame.render_widget(body, layout[1]);
        }
    }
}

fn render_compact(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    match app.matcher_view() {
        MatcherView::Odds => render_compact_odds(frame, area, app),
        MatcherView::Horse => render_compact_horse(frame, area, app),
        MatcherView::Acca => {
            let body = Paragraph::new(
                "Acca Matcher is scaffolded. Ranking and execution wiring still need API-backed leg aggregation.",
            )
            .block(section_block("Acca Matcher", accent_gold()))
            .wrap(Wrap { trim: true });
            frame.render_widget(body, area);
        }
    }
}

fn render_compact_odds(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let info_height = if area.height >= 18 { 4 } else { 3 };
    let detail_height = if area.height >= 18 { 5 } else { 4 };
    let layout = Layout::vertical([
        Constraint::Length(info_height),
        Constraint::Min(6),
        Constraint::Length(detail_height),
    ])
    .split(area);

    let summary = Paragraph::new(vec![
        Line::from(vec![
            badge("Rows", &app.oddsmatcher_rows().len().to_string(), accent_blue()),
            Span::raw(" "),
            badge(
                "Focus",
                match app.oddsmatcher_focus() {
                    OddsMatcherFocus::Filters => "filters",
                    OddsMatcherFocus::Results => "results",
                },
                accent_green(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Filter ", Style::default().fg(muted_text())),
            Span::styled(
                displayed_query_field(app),
                Style::default().fg(text_color()).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            truncate_text(app.oddsmatcher_query_note(), 56),
            Style::default().fg(muted_text()),
        )),
    ])
    .block(section_block("OddsMatcher", accent_blue()))
    .wrap(Wrap { trim: true });
    frame.render_widget(summary, layout[0]);

    let rows = app.oddsmatcher_rows().iter().map(compact_row);
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(42),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(9),
        ],
    )
    .header(
        Row::new(vec!["Event", "Back", "Lay", "Rate"]).style(
            Style::default()
                .fg(selected_text())
                .bg(selected_background())
                .add_modifier(Modifier::BOLD),
        ),
    )
    .row_highlight_style(
        Style::default()
            .bg(selected_background())
            .fg(selected_text())
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol(if app.oddsmatcher_rows().is_empty() { "  " } else { "● " })
    .block(section_block("Offers", accent_green()));
    frame.render_stateful_widget(table, layout[1], app.oddsmatcher_table_state());

    render_compact_details(frame, layout[2], app.selected_oddsmatcher_row(), app.status_message());
}

fn render_compact_horse(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let info_height = if area.height >= 18 { 4 } else { 3 };
    let detail_height = if area.height >= 18 { 5 } else { 4 };
    let layout = Layout::vertical([
        Constraint::Length(info_height),
        Constraint::Min(6),
        Constraint::Length(detail_height),
    ])
    .split(area);

    let summary = Paragraph::new(vec![
        Line::from(vec![
            badge("Rows", &app.horse_matcher_rows().len().to_string(), accent_blue()),
            Span::raw(" "),
            badge(
                "Focus",
                match app.horse_matcher_focus() {
                    OddsMatcherFocus::Filters => "filters",
                    OddsMatcherFocus::Results => "results",
                },
                accent_green(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Query  ", Style::default().fg(muted_text())),
            Span::styled(
                truncate_text(app.horse_matcher_query_note(), 42),
                Style::default().fg(text_color()).add_modifier(Modifier::BOLD),
            ),
        ]),
    ])
    .block(section_block("HorseMatcher", accent_blue()))
    .wrap(Wrap { trim: true });
    frame.render_widget(summary, layout[0]);

    let rows = app.horse_matcher_rows().iter().map(compact_row);
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(42),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(9),
        ],
    )
    .header(
        Row::new(vec!["Race", "Back", "Lay", "Rate"]).style(
            Style::default()
                .fg(selected_text())
                .bg(selected_background())
                .add_modifier(Modifier::BOLD),
        ),
    )
    .row_highlight_style(
        Style::default()
            .bg(selected_background())
            .fg(selected_text())
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol(if app.horse_matcher_rows().is_empty() { "  " } else { "● " })
    .block(section_block("Racing Rows", accent_green()));
    frame.render_stateful_widget(table, layout[1], app.horse_matcher_table_state());

    render_compact_details(frame, layout[2], app.selected_horse_matcher_row(), app.status_message());
}

fn compact_row(row: &OddsMatcherRow) -> Row<'static> {
    Row::new(vec![
        Cell::from(truncate_text(&format!("{} • {}", row.event_name, row.selection_name), 22)),
        Cell::from(format!("{:.2}", row.back.odds)),
        Cell::from(format!("{:.2}", row.lay.odds)),
        Cell::from(format!("{:.1}%", row.rating)),
    ])
}

fn render_compact_details(
    frame: &mut Frame<'_>,
    area: Rect,
    row: Option<&OddsMatcherRow>,
    status_message: &str,
) {
    let lines = if let Some(row) = row {
        vec![
            Line::from(vec![
                Span::styled("Selected ", Style::default().fg(accent_gold())),
                Span::styled(
                    truncate_text(&row.selection_name, 26),
                    Style::default().fg(text_color()).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Route    ", Style::default().fg(muted_text())),
                Span::raw(format!(
                    "{}/{}",
                    short_book_name(&row.back.bookmaker.display_name),
                    short_book_name(&row.lay.bookmaker.display_name)
                )),
            ]),
            Line::from(Span::styled(
                truncate_text(status_message, 54),
                Style::default().fg(muted_text()),
            )),
        ]
    } else {
        vec![
            Line::raw("No matcher row selected."),
            Line::from(Span::styled(
                truncate_text(status_message, 54),
                Style::default().fg(muted_text()),
            )),
        ]
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(section_block("Detail", accent_gold()))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn register_tab_targets(area: Rect, titles: &[&str]) -> Vec<Rect> {
    let mut targets = Vec::new();
    let mut x = area.x.saturating_add(1);
    let y = area.y.saturating_add(1);
    for title in titles {
        let width = title.len() as u16;
        targets.push(Rect {
            x,
            y,
            width,
            height: 1,
        });
        x = x.saturating_add(width).saturating_add(2);
    }
    targets
}

fn section_block(title: &'static str, accent: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            format!(" {} ", title),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .border_style(Style::default().fg(border_color()))
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

fn accent_green() -> Color {
    crate::theme::accent_green()
}

fn accent_gold() -> Color {
    crate::theme::accent_gold()
}

fn selected_background() -> Color {
    crate::theme::selected_background()
}

fn selected_text() -> Color {
    crate::theme::selected_text()
}

fn badge(label: &str, value: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!("{label}: {value}"),
        Style::default()
            .fg(selected_text())
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

fn displayed_query_field(app: &App) -> String {
    let query = app.oddsmatcher_query();
    let market = query
        .permitted_market_groups
        .first()
        .map(String::as_str)
        .unwrap_or("all");
    let book = query.bookmaker.first().map(String::as_str).unwrap_or("all");
    let exchange = query.exchange.first().map(String::as_str).unwrap_or("all");
    let market = if market.is_empty() {
        "all"
    } else {
        market
    };
    format!("{market} • {book}/{exchange}")
}

fn truncate_text(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }

    let truncated = value
        .chars()
        .take(max.saturating_sub(3))
        .collect::<String>();
    format!("{truncated}...")
}

fn short_book_name(value: &str) -> String {
    truncate_text(value, 10)
}
