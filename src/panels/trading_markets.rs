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

    render_overview(frame, layout[0], app, app.selected_owls_endpoint());
    render_endpoint_table(frame, body[0], app);
    render_overlay_preview(
        frame,
        body[1],
        app.active_trading_section(),
        app.selected_owls_endpoint(),
    );
}

pub fn render_overlay(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if !app.markets_overlay_visible() || !is_owls_section(app.active_trading_section()) {
        return;
    }

    let popup = popup_area(area, 92, 84);
    frame.render_widget(Clear, popup);
    let block = section_block(section_title(app.active_trading_section()), accent_gold());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let layout = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(9),
        Constraint::Min(14),
    ])
    .split(inner);
    let body = Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(layout[1]);
    let selected = app.selected_owls_endpoint();
    render_overlay_summary(frame, layout[0], app, selected);
    render_selection_meta(frame, body[0], selected);
    render_selection_request(frame, body[1], selected);
    render_overlay_preview(frame, layout[2], app.active_trading_section(), selected);
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
                "{} {} [{}] rows {} quotes {} polls {} Δ{}",
                endpoint.method,
                endpoint.path,
                endpoint.status,
                endpoint.count,
                endpoint.quotes.len(),
                endpoint.poll_count,
                endpoint.change_count
            )
        })
        .unwrap_or_else(|| String::from("No endpoint selected."));
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(72), Constraint::Percentage(28)]).areas(area);

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
        Line::raw(""),
        Line::from(vec![
            Span::styled("Sync      ", Style::default().fg(accent_cyan())),
            Span::raw(format!(
                "{} • polls {}",
                owls.last_sync_mode, owls.total_polls
            )),
        ]),
        Line::from(vec![
            Span::styled("Coverage  ", Style::default().fg(accent_gold())),
            Span::raw(group_summary_line(
                &owls.groups,
                app.active_trading_section(),
            )),
        ]),
        Line::from(vec![
            Span::styled("Controls  ", Style::default().fg(accent_pink())),
            Span::raw("[/] cycle sport • ↑/↓ endpoint • Enter inspect"),
        ]),
        Line::from(vec![
            Span::styled("Selected  ", Style::default().fg(accent_green())),
            Span::styled(
                truncate(&selected_line, 78),
                Style::default()
                    .fg(text_color())
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ])
    .block(section_block(
        section_title(app.active_trading_section()),
        accent_blue(),
    ))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, left);

    let board = Table::new(
        vec![
            Row::new(vec![
                Cell::from("View"),
                Cell::from(section_title(app.active_trading_section())),
            ]),
            Row::new(vec![
                Cell::from("Endpoints"),
                Cell::from(app.visible_owls_endpoints().len().to_string()),
            ]),
            Row::new(vec![
                Cell::from("Books"),
                Cell::from(
                    selected
                        .map(|endpoint| {
                            if endpoint.books_returned.is_empty() {
                                String::from("-")
                            } else {
                                endpoint.books_returned.len().to_string()
                            }
                        })
                        .unwrap_or_else(|| String::from("-")),
                ),
            ]),
            Row::new(vec![Cell::from("Hint"), Cell::from("[/] cycle sport")]),
        ],
        [Constraint::Length(10), Constraint::Min(8)],
    )
    .header(
        Row::new(vec!["Metric", "Value"]).style(
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        ),
    )
    .column_spacing(1)
    .block(section_block("Board", accent_cyan()));
    frame.render_widget(board, right);
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
                Cell::from(truncate(&compact_endpoint_detail(&endpoint.detail), 18)),
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
            .fg(selected_text())
            .bg(selected_background())
            .add_modifier(Modifier::BOLD),
    )
    .column_spacing(1)
    .block(section_block(
        board_title(app.active_trading_section()),
        accent_cyan(),
    ));
    frame.render_stateful_widget(table, area, app.owls_endpoint_table_state());
}

