use std::collections::{BTreeMap, BTreeSet};

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Borders, Cell, Chart, Dataset, Gauge, GraphType, Paragraph, Row, Table, Wrap,
};
use ratatui::Frame;

use crate::domain::{ExchangePanelSnapshot, OpenPositionRow, TrackedBetRow, VenueStatus};
use crate::exchange_api::MatchbookAccountState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FundingKind {
    Standard,
    Promo,
    Unknown,
}

#[derive(Debug, Clone)]
struct RunningPnlPoint {
    sequence: usize,
    at: String,
    total: f64,
    promo: f64,
}

#[derive(Debug, Clone, Default)]
struct RunningPnlSummary {
    points: Vec<RunningPnlPoint>,
    realised_total: f64,
    marked_total: f64,
    standard_count: usize,
    promo_count: usize,
    unknown_count: usize,
    source: &'static str,
}

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    matchbook: Option<&MatchbookAccountState>,
) {
    let total_open_stake: f64 = snapshot.open_positions.iter().map(|row| row.stake).sum();
    let total_liability: f64 = snapshot
        .open_positions
        .iter()
        .map(|row| row.liability)
        .sum();
    let total_open_pnl: f64 = snapshot
        .open_positions
        .iter()
        .map(|row| row.pnl_amount)
        .sum();
    let total_bet_stake: f64 = snapshot.other_open_bets.iter().map(|row| row.stake).sum();
    let actionable_decisions = snapshot
        .exit_recommendations
        .iter()
        .filter(|recommendation| recommendation.action != "hold")
        .count();
    let tracked_source_count = snapshot
        .tracked_bets
        .iter()
        .flat_map(|tracked_bet| tracked_bet.legs.iter().map(|leg| leg.venue.as_str()))
        .collect::<BTreeSet<_>>()
        .len();
    let runtime = snapshot.runtime.as_ref();

    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3), // row 1
        Constraint::Length(2), // row 2
        Constraint::Min(10),   // lower
    ])
    .split(area);

    let ratios = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(layout[1]);
    let action_queue_area = Layout::horizontal([Constraint::Percentage(100)]).split(layout[2])[0];

    let lower = Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)])
        .split(layout[3]);
    let left = Layout::vertical([Constraint::Length(10), Constraint::Min(7)]).split(lower[0]);
    let right = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Min(7),
    ])
    .split(lower[1]);
    let running_pnl = build_running_pnl_summary(snapshot, total_open_pnl);

    render_summary_cards(
        frame,
        layout[0],
        snapshot,
        total_open_pnl,
        total_liability,
        total_open_stake,
        total_bet_stake,
        actionable_decisions,
        tracked_source_count,
        runtime,
    );

    let exposure_ratio = snapshot
        .account_stats
        .as_ref()
        .map(|account| ratio(account.exposure, account.available_balance))
        .unwrap_or(0.0);
    let trade_ready_ratio = if snapshot.open_positions.is_empty() {
        0.0
    } else {
        snapshot
            .open_positions
            .iter()
            .filter(|row| row.can_trade_out)
            .count() as f64
            / snapshot.open_positions.len() as f64
    };
    let action_ratio = if snapshot.decisions.is_empty() {
        0.0
    } else {
        actionable_decisions as f64 / snapshot.decisions.len() as f64
    };

    render_gauge(
        frame,
        ratios[0],
        "Exposure vs Balance",
        exposure_ratio,
        accent_pink(),
        format!("{:.0}%", exposure_ratio * 100.0),
    );
    render_gauge(
        frame,
        ratios[1],
        "Trade-out Ready",
        trade_ready_ratio,
        accent_green(),
        format!(
            "{}/{}",
            snapshot
                .open_positions
                .iter()
                .filter(|row| row.can_trade_out)
                .count(),
            snapshot.open_positions.len()
        ),
    );
    // Gauge 3 now in its own dedicated row constraint
    render_gauge(
        frame,
        action_queue_area,
        "Action Queue",
        action_ratio,
        accent_gold(),
        format!("{}/{}", actionable_decisions, snapshot.decisions.len()),
    );

    // Remaining rendering needs to shift to lower constraints or layout rows
    // ... need to adjust lower/left/right layouts if they rely on layout[2] ...

    render_running_pnl_chart(frame, left[0], &running_pnl);
    render_capital_table(
        frame,
        left[1],
        snapshot,
        total_open_stake,
        total_liability,
        total_open_pnl,
        total_bet_stake,
    );
    render_venue_table(frame, right[0], snapshot);
    render_decision_table(frame, right[1], snapshot);
    render_matchbook_table(frame, right[2], snapshot, matchbook);
}

