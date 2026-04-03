use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::Frame;

use crate::app::{App, IntelRow, IntelSourceStatus, IntelView};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Min(12),
    ])
    .split(area);
    let body = Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)])
        .split(layout[2]);

    render_tabs(frame, layout[0], app);
    render_overview(frame, layout[1], app);
    render_table(frame, body[0], app);
    render_detail(frame, body[1], app);
}

fn render_tabs(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let titles = IntelView::ALL.map(IntelView::label);
    let selected = IntelView::ALL
        .iter()
        .position(|view| *view == app.intel_view())
        .unwrap_or(0);

    let tabs = Tabs::new(titles.to_vec())
        .select(selected)
        .block(section_block("Intel", accent_blue()))
        .style(Style::default().fg(muted_text()).bg(panel_background()))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        )
        .divider("│");
    frame.render_widget(tabs, area);

    register_tab_targets(area, &titles)
        .into_iter()
        .enumerate()
        .for_each(|(index, rect)| app.register_intel_view_target(rect, IntelView::ALL[index]));
}

fn render_overview(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = app.intel_rows();
    let tradable_count = rows.iter().filter(|row| row.can_open_action()).count();
    let best_edge = rows
        .iter()
        .filter_map(|row| row.edge_pct.or(row.arb_pct))
        .fold(None, |best, current| match best {
            Some(best) if best >= current => Some(best),
            _ => Some(current),
        })
        .unwrap_or(0.0);
    let selected = app.selected_intel_row();
    let title = format!("Intel {}", app.intel_view().label());

    let body =
        Paragraph::new(vec![
            Line::from(vec![
                badge("View", app.intel_view().label(), accent_blue()),
                Span::raw("  "),
                badge(
                    "Sources",
                    &format!(
                        "{}/{}",
                        app.intel_ready_sources(),
                        app.intel_source_statuses().len()
                    ),
                    accent_green(),
                ),
                Span::raw("  "),
                badge("Tradable", &tradable_count.to_string(), accent_gold()),
                Span::raw("  "),
                badge("Top", &format!("{best_edge:.1}%"), accent_pink()),
            ]),
            Line::from(vec![
                Span::styled("Selection ", Style::default().fg(accent_cyan())),
                Span::raw(
                    selected
                        .as_ref()
                        .map(|row| {
                            format!("{} • {} • {}", row.event, row.selection, row.source.label())
                        })
                        .unwrap_or_else(|| String::from("No Intel opportunity selected.")),
                ),
            ]),
            Line::from(vec![
                Span::styled("Workflow ", Style::default().fg(accent_gold())),
                Span::raw(selected.as_ref().map(workflow_summary).unwrap_or(
                    "tab cycle Intel view  •  select a row for calculator/action handoff",
                )),
            ]),
        ])
        .block(section_block(&title, accent_blue()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_table(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let rows = app
        .intel_rows()
        .into_iter()
        .map(|row| {
            Row::new(vec![
                Cell::from(row.source.label()),
                Cell::from(truncate(&row.event, 22)),
                Cell::from(truncate(&row.selection, 16)),
                Cell::from(format!("{}/{}", row.bookmaker, row.exchange)),
                Cell::from(metric_summary(&row)).style(Style::default().fg(metric_color(&row))),
                Cell::from(row.updated_at.clone()).style(Style::default().fg(muted_text())),
            ])
        })
        .collect::<Vec<_>>();

    let table = Table::new(
        rows,
        [
            Constraint::Length(11),
            Constraint::Length(24),
            Constraint::Length(18),
            Constraint::Length(22),
            Constraint::Length(14),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            "Source",
            "Event",
            "Selection",
            "Route",
            "Signal",
            "Fresh",
        ])
        .style(
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        ),
    )
    .row_highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(accent_cyan())
            .add_modifier(Modifier::BOLD),
    )
    .column_spacing(1)
    .block(section_block("Opportunity Board", accent_cyan()));
    frame.render_stateful_widget(table, area, app.intel_table_state());
}

fn render_detail(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let layout = Layout::vertical([
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Min(7),
    ])
    .split(area);
    render_selected_row(frame, layout[0], app.selected_intel_row().as_ref());
    render_source_health(frame, layout[1], &app.intel_source_statuses());
    render_workflow(frame, layout[2], app.selected_intel_row().as_ref());
}

fn render_selected_row(frame: &mut Frame<'_>, area: Rect, selected: Option<&IntelRow>) {
    let Some(row) = selected else {
        let body =
            Paragraph::new("Select an Intel row to inspect source, quote, and workflow detail.")
                .block(section_block("Detail Rail", accent_gold()))
                .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    };

    let body = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                row.source.label(),
                Style::default()
                    .fg(accent_blue())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                row.status.as_str(),
                Style::default().fg(status_color(&row.status)),
            ),
        ]),
        Line::raw(truncate(&row.event, 42)),
        Line::raw(format!("{} • {}", row.market, row.selection)),
        Line::raw(format!(
            "Back {:.2}  Lay {}  Fair {}",
            row.back_odds,
            row.lay_odds
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| String::from("-")),
            row.fair_odds
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| String::from("-"))
        )),
        Line::raw(format!(
            "Edge {}  Arb {}  Liq {}",
            optional_pct(row.edge_pct),
            optional_pct(row.arb_pct),
            row.liquidity
                .map(|value| format!("{value:.0}"))
                .unwrap_or_else(|| String::from("-"))
        )),
        Line::raw(truncate(&row.note, 48)),
    ])
    .block(section_block("Detail Rail", accent_gold()))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_source_health(frame: &mut Frame<'_>, area: Rect, statuses: &[IntelSourceStatus]) {
    let lines = statuses
        .iter()
        .flat_map(|status| {
            [
                Line::from(vec![
                    Span::styled(
                        status.source.label(),
                        Style::default()
                            .fg(accent_cyan())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        status.health.as_str(),
                        Style::default().fg(status_color(&status.health)),
                    ),
                    Span::raw("  "),
                    Span::raw(format!("{} • {}", status.freshness, status.transport)),
                ]),
                Line::raw(truncate(&status.detail, 46)),
            ]
        })
        .collect::<Vec<_>>();

    let body = Paragraph::new(lines)
        .block(section_block("Source Health", accent_green()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_workflow(frame: &mut Frame<'_>, area: Rect, selected: Option<&IntelRow>) {
    let lines = match selected {
        Some(row) => vec![
            Line::raw(format!("Competition: {}", row.competition)),
            Line::raw(format!("Route: {}", truncate(&row.route, 46))),
            Line::raw(format!("Action: {}", workflow_summary(row))),
        ],
        None => vec![Line::raw(
            "No Intel workflow is available without a selection.",
        )],
    };

    let body = Paragraph::new(lines)
        .block(section_block("Workflow", accent_pink()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
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

fn metric_summary(row: &IntelRow) -> String {
    if let Some(edge) = row.edge_pct {
        format!("EV {edge:.1}%")
    } else if let Some(arb) = row.arb_pct {
        format!("Arb {arb:.1}%")
    } else {
        format!(
            "{:.2}/{:.2}",
            row.back_odds,
            row.lay_odds.unwrap_or(row.back_odds)
        )
    }
}

fn metric_color(row: &IntelRow) -> Color {
    if row.edge_pct.unwrap_or_default() >= 4.0 || row.arb_pct.unwrap_or_default() >= 1.5 {
        accent_green()
    } else {
        accent_gold()
    }
}

fn optional_pct(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.1}%"))
        .unwrap_or_else(|| String::from("-"))
}

fn workflow_summary(row: &IntelRow) -> &'static str {
    match (row.can_seed_calculator(), row.can_open_action()) {
        (true, true) => {
            "enter preload calculator  •  p open trading action  •  tab cycle Intel view"
        }
        (true, false) => "enter preload calculator  •  tab cycle Intel view",
        (false, true) => "p open trading action  •  tab cycle Intel view",
        (false, false) => {
            "await lay quote before calculator/action handoff  •  tab cycle Intel view"
        }
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }

    let truncated = value
        .chars()
        .take(max.saturating_sub(3))
        .collect::<String>();
    format!("{truncated}...")
}

fn badge(label: &str, value: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!("{label} {value}"),
        Style::default()
            .fg(Color::Black)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

fn section_block(title: &str, accent: Color) -> Block<'_> {
    Block::default()
        .title(Span::styled(
            title.to_string(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .border_style(Style::default().fg(border_color()))
}

fn status_color(status: &str) -> Color {
    match status {
        "ready" | "fixture" | "open" => accent_green(),
        "watch" | "degraded" | "delayed" => accent_gold(),
        "error" | "closed" => accent_red(),
        _ => muted_text(),
    }
}

fn panel_background() -> Color {
    Color::Rgb(11, 17, 24)
}

fn text_color() -> Color {
    Color::Rgb(230, 235, 245)
}

fn muted_text() -> Color {
    Color::Rgb(129, 147, 169)
}

fn border_color() -> Color {
    Color::Rgb(58, 71, 89)
}

fn accent_blue() -> Color {
    Color::Rgb(90, 169, 255)
}

fn accent_cyan() -> Color {
    Color::Rgb(78, 201, 176)
}

fn accent_green() -> Color {
    Color::Rgb(134, 239, 172)
}

fn accent_gold() -> Color {
    Color::Rgb(229, 192, 123)
}

fn accent_pink() -> Color {
    Color::Rgb(244, 143, 177)
}

fn accent_red() -> Color {
    Color::Rgb(248, 113, 113)
}
