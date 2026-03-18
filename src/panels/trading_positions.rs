use std::collections::BTreeSet;

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
    status_message: &str,
    help_text: &str,
) {
    let selected_position = selected_position(snapshot, table_state);
    let layout = Layout::vertical([Constraint::Length(11), Constraint::Min(18)]).split(area);
    let body = Layout::horizontal([Constraint::Percentage(64), Constraint::Percentage(36)])
        .split(layout[1]);
    let left = Layout::vertical([
        Constraint::Length(active_positions_section_height(snapshot, body[0].height)),
        Constraint::Min(10),
    ])
    .split(body[0]);
    let right = Layout::vertical([
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Min(6),
    ])
    .split(body[1]);

    render_summary(frame, layout[0], snapshot, selected_position);
    render_stateful_table(
        frame,
        left[0],
        &format!("Active Positions ({})", snapshot.open_positions.len()),
        vec![
            Constraint::Percentage(22),
            Constraint::Percentage(27),
            Constraint::Length(5),
            Constraint::Length(12),
            Constraint::Length(3),
            Constraint::Length(8),
            Constraint::Length(12),
        ],
        position_rows(snapshot),
        empty_row(
            "No active positions are loaded. Start the recorder or refresh the provider.",
            7,
        ),
        table_state,
    );
    render_table(
        frame,
        left[1],
        &format!(
            "Historical Positions ({})",
            snapshot.historical_positions.len()
        ),
        vec![
            Constraint::Percentage(22),
            Constraint::Percentage(27),
            Constraint::Length(5),
            Constraint::Length(12),
            Constraint::Length(3),
            Constraint::Length(8),
            Constraint::Length(12),
        ],
        historical_position_rows(snapshot),
        empty_row(
            "No historical positions are loaded. Import ledger history to populate this section.",
            7,
        ),
    );
    render_table(
        frame,
        right[0],
        &format!(
            "Exit Recommendations ({})",
            snapshot.exit_recommendations.len()
        ),
        vec![
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Min(16),
            Constraint::Length(9),
            Constraint::Length(10),
        ],
        exit_rows(snapshot),
        empty_row("No exit recommendations are loaded.", 5),
    );
    render_table(
        frame,
        right[1],
        &format!(
            "Watch Plan ({})",
            snapshot
                .watch
                .as_ref()
                .map(|watch| watch.watch_count)
                .unwrap_or(0)
        ),
        vec![
            Constraint::Percentage(26),
            Constraint::Percentage(24),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(8),
        ],
        watch_rows(snapshot),
        empty_row("No grouped watch plan is loaded.", 6),
    );
    render_table(
        frame,
        right[2],
        &format!("Tracked Bets ({})", snapshot.tracked_bets.len()),
        vec![
            Constraint::Length(10),
            Constraint::Percentage(28),
            Constraint::Percentage(22),
            Constraint::Length(8),
            Constraint::Min(12),
        ],
        tracked_rows(snapshot),
        empty_row("No tracked bets are loaded.", 5),
    );
    render_table(
        frame,
        right[3],
        &format!("Other Open Bets ({})", snapshot.other_open_bets.len()),
        vec![
            Constraint::Percentage(32),
            Constraint::Percentage(26),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Min(10),
        ],
        other_open_bet_rows(snapshot),
        empty_row("No other open bets are loaded.", 5),
    );
    render_operator_log(frame, right[4], status_message, help_text);
}

