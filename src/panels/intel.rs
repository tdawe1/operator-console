use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::Frame;

use crate::app::{App, IntelRow, IntelView};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let overview_height = if area.width < 44 { 7 } else { 6 };
    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(overview_height),
        Constraint::Min(10),
    ])
    .split(area);

    render_tabs(frame, layout[0], app);
    render_overview(frame, layout[1], app);
    render_table(frame, layout[2], app);
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
                .fg(selected_text())
                .bg(selected_background())
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
    let compact = area.width < 44;
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

    let lines = if compact {
        vec![
            Line::from(vec![
                badge("View", app.intel_view().label(), accent_blue()),
                Span::raw(" "),
                badge(
                    "Sources",
                    &format!(
                        "{}/{}",
                        app.intel_ready_sources(),
                        app.intel_source_statuses().len()
                    ),
                    accent_green(),
                ),
            ]),
            Line::from(vec![
                badge("Tradable", &tradable_count.to_string(), accent_gold()),
                Span::raw(" "),
                badge("Top", &format!("{best_edge:.1}%"), accent_pink()),
            ]),
            Line::from(vec![
                Span::styled("Selection ", Style::default().fg(accent_cyan())),
                Span::styled(
                    selected
                        .as_ref()
                        .map(|row| format!("{} • {}", truncate(&row.event, 18), truncate(&row.selection, 12)))
                        .unwrap_or_else(|| String::from("No Intel opportunity selected.")),
                    Style::default()
                        .fg(text_color())
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Workflow  ", Style::default().fg(accent_gold())),
                Span::raw(
                    selected
                        .as_ref()
                        .map(workflow_summary)
                        .unwrap_or("tab cycle Intel view • select a row"),
                ),
            ]),
        ]
    } else {
        vec![
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
                Span::styled("Selection  ", Style::default().fg(accent_cyan())),
                Span::styled(
                    selected
                        .as_ref()
                        .map(|row| format!("{}  •  {}", truncate(&row.event, 34), row.selection))
                        .unwrap_or_else(|| String::from("No Intel opportunity selected.")),
                    Style::default()
                        .fg(text_color())
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Source     ", Style::default().fg(accent_cyan())),
                Span::raw(
                    selected
                        .as_ref()
                        .map(|row| format!("{}  •  {}", row.source.label(), row.status))
                        .unwrap_or_else(|| String::from("Awaiting selection")),
                ),
            ]),
            Line::from(vec![
                Span::styled("Workflow   ", Style::default().fg(accent_gold())),
                Span::raw(selected.as_ref().map(workflow_summary).unwrap_or(
                    "tab cycle Intel view  •  select a row for calculator/action handoff",
                )),
            ]),
        ]
    };

    let body = Paragraph::new(lines)
        .block(section_block(&title, accent_blue()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_table(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let intel_rows = app.intel_rows();
    if intel_rows.is_empty() {
        let message = match app.market_intel_phase() {
            "error" | "stale" => app
                .market_intel_last_error()
                .unwrap_or("Market intel is unavailable.")
                .to_string(),
            "loading" => String::from("Market intel is still loading."),
            _ => String::from("No market-intel opportunities are available for this view yet."),
        };

        frame.render_widget(
            Paragraph::new(message)
                .block(section_block("Opportunity Board", accent_cyan()))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let compact = area.width < 52;
    let rows = intel_rows
        .into_iter()
        .map(|row| {
            if compact {
                Row::new(vec![
                    Cell::from(row.source.label()),
                    Cell::from(truncate(&row.event, 18)),
                    Cell::from(truncate(&row.selection, 12)),
                    Cell::from(metric_summary(&row)).style(Style::default().fg(metric_color(&row))),
                ])
            } else {
                Row::new(vec![
                    Cell::from(row.source.label()),
                    Cell::from(truncate(&row.event, 22)),
                    Cell::from(truncate(&row.selection, 16)),
                    Cell::from(format!("{}/{}", row.bookmaker, row.exchange)),
                    Cell::from(metric_summary(&row)).style(Style::default().fg(metric_color(&row))),
                    Cell::from(row.updated_at.clone()).style(Style::default().fg(muted_text())),
                ])
            }
        })
        .collect::<Vec<_>>();

    let layout = Layout::vertical([Constraint::Min(8), Constraint::Length(7)]).split(area);

    let table = Table::new(
        rows,
        if compact {
            vec![
                Constraint::Length(6),
                Constraint::Min(12),
                Constraint::Length(12),
                Constraint::Length(10),
            ]
        } else {
            vec![
                Constraint::Length(11),
                Constraint::Length(24),
                Constraint::Length(18),
                Constraint::Length(22),
                Constraint::Length(14),
                Constraint::Min(10),
            ]
        },
    )
    .header(
        Row::new(if compact {
            vec!["Source", "Event", "Sel", "Signal"]
        } else {
            vec!["Source", "Event", "Selection", "Route", "Signal", "Fresh"]
        })
        .style(
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        ),
    )
    .row_highlight_style(
        Style::default()
            .fg(selected_text())
            .bg(selected_background())
            .add_modifier(Modifier::BOLD),
    )
    .column_spacing(if compact { 1 } else { 2 })
    .block(section_block("Opportunity Board", accent_cyan()));
    frame.render_stateful_widget(table, layout[0], app.intel_table_state());
    render_selected_summary(frame, layout[1], app);
}

fn render_selected_summary(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let selected = app.selected_intel_row();
    let statuses = app.intel_source_statuses();

    let lines = match selected.as_ref() {
        Some(row) => vec![
            Line::from(vec![
                badge("Selected", &truncate(&row.selection, 18), accent_gold()),
                Span::raw("  "),
                badge("Signal", &metric_summary(row), metric_color(row)),
            ]),
            Line::from(vec![
                Span::styled("Event  ", Style::default().fg(accent_cyan())),
                Span::raw(truncate(&row.event, 34)),
            ]),
            Line::from(vec![
                Span::styled("Route  ", Style::default().fg(accent_cyan())),
                Span::raw(format!("{}/{}", row.bookmaker, row.exchange)),
            ]),
            Line::from(vec![
                Span::styled("Action ", Style::default().fg(accent_pink())),
                Span::raw(workflow_summary(row)),
            ]),
            Line::from(vec![
                Span::styled("Source ", Style::default().fg(accent_green())),
                Span::raw(
                    statuses
                        .iter()
                        .find(|s| s.source == row.source)
                        .map(|status| {
                            format!("{} • {}", status.source.label(), truncate(&status.detail, 24))
                        })
                        .unwrap_or_else(|| String::from("No source health loaded.")),
                ),
            ]),
        ],
        None => vec![
            Line::from(vec![Span::styled(
                "Select an Intel row to inspect the decision rail.",
                Style::default().fg(muted_text()),
            )]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Source ", Style::default().fg(accent_green())),
                Span::raw(
                    statuses
                        .first()
                        .map(|status| {
                            format!("{} • {}", status.source.label(), truncate(&status.detail, 26))
                        })
                        .unwrap_or_else(|| String::from("No source health loaded.")),
                ),
            ]),
        ],
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(section_block("Decision Rail", accent_gold()))
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
            .fg(on_color(color))
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

fn section_block(title: &str, accent: Color) -> Block<'_> {
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

fn selected_background() -> Color {
    crate::theme::selected_background()
}

fn selected_text() -> Color {
    crate::theme::selected_text()
}

fn on_color(color: Color) -> Color {
    crate::theme::contrast_text(color)
}