fn render_overlay_summary(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    selected: Option<&OwlsEndpointSummary>,
) {
    let Some(endpoint) = selected else {
        let body = Paragraph::new("Select an endpoint to inspect the route, filters, and preview.")
            .block(section_block("Inspect", accent_gold()))
            .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    };

    let body = Paragraph::new(vec![
        Line::from(vec![
            badge("Sport", app.owls_dashboard().sport.as_str(), accent_blue()),
            Span::raw("  "),
            badge("Group", endpoint.group.label(), group_color(endpoint.group)),
            Span::raw("  "),
            badge("State", &endpoint.status, status_color(&endpoint.status)),
            Span::raw("  "),
            badge("Rows", &endpoint.count.to_string(), accent_green()),
            Span::raw("  "),
            badge("Polls", &endpoint.poll_count.to_string(), accent_cyan()),
            Span::raw("  "),
            badge("Δ", &endpoint.change_count.to_string(), accent_pink()),
        ]),
        Line::from(vec![
            Span::styled("Endpoint ", Style::default().fg(accent_cyan())),
            Span::styled(
                endpoint.label.as_str(),
                Style::default()
                    .fg(text_color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("Updated ", Style::default().fg(accent_gold())),
            Span::raw(endpoint.updated_at.as_str().if_empty("-")),
            Span::raw("  "),
            Span::styled("Quotes ", Style::default().fg(accent_green())),
            Span::raw(endpoint.quotes.len().to_string()),
        ]),
    ])
    .block(section_block("Inspect", accent_gold()))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_selection_meta(
    frame: &mut Frame<'_>,
    area: Rect,
    selected: Option<&OwlsEndpointSummary>,
) {
    let Some(endpoint) = selected else {
        let body = Paragraph::new("Select an endpoint to inspect the route and filters.")
            .block(section_block("Overview", accent_gold()))
            .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    };

    let lines = vec![
        Line::from(vec![Span::styled(
            "About",
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::raw(truncate(&endpoint.description, 92)),
        Line::raw(""),
        Line::from(vec![Span::styled(
            "Detail",
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::raw(truncate(&compact_endpoint_detail(&endpoint.detail), 92)),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Group", Style::default().fg(accent_cyan())),
            Span::raw(format!(" {}", endpoint.group.label())),
        ]),
        Line::from(vec![
            Span::styled("Board", Style::default().fg(accent_cyan())),
            Span::raw(format!(" {}", endpoint.label)),
        ]),
        Line::from(vec![
            Span::styled("Rows", Style::default().fg(accent_cyan())),
            Span::raw(format!(" {}", endpoint.count)),
        ]),
        Line::from(vec![
            Span::styled("Quotes", Style::default().fg(accent_cyan())),
            Span::raw(format!(" {}", endpoint.quotes.len())),
        ]),
        Line::from(vec![
            Span::styled("Books", Style::default().fg(accent_cyan())),
            Span::raw(format!(
                " {} / {} returned",
                endpoint.books_returned.len(),
                endpoint
                    .available_books
                    .len()
                    .max(endpoint.books_returned.len())
            )),
        ]),
        Line::from(vec![
            Span::styled("Requested", Style::default().fg(accent_cyan())),
            Span::raw(format!(
                " {}",
                endpoint.requested_books.join(", ").if_empty("all books")
            )),
        ]),
        Line::from(vec![
            Span::styled("Freshness", Style::default().fg(accent_cyan())),
            Span::raw(format!(
                " age {}s{}",
                endpoint
                    .freshness_age_seconds
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| String::from("-")),
                if endpoint.freshness_stale.unwrap_or(false) {
                    " stale"
                } else {
                    ""
                }
            )),
        ]),
    ];

    let body = Paragraph::new(lines)
        .block(section_block("Overview", accent_gold()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_selection_request(
    frame: &mut Frame<'_>,
    area: Rect,
    selected: Option<&OwlsEndpointSummary>,
) {
    let Some(endpoint) = selected else {
        let body =
            Paragraph::new("Request metadata will appear here once an endpoint is selected.")
                .block(section_block("Request", accent_cyan()))
                .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    };

    let lines = vec![
        Line::from(vec![Span::styled(
            "Route",
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::styled(
            format!("{} {}", endpoint.method, endpoint.path),
            Style::default()
                .fg(text_color())
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::from(vec![Span::styled(
            "Filters",
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::raw(endpoint.query_hint.as_str().if_empty("No query hints")),
        Line::raw(""),
        Line::from(vec![Span::styled(
            "Resolved books",
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::raw(
            endpoint
                .books_returned
                .join(", ")
                .if_empty("No per-book metadata on this endpoint"),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Status", Style::default().fg(accent_cyan())),
            Span::raw(format!(" {}", endpoint.status)),
            Span::raw("   "),
            Span::styled("Updated", Style::default().fg(accent_cyan())),
            Span::raw(format!(" {}", endpoint.updated_at.as_str().if_empty("-"))),
        ]),
    ];

    let body = Paragraph::new(lines)
        .block(section_block("Request", accent_cyan()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_overlay_preview(
    frame: &mut Frame<'_>,
    area: Rect,
    section: TradingSection,
    selected: Option<&OwlsEndpointSummary>,
) {
    match selected {
        Some(endpoint) if !endpoint.quotes.is_empty() => {
            render_quote_ladder(frame, area, section, endpoint);
        }
        Some(endpoint) if !endpoint.preview.is_empty() => {
            let rows = endpoint
                .preview
                .iter()
                .take(8)
                .map(|row| {
                    Row::new(vec![
                        Cell::from(truncate(&row.label, 36)),
                        Cell::from(row.metric.as_str().if_empty("-")).style(
                            Style::default()
                                .fg(accent_green())
                                .add_modifier(Modifier::BOLD),
                        ),
                        Cell::from(truncate(&row.detail, 46))
                            .style(Style::default().fg(muted_text())),
                    ])
                })
                .collect::<Vec<_>>();
            let table = Table::new(
                rows,
                [
                    Constraint::Length(38),
                    Constraint::Length(14),
                    Constraint::Min(16),
                ],
            )
            .header(
                Row::new(vec!["Label", "Metric", "Context"]).style(
                    Style::default()
                        .fg(accent_cyan())
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .column_spacing(1)
            .block(section_block(preview_title(section), accent_pink()));
            frame.render_widget(table, area);
        }
        Some(endpoint) => {
            let body = Paragraph::new(format!("No preview rows returned for {}.", endpoint.label))
                .block(section_block(preview_title(section), accent_pink()))
                .wrap(Wrap { trim: true });
            frame.render_widget(body, area);
        }
        None => {
            let body = Paragraph::new("No endpoint selected.")
                .block(section_block(preview_title(section), accent_pink()))
                .wrap(Wrap { trim: true });
            frame.render_widget(body, area);
        }
    }
}

fn render_quote_ladder(
    frame: &mut Frame<'_>,
    area: Rect,
    section: TradingSection,
    endpoint: &OwlsEndpointSummary,
) {
    let quotes = top_market_quotes(endpoint, 8);

    let [left, center, right] = Layout::horizontal([
        Constraint::Percentage(42),
        Constraint::Length(18),
        Constraint::Percentage(42),
    ])
    .areas(area);

    let best = quotes.iter().take(4).copied().collect::<Vec<_>>();
    let rest = quotes.iter().skip(4).take(4).copied().collect::<Vec<_>>();

    let left_table = Table::new(
        best.iter()
            .map(|quote| ladder_row(quote))
            .collect::<Vec<_>>(),
        [
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(8),
        ],
    )
    .header(
        Row::new(vec!["Book", "Price", "Limit"]).style(
            Style::default()
                .fg(accent_green())
                .add_modifier(Modifier::BOLD),
        ),
    )
    .column_spacing(1)
    .block(section_block("Top Books", accent_green()));
    frame.render_widget(left_table, left);

    let best_price = quotes
        .first()
        .and_then(|quote| quote.decimal_price)
        .unwrap_or_default();
    let low_price = quotes
        .last()
        .and_then(|quote| quote.decimal_price)
        .unwrap_or_default();
    let mid = if best_price > 0.0 && low_price > 0.0 {
        (best_price + low_price) / 2.0
    } else {
        0.0
    };

    let center_body = Paragraph::new(vec![
        Line::styled(
            truncate(&endpoint.label, 16),
            Style::default()
                .fg(text_color())
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Best", Style::default().fg(accent_cyan())),
            Span::raw(format!(" {:.2}", best_price)),
        ]),
        Line::from(vec![
            Span::styled("Low ", Style::default().fg(accent_gold())),
            Span::raw(format!(" {:.2}", low_price)),
        ]),
        Line::from(vec![
            Span::styled("Mid ", Style::default().fg(accent_pink())),
            Span::raw(format!(" {:.2}", mid)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Rows", Style::default().fg(muted_text())),
            Span::raw(format!(" {}", endpoint.count)),
        ]),
    ])
    .block(section_block(preview_title(section), accent_pink()))
    .wrap(Wrap { trim: true });
    frame.render_widget(center_body, center);

    let right_table = Table::new(
        rest.iter()
            .map(|quote| ladder_row(quote))
            .collect::<Vec<_>>(),
        [
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(8),
        ],
    )
    .header(
        Row::new(vec!["Book", "Price", "Limit"]).style(
            Style::default()
                .fg(accent_gold())
                .add_modifier(Modifier::BOLD),
        ),
    )
    .column_spacing(1)
    .block(section_block("Field", accent_gold()));
    frame.render_widget(right_table, right);
}

fn top_market_quotes(
    endpoint: &OwlsEndpointSummary,
    limit: usize,
) -> Vec<&crate::owls::OwlsMarketQuote> {
    let mut top = Vec::new();
    for quote in endpoint
        .quotes
        .iter()
        .filter(|quote| quote.decimal_price.is_some())
    {
        let price = quote.decimal_price.unwrap_or_default();
        let insert_at = top
            .iter()
            .position(|existing: &&crate::owls::OwlsMarketQuote| {
                existing.decimal_price.unwrap_or_default() < price
            })
            .unwrap_or(top.len());
        if insert_at < limit {
            top.insert(insert_at, quote);
            if top.len() > limit {
                top.pop();
            }
        } else if top.len() < limit {
            top.push(quote);
        }
    }
    top
}

fn ladder_row(quote: &crate::owls::OwlsMarketQuote) -> Row<'static> {
    let price = quote
        .decimal_price
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| String::from("-"));
    let limit = quote
        .limit_amount
        .map(|value| format!("{value:.0}"))
        .unwrap_or_else(|| String::from("-"));
    Row::new(vec![
        Cell::from(truncate(&quote.book, 10)),
        Cell::from(price).style(
            Style::default()
                .fg(accent_green())
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from(limit).style(Style::default().fg(muted_text())),
    ])
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
            format!(" {} ", title),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .border_style(Style::default().fg(border_color()))
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
        OwlsEndpointGroup::History => muted_text(),
        OwlsEndpointGroup::Realtime => accent_pink(),
    }
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }

    let cutoff = limit.saturating_sub(3);
    let truncated = value.chars().take(cutoff).collect::<String>();
    format!("{truncated}...")
}

fn compact_endpoint_detail(detail: &str) -> String {
    let trimmed = detail.trim();
    if trimmed.is_empty() {
        return String::from("-");
    }
    if trimmed.starts_with("HTTP ") {
        return trimmed
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
    }
    if let Some(http_index) = trimmed.find("HTTP ") {
        let tail = &trimmed[http_index..];
        return tail
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
    }
    if trimmed.to_ascii_lowercase().contains("awaiting") {
        return String::from("awaiting data");
    }
    if trimmed.to_ascii_lowercase().contains("returned") {
        return String::from("response issue");
    }
    truncate(trimmed, 24)
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

fn muted_text() -> Color {
    crate::theme::muted_text()
}

fn panel_background() -> Color {
    crate::theme::panel_background()
}

fn text_color() -> Color {
    crate::theme::text_color()
}

fn border_color() -> Color {
    crate::theme::border_color()
}

fn selected_background() -> Color {
    crate::theme::selected_background()
}

fn selected_text() -> Color {
    crate::theme::selected_text()
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
