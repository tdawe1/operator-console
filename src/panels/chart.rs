use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Rectangle};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::app::{App, IntelRow};
use crate::market_intel::{MarketHistoryPoint, MarketQuoteComparisonRow};
use crate::market_normalization::normalize_key;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let model = build_chart_model(app);
    let shell = section_block("Market Chart", accent_blue());
    let inner = shell.inner(area);
    frame.render_widget(shell, area);

    if inner.width < 20 || inner.height < 8 {
        return;
    }

    let [legend_area, content_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(6)]).areas(inner);
    render_legend(frame, legend_area, &model);

    let [curve_area, ladder_area] =
        Layout::vertical([Constraint::Percentage(65), Constraint::Percentage(35)])
            .areas(content_area);
    let [price_area, volume_area] =
        Layout::vertical([Constraint::Percentage(65), Constraint::Percentage(35)])
            .areas(curve_area);

    render_price_curve(frame, price_area, &model);
    render_volume_histogram(frame, volume_area, &model);
    render_market_ladder(frame, ladder_area, &model);
}

#[derive(Clone)]
struct ChartModel {
    title: String,
    subtitle: String,
    source: String,
    price_points: Vec<(f64, f64)>,
    volume_points: Vec<(f64, f64)>,
    ladder_quotes: Vec<LadderQuote>,
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    trend_up: bool,
    last_price: f64,
    high_price: f64,
    low_price: f64,
    average_price: f64,
    last_volume: f64,
    average_volume: f64,
}

#[derive(Clone)]
struct LadderQuote {
    venue: String,
    side: String,
    price: f64,
    liquidity: Option<f64>,
}

fn build_chart_model(app: &App) -> ChartModel {
    chart_from_intel_history(app)
        .or_else(|| chart_from_owls_quotes(app))
        .or_else(|| chart_from_intel_snapshot(app.selected_intel_row().as_ref()))
        .unwrap_or_else(empty_chart_model)
}

fn chart_from_intel_history(app: &App) -> Option<ChartModel> {
    let dashboard = app.market_intel_dashboard()?;
    let detail = dashboard.event_detail.as_ref()?;
    let selected = app.selected_intel_row()?;
    let history = detail
        .history
        .iter()
        .filter(|point| history_matches_selected(point, &selected))
        .collect::<Vec<_>>();
    if history.len() < 2 {
        return None;
    }

    let price_points = history
        .iter()
        .enumerate()
        .map(|(index, point)| (index as f64, point.price))
        .collect::<Vec<_>>();
    let volume_points = synthetic_volume_series(&history);
    let ladder_quotes = detail
        .quotes
        .iter()
        .filter(|quote| quote_matches_selected(quote, &selected))
        .map(ladder_quote_from_market_quote)
        .collect::<Vec<_>>();

    Some(finalize_chart_model(
        selected.selection,
        format!("{} • {}", truncate(&selected.event, 34), selected.market),
        String::from("event history"),
        price_points,
        volume_points,
        ladder_quotes,
    ))
}

fn chart_from_owls_quotes(app: &App) -> Option<ChartModel> {
    let endpoint = app.selected_owls_endpoint()?;
    let quotes = top_chart_quotes(endpoint, 12);
    if quotes.len() < 2 {
        return None;
    }

    let price_points = quotes
        .iter()
        .enumerate()
        .map(|(index, (_, price))| (index as f64, *price))
        .collect::<Vec<_>>();
    let volume_points = quotes
        .iter()
        .enumerate()
        .map(|(index, (quote, _))| {
            (
                index as f64,
                quote
                    .limit_amount
                    .unwrap_or_else(|| (quotes.len() - index) as f64),
            )
        })
        .collect::<Vec<_>>();
    let ladder_quotes = quotes
        .iter()
        .map(|(quote, price)| LadderQuote {
            venue: quote.book.clone(),
            side: String::from("book"),
            price: *price,
            liquidity: quote.limit_amount,
        })
        .collect::<Vec<_>>();

    Some(finalize_chart_model(
        endpoint.label.clone(),
        format!("{} {}", endpoint.method, endpoint.path),
        String::from("endpoint quotes"),
        price_points,
        volume_points,
        ladder_quotes,
    ))
}