fn render_summary(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    selected_position: Option<&crate::domain::OpenPositionRow>,
) {
    let summary =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    let runtime = snapshot.runtime.as_ref();
    let overview_rows = vec![
        (
            "Refresh",
            runtime
                .map(|summary| {
                    summary
                        .updated_at
                        .replace('T', " ")
                        .trim_end_matches('Z')
                        .to_string()
                })
                .unwrap_or_else(|| String::from("unknown")),
            accent_green(),
        ),
        (
            "Worker",
            format!("{:?}", snapshot.worker.status),
            accent_cyan(),
        ),
        (
            "Source",
            runtime
                .map(|summary| summary.source.clone())
                .unwrap_or_else(|| String::from("snapshot")),
            accent_gold(),
        ),
        (
            "Counts",
            format!(
                "pos {} | hist {} | open {} | tracked {}",
                snapshot.open_positions.len(),
                snapshot.historical_positions.len(),
                snapshot.other_open_bets.len(),
                snapshot.tracked_bets.len(),
            ),
            accent_blue(),
        ),
        (
            "State",
            format!(
                "live {} | susp {} | watch {} | rec {}",
                in_play_count(snapshot),
                suspended_count(snapshot),
                snapshot
                    .watch
                    .as_ref()
                    .map(|watch| watch.watch_count)
                    .unwrap_or(0),
                snapshot.exit_recommendations.len(),
            ),
            accent_pink(),
        ),
        (
            "Status",
            if runtime.map(|summary| summary.stale).unwrap_or(false) {
                format!("STALE | {}", snapshot.status_line)
            } else {
                snapshot.status_line.clone()
            },
            if runtime.map(|summary| summary.stale).unwrap_or(false) {
                accent_red()
            } else {
                accent_green()
            },
        ),
    ];
    render_key_value_table(
        frame,
        summary[0],
        "Snapshot",
        overview_rows,
        Constraint::Length(8),
    );

    let selected_rows = if let Some(row) = selected_position {
        vec![
            ("Event", event_label(row), accent_blue()),
            ("Position", position_label(row), accent_gold()),
            ("Score", score_label(row), accent_green()),
            ("Phase", phase_label(row), accent_cyan()),
            (
                "Trade",
                format!("{} ({})", trade_label(row), trade_code(row)),
                trade_color(row),
            ),
            (
                "Order",
                if row.status.is_empty() {
                    String::from("-")
                } else {
                    row.status.clone()
                },
                accent_green(),
            ),
            (
                "Exposure",
                format!(
                    "stake {:.2} | liab {:.2} | value {:.2}",
                    row.stake, row.liability, row.current_value,
                ),
                accent_pink(),
            ),
            (
                "Market",
                format!(
                    "buy {} | sell {} | {}",
                    format_optional_back_odds(primary_market_buy_odds(row)),
                    format_optional_back_odds(row.current_sell_odds),
                    format_optional_probability(primary_market_implied_probability(row)),
                ),
                accent_blue(),
            ),
            (
                "Marked",
                format!(
                    "value {:.2} | pnl {:+.2}",
                    row.current_value, row.pnl_amount,
                ),
                pnl_color(row.pnl_amount),
            ),
        ]
    } else {
        vec![
            ("Event", String::from("-"), muted_text()),
            (
                "Position",
                String::from("No active position selected"),
                muted_text(),
            ),
            ("Score", String::from("-"), muted_text()),
            ("Phase", String::from("-"), muted_text()),
            ("Trade", String::from("-"), muted_text()),
            ("Order", String::from("-"), muted_text()),
            ("Exposure", String::from("-"), muted_text()),
            ("Market", String::from("-"), muted_text()),
            ("Marked", String::from("-"), muted_text()),
        ]
    };
    render_key_value_table(
        frame,
        summary[1],
        "Selected Position",
        selected_rows,
        Constraint::Length(9),
    );
}

fn render_key_value_table(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    rows: Vec<(&'static str, String, Color)>,
    key_width: Constraint,
) {
    let table_rows = rows.into_iter().map(|(label, value, color)| {
        Row::new(vec![
            Cell::from(Span::styled(
                label.to_string(),
                Style::default()
                    .fg(muted_text())
                    .add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(value, Style::default().fg(color))),
        ])
    });
    let table = Table::new(table_rows, [key_width, Constraint::Min(10)])
        .block(section_block(title, accent_blue()))
        .column_spacing(1);
    frame.render_widget(table, area);
}

#[cfg(test)]
fn summary_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
    vec![
        Line::raw(format!(
            "Positions: {} | Other bets: {} | Tracked bets: {} | Recommendations: {}",
            snapshot.open_positions.len(),
            snapshot.other_open_bets.len(),
            snapshot.tracked_bets.len(),
            snapshot.exit_recommendations.len(),
        )),
        Line::raw(format!(
            "Selected venue: {}",
            snapshot
                .selected_venue
                .map(|venue| venue.as_str().to_string())
                .unwrap_or_else(|| String::from("none"))
        )),
        Line::raw(format!(
            "Worker: {:?} | {}",
            snapshot.worker.status, snapshot.worker.detail
        )),
        Line::raw(format!(
            "Watch groups: {} | Decision queue: {}",
            snapshot
                .watch
                .as_ref()
                .map(|watch| watch.watch_count)
                .unwrap_or(0),
            snapshot.decisions.len(),
        )),
        Line::raw(format!(
            "Tracked sources: {} | Market EV proxy: {}",
            tracked_source_count(snapshot),
            format_optional_value(total_market_ev(snapshot))
        )),
    ]
}

#[cfg(test)]
fn open_position_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
    if snapshot.open_positions.is_empty() && snapshot.historical_positions.is_empty() {
        return vec![Line::raw("No open positions are loaded.")];
    }

    let mut rows = Vec::new();
    if !snapshot.open_positions.is_empty() {
        rows.push(Line::raw(format!(
            "Active Positions ({})",
            snapshot.open_positions.len()
        )));
    }
    for row in snapshot.open_positions.iter().take(6) {
        rows.push(Line::raw(format!(
            "{} | {}",
            event_label(row),
            position_label(row)
        )));
        rows.push(Line::raw(format!(
            "score {} | phase {} | trade {}",
            score_label(row),
            phase_label(row),
            trade_label(row),
        )));
        rows.push(Line::raw(format!(
            "status {} | pnl {:+.2} | buy {} | {}",
            if row.status.is_empty() {
                String::from("-")
            } else {
                row.status.clone()
            },
            row.pnl_amount,
            format_optional_back_odds(primary_market_buy_odds(row)),
            format_optional_probability(primary_market_implied_probability(row)),
        )));
    }
    if !snapshot.historical_positions.is_empty() {
        rows.push(Line::raw(format!(
            "Historical Positions ({})",
            snapshot.historical_positions.len()
        )));
        for row in snapshot.historical_positions.iter().take(6) {
            rows.push(Line::raw(format!(
                "{} | {}",
                event_label(row),
                position_label(row)
            )));
            rows.push(Line::raw(format!(
                "score {} | phase {} | trade {}",
                score_label(row),
                phase_label(row),
                trade_label(row),
            )));
            rows.push(Line::raw(format!(
                "status {} | pnl {:+.2} | buy {} | {}",
                if row.status.is_empty() {
                    String::from("-")
                } else {
                    row.status.clone()
                },
                row.pnl_amount,
                format_optional_back_odds(primary_market_buy_odds(row)),
                format_optional_probability(primary_market_implied_probability(row)),
            )));
        }
    }
    rows
}

