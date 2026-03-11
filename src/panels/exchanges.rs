use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::domain::{ExchangePanelSnapshot, VenueSummary};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    list_state: &mut ListState,
) {
    let layout =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).split(area);

    let items = snapshot.venues.iter().map(render_venue_item);
    let venue_list = List::new(items)
        .block(Block::default().title("Venues").borders(Borders::ALL))
        .highlight_symbol(">> ");
    frame.render_stateful_widget(venue_list, layout[0], list_state);

    render_details(frame, layout[1], snapshot, list_state.selected());
}

fn render_venue_item(venue: &VenueSummary) -> ListItem<'static> {
    ListItem::new(format!(
        "{} [{}] events={} markets={}",
        venue.label,
        venue.id.as_str(),
        venue.event_count,
        venue.market_count,
    ))
}

#[derive(Debug, Clone)]
struct DetailSection {
    title: &'static str,
    rows: Vec<Line<'static>>,
    compact: bool,
}

fn render_details(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    selected_index: Option<usize>,
) {
    let sections = selected_sections(snapshot, selected_index);
    if sections.is_empty() {
        return;
    }

    let constraints = sections
        .iter()
        .map(|section| {
            if section.compact {
                Constraint::Length((section.rows.len() as u16).saturating_add(2))
            } else {
                Constraint::Min(5)
            }
        })
        .collect::<Vec<_>>();
    let layout = Layout::vertical(constraints).split(area);

    for (rect, section) in layout.iter().zip(sections.into_iter()) {
        let body = Paragraph::new(section.rows)
            .block(Block::default().title(section.title).borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        frame.render_widget(body, *rect);
    }
}

fn selected_sections(
    snapshot: &ExchangePanelSnapshot,
    selected_index: Option<usize>,
) -> Vec<DetailSection> {
    let venue = selected_index
        .and_then(|index| snapshot.venues.get(index))
        .or_else(|| {
            snapshot
                .selected_venue
                .and_then(|selected| snapshot.venues.iter().find(|venue| venue.id == selected))
        });

    let Some(venue) = venue else {
        return vec![DetailSection {
            title: "Venue Details",
            rows: vec![
                Line::raw("No venue selected."),
                Line::raw("Press j/k or arrow keys in the Exchanges panel."),
            ],
            compact: true,
        }];
    };

    let latest_event = snapshot
        .events
        .first()
        .map(|event| format!("{} ({})", event.label, event.competition))
        .unwrap_or_else(|| String::from("No event selected"));
    let latest_market = snapshot
        .markets
        .first()
        .map(|market| format!("{} ({} contracts)", market.name, market.contract_count))
        .unwrap_or_else(|| String::from("No market snapshot loaded"));

    let mut sections = vec![DetailSection {
        title: "Summary",
        rows: vec![
            Line::raw(format!("Venue: {}", venue.label)),
            Line::raw(format!("Status: {:?}", venue.status)),
            Line::raw(format!("Detail: {}", venue.detail)),
            Line::raw(format!("Latest event: {}", latest_event)),
            Line::raw(format!("Latest market: {}", latest_market)),
            Line::raw(format!("Worker: {}", snapshot.worker.detail)),
        ],
        compact: true,
    }];

    if let Some(account_stats) = &snapshot.account_stats {
        sections.push(DetailSection {
            title: "Account",
            rows: vec![
                Line::raw(format!(
                    "Balance: {:.2} {}",
                    account_stats.available_balance, account_stats.currency
                )),
                Line::raw(format!(
                    "Exposure: {:.2} | P/L: {:.2}",
                    account_stats.exposure, account_stats.unrealized_pnl
                )),
            ],
            compact: true,
        });
    }

    if !snapshot.open_positions.is_empty() {
        let mut rows = vec![Line::raw(format!(
            "Count: {}",
            snapshot.open_positions.len()
        ))];
        for row in snapshot.open_positions.iter().take(3) {
            rows.push(Line::raw(format!("{} | {}", row.contract, row.market)));
            rows.push(Line::raw(format!(
                "stake {:.2} | pnl {:.2} | trade-out {}",
                row.stake,
                row.pnl_amount,
                yes_no(row.can_trade_out)
            )));
        }
        sections.push(DetailSection {
            title: "Open Positions",
            rows,
            compact: false,
        });
    }

    if !snapshot.other_open_bets.is_empty() {
        let mut rows = vec![Line::raw(format!(
            "Count: {}",
            snapshot.other_open_bets.len()
        ))];
        for row in snapshot.other_open_bets.iter().take(3) {
            rows.push(Line::raw(format!("{} | {}", row.label, row.market)));
            rows.push(Line::raw(format!(
                "{} @ {:.2} | stake {:.2} | {}",
                row.side, row.odds, row.stake, row.status
            )));
        }
        sections.push(DetailSection {
            title: "Other Open Bets",
            rows,
            compact: false,
        });
    }

    if let Some(watch) = &snapshot.watch {
        let mut rows = vec![Line::raw(format!("Groups: {}", watch.watch_count))];
        for row in watch.watches.iter().take(3) {
            rows.push(Line::raw(format!("{} | {}", row.contract, row.market)));
            rows.push(Line::raw(format!(
                "profit {:.2} | stop {:.2}",
                row.profit_take_back_odds, row.stop_loss_back_odds,
            )));
        }
        sections.push(DetailSection {
            title: "Watch Plan",
            rows,
            compact: false,
        });
    }

    sections
}

#[cfg(test)]
fn section_texts(snapshot: &ExchangePanelSnapshot, selected_index: Option<usize>) -> Vec<String> {
    selected_sections(snapshot, selected_index)
        .into_iter()
        .flat_map(|section| {
            std::iter::once(section.title.to_string())
                .chain(section.rows.into_iter().map(|line| line.to_string()))
        })
        .collect()
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::section_texts;
    use crate::domain::{
        AccountStats, ExchangePanelSnapshot, OpenPositionRow, OtherOpenBetRow, VenueId,
        VenueStatus, VenueSummary, WatchRow, WatchSnapshot, WorkerStatus, WorkerSummary,
    };

    #[test]
    fn selected_details_include_watch_rows_when_present() {
        let snapshot = ExchangePanelSnapshot {
            worker: WorkerSummary {
                name: String::from("bet-recorder"),
                status: WorkerStatus::Ready,
                detail: String::from("Loaded watch snapshot"),
            },
            venues: vec![VenueSummary {
                id: VenueId::Smarkets,
                label: String::from("Smarkets"),
                status: VenueStatus::Ready,
                detail: String::from("Watching positions"),
                event_count: 2,
                market_count: 2,
            }],
            selected_venue: Some(VenueId::Smarkets),
            events: vec![],
            markets: vec![],
            preflight: None,
            status_line: String::from("Loaded watch snapshot"),
            account_stats: Some(AccountStats {
                available_balance: 120.45,
                exposure: 41.63,
                unrealized_pnl: -0.49,
                currency: String::from("GBP"),
            }),
            open_positions: vec![OpenPositionRow {
                contract: String::from("Draw"),
                market: String::from("Full-time result"),
                price: 3.35,
                stake: 9.91,
                liability: 23.29,
                current_value: 9.60,
                pnl_amount: -0.31,
                can_trade_out: true,
            }],
            other_open_bets: vec![OtherOpenBetRow {
                label: String::from("Arsenal"),
                market: String::from("Full-time result"),
                side: String::from("back"),
                odds: 2.12,
                stake: 5.0,
                status: String::from("Open"),
            }],
            watch: Some(WatchSnapshot {
                position_count: 3,
                watch_count: 2,
                commission_rate: 0.0,
                target_profit: 1.0,
                stop_loss: 1.0,
                watches: vec![WatchRow {
                    contract: String::from("Draw"),
                    market: String::from("Full-time result"),
                    position_count: 1,
                    can_trade_out: true,
                    total_stake: 9.91,
                    total_liability: 23.29,
                    current_pnl_amount: -0.31,
                    average_entry_lay_odds: 3.35,
                    entry_implied_probability: 0.2985,
                    profit_take_back_odds: 3.73,
                    profit_take_implied_probability: 0.2684,
                    stop_loss_back_odds: 3.04,
                    stop_loss_implied_probability: 0.3286,
                }],
            }),
        };

        let rendered = section_texts(&snapshot, Some(0)).join("\n");

        assert!(rendered.contains("Summary"));
        assert!(rendered.contains("Account"));
        assert!(rendered.contains("Open Positions"));
        assert!(rendered.contains("Other Open Bets"));
        assert!(rendered.contains("Watch Plan"));
        assert!(rendered.contains("Balance: 120.45 GBP"));
        assert!(rendered.contains("Count: 1"));
        assert!(rendered.contains("trade-out yes"));
        assert!(rendered.contains("Arsenal | Full-time result"));
        assert!(rendered.contains("profit 3.73 | stop 3.04"));
    }
}
