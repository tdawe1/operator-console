use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use crate::app::{App, ObservabilitySection, Panel, TradingSection};
use crate::domain::WorkerStatus;
use crate::panels;
use crate::recorder::RecorderStatus;

pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    frame.render_widget(
        Block::default().style(Style::default().bg(shell_background()).fg(Color::White)),
        frame.area(),
    );

    let positions_owns_footer = matches!(
        (app.active_panel(), app.active_trading_section()),
        (Panel::Trading, TradingSection::Positions)
    );
    let shell = if positions_owns_footer {
        Layout::vertical([Constraint::Length(6), Constraint::Min(10)]).split(frame.area())
    } else {
        Layout::vertical([
            Constraint::Length(6),
            Constraint::Min(10),
            Constraint::Length(5),
        ])
        .split(frame.area())
    };

    render_status_bar(frame, shell[0], app);
    render_main(frame, shell[1], app);

    if !positions_owns_footer {
        render_footer(frame, shell[2], app);
    }
}

fn render_main(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(8)]).split(area);

    match app.active_panel() {
        Panel::Trading => {
            render_subnav(
                frame,
                layout[0],
                &TradingSection::ALL.map(TradingSection::label),
                trading_index(app.active_trading_section()),
                "󰊠 Trading",
            );
            match app.active_trading_section() {
                TradingSection::Accounts => {
                    let snapshot = app.snapshot().clone();
                    panels::exchanges::render(
                        frame,
                        layout[1],
                        &snapshot,
                        app.exchange_list_state(),
                    )
                }
                TradingSection::Positions => {
                    let snapshot = app.snapshot().clone();
                    let status_message = app.status_message().to_string();
                    let help_text = app.help_text().to_string();
                    let positions_focus = app.positions_focus();
                    let show_live_view_overlay = app.live_view_overlay_visible();
                    let (open_state, historical_state) = app.position_table_states();
                    panels::trading_positions::render(
                        frame,
                        layout[1],
                        &snapshot,
                        open_state,
                        historical_state,
                        positions_focus,
                        show_live_view_overlay,
                        &status_message,
                        &help_text,
                    )
                }
                TradingSection::Markets => {
                    let snapshot = app.snapshot().clone();
                    panels::trading_markets::render(
                        frame,
                        layout[1],
                        &snapshot,
                        app.open_position_table_state(),
                    )
                }
                TradingSection::OddsMatcher => panels::oddsmatcher::render(frame, layout[1], app),
                TradingSection::Stats => {
                    panels::trading_stats::render(frame, layout[1], app.snapshot())
                }
                TradingSection::Calculator => panels::calculator::render(frame, layout[1], app),
                TradingSection::Recorder => panels::recorder::render(frame, layout[1], app),
            }
        }
        Panel::Observability => {
            render_subnav(
                frame,
                layout[0],
                &ObservabilitySection::ALL.map(ObservabilitySection::label),
                observability_index(app.active_observability_section()),
                "󰍹 Observability",
            );
            panels::observability::render(
                frame,
                layout[1],
                app,
                app.active_observability_section(),
            );
        }
    }
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("󱂬 ", Style::default().fg(accent_blue())),
            Span::raw(app.status_message()),
        ]),
        Line::raw("q quit • o observability • r refresh • v live view • s start recorder • x stop recorder"),
        Line::raw("enter edit • esc cancel • [/] cycle suggestions • u reload • D defaults"),
    ])
    .block(shell_block("󰘳 Keymap", accent_gold()).padding(Padding::horizontal(1)))
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, area);
}

fn render_subnav(frame: &mut Frame<'_>, area: Rect, titles: &[&str], selected: usize, title: &str) {
    let tabs = Tabs::new(titles.to_vec())
        .select(selected)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(accent_blue()))
                .style(Style::default().bg(panel_background()).fg(text_color())),
        )
        .style(Style::default().fg(muted_text()).bg(panel_background()))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        )
        .divider("│");
    frame.render_widget(tabs, area);
}

