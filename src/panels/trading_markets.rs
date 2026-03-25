use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::app::{App, TradingSection};
use crate::owls::{OwlsEndpointGroup, OwlsEndpointSummary, OwlsGroupSummary};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let layout = Layout::vertical([Constraint::Length(4), Constraint::Min(13)]).split(area);
    let body = Layout::horizontal([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(layout[1]);

    let selected = app.selected_owls_endpoint().cloned();
    render_overview(frame, layout[0], app, selected.as_ref());
    render_endpoint_table(frame, body[0], app);
    render_preview(
        frame,
        body[1],
        app.active_trading_section(),
        selected.as_ref(),
    );
}

pub fn render_overlay(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if !app.markets_overlay_visible() || !is_owls_section(app.active_trading_section()) {
        return;
    }

    let popup = popup_area(area, 86, 78);
    frame.render_widget(Clear, popup);
    let block = section_block(section_title(app.active_trading_section()), accent_gold());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let layout = Layout::vertical([Constraint::Length(9), Constraint::Min(10)]).split(inner);
    let selected = app.selected_owls_endpoint();
    render_selection_detail(frame, layout[0], selected);
    render_preview(frame, layout[1], app.active_trading_section(), selected);
}

fn render_overview(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    selected: Option<&OwlsEndpointSummary>,
) {
    let owls = app.owls_dashboard();
    let ready = owls
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.status == "ready")
        .count();
    let waiting = owls
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.status == "waiting")
        .count();
    let errors = owls
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.status == "error")
        .count();

    let selected_line = selected
        .map(|endpoint| {
            format!(
                "{} {} [{}] rows {} polls {} Δ{}",
                endpoint.method,
                endpoint.path,
                endpoint.status,
                endpoint.count,
                endpoint.poll_count,
                endpoint.change_count
            )
        })
        .unwrap_or_else(|| String::from("No endpoint selected."));
    let body = Paragraph::new(vec![
        Line::from(vec![
            badge("Sport", owls.sport.as_str(), accent_blue()),
            Span::raw("  "),
            badge(
                "Ready",
                &format!("{ready}/{}", app.visible_owls_endpoints().len()),
                accent_green(),
            ),
            Span::raw("  "),
            badge("Wait", &waiting.to_string(), accent_gold()),
            Span::raw("  "),
            badge("Err", &errors.to_string(), accent_red()),
            Span::raw("  "),
            badge("Chk", &owls.sync_checks.to_string(), accent_cyan()),
            Span::raw("  "),
            badge("Δ", &owls.sync_changes.to_string(), accent_pink()),
        ]),
        Line::from(vec![
            Span::styled("Sync ", Style::default().fg(accent_cyan())),
            Span::raw(format!(
                "{} • polls {}",
                owls.last_sync_mode, owls.total_polls
            )),
            Span::raw("  "),
            Span::styled("View ", Style::default().fg(accent_green())),
            Span::raw(section_title(app.active_trading_section())),
            Span::raw("  "),
            Span::styled("Coverage ", Style::default().fg(accent_gold())),
            Span::raw(group_summary_line(
                &owls.groups,
                app.active_trading_section(),
            )),
        ]),
        Line::from(vec![
            Span::styled("Sel ", Style::default().fg(accent_green())),
            Span::raw(truncate(&selected_line, 94)),
            Span::raw("  "),
            Span::styled("[ ] sport", Style::default().fg(accent_pink())),
        ]),
    ])
    .block(section_block(
        section_title(app.active_trading_section()),
        accent_blue(),
    ))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_endpoint_table(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let rows = app
        .visible_owls_endpoints()
        .into_iter()
        .map(|endpoint| {
            Row::new(vec![
                Cell::from(endpoint.group.short()),
                Cell::from(endpoint.label.clone()),
                Cell::from(endpoint.status.clone()).style(
                    Style::default()
                        .fg(status_color(&endpoint.status))
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from(endpoint.count.to_string()),
                Cell::from(truncate(&endpoint.path, 28)).style(Style::default().fg(muted_text())),
                Cell::from(truncate(&endpoint.detail, 18)),
            ])
        })
        .collect::<Vec<_>>();

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(18),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(30),
            Constraint::Min(12),
        ],
    )
    .header(
        Row::new(vec!["G", "Endpoint", "State", "Rows", "Route", "Detail"]).style(
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
    .block(section_block(
        board_title(app.active_trading_section()),
        accent_cyan(),
    ));
    frame.render_stateful_widget(table, area, app.owls_endpoint_table_state());
}

fn render_selection_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    selected: Option<&OwlsEndpointSummary>,
) {
    let Some(endpoint) = selected else {
        let body = Paragraph::new("Select an endpoint to inspect the route and filters.")
            .block(section_block("Endpoint Detail", accent_gold()))
            .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    };

    let lines = vec![
        Line::from(vec![
            badge("Group", endpoint.group.label(), group_color(endpoint.group)),
            Span::raw("  "),
            badge("Rows", &endpoint.count.to_string(), accent_green()),
            Span::raw("  "),
            badge("Poll", &endpoint.poll_count.to_string(), accent_cyan()),
            Span::raw("  "),
            badge("Δ", &endpoint.change_count.to_string(), accent_pink()),
            Span::raw("  "),
            badge(
                "Updated",
                &endpoint.updated_at.as_str().if_empty("-"),
                accent_gold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Route ", Style::default().fg(accent_cyan())),
            Span::raw(format!("{} {}", endpoint.method, endpoint.path)),
        ]),
        Line::from(vec![
            Span::styled("Filters ", Style::default().fg(accent_cyan())),
            Span::raw(endpoint.query_hint.clone()),
        ]),
        Line::from(vec![
            Span::styled("Detail ", Style::default().fg(accent_cyan())),
            Span::raw(truncate(&endpoint.detail, 84)),
        ]),
        Line::from(vec![
            Span::styled("About ", Style::default().fg(accent_cyan())),
            Span::raw(truncate(&endpoint.description, 82)),
        ]),
    ];

    let body = Paragraph::new(lines)
        .block(section_block("Endpoint Detail", accent_gold()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_preview(
    frame: &mut Frame<'_>,
    area: Rect,
    section: TradingSection,
    selected: Option<&OwlsEndpointSummary>,
) {
    let mut lines = Vec::new();

    match selected {
        Some(endpoint) if !endpoint.preview.is_empty() => {
            for row in endpoint.preview.iter().take(10) {
                lines.push(Line::styled(
                    truncate(&row.label, 40),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ));
                lines.push(Line::raw(format!(
                    "{} | {}",
                    truncate(&row.detail, 32),
                    truncate(&row.metric, 24)
                )));
            }
        }
        Some(endpoint) => lines.push(Line::raw(format!(
            "No preview rows returned for {}.",
            endpoint.label
        ))),
        None => lines.push(Line::raw("No endpoint selected.")),
    }

    let body = Paragraph::new(lines)
        .block(section_block(preview_title(section), accent_pink()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical =
        Layout::vertical([Constraint::Percentage(percent_y)]).flex(ratatui::layout::Flex::Center);
    let horizontal =
        Layout::horizontal([Constraint::Percentage(percent_x)]).flex(ratatui::layout::Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

fn group_summary_line(groups: &[OwlsGroupSummary], section: TradingSection) -> String {
    groups
        .iter()
        .filter(|group| section_group_matches(section, group.group))
        .map(|group| format!("{} {}/{}", group.group.short(), group.ready, group.total))
        .collect::<Vec<_>>()
        .join(" • ")
}

fn section_group_matches(section: TradingSection, group: OwlsEndpointGroup) -> bool {
    match section {
        TradingSection::Live => matches!(
            group,
            OwlsEndpointGroup::Realtime | OwlsEndpointGroup::Scores | OwlsEndpointGroup::Odds
        ),
        TradingSection::Props => {
            matches!(group, OwlsEndpointGroup::Props | OwlsEndpointGroup::History)
        }
        _ => true,
    }
}

fn is_owls_section(section: TradingSection) -> bool {
    matches!(
        section,
        TradingSection::Markets | TradingSection::Live | TradingSection::Props
    )
}

fn section_title(section: TradingSection) -> &'static str {
    match section {
        TradingSection::Markets => "Owls Markets",
        TradingSection::Live => "Owls Live",
        TradingSection::Props => "Owls Props",
        _ => "Owls",
    }
}

fn board_title(section: TradingSection) -> &'static str {
    match section {
        TradingSection::Markets => "Endpoint Board",
        TradingSection::Live => "Live Board",
        TradingSection::Props => "Props Board",
        _ => "Board",
    }
}

fn preview_title(section: TradingSection) -> &'static str {
    match section {
        TradingSection::Markets => "Preview",
        TradingSection::Live => "Feed Preview",
        TradingSection::Props => "Prop Preview",
        _ => "Preview",
    }
}

fn section_block(title: &'static str, color: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
}

fn badge(label: &str, value: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!("{label}:{value}"),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn status_color(status: &str) -> Color {
    match status {
        "ready" => accent_green(),
        "waiting" => accent_gold(),
        "error" => accent_red(),
        _ => muted_text(),
    }
}

fn group_color(group: OwlsEndpointGroup) -> Color {
    match group {
        OwlsEndpointGroup::Odds => accent_blue(),
        OwlsEndpointGroup::Props => accent_pink(),
        OwlsEndpointGroup::Scores => accent_green(),
        OwlsEndpointGroup::Stats => accent_cyan(),
        OwlsEndpointGroup::Prediction => accent_gold(),
        OwlsEndpointGroup::History => Color::Rgb(255, 171, 145),
        OwlsEndpointGroup::Realtime => Color::Rgb(196, 181, 253),
    }
}

fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    format!("{}...", &value[..limit.saturating_sub(3)])
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
    Color::Rgb(250, 204, 21)
}

fn accent_pink() -> Color {
    Color::Rgb(244, 143, 177)
}

fn accent_red() -> Color {
    Color::Rgb(248, 113, 113)
}

fn muted_text() -> Color {
    Color::Rgb(152, 166, 181)
}

trait EmptyFallback {
    fn if_empty(self, fallback: &str) -> String;
}

impl EmptyFallback for &str {
    fn if_empty(self, fallback: &str) -> String {
        if self.trim().is_empty() {
            return String::from(fallback);
        }
        self.to_string()
    }
}
