use std::collections::BTreeSet;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Borders, Cell, Chart, Clear, Dataset, GraphType, Padding, Paragraph, Row, Table,
    TableState, Wrap,
};
use ratatui::Frame;

use crate::app::PositionsFocus;
use crate::domain::VenueId;
use crate::domain::{
    ExchangePanelSnapshot, OpenPositionRow, OtherOpenBetRow, RecorderEventSummary, TrackedBetRow,
    TrackedLeg, TransportMarkerSummary,
};
use crate::owls::{OwlsDashboard, OwlsEndpointId, OwlsPreviewRow};
use crate::trading_actions::{
    TradingActionSeed, TradingActionSide, TradingActionSource, TradingActionSourceContext,
};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    owls_dashboard: &OwlsDashboard,
    active_table_state: &mut TableState,
    historical_table_state: &mut TableState,
    positions_focus: PositionsFocus,
    show_live_view_overlay: bool,
    status_message: &str,
    status_scroll: u16,
) {
    let active_views = active_position_views(snapshot);
    let exit_recommendations = derived_exit_recommendations(snapshot);
    let selected_active = selected_active_position(&active_views, active_table_state);
    let selected_historical = selected_historical_position(snapshot, historical_table_state);
    let layout = Layout::vertical([Constraint::Length(6), Constraint::Min(18)]).split(area);
    let body = Layout::horizontal([Constraint::Percentage(71), Constraint::Percentage(29)])
        .split(layout[1]);
    let (active_height, historical_height) =
        position_section_heights(snapshot, body[0].height.max(14));
    let left = Layout::vertical([
        Constraint::Length(active_height),
        Constraint::Length(historical_height),
    ])
    .split(body[0]);
    let right = Layout::vertical([
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Min(6),
    ])
    .split(body[1]);

    render_summary(
        frame,
        layout[0],
        snapshot,
        selected_active,
        selected_historical,
        positions_focus,
    );
    let active_title = positions_table_title(
        "󰞇 Active Positions",
        active_views.len(),
        positions_focus == PositionsFocus::Active,
    );
    render_stateful_table(
        frame,
        left[0],
        &active_title,
        vec![
            Constraint::Percentage(18),
            Constraint::Percentage(21),
            Constraint::Length(10),
            Constraint::Length(5),
            Constraint::Length(11),
            Constraint::Length(11),
            Constraint::Length(3),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(12),
        ],
        active_position_rows(snapshot, &active_views),
        empty_row(
            "No active positions are loaded. Start the recorder or refresh the provider.",
            10,
        ),
        active_table_state,
    );
    let historical_title = positions_table_title(
        "󰋪 Historical Positions",
        snapshot.historical_positions.len(),
        positions_focus == PositionsFocus::Historical,
    );
    let history_empty = if snapshot.historical_positions.is_empty() {
        "No historical positions are loaded. Import ledger history to populate this section."
    } else {
        "Historical selection is out of range."
    };
    render_table(
        frame,
        left[1],
        &historical_title,
        vec![
            Constraint::Percentage(18),
            Constraint::Percentage(22),
            Constraint::Length(10),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Length(8),
            Constraint::Min(12),
        ],
        historical_position_rows(snapshot),
        empty_row(history_empty, 9),
        Some(historical_table_state),
    );
    render_signal_board(
        frame,
        right[0],
        snapshot,
        owls_dashboard,
        &exit_recommendations,
        selected_active,
    );
    render_table(
        frame,
        right[1],
        &format!(
            "󰄦 Watch Plan ({})",
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
        None,
    );
    render_operator_log(
        frame,
        right[2],
        snapshot,
        selected_active,
        status_message,
        positions_focus,
        status_scroll,
    );

    if show_live_view_overlay {
        if positions_focus == PositionsFocus::Historical {
            render_historical_view_overlay(frame, area, snapshot, selected_historical);
        } else {
            render_live_view_overlay(frame, area, snapshot, selected_active);
        }
    }
}

#[derive(Clone, Copy)]
struct ActivePositionView<'a> {
    open_position: Option<&'a OpenPositionRow>,
    sportsbook_bet: Option<&'a OtherOpenBetRow>,
    tracked_bet: Option<&'a TrackedBetRow>,
    commission_rate: f64,
    target_profit: f64,
    stop_loss: f64,
    hard_margin_call_profit_floor: Option<f64>,
    warn_only_default: bool,
}

#[derive(Clone)]
struct DerivedExitRecommendation {
    bet_id: String,
    action: String,
    reason: String,
    worst_case_pnl: f64,
}

pub(crate) fn active_position_row_count(snapshot: &ExchangePanelSnapshot) -> usize {
    active_position_views(snapshot).len()
}

pub(crate) fn selected_active_position_seed(
    snapshot: &ExchangePanelSnapshot,
    active_table_state: &TableState,
) -> Option<TradingActionSeed> {
    let active_views = active_position_views(snapshot);
    let view = selected_active_position(&active_views, active_table_state)?;

    let event_name = active_event_label(view);
    let market_name = active_market_name(view);
    let selection_name = active_selection_label(view);
    let event_url = view
        .open_position
        .map(|open_position| open_position.event_url.trim().to_string())
        .filter(|value| !value.is_empty());
    let tracked_bet_id = view
        .tracked_bet
        .map(|tracked_bet| tracked_bet.bet_id.clone())
        .unwrap_or_else(|| String::from("position-row"));
    let default_stake = view
        .open_position
        .map(|open_position| open_position.stake)
        .or_else(|| {
            view.tracked_bet
                .and_then(|tracked_bet| tracked_bet.stake_gbp)
        })
        .or_else(|| {
            view.sportsbook_bet
                .map(|sportsbook_bet| sportsbook_bet.stake)
        });
    let buy_price = view
        .open_position
        .and_then(|open_position| open_position.current_buy_odds);
    let sell_price = view
        .open_position
        .and_then(|open_position| open_position.current_sell_odds);
    let default_side = if buy_price.is_some() {
        TradingActionSide::Buy
    } else {
        TradingActionSide::Sell
    };

    Some(TradingActionSeed {
        source: TradingActionSource::Positions,
        venue: VenueId::Smarkets,
        source_ref: tracked_bet_id,
        event_name,
        market_name,
        selection_name,
        event_url,
        deep_link_url: None,
        betslip_market_id: None,
        betslip_selection_id: None,
        buy_price,
        sell_price,
        default_side,
        default_stake,
        source_context: TradingActionSourceContext {
            is_in_play: view
                .open_position
                .map(|open_position| open_position.is_in_play)
                .unwrap_or(false),
            event_status: view
                .open_position
                .map(|open_position| open_position.event_status.clone())
                .unwrap_or_default(),
            market_status: view
                .open_position
                .map(|open_position| open_position.market_status.clone())
                .unwrap_or_default(),
            live_clock: view
                .open_position
                .map(|open_position| open_position.live_clock.clone())
                .unwrap_or_default(),
            can_trade_out: view
                .open_position
                .map(|open_position| open_position.can_trade_out)
                .unwrap_or(false),
            current_pnl_amount: view
                .open_position
                .map(|open_position| open_position.pnl_amount),
            baseline_stake: view.open_position.map(|open_position| open_position.stake),
            baseline_liability: view
                .open_position
                .map(|open_position| open_position.liability),
            baseline_price: view.open_position.map(|open_position| open_position.price),
        },
        notes: vec![String::from("positions")],
    })
}

pub(crate) fn next_actionable_cash_out_bet_id(snapshot: &ExchangePanelSnapshot) -> Option<String> {
    derived_exit_recommendations(snapshot)
        .into_iter()
        .find(|recommendation| recommendation.action == "cash_out")
        .map(|recommendation| recommendation.bet_id)
}

fn active_position_views(snapshot: &ExchangePanelSnapshot) -> Vec<ActivePositionView<'_>> {
    let commission_rate = snapshot
        .watch
        .as_ref()
        .map(|watch| watch.commission_rate)
        .unwrap_or(0.0);
    let target_profit = snapshot.exit_policy.target_profit;
    let stop_loss = snapshot.exit_policy.stop_loss;
    let hard_margin_call_profit_floor = snapshot.exit_policy.hard_margin_call_profit_floor;
    let warn_only_default = snapshot.exit_policy.warn_only_default;
    let mut used_sportsbook = BTreeSet::new();
    let mut used_tracked = BTreeSet::new();
    let mut rows = Vec::new();

    for open_position in &snapshot.open_positions {
        let tracked_bet = snapshot
            .tracked_bets
            .iter()
            .find(|tracked_bet| tracked_bet_matches_open_position(tracked_bet, open_position));
        if let Some(tracked_bet) = tracked_bet {
            used_tracked.insert(tracked_bet.bet_id.clone());
        }

        let sportsbook_bet = snapshot.other_open_bets.iter().find(|sportsbook_bet| {
            sportsbook_bet_matches_open_position(sportsbook_bet, open_position)
        });
        if let Some(sportsbook_bet) = sportsbook_bet {
            used_sportsbook.insert(sportsbook_bet_identity(sportsbook_bet));
        }

        rows.push(ActivePositionView {
            open_position: Some(open_position),
            sportsbook_bet,
            tracked_bet,
            commission_rate,
            target_profit,
            stop_loss,
            hard_margin_call_profit_floor,
            warn_only_default,
        });
    }

    for tracked_bet in snapshot
        .tracked_bets
        .iter()
        .filter(|tracked_bet| !tracked_bet_is_closed(tracked_bet))
    {
        if used_tracked.contains(&tracked_bet.bet_id) {
            continue;
        }
        let sportsbook_bet = snapshot
            .other_open_bets
            .iter()
            .find(|sportsbook_bet| sportsbook_bet_matches_tracked_bet(sportsbook_bet, tracked_bet));
        if let Some(sportsbook_bet) = sportsbook_bet {
            used_sportsbook.insert(sportsbook_bet_identity(sportsbook_bet));
        }
        rows.push(ActivePositionView {
            open_position: None,
            sportsbook_bet,
            tracked_bet: Some(tracked_bet),
            commission_rate,
            target_profit,
            stop_loss,
            hard_margin_call_profit_floor,
            warn_only_default,
        });
    }

    for sportsbook_bet in &snapshot.other_open_bets {
        if used_sportsbook.contains(&sportsbook_bet_identity(sportsbook_bet)) {
            continue;
        }
        rows.push(ActivePositionView {
            open_position: None,
            sportsbook_bet: Some(sportsbook_bet),
            tracked_bet: None,
            commission_rate,
            target_profit,
            stop_loss,
            hard_margin_call_profit_floor,
            warn_only_default,
        });
    }

    rows
}

fn selected_active_position<'a>(
    active_views: &'a [ActivePositionView<'a>],
    active_table_state: &TableState,
) -> Option<ActivePositionView<'a>> {
    active_table_state
        .selected()
        .and_then(|index| active_views.get(index).copied())
        .or_else(|| active_views.first().copied())
}

fn selected_historical_position<'a>(
    snapshot: &'a ExchangePanelSnapshot,
    historical_table_state: &TableState,
) -> Option<&'a OpenPositionRow> {
    historical_table_state
        .selected()
        .and_then(|index| snapshot.historical_positions.get(index))
        .or_else(|| snapshot.historical_positions.first())
}

fn derived_exit_recommendations(
    snapshot: &ExchangePanelSnapshot,
) -> Vec<DerivedExitRecommendation> {
    active_position_views(snapshot)
        .into_iter()
        .filter_map(derived_exit_recommendation)
        .collect()
}

fn derived_exit_recommendation(view: ActivePositionView<'_>) -> Option<DerivedExitRecommendation> {
    let tracked_bet = view.tracked_bet?;
    let worst_case_pnl = active_current_worst_case(view)
        .or_else(|| active_hold_outcomes(view).map(|(win, lose)| win.min(lose)))
        .unwrap_or(0.0);

    let (action, reason) = if active_exchange_leg(view).is_none() {
        ("hold", "missing_smarkets_leg")
    } else if active_current_back_odds(view).is_none()
        || !view
            .open_position
            .map(|open_position| open_position.can_trade_out)
            .unwrap_or(false)
    {
        ("hold", "cash_out_unavailable")
    } else if let Some(hard_floor) = view.hard_margin_call_profit_floor {
        if worst_case_pnl >= hard_floor {
            ("cash_out", "hard_margin_call")
        } else {
            if worst_case_pnl >= view.target_profit {
                (
                    if view.warn_only_default {
                        "warn"
                    } else {
                        "cash_out"
                    },
                    "target_profit",
                )
            } else if worst_case_pnl <= -view.stop_loss {
                (
                    if view.warn_only_default {
                        "warn"
                    } else {
                        "cash_out"
                    },
                    "stop_loss",
                )
            } else {
                ("hold", "within_thresholds")
            }
        }
    } else if worst_case_pnl >= view.target_profit {
        (
            if view.warn_only_default {
                "warn"
            } else {
                "cash_out"
            },
            "target_profit",
        )
    } else if worst_case_pnl <= -view.stop_loss {
        (
            if view.warn_only_default {
                "warn"
            } else {
                "cash_out"
            },
            "stop_loss",
        )
    } else {
        ("hold", "within_thresholds")
    };

    Some(DerivedExitRecommendation {
        bet_id: tracked_bet.bet_id.clone(),
        action: action.to_string(),
        reason: reason.to_string(),
        worst_case_pnl,
    })
}