fn render_status_bar(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let layout = Layout::horizontal([
        Constraint::Percentage(42),
        Constraint::Percentage(28),
        Constraint::Percentage(30),
    ])
    .split(area);
    let runtime = app.snapshot().runtime.as_ref();

    let focus = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            active_context_label(app),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("󰥔 View ", Style::default().fg(muted_text())),
            Span::raw(panel_subtitle(app)),
        ]),
        Line::from(vec![
            Span::styled("󰅐 Updated ", Style::default().fg(muted_text())),
            Span::styled(last_refresh_label(app), Style::default().fg(accent_green())),
            Span::raw("   "),
            Span::styled("󰞇 Pos ", Style::default().fg(muted_text())),
            Span::styled(
                app.snapshot().open_positions.len().to_string(),
                Style::default()
                    .fg(accent_cyan())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled("󰍵 Dec ", Style::default().fg(muted_text())),
            Span::styled(
                app.snapshot().decisions.len().to_string(),
                Style::default()
                    .fg(accent_pink())
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("󰕮 Panel ", Style::default().fg(muted_text())),
            Span::raw(panel_cycle_label(app)),
        ]),
    ])
    .block(shell_block("󰘳 Focus", accent_blue()).padding(Padding::horizontal(1)));
    frame.render_widget(focus, layout[0]);

    let runtime_summary = Paragraph::new(vec![
        badge_line(
            "󰒋 Worker",
            &worker_status_label(app),
            worker_status_color(app),
        ),
        badge_line(
            "󰑓 Recorder",
            &format!("{:?}", app.recorder_status()),
            recorder_status_color(app.recorder_status()),
        ),
        badge_line("󰆼 Source", source_mode(app), accent_gold()),
    ])
    .block(shell_block("󱎆 Runtime", accent_pink()).padding(Padding::horizontal(1)));
    frame.render_widget(runtime_summary, layout[1]);

    let refresh = Paragraph::new(vec![
        badge_line("󰅐 Last refresh", &last_refresh_label(app), accent_green()),
        badge_line(
            "󰑮 Iteration",
            &runtime
                .and_then(|summary| summary.watcher_iteration)
                .map(|value| value.to_string())
                .unwrap_or_else(|| String::from("-")),
            accent_cyan(),
        ),
        badge_line(
            "󰄬 Freshness",
            if runtime.map(|summary| summary.stale).unwrap_or(false) {
                "stale"
            } else {
                "fresh"
            },
            if runtime.map(|summary| summary.stale).unwrap_or(false) {
                accent_red()
            } else {
                accent_green()
            },
        ),
    ])
    .block(shell_block("󰐹 Snapshot", accent_green()).padding(Padding::horizontal(1)));
    frame.render_widget(refresh, layout[2]);
}

fn shell_block(title: &'static str, color: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .border_style(Style::default().fg(border_color()))
}

fn shell_background() -> Color {
    Color::Rgb(10, 14, 20)
}

fn panel_background() -> Color {
    Color::Rgb(16, 22, 30)
}

fn text_color() -> Color {
    Color::Rgb(234, 240, 246)
}

fn muted_text() -> Color {
    Color::Rgb(152, 166, 181)
}

fn border_color() -> Color {
    Color::Rgb(74, 88, 104)
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

fn badge_line(label: &'static str, value: &str, accent: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(muted_text())),
        Span::styled(
            value.to_string(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn panel_subtitle(app: &App) -> &'static str {
    match app.active_panel() {
        Panel::Trading => match app.active_trading_section() {
            TradingSection::Accounts => "Venue state, exchange coverage, and selection context.",
            TradingSection::Positions => "Live positions, exit readiness, and watch thresholds.",
            TradingSection::Markets => "Markets and watch candidates from the current provider.",
            TradingSection::OddsMatcher => {
                "Live bookmaker/exchange opportunities from OddsMatcher."
            }
            TradingSection::Stats => "Trading account and performance rollups.",
            TradingSection::Calculator => {
                "Native matched-betting calculator and scenario analysis."
            }
            TradingSection::Recorder => "Recorder controls and live capture configuration.",
        },
        Panel::Observability => "Workers, watcher freshness, and operator diagnostics.",
    }
}

fn panel_cycle_label(app: &App) -> String {
    Panel::ALL
        .iter()
        .map(|panel| {
            if *panel == app.active_panel() {
                format!("[{}]", panel.label())
            } else {
                panel.label().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

fn active_context_label(app: &App) -> String {
    match app.active_panel() {
        Panel::Trading => format!("Trading / {}", app.active_trading_section().label()),
        Panel::Observability => format!(
            "Observability / {}",
            app.active_observability_section().label()
        ),
    }
}

fn last_refresh_label(app: &App) -> String {
    app.snapshot()
        .runtime
        .as_ref()
        .map(|runtime| {
            runtime
                .updated_at
                .replace('T', " ")
                .trim_end_matches('Z')
                .to_string()
        })
        .unwrap_or_else(|| String::from("unknown"))
}

fn worker_status_label(app: &App) -> String {
    format!("{:?}", app.snapshot().worker.status)
}

fn source_mode(app: &App) -> &'static str {
    if app.snapshot().worker.detail.contains("Stub demo") {
        "stub/demo"
    } else if *app.recorder_status() == RecorderStatus::Running
        || app.snapshot().worker.name == "bet-recorder"
    {
        "recorder-backed"
    } else {
        "provider-backed"
    }
}

fn worker_status_color(app: &App) -> Color {
    match app.snapshot().worker.status {
        WorkerStatus::Ready => accent_green(),
        WorkerStatus::Busy => accent_gold(),
        WorkerStatus::Idle => muted_text(),
        WorkerStatus::Error => accent_red(),
    }
}

fn recorder_status_color(status: &RecorderStatus) -> Color {
    match status {
        RecorderStatus::Running => accent_green(),
        RecorderStatus::Stopped => accent_gold(),
        RecorderStatus::Error => accent_red(),
        RecorderStatus::Disabled => muted_text(),
    }
}

fn trading_index(section: TradingSection) -> usize {
    match section {
        TradingSection::Accounts => 0,
        TradingSection::Positions => 1,
        TradingSection::Markets => 2,
        TradingSection::OddsMatcher => 3,
        TradingSection::Stats => 4,
        TradingSection::Calculator => 5,
        TradingSection::Recorder => 6,
    }
}

fn observability_index(section: ObservabilitySection) -> usize {
    match section {
        ObservabilitySection::Workers => 0,
        ObservabilitySection::Watchers => 1,
        ObservabilitySection::Configs => 2,
        ObservabilitySection::Logs => 3,
        ObservabilitySection::Health => 4,
    }
}