fn position_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    snapshot
        .open_positions
        .iter()
        .take(8)
        .map(|row| {
            Row::new(vec![
                Cell::from(event_table_label(row)),
                Cell::from(position_table_label(row)),
                Cell::from(score_label(row)),
                Cell::from(phase_label(row)),
                trade_cell(row),
                pnl_cell(row.pnl_amount),
                Cell::from(market_price_label(row)),
            ])
        })
        .collect()
}

fn historical_position_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    snapshot
        .historical_positions
        .iter()
        .take(8)
        .map(|row| {
            Row::new(vec![
                Cell::from(event_table_label(row)),
                Cell::from(position_table_label(row)),
                Cell::from(score_label(row)),
                Cell::from(phase_label(row)),
                trade_cell(row),
                pnl_cell(row.pnl_amount),
                Cell::from(market_price_label(row)),
            ])
        })
        .collect()
}

fn other_open_bet_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    snapshot
        .other_open_bets
        .iter()
        .take(6)
        .map(|row| {
            Row::new(vec![
                Cell::from(row.label.clone()),
                Cell::from(row.market.clone()),
                Cell::from(row.side.clone()),
                Cell::from(format!("{:.2}", row.stake)),
                Cell::from(row.status.clone()),
            ])
        })
        .collect()
}

#[cfg(test)]
fn watch_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
    let Some(watch) = &snapshot.watch else {
        return vec![Line::raw("No grouped watch plan is loaded.")];
    };

    let mut rows = vec![
        Line::raw(format!(
            "Target profit {:.2} | Stop loss {:.2}",
            watch.target_profit, watch.stop_loss
        )),
        Line::raw(format!("Commission rate {:.2}", watch.commission_rate)),
        Line::raw(String::new()),
    ];

    for row in watch.watches.iter().take(6) {
        rows.push(Line::raw(format!("{} | {}", row.contract, row.market)));
        rows.push(Line::raw(format!(
            "live {} | profit {:.2} | stop {:.2}",
            row.current_back_odds
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| String::from("-")),
            row.profit_take_back_odds,
            row.stop_loss_back_odds,
        )));
        rows.push(Line::raw(format!(
            "prob entry {} | live {} | profit {} | stop {}",
            format_probability(row.entry_implied_probability),
            row.current_back_odds
                .map(implied_probability)
                .map(format_probability)
                .unwrap_or_else(|| String::from("-")),
            format_probability(row.profit_take_implied_probability),
            format_probability(row.stop_loss_implied_probability),
        )));
        rows.push(Line::raw(format!(
            "market EV {} | gaps profit {} stop {}",
            format_optional_value(market_implied_ev(
                row.total_stake,
                row.total_liability,
                row.current_back_odds,
                watch.commission_rate,
            )),
            row.current_back_odds
                .map(|live| format!("{:+.2}", row.profit_take_back_odds - live))
                .unwrap_or_else(|| String::from("-")),
            row.current_back_odds
                .map(|live| format!("{:+.2}", row.stop_loss_back_odds - live))
                .unwrap_or_else(|| String::from("-")),
        )));
    }
    rows
}

fn watch_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    let Some(watch) = &snapshot.watch else {
        return Vec::new();
    };

    watch
        .watches
        .iter()
        .take(6)
        .map(|row| {
            Row::new(vec![
                Cell::from(row.contract.clone()),
                Cell::from(row.market.clone()),
                Cell::from(
                    row.current_back_odds
                        .map(|value| format!("{value:.2}"))
                        .unwrap_or_else(|| String::from("-")),
                ),
                Cell::from(format!("{:.2}", row.profit_take_back_odds)),
                Cell::from(format!("{:.2}", row.stop_loss_back_odds)),
                pnl_cell(row.current_pnl_amount),
            ])
        })
        .collect()
}