fn render_summary_cards(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    total_open_pnl: f64,
    total_liability: f64,
    total_open_stake: f64,
    total_bet_stake: f64,
    actionable_decisions: usize,
    tracked_source_count: usize,
    runtime: Option<&crate::domain::RuntimeSummary>,
) {
    let cards = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .split(area);

    let coverage = Paragraph::new(vec![
        Line::styled("󰄨 coverage", Style::default().fg(muted_text())),
        Line::styled(
            format!(
                "{} venues • {} tracked",
                snapshot.venues.len(),
                snapshot.tracked_bets.len()
            ),
            Style::default()
                .fg(accent_blue())
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(format!(
            "{} exchange • {} sportsbook",
            snapshot.open_positions.len(),
            snapshot.other_open_bets.len()
        )),
    ])
    .block(section_block("󰊠 Trading Stats", accent_blue()))
    .wrap(Wrap { trim: true });
    frame.render_widget(coverage, cards[0]);

    let capital = Paragraph::new(vec![
        Line::styled("󰞇 open risk", Style::default().fg(muted_text())),
        Line::styled(
            format!(
                "{:.2} stake • {:.2} liability",
                total_open_stake, total_liability
            ),
            Style::default()
                .fg(accent_pink())
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(format!("sportsbook stake {:.2}", total_bet_stake)),
    ])
    .block(section_block("󰖌 Capital", accent_pink()))
    .wrap(Wrap { trim: true });
    frame.render_widget(capital, cards[1]);

    let flow = Paragraph::new(vec![
        Line::styled("󰍵 action queue", Style::default().fg(muted_text())),
        Line::styled(
            format!(
                "{} actionable • {} decisions",
                actionable_decisions,
                snapshot.decisions.len()
            ),
            Style::default()
                .fg(accent_gold())
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(format!(
            "{} watch rows • {} sources",
            snapshot
                .watch
                .as_ref()
                .map(|watch| watch.watch_count)
                .unwrap_or(0),
            tracked_source_count,
        )),
        Line::raw(format!(
            "mtm {:+.2} • {}",
            total_open_pnl,
            runtime
                .map(|summary| summary.updated_at.as_str())
                .unwrap_or("unknown")
        )),
    ])
    .block(section_block("󰆼 Flow", accent_gold()))
    .wrap(Wrap { trim: true });
    frame.render_widget(flow, cards[2]);
}

fn render_venue_table(frame: &mut Frame<'_>, area: Rect, snapshot: &ExchangePanelSnapshot) {
    if snapshot.venues.is_empty() {
        let body = Paragraph::new(vec![
            Line::raw("No venue summaries loaded."),
            Line::raw("Refresh the provider or start the configured capture source."),
        ])
        .block(section_block("󰀶 Venue Split", accent_cyan()))
        .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    }

    let rows = snapshot.venues.iter().map(|venue| {
        Row::new(vec![
            Cell::from(venue.label.clone()),
            Cell::from(format!("{:?}", venue.status)),
            Cell::from(venue.event_count.to_string()),
            Cell::from(venue.market_count.to_string()),
        ])
        .style(Style::default().fg(venue_status_color(venue.status)))
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(38),
            Constraint::Length(10),
            Constraint::Length(7),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec!["Venue", "State", "Events", "Markets"]).style(
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(section_block("󰀶 Venue Split", accent_cyan()))
    .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_running_pnl_chart(frame: &mut Frame<'_>, area: Rect, summary: &RunningPnlSummary) {
    if summary.points.is_empty() {
        let body = Paragraph::new(vec![
            Line::styled("󱂬 running pnl", Style::default().fg(muted_text())),
            Line::raw("No settled P/L history is loaded yet."),
            Line::raw("Load ledger history, tracked bets, or settled positions to draw the curve."),
        ])
        .block(section_block("󰁔 Running P/L", accent_green()))
        .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    }

    let header = Layout::vertical([Constraint::Length(3), Constraint::Min(7)]).split(area);
    let net_total = summary.realised_total + summary.marked_total;
    let headline = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("realised ", Style::default().fg(muted_text())),
            Span::styled(
                format!("{:+.2}", summary.realised_total),
                Style::default()
                    .fg(pnl_color(summary.realised_total))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled("live ", Style::default().fg(muted_text())),
            Span::styled(
                format!("{:+.2}", summary.marked_total),
                Style::default().fg(pnl_color(summary.marked_total)),
            ),
            Span::raw("   "),
            Span::styled("net ", Style::default().fg(muted_text())),
            Span::styled(
                format!("{:+.2}", net_total),
                Style::default()
                    .fg(pnl_color(net_total))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::raw(format!(
            "source {} • std {} • promo {} • unknown {}",
            summary.source, summary.standard_count, summary.promo_count, summary.unknown_count
        )),
    ])
    .block(section_block("󰁔 Running P/L", accent_green()))
    .wrap(Wrap { trim: true });
    frame.render_widget(headline, header[0]);

    let total_points = chart_points(
        summary
            .points
            .iter()
            .map(|point| (point.sequence, point.total)),
    );
    let promo_points = chart_points(
        summary
            .points
            .iter()
            .map(|point| (point.sequence, point.promo)),
    );
    let y_bounds = pnl_bounds(summary);
    let x_bounds = [0.0, total_points.last().map(|point| point.0).unwrap_or(1.0)];
    let x_labels = chart_x_labels(summary);
    let y_labels = chart_y_labels(y_bounds);

    let mut datasets = vec![Dataset::default()
        .name("Total")
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(accent_cyan()))
        .data(&total_points)];
    if summary.promo_count > 0 {
        datasets.push(
            Dataset::default()
                .name("Promo")
                .marker(Marker::Dot)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(accent_gold()))
                .data(&promo_points),
        );
    }

    let chart = Chart::new(datasets)
        .block(section_block("󰄧 Curve", accent_blue()))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(muted_text()))
                .bounds(x_bounds)
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(muted_text()))
                .bounds(y_bounds)
                .labels(y_labels),
        );
    frame.render_widget(chart, header[1]);
}

fn render_capital_table(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    total_open_stake: f64,
    total_liability: f64,
    total_open_pnl: f64,
    total_bet_stake: f64,
) {
    let average_position_size = if snapshot.open_positions.is_empty() {
        0.0
    } else {
        total_open_stake / snapshot.open_positions.len() as f64
    };
    let account = snapshot.account_stats.as_ref();
    let rows = vec![
        key_value_row(
            "󰟈 Balance",
            account
                .map(|stats| format!("{:.2} {}", stats.available_balance, stats.currency))
                .unwrap_or_else(|| String::from("-")),
            accent_green(),
        ),
        key_value_row(
            "󰞇 Exchange stake",
            format!("{total_open_stake:.2}"),
            accent_blue(),
        ),
        key_value_row(
            "󰖌 Liability",
            format!("{total_liability:.2}"),
            accent_pink(),
        ),
        key_value_row(
            "󱂬 Marked P/L",
            format!("{total_open_pnl:+.2}"),
            pnl_color(total_open_pnl),
        ),
        key_value_row(
            "󰇚 Sportsbook stake",
            format!("{total_bet_stake:.2}"),
            accent_gold(),
        ),
        key_value_row(
            "󰔉 Avg position",
            format!("{average_position_size:.2}"),
            text_color(),
        ),
        key_value_row(
            "󰹈 Implied band",
            probability_band_line(snapshot),
            muted_text(),
        ),
        key_value_row(
            "󰔟 Exit policy",
            format!(
                "target {:.2} | stop {:.2} | warn {}",
                snapshot.exit_policy.target_profit,
                snapshot.exit_policy.stop_loss,
                snapshot.exit_policy.warn_only_default,
            ),
            muted_text(),
        ),
    ];

    let table = Table::new(rows, [Constraint::Length(15), Constraint::Min(10)])
        .block(section_block("󰖌 Capital & Risk", accent_pink()))
        .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_decision_table(frame: &mut Frame<'_>, area: Rect, snapshot: &ExchangePanelSnapshot) {
    let mut counts = BTreeMap::<String, usize>::new();
    for decision in &snapshot.decisions {
        *counts.entry(decision.status.clone()).or_default() += 1;
    }
    if counts.is_empty() {
        let body = Paragraph::new(vec![
            Line::raw("No decision rows loaded."),
            Line::raw(
                "Watch recommendations will appear here once the recorder has a live snapshot.",
            ),
        ])
        .block(section_block("󰍵 Decision Mix", accent_gold()))
        .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    }

    let rows = counts.into_iter().map(|(status, count)| {
        let label = if status.is_empty() {
            String::from("unknown")
        } else {
            status
        };
        Row::new(vec![
            Cell::from(label.clone()),
            Cell::from(count.to_string()),
            Cell::from(decision_hint(&label)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(34),
            Constraint::Length(7),
            Constraint::Min(16),
        ],
    )
    .header(
        Row::new(vec!["Status", "Count", "Meaning"]).style(
            Style::default()
                .fg(accent_gold())
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(section_block("󰍵 Decision Mix", accent_gold()))
    .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_matchbook_table(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    matchbook: Option<&MatchbookAccountState>,
) {
    if let Some(matchbook) = matchbook {
        let open_offer_stake: f64 = matchbook
            .current_offers
            .iter()
            .filter_map(|offer| offer.remaining_stake.or(offer.stake))
            .sum();
        let current_bet_stake: f64 = matchbook
            .current_bets
            .iter()
            .filter_map(|bet| bet.stake)
            .sum();
        let net_exposure: f64 = matchbook
            .positions
            .iter()
            .filter_map(|position| position.exposure)
            .sum();
        let layout = Layout::vertical([Constraint::Length(5), Constraint::Min(4)]).split(area);
        let summary = Paragraph::new(vec![
            Line::styled("󰍹 account", Style::default().fg(muted_text())),
            Line::styled(
                matchbook.balance_label.clone(),
                Style::default()
                    .fg(accent_green())
                    .add_modifier(Modifier::BOLD),
            ),
            Line::raw(format!(
                "offers {} • bets {} • matched {} • positions {}",
                matchbook.summary.open_offer_count,
                matchbook.summary.current_bet_count,
                matchbook.summary.matched_bet_count,
                matchbook.summary.position_count
            )),
            Line::raw(format!(
                "offer stake {:.2} • bet stake {:.2} • net exposure {:+.2}",
                open_offer_stake, current_bet_stake, net_exposure
            )),
        ])
        .block(section_block("󰇚 Matchbook API", accent_blue()))
        .wrap(Wrap { trim: true });
        frame.render_widget(summary, layout[0]);

        let rows = matchbook
            .current_offers
            .iter()
            .take(5)
            .map(|offer| {
                Row::new(vec![
                    Cell::from(offer.selection_name.clone()),
                    Cell::from(offer.side.clone()),
                    Cell::from(
                        offer
                            .odds
                            .map(|value| format!("{value:.2}"))
                            .unwrap_or_else(|| String::from("-")),
                    ),
                    Cell::from(
                        offer
                            .remaining_stake
                            .or(offer.stake)
                            .map(|value| format!("{value:.2}"))
                            .unwrap_or_else(|| String::from("-")),
                    ),
                    Cell::from(offer.status.clone()),
                ])
            })
            .collect::<Vec<_>>();
        if !rows.is_empty() {
            let table = Table::new(
                rows,
                [
                    Constraint::Percentage(42),
                    Constraint::Length(7),
                    Constraint::Length(6),
                    Constraint::Length(7),
                    Constraint::Length(10),
                ],
            )
            .header(
                Row::new(vec!["Selection", "Side", "Odds", "Stake", "Status"]).style(
                    Style::default()
                        .fg(accent_green())
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .block(section_block("󰋼 Matchbook Orders", accent_green()))
            .column_spacing(1);
            frame.render_widget(table, layout[1]);
            return;
        }

        let bet_rows = matchbook
            .current_bets
            .iter()
            .take(5)
            .map(|bet| {
                Row::new(vec![
                    Cell::from(bet.selection_name.clone()),
                    Cell::from(bet.side.clone()),
                    Cell::from(
                        bet.odds
                            .map(|value| format!("{value:.2}"))
                            .unwrap_or_else(|| String::from("-")),
                    ),
                    Cell::from(
                        bet.stake
                            .map(|value| format!("{value:.2}"))
                            .unwrap_or_else(|| String::from("-")),
                    ),
                    Cell::from(
                        bet.profit_loss
                            .map(|value| format!("{value:+.2}"))
                            .unwrap_or_else(|| bet.status.clone()),
                    ),
                ])
            })
            .collect::<Vec<_>>();
        if !bet_rows.is_empty() {
            let table = Table::new(
                bet_rows,
                [
                    Constraint::Percentage(42),
                    Constraint::Length(7),
                    Constraint::Length(6),
                    Constraint::Length(7),
                    Constraint::Length(10),
                ],
            )
            .header(
                Row::new(vec!["Selection", "Side", "Odds", "Stake", "P/L"]).style(
                    Style::default()
                        .fg(accent_green())
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .block(section_block("󱂬 Matchbook Bets", accent_green()))
            .column_spacing(1);
            frame.render_widget(table, layout[1]);
            return;
        }

        let position_rows = matchbook
            .positions
            .iter()
            .take(5)
            .map(|position| {
                Row::new(vec![
                    Cell::from(position.selection_name.clone()),
                    Cell::from(
                        position
                            .exposure
                            .map(|value| format!("{value:+.2}"))
                            .unwrap_or_else(|| String::from("-")),
                    ),
                    Cell::from(
                        position
                            .profit_loss
                            .map(|value| format!("{value:+.2}"))
                            .unwrap_or_else(|| String::from("-")),
                    ),
                ])
            })
            .collect::<Vec<_>>();
        if !position_rows.is_empty() {
            let table = Table::new(
                position_rows,
                [
                    Constraint::Percentage(54),
                    Constraint::Length(10),
                    Constraint::Length(10),
                ],
            )
            .header(
                Row::new(vec!["Selection", "Exposure", "P/L"]).style(
                    Style::default()
                        .fg(accent_green())
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .block(section_block("󰬍 Matchbook Positions", accent_green()))
            .column_spacing(1);
            frame.render_widget(table, layout[1]);
            return;
        }

        let body = Paragraph::new(vec![
            Line::raw("No current Matchbook offers, bets, or positions."),
            Line::raw("Exchange inventory will appear here when the API sees it."),
        ])
        .block(section_block("󰋼 Matchbook Orders", accent_green()))
        .wrap(Wrap { trim: true });
        frame.render_widget(body, layout[1]);
        return;
    }

    let mut counts = BTreeMap::<String, usize>::new();
    for tracked_bet in &snapshot.tracked_bets {
        let source = if !tracked_bet.platform.is_empty() {
            tracked_bet.platform.clone()
        } else if let Some(first_leg) = tracked_bet.legs.first() {
            first_leg.venue.clone()
        } else if !tracked_bet.platform_kind.is_empty() {
            tracked_bet.platform_kind.clone()
        } else {
            String::from("unknown")
        };
        *counts.entry(source).or_default() += 1;
    }
    counts
        .entry(String::from("sportsbook_open"))
        .or_insert(snapshot.other_open_bets.len());
    counts
        .entry(String::from("exchange_open"))
        .or_insert(snapshot.open_positions.len());

    let rows = counts
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(source, count)| Row::new(vec![Cell::from(source), Cell::from(count.to_string())]))
        .collect::<Vec<_>>();

    if rows.is_empty() {
        let body = Paragraph::new(vec![
            Line::raw("No tracked activity loaded."),
            Line::raw("Capture backfills and ledger imports will populate this board."),
        ])
        .block(section_block("󰋼 Tracked Mix", accent_green()))
        .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    }

    let table = Table::new(rows, [Constraint::Percentage(60), Constraint::Length(8)])
        .header(
            Row::new(vec!["Source", "Count"]).style(
                Style::default()
                    .fg(accent_green())
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(section_block("󰋼 Tracked Mix", accent_green()))
        .column_spacing(1);
    frame.render_widget(table, area);
}

fn decision_hint(status: &str) -> &'static str {
    match status {
        "take_profit_ready" => "profit take",
        "stop_loss_ready" => "stop loss",
        "cash_out" => "cash out",
        "hold" => "monitor",
        "monitor_only" => "watch only",
        _ => "custom",
    }
}

fn key_value_row(label: &'static str, value: String, color: Color) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(
            label,
            Style::default()
                .fg(muted_text())
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(value, Style::default().fg(color))),
    ])
}

fn render_gauge(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &'static str,
    value: f64,
    color: Color,
    label: String,
) {
    let gauge = Gauge::default()
        .block(section_block(title, color))
        .gauge_style(Style::default().fg(color).bg(panel_background()))
        .ratio(value.clamp(0.0, 1.0))
        .label(Span::raw(label));
    frame.render_widget(gauge, area);
}

fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator <= 0.0 {
        0.0
    } else {
        (numerator / denominator).clamp(0.0, 1.0)
    }
}

#[allow(dead_code)]
fn total_market_ev(snapshot: &ExchangePanelSnapshot) -> Option<f64> {
    let watch = snapshot.watch.as_ref()?;
    let values = watch
        .watches
        .iter()
        .filter_map(|row| {
            let current_back_odds = row.current_back_odds?;
            let win_probability = 1.0 / current_back_odds;
            let lose_probability = 1.0 - win_probability;
            let effective_commission = if watch.commission_rate > 1.0 {
                watch.commission_rate / 100.0
            } else {
                watch.commission_rate
            };
            let selection_loses_pnl = row.total_stake * (1.0 - effective_commission);
            let selection_wins_pnl = -row.total_liability;
            Some((lose_probability * selection_loses_pnl) + (win_probability * selection_wins_pnl))
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values.into_iter().sum())
    }
}

fn probability_band_line(snapshot: &ExchangePanelSnapshot) -> String {
    let Some(watch) = snapshot.watch.as_ref() else {
        return String::from("no watch plan");
    };
    if watch.watches.is_empty() {
        return String::from("no grouped rows");
    }

    let entry_average = watch
        .watches
        .iter()
        .map(|row| row.entry_implied_probability)
        .sum::<f64>()
        / watch.watches.len() as f64;
    let target_average = watch
        .watches
        .iter()
        .map(|row| row.profit_take_implied_probability)
        .sum::<f64>()
        / watch.watches.len() as f64;
    let stop_average = watch
        .watches
        .iter()
        .map(|row| row.stop_loss_implied_probability)
        .sum::<f64>()
        / watch.watches.len() as f64;

    format!(
        "entry {:.1}% | target {:.1}% | stop {:.1}%",
        entry_average * 100.0,
        target_average * 100.0,
        stop_average * 100.0,
    )
}

fn build_running_pnl_summary(
    snapshot: &ExchangePanelSnapshot,
    total_open_pnl: f64,
) -> RunningPnlSummary {
    if snapshot.ledger_pnl_summary.settled_count > 0 {
        return RunningPnlSummary {
            points: snapshot
                .ledger_pnl_summary
                .points
                .iter()
                .enumerate()
                .map(|(index, point)| RunningPnlPoint {
                    sequence: index,
                    at: point.occurred_at.clone(),
                    total: point.total,
                    promo: point.promo_total,
                })
                .collect(),
            realised_total: snapshot.ledger_pnl_summary.realised_total,
            marked_total: total_open_pnl,
            standard_count: snapshot.ledger_pnl_summary.standard_count,
            promo_count: snapshot.ledger_pnl_summary.promo_count,
            unknown_count: snapshot.ledger_pnl_summary.unknown_count,
            source: "ledger",
        };
    }

    let tracked_history = build_tracked_bet_pnl_points(snapshot);
    if !tracked_history.points.is_empty() {
        return RunningPnlSummary {
            marked_total: total_open_pnl,
            ..tracked_history
        };
    }

    let fallback_points = build_historical_position_pnl_points(snapshot);
    if !fallback_points.is_empty() {
        let realised_total = fallback_points
            .last()
            .map(|point| point.total)
            .unwrap_or(0.0);
        return RunningPnlSummary {
            points: fallback_points,
            realised_total,
            marked_total: total_open_pnl,
            standard_count: 0,
            promo_count: 0,
            unknown_count: snapshot.historical_positions.len(),
            source: "history",
        };
    }

    RunningPnlSummary {
        marked_total: total_open_pnl,
        ..RunningPnlSummary::default()
    }
}

fn build_tracked_bet_pnl_points(snapshot: &ExchangePanelSnapshot) -> RunningPnlSummary {
    let mut rows = snapshot
        .tracked_bets
        .iter()
        .filter_map(|bet| {
            let realised_pnl = bet.realised_pnl_gbp?;
            let occurred_at = tracked_bet_occurred_at(bet);
            if occurred_at.is_empty() {
                return None;
            }
            Some((
                occurred_at,
                bet.bet_id.clone(),
                realised_pnl,
                classify_funding(bet),
            ))
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

    let mut total = 0.0;
    let mut promo_total = 0.0;
    let mut standard_count = 0;
    let mut promo_count = 0;
    let mut unknown_count = 0;
    let mut points = Vec::with_capacity(rows.len());
    for (index, (occurred_at, _, realised_pnl, funding)) in rows.into_iter().enumerate() {
        total += realised_pnl;
        match funding {
            FundingKind::Standard => {
                standard_count += 1;
            }
            FundingKind::Promo => {
                promo_count += 1;
                promo_total += realised_pnl;
            }
            FundingKind::Unknown => {
                unknown_count += 1;
            }
        }
        points.push(RunningPnlPoint {
            sequence: index,
            at: occurred_at,
            total,
            promo: promo_total,
        });
    }

    RunningPnlSummary {
        realised_total: total,
        standard_count,
        promo_count,
        unknown_count,
        points,
        source: "tracked",
        ..RunningPnlSummary::default()
    }
}

fn build_historical_position_pnl_points(snapshot: &ExchangePanelSnapshot) -> Vec<RunningPnlPoint> {
    let mut rows = snapshot
        .historical_positions
        .iter()
        .filter_map(|row| {
            historical_position_occurred_at(row).map(|occurred_at| (occurred_at, row.pnl_amount))
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0));

    let mut total = 0.0;
    rows.into_iter()
        .enumerate()
        .map(|(index, (occurred_at, pnl_amount))| {
            total += pnl_amount;
            RunningPnlPoint {
                sequence: index,
                at: occurred_at,
                total,
                promo: 0.0,
            }
        })
        .collect()
}

fn tracked_bet_occurred_at(bet: &TrackedBetRow) -> String {
    if !bet.settled_at.is_empty() {
        return bet.settled_at.clone();
    }
    if !bet.placed_at.is_empty() {
        return bet.placed_at.clone();
    }
    for activity in &bet.activities {
        if !activity.occurred_at.is_empty() {
            return activity.occurred_at.clone();
        }
    }
    String::new()
}

fn historical_position_occurred_at(row: &OpenPositionRow) -> Option<String> {
    if looks_like_iso_timestamp(&row.live_clock) {
        return Some(row.live_clock.clone());
    }
    if let Some(date) = event_date_from_row(row) {
        let time = event_time_from_row(row).unwrap_or_else(|| String::from("00:00"));
        return Some(format!("{date}T{time}:00"));
    }
    None
}

fn event_date_from_row(row: &OpenPositionRow) -> Option<String> {
    if let Some((date, _)) = parse_iso_timestamp(row.event_status.split('|').next().unwrap_or("")) {
        return Some(date);
    }
    if let Some((date, _)) = parse_iso_timestamp(&row.live_clock) {
        return Some(date);
    }
    if let Some((date, _)) = parse_url_datetime(&row.event_url) {
        return Some(date);
    }
    None
}

fn event_time_from_row(row: &OpenPositionRow) -> Option<String> {
    if let Some((_, time)) = parse_iso_timestamp(row.event_status.split('|').next().unwrap_or("")) {
        return Some(time);
    }
    if let Some((_, time)) = parse_iso_timestamp(&row.live_clock) {
        return Some(time);
    }
    if let Some((_, time)) = parse_url_datetime(&row.event_url) {
        return Some(time);
    }
    None
}

fn parse_iso_timestamp(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim();
    if trimmed.len() < 16 {
        return None;
    }
    let bytes = trimmed.as_bytes();
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
    {
        return None;
    }
    Some((
        trimmed.get(0..10)?.to_string(),
        trimmed.get(11..16)?.to_string(),
    ))
}

fn parse_url_datetime(event_url: &str) -> Option<(String, String)> {
    let segments = event_url
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    for window in segments.windows(4) {
        let [year, month, day, time] = window else {
            continue;
        };
        if year.len() != 4
            || month.len() != 2
            || day.len() != 2
            || time.len() < 5
            || !year.chars().all(|c| c.is_ascii_digit())
            || !month.chars().all(|c| c.is_ascii_digit())
            || !day.chars().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        let bytes = time.as_bytes();
        if bytes.get(2) == Some(&b'-')
            && bytes[0..2].iter().all(|byte| byte.is_ascii_digit())
            && bytes[3..5].iter().all(|byte| byte.is_ascii_digit())
        {
            return Some((
                format!("{year}-{month}-{day}"),
                format!("{}:{}", &time[0..2], &time[3..5]),
            ));
        }
    }
    None
}

fn looks_like_iso_timestamp(value: &str) -> bool {
    parse_iso_timestamp(value).is_some()
}

fn classify_funding(bet: &TrackedBetRow) -> FundingKind {
    match bet.funding_kind.trim().to_ascii_lowercase().as_str() {
        "cash" => return FundingKind::Standard,
        "free_bet" | "risk_free" | "bonus" => return FundingKind::Promo,
        _ => {}
    }

    let notes = bet.notes.to_lowercase();
    let bet_type = bet.bet_type.to_lowercase();
    let status = bet.status.to_lowercase();
    let haystack = format!("{notes} {bet_type} {status}");

    if [
        "free bet",
        "freebet",
        "snr",
        "stake returned",
        "risk free",
        "refund",
        "bonus",
        "promo",
        "promotion",
        "boost",
    ]
    .iter()
    .any(|keyword| haystack.contains(keyword))
    {
        return FundingKind::Promo;
    }

    if ["qualifying", "cash", "normal"]
        .iter()
        .any(|keyword| haystack.contains(keyword))
    {
        return FundingKind::Standard;
    }

    FundingKind::Unknown
}

fn chart_points(points: impl Iterator<Item = (usize, f64)>) -> Vec<(f64, f64)> {
    let mut output = points
        .map(|(index, value)| (index as f64, value))
        .collect::<Vec<_>>();
    if output.len() == 1 {
        output.push((1.0, output[0].1));
    }
    output
}

fn pnl_bounds(summary: &RunningPnlSummary) -> [f64; 2] {
    let mut min_value: f64 = 0.0;
    let mut max_value: f64 = 0.0;
    for point in &summary.points {
        min_value = min_value.min(point.total).min(point.promo);
        max_value = max_value.max(point.total).max(point.promo);
    }
    if (max_value - min_value).abs() < f64::EPSILON {
        return [min_value - 1.0, max_value + 1.0];
    }
    let padding = ((max_value - min_value) * 0.1).max(0.5);
    [min_value - padding, max_value + padding]
}

fn chart_x_labels(summary: &RunningPnlSummary) -> Vec<Line<'static>> {
    let first = summary
        .points
        .first()
        .map(|point| compact_label(&point.at))
        .unwrap_or_else(|| String::from("start"));
    let middle = summary
        .points
        .get(summary.points.len().saturating_sub(1) / 2)
        .map(|point| compact_label(&point.at))
        .unwrap_or_else(|| first.clone());
    let last = summary
        .points
        .last()
        .map(|point| compact_label(&point.at))
        .unwrap_or_else(|| first.clone());
    vec![Line::from(first), Line::from(middle), Line::from(last)]
}

fn chart_y_labels(bounds: [f64; 2]) -> Vec<Line<'static>> {
    let middle = (bounds[0] + bounds[1]) / 2.0;
    vec![
        Line::from(format!("{:.0}", bounds[0])),
        Line::from(format!("{middle:.0}")),
        Line::from(format!("{:.0}", bounds[1])),
    ]
}

fn compact_label(value: &str) -> String {
    if value.len() >= 10 && value.as_bytes().get(4) == Some(&b'-') {
        return value[5..10].to_string();
    }
    value.chars().take(10).collect()
}

fn section_block(title: &'static str, color: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            format!(" {} ", title),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::TOP)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .border_style(Style::default().fg(border_color()))
}

fn venue_status_color(status: VenueStatus) -> Color {
    match status {
        VenueStatus::Connected | VenueStatus::Ready => accent_green(),
        VenueStatus::Planned => accent_gold(),
        VenueStatus::Error => accent_red(),
    }
}

fn pnl_color(value: f64) -> Color {
    if value > 0.0 {
        accent_green()
    } else if value < 0.0 {
        accent_red()
    } else {
        muted_text()
    }
}

fn panel_background() -> Color {
    crate::theme::panel_background()
}

fn border_color() -> Color {
    crate::theme::border_color()
}

fn text_color() -> Color {
    crate::theme::text_color()
}

fn muted_text() -> Color {
    crate::theme::muted_text()
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

#[cfg(test)]
mod tests {
    use super::{build_running_pnl_summary, classify_funding, FundingKind};
    use crate::domain::{ExchangePanelSnapshot, OpenPositionRow, TrackedBetRow};

    #[test]
    fn classifies_promo_bets_from_notes_and_type() {
        let mut bet = TrackedBetRow::default();
        bet.bet_type = String::from("single");
        bet.notes = String::from("Free Bet SNR");

        assert_eq!(classify_funding(&bet), FundingKind::Promo);
    }

    #[test]
    fn classifies_explicit_funding_kind_before_note_heuristics() {
        let mut bet = TrackedBetRow::default();
        bet.funding_kind = String::from("cash");
        bet.notes = String::from("Free Bet SNR");

        assert_eq!(classify_funding(&bet), FundingKind::Standard);
    }

    #[test]
    fn running_pnl_summary_prefers_tracked_bet_history() {
        let mut snapshot = ExchangePanelSnapshot::default();
        let mut open_row = sample_history_row();
        open_row.pnl_amount = 1.5;
        snapshot.open_positions = vec![open_row];
        snapshot.tracked_bets = vec![
            TrackedBetRow {
                bet_id: String::from("bet-1"),
                settled_at: String::from("2026-03-10T12:00:00Z"),
                realised_pnl_gbp: Some(-1.25),
                notes: String::from("qualifying"),
                ..TrackedBetRow::default()
            },
            TrackedBetRow {
                bet_id: String::from("bet-2"),
                settled_at: String::from("2026-03-11T12:00:00Z"),
                realised_pnl_gbp: Some(6.40),
                notes: String::from("free bet snr"),
                ..TrackedBetRow::default()
            },
        ];

        let summary = build_running_pnl_summary(&snapshot, 1.5);

        assert_eq!(summary.source, "tracked");
        assert_eq!(summary.realised_total, 5.15);
        assert_eq!(summary.marked_total, 1.5);
        assert_eq!(summary.standard_count, 1);
        assert_eq!(summary.promo_count, 1);
        assert_eq!(summary.points.len(), 2);
        assert_eq!(summary.points[1].total, 5.15);
        assert_eq!(summary.points[1].promo, 6.40);
    }

    #[test]
    fn running_pnl_summary_falls_back_to_historical_positions() {
        let mut snapshot = ExchangePanelSnapshot::default();
        snapshot.historical_positions = vec![
            OpenPositionRow {
                event_status: String::from("2026-03-10T12:00:00|Football"),
                pnl_amount: -1.0,
                overall_pnl_known: true,
                ..sample_history_row()
            },
            OpenPositionRow {
                event_status: String::from("2026-03-11T12:00:00|Football"),
                pnl_amount: 3.5,
                overall_pnl_known: true,
                ..sample_history_row()
            },
        ];

        let summary = build_running_pnl_summary(&snapshot, 0.0);

        assert_eq!(summary.source, "history");
        assert_eq!(summary.realised_total, 2.5);
        assert_eq!(summary.unknown_count, 2);
        assert_eq!(summary.points.len(), 2);
        assert_eq!(summary.points[1].total, 2.5);
    }

    #[test]
    fn running_pnl_summary_prefers_ledger_history_when_available() {
        let mut snapshot = ExchangePanelSnapshot::default();
        snapshot.ledger_pnl_summary.realised_total = 12.5;
        snapshot.ledger_pnl_summary.settled_count = 3;
        snapshot.ledger_pnl_summary.standard_count = 2;
        snapshot.ledger_pnl_summary.promo_count = 1;
        snapshot.ledger_pnl_summary.points = vec![
            crate::domain::LedgerPnlPoint {
                occurred_at: String::from("2026-03-10T12:00:00Z"),
                total: 10.0,
                promo_total: 0.0,
                ..crate::domain::LedgerPnlPoint::default()
            },
            crate::domain::LedgerPnlPoint {
                occurred_at: String::from("2026-03-11T12:00:00Z"),
                total: 8.0,
                promo_total: 0.0,
                ..crate::domain::LedgerPnlPoint::default()
            },
            crate::domain::LedgerPnlPoint {
                occurred_at: String::from("2026-03-12T12:00:00Z"),
                total: 12.5,
                promo_total: 4.5,
                ..crate::domain::LedgerPnlPoint::default()
            },
        ];

        let summary = build_running_pnl_summary(&snapshot, 1.5);

        assert_eq!(summary.source, "ledger");
        assert_eq!(summary.realised_total, 12.5);
        assert_eq!(summary.marked_total, 1.5);
        assert_eq!(summary.standard_count, 2);
        assert_eq!(summary.promo_count, 1);
        assert_eq!(summary.points.len(), 3);
        assert_eq!(summary.points[2].total, 12.5);
        assert_eq!(summary.points[2].promo, 4.5);
    }

    fn sample_history_row() -> OpenPositionRow {
        OpenPositionRow {
            event: String::new(),
            event_status: String::new(),
            event_url: String::new(),
            contract: String::new(),
            market: String::new(),
            status: String::new(),
            market_status: String::new(),
            is_in_play: false,
            price: 0.0,
            stake: 0.0,
            liability: 0.0,
            current_value: 0.0,
            pnl_amount: 0.0,
            overall_pnl_known: true,
            current_back_odds: None,
            current_implied_probability: None,
            current_implied_percentage: None,
            current_buy_odds: None,
            current_buy_implied_probability: None,
            current_sell_odds: None,
            current_sell_implied_probability: None,
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::new(),
            can_trade_out: false,
        }
    }
}
