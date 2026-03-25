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

    let shell = Layout::vertical([Constraint::Length(4), Constraint::Min(10)]).split(frame.area());

    render_status_bar(frame, shell[0], app);
    render_main(frame, shell[1], app);

    panels::trading_markets::render_overlay(frame, frame.area(), app);
    panels::trading_action_overlay::render(frame, frame.area(), app);
    render_keymap_overlay(frame, frame.area(), app);
}

fn render_main(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(8)]).split(area);

    match app.active_panel() {
        Panel::Trading => {
            render_trading_subnav(frame, layout[0], app);
            match app.active_trading_section() {
                TradingSection::Positions => {
                    let snapshot = app.snapshot().clone();
                    let owls_dashboard = app.owls_dashboard().clone();
                    let status_message = app.status_message().to_string();
                    let status_scroll = app.status_scroll();
                    let positions_focus = app.positions_focus();
                    let show_live_view_overlay = app.live_view_overlay_visible();
                    let (open_state, historical_state) = app.position_table_states();
                    panels::trading_positions::render(
                        frame,
                        layout[1],
                        &snapshot,
                        &owls_dashboard,
                        open_state,
                        historical_state,
                        positions_focus,
                        show_live_view_overlay,
                        &status_message,
                        status_scroll,
                    )
                }
                TradingSection::Markets => panels::trading_markets::render(frame, layout[1], app),
                TradingSection::Live => panels::trading_markets::render(frame, layout[1], app),
                TradingSection::Props => panels::trading_markets::render(frame, layout[1], app),
                TradingSection::Matcher => panels::matcher::render(frame, layout[1], app),
                TradingSection::Stats => panels::trading_stats::render(
                    frame,
                    layout[1],
                    app.snapshot(),
                    app.matchbook_account_state(),
                ),
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

fn render_trading_subnav(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let titles = TradingSection::ALL.map(TradingSection::label);
    render_subnav(
        frame,
        area,
        &titles,
        trading_index(app.active_trading_section()),
        "󰊠 Trading",
    );
    register_tab_targets(area, &titles)
        .into_iter()
        .enumerate()
        .for_each(|(index, rect)| {
            app.register_trading_section_target(rect, TradingSection::ALL[index]);
        });
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

fn register_tab_targets(area: Rect, titles: &[&str]) -> Vec<Rect> {
    let mut targets = Vec::new();
    let mut x = area.x.saturating_add(1);
    let y = area.y.saturating_add(1);
    for title in titles {
        let width = title.len() as u16;
        targets.push(Rect {
            x,
            y,
            width,
            height: 1,
        });
        x = x.saturating_add(width).saturating_add(2);
    }
    targets
}

fn render_status_bar(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let runtime = app.snapshot().runtime.as_ref();
    let owls_ready = app
        .owls_dashboard()
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.status == "ready")
        .count();
    let owls_total = app.owls_dashboard().endpoints.len();

    let body = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                active_context_label(app),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            badge_line(
                "󰒋 Worker",
                &worker_status_label(app),
                worker_status_color(app),
            ),
            Span::raw("  "),
            badge_line(
                "󰑓 Recorder",
                &format!("{:?}", app.recorder_status()),
                recorder_status_color(app.recorder_status()),
            ),
            Span::raw("  "),
            badge_line("󰆼 Source", source_mode(app), accent_gold()),
        ]),
        Line::from(vec![
            badge_line("󰅐 Updated", &last_refresh_label(app), accent_green()),
            Span::raw("  "),
            badge_line(
                "󰞇 Pos",
                &app.snapshot().open_positions.len().to_string(),
                accent_cyan(),
            ),
            Span::raw("  "),
            badge_line(
                "󰍵 Dec",
                &app.snapshot().decisions.len().to_string(),
                accent_pink(),
            ),
            Span::raw("  "),
            badge_line("󰑐 Mode", &refresh_kind_label(app), accent_gold()),
            Span::raw("  "),
            badge_line(
                "󰄬 Fresh",
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
            Span::raw("  "),
            badge_line(
                "󰇚 Owls",
                &format!("{owls_ready}/{owls_total}"),
                accent_blue(),
            ),
        ]),
    ])
    .block(shell_block("Status", accent_blue()).padding(Padding::horizontal(1)))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_keymap_overlay(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if !app.keymap_overlay_visible() {
        return;
    }

    let popup = popup_area(area, 68, 54);
    let lines = vec![
        Line::from(vec![
            Span::styled("Status ", Style::default().fg(accent_blue())),
            Span::raw(truncate_line(app.status_message(), 70)),
        ]),
        Line::raw(""),
        Line::raw("? toggle keymap  •  q quit  •  esc close overlay/cancel"),
        Line::raw("o observability  •  h/l sections  •  arrows or j/k navigate"),
        Line::raw("tab rotate pane tool/view  •  enter open/edit  •  r cache  •  R live"),
        Line::raw("[ / ] cycle Owls sport  •  s start recorder  •  x stop recorder"),
        Line::raw("c cash out first actionable  •  p place action"),
        Line::raw("v live overlay  •  u reload config  •  D defaults"),
        Line::raw("b cycle calc type  •  m toggle calc mode"),
    ];
    let overlay = Paragraph::new(lines)
        .block(shell_block("󰘳 Keymap", accent_gold()).padding(Padding::horizontal(1)))
        .wrap(Wrap { trim: true });
    frame.render_widget(
        Block::default().style(Style::default().bg(shell_background())),
        popup,
    );
    frame.render_widget(overlay, popup);
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

fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical =
        Layout::vertical([Constraint::Percentage(percent_y)]).flex(ratatui::layout::Flex::Center);
    let horizontal =
        Layout::horizontal([Constraint::Percentage(percent_x)]).flex(ratatui::layout::Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
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

fn badge_line(label: &'static str, value: &str, accent: Color) -> Span<'static> {
    Span::styled(
        format!("{label}:{value}"),
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )
}

fn truncate_line(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
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

fn refresh_kind_label(app: &App) -> String {
    match app
        .snapshot()
        .runtime
        .as_ref()
        .map(|runtime| runtime.refresh_kind.as_str())
    {
        Some("bootstrap") => String::from("bootstrap"),
        Some("cached") => String::from("cached"),
        Some("live_capture") => String::from("live"),
        Some(value) if !value.trim().is_empty() => value.replace('_', " "),
        _ => String::from("unknown"),
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
        TradingSection::Positions => 0,
        TradingSection::Markets => 1,
        TradingSection::Live => 2,
        TradingSection::Props => 3,
        TradingSection::Matcher => 4,
        TradingSection::Stats => 5,
        TradingSection::Calculator => 6,
        TradingSection::Recorder => 7,
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