#[cfg(test)]
fn tracked_bet_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
    if snapshot.tracked_bets.is_empty() {
        return vec![Line::raw("No tracked bets are loaded.")];
    }

    let mut rows = Vec::new();
    for tracked_bet in snapshot.tracked_bets.iter().take(6) {
        rows.push(Line::raw(format!(
            "{} | {} | {} | {}",
            if tracked_bet.platform.is_empty() {
                "-"
            } else {
                tracked_bet.platform.as_str()
            },
            tracked_bet.selection,
            tracked_bet.market,
            tracked_bet.status
        )));
        rows.push(Line::raw(format!(
            "bet {} | group {} | type {} | sport {}",
            tracked_bet.bet_id,
            tracked_bet.group_id,
            tracked_bet.bet_type,
            if tracked_bet.sport_name.is_empty() {
                "-"
            } else {
                tracked_bet.sport_name.as_str()
            },
        )));
        rows.push(Line::raw(format!(
            "event {} | back {} | lay {} | ev {} | venues {}",
            tracked_bet.event,
            format_optional_value(tracked_bet.back_price),
            format_optional_value(tracked_bet.lay_price),
            format_optional_value(tracked_bet.expected_ev.gbp),
            tracked_bet_source_summary(tracked_bet),
        )));
    }
    rows
}

fn tracked_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    snapshot
        .tracked_bets
        .iter()
        .take(6)
        .map(|tracked_bet| {
            Row::new(vec![
                Cell::from(tracked_bet.bet_id.clone()),
                Cell::from(
                    format!("{} {}", tracked_bet.platform, tracked_bet.selection)
                        .trim()
                        .to_string(),
                ),
                Cell::from(tracked_bet.market.clone()),
                Cell::from(tracked_bet.status.clone()),
                Cell::from(
                    tracked_bet
                        .expected_ev
                        .gbp
                        .map(|value| format!("EV {value:.2}"))
                        .unwrap_or_else(|| tracked_bet_source_summary(tracked_bet)),
                ),
            ])
        })
        .collect()
}

#[cfg(test)]
fn exit_recommendation_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
    if snapshot.exit_recommendations.is_empty() {
        return vec![Line::raw("No exit recommendations are loaded.")];
    }

    let mut rows = vec![Line::raw(format!(
        "Target {:.2} | Stop {:.2} | Hard floor {} | Warn default {}",
        snapshot.exit_policy.target_profit,
        snapshot.exit_policy.stop_loss,
        snapshot
            .exit_policy
            .hard_margin_call_profit_floor
            .map(|value| format!("{value:.2}"))
            .unwrap_or_else(|| String::from("-")),
        snapshot.exit_policy.warn_only_default,
    ))];
    rows.push(Line::raw(
        "Press c in Trading > Positions to request the first actionable cash out.",
    ));

    for recommendation in snapshot.exit_recommendations.iter().take(6) {
        rows.push(Line::raw(format!(
            "{} | {} | worst {:.2}",
            recommendation.bet_id, recommendation.action, recommendation.worst_case_pnl
        )));
        rows.push(Line::raw(format!(
            "reason {} | venue {}",
            recommendation.reason,
            recommendation
                .cash_out_venue
                .clone()
                .unwrap_or_else(|| String::from("-")),
        )));
    }
    rows
}

fn exit_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    snapshot
        .exit_recommendations
        .iter()
        .take(6)
        .map(|recommendation| {
            Row::new(vec![
                Cell::from(recommendation.bet_id.clone()),
                Cell::from(recommendation.action.clone()),
                Cell::from(recommendation.reason.clone()),
                pnl_cell(recommendation.worst_case_pnl),
                Cell::from(
                    recommendation
                        .cash_out_venue
                        .clone()
                        .unwrap_or_else(|| String::from("-")),
                ),
            ])
        })
        .collect()
}

#[cfg(test)]
fn tracked_source_count(snapshot: &ExchangePanelSnapshot) -> usize {
    snapshot
        .tracked_bets
        .iter()
        .flat_map(|tracked_bet| tracked_bet.legs.iter().map(|leg| leg.venue.as_str()))
        .collect::<BTreeSet<_>>()
        .len()
}

fn tracked_bet_source_summary(tracked_bet: &crate::domain::TrackedBetRow) -> String {
    let venues = tracked_bet
        .legs
        .iter()
        .map(|leg| leg.venue.as_str())
        .collect::<BTreeSet<_>>();
    if venues.is_empty() {
        String::from("-")
    } else {
        venues.into_iter().collect::<Vec<_>>().join(", ")
    }
}

#[cfg(test)]
#[cfg(test)]
fn implied_probability(odds: f64) -> f64 {
    1.0 / odds
}

#[cfg(test)]
fn total_market_ev(snapshot: &ExchangePanelSnapshot) -> Option<f64> {
    let watch = snapshot.watch.as_ref()?;
    let values = watch
        .watches
        .iter()
        .filter_map(|row| {
            market_implied_ev(
                row.total_stake,
                row.total_liability,
                row.current_back_odds,
                watch.commission_rate,
            )
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values.into_iter().sum())
    }
}