fn chart_from_intel_snapshot(selected: Option<&IntelRow>) -> Option<ChartModel> {
    let row = selected?;
    let mut ladder_quotes = vec![LadderQuote {
        venue: row.bookmaker.clone(),
        side: String::from("back"),
        price: row.back_odds,
        liquidity: row.liquidity,
    }];
    if let Some(fair_odds) = row.fair_odds {
        ladder_quotes.push(LadderQuote {
            venue: String::from("fair"),
            side: String::from("fair"),
            price: fair_odds,
            liquidity: row.liquidity,
        });
    }
    if let Some(lay_odds) = row.lay_odds {
        ladder_quotes.push(LadderQuote {
            venue: row.exchange.clone(),
            side: String::from("lay"),
            price: lay_odds,
            liquidity: row.liquidity,
        });
    }
    if ladder_quotes.len() < 2 {
        return None;
    }

    let price_points = ladder_quotes
        .iter()
        .enumerate()
        .map(|(index, quote)| (index as f64, quote.price))
        .collect::<Vec<_>>();
    let volume_points = ladder_quotes
        .iter()
        .enumerate()
        .map(|(index, quote)| (index as f64, quote.liquidity.unwrap_or(1.0)))
        .collect::<Vec<_>>();

    Some(finalize_chart_model(
        row.selection.clone(),
        format!("{} • {}", truncate(&row.event, 34), row.market),
        String::from("snapshot ladder"),
        price_points,
        volume_points,
        ladder_quotes,
    ))
}

fn finalize_chart_model(
    title: String,
    subtitle: String,
    source: String,
    price_points: Vec<(f64, f64)>,
    volume_points: Vec<(f64, f64)>,
    ladder_quotes: Vec<LadderQuote>,
) -> ChartModel {
    let x_max = price_points.last().map(|(x, _)| *x).unwrap_or(1.0).max(1.0);
    let (min_price, max_price) = if price_points.is_empty() {
        (0.0, 1.0)
    } else {
        let min = price_points
            .iter()
            .map(|(_, value)| *value)
            .fold(f64::INFINITY, f64::min);
        let max = price_points
            .iter()
            .map(|(_, value)| *value)
            .fold(f64::NEG_INFINITY, f64::max);
        (min, max)
    };
    let padding = ((max_price - min_price).abs() * 0.12).max(0.05);
    let last_price = price_points
        .last()
        .map(|(_, value)| *value)
        .unwrap_or_default();
    let average_price = if price_points.is_empty() {
        0.0
    } else {
        price_points.iter().map(|(_, value)| *value).sum::<f64>() / price_points.len() as f64
    };
    let last_volume = volume_points
        .last()
        .map(|(_, value)| *value)
        .unwrap_or_default();
    let average_volume = if volume_points.is_empty() {
        0.0
    } else {
        volume_points.iter().map(|(_, value)| *value).sum::<f64>() / volume_points.len() as f64
    };

    ChartModel {
        title,
        subtitle,
        source,
        price_points: price_points.clone(),
        volume_points,
        ladder_quotes,
        x_bounds: [0.0, x_max],
        y_bounds: [min_price - padding, max_price + padding],
        trend_up: price_points
            .first()
            .zip(price_points.last())
            .map(|(first, last)| last.1 >= first.1)
            .unwrap_or(true),
        last_price,
        high_price: max_price,
        low_price: min_price,
        average_price,
        last_volume,
        average_volume,
    }
}