fn render_summary(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    selected_active: Option<ActivePositionView<'_>>,
    selected_historical: Option<&OpenPositionRow>,
    positions_focus: PositionsFocus,
) {
    let summary =
        Layout::horizontal([Constraint::Percentage(44), Constraint::Percentage(56)]).split(area);
    let runtime = snapshot.runtime.as_ref();
    let exit_recommendations = derived_exit_recommendations(snapshot);
    let (_, promo_funding_count, _) = tracked_bet_funding_counts(snapshot);
    let (realised_pnl, live_pnl, net_pnl, promo_pnl) = positions_pnl_summary(snapshot);
    let (recent_interactions, pending_interactions, issue_interactions) =
        active_interaction_summary(snapshot);
    let overview_rows = vec![
        (
            "󰅐 Refresh",
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
            "󰒋 Worker",
            format!("{:?}", snapshot.worker.status),
            accent_cyan(),
        ),
        (
            "󰆼 Source",
            runtime
                .map(|summary| summary.source.clone())
                .unwrap_or_else(|| String::from("snapshot")),
            accent_gold(),
        ),
        (
            "󰐹 Scope",
            format!(
                "{} act | {} hist | {} open | {} tracked",
                active_position_row_count(snapshot),
                snapshot.historical_positions.len(),
                snapshot.other_open_bets.len(),
                snapshot.tracked_bets.len(),
            ),
            accent_blue(),
        ),
        (
            "󰥔 State",
            format!(
                "{} live | {} susp | {} rec | {} watch",
                in_play_count(snapshot),
                suspended_count(snapshot),
                exit_recommendations.len(),
                snapshot
                    .watch
                    .as_ref()
                    .map(|watch| watch.watch_count)
                    .unwrap_or(0),
            ),
            accent_pink(),
        ),
        (
            "󰐊 I/O",
            if recent_interactions == 0 {
                String::from("no recent action markers")
            } else {
                format!(
                    "recent {} | pending {} | issues {}",
                    recent_interactions, pending_interactions, issue_interactions
                )
            },
            if issue_interactions > 0 {
                accent_red()
            } else if pending_interactions > 0 {
                accent_gold()
            } else {
                accent_cyan()
            },
        ),
        (
            "󰁔 P/L",
            format!(
                "real {:+.2} | live {:+.2} | net {:+.2}{}",
                realised_pnl,
                live_pnl,
                net_pnl,
                if promo_funding_count > 0 {
                    format!(" | promo {:+.2}", promo_pnl)
                } else {
                    String::new()
                }
            ),
            pnl_color(net_pnl),
        ),
    ];
    render_key_value_table(
        frame,
        summary[0],
        "󰐹 Snapshot",
        overview_rows,
        Constraint::Length(12),
    );

    let selected_rows = if positions_focus == PositionsFocus::Active {
        selected_active
            .map(selected_active_rows)
            .unwrap_or_else(empty_selected_rows)
    } else if let Some(row) = selected_historical {
        vec![
            ("󰕮 Pane", positions_focus.label().to_string(), accent_cyan()),
            ("󰍹 Event", event_label(row), accent_blue()),
            ("󰃭 Date", event_date_label(row), accent_green()),
            ("󰥔 Time", event_time_label(row), accent_cyan()),
            ("󰆼 Position", position_label(row), accent_gold()),
            ("󰄬 Score", score_label(row), accent_green()),
            ("󰅐 Phase", phase_label(row), accent_cyan()),
            (
                "󰄬 Trade",
                format!("{} ({})", trade_label(row), trade_code(row)),
                trade_color(row),
            ),
            (
                "󰌑 Order",
                if row.status.is_empty() {
                    String::from("-")
                } else {
                    row.status.clone()
                },
                accent_green(),
            ),
            (
                "󰖌 Exposure",
                format!(
                    "stake {:.2} | liab {:.2} | value {:.2}",
                    row.stake, row.liability, row.current_value,
                ),
                accent_pink(),
            ),
            (
                "󰇈 Market",
                format!(
                    "buy {} | sell {} | {}",
                    format_optional_back_odds(primary_market_buy_odds(row)),
                    format_optional_back_odds(row.current_sell_odds),
                    format_optional_probability(primary_market_implied_probability(row)),
                ),
                accent_blue(),
            ),
            (
                "󱂬 Marked",
                format!(
                    "value {:.2} | pnl {}",
                    row.current_value,
                    historical_pnl_label(row),
                ),
                historical_pnl_style(row),
            ),
        ]
    } else {
        vec![
            ("󰕮 Pane", positions_focus.label().to_string(), accent_cyan()),
            ("󰍹 Event", String::from("-"), muted_text()),
            ("󰃭 Date", String::from("-"), muted_text()),
            ("󰥔 Time", String::from("-"), muted_text()),
            (
                "󰆼 Position",
                String::from("No active position selected"),
                muted_text(),
            ),
            ("󰄬 Score", String::from("-"), muted_text()),
            ("󰅐 Phase", String::from("-"), muted_text()),
            ("󰄬 Trade", String::from("-"), muted_text()),
            ("󰌑 Order", String::from("-"), muted_text()),
            ("󰖌 Exposure", String::from("-"), muted_text()),
            ("󰇈 Market", String::from("-"), muted_text()),
            ("󱂬 Marked", String::from("-"), muted_text()),
        ]
    };
    render_key_value_table(
        frame,
        summary[1],
        "󰄬 Selected Row",
        selected_rows,
        Constraint::Length(13),
    );
}

fn render_signal_board(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    owls_dashboard: &OwlsDashboard,
    exit_recommendations: &[DerivedExitRecommendation],
    selected_active: Option<ActivePositionView<'_>>,
) {
    let next_action = exit_recommendations
        .first()
        .map(|recommendation| {
            format!(
                "{} {} | worst {:+.2}",
                recommendation.bet_id, recommendation.action, recommendation.worst_case_pnl
            )
        })
        .unwrap_or_else(|| String::from("no exit trigger"));
    let (recent_interactions, pending_interactions, issue_interactions) =
        active_interaction_summary(snapshot);
    let rows = vec![
        ("󰘳 Next", next_action, accent_gold()),
        (
            "󱂬 Selected",
            selected_active
                .map(active_action_label)
                .unwrap_or_else(|| String::from("-")),
            accent_green(),
        ),
        (
            "󰇚 Sharp",
            selected_active
                .map(|view| active_sharp_quote_label(owls_dashboard, view))
                .unwrap_or_else(|| String::from("-")),
            accent_blue(),
        ),
        (
            "󰐊 I/O",
            format!(
                "{} recent | {} pending | {} issues",
                recent_interactions, pending_interactions, issue_interactions
            ),
            if issue_interactions > 0 {
                accent_red()
            } else if pending_interactions > 0 {
                accent_gold()
            } else {
                accent_cyan()
            },
        ),
        (
            "󰋼 Inventory",
            format!(
                "{} tracked | {} open",
                snapshot.tracked_bets.len(),
                snapshot.other_open_bets.len()
            ),
            accent_blue(),
        ),
    ];
    render_key_value_table(frame, area, "󰔟 Signal Board", rows, Constraint::Length(10));
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
    let exit_recommendations = derived_exit_recommendations(snapshot);
    let (recent_interactions, pending_interactions, issue_interactions) =
        active_interaction_summary(snapshot);
    vec![
        Line::raw(format!(
            "Positions: {} | Other bets: {} | Tracked bets: {} | Recommendations: {}",
            active_position_row_count(snapshot),
            snapshot.other_open_bets.len(),
            snapshot.tracked_bets.len(),
            exit_recommendations.len(),
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
        Line::raw(format!(
            "I/O recent {} | pending {} | issues {}",
            recent_interactions, pending_interactions, issue_interactions
        )),
    ]
}

#[cfg(test)]
fn open_position_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
    let active_views = active_position_views(snapshot);
    if active_views.is_empty() && snapshot.historical_positions.is_empty() {
        return vec![Line::raw("No open positions are loaded.")];
    }

    let mut rows = Vec::new();
    if !active_views.is_empty() {
        rows.push(Line::raw(format!(
            "Active Positions ({})",
            active_views.len()
        )));
    }
    for view in active_views.iter().take(6) {
        rows.push(Line::raw(format!(
            "{} | {}",
            active_event_label(*view),
            active_position_label(*view)
        )));
        rows.push(Line::raw(format!(
            "hold {} | lock {} | action {}",
            active_hold_label(*view),
            active_lock_label(*view),
            active_action_label(*view),
        )));
        rows.push(Line::raw(format!(
            "status {} | live {} | {}",
            active_status_label(*view),
            active_live_odds_label(*view),
            active_trigger_label(*view),
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
                "status {} | pnl {} | buy {} | {}",
                if row.status.is_empty() {
                    String::from("-")
                } else {
                    row.status.clone()
                },
                historical_pnl_label(row),
                format_optional_back_odds(primary_market_buy_odds(row)),
                format_optional_probability(primary_market_implied_probability(row)),
            )));
        }
    }
    rows
}

fn active_position_rows(
    snapshot: &ExchangePanelSnapshot,
    active_views: &[ActivePositionView<'_>],
) -> Vec<Row<'static>> {
    active_views
        .iter()
        .copied()
        .map(|view| {
            Row::new(vec![
                Cell::from(truncate_text(&active_event_label(view), 28)),
                Cell::from(truncate_text(&active_position_label(view), 34)),
                Cell::from(active_date_label(view)),
                Cell::from(active_time_label(view)),
                Cell::from(active_hold_label(view)),
                Cell::from(active_lock_label(view)),
                active_action_cell(view),
                active_interaction_cell(snapshot, view),
                Cell::from(active_probability_label(view)),
                Cell::from(active_trigger_label(view)),
            ])
        })
        .collect()
}

fn historical_position_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    snapshot
        .historical_positions
        .iter()
        .map(|row| {
            Row::new(vec![
                Cell::from(event_table_label(row)),
                Cell::from(position_table_label(row)),
                Cell::from(event_date_label(row)),
                Cell::from(event_time_label(row)),
                Cell::from(score_label(row)),
                Cell::from(phase_label(row)),
                trade_cell(row),
                historical_pnl_cell(row),
                Cell::from(market_price_label(row)),
            ])
        })
        .collect()
}

#[cfg(test)]
fn watch_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
    let active_views = active_position_views(snapshot);
    if active_views.is_empty() {
        return legacy_watch_lines(snapshot);
    }
    if !has_paired_active_position(&active_views) {
        return legacy_watch_lines(snapshot);
    }

    let mut rows = vec![
        Line::raw(format!(
            "Target profit {:.2} | Stop loss {:.2}",
            snapshot
                .watch
                .as_ref()
                .map(|watch| watch.target_profit)
                .unwrap_or(0.0),
            snapshot
                .watch
                .as_ref()
                .map(|watch| watch.stop_loss)
                .unwrap_or(0.0)
        )),
        Line::raw(format!(
            "Commission rate {:.2}",
            snapshot
                .watch
                .as_ref()
                .map(|watch| watch.commission_rate)
                .unwrap_or(0.0)
        )),
        Line::raw(String::new()),
    ];

    for view in active_views.iter().take(6).copied() {
        rows.push(Line::raw(format!(
            "{} | {}",
            active_selection_label(view),
            active_market_name(view)
        )));
        rows.push(Line::raw(format!(
            "live {} | profit {} | stop {}",
            active_live_odds_label(view),
            active_profit_target_odds(view)
                .map(|value| format!("{:.2}", value))
                .unwrap_or_else(|| String::from("-")),
            active_stop_loss_odds(view)
                .map(|value| format!("{:.2}", value))
                .unwrap_or_else(|| String::from("-")),
        )));
        rows.push(Line::raw(format!(
            "prob entry {} | live {} | profit {} | stop {}",
            active_entry_probability_label(view),
            active_probability_label(view),
            active_profit_target_odds(view)
                .map(implied_probability)
                .map(format_probability)
                .unwrap_or_else(|| String::from("-")),
            active_stop_loss_odds(view)
                .map(implied_probability)
                .map(format_probability)
                .unwrap_or_else(|| String::from("-")),
        )));
        rows.push(Line::raw(format!(
            "hold {} | lock {} | action {}",
            active_hold_label(view),
            active_lock_label(view),
            active_action_label(view),
        )));
        rows.push(Line::raw(format!(
            "gaps profit {} stop {}",
            active_profit_target_odds(view)
                .zip(active_current_back_odds(view))
                .map(|(target, live)| format!("{:+.2}", target - live))
                .unwrap_or_else(|| String::from("-")),
            active_stop_loss_odds(view)
                .zip(active_current_back_odds(view))
                .map(|(stop, live)| format!("{:+.2}", stop - live))
                .unwrap_or_else(|| String::from("-")),
        )));
    }
    rows
}

fn watch_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    let active_views = active_position_views(snapshot);
    if active_views.is_empty() {
        return legacy_watch_rows(snapshot);
    }
    if !has_paired_active_position(&active_views) {
        return legacy_watch_rows(snapshot);
    }

    active_views
        .into_iter()
        .map(|view| {
            Row::new(vec![
                Cell::from(active_selection_label(view)),
                Cell::from(active_leg_summary(view)),
                Cell::from(active_live_odds_label(view)),
                Cell::from(
                    active_profit_target_odds(view)
                        .map(|value| format!("{:.2}", value))
                        .unwrap_or_else(|| String::from("-")),
                ),
                Cell::from(
                    active_stop_loss_odds(view)
                        .map(|value| format!("{:.2}", value))
                        .unwrap_or_else(|| String::from("-")),
                ),
                pnl_cell(
                    active_current_worst_case(view).unwrap_or_else(|| active_marked_pnl(view)),
                ),
            ])
        })
        .collect()
}

