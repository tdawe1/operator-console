use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, TradingActionField};
use crate::trading_actions::{format_decimal, TradingRiskSeverity};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let Some(overlay) = app.trading_action_overlay() else {
        return;
    };

    let popup = popup_area(area, 78, 76);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(Span::styled(
            " 󰍹 Trading Action ",
            Style::default()
                .fg(accent_blue())
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .border_style(Style::default().fg(border_color()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let stake_display = if overlay.editing {
        format!("{}_", overlay.buffer)
    } else {
        overlay.buffer.clone()
    };
    let price_display = overlay
        .selected_price()
        .map(format_decimal)
        .unwrap_or_else(|| String::from("-"));
    let header_lines = vec![
        Line::from(vec![
            label("Source"),
            value(match overlay.seed.source {
                crate::trading_actions::TradingActionSource::OddsMatcher => "OddsMatcher",
                crate::trading_actions::TradingActionSource::HorseMatcher => "HorseMatcher",
                crate::trading_actions::TradingActionSource::MarketIntel => "Market Intel",
                crate::trading_actions::TradingActionSource::Positions => "Positions",
            }),
            Span::raw("   "),
            label("Venue"),
            value(overlay.seed.venue.as_str()),
        ]),
        Line::from(vec![label("Event "), value(&overlay.seed.event_name)]),
        Line::from(vec![label("Market"), value(&overlay.seed.market_name)]),
        Line::from(vec![label("Pick  "), value(&overlay.seed.selection_name)]),
        Line::from(vec![
            label("Route "),
            value(
                overlay
                    .seed
                    .deep_link_url
                    .as_deref()
                    .or(overlay.seed.event_url.as_deref())
                    .unwrap_or("-"),
            ),
        ]),
    ];
    let layout = Layout::vertical([Constraint::Length(7), Constraint::Min(12)]).split(inner);
    let body = Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(layout[1]);

    let ticket_lines = vec![
        Line::from(vec![
            field_value(
                overlay.selected_field == TradingActionField::Mode,
                "Mode",
                overlay.mode.label(),
            ),
            Span::raw("   "),
            field_value(
                overlay.selected_field == TradingActionField::Side,
                "Side",
                overlay.side.label(),
            ),
            Span::raw("   "),
            field_value(
                overlay.selected_field == TradingActionField::TimeInForce,
                "Order",
                overlay.time_in_force.label(),
            ),
            Span::raw("   "),
            field_value(
                overlay.selected_field == TradingActionField::Stake,
                "Stake",
                &stake_display,
            ),
        ]),
        Line::from(vec![label("Prices"), value(&price_summary(app))]),
        Line::from(vec![
            label("Quote "),
            value(&price_display),
            Span::raw("   "),
            label("Exec  "),
            value(
                if overlay.mode == crate::trading_actions::TradingActionMode::Review {
                    "dry review"
                } else {
                    "submit"
                },
            ),
        ]),
        Line::from(vec![
            label("Slip  "),
            value(overlay.seed.betslip_market_id.as_deref().unwrap_or("-")),
        ]),
        Line::from(vec![
            label("SelId "),
            value(overlay.seed.betslip_selection_id.as_deref().unwrap_or("-")),
        ]),
        Line::from(vec![
            if overlay.selected_field == TradingActionField::Execute {
                Span::styled(
                    " Submit Ticket ",
                    Style::default()
                        .fg(on_color(accent_gold()))
                        .bg(accent_gold())
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    " Submit Ticket ",
                    Style::default()
                        .fg(text_color())
                        .bg(elevated_background())
                        .add_modifier(Modifier::BOLD),
                )
            },
            Span::raw("   "),
            Span::styled(
                "Enter run • h/l or [/] cycle • j/k move • Esc close",
                Style::default().fg(muted_text()),
            ),
        ]),
    ];
    let mut risk_lines = vec![Line::from(vec![
        label("Policy"),
        value(if overlay.risk_report.reduce_only {
            "reduce-only"
        } else {
            "open/increase"
        }),
        Span::raw("   "),
        label("Warn"),
        value(&overlay.risk_report.warning_count.to_string()),
        Span::raw("   "),
        label("Block"),
        value(&overlay.risk_report.blocking_submit_count.to_string()),
    ])];
    for check in overlay.risk_report.checks.iter().take(4) {
        risk_lines.push(Line::from(vec![
            Span::styled(
                format!("[{}:{}] ", check.severity.label(), check.scope.label()),
                Style::default().fg(match check.severity {
                    TradingRiskSeverity::Info => accent_blue(),
                    TradingRiskSeverity::Warning => accent_gold(),
                    TradingRiskSeverity::Block => accent_red(),
                }),
            ),
            Span::styled(
                check.summary.clone(),
                Style::default()
                    .fg(text_color())
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        risk_lines.push(Line::from(Span::styled(
            check.detail.clone(),
            Style::default().fg(muted_text()),
        )));
    }
    if !overlay.seed.notes.is_empty() {
        risk_lines.push(Line::raw(""));
        for note in overlay.seed.notes.iter().take(3) {
            risk_lines.push(Line::from(Span::styled(
                format!("• {note}"),
                Style::default().fg(muted_text()),
            )));
        }
    }
    if overlay.seed.venue == crate::domain::VenueId::Matchbook {
        risk_lines.extend(matchbook_context_lines(app, overlay));
    }
    if let Some(backend) = overlay.backend_gateway.as_ref() {
        risk_lines.push(Line::raw(""));
        risk_lines.push(Line::from(vec![
            Span::styled("Backend ", Style::default().fg(accent_blue())),
            Span::styled(
                format!("{} ({})", backend.gateway, backend.mode),
                Style::default()
                    .fg(text_color())
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        risk_lines.push(Line::from(Span::styled(
            backend.detail.clone(),
            Style::default().fg(muted_text()),
        )));
        if let Some(status) = backend.last_status.as_ref() {
            risk_lines.push(Line::from(vec![
                Span::styled("Result ", Style::default().fg(accent_gold())),
                Span::styled(status.clone(), Style::default().fg(text_color())),
            ]));
        }
        if let Some(detail) = backend.last_detail.as_ref() {
            risk_lines.push(Line::from(Span::styled(
                detail.clone(),
                Style::default().fg(muted_text()),
            )));
        }
        if let Some(executable) = backend.executable {
            risk_lines.push(Line::from(Span::styled(
                format!("Executable: {}", if executable { "yes" } else { "no" }),
                Style::default().fg(if executable {
                    accent_green()
                } else {
                    accent_gold()
                }),
            )));
        }
        if let Some(accepted) = backend.accepted {
            risk_lines.push(Line::from(Span::styled(
                format!("Accepted: {}", if accepted { "yes" } else { "no" }),
                Style::default().fg(if accepted {
                    accent_green()
                } else {
                    accent_red()
                }),
            )));
        }
    }
    risk_lines.extend([
        Line::raw(""),
        Line::from(Span::styled(
            app.status_message(),
            Style::default().fg(muted_text()),
        )),
    ]);
    let header = Paragraph::new(header_lines)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(border_color())),
        );
    frame.render_widget(header, layout[0]);
    let ticket = Paragraph::new(ticket_lines)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .title(Span::styled(
                    " Bet Slip ",
                    Style::default()
                        .fg(accent_cyan())
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::RIGHT),
        );
    frame.render_widget(ticket, body[0]);
    let paragraph = Paragraph::new(risk_lines)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .block(
            Block::default().title(Span::styled(
                " Risk Tape ",
                Style::default()
                    .fg(accent_gold())
                    .add_modifier(Modifier::BOLD),
            )),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, body[1]);
}

fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let [area] = Layout::vertical([Constraint::Percentage(percent_y)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(Flex::Center)
        .areas(area);
    area
}

fn matchbook_context_lines(
    app: &App,
    overlay: &crate::app_state::TradingActionOverlayState,
) -> Vec<Line<'static>> {
    let Some(state) = app.matchbook_account_state() else {
        return vec![
            Line::raw(""),
            Line::from(Span::styled(
                "Matchbook API monitor syncing...",
                Style::default().fg(muted_text()),
            )),
        ];
    };

    let runner_id = overlay
        .seed
        .betslip_selection_id
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string();
    let runner_offers = state
        .current_offers
        .iter()
        .filter(|offer| !runner_id.is_empty() && offer.runner_id == runner_id)
        .collect::<Vec<_>>();
    let runner_bets = state
        .current_bets
        .iter()
        .filter(|bet| !runner_id.is_empty() && bet.runner_id == runner_id)
        .collect::<Vec<_>>();
    let runner_positions = state
        .positions
        .iter()
        .filter(|position| !runner_id.is_empty() && position.runner_id == runner_id)
        .collect::<Vec<_>>();

    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("Matchbook ", Style::default().fg(accent_blue())),
            Span::styled(
                state.balance_label.clone(),
                Style::default()
                    .fg(accent_green())
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::raw(format!(
            "orders {} • bets {} • positions {}",
            state.summary.open_offer_count,
            state.summary.current_bet_count,
            state.summary.position_count
        )),
    ];

    if runner_id.is_empty() {
        lines.push(Line::raw(
            "runner id unavailable • market-specific Matchbook context limited",
        ));
    }

    if !runner_offers.is_empty() || !runner_bets.is_empty() || !runner_positions.is_empty() {
        lines.push(Line::raw(format!(
            "runner offers {} • runner bets {} • runner positions {}",
            runner_offers.len(),
            runner_bets.len(),
            runner_positions.len()
        )));
    }
    for offer in runner_offers.iter().take(2) {
        lines.push(Line::from(Span::styled(
            format!(
                "• {} {} @ {} rem {}",
                offer.side,
                offer.selection_name,
                offer
                    .odds
                    .map(format_decimal)
                    .unwrap_or_else(|| String::from("-")),
                offer
                    .remaining_stake
                    .or(offer.stake)
                    .map(format_decimal)
                    .unwrap_or_else(|| String::from("-"))
            ),
            Style::default().fg(muted_text()),
        )));
    }
    for bet in runner_bets.iter().take(2) {
        lines.push(Line::from(Span::styled(
            format!(
                "• bet {} {} @ {} pnl {}",
                bet.side,
                bet.selection_name,
                bet.odds
                    .map(format_decimal)
                    .unwrap_or_else(|| String::from("-")),
                bet.profit_loss
                    .map(|value| format!("{value:+.2}"))
                    .unwrap_or_else(|| String::from("-"))
            ),
            Style::default().fg(muted_text()),
        )));
    }
    for position in runner_positions.iter().take(2) {
        lines.push(Line::from(Span::styled(
            format!(
                "• pos {} exp {} pnl {}",
                position.selection_name,
                position
                    .exposure
                    .map(format_decimal)
                    .unwrap_or_else(|| String::from("-")),
                position
                    .profit_loss
                    .map(|value| format!("{value:+.2}"))
                    .unwrap_or_else(|| String::from("-"))
            ),
            Style::default().fg(muted_text()),
        )));
    }
    lines
}

fn price_summary(app: &App) -> String {
    let Some(overlay) = app.trading_action_overlay() else {
        return String::from("-");
    };
    let buy = overlay
        .seed
        .buy_price
        .map(format_decimal)
        .unwrap_or_else(|| String::from("-"));
    let sell = overlay
        .seed
        .sell_price
        .map(format_decimal)
        .unwrap_or_else(|| String::from("-"));
    format!("buy {buy} | sell {sell}")
}

fn field_value(selected: bool, label_text: &str, value_text: &str) -> Span<'static> {
    let style = if selected {
        Style::default()
            .fg(on_color(accent_cyan()))
            .bg(accent_cyan())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(text_color())
    };
    Span::styled(format!("{label_text}: {value_text}"), style)
}

fn label(value: &str) -> Span<'static> {
    Span::styled(format!("{value} "), Style::default().fg(muted_text()))
}

fn value(value: &str) -> Span<'static> {
    Span::styled(
        value.to_string(),
        Style::default()
            .fg(text_color())
            .add_modifier(Modifier::BOLD),
    )
}

fn accent_blue() -> Color {
    crate::theme::accent_blue()
}

fn accent_cyan() -> Color {
    crate::theme::accent_cyan()
}

fn accent_gold() -> Color {
    crate::theme::accent_gold()
}

fn accent_green() -> Color {
    crate::theme::accent_green()
}

fn accent_red() -> Color {
    crate::theme::accent_red()
}

fn panel_background() -> Color {
    crate::theme::panel_background()
}

fn elevated_background() -> Color {
    crate::theme::elevated_background()
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

fn on_color(color: Color) -> Color {
    crate::theme::contrast_text(color)
}