#[cfg(test)]
fn market_implied_ev(
    stake: f64,
    liability: f64,
    current_back_odds: Option<f64>,
    commission_rate: f64,
) -> Option<f64> {
    let current_back_odds = current_back_odds?;
    let win_probability = implied_probability(current_back_odds);
    let lose_probability = 1.0 - win_probability;
    let effective_commission = if commission_rate > 1.0 {
        commission_rate / 100.0
    } else {
        commission_rate
    };
    let selection_loses_pnl = stake * (1.0 - effective_commission);
    let selection_wins_pnl = -liability;
    Some((lose_probability * selection_loses_pnl) + (win_probability * selection_wins_pnl))
}

fn format_probability(probability: f64) -> String {
    format!("{:.2}%", probability * 100.0)
}

#[cfg(test)]
fn format_optional_value(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| String::from("-"))
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

fn render_operator_log(frame: &mut Frame<'_>, area: Rect, status_message: &str, help_text: &str) {
    let lines = std::iter::once(Line::raw(status_message.to_string()))
        .chain(help_text.lines().map(|line| Line::raw(line.to_string())))
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(lines)
        .block(section_block("Operator Log", accent_blue()))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn active_positions_section_height(snapshot: &ExchangePanelSnapshot, available_height: u16) -> u16 {
    let visible_rows = snapshot.open_positions.len().clamp(1, 8) as u16;
    let desired_height = visible_rows + 4;
    let max_allowed = available_height.saturating_sub(10);
    desired_height.clamp(6, max_allowed.max(6))
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
        heading if heading.starts_with("Active Positions") => {
            vec!["Event", "Position", "Score", "Phase", "T", "PnL", "Market"]
        }
        heading if heading.starts_with("Historical Positions") => {
            vec!["Event", "Position", "Score", "Phase", "T", "PnL", "Market"]
        }
        heading if heading.starts_with("Exit Recommendations") => {
            vec!["Bet", "Action", "Reason", "Worst", "Venue"]
        }
        heading if heading.starts_with("Watch Plan") => {
            vec!["Contract", "Market", "Live", "Profit", "Stop", "PnL"]
        }
        heading if heading.starts_with("Tracked Bets") => {
            vec!["Bet", "Selection", "Market", "Status", "Venues"]
        }
        heading if heading.starts_with("Other Open Bets") => {
            vec!["Label", "Market", "Side", "Stake", "Status"]
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
    let color = pnl_color(value);
    Cell::from(format!("{value:+.2}")).style(Style::default().fg(color))
}

fn trade_cell(row: &crate::domain::OpenPositionRow) -> Cell<'static> {
    Cell::from(trade_code(row)).style(Style::default().fg(trade_color(row)))
}

fn event_label(row: &crate::domain::OpenPositionRow) -> String {
    if row.event.is_empty() {
        String::from("-")
    } else {
        row.event.clone()
    }
}

fn score_label(row: &crate::domain::OpenPositionRow) -> String {
    if effective_market_status(row) == "settled" {
        return String::from("-");
    }
    if row.current_score.is_empty() {
        if row.live_clock.is_empty() {
            return String::from("-");
        }
        return row.live_clock.clone();
    }
    row.current_score.clone()
}

fn market_price_label(row: &crate::domain::OpenPositionRow) -> String {
    let odds = format_optional_back_odds(primary_market_buy_odds(row));
    let probability = format_optional_probability(primary_market_implied_probability(row));
    if odds == "-" && probability == "-" {
        return String::from("-");
    }
    format!("{odds} {probability}")
}

fn primary_market_buy_odds(row: &crate::domain::OpenPositionRow) -> Option<f64> {
    row.current_buy_odds.or(row.current_back_odds)
}

fn primary_market_implied_probability(row: &crate::domain::OpenPositionRow) -> Option<f64> {
    row.current_buy_implied_probability
        .or(row.current_implied_probability)
        .or_else(|| primary_market_buy_odds(row).map(|odds| 1.0 / odds))
}

fn format_optional_back_odds(value: Option<f64>) -> String {
    value
        .map(|odds| format!("{odds:.2}"))
        .unwrap_or_else(|| String::from("-"))
}

fn format_optional_probability(value: Option<f64>) -> String {
    value
        .map(format_probability)
        .unwrap_or_else(|| String::from("-"))
}

fn position_label(row: &crate::domain::OpenPositionRow) -> String {
    format!("{} · {}", row.contract, row.market)
}

fn event_table_label(row: &crate::domain::OpenPositionRow) -> String {
    truncate_text(&event_label(row), 28)
}

fn position_table_label(row: &crate::domain::OpenPositionRow) -> String {
    truncate_text(&position_label(row), 34)
}

fn phase_label(row: &crate::domain::OpenPositionRow) -> String {
    if row.event_status.is_empty() {
        if !row.live_clock.is_empty() {
            return row.live_clock.clone();
        }
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

fn trade_label(row: &crate::domain::OpenPositionRow) -> String {
    match effective_market_status(row) {
        "tradable" => String::from("Tradable"),
        "suspended" => String::from("Suspended"),
        "pre_event" => String::from("Pre-match"),
        "settled" => String::from("Settled"),
        _ if row.status == "Order filled" => String::from("Active"),
        _ => String::from("unknown"),
    }
}

fn trade_code(row: &crate::domain::OpenPositionRow) -> &'static str {
    match effective_market_status(row) {
        "tradable" => "Y",
        "suspended" => "N",
        "pre_event" => "P",
        "settled" => "X",
        _ => "-",
    }
}

fn trade_color(row: &crate::domain::OpenPositionRow) -> Color {
    match effective_market_status(row) {
        "tradable" => accent_green(),
        "suspended" => accent_red(),
        "pre_event" => accent_gold(),
        "settled" => accent_pink(),
        _ => muted_text(),
    }
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

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() && max_chars > 3 {
        format!(
            "{}...",
            truncated.chars().take(max_chars - 3).collect::<String>()
        )
    } else if truncated.is_empty() {
        String::from("-")
    } else {
        truncated
    }
}

fn in_play_count(snapshot: &ExchangePanelSnapshot) -> usize {
    snapshot
        .open_positions
        .iter()
        .filter(|row| row.is_in_play || effective_market_status(row) == "suspended")
        .count()
}

fn suspended_count(snapshot: &ExchangePanelSnapshot) -> usize {
    snapshot
        .open_positions
        .iter()
        .filter(|row| effective_market_status(row) == "suspended")
        .count()
}

fn selected_position<'a>(
    snapshot: &'a ExchangePanelSnapshot,
    table_state: &TableState,
) -> Option<&'a crate::domain::OpenPositionRow> {
    table_state
        .selected()
        .and_then(|index| snapshot.open_positions.get(index))
        .or_else(|| snapshot.open_positions.first())
        .or_else(|| snapshot.historical_positions.first())
}