fn has_paired_active_position(active_views: &[ActivePositionView<'_>]) -> bool {
    active_views
        .iter()
        .any(|view| view.sportsbook_bet.is_some() || view.tracked_bet.is_some())
}

#[cfg(test)]
fn legacy_watch_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
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
            "live {} | profit {} | stop {}",
            row.current_back_odds
                .map(|value| format!("{:.2}", value))
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

fn legacy_watch_rows(snapshot: &ExchangePanelSnapshot) -> Vec<Row<'static>> {
    let Some(watch) = &snapshot.watch else {
        return Vec::new();
    };

    watch
        .watches
        .iter()
        .map(|row| {
            Row::new(vec![
                Cell::from(row.contract.clone()),
                Cell::from(row.market.clone()),
                Cell::from(
                    row.current_back_odds
                        .map(|value| format!("{:.2}", value))
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

fn active_event_label(view: ActivePositionView<'_>) -> String {
    if let Some(open_position) = view.open_position {
        if !open_position.event.is_empty() {
            return open_position.event.clone();
        }
    }
    if let Some(tracked_bet) = view.tracked_bet {
        if !tracked_bet.event.is_empty() {
            return tracked_bet.event.clone();
        }
    }
    view.sportsbook_bet
        .map(|sportsbook_bet| sportsbook_bet.event.clone())
        .filter(|event| !event.is_empty())
        .unwrap_or_else(|| String::from("-"))
}

fn active_position_label(view: ActivePositionView<'_>) -> String {
    format!(
        "{} · {}",
        active_selection_label(view),
        active_leg_summary(view)
    )
}

fn active_selection_label(view: ActivePositionView<'_>) -> String {
    if let Some(tracked_bet) = view.tracked_bet {
        if !tracked_bet.selection.is_empty() {
            return tracked_bet.selection.clone();
        }
    }
    if let Some(open_position) = view.open_position {
        if !open_position.contract.is_empty() {
            return open_position.contract.clone();
        }
    }
    view.sportsbook_bet
        .map(|sportsbook_bet| sportsbook_bet.label.clone())
        .unwrap_or_else(|| String::from("-"))
}

fn active_market_name(view: ActivePositionView<'_>) -> String {
    if let Some(open_position) = view.open_position {
        if !open_position.market.is_empty() {
            return open_position.market.clone();
        }
    }
    if let Some(sportsbook_bet) = view.sportsbook_bet {
        if !sportsbook_bet.market.is_empty() {
            return sportsbook_bet.market.clone();
        }
    }
    if let Some(tracked_bet) = view.tracked_bet {
        if !tracked_bet.market.is_empty() {
            return tracked_bet.market.clone();
        }
    }
    String::from("-")
}

fn active_leg_summary(view: ActivePositionView<'_>) -> String {
    let sportsbook = active_sportsbook_leg(view)
        .map(|(venue, _, odds, stake)| format!("{venue} {stake:.2}@{odds:.2}"))
        .unwrap_or_else(|| String::from("book -"));
    let exchange = active_live_exchange_leg(view)
        .map(|(venue, entry_odds, stake, _)| format!("{venue} {stake:.2}@{entry_odds:.2}"))
        .or_else(|| {
            active_tracked_exchange_leg(view).map(|(venue, _, _, _)| format!("{venue} closed"))
        })
        .unwrap_or_else(|| String::from("lay -"));
    format!("{sportsbook} ↔ {exchange}")
}

fn active_date_label(view: ActivePositionView<'_>) -> String {
    if let Some(open_position) = view.open_position {
        let date = event_date_label(open_position);
        if date != "-" {
            return date;
        }
    }
    if let Some(tracked_bet) = view.tracked_bet {
        if let Some((date, _)) = parse_isoish_datetime(&tracked_bet.placed_at) {
            return date;
        }
    }
    String::from("-")
}

fn active_time_label(view: ActivePositionView<'_>) -> String {
    if let Some(open_position) = view.open_position {
        let time = event_time_label(open_position);
        if time != "-" {
            return time;
        }
    }
    if let Some(tracked_bet) = view.tracked_bet {
        if let Some((_, time)) = parse_isoish_datetime(&tracked_bet.placed_at) {
            return time;
        }
    }
    String::from("-")
}

fn active_hold_label(view: ActivePositionView<'_>) -> String {
    match active_hold_outcomes(view) {
        Some((win, lose)) => format!("{win:+.2}/{lose:+.2}"),
        None => format!("{:+.2}", active_marked_pnl(view)),
    }
}

fn active_half_exit_label(view: ActivePositionView<'_>) -> String {
    match active_half_cashout_outcomes(view) {
        Some((win, lose)) => format!("{win:+.2}/{lose:+.2}"),
        None => String::from("-"),
    }
}

fn active_lock_label(view: ActivePositionView<'_>) -> String {
    match active_total_cashout_outcomes(view) {
        Some((win, lose)) => format!("{win:+.2}/{lose:+.2}"),
        None => String::from("-"),
    }
}

fn active_probability_label(view: ActivePositionView<'_>) -> String {
    format_optional_probability(active_current_probability(view))
}

fn active_live_odds_label(view: ActivePositionView<'_>) -> String {
    active_current_back_odds(view)
        .map(|value| format!("{:.2}", value))
        .unwrap_or_else(|| String::from("-"))
}

fn active_trigger_label(view: ActivePositionView<'_>) -> String {
    let live = active_live_odds_label(view);
    let profit = active_profit_target_odds(view)
        .map(|value| format!("{:.2}", value))
        .unwrap_or_else(|| String::from("-"));
    let stop = active_stop_loss_odds(view)
        .map(|value| format!("{:.2}", value))
        .unwrap_or_else(|| String::from("-"));
    format!("live {live} | tgt {profit} | stop {stop}")
}

fn active_status_label(view: ActivePositionView<'_>) -> String {
    let sportsbook = view
        .sportsbook_bet
        .map(|sportsbook_bet| sportsbook_bet.status.as_str())
        .unwrap_or("-");
    let exchange = if let Some(open_position) = view.open_position {
        if open_position.status.is_empty() {
            "-"
        } else {
            open_position.status.as_str()
        }
    } else if active_tracked_exchange_leg(view).is_some() {
        "closed"
    } else {
        "-"
    };
    format!("book {sportsbook} | lay {exchange}")
}

fn active_exposure_label(view: ActivePositionView<'_>) -> String {
    if let Some(open_position) = view.open_position {
        format!(
            "book {:.2} | lay {:.2} | liab {:.2}",
            view.sportsbook_bet.map(|bet| bet.stake).unwrap_or(0.0),
            open_position.stake,
            open_position.liability,
        )
    } else {
        format!(
            "book {:.2}",
            view.sportsbook_bet.map(|bet| bet.stake).unwrap_or(0.0)
        )
    }
}

fn active_bookie_cashout_label(view: ActivePositionView<'_>) -> String {
    view.sportsbook_bet
        .and_then(|sportsbook_bet| sportsbook_bet.current_cashout_value)
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| String::from("-"))
}

fn active_action_label(view: ActivePositionView<'_>) -> String {
    if let (Some(current), Some(target)) = (
        active_current_back_odds(view),
        active_profit_target_odds(view),
    ) {
        if current >= target {
            return String::from("target");
        }
    }
    if let (Some(current), Some(stop)) =
        (active_current_back_odds(view), active_stop_loss_odds(view))
    {
        if current <= stop {
            return String::from("stop");
        }
    }
    if view.open_position.is_some() && (view.sportsbook_bet.is_some() || view.tracked_bet.is_some())
    {
        String::from("watch")
    } else {
        String::from("hold")
    }
}

fn active_action_cell(view: ActivePositionView<'_>) -> Cell<'static> {
    let label = active_action_label(view);
    let color = if label == "target" {
        accent_green()
    } else if label == "stop" {
        accent_red()
    } else if label == "watch" {
        accent_gold()
    } else {
        muted_text()
    };
    Cell::from(truncate_text(&label, 3)).style(Style::default().fg(color))
}

fn active_marked_pnl(view: ActivePositionView<'_>) -> f64 {
    view.open_position
        .map(|open_position| open_position.pnl_amount)
        .unwrap_or(0.0)
}

fn active_current_worst_case(view: ActivePositionView<'_>) -> Option<f64> {
    active_total_cashout_outcomes(view).map(|(win, lose)| win.min(lose))
}

fn active_half_cashout_outcomes(view: ActivePositionView<'_>) -> Option<(f64, f64)> {
    active_fractional_cashout_outcomes(view, 0.5)
}

fn active_current_back_odds(view: ActivePositionView<'_>) -> Option<f64> {
    view.open_position
        .and_then(|open_position| open_position.current_back_odds)
}

fn active_current_probability(view: ActivePositionView<'_>) -> Option<f64> {
    view.open_position
        .and_then(|open_position| open_position.current_implied_probability)
        .or_else(|| active_current_back_odds(view).map(implied_probability))
}

fn active_entry_probability_label(view: ActivePositionView<'_>) -> String {
    active_live_exchange_leg(view)
        .map(|(_, entry_odds, _, _)| implied_probability(entry_odds))
        .map(format_probability)
        .unwrap_or_else(|| String::from("-"))
}

fn active_profit_target_odds(view: ActivePositionView<'_>) -> Option<f64> {
    active_exit_odds_for_total_target(view, view.target_profit)
}

fn active_stop_loss_odds(view: ActivePositionView<'_>) -> Option<f64> {
    active_exit_odds_for_total_target(view, -view.stop_loss)
}

fn active_exit_odds_for_total_target(
    view: ActivePositionView<'_>,
    overall_target: f64,
) -> Option<f64> {
    let (_, entry_odds, stake, commission_rate) = active_live_exchange_leg(view)?;
    let (other_win, other_lose) = active_non_exchange_outcomes(view)?;
    let other_worst_case = other_win.min(other_lose);
    exit_odds_for_target_profit(
        entry_odds,
        stake,
        commission_rate,
        overall_target - other_worst_case,
    )
}

fn active_hold_outcomes(view: ActivePositionView<'_>) -> Option<(f64, f64)> {
    let (other_wins, other_loses) = active_non_exchange_outcomes(view).unwrap_or((0.0, 0.0));
    let Some((_, entry_odds, stake, commission_rate)) = active_live_exchange_leg(view) else {
        if view.sportsbook_bet.is_some() || view.tracked_bet.is_some() {
            return Some((other_wins, other_loses));
        }
        return None;
    };
    let exchange_wins = settled_leg_pnl("lay", entry_odds, stake, commission_rate, true);
    let exchange_loses = settled_leg_pnl("lay", entry_odds, stake, commission_rate, false);
    Some((exchange_wins + other_wins, exchange_loses + other_loses))
}

fn active_cashout_outcomes(view: ActivePositionView<'_>) -> Option<(f64, f64)> {
    let (_, entry_odds, stake, commission_rate) = active_live_exchange_leg(view)?;
    let current_back_odds = active_current_back_odds(view)?;
    let locked_profit =
        lay_trade_out_locked_profit(entry_odds, stake, current_back_odds, commission_rate);
    let (other_wins, other_loses) = active_non_exchange_outcomes(view)?;
    Some((locked_profit + other_wins, locked_profit + other_loses))
}

fn active_fractional_cashout_outcomes(
    view: ActivePositionView<'_>,
    fraction: f64,
) -> Option<(f64, f64)> {
    let (_, entry_odds, lay_stake, commission_rate) = active_live_exchange_leg(view)?;
    let current_back_odds = active_current_back_odds(view)?;
    let hedge_stake = active_full_hedge_back_stake(view)? * fraction.clamp(0.0, 1.0);
    let exchange_win = settled_leg_pnl("lay", entry_odds, lay_stake, commission_rate, true)
        + settled_leg_pnl("back", current_back_odds, hedge_stake, 0.0, true);
    let exchange_lose = settled_leg_pnl("lay", entry_odds, lay_stake, commission_rate, false)
        + settled_leg_pnl("back", current_back_odds, hedge_stake, 0.0, false);
    let (other_wins, other_loses) = active_non_exchange_outcomes(view)?;
    Some((exchange_win + other_wins, exchange_lose + other_loses))
}

fn active_bookie_cashout_outcomes(view: ActivePositionView<'_>) -> Option<(f64, f64)> {
    let sportsbook_bet = view.sportsbook_bet?;
    let current_cashout_value = sportsbook_bet.current_cashout_value?;
    let book_locked_profit = current_cashout_value - sportsbook_bet.stake;
    let exchange_outcomes =
        if let Some((_, entry_odds, stake, commission_rate)) = active_live_exchange_leg(view) {
            (
                settled_leg_pnl("lay", entry_odds, stake, commission_rate, true),
                settled_leg_pnl("lay", entry_odds, stake, commission_rate, false),
            )
        } else {
            (0.0, 0.0)
        };
    Some((
        exchange_outcomes.0 + book_locked_profit,
        exchange_outcomes.1 + book_locked_profit,
    ))
}

fn active_total_cashout_outcomes(view: ActivePositionView<'_>) -> Option<(f64, f64)> {
    active_cashout_outcomes(view).or_else(|| active_bookie_cashout_outcomes(view))
}

fn active_full_hedge_back_stake(view: ActivePositionView<'_>) -> Option<f64> {
    let (_, entry_lay_odds, lay_stake, commission_rate) = active_live_exchange_leg(view)?;
    let current_back_odds = active_current_back_odds(view)?;
    let effective_commission = normalize_commission_rate(commission_rate);
    Some((lay_stake * (entry_lay_odds - effective_commission)) / current_back_odds)
}

fn active_non_exchange_outcomes(view: ActivePositionView<'_>) -> Option<(f64, f64)> {
    if let Some(tracked_bet) = view.tracked_bet {
        let mut win = 0.0;
        let mut lose = 0.0;
        let mut found = false;
        for leg in &tracked_bet.legs {
            if normalize_key(&leg.venue) == "smarkets" {
                continue;
            }
            found = true;
            win += settled_leg_from_tracked_leg(leg, true);
            lose += settled_leg_from_tracked_leg(leg, false);
        }
        if found {
            return Some((win, lose));
        }
    }

    let sportsbook_bet = view.sportsbook_bet?;
    Some((
        settled_leg_pnl("back", sportsbook_bet.odds, sportsbook_bet.stake, 0.0, true),
        settled_leg_pnl(
            "back",
            sportsbook_bet.odds,
            sportsbook_bet.stake,
            0.0,
            false,
        ),
    ))
}

fn active_live_exchange_leg(view: ActivePositionView<'_>) -> Option<(&'static str, f64, f64, f64)> {
    let open_position = view.open_position?;
    if let Some((venue, entry_odds, stake, commission_rate)) = active_tracked_exchange_leg(view) {
        return Some((venue, entry_odds, stake, commission_rate));
    }
    Some((
        "smarkets",
        open_position.price,
        open_position.stake,
        view.commission_rate,
    ))
}

fn active_tracked_exchange_leg(
    view: ActivePositionView<'_>,
) -> Option<(&'static str, f64, f64, f64)> {
    let tracked_bet = view.tracked_bet?;
    tracked_bet
        .legs
        .iter()
        .find(|leg| normalize_key(&leg.venue) == "smarkets" && normalize_key(&leg.side) == "lay")
        .map(|leg| {
            (
                "smarkets",
                leg.odds,
                leg.stake,
                leg.commission_rate.unwrap_or(view.commission_rate),
            )
        })
}

fn active_exchange_leg(view: ActivePositionView<'_>) -> Option<(&'static str, f64, f64, f64)> {
    active_live_exchange_leg(view).or_else(|| active_tracked_exchange_leg(view))
}

fn active_sportsbook_leg(view: ActivePositionView<'_>) -> Option<(String, String, f64, f64)> {
    if let Some(tracked_bet) = view.tracked_bet {
        if let Some(leg) = tracked_bet.legs.iter().find(|leg| {
            normalize_key(&leg.venue) != "smarkets" && normalize_key(&leg.side) == "back"
        }) {
            return Some((leg.venue.clone(), leg.outcome.clone(), leg.odds, leg.stake));
        }
    }

    view.sportsbook_bet.map(|sportsbook_bet| {
        (
            sportsbook_bet.venue.clone(),
            sportsbook_bet.label.clone(),
            sportsbook_bet.odds,
            sportsbook_bet.stake,
        )
    })
}

fn tracked_bet_matches_open_position(
    tracked_bet: &TrackedBetRow,
    open_position: &OpenPositionRow,
) -> bool {
    text_matches(&tracked_bet.selection, &open_position.contract)
        && market_matches(&tracked_bet.market, &open_position.market)
        && event_matches(&tracked_bet.event, &open_position.event)
}

fn sportsbook_bet_matches_open_position(
    sportsbook_bet: &OtherOpenBetRow,
    open_position: &OpenPositionRow,
) -> bool {
    text_matches(&sportsbook_bet.label, &open_position.contract)
        && market_matches(&sportsbook_bet.market, &open_position.market)
        && event_matches(&sportsbook_bet.event, &open_position.event)
}

fn sportsbook_bet_matches_tracked_bet(
    sportsbook_bet: &OtherOpenBetRow,
    tracked_bet: &TrackedBetRow,
) -> bool {
    text_matches(&sportsbook_bet.label, &tracked_bet.selection)
        && market_matches(&sportsbook_bet.market, &tracked_bet.market)
        && event_matches(&sportsbook_bet.event, &tracked_bet.event)
}

fn sportsbook_bet_identity(sportsbook_bet: &OtherOpenBetRow) -> String {
    format!(
        "{}|{}|{}|{}|{:.2}|{:.2}",
        sportsbook_bet.venue,
        sportsbook_bet.event,
        sportsbook_bet.label,
        sportsbook_bet.market,
        sportsbook_bet.odds,
        sportsbook_bet.stake
    )
}

fn tracked_bet_is_closed(tracked_bet: &TrackedBetRow) -> bool {
    if !tracked_bet.settled_at.is_empty() {
        return true;
    }
    matches!(
        normalize_key(&tracked_bet.status).as_str(),
        "settled" | "closed" | "cashedout" | "void" | "lost" | "won"
    )
}

fn settled_leg_from_tracked_leg(leg: &TrackedLeg, selection_wins: bool) -> f64 {
    settled_leg_pnl(
        &leg.side,
        leg.odds,
        leg.stake,
        leg.commission_rate.unwrap_or(0.0),
        selection_wins,
    )
}

fn settled_leg_pnl(
    side: &str,
    odds: f64,
    stake: f64,
    commission_rate: f64,
    selection_wins: bool,
) -> f64 {
    match normalize_key(side).as_str() {
        "back" => {
            if selection_wins {
                stake * (odds - 1.0)
            } else {
                -stake
            }
        }
        "lay" => {
            if selection_wins {
                -(stake * (odds - 1.0))
            } else {
                stake * (1.0 - normalize_commission_rate(commission_rate))
            }
        }
        _ => 0.0,
    }
}

fn lay_trade_out_locked_profit(
    entry_lay_odds: f64,
    lay_stake: f64,
    current_back_odds: f64,
    commission_rate: f64,
) -> f64 {
    let effective_commission = normalize_commission_rate(commission_rate);
    let hedge_back_stake =
        (lay_stake * (entry_lay_odds - effective_commission)) / current_back_odds;
    (lay_stake * (1.0 - effective_commission)) - hedge_back_stake
}

fn active_price_edge_label(view: ActivePositionView<'_>) -> String {
    match (
        active_live_exchange_leg(view).map(|(_, odds, _, _)| odds),
        active_current_back_odds(view),
        active_live_exchange_leg(view).map(|(_, odds, _, _)| implied_probability(odds)),
        active_current_probability(view),
    ) {
        (Some(entry_odds), Some(live_odds), Some(entry_prob), Some(live_prob)) => format!(
            "odds {:+.2} | prob {:+.2}pp",
            live_odds - entry_odds,
            (live_prob - entry_prob) * 100.0
        ),
        _ => String::from("-"),
    }
}

fn active_exit_edge_label(view: ActivePositionView<'_>) -> String {
    let hold_worst = active_hold_outcomes(view).map(|(win, lose)| win.min(lose));
    let half_worst = active_half_cashout_outcomes(view).map(|(win, lose)| win.min(lose));
    let lock_worst = active_total_cashout_outcomes(view).map(|(win, lose)| win.min(lose));
    match (hold_worst, half_worst, lock_worst) {
        (Some(hold), Some(half), Some(lock)) => {
            format!("half {:+.2} | lock {:+.2}", half - hold, lock - hold)
        }
        (Some(hold), None, Some(lock)) => format!("lock {:+.2}", lock - hold),
        _ => String::from("-"),
    }
}

fn active_half_exit_stake_label(view: ActivePositionView<'_>) -> String {
    match (
        active_full_hedge_back_stake(view),
        active_half_cashout_outcomes(view),
    ) {
        (Some(stake), Some((win, lose))) => {
            format!("stake {:.2} | {:+.2}/{:+.2}", stake / 2.0, win, lose)
        }
        _ => String::from("-"),
    }
}

fn active_full_exit_stake_label(view: ActivePositionView<'_>) -> String {
    match (
        active_full_hedge_back_stake(view),
        active_total_cashout_outcomes(view),
    ) {
        (Some(stake), Some((win, lose))) => {
            format!("stake {:.2} | {:+.2}/{:+.2}", stake, win, lose)
        }
        _ => String::from("-"),
    }
}

fn active_entry_ev_label(view: ActivePositionView<'_>) -> String {
    let Some(tracked_bet) = view.tracked_bet else {
        return String::from("-");
    };
    match (tracked_bet.expected_ev.gbp, tracked_bet.expected_ev.pct) {
        (Some(gbp), Some(pct)) => format!("{gbp:+.2} | {pct:+.2}%"),
        (Some(gbp), None) => format!("{gbp:+.2}"),
        (None, Some(pct)) => format!("{pct:+.2}%"),
        (None, None) => String::from("-"),
    }
}

fn active_historical_summary_label(
    snapshot: &ExchangePanelSnapshot,
    view: ActivePositionView<'_>,
) -> String {
    let selection = active_selection_label(view);
    let market = active_market_name(view);
    let matches = snapshot
        .historical_positions
        .iter()
        .filter(|row| {
            text_matches(&selection, &row.contract) && market_matches(&market, &row.market)
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return String::from("no matched history");
    }
    let realised = matches
        .iter()
        .filter(|row| row.overall_pnl_known)
        .map(|row| row.pnl_amount)
        .sum::<f64>();
    let wins = matches
        .iter()
        .filter(|row| row.overall_pnl_known && row.pnl_amount > 0.0)
        .count();
    format!("n {} | win {} | pnl {:+.2}", matches.len(), wins, realised)
}

fn active_sharp_quote_label(
    owls_dashboard: &OwlsDashboard,
    view: ActivePositionView<'_>,
) -> String {
    let event = active_event_label(view);
    let selection = active_selection_label(view);
    let preferred = [
        OwlsEndpointId::Realtime,
        OwlsEndpointId::Moneyline,
        OwlsEndpointId::Odds,
        OwlsEndpointId::Spreads,
        OwlsEndpointId::Totals,
    ];
    for endpoint_id in preferred {
        let Some(endpoint) = owls_dashboard
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == endpoint_id)
        else {
            continue;
        };
        if let Some(row) = endpoint.preview.iter().find(|row| {
            text_matches(&row.label, &event)
                && (text_matches(&row.metric, &selection)
                    || text_matches(&row.detail, "pinnacle")
                    || normalize_key(&selection) == "draw")
        }) {
            return compact_sharp_quote(row);
        }
        if let Some(row) = endpoint
            .preview
            .iter()
            .find(|row| text_matches(&row.label, &event))
        {
            return compact_sharp_quote(row);
        }
    }
    String::from("no Owls match")
}

fn compact_sharp_quote(row: &OwlsPreviewRow) -> String {
    format!(
        "{} | {}",
        truncate_text(&row.detail, 12),
        truncate_text(&row.metric, 18)
    )
}

fn exit_odds_for_target_profit(
    entry_lay_odds: f64,
    lay_stake: f64,
    commission_rate: f64,
    target_profit: f64,
) -> Option<f64> {
    let effective_commission = normalize_commission_rate(commission_rate);
    let denominator = (lay_stake * (1.0 - effective_commission)) - target_profit;
    if denominator <= 0.0 {
        None
    } else {
        Some((lay_stake * (entry_lay_odds - effective_commission)) / denominator)
    }
}

fn normalize_commission_rate(value: f64) -> f64 {
    if value > 1.0 {
        value / 100.0
    } else {
        value
    }
}

fn normalize_key(value: &str) -> String {
    value
        .to_lowercase()
        .replace("vs", "v")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn text_matches(left: &str, right: &str) -> bool {
    let left = normalize_key(left);
    let right = normalize_key(right);
    !left.is_empty()
        && !right.is_empty()
        && (left == right || left.contains(&right) || right.contains(&left))
}

fn market_matches(left: &str, right: &str) -> bool {
    let left = normalize_market(left);
    let right = normalize_market(right);
    !left.is_empty() && !right.is_empty() && left == right
}

fn normalize_market(value: &str) -> String {
    let normalized = normalize_key(value);
    if matches!(
        normalized.as_str(),
        "full time result" | "match odds" | "to win" | "winner"
    ) {
        return String::from("match odds");
    }
    normalized
}

fn event_matches(left: &str, right: &str) -> bool {
    if left.is_empty() || right.is_empty() {
        return true;
    }
    let left = normalize_key(left);
    let right = normalize_key(right);
    left == right
}

fn selected_active_rows(view: ActivePositionView<'_>) -> Vec<(&'static str, String, Color)> {
    let recommendation = derived_exit_recommendation(view)
        .map(|recommendation| format!("{} ({})", recommendation.action, recommendation.reason))
        .unwrap_or_else(|| active_action_label(view));
    let exposure = if let Some(open_position) = view.open_position {
        format!(
            "book {:.2} | lay {:.2} | liab {:.2}",
            view.sportsbook_bet.map(|bet| bet.stake).unwrap_or(0.0),
            open_position.stake,
            open_position.liability,
        )
    } else {
        format!(
            "book {:.2}",
            view.sportsbook_bet.map(|bet| bet.stake).unwrap_or(0.0)
        )
    };

    vec![
        ("󰕮 Pane", String::from("Active"), accent_cyan()),
        ("󰍹 Event", active_event_label(view), accent_blue()),
        ("󰃭 Date", active_date_label(view), accent_green()),
        ("󰥔 Time", active_time_label(view), accent_cyan()),
        ("󰆼 Position", active_position_label(view), accent_gold()),
        ("󰇈 Market", active_market_name(view), accent_blue()),
        (
            "󰈀 Entry Prob",
            active_entry_probability_label(view),
            accent_cyan(),
        ),
        (
            "󰐃 Book Cash",
            active_bookie_cashout_label(view),
            accent_green(),
        ),
        ("󱂬 Hold", active_hold_label(view), accent_green()),
        ("󰔟 Lock", active_lock_label(view), accent_green()),
        ("󰄬 Trigger", active_trigger_label(view), accent_cyan()),
        ("󰖌 Exposure", exposure, accent_pink()),
        ("󰌑 Status", active_status_label(view), accent_green()),
        ("󰘳 Action", recommendation, accent_gold()),
    ]
}

fn empty_selected_rows() -> Vec<(&'static str, String, Color)> {
    vec![
        ("󰕮 Pane", String::from("Active"), accent_cyan()),
        ("󰍹 Event", String::from("-"), muted_text()),
        ("󰃭 Date", String::from("-"), muted_text()),
        ("󰥔 Time", String::from("-"), muted_text()),
        (
            "󰆼 Position",
            String::from("No active position selected"),
            muted_text(),
        ),
        ("󰇈 Market", String::from("-"), muted_text()),
        ("󰈀 Entry Prob", String::from("-"), muted_text()),
        ("󰐃 Book Cash", String::from("-"), muted_text()),
        ("󱂬 Hold", String::from("-"), muted_text()),
        ("󰔟 Lock", String::from("-"), muted_text()),
        ("󰄬 Trigger", String::from("-"), muted_text()),
        ("󰖌 Exposure", String::from("-"), muted_text()),
        ("󰌑 Status", String::from("-"), muted_text()),
        ("󰘳 Action", String::from("-"), muted_text()),
    ]
}

fn active_position_reference_id(view: ActivePositionView<'_>) -> Option<&str> {
    view.tracked_bet
        .map(|tracked_bet| tracked_bet.bet_id.trim())
        .filter(|bet_id| !bet_id.is_empty())
}

fn selected_transport_marker<'a>(
    snapshot: &'a ExchangePanelSnapshot,
    view: ActivePositionView<'_>,
) -> Option<&'a TransportMarkerSummary> {
    let reference_id = active_position_reference_id(view)?;
    snapshot
        .transport_events
        .iter()
        .find(|event| event.reference_id == reference_id)
}

fn selected_recorder_event<'a>(
    snapshot: &'a ExchangePanelSnapshot,
    view: ActivePositionView<'_>,
    transport_event: Option<&TransportMarkerSummary>,
) -> Option<&'a RecorderEventSummary> {
    let reference_id = active_position_reference_id(view);
    let request_id = transport_event
        .map(|event| event.request_id.as_str())
        .filter(|value| !value.is_empty());
    snapshot.recorder_events.iter().find(|event| {
        if event.kind != "operator_interaction" {
            return false;
        }
        if let Some(reference_id) = reference_id {
            if event.reference_id == reference_id {
                return true;
            }
        }
        if let Some(request_id) = request_id {
            return event.request_id == request_id;
        }
        false
    })
}

fn compact_transport_label(event: &TransportMarkerSummary) -> String {
    let mut parts = vec![event.phase.clone(), event.action.clone()];
    if !event.request_id.is_empty() {
        parts.push(event.request_id.clone());
    } else if !event.reference_id.is_empty() {
        parts.push(event.reference_id.clone());
    }
    parts.join(" ")
}

fn compact_recorder_label(event: &RecorderEventSummary) -> String {
    let mut parts = Vec::new();
    if !event.action.is_empty() {
        parts.push(event.action.clone());
    } else if !event.kind.is_empty() {
        parts.push(event.kind.clone());
    }
    if !event.status.is_empty() {
        parts.push(event.status.clone());
    }
    if !event.request_id.is_empty() {
        parts.push(event.request_id.clone());
    } else if !event.reference_id.is_empty() {
        parts.push(event.reference_id.clone());
    }
    if parts.is_empty() {
        return String::from("none");
    }
    parts.join(" ")
}

fn interaction_action_code(action: &str) -> String {
    match action.trim() {
        "place_bet" => String::from("bet"),
        "cash_out" => String::from("cash"),
        value if value.is_empty() => String::from("-"),
        value => truncate_text(value, 4),
    }
}

fn interaction_state_code(status: &str, phase: &str) -> String {
    let normalized_status = status.trim().split(':').next_back().unwrap_or("").trim();
    match normalized_status {
        "requested" => String::from("req"),
        "submitted" => String::from("subm"),
        "not_implemented" => String::from("n/i"),
        "error" => String::from("err"),
        value if !value.is_empty() => truncate_text(value, 4),
        _ => match phase.trim() {
            "request" => String::from("req"),
            "response" => String::from("resp"),
            value if value.is_empty() => String::from("-"),
            value => truncate_text(value, 4),
        },
    }
}

fn active_interaction_label(
    snapshot: &ExchangePanelSnapshot,
    view: ActivePositionView<'_>,
) -> String {
    let transport_event = selected_transport_marker(snapshot, view);
    let recorder_event = selected_recorder_event(snapshot, view, transport_event);
    let action = recorder_event
        .map(|event| event.action.as_str())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            transport_event
                .map(|event| event.action.as_str())
                .filter(|value| !value.is_empty())
        });
    let phase = transport_event
        .map(|event| event.phase.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    let status = recorder_event
        .map(|event| event.status.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("");

    let Some(action) = action else {
        return String::from("-");
    };
    let action_code = interaction_action_code(action);
    let state_code = interaction_state_code(status, phase);
    if state_code == "-" {
        action_code
    } else {
        format!("{action_code} {state_code}")
    }
}

fn active_interaction_summary(snapshot: &ExchangePanelSnapshot) -> (usize, usize, usize) {
    let mut recent = 0;
    let mut pending = 0;
    let mut issues = 0;

    for view in active_position_views(snapshot) {
        let label = active_interaction_label(snapshot, view);
        if label == "-" {
            continue;
        }
        recent += 1;
        if label.ends_with(" req") {
            pending += 1;
        }
        if label.ends_with(" err") || label.ends_with(" n/i") {
            issues += 1;
        }
    }

    (recent, pending, issues)
}

fn active_interaction_cell(
    snapshot: &ExchangePanelSnapshot,
    view: ActivePositionView<'_>,
) -> Cell<'static> {
    let label = active_interaction_label(snapshot, view);
    let color = if label == "-" {
        muted_text()
    } else if label.ends_with(" err") || label.ends_with(" n/i") {
        accent_red()
    } else if label.ends_with(" req") {
        accent_gold()
    } else if label.ends_with(" subm") || label.ends_with(" resp") {
        accent_cyan()
    } else {
        accent_blue()
    };
    Cell::from(truncate_text(&label, 10)).style(Style::default().fg(color))
}

fn selected_position_interaction_lines(
    snapshot: &ExchangePanelSnapshot,
    selected_active: Option<ActivePositionView<'_>>,
) -> Vec<Line<'static>> {
    let Some(view) = selected_active else {
        return Vec::new();
    };
    let Some(reference_id) = active_position_reference_id(view) else {
        return vec![Line::raw(
            "󰐊 selected position has no tracked bet id, so no correlated interaction markers are available.",
        )];
    };
    let transport_event = selected_transport_marker(snapshot, view);
    let recorder_event = selected_recorder_event(snapshot, view, transport_event);
    let mut lines = vec![Line::raw(format!(
        "󰋼 selected ref {reference_id} • {}",
        active_selection_label(view)
    ))];

    if let Some(event) = transport_event {
        lines.push(Line::raw(format!(
            "󰐊 transport {}",
            compact_transport_label(event)
        )));
        if !event.detail.is_empty() {
            lines.push(Line::raw(format!("    {}", event.detail)));
        }
    } else {
        lines.push(Line::raw(
            "󰐊 transport no correlated request/response markers",
        ));
    }

    if let Some(event) = recorder_event {
        lines.push(Line::raw(format!(
            "󰛿 recorder {}",
            compact_recorder_label(event)
        )));
        if !event.detail.is_empty() {
            lines.push(Line::raw(format!("    {}", event.detail)));
        }
    } else {
        lines.push(Line::raw(
            "󰛿 recorder no correlated operator interaction event",
        ));
    }

    lines
}

#[cfg(test)]
fn exit_recommendation_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
    let recommendations = derived_exit_recommendations(snapshot);
    if recommendations.is_empty() {
        return vec![Line::raw("No exit recommendations are loaded.")];
    }

    let mut rows = vec![Line::raw(format!(
        "Target {:.2} | Stop {:.2} | Hard floor {} | Warn default {}",
        snapshot.exit_policy.target_profit,
        snapshot.exit_policy.stop_loss,
        snapshot
            .exit_policy
            .hard_margin_call_profit_floor
            .map(|value| format!("{:.2}", value))
            .unwrap_or_else(|| String::from("-")),
        snapshot.exit_policy.warn_only_default,
    ))];
    rows.push(Line::raw(
        "Press c in Trading > Positions to request the first actionable cash out.",
    ));

    for recommendation in recommendations.iter().take(6) {
        rows.push(Line::raw(format!(
            "{} | {} | worst {:.2}",
            recommendation.bet_id, recommendation.action, recommendation.worst_case_pnl
        )));
        rows.push(Line::raw(format!("reason {}", recommendation.reason)));
    }
    rows
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

#[cfg(test)]
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
        .map(|value| format!("{:.2}", value))
        .unwrap_or_else(|| String::from("-"))
}

