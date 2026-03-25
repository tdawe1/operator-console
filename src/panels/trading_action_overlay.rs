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
            "󰍹 Trading Action",
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
                        .fg(Color::Black)
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
                    "Bet Slip",
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
                "Risk Tape",
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
            .fg(Color::Black)
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
    Color::Rgb(94, 188, 255)
}

fn accent_cyan() -> Color {
    Color::Rgb(84, 214, 208)
}

fn accent_gold() -> Color {
    Color::Rgb(255, 205, 96)
}

fn accent_red() -> Color {
    Color::Rgb(255, 118, 118)
}

fn panel_background() -> Color {
    Color::Rgb(16, 22, 30)
}

fn elevated_background() -> Color {
    Color::Rgb(28, 36, 48)
}

fn border_color() -> Color {
    Color::Rgb(57, 72, 89)
}

fn text_color() -> Color {
    Color::Rgb(236, 241, 246)
}

fn muted_text() -> Color {
    Color::Rgb(138, 152, 168)
}