fn pnl_color(value: f64) -> Color {
    if value > 0.0 {
        accent_green()
    } else if value < 0.0 {
        accent_red()
    } else {
        accent_gold()
    }
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

#[cfg(test)]
mod tests {
    use crate::domain::{
        ExchangePanelSnapshot, ExitPolicySummary, ExitRecommendation, OpenPositionRow,
        TrackedBetRow, TrackedLeg, ValueMetric, VenueId, VenueStatus, VenueSummary, WatchSnapshot,
        WorkerStatus, WorkerSummary,
    };

    use super::{
        exit_recommendation_lines, open_position_lines, summary_lines, tracked_bet_lines,
        watch_lines,
    };

    #[test]
    fn summary_mentions_worker_error_detail() {
        let snapshot = ExchangePanelSnapshot {
            worker: WorkerSummary {
                name: String::from("bet-recorder"),
                status: WorkerStatus::Error,
                detail: String::from("Recorder start failed: watcher timed out"),
            },
            venues: vec![VenueSummary {
                id: VenueId::Smarkets,
                label: String::from("Smarkets"),
                status: VenueStatus::Error,
                detail: String::from("Recorder start failed: watcher timed out"),
                event_count: 0,
                market_count: 0,
            }],
            selected_venue: Some(VenueId::Smarkets),
            events: Vec::new(),
            markets: Vec::new(),
            preflight: None,
            status_line: String::from("Recorder start failed: watcher timed out"),
            runtime: None,
            account_stats: None,
            open_positions: Vec::new(),
            historical_positions: Vec::new(),
            other_open_bets: Vec::new(),
            decisions: Vec::new(),
            watch: Some(WatchSnapshot {
                watch_count: 0,
                position_count: 0,
                commission_rate: 0.0,
                target_profit: 1.0,
                stop_loss: 1.0,
                watches: Vec::new(),
            }),
            tracked_bets: Vec::new(),
            exit_policy: Default::default(),
            exit_recommendations: Vec::new(),
        };

        let rendered = summary_lines(&snapshot)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("watcher timed out"));
    }

    #[test]
    fn summary_mentions_tracked_bets_and_recommendations() {
        let snapshot = sample_snapshot();

        let rendered = summary_lines(&snapshot)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Tracked bets: 1"));
        assert!(rendered.contains("Recommendations: 1"));
    }

    #[test]
    fn tracked_bet_lines_show_canonical_bet_rows() {
        let snapshot = sample_snapshot();

        let rendered = tracked_bet_lines(&snapshot)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("bet365 | Draw | Full-time result | open"));
        assert!(rendered.contains("bet bet-001 | group group-arsenal-everton | type single"));
        assert!(rendered.contains("back 2.12 | lay 3.35 | ev 0.42"));
    }

    #[test]
    fn open_position_lines_show_score_and_market_probabilities() {
        let mut snapshot = sample_snapshot();
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("West Ham vs Man City"),
            event_status: String::from("27'|Premier League"),
            event_url: String::from(
                "https://smarkets.com/football/england-premier-league/2026/03/14/20-00/west-ham-vs-manchester-city/44919693/",
            ),
            contract: String::from("Man City"),
            market: String::from("Full-time result"),
            status: String::from("Order filled"),
            market_status: String::from("tradable"),
            is_in_play: true,
            price: 2.40,
            stake: 10.0,
            liability: 14.0,
            current_value: 8.4,
            pnl_amount: -1.6,
            current_back_odds: Some(1.91),
            current_implied_probability: Some(1.0 / 1.91),
            current_implied_percentage: Some(100.0 / 1.91),
            current_buy_odds: Some(1.91),
            current_buy_implied_probability: Some(1.0 / 1.91),
            current_sell_odds: Some(1.94),
            current_sell_implied_probability: Some(1.0 / 1.94),
            current_score: String::from("0-0"),
            current_score_home: Some(0),
            current_score_away: Some(0),
            live_clock: String::from("27'"),
            can_trade_out: true,
        }];

        let rendered = open_position_lines(&snapshot)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("score 0-0 | phase 27' | trade Tradable"));
        assert!(rendered.contains("buy 1.91"));
        assert!(rendered.contains("52.36%"));
    }

    #[test]
    fn open_position_lines_include_historical_positions_section() {
        let mut snapshot = sample_snapshot();
        snapshot.historical_positions = vec![OpenPositionRow {
            event: String::from("Aston Villa v Chelsea"),
            event_status: String::from("2026-03-03T14:08:00|Football"),
            event_url: String::new(),
            contract: String::from("Reece James (Chelsea)"),
            market: String::from("Player To Receive A Card"),
            status: String::from("settled"),
            market_status: String::from("settled"),
            is_in_play: false,
            price: 4.5,
            stake: 2.0,
            liability: 2.0,
            current_value: 0.0,
            pnl_amount: -2.0,
            current_back_odds: Some(4.5),
            current_implied_probability: Some(1.0 / 4.5),
            current_implied_percentage: Some(100.0 / 4.5),
            current_buy_odds: Some(4.5),
            current_buy_implied_probability: Some(1.0 / 4.5),
            current_sell_odds: None,
            current_sell_implied_probability: None,
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::from("2026-03-03T14:08:00"),
            can_trade_out: false,
        }];

        let rendered = open_position_lines(&snapshot)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Historical Positions (1)"));
        assert!(rendered
            .contains("Aston Villa v Chelsea | Reece James (Chelsea) · Player To Receive A Card"));
        assert!(rendered.contains("trade Settled"));
    }

    #[test]
    fn active_positions_section_height_shrinks_when_few_rows_are_visible() {
        let mut snapshot = sample_snapshot();
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("West Ham vs Man City"),
            event_status: String::from("27'|Premier League"),
            event_url: String::new(),
            contract: String::from("Man City"),
            market: String::from("Full-time result"),
            status: String::from("Order filled"),
            market_status: String::from("tradable"),
            is_in_play: true,
            price: 2.40,
            stake: 10.0,
            liability: 14.0,
            current_value: 8.4,
            pnl_amount: -1.6,
            current_back_odds: Some(1.91),
            current_implied_probability: Some(1.0 / 1.91),
            current_implied_percentage: Some(100.0 / 1.91),
            current_buy_odds: Some(1.91),
            current_buy_implied_probability: Some(1.0 / 1.91),
            current_sell_odds: Some(1.94),
            current_sell_implied_probability: Some(1.0 / 1.94),
            current_score: String::from("0-0"),
            current_score_home: Some(0),
            current_score_away: Some(0),
            live_clock: String::from("27'"),
            can_trade_out: true,
        }];

        let compact_height = super::active_positions_section_height(&snapshot, 40);
        snapshot.open_positions = vec![snapshot.open_positions[0].clone(); 8];
        let expanded_height = super::active_positions_section_height(&snapshot, 40);

        assert!(compact_height < expanded_height);
        assert_eq!(compact_height, 6);
        assert_eq!(expanded_height, 12);
    }

    #[test]
    fn exit_recommendation_lines_show_policy_and_recommendation() {
        let snapshot = sample_snapshot();

        let rendered = exit_recommendation_lines(&snapshot)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Target 5.00 | Stop 5.00 | Hard floor - | Warn default true"));
        assert!(rendered.contains("bet-001 | hold | worst 1.27"));
        assert!(rendered.contains("reason within_thresholds | venue smarkets"));
    }

    #[test]
    fn watch_lines_show_probabilities_and_market_ev() {
        let mut snapshot = sample_snapshot();
        snapshot.watch = Some(WatchSnapshot {
            watch_count: 1,
            position_count: 1,
            commission_rate: 0.0,
            target_profit: 5.0,
            stop_loss: 5.0,
            watches: vec![crate::domain::WatchRow {
                contract: String::from("Draw"),
                market: String::from("Full-time result"),
                position_count: 1,
                can_trade_out: true,
                total_stake: 9.91,
                total_liability: 23.29,
                current_pnl_amount: -0.31,
                current_back_odds: Some(5.0),
                average_entry_lay_odds: 3.35,
                entry_implied_probability: 1.0 / 3.35,
                profit_take_back_odds: 4.2,
                profit_take_implied_probability: 1.0 / 4.2,
                stop_loss_back_odds: 2.8,
                stop_loss_implied_probability: 1.0 / 2.8,
            }],
        });

        let rendered = watch_lines(&snapshot)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("prob entry 29.85% | live 20.00% | profit 23.81% | stop 35.71%"));
        assert!(rendered.contains("market EV"));
    }

    fn sample_snapshot() -> ExchangePanelSnapshot {
        ExchangePanelSnapshot {
            worker: WorkerSummary {
                name: String::from("bet-recorder"),
                status: WorkerStatus::Ready,
                detail: String::from("Ledger snapshot loaded"),
            },
            venues: vec![VenueSummary {
                id: VenueId::Smarkets,
                label: String::from("Smarkets"),
                status: VenueStatus::Ready,
                detail: String::from("Watching positions"),
                event_count: 1,
                market_count: 1,
            }],
            selected_venue: Some(VenueId::Smarkets),
            events: Vec::new(),
            markets: Vec::new(),
            preflight: None,
            status_line: String::from("Ledger snapshot loaded"),
            runtime: None,
            account_stats: None,
            open_positions: Vec::new(),
            historical_positions: Vec::new(),
            other_open_bets: Vec::new(),
            decisions: Vec::new(),
            watch: Some(WatchSnapshot {
                watch_count: 1,
                position_count: 1,
                commission_rate: 0.0,
                target_profit: 5.0,
                stop_loss: 5.0,
                watches: Vec::new(),
            }),
            tracked_bets: vec![TrackedBetRow {
                bet_id: String::from("bet-001"),
                group_id: String::from("group-arsenal-everton"),
                event: String::from("Arsenal v Everton"),
                market: String::from("Full-time result"),
                selection: String::from("Draw"),
                status: String::from("open"),
                placed_at: String::from("2026-03-13T10:30:00Z"),
                settled_at: String::new(),
                platform: String::from("bet365"),
                platform_kind: String::from("sportsbook"),
                exchange: Some(String::from("smarkets")),
                sport_key: String::from("soccer_epl"),
                sport_name: String::from("Premier League"),
                bet_type: String::from("single"),
                market_family: String::from("match_odds"),
                selection_line: None,
                currency: String::from("GBP"),
                stake_gbp: Some(2.0),
                potential_returns_gbp: Some(4.24),
                payout_gbp: None,
                realised_pnl_gbp: None,
                back_price: Some(2.12),
                lay_price: Some(3.35),
                spread: None,
                expected_ev: ValueMetric {
                    gbp: Some(0.42),
                    pct: Some(0.21),
                    method: String::from("fair_price"),
                    source: String::from("local_formula"),
                    status: String::from("calculated"),
                },
                realised_ev: Default::default(),
                activities: Vec::new(),
                parse_confidence: String::from("high"),
                notes: String::new(),
                legs: vec![
                    TrackedLeg {
                        venue: String::from("smarkets"),
                        outcome: String::from("Draw"),
                        side: String::from("lay"),
                        odds: 3.35,
                        stake: 9.91,
                        status: String::from("open"),
                        market: String::from("Full-time result"),
                        market_family: String::from("match_odds"),
                        line: None,
                        liability: None,
                        commission_rate: Some(0.0),
                        exchange: Some(String::from("smarkets")),
                        placed_at: String::new(),
                        settled_at: String::new(),
                    },
                    TrackedLeg {
                        venue: String::from("bet365"),
                        outcome: String::from("Draw"),
                        side: String::from("back"),
                        odds: 2.12,
                        stake: 2.0,
                        status: String::from("matched"),
                        market: String::from("Full-time result"),
                        market_family: String::from("match_odds"),
                        line: None,
                        liability: None,
                        commission_rate: None,
                        exchange: None,
                        placed_at: String::new(),
                        settled_at: String::new(),
                    },
                ],
            }],
            exit_policy: ExitPolicySummary {
                target_profit: 5.0,
                stop_loss: 5.0,
                hard_margin_call_profit_floor: None,
                warn_only_default: true,
            },
            exit_recommendations: vec![ExitRecommendation {
                bet_id: String::from("bet-001"),
                action: String::from("hold"),
                reason: String::from("within_thresholds"),
                worst_case_pnl: 1.27,
                cash_out_venue: Some(String::from("smarkets")),
            }],
        }
    }
}