fn render_table(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    widths: Vec<Constraint>,
    rows: Vec<Row<'static>>,
    empty: Row<'static>,
    table_state: Option<&mut TableState>,
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
    if let Some(table_state) = table_state {
        frame.render_stateful_widget(
            table
                .row_highlight_style(
                    Style::default()
                        .bg(Color::Rgb(28, 39, 52))
                        .fg(Color::Rgb(255, 255, 255))
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("● "),
            area,
            table_state,
        );
    } else {
        frame.render_widget(table, area);
    }
}

fn render_operator_log(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    selected_active: Option<ActivePositionView<'_>>,
    status_message: &str,
    positions_focus: PositionsFocus,
    status_scroll: u16,
) {
    let lines = vec![Line::from(vec![
        Span::styled("󱂬 ", Style::default().fg(accent_blue())),
        Span::raw(status_message.to_string()),
    ])]
    .into_iter()
    .chain(selected_position_interaction_lines(
        snapshot,
        selected_active,
    ))
    .chain(std::iter::once(Line::raw(format!(
        "󰕮 pane {}",
        positions_focus.label()
    ))))
    .collect::<Vec<_>>();
    let paragraph = Paragraph::new(lines)
        .block(section_block("󰌌 Operator Feed", accent_blue()))
        .scroll((status_scroll, 0))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_historical_view_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    selected_historical: Option<&OpenPositionRow>,
) {
    let popup = popup_area(area, 82, 72);
    frame.render_widget(Clear, popup);
    let block = section_block("󰋪 History View", accent_pink());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let Some(row) = selected_historical else {
        let empty = Paragraph::new("No historical position is selected.")
            .block(section_block("󰄬 Selected History", accent_blue()))
            .wrap(Wrap { trim: true });
        frame.render_widget(empty, inner);
        return;
    };

    let layout = Layout::vertical([Constraint::Length(9), Constraint::Min(8)]).split(inner);
    let body = Layout::horizontal([Constraint::Percentage(44), Constraint::Percentage(56)])
        .split(layout[1]);

    render_key_value_table(
        frame,
        layout[0],
        "󰄬 Historical Snapshot",
        vec![
            ("󰍹 Event", event_label(row), accent_blue()),
            ("󰃭 Date", event_date_label(row), accent_green()),
            ("󰥔 Time", event_time_label(row), accent_cyan()),
            ("󰆼 Position", position_label(row), accent_gold()),
            (
                "󰌑 Trade",
                format!("{} ({})", trade_label(row), trade_code(row)),
                trade_color(row),
            ),
            (
                "󱂬 P/L",
                historical_pnl_label(row),
                historical_pnl_style(row),
            ),
        ],
        Constraint::Length(12),
    );

    render_key_value_table(
        frame,
        body[0],
        "󰖌 Position Detail",
        vec![
            ("󰇈 Market", row.market.clone(), accent_blue()),
            ("󰄬 Score", score_label(row), accent_green()),
            ("󰅐 Phase", phase_label(row), accent_cyan()),
            (
                "󰔟 Prices",
                format!(
                    "buy {} | sell {}",
                    format_optional_back_odds(primary_market_buy_odds(row)),
                    format_optional_back_odds(row.current_sell_odds)
                ),
                accent_gold(),
            ),
            (
                "󰈀 Prob",
                format_optional_probability(primary_market_implied_probability(row)),
                accent_cyan(),
            ),
            (
                "󰐃 Exposure",
                format!(
                    "stake {:.2} | liab {:.2} | value {:.2}",
                    row.stake, row.liability, row.current_value
                ),
                accent_pink(),
            ),
        ],
        Constraint::Length(12),
    );

    let related = snapshot
        .historical_positions
        .iter()
        .filter(|candidate| {
            text_matches(&candidate.contract, &row.contract)
                && market_matches(&candidate.market, &row.market)
        })
        .take(10)
        .map(|candidate| {
            Line::raw(format!(
                "{} {} {} {}",
                truncate_text(&event_label(candidate), 28),
                event_date_label(candidate),
                trade_code(candidate),
                historical_pnl_label(candidate)
            ))
        })
        .collect::<Vec<_>>();

    let history_lines = if related.is_empty() {
        vec![Line::raw("No comparable historical rows.")]
    } else {
        related
    };
    let history = Paragraph::new(history_lines)
        .block(section_block("󰋪 Comparable History", accent_blue()))
        .wrap(Wrap { trim: true });
    frame.render_widget(history, body[1]);
}

fn render_live_view_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    selected_active: Option<ActivePositionView<'_>>,
) {
    let popup = popup_area(area, 90, 82);
    frame.render_widget(Clear, popup);
    let block = section_block("󰕮 Live View", accent_cyan());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let Some(view) = selected_active else {
        let empty = Paragraph::new("No active position is selected.")
            .block(section_block("󰄬 Selected Position", accent_blue()))
            .wrap(Wrap { trim: true });
        frame.render_widget(empty, inner);
        return;
    };

    let layout = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(14),
        Constraint::Min(12),
    ])
    .split(inner);
    let middle = Layout::horizontal([
        Constraint::Percentage(38),
        Constraint::Percentage(30),
        Constraint::Percentage(32),
    ])
    .split(layout[1]);
    let bottom = Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(layout[2]);
    let selected_transport = selected_transport_marker(snapshot, view);
    let selected_recorder = selected_recorder_event(snapshot, view, selected_transport);

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                active_event_label(view),
                Style::default()
                    .fg(accent_blue())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(active_market_name(view), Style::default().fg(accent_gold())),
        ]),
        Line::from(vec![
            Span::styled("Hold ", Style::default().fg(muted_text())),
            Span::styled(active_hold_label(view), Style::default().fg(accent_green())),
            Span::raw("   "),
            Span::styled("Lock ", Style::default().fg(muted_text())),
            Span::styled(active_lock_label(view), Style::default().fg(accent_green())),
            Span::raw("   "),
            Span::styled("Half ", Style::default().fg(muted_text())),
            Span::styled(
                active_half_exit_label(view),
                Style::default().fg(accent_pink()),
            ),
            Span::raw("   "),
            Span::styled("Book cash ", Style::default().fg(muted_text())),
            Span::styled(
                active_bookie_cashout_label(view),
                Style::default().fg(accent_cyan()),
            ),
            Span::raw("   "),
            Span::styled("Action ", Style::default().fg(muted_text())),
            Span::styled(
                active_action_label(view),
                Style::default().fg(accent_gold()),
            ),
        ]),
    ])
    .block(section_block("󰘵 Market State", accent_blue()))
    .wrap(Wrap { trim: true });
    frame.render_widget(header, layout[0]);

    render_live_odds_chart(frame, middle[0], view);
    render_key_value_table(
        frame,
        middle[1],
        "󰄬 Position Detail",
        vec![
            ("󰆼 Position", active_position_label(view), accent_gold()),
            ("󰇈 Market", active_market_name(view), accent_blue()),
            (
                "󰈀 Prob",
                format!(
                    "entry {} | live {}",
                    active_entry_probability_label(view),
                    active_probability_label(view)
                ),
                accent_cyan(),
            ),
            (
                "󰐃 Cash / Lay",
                format!(
                    "{} | {}",
                    active_bookie_cashout_label(view),
                    active_live_odds_label(view)
                ),
                accent_green(),
            ),
            (
                "󰌑 Flow",
                format!(
                    "{} | {}",
                    active_status_label(view),
                    active_trigger_label(view)
                ),
                accent_green(),
            ),
            ("󰖌 Exposure", active_exposure_label(view), accent_pink()),
        ],
        Constraint::Length(12),
    );
    render_key_value_table(
        frame,
        middle[2],
        "󰆑 Opportunity Lens",
        vec![
            ("󰈀 Price Edge", active_price_edge_label(view), accent_cyan()),
            ("󰔟 Exit Edge", active_exit_edge_label(view), accent_green()),
            (
                "󰐃 Half Exit",
                active_half_exit_stake_label(view),
                accent_gold(),
            ),
            (
                "󰔠 Full Exit",
                active_full_exit_stake_label(view),
                accent_gold(),
            ),
            ("󰖟 Entry EV", active_entry_ev_label(view), accent_pink()),
            (
                "󰋪 History",
                active_historical_summary_label(snapshot, view),
                accent_blue(),
            ),
        ],
        Constraint::Length(12),
    );

    render_live_decision_matrix(frame, bottom[0], view);
    render_live_execution_feed(
        frame,
        bottom[1],
        snapshot,
        view,
        selected_transport,
        selected_recorder,
    );
}