fn empty_chart_model() -> ChartModel {
    finalize_chart_model(
        String::from("No Market Selected"),
        String::from("Awaiting endpoint quotes or event history"),
        String::from("idle"),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

fn render_legend(frame: &mut Frame<'_>, area: Rect, model: &ChartModel) {
    let compact = area.width < 72;
    let lines = if compact {
        vec![Line::from(vec![
            Span::styled("● ", Style::default().fg(accent_cyan())),
            Span::styled(
                truncate(&model.title, 18),
                Style::default().fg(text_color()),
            ),
            Span::raw("  "),
            Span::styled("Last ", Style::default().fg(muted_text())),
            Span::styled(
                format!("{:.2}", model.last_price),
                Style::default()
                    .fg(if model.trend_up {
                        accent_green()
                    } else {
                        accent_red()
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("Src ", Style::default().fg(muted_text())),
            Span::styled(
                truncate(&model.source, 14),
                Style::default().fg(accent_pink()),
            ),
        ])]
    } else {
        vec![
            Line::from(vec![
                Span::styled(" ● ", Style::default().fg(accent_cyan())),
                Span::styled("Day Session   ", Style::default().fg(muted_text())),
                Span::styled(
                    truncate(&model.title, 22),
                    Style::default().fg(text_color()),
                ),
                Span::raw("  "),
                Span::styled("Last ", Style::default().fg(muted_text())),
                Span::styled(
                    format!("{:.2}", model.last_price),
                    Style::default()
                        .fg(if model.trend_up {
                            accent_green()
                        } else {
                            accent_red()
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("   "),
                Span::styled("High ", Style::default().fg(muted_text())),
                Span::styled(
                    format!("{:.2}", model.high_price),
                    Style::default().fg(text_color()),
                ),
            ]),
            Line::from(vec![
                Span::styled("   ", Style::default()),
                Span::styled(
                    truncate(&model.subtitle, 24),
                    Style::default().fg(accent_gold()),
                ),
                Span::raw("  "),
                Span::styled("Avg ", Style::default().fg(muted_text())),
                Span::styled(
                    format!("{:.2}", model.average_price),
                    Style::default().fg(text_color()),
                ),
                Span::raw("   "),
                Span::styled("Low ", Style::default().fg(muted_text())),
                Span::styled(
                    format!("{:.2}", model.low_price),
                    Style::default().fg(text_color()),
                ),
                Span::raw("   "),
                Span::styled("Source ", Style::default().fg(muted_text())),
                Span::styled(
                    truncate(&model.source, 16),
                    Style::default().fg(accent_pink()),
                ),
            ]),
        ]
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .style(Style::default().bg(panel_background())),
        area,
    );
}

fn render_price_curve(frame: &mut Frame<'_>, area: Rect, model: &ChartModel) {
    let block = section_block("Curve", accent_blue());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 10 || inner.height < 4 {
        return;
    }
    if model.price_points.len() < 2 {
        frame.render_widget(
            Paragraph::new("No chartable series is available yet.")
                .wrap(Wrap { trim: true })
                .style(Style::default().bg(panel_background()).fg(muted_text())),
            inner,
        );
        return;
    }

    let fill_color = if model.trend_up {
        elevated_background()
    } else {
        selected_background()
    };
    let line_color = if model.trend_up {
        accent_cyan()
    } else {
        accent_red()
    };
    let points = model.price_points.clone();
    let x_bounds = model.x_bounds;
    let y_bounds = model.y_bounds;

    frame.render_widget(
        Canvas::default()
            .marker(Marker::Braille)
            .x_bounds(x_bounds)
            .y_bounds(y_bounds)
            .paint(move |ctx| {
                for point in &points {
                    ctx.draw(&CanvasLine {
                        x1: point.0,
                        y1: y_bounds[0],
                        x2: point.0,
                        y2: point.1,
                        color: fill_color,
                    });
                }
                for pair in points.windows(2) {
                    let first = pair[0];
                    let second = pair[1];
                    ctx.draw(&CanvasLine {
                        x1: first.0,
                        y1: first.1,
                        x2: second.0,
                        y2: second.1,
                        color: line_color,
                    });
                }
            }),
        inner,
    );
}

fn render_volume_histogram(frame: &mut Frame<'_>, area: Rect, model: &ChartModel) {
    let block = section_block("Volume Histogram", accent_pink());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 10 || inner.height < 3 {
        return;
    }
    if model.volume_points.is_empty() {
        return;
    }

    let [header_area, chart_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(2)]).areas(inner);

    let x_bounds = model.x_bounds;
    let max_volume = model
        .volume_points
        .iter()
        .map(|(_, value)| *value)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let volumes = model.volume_points.clone();
    let average_volume = model.average_volume;
    let smavg_points = compute_smavg(&volumes, 5);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ░ ", Style::default().fg(accent_pink())),
            Span::styled("Volume ", Style::default().fg(muted_text())),
            Span::styled(
                format!("{:.0} ", model.last_volume),
                Style::default().fg(text_color()),
            ),
            Span::styled(" █ ", Style::default().fg(accent_green())),
            Span::styled("SMAVG(5) ", Style::default().fg(muted_text())),
            Span::styled(
                format!("{average_volume:.0}"),
                Style::default().fg(text_color()),
            ),
        ]))
        .wrap(Wrap { trim: true })
        .style(Style::default().bg(panel_background())),
        header_area,
    );

    frame.render_widget(
        Canvas::default()
            .marker(Marker::Block)
            .x_bounds(x_bounds)
            .y_bounds([0.0, max_volume * 1.1])
            .paint(move |ctx| {
                for point in &volumes {
                    ctx.draw(&Rectangle {
                        x: point.0,
                        y: 0.0,
                        width: 0.8,
                        height: point.1.max(0.1),
                        color: accent_pink(),
                    });
                }
                for pair in smavg_points.windows(2) {
                    let first = pair[0];
                    let second = pair[1];
                    ctx.draw(&CanvasLine {
                        x1: first.0,
                        y1: first.1,
                        x2: second.0,
                        y2: second.1,
                        color: accent_green(),
                    });
                }
                ctx.draw(&CanvasLine {
                    x1: x_bounds[0],
                    y1: average_volume,
                    x2: x_bounds[1],
                    y2: average_volume,
                    color: accent_green(),
                });
            }),
        chart_area,
    );
}

fn render_market_ladder(frame: &mut Frame<'_>, area: Rect, model: &ChartModel) {
    let block = section_block("Book", accent_gold());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 24 || inner.height < 4 {
        return;
    }
    if model.ladder_quotes.is_empty() {
        frame.render_widget(
            Paragraph::new("No ladder quotes available for the current market.")
                .wrap(Wrap { trim: true })
                .style(Style::default().bg(panel_background()).fg(muted_text())),
            inner,
        );
        return;
    }

    let mut quotes = model.ladder_quotes.clone();
    quotes.sort_by(|left, right| right.price.total_cmp(&left.price));
    let [left_area, center_area, right_area] = Layout::horizontal([
        Constraint::Percentage(42),
        Constraint::Length(18),
        Constraint::Percentage(42),
    ])
    .areas(inner);

    render_ladder_table(
        frame,
        left_area,
        "Top Books",
        &quotes[..quotes.len().min(4)],
        accent_green(),
    );
    let split_index = quotes.len().min(4);
    render_ladder_table(
        frame,
        right_area,
        "Field",
        &quotes[split_index..],
        accent_gold(),
    );

    let best = quotes.first().map(|quote| quote.price).unwrap_or_default();
    let low = quotes.last().map(|quote| quote.price).unwrap_or_default();
    let liquidity = quotes
        .iter()
        .filter_map(|quote| quote.liquidity)
        .sum::<f64>();
    let center = Paragraph::new(vec![
        Line::styled(
            truncate(&model.title, 16),
            Style::default()
                .fg(text_color())
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Best ", Style::default().fg(accent_cyan())),
            Span::raw(format!("{best:.2}")),
        ]),
        Line::from(vec![
            Span::styled("Low  ", Style::default().fg(accent_gold())),
            Span::raw(format!("{low:.2}")),
        ]),
        Line::from(vec![
            Span::styled("Range ", Style::default().fg(accent_pink())),
            Span::raw(format!("{:.2}", best - low)),
        ]),
        Line::from(vec![
            Span::styled("Vol   ", Style::default().fg(muted_text())),
            Span::raw(format!("{liquidity:.0}")),
        ]),
        Line::from(vec![
            Span::styled("Now   ", Style::default().fg(muted_text())),
            Span::raw(format!("{:.0}", model.last_volume)),
        ]),
    ])
    .wrap(Wrap { trim: true })
    .style(Style::default().bg(panel_background()));
    frame.render_widget(center, center_area);
}

fn top_chart_quotes(
    endpoint: &crate::owls::OwlsEndpointSummary,
    limit: usize,
) -> Vec<(&crate::owls::OwlsMarketQuote, f64)> {
    let mut top = Vec::new();
    for quote in endpoint
        .quotes
        .iter()
        .filter_map(|quote| quote.decimal_price.map(|price| (quote, price)))
    {
        let insert_at = top
            .iter()
            .position(|existing: &(&crate::owls::OwlsMarketQuote, f64)| existing.1 < quote.1)
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

fn render_ladder_table(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    quotes: &[LadderQuote],
    accent: Color,
) {
    let rows = if quotes.is_empty() {
        vec![Row::new(vec![
            Cell::from(String::from("-")),
            Cell::from(String::from("-")),
            Cell::from(String::from("-")),
        ])]
    } else {
        quotes
            .iter()
            .take(4)
            .map(|quote| {
                Row::new(vec![
                    Cell::from(truncate(&quote.venue, 10)),
                    Cell::from(format!("{:.2}", quote.price))
                        .style(Style::default().fg(accent).add_modifier(Modifier::BOLD)),
                    Cell::from(
                        quote
                            .liquidity
                            .map(|value| format!("{value:.0}"))
                            .unwrap_or_else(|| quote.side.clone()),
                    )
                    .style(Style::default().fg(muted_text())),
                ])
            })
            .collect::<Vec<_>>()
    };

    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Length(8),
                Constraint::Min(6),
            ],
        )
        .header(
            Row::new(vec!["Venue", "Price", "Flow"])
                .style(Style::default().fg(accent).add_modifier(Modifier::BOLD)),
        )
        .column_spacing(1)
        .block(section_block(title, accent)),
        area,
    );
}

fn history_matches_selected(point: &MarketHistoryPoint, selected: &IntelRow) -> bool {
    normalize_key(&point.market_name) == normalize_key(&selected.market)
        && normalize_key(&point.selection_name) == normalize_key(&selected.selection)
}

fn quote_matches_selected(quote: &MarketQuoteComparisonRow, selected: &IntelRow) -> bool {
    normalize_key(&quote.market_name) == normalize_key(&selected.market)
        && normalize_key(&quote.selection_name) == normalize_key(&selected.selection)
}

fn ladder_quote_from_market_quote(quote: &MarketQuoteComparisonRow) -> LadderQuote {
    LadderQuote {
        venue: quote.venue.clone(),
        side: if quote.side.trim().is_empty() {
            String::from("quote")
        } else {
            quote.side.clone()
        },
        price: quote.price.unwrap_or_default(),
        liquidity: quote.liquidity,
    }
}

fn synthetic_volume_series(history: &[&MarketHistoryPoint]) -> Vec<(f64, f64)> {
    history
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let previous_price = if index == 0 {
                point.price
            } else {
                history[index - 1].price
            };
            let activity = 1.0 + (point.price - previous_price).abs() * 120.0;
            (index as f64, activity)
        })
        .collect()
}

fn compute_smavg(values: &[(f64, f64)], window: usize) -> Vec<(f64, f64)> {
    if values.is_empty() || window == 0 {
        return Vec::new();
    }

    values
        .iter()
        .enumerate()
        .map(|(index, (x, _))| {
            let start = index.saturating_sub(window.saturating_sub(1));
            let slice = &values[start..=index];
            let average = slice.iter().map(|(_, value)| *value).sum::<f64>() / slice.len() as f64;
            (*x, average)
        })
        .collect()
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

fn elevated_background() -> Color {
    crate::theme::elevated_background()
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

fn selected_background() -> Color {
    crate::theme::selected_background()
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
