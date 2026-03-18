use std::collections::{BTreeMap, BTreeSet};

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::domain::{ExchangePanelSnapshot, VenueStatus};

pub fn render(frame: &mut Frame<'_>, area: Rect, snapshot: &ExchangePanelSnapshot) {
    let total_open_stake: f64 = snapshot.open_positions.iter().map(|row| row.stake).sum();
    let total_liability: f64 = snapshot.open_positions.iter().map(|row| row.liability).sum();
    let total_open_pnl: f64 = snapshot.open_positions.iter().map(|row| row.pnl_amount).sum();
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
        Constraint::Length(6),
        Constraint::Length(5),
        Constraint::Min(12),
    ])
    .split(area);
    let ratios = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .split(layout[1]);
    let lower = Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(layout[2]);
    let left = Layout::vertical([Constraint::Length(9), Constraint::Min(8)]).split(lower[0]);
    let right = Layout::vertical([Constraint::Length(9), Constraint::Min(8)]).split(lower[1]);

    let summary = Paragraph::new(vec![
        Line::raw(format!(
            "Open positions: {} | Sportsbook bets: {} | Tracked bets: {} | Venues in scope: {}",
            snapshot.open_positions.len(),
            snapshot.other_open_bets.len(),
            snapshot.tracked_bets.len(),
            snapshot.venues.len(),
        )),
        Line::raw(format!(
            "Marked P/L: {:+.2} | Liability: {:.2} | Exchange stake: {:.2} | Sportsbook stake: {:.2}",
            total_open_pnl, total_liability, total_open_stake, total_bet_stake,
        )),
        Line::raw(format!(
            "Actionable exits: {} | Decisions: {} | Watch rows: {} | Tracked sources: {}",
            actionable_decisions,
            snapshot.decisions.len(),
            snapshot
                .watch
                .as_ref()
                .map(|watch| watch.watch_count)
                .unwrap_or(0),
            tracked_source_count,
        )),
        Line::raw(format!(
            "Updated: {} | Source: {} | Market EV proxy: {}",
            runtime
                .map(|summary| summary.updated_at.as_str())
                .unwrap_or("unknown"),
            runtime
                .map(|summary| summary.source.as_str())
                .unwrap_or("snapshot"),
            total_market_ev(snapshot)
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| String::from("-")),
        )),
    ])
    .block(section_block("Trading Stats", accent_blue()))
    .wrap(Wrap { trim: true });
    frame.render_widget(summary, layout[0]);

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
    render_gauge(
        frame,
        ratios[2],
        "Action Queue",
        action_ratio,
        accent_gold(),
        format!("{}/{}", actionable_decisions, snapshot.decisions.len()),
    );

    render_venue_table(frame, left[0], snapshot);
    render_capital_table(
        frame,
        left[1],
        snapshot,
        total_open_stake,
        total_liability,
        total_open_pnl,
        total_bet_stake,
    );
    render_decision_table(frame, right[0], snapshot);
    render_tracked_mix_table(frame, right[1], snapshot);
}

fn render_venue_table(frame: &mut Frame<'_>, area: Rect, snapshot: &ExchangePanelSnapshot) {
    if snapshot.venues.is_empty() {
        let body = Paragraph::new(vec![
            Line::raw("No venue summaries loaded."),
            Line::raw("Refresh the provider or start the recorder-backed source."),
        ])
        .block(section_block("Venue Split", accent_cyan()))
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
    .block(section_block("Venue Split", accent_cyan()))
    .column_spacing(1);
    frame.render_widget(table, area);
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
            "Balance",
            account
                .map(|stats| format!("{:.2} {}", stats.available_balance, stats.currency))
                .unwrap_or_else(|| String::from("-")),
            accent_green(),
        ),
        key_value_row("Exchange stake", format!("{total_open_stake:.2}"), accent_blue()),
        key_value_row("Liability", format!("{total_liability:.2}"), accent_pink()),
        key_value_row("Marked P/L", format!("{total_open_pnl:+.2}"), pnl_color(total_open_pnl)),
        key_value_row("Sportsbook stake", format!("{total_bet_stake:.2}"), accent_gold()),
        key_value_row(
            "Avg position",
            format!("{average_position_size:.2}"),
            text_color(),
        ),
        key_value_row(
            "Implied band",
            probability_band_line(snapshot),
            muted_text(),
        ),
        key_value_row(
            "Exit policy",
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
        .block(section_block("Capital & Risk", accent_pink()))
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
            Line::raw("Watch recommendations will appear here once the recorder has a live snapshot."),
        ])
        .block(section_block("Decision Mix", accent_gold()))
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
    .block(section_block("Decision Mix", accent_gold()))
    .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_tracked_mix_table(frame: &mut Frame<'_>, area: Rect, snapshot: &ExchangePanelSnapshot) {
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
    counts.entry(String::from("sportsbook_open")).or_insert(snapshot.other_open_bets.len());
    counts.entry(String::from("exchange_open")).or_insert(snapshot.open_positions.len());

    let rows = counts
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(source, count)| Row::new(vec![Cell::from(source), Cell::from(count.to_string())]))
        .collect::<Vec<_>>();

    if rows.is_empty() {
        let body = Paragraph::new(vec![
            Line::raw("No tracked activity loaded."),
            Line::raw("Recorder and ledger imports will populate this board."),
        ])
        .block(section_block("Tracked Mix", accent_green()))
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
        .block(section_block("Tracked Mix", accent_green()))
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

fn section_block(title: &'static str, color: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
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
    Color::Rgb(16, 22, 30)
}

fn border_color() -> Color {
    Color::Rgb(48, 64, 86)
}

fn text_color() -> Color {
    Color::Rgb(234, 240, 246)
}

fn muted_text() -> Color {
    Color::Rgb(148, 163, 184)
}

fn accent_blue() -> Color {
    Color::Rgb(104, 179, 255)
}

fn accent_cyan() -> Color {
    Color::Rgb(110, 231, 255)
}

fn accent_green() -> Color {
    Color::Rgb(90, 214, 154)
}

fn accent_gold() -> Color {
    Color::Rgb(245, 196, 89)
}

fn accent_pink() -> Color {
    Color::Rgb(255, 122, 162)
}

fn accent_red() -> Color {
    Color::Rgb(255, 107, 107)
}