fn render_live_odds_chart(frame: &mut Frame<'_>, area: Rect, view: ActivePositionView<'_>) {
    let points = live_odds_points(view);
    let (min_y, max_y) = chart_bounds(&points);
    let datasets = vec![Dataset::default()
        .name("odds")
        .graph_type(GraphType::Line)
        .style(Style::default().fg(accent_cyan()))
        .data(&points)];
    let chart = Chart::new(datasets)
        .block(section_block("󰑭 Odds Ladder", accent_blue()))
        .x_axis(Axis::default().bounds([0.0, 3.0]).labels(vec![
            Line::raw("Entry"),
            Line::raw("Live"),
            Line::raw("Target"),
            Line::raw("Stop"),
        ]))
        .y_axis(Axis::default().bounds([min_y, max_y]).labels(vec![
            Line::raw(format!("{min_y:.2}")),
            Line::raw(format!("{max_y:.2}")),
        ]));
    frame.render_widget(chart, area);
}

fn render_live_decision_matrix(frame: &mut Frame<'_>, area: Rect, view: ActivePositionView<'_>) {
    let hold = active_hold_outcomes(view);
    let half = active_half_cashout_outcomes(view);
    let lock = active_total_cashout_outcomes(view);
    let rows = vec![
        Row::new(vec![
            Cell::from("Selection wins"),
            Cell::from(
                hold.map(|(win, _)| format!("{win:+.2}"))
                    .unwrap_or_else(|| String::from("-")),
            ),
            Cell::from(
                half.map(|(win, _)| format!("{win:+.2}"))
                    .unwrap_or_else(|| String::from("-")),
            ),
            Cell::from(
                lock.map(|(win, _)| format!("{win:+.2}"))
                    .unwrap_or_else(|| String::from("-")),
            ),
        ]),
        Row::new(vec![
            Cell::from("Selection loses"),
            Cell::from(
                hold.map(|(_, lose)| format!("{lose:+.2}"))
                    .unwrap_or_else(|| String::from("-")),
            ),
            Cell::from(
                half.map(|(_, lose)| format!("{lose:+.2}"))
                    .unwrap_or_else(|| String::from("-")),
            ),
            Cell::from(
                lock.map(|(_, lose)| format!("{lose:+.2}"))
                    .unwrap_or_else(|| String::from("-")),
            ),
        ]),
        Row::new(vec![
            Cell::from("Worst case"),
            Cell::from(
                hold.map(|(win, lose)| format!("{:+.2}", win.min(lose)))
                    .unwrap_or_else(|| String::from("-")),
            ),
            Cell::from(
                half.map(|(win, lose)| format!("{:+.2}", win.min(lose)))
                    .unwrap_or_else(|| String::from("-")),
            ),
            Cell::from(
                lock.map(|(win, lose)| format!("{:+.2}", win.min(lose)))
                    .unwrap_or_else(|| String::from("-")),
            ),
        ]),
    ];
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(28),
            Constraint::Percentage(24),
            Constraint::Percentage(24),
            Constraint::Percentage(24),
        ],
    )
    .header(
        Row::new(vec!["Scenario", "Hold", "Half", "Lock"])
            .style(
                Style::default()
                    .fg(Color::Black)
                    .bg(accent_cyan())
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(1),
    )
    .block(section_block("󰄵 Decision Matrix", accent_blue()))
    .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_live_execution_feed(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ExchangePanelSnapshot,
    view: ActivePositionView<'_>,
    selected_transport: Option<&TransportMarkerSummary>,
    selected_recorder: Option<&RecorderEventSummary>,
) {
    let mut lines = vec![Line::raw(format!(
        "transport {}",
        selected_transport
            .map(compact_transport_label)
            .unwrap_or_else(|| String::from("no correlated marker"))
    ))];
    if let Some(event) = selected_transport.filter(|event| !event.detail.is_empty()) {
        lines.push(Line::raw(truncate_text(&event.detail, 56)));
    }
    lines.push(Line::raw(format!(
        "recorder {}",
        selected_recorder
            .map(compact_recorder_label)
            .unwrap_or_else(|| String::from("no correlated event"))
    )));
    if let Some(event) = selected_recorder.filter(|event| !event.detail.is_empty()) {
        lines.push(Line::raw(truncate_text(&event.detail, 56)));
    }
    lines.push(Line::raw(String::new()));
    lines.extend(selected_position_interaction_lines(snapshot, Some(view)));

    let paragraph = Paragraph::new(lines)
        .block(section_block("󰐊 Execution Trail", accent_blue()))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn live_odds_points(view: ActivePositionView<'_>) -> Vec<(f64, f64)> {
    let entry = active_exchange_leg(view)
        .map(|(_, odds, _, _)| odds)
        .unwrap_or(0.0);
    let live = active_current_back_odds(view).unwrap_or(entry);
    let target = active_profit_target_odds(view).unwrap_or(live);
    let stop = active_stop_loss_odds(view).unwrap_or(live);
    vec![(0.0, entry), (1.0, live), (2.0, target), (3.0, stop)]
}

fn chart_bounds(points: &[(f64, f64)]) -> (f64, f64) {
    let mut min_y = points
        .iter()
        .map(|(_, y)| *y)
        .filter(|value| *value > 0.0)
        .fold(f64::INFINITY, f64::min);
    let mut max_y = points
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max);
    if !min_y.is_finite() || !max_y.is_finite() || (max_y - min_y).abs() < f64::EPSILON {
        min_y = 0.0;
        max_y = 1.0;
    } else {
        let padding = ((max_y - min_y) * 0.12).max(0.1);
        min_y = (min_y - padding).max(0.0);
        max_y += padding;
    }
    (min_y, max_y)
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

fn position_section_heights(snapshot: &ExchangePanelSnapshot, available_height: u16) -> (u16, u16) {
    let minimum = 7;
    if available_height <= minimum * 2 {
        let active_height = available_height.saturating_div(2).max(6);
        return (
            active_height,
            available_height.saturating_sub(active_height),
        );
    }

    let active_rows = active_position_row_count(snapshot).max(1) as u16;
    let historical_rows = snapshot.historical_positions.len().max(1) as u16;
    let total_rows = active_rows + historical_rows;
    let distributable = available_height.saturating_sub(minimum * 2);
    let active_extra =
        ((distributable as u32 * active_rows as u32) / total_rows.max(1) as u32) as u16;
    let mut active_height = minimum + active_extra;
    let max_active_height = available_height.saturating_sub(minimum);
    active_height = active_height.clamp(minimum, max_active_height.max(minimum));
    let historical_height = available_height.saturating_sub(active_height);
    (active_height, historical_height.max(minimum))
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
        .highlight_symbol("● ");
    frame.render_stateful_widget(table, area, table_state);
}

fn table_header(title: &str) -> Vec<&'static str> {
    match title {
        heading if heading.contains("Active Positions") => {
            vec![
                "Event", "Position", "Date", "Time", "Hold", "Lock", "A", "I/O", "Prob", "Trigger",
            ]
        }
        heading if heading.contains("Historical Positions") => {
            vec![
                "Event", "Position", "Date", "Time", "Score", "Phase", "T", "PnL", "Market",
            ]
        }
        heading if heading.contains("Exit Recommendations") => {
            vec!["Bet", "Action", "Reason", "Worst", "Venue"]
        }
        heading if heading.contains("Watch Plan") => {
            vec!["Contract", "Legs", "Live", "Profit", "Stop", "Worst"]
        }
        heading if heading.contains("Tracked Bets") => {
            vec!["Bet", "Selection", "Market", "Status", "Fund", "Venues"]
        }
        heading if heading.contains("Other Open Bets") => {
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

fn historical_pnl_cell(row: &OpenPositionRow) -> Cell<'static> {
    if !row.overall_pnl_known {
        return Cell::from("-").style(Style::default().fg(muted_text()));
    }
    pnl_cell(row.pnl_amount)
}

fn historical_pnl_label(row: &OpenPositionRow) -> String {
    if row.overall_pnl_known {
        format!("{:+.2}", row.pnl_amount)
    } else {
        String::from("-")
    }
}

fn historical_pnl_style(row: &OpenPositionRow) -> Color {
    if row.overall_pnl_known {
        pnl_color(row.pnl_amount)
    } else {
        muted_text()
    }
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
        .map(|odds| format!("{:.2}", odds))
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

fn event_date_label(row: &crate::domain::OpenPositionRow) -> String {
    if let Some((date, _)) = iso_date_time(row) {
        return date;
    }
    if let Some((date, _)) = url_date_time(row) {
        return date;
    }
    String::from("-")
}

fn event_time_label(row: &crate::domain::OpenPositionRow) -> String {
    if let Some((_, time)) = iso_date_time(row) {
        return time;
    }
    if let Some((_, time)) = url_date_time(row) {
        return time;
    }
    if let Some(time) = event_status_clock(row) {
        return time;
    }
    if looks_like_time(&row.live_clock) {
        return row.live_clock.clone();
    }
    String::from("-")
}

fn event_table_label(row: &crate::domain::OpenPositionRow) -> String {
    truncate_text(&event_label(row), 28)
}

fn position_table_label(row: &crate::domain::OpenPositionRow) -> String {
    truncate_text(&position_label(row), 34)
}

fn iso_date_time(row: &crate::domain::OpenPositionRow) -> Option<(String, String)> {
    [
        row.event_status.split('|').next().unwrap_or(""),
        row.live_clock.as_str(),
    ]
    .into_iter()
    .find_map(parse_isoish_datetime)
}

fn url_date_time(row: &crate::domain::OpenPositionRow) -> Option<(String, String)> {
    let segments = row
        .event_url
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    for window in segments.windows(4) {
        let [year, month, day, time] = window else {
            continue;
        };
        if year.len() == 4
            && month.len() == 2
            && day.len() == 2
            && year.chars().all(|c| c.is_ascii_digit())
            && month.chars().all(|c| c.is_ascii_digit())
            && day.chars().all(|c| c.is_ascii_digit())
            && time.len() >= 5
        {
            let bytes = time.as_bytes();
            if bytes.get(2) == Some(&b'-')
                && bytes[0..2].iter().all(|c| c.is_ascii_digit())
                && bytes[3..5].iter().all(|c| c.is_ascii_digit())
            {
                return Some((
                    format!("{year}-{month}-{day}"),
                    format!("{}:{}", &time[0..2], &time[3..5]),
                ));
            }
        }
    }
    None
}

fn parse_isoish_datetime(value: &str) -> Option<(String, String)> {
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
    let date = trimmed.get(0..10)?.to_string();
    let time = trimmed.get(11..16)?.to_string();
    Some((date, time))
}

fn event_status_clock(row: &crate::domain::OpenPositionRow) -> Option<String> {
    row.event_status
        .split('|')
        .nth(1)
        .map(str::trim)
        .filter(|value| looks_like_time(value))
        .map(ToOwned::to_owned)
}

fn looks_like_time(value: &str) -> bool {
    value.len() == 5
        && value.as_bytes().get(2) == Some(&b':')
        && value.as_bytes()[0..2].iter().all(|c| c.is_ascii_digit())
        && value.as_bytes()[3..5].iter().all(|c| c.is_ascii_digit())
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

fn tracked_bet_funding_counts(snapshot: &ExchangePanelSnapshot) -> (usize, usize, usize) {
    if snapshot.tracked_bets.is_empty() && snapshot.ledger_pnl_summary.settled_count > 0 {
        return (
            snapshot.ledger_pnl_summary.standard_count,
            snapshot.ledger_pnl_summary.promo_count,
            snapshot.ledger_pnl_summary.unknown_count,
        );
    }

    let mut standard = 0;
    let mut promo = 0;
    let mut unknown = 0;
    for tracked_bet in &snapshot.tracked_bets {
        match tracked_bet_funding_label(tracked_bet) {
            "Promo" => promo += 1,
            "Std" => standard += 1,
            _ => unknown += 1,
        }
    }
    (standard, promo, unknown)
}

fn positions_pnl_summary(snapshot: &ExchangePanelSnapshot) -> (f64, f64, f64, f64) {
    let live_pnl = snapshot
        .open_positions
        .iter()
        .map(|row| row.pnl_amount)
        .sum::<f64>();
    let realised_pnl = if snapshot.ledger_pnl_summary.settled_count > 0 {
        snapshot.ledger_pnl_summary.realised_total
    } else if snapshot.tracked_bets.is_empty() {
        snapshot
            .historical_positions
            .iter()
            .filter(|row| row.overall_pnl_known)
            .map(|row| row.pnl_amount)
            .sum::<f64>()
    } else {
        snapshot
            .tracked_bets
            .iter()
            .filter_map(|tracked_bet| tracked_bet.realised_pnl_gbp)
            .sum::<f64>()
    };
    let promo_pnl =
        if snapshot.tracked_bets.is_empty() && snapshot.ledger_pnl_summary.settled_count > 0 {
            snapshot.ledger_pnl_summary.promo_total
        } else {
            snapshot
                .tracked_bets
                .iter()
                .filter(|tracked_bet| tracked_bet_funding_label(tracked_bet) == "Promo")
                .filter_map(|tracked_bet| tracked_bet.realised_pnl_gbp)
                .sum::<f64>()
        };
    (realised_pnl, live_pnl, realised_pnl + live_pnl, promo_pnl)
}

fn tracked_bet_funding_label(tracked_bet: &crate::domain::TrackedBetRow) -> &'static str {
    let funding_kind = tracked_bet.funding_kind.trim().to_ascii_lowercase();
    if matches!(funding_kind.as_str(), "free_bet" | "risk_free" | "bonus") {
        return "Promo";
    }

    let notes = tracked_bet.notes.to_lowercase();
    let bet_type = tracked_bet.bet_type.to_lowercase();
    let status = tracked_bet.status.to_lowercase();
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
        return "Promo";
    }

    if funding_kind == "cash" {
        return "Std";
    }

    if ["qualifying", "cash", "normal"]
        .iter()
        .any(|keyword| haystack.contains(keyword))
    {
        return "Std";
    }

    "Unknown"
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
        .padding(Padding::horizontal(1))
        .style(
            Style::default()
                .bg(Color::Rgb(16, 22, 30))
                .fg(Color::Rgb(234, 240, 246)),
        )
        .border_style(Style::default().fg(Color::Rgb(74, 88, 104)))
}

fn positions_table_title(label: &str, count: usize, focused: bool) -> String {
    let marker = if focused { "●" } else { "◦" };
    format!("{marker} {label} ({count})")
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
        RecorderEventSummary, TrackedBetRow, TrackedLeg, TransportMarkerSummary, ValueMetric,
        VenueId, VenueStatus, VenueSummary, WatchSnapshot, WorkerStatus, WorkerSummary,
    };

    use super::{
        active_interaction_label, active_interaction_summary, active_position_rows,
        active_position_views, exit_recommendation_lines, historical_position_rows,
        open_position_lines, positions_pnl_summary, selected_position_interaction_lines,
        summary_lines, tracked_bet_funding_counts, tracked_bet_funding_label, tracked_bet_lines,
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
            ledger_pnl_summary: Default::default(),
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
            recorder_bundle: None,
            recorder_events: Vec::new(),
            transport_summary: None,
            transport_events: Vec::new(),
            tracked_bets: Vec::new(),
            exit_policy: Default::default(),
            exit_recommendations: Vec::new(),
            horse_matcher: None,
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
        assert!(rendered.contains("I/O recent 0 | pending 0 | issues 0"));
    }

    #[test]
    fn selected_position_interaction_lines_show_correlated_transport_and_recorder_events() {
        let mut snapshot = sample_snapshot();
        snapshot.transport_events = vec![TransportMarkerSummary {
            captured_at: String::from("2026-03-20T12:00:01Z"),
            kind: String::from("interaction_marker"),
            action: String::from("place_bet"),
            phase: String::from("response"),
            request_id: String::from("req-7"),
            reference_id: String::from("bet-001"),
            summary: String::from("response place_bet req-7 bet-001"),
            detail: String::from("loaded in review mode"),
        }];
        snapshot.recorder_events = vec![RecorderEventSummary {
            captured_at: String::from("2026-03-20T12:00:01Z"),
            kind: String::from("operator_interaction"),
            source: String::from("operator_console"),
            page: String::from("worker_request"),
            action: String::from("place_bet"),
            status: String::from("response:submitted"),
            request_id: String::from("req-7"),
            reference_id: String::from("bet-001"),
            summary: String::from("place_bet bet-001 -> response:submitted"),
            detail: String::from("loaded in review mode"),
        }];

        let active_view = active_position_views(&snapshot)
            .into_iter()
            .next()
            .expect("active position");
        let rendered = selected_position_interaction_lines(&snapshot, Some(active_view))
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("selected ref bet-001"));
        assert!(rendered.contains("transport response place_bet req-7"));
        assert!(rendered.contains("recorder place_bet response:submitted req-7"));
        assert!(rendered.contains("loaded in review mode"));
    }

    #[test]
    fn active_position_rows_include_compact_interaction_state() {
        let mut snapshot = sample_snapshot();
        snapshot.transport_events = vec![TransportMarkerSummary {
            captured_at: String::from("2026-03-20T12:00:01Z"),
            kind: String::from("interaction_marker"),
            action: String::from("place_bet"),
            phase: String::from("response"),
            request_id: String::from("req-7"),
            reference_id: String::from("bet-001"),
            summary: String::from("response place_bet req-7 bet-001"),
            detail: String::from("loaded in review mode"),
        }];
        snapshot.recorder_events = vec![RecorderEventSummary {
            captured_at: String::from("2026-03-20T12:00:01Z"),
            kind: String::from("operator_interaction"),
            source: String::from("operator_console"),
            page: String::from("worker_request"),
            action: String::from("place_bet"),
            status: String::from("response:submitted"),
            request_id: String::from("req-7"),
            reference_id: String::from("bet-001"),
            summary: String::from("place_bet bet-001 -> response:submitted"),
            detail: String::from("loaded in review mode"),
        }];

        let active_view = active_position_views(&snapshot)
            .into_iter()
            .next()
            .expect("active position");
        assert_eq!(active_interaction_label(&snapshot, active_view), "bet subm");

        let rows = active_position_rows(&snapshot, &active_position_views(&snapshot));
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn active_interaction_summary_counts_pending_and_issue_rows() {
        let mut snapshot = sample_snapshot();
        snapshot.transport_events = vec![
            TransportMarkerSummary {
                captured_at: String::from("2026-03-20T12:00:01Z"),
                kind: String::from("interaction_marker"),
                action: String::from("place_bet"),
                phase: String::from("request"),
                request_id: String::from("req-7"),
                reference_id: String::from("bet-001"),
                summary: String::from("request place_bet req-7 bet-001"),
                detail: String::from("review buy"),
            },
            TransportMarkerSummary {
                captured_at: String::from("2026-03-20T12:00:02Z"),
                kind: String::from("interaction_marker"),
                action: String::from("cash_out"),
                phase: String::from("response"),
                request_id: String::new(),
                reference_id: String::from("bet-002"),
                summary: String::from("response cash_out bet-002"),
                detail: String::from("not implemented"),
            },
        ];
        snapshot.recorder_events = vec![
            RecorderEventSummary {
                captured_at: String::from("2026-03-20T12:00:01Z"),
                kind: String::from("operator_interaction"),
                source: String::from("operator_console"),
                page: String::from("worker_request"),
                action: String::from("place_bet"),
                status: String::from("request:requested"),
                request_id: String::from("req-7"),
                reference_id: String::from("bet-001"),
                summary: String::from("place_bet bet-001 -> request:requested"),
                detail: String::from("review buy"),
            },
            RecorderEventSummary {
                captured_at: String::from("2026-03-20T12:00:02Z"),
                kind: String::from("operator_interaction"),
                source: String::from("operator_console"),
                page: String::from("worker_request"),
                action: String::from("cash_out"),
                status: String::from("response:not_implemented"),
                request_id: String::new(),
                reference_id: String::from("bet-002"),
                summary: String::from("cash_out bet-002 -> response:not_implemented"),
                detail: String::from("not implemented"),
            },
        ];
        snapshot.tracked_bets.push(TrackedBetRow {
            bet_id: String::from("bet-002"),
            group_id: String::from("group-arsenal-everton-2"),
            event: String::from("Arsenal v Everton"),
            market: String::from("Full-time result"),
            selection: String::from("Arsenal"),
            status: String::from("open"),
            platform: String::from("bet365"),
            legs: vec![TrackedLeg {
                venue: String::from("bet365"),
                outcome: String::from("Arsenal"),
                side: String::from("back"),
                odds: 3.10,
                stake: 10.0,
                status: String::from("open"),
                ..TrackedLeg::default()
            }],
            ..TrackedBetRow::default()
        });

        let (recent, pending, issues) = active_interaction_summary(&snapshot);

        assert_eq!((recent, pending, issues), (2, 1, 1));
    }

    #[test]
    fn positions_pnl_summary_prefers_tracked_realised_when_available() {
        let mut snapshot = sample_snapshot();
        snapshot.open_positions = vec![OpenPositionRow {
            pnl_amount: -1.6,
            overall_pnl_known: true,
            ..sample_open_row()
        }];
        snapshot.historical_positions = vec![
            OpenPositionRow {
                pnl_amount: 4.0,
                overall_pnl_known: true,
                live_clock: String::from("2026-03-01T10:00:00"),
                ..sample_open_row()
            },
            OpenPositionRow {
                pnl_amount: -0.5,
                overall_pnl_known: true,
                live_clock: String::from("2026-03-02T10:00:00"),
                ..sample_open_row()
            },
        ];
        snapshot.tracked_bets[0].realised_pnl_gbp = Some(3.4);
        snapshot.tracked_bets[0].notes = String::from("Free Bet SNR");

        let (realised, live, net, promo) = positions_pnl_summary(&snapshot);

        assert_eq!(realised, 3.4);
        assert_eq!(live, -1.6);
        assert!((net - 1.8).abs() < 1e-9);
        assert_eq!(promo, 3.4);
    }

    #[test]
    fn positions_pnl_summary_falls_back_to_historical_when_tracked_bets_are_missing() {
        let mut snapshot = sample_snapshot();
        snapshot.tracked_bets.clear();
        snapshot.open_positions = vec![OpenPositionRow {
            pnl_amount: -1.6,
            overall_pnl_known: true,
            ..sample_open_row()
        }];
        snapshot.historical_positions = vec![
            OpenPositionRow {
                pnl_amount: 4.0,
                overall_pnl_known: true,
                live_clock: String::from("2026-03-01T10:00:00"),
                ..sample_open_row()
            },
            OpenPositionRow {
                pnl_amount: -0.5,
                overall_pnl_known: true,
                live_clock: String::from("2026-03-02T10:00:00"),
                ..sample_open_row()
            },
        ];

        let (realised, live, net, promo) = positions_pnl_summary(&snapshot);

        assert_eq!(realised, 3.5);
        assert_eq!(live, -1.6);
        assert!((net - 1.9).abs() < 1e-9);
        assert_eq!(promo, 0.0);
    }

    #[test]
    fn positions_pnl_summary_ignores_historical_rows_without_overall_pnl() {
        let mut snapshot = sample_snapshot();
        snapshot.tracked_bets.clear();
        snapshot.open_positions = vec![OpenPositionRow {
            pnl_amount: -1.6,
            overall_pnl_known: true,
            ..sample_open_row()
        }];
        snapshot.historical_positions = vec![
            OpenPositionRow {
                pnl_amount: 4.0,
                overall_pnl_known: true,
                live_clock: String::from("2026-03-01T10:00:00"),
                ..sample_open_row()
            },
            OpenPositionRow {
                pnl_amount: 7.03,
                overall_pnl_known: false,
                live_clock: String::from("2026-03-02T10:00:00"),
                ..sample_open_row()
            },
        ];

        let (realised, live, net, promo) = positions_pnl_summary(&snapshot);

        assert_eq!(realised, 4.0);
        assert_eq!(live, -1.6);
        assert!((net - 2.4).abs() < 1e-9);
        assert_eq!(promo, 0.0);
    }

    #[test]
    fn positions_pnl_summary_prefers_ledger_totals_when_available() {
        let mut snapshot = sample_snapshot();
        snapshot.tracked_bets.clear();
        snapshot.open_positions = vec![OpenPositionRow {
            pnl_amount: -1.6,
            overall_pnl_known: true,
            ..sample_open_row()
        }];
        snapshot.historical_positions = vec![OpenPositionRow {
            pnl_amount: 4.0,
            overall_pnl_known: true,
            live_clock: String::from("2026-03-01T10:00:00"),
            ..sample_open_row()
        }];
        snapshot.ledger_pnl_summary.realised_total = 12.5;
        snapshot.ledger_pnl_summary.promo_total = 4.5;
        snapshot.ledger_pnl_summary.settled_count = 3;
        snapshot.ledger_pnl_summary.standard_count = 2;
        snapshot.ledger_pnl_summary.promo_count = 1;

        let (realised, live, net, promo) = positions_pnl_summary(&snapshot);

        assert_eq!(realised, 12.5);
        assert_eq!(live, -1.6);
        assert!((net - 10.9).abs() < 1e-9);
        assert_eq!(promo, 4.5);
        assert_eq!(tracked_bet_funding_counts(&snapshot), (2, 1, 0));
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
    fn tracked_bet_funding_detects_promotional_notes() {
        let mut snapshot = sample_snapshot();
        snapshot.tracked_bets[0].notes = String::from("Free Bet SNR");

        assert_eq!(
            tracked_bet_funding_label(&snapshot.tracked_bets[0]),
            "Promo"
        );
    }

    #[test]
    fn open_position_lines_show_score_and_market_probabilities() {
        let mut snapshot = sample_snapshot();
        snapshot.tracked_bets.clear();
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
            overall_pnl_known: true,
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

        assert!(rendered.contains("West Ham vs Man City | Man City · book - ↔ smarkets 10.00@2.40"));
        assert!(rendered.contains("hold -14.00/+10.00 | lock - | action hold"));
        assert!(rendered
            .contains("status book - | lay Order filled | live 1.91 | live 1.91 | tgt - | stop -"));
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
            overall_pnl_known: true,
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
    fn historical_position_rows_do_not_cap_the_history_list() {
        let mut snapshot = sample_snapshot();
        snapshot.historical_positions = (0..12)
            .map(|index| OpenPositionRow {
                event: format!("Event {index}"),
                event_status: String::from("Settled"),
                event_url: String::new(),
                contract: format!("Selection {index}"),
                market: String::from("Full-time result"),
                status: String::from("settled"),
                market_status: String::from("settled"),
                is_in_play: false,
                price: 2.0,
                stake: 5.0,
                liability: 5.0,
                current_value: 0.0,
                pnl_amount: index as f64,
                overall_pnl_known: true,
                current_back_odds: Some(2.0),
                current_implied_probability: Some(0.5),
                current_implied_percentage: Some(50.0),
                current_buy_odds: Some(2.0),
                current_buy_implied_probability: Some(0.5),
                current_sell_odds: None,
                current_sell_implied_probability: None,
                current_score: String::new(),
                current_score_home: None,
                current_score_away: None,
                live_clock: format!("2026-03-18T12:{index:02}:00Z"),
                can_trade_out: false,
            })
            .collect();

        let rows = historical_position_rows(&snapshot);

        assert_eq!(rows.len(), 12);
    }

    #[test]
    fn historical_position_rows_render_dash_when_overall_pnl_is_unknown() {
        let mut snapshot = sample_snapshot();
        snapshot.historical_positions = vec![OpenPositionRow {
            event: String::from("Tottenham vs Atletico Madrid"),
            event_status: String::from("Event ended|UEFA Champions League"),
            event_url: String::new(),
            contract: String::from("Tottenham"),
            market: String::from("Full-time result"),
            status: String::from("Won"),
            market_status: String::from("settled"),
            is_in_play: false,
            price: 1.40,
            stake: 17.57,
            liability: 17.57,
            current_value: 24.60,
            pnl_amount: 7.03,
            overall_pnl_known: false,
            current_back_odds: Some(1.40),
            current_implied_probability: Some(1.0 / 1.40),
            current_implied_percentage: Some(100.0 / 1.40),
            current_buy_odds: Some(1.40),
            current_buy_implied_probability: Some(1.0 / 1.40),
            current_sell_odds: None,
            current_sell_implied_probability: None,
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::from("2026-03-22T16:10:26Z"),
            can_trade_out: false,
        }];

        let rows = historical_position_rows(&snapshot);
        let rendered = format!("{:?}", rows[0]);

        assert!(rendered.contains("\"-\""));
    }

    #[test]
    fn event_date_and_time_can_be_derived_from_smarkets_url() {
        let row = OpenPositionRow {
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
            overall_pnl_known: true,
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
        };

        assert_eq!(super::event_date_label(&row), "2026-03-14");
        assert_eq!(super::event_time_label(&row), "20:00");
    }

    #[test]
    fn event_date_and_time_can_be_derived_from_iso_timestamp() {
        let row = OpenPositionRow {
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
            overall_pnl_known: true,
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
        };

        assert_eq!(super::event_date_label(&row), "2026-03-03");
        assert_eq!(super::event_time_label(&row), "14:08");
    }

    #[test]
    fn position_section_heights_scale_with_relative_row_volume() {
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
            overall_pnl_known: true,
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
        snapshot.historical_positions = vec![snapshot.open_positions[0].clone(); 8];

        let (compact_active_height, compact_history_height) =
            super::position_section_heights(&snapshot, 40);
        snapshot.open_positions = vec![snapshot.open_positions[0].clone(); 8];
        snapshot.historical_positions = vec![snapshot.open_positions[0].clone(); 1];
        let (expanded_active_height, expanded_history_height) =
            super::position_section_heights(&snapshot, 40);

        assert!(compact_active_height < expanded_active_height);
        assert!(compact_history_height > expanded_history_height);
        assert_eq!(compact_active_height + compact_history_height, 40);
        assert_eq!(expanded_active_height + expanded_history_height, 40);
    }

    #[test]
    fn exit_recommendation_lines_show_policy_and_recommendation() {
        let mut snapshot = sample_snapshot();
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("Arsenal v Everton"),
            event_status: String::from("27'|Premier League"),
            event_url: String::from(
                "https://smarkets.com/football/england-premier-league/2026/03/14/20-00/arsenal-vs-everton/44919693/",
            ),
            contract: String::from("Draw"),
            market: String::from("Full-time result"),
            status: String::from("Order filled"),
            market_status: String::from("tradable"),
            is_in_play: true,
            price: 3.35,
            stake: 9.91,
            liability: 23.29,
            current_value: 6.64,
            pnl_amount: 3.27,
            overall_pnl_known: true,
            current_back_odds: Some(5.0),
            current_implied_probability: Some(0.2),
            current_implied_percentage: Some(20.0),
            current_buy_odds: Some(5.0),
            current_buy_implied_probability: Some(0.2),
            current_sell_odds: Some(5.1),
            current_sell_implied_probability: Some(1.0 / 5.1),
            current_score: String::from("0-0"),
            current_score_home: Some(0),
            current_score_away: Some(0),
            live_clock: String::from("27'"),
            can_trade_out: true,
        }];

        let rendered = exit_recommendation_lines(&snapshot)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Target 5.00 | Stop 5.00 | Hard floor - | Warn default true"));
        assert!(rendered.contains("bet-001 | hold | worst 1.27"));
        assert!(rendered.contains("reason within_thresholds"));
    }

    #[test]
    fn watch_lines_show_probabilities_and_market_ev() {
        let mut snapshot = sample_snapshot();
        snapshot.tracked_bets.clear();
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

    #[test]
    fn watch_lines_use_combined_position_outcomes_when_both_legs_exist() {
        let mut snapshot = sample_snapshot();
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("Arsenal v Everton"),
            event_status: String::from("27'|Premier League"),
            event_url: String::from(
                "https://smarkets.com/football/england-premier-league/2026/03/14/20-00/arsenal-vs-everton/44919693/",
            ),
            contract: String::from("Draw"),
            market: String::from("Full-time result"),
            status: String::from("Order filled"),
            market_status: String::from("tradable"),
            is_in_play: true,
            price: 3.35,
            stake: 9.91,
            liability: 23.29,
            current_value: 6.64,
            pnl_amount: 3.27,
            overall_pnl_known: true,
            current_back_odds: Some(5.0),
            current_implied_probability: Some(0.2),
            current_implied_percentage: Some(20.0),
            current_buy_odds: Some(5.0),
            current_buy_implied_probability: Some(0.2),
            current_sell_odds: Some(5.1),
            current_sell_implied_probability: Some(1.0 / 5.1),
            current_score: String::from("0-0"),
            current_score_home: Some(0),
            current_score_away: Some(0),
            live_clock: String::from("27'"),
            can_trade_out: true,
        }];

        let rendered = watch_lines(&snapshot)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Draw | Full-time result"));
        assert!(rendered.contains("live 5.00 | profit 11.41 | stop 2.57"));
        assert!(rendered.contains("prob entry 29.85% | live 20.00% | profit 8.77% | stop 38.89%"));
        assert!(rendered.contains("hold -21.05/+7.91 | lock +5.51/+1.27 | action watch"));
    }

    #[test]
    fn half_cashout_outcomes_reduce_downside_without_flattening_upside() {
        let mut snapshot = sample_snapshot();
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("Arsenal v Everton"),
            event_status: String::from("27'|Premier League"),
            event_url: String::from(
                "https://smarkets.com/football/england-premier-league/2026/03/14/20-00/arsenal-vs-everton/44919693/",
            ),
            contract: String::from("Draw"),
            market: String::from("Full-time result"),
            status: String::from("Order filled"),
            market_status: String::from("tradable"),
            is_in_play: true,
            price: 3.35,
            stake: 9.91,
            liability: 23.29,
            current_value: 6.64,
            pnl_amount: 3.27,
            overall_pnl_known: true,
            current_back_odds: Some(5.0),
            current_implied_probability: Some(0.2),
            current_implied_percentage: Some(20.0),
            current_buy_odds: Some(5.0),
            current_buy_implied_probability: Some(0.2),
            current_sell_odds: Some(5.1),
            current_sell_implied_probability: Some(1.0 / 5.1),
            current_score: String::from("0-0"),
            current_score_home: Some(0),
            current_score_away: Some(0),
            live_clock: String::from("27'"),
            can_trade_out: true,
        }];

        let view = super::active_position_views(&snapshot)
            .into_iter()
            .next()
            .expect("paired active view");
        let hold = super::active_hold_outcomes(view).expect("hold");
        let half = super::active_half_cashout_outcomes(view).expect("half");
        let lock = super::active_total_cashout_outcomes(view).expect("lock");

        assert!((hold.0 + 21.05).abs() < 0.02);
        assert!((hold.1 - 7.91).abs() < 0.02);
        assert!((half.0 + 7.77).abs() < 0.02);
        assert!((half.1 - 4.59).abs() < 0.02);
        assert!((lock.0 - 5.51).abs() < 0.02);
        assert!((lock.1 - 1.27).abs() < 0.02);
        assert!(half.0 > hold.0);
        assert!(half.0 < lock.0);
        assert!(half.1 < hold.1);
        assert!(half.1 > lock.1);
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
            ledger_pnl_summary: Default::default(),
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
            recorder_bundle: None,
            recorder_events: Vec::new(),
            transport_summary: None,
            transport_events: Vec::new(),
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
                funding_kind: String::from("cash"),
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
            horse_matcher: None,
        }
    }

    fn sample_open_row() -> OpenPositionRow {
        OpenPositionRow {
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
            overall_pnl_known: true,
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
        }
    }
}
