use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Gauge, List, ListItem, ListState, Paragraph, Row, Table, Wrap,
};
use ratatui::Frame;

use crate::domain::{ExchangePanelSnapshot, VenueStatus, VenueSummary};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    list_state: &mut ListState,
) {
    let layout = Layout::vertical([Constraint::Length(7), Constraint::Min(16)]).split(area);
    let body = Layout::horizontal([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(layout[1]);
    let right = Layout::vertical([
        Constraint::Length(10),
        Constraint::Length(5),
        Constraint::Min(8),
    ])
    .split(body[1]);

    render_summary(frame, layout[0], snapshot);
    render_venue_list(frame, body[0], snapshot, list_state);
    render_selected_venue(frame, right[0], snapshot, list_state.selected());
    render_health_gauges(frame, right[1], snapshot);
    render_feed_preview(frame, right[2], snapshot);
}

fn render_summary(frame: &mut Frame<'_>, area: Rect, snapshot: &ExchangePanelSnapshot) {
    let connected = snapshot
        .venues
        .iter()
        .filter(|venue| matches!(venue.status, VenueStatus::Connected | VenueStatus::Ready))
        .count();
    let selected = selected_venue(snapshot, None)
        .map(|venue| venue.label.as_str())
        .unwrap_or("none");
    let runtime = snapshot.runtime.as_ref();
    let cards = Layout::horizontal([
        Constraint::Percentage(24),
        Constraint::Percentage(24),
        Constraint::Percentage(24),
        Constraint::Percentage(28),
    ])
    .split(area);

    let connected_card = Paragraph::new(vec![
        Line::styled("󰄨 connected", Style::default().fg(muted_text())),
        Line::styled(
            format!("{connected}/{}", snapshot.venues.len()),
            Style::default()
                .fg(accent_green())
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .block(section_block("󰀶 Venue Board", accent_blue()));
    frame.render_widget(connected_card, cards[0]);

    let exposure = snapshot
        .account_stats
        .as_ref()
        .map(|stats| format!("{:.2} {}", stats.exposure, stats.currency))
        .unwrap_or_else(|| String::from("-"));
    let exposure_card = Paragraph::new(vec![
        Line::styled("󰞇 exposure", Style::default().fg(muted_text())),
        Line::styled(
            exposure,
            Style::default()
                .fg(accent_pink())
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .block(section_block("󰖌 Risk", accent_pink()));
    frame.render_widget(exposure_card, cards[1]);

    let feed_card = Paragraph::new(vec![
        Line::styled("󰇚 live rows", Style::default().fg(muted_text())),
        Line::styled(
            format!(
                "{} exchange • {} sportsbook",
                snapshot.open_positions.len(),
                snapshot.other_open_bets.len()
            ),
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .block(section_block("󰈀 Feed", accent_cyan()));
    frame.render_widget(feed_card, cards[2]);

    let runtime_card = Paragraph::new(vec![
        Line::raw(snapshot.status_line.as_str()),
        Line::from(vec![
            Span::styled("󰀵 ", Style::default().fg(muted_text())),
            Span::raw(selected),
            Span::raw("   "),
            Span::styled("󰒋 ", Style::default().fg(muted_text())),
            Span::styled(
                format!("{:?}", snapshot.worker.status),
                Style::default().fg(venue_status_color_from_worker(snapshot.worker.status)),
            ),
        ]),
        Line::from(vec![
            Span::styled("󰅐 ", Style::default().fg(muted_text())),
            Span::raw(
                runtime
                    .map(|summary| summary.updated_at.as_str())
                    .unwrap_or("unknown"),
            ),
        ]),
    ])
    .block(section_block("󱎆 Runtime", accent_green()))
    .wrap(Wrap { trim: true });
    frame.render_widget(runtime_card, cards[3]);
}

fn render_venue_list(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    list_state: &mut ListState,
) {
    let items = if snapshot.venues.is_empty() {
        vec![ListItem::new("No venues loaded.")]
    } else {
        snapshot
            .venues
            .iter()
            .map(render_venue_item)
            .collect::<Vec<_>>()
    };

    let venue_list = List::new(items)
        .block(section_block("󰀶 Accounts", accent_cyan()))
        .highlight_symbol("● ")
        .highlight_style(
            Style::default()
                .fg(selected_text())
                .bg(selected_background())
                .add_modifier(Modifier::BOLD),
        )
        .repeat_highlight_symbol(true);
    frame.render_stateful_widget(venue_list, area, list_state);
}

fn render_venue_item(venue: &VenueSummary) -> ListItem<'static> {
    let status_color = venue_status_color(venue.status);
    let status_icon = match venue.status {
        VenueStatus::Connected | VenueStatus::Ready => "󰄬",
        VenueStatus::Planned => "󰔷",
        VenueStatus::Error => "󰅚",
    };
    ListItem::new(vec![
        Line::from(vec![
            Span::styled(status_icon, Style::default().fg(status_color)),
            Span::raw(" "),
            Span::styled(
                venue.label.clone(),
                Style::default()
                    .fg(text_color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{:?}", venue.status),
                Style::default().fg(status_color),
            ),
        ]),
        Line::from(vec![
            Span::styled(venue.id.as_str(), Style::default().fg(muted_text())),
            Span::raw("  "),
            Span::raw(format!(
                "events {} | markets {}",
                venue.event_count, venue.market_count
            )),
        ]),
    ])
}

fn render_selected_venue(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    selected_index: Option<usize>,
) {
    let Some(venue) = selected_venue(snapshot, selected_index) else {
        let body = Paragraph::new(vec![
            Line::raw("No venue selected."),
            Line::raw("Use Up/Down in Trading > Accounts."),
        ])
        .block(section_block("Selected Venue", accent_gold()))
        .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    };

    let latest_event = snapshot
        .events
        .first()
        .map(|event| format!("{} ({})", event.label, event.competition))
        .unwrap_or_else(|| String::from("No event snapshot"));
    let latest_market = snapshot
        .markets
        .first()
        .map(|market| format!("{} ({} contracts)", market.name, market.contract_count))
        .unwrap_or_else(|| String::from("No market snapshot"));
    let account = snapshot.account_stats.as_ref();

    let rows = vec![
        key_value_row("󰀵 Venue", venue.label.clone(), accent_blue()),
        key_value_row(
            "󰄬 Status",
            format!("{:?}", venue.status),
            venue_status_color(venue.status),
        ),
        key_value_row(
            "󰟈 Balance",
            account
                .map(|stats| format!("{:.2} {}", stats.available_balance, stats.currency))
                .unwrap_or_else(|| String::from("-")),
            accent_green(),
        ),
        key_value_row(
            "󰖌 Exposure",
            account
                .map(|stats| format!("{:.2}", stats.exposure))
                .unwrap_or_else(|| String::from("-")),
            accent_pink(),
        ),
        key_value_row(
            "󰢬 Unrealized",
            account
                .map(|stats| format!("{:+.2}", stats.unrealized_pnl))
                .unwrap_or_else(|| String::from("-")),
            pnl_color(account.map(|stats| stats.unrealized_pnl).unwrap_or(0.0)),
        ),
        key_value_row("󰍹 Latest event", latest_event, text_color()),
        key_value_row("󰇈 Latest market", latest_market, text_color()),
        key_value_row("󱂬 Detail", venue.detail.clone(), muted_text()),
        key_value_row("󰒋 Worker", snapshot.worker.detail.clone(), muted_text()),
    ];
    let table = Table::new(rows, [Constraint::Length(13), Constraint::Min(10)])
        .block(section_block("󰀵 Selected Venue", accent_gold()))
        .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_health_gauges(frame: &mut Frame<'_>, area: Rect, snapshot: &ExchangePanelSnapshot) {
    let sections =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    let exposure_ratio = snapshot
        .account_stats
        .as_ref()
        .map(|stats| ratio(stats.exposure, stats.available_balance))
        .unwrap_or(0.0);
    let connected_ratio = if snapshot.venues.is_empty() {
        0.0
    } else {
        snapshot
            .venues
            .iter()
            .filter(|venue| matches!(venue.status, VenueStatus::Connected | VenueStatus::Ready))
            .count() as f64
            / snapshot.venues.len() as f64
    };

    render_gauge(
        frame,
        sections[0],
        "Balance Use",
        exposure_ratio,
        accent_pink(),
        format!("{:.0}%", exposure_ratio * 100.0),
    );
    render_gauge(
        frame,
        sections[1],
        "Connected Venues",
        connected_ratio,
        accent_green(),
        format!(
            "{}/{}",
            snapshot
                .venues
                .iter()
                .filter(|venue| matches!(venue.status, VenueStatus::Connected | VenueStatus::Ready))
                .count(),
            snapshot.venues.len()
        ),
    );
}

fn render_feed_preview(frame: &mut Frame<'_>, area: Rect, snapshot: &ExchangePanelSnapshot) {
    let mut rows = snapshot
        .open_positions
        .iter()
        .take(4)
        .map(|row| {
            Row::new(vec![
                Cell::from(String::from("exchange")),
                Cell::from(if row.event.is_empty() {
                    row.contract.clone()
                } else {
                    row.event.clone()
                }),
                Cell::from(row.market.clone()),
                Cell::from(if row.market_status.is_empty() {
                    if row.can_trade_out {
                        String::from("trade-out")
                    } else if row.is_in_play {
                        String::from("in-play")
                    } else {
                        String::from("open")
                    }
                } else {
                    row.market_status.clone()
                }),
            ])
        })
        .collect::<Vec<_>>();

    rows.extend(snapshot.other_open_bets.iter().take(4).map(|row| {
        Row::new(vec![
            Cell::from(String::from("sportsbook")),
            Cell::from(row.label.clone()),
            Cell::from(row.market.clone()),
            Cell::from(row.status.clone()),
        ])
    }));

    if rows.is_empty() {
        let body = Paragraph::new(vec![
            Line::raw("No live rows loaded."),
            Line::raw("Start the recorder or refresh the active provider."),
        ])
        .block(section_block("Live Feed Preview", accent_green()))
        .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Percentage(32),
            Constraint::Percentage(34),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec!["Type", "Item", "Market", "State"]).style(
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(section_block("󰈀 Live Feed Preview", accent_green()))
    .column_spacing(1);
    frame.render_widget(table, area);
}

fn selected_venue(
    snapshot: &ExchangePanelSnapshot,
    selected_index: Option<usize>,
) -> Option<&VenueSummary> {
    selected_index
        .and_then(|index| snapshot.venues.get(index))
        .or_else(|| {
            snapshot
                .selected_venue
                .and_then(|selected| snapshot.venues.iter().find(|venue| venue.id == selected))
        })
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

fn pnl_color(value: f64) -> Color {
    if value > 0.0 {
        accent_green()
    } else if value < 0.0 {
        accent_red()
    } else {
        muted_text()
    }
}

fn venue_status_color_from_worker(status: crate::domain::WorkerStatus) -> Color {
    match status {
        crate::domain::WorkerStatus::Ready => accent_green(),
        crate::domain::WorkerStatus::Busy => accent_gold(),
        crate::domain::WorkerStatus::Idle => muted_text(),
        crate::domain::WorkerStatus::Error => accent_red(),
    }
}

fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator <= 0.0 {
        0.0
    } else {
        (numerator / denominator).clamp(0.0, 1.0)
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

fn venue_status_color(status: VenueStatus) -> Color {
    match status {
        VenueStatus::Connected | VenueStatus::Ready => accent_green(),
        VenueStatus::Planned => accent_gold(),
        VenueStatus::Error => accent_red(),
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

fn selected_background() -> Color {
    crate::theme::selected_background()
}

fn selected_text() -> Color {
    crate::theme::selected_text()
}
