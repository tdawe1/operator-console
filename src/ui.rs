use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use crate::app::{App, ObservabilitySection, Panel, PositionsRenderState, TradingSection};
use crate::domain::WorkerStatus;
use crate::manual_positions::ManualPositionField;
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
    render_manual_position_overlay(frame, frame.area(), app);
    render_keymap_overlay(frame, frame.area(), app);
    render_notifications_overlay(frame, frame.area(), app);
}

fn render_main(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(8)]).split(area);

    match app.active_panel() {
        Panel::Trading => {
            render_trading_subnav(frame, layout[0], app);
            match app.active_trading_section() {
                TradingSection::Positions => {
                    let PositionsRenderState {
                        snapshot,
                        owls_dashboard,
                        matchbook_account_state,
                        open_table_state,
                        historical_table_state,
                        positions_focus,
                        show_live_view_overlay,
                        status_message,
                        status_scroll,
                    } = app.positions_render_state();
                    panels::trading_positions::render(
                        frame,
                        layout[1],
                        snapshot,
                        owls_dashboard,
                        matchbook_account_state,
                        open_table_state,
                        historical_table_state,
                        positions_focus,
                        show_live_view_overlay,
                        status_message,
                        status_scroll,
                    )
                }
                TradingSection::Markets => panels::trading_markets::render(frame, layout[1], app),
                TradingSection::Live => panels::trading_markets::render(frame, layout[1], app),
                TradingSection::Props => panels::trading_markets::render(frame, layout[1], app),
                TradingSection::Intel => panels::intel::render(frame, layout[1], app),
                TradingSection::Matcher => panels::matcher::render(frame, layout[1], app),
                TradingSection::Stats => panels::trading_stats::render(
                    frame,
                    layout[1],
                    app.snapshot(),
                    app.matchbook_account_state(),
                ),
                TradingSection::Alerts => panels::alerts::render(frame, layout[1], app),
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
    let intel_total = app.intel_source_statuses().len();
    let intel_ready = app.intel_ready_sources();

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
            Span::raw("  "),
            badge_line(
                "󰛨 Intel",
                &format!(
                    "{intel_ready}/{intel_total} {}",
                    app.intel_freshness_label()
                ),
                accent_pink(),
            ),
            Span::raw("  "),
            badge_line(
                "󰍡 Alerts",
                &app.unread_notification_count().to_string(),
                if app.unread_notification_count() > 0 {
                    accent_gold()
                } else {
                    muted_text()
                },
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
        Line::raw("? toggle keymap  •  n toggle alerts  •  q quit  •  esc close overlay/cancel"),
        Line::raw("o observability  •  h/l sections  •  arrows or j/k navigate"),
        Line::raw("tab rotate pane tool/view  •  enter open/edit  •  r cache  •  R live"),
        Line::raw("[ / ] cycle Owls sport  •  s start recorder  •  x stop recorder"),
        Line::raw("c cash out first actionable  •  p place action  •  a manual position"),
        Line::raw("v live overlay  •  u reload config  •  D defaults"),
        Line::raw("b cycle calc type  •  m toggle calc mode"),
    ];
    let overlay = Paragraph::new(lines)
        .block(shell_block("󰘳 Keymap", accent_gold()).padding(Padding::horizontal(1)))
        .wrap(Wrap { trim: true });
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().style(Style::default().bg(shell_background())),
        popup,
    );
    frame.render_widget(overlay, popup);
}

fn render_notifications_overlay(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if !app.notifications_overlay_visible() {
        return;
    }

    let popup = popup_area(area, 74, 58);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Unread ", Style::default().fg(accent_gold())),
            Span::raw(app.unread_notification_count().to_string()),
            Span::raw("  "),
            Span::styled("Recent ", Style::default().fg(accent_blue())),
            Span::raw(app.notifications().len().to_string()),
        ]),
        Line::raw(""),
    ];
    if app.notifications().is_empty() {
        lines.push(Line::raw("No notifications yet."));
    } else {
        for entry in app.notifications().iter().rev().take(10) {
            let level_color = match entry.level {
                crate::alerts::NotificationLevel::Info => accent_blue(),
                crate::alerts::NotificationLevel::Warning => accent_gold(),
                crate::alerts::NotificationLevel::Critical => accent_red(),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} {} ", entry.created_at, entry.level.label()),
                    Style::default()
                        .fg(level_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(truncate_line(&entry.title, 28)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(muted_text())),
                Span::raw(truncate_line(&entry.detail, 72)),
            ]));
        }
    }

    let overlay = Paragraph::new(lines)
        .block(shell_block("󰍡 Notifications", accent_gold()).padding(Padding::horizontal(1)))
        .wrap(Wrap { trim: true });
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().style(Style::default().bg(shell_background())),
        popup,
    );
    frame.render_widget(overlay, popup);
}

fn render_manual_position_overlay(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let Some(overlay) = app.manual_position_overlay() else {
        return;
    };

    let popup = popup_area(area, 70, 66);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Selected ", Style::default().fg(accent_blue())),
            Span::raw(format!(
                "{} / {} / {}",
                truncate_line(&overlay.draft.event, 28),
                truncate_line(&overlay.draft.market, 20),
                truncate_line(&overlay.draft.selection, 16)
            )),
        ]),
        Line::raw(""),
    ];

    for field in ManualPositionField::ALL {
        let selected = overlay.selected_field() == field;
        let label = format!("{:>9}", field.label());
        let value = match field {
            ManualPositionField::Save => String::from("Press Enter to save"),
            _ if overlay.editing && selected => overlay.input_buffer.clone(),
            _ => overlay.selected_value_for(field),
        };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▶ " } else { "  " },
                Style::default().fg(if selected {
                    accent_gold()
                } else {
                    muted_text()
                }),
            ),
            Span::styled(
                format!("{label}: "),
                Style::default()
                    .fg(if selected {
                        accent_cyan()
                    } else {
                        muted_text()
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                truncate_line(&value, 52),
                Style::default().fg(if selected { text_color() } else { muted_text() }),
            ),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::raw(
        "j/k move  •  enter edit/apply/save  •  esc/q close",
    ));
    lines.push(Line::raw(truncate_line(app.manual_positions_note(), 72)));

    let overlay_widget = Paragraph::new(lines)
        .block(shell_block("󰍉 Manual Position", accent_gold()).padding(Padding::horizontal(1)))
        .wrap(Wrap { trim: true });
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().style(Style::default().bg(shell_background())),
        popup,
    );
    frame.render_widget(overlay_widget, popup);
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
    Color::Rgb(255, 140, 205)
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
        TradingSection::Intel => 4,
        TradingSection::Matcher => 5,
        TradingSection::Stats => 6,
        TradingSection::Alerts => 7,
        TradingSection::Calculator => 8,
        TradingSection::Recorder => 9,
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

#[cfg(test)]
mod tests {
    use color_eyre::Result;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::render;
    use crate::app::{App, TradingSection};
    use crate::domain::ExchangePanelSnapshot;
    use crate::owls::{
        self, OwlsEndpointId, OwlsLiveIncident, OwlsLiveScoreEvent, OwlsLiveStat, OwlsPlayerRating,
        OwlsPreviewRow,
    };
    use crate::provider::{ExchangeProvider, ProviderRequest};

    struct StaticProvider;

    impl ExchangeProvider for StaticProvider {
        fn handle(&mut self, _request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
            Ok(ExchangePanelSnapshot::default())
        }
    }

    #[test]
    fn live_panel_render_does_not_clone_selected_endpoint() {
        let mut app = App::from_provider(StaticProvider).expect("app");
        app.set_trading_section(TradingSection::Live);
        app.set_owls_dashboard_for_test(large_soccer_dashboard());

        let selected_index = app
            .visible_owls_endpoints()
            .iter()
            .position(|endpoint| endpoint.id == OwlsEndpointId::ScoresSport)
            .expect("scores endpoint visible");
        app.owls_endpoint_table_state().select(Some(selected_index));

        owls::reset_endpoint_summary_clone_count_for_test();

        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(frame, &mut app))
            .expect("draw ui");

        assert_eq!(owls::endpoint_summary_clone_count_for_test(), 0);
    }

    fn large_soccer_dashboard() -> owls::OwlsDashboard {
        let mut dashboard = owls::dashboard_for_sport("soccer");
        if let Some(endpoint) = dashboard
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == OwlsEndpointId::ScoresSport)
        {
            endpoint.status = String::from("ready");
            endpoint.count = 512;
            endpoint.detail = String::from("live feed soccer");
            endpoint.preview = vec![OwlsPreviewRow {
                label: String::from("Malta at Luxembourg"),
                detail: String::from("72'"),
                metric: String::from("2-1"),
            }];
            endpoint.live_scores = (0..512)
                .map(|index| OwlsLiveScoreEvent {
                    sport: String::from("soccer"),
                    event_id: format!("soccer:event-{index}"),
                    name: format!("Away {index} at Home {index}"),
                    home_team: format!("Home {index}"),
                    away_team: format!("Away {index}"),
                    home_score: Some((index % 4) as i64),
                    away_score: Some(((index + 1) % 4) as i64),
                    status_state: String::from("in"),
                    status_detail: format!("{}'", 10 + (index % 80)),
                    display_clock: (10 + (index % 80)).to_string(),
                    source_match_id: format!("source-{index}"),
                    last_updated: String::from("2026-03-26T12:00:00Z"),
                    stats: vec![
                        OwlsLiveStat {
                            key: String::from("possession"),
                            label: String::from("Possession"),
                            home_value: String::from("54"),
                            away_value: String::from("46"),
                        },
                        OwlsLiveStat {
                            key: String::from("expectedGoals"),
                            label: String::from("xG"),
                            home_value: String::from("1.2"),
                            away_value: String::from("0.8"),
                        },
                    ],
                    incidents: vec![
                        OwlsLiveIncident {
                            minute: Some(22),
                            incident_type: String::from("goal"),
                            team_side: String::from("home"),
                            player_name: format!("Player {index}"),
                            detail: String::from("assist Teammate"),
                        },
                        OwlsLiveIncident {
                            minute: Some(61),
                            incident_type: String::from("yellow"),
                            team_side: String::from("away"),
                            player_name: format!("Defender {index}"),
                            detail: String::new(),
                        },
                    ],
                    player_ratings: vec![
                        OwlsPlayerRating {
                            player_name: format!("Midfielder {index}"),
                            team_side: String::from("home"),
                            rating: Some(7.6),
                        },
                        OwlsPlayerRating {
                            player_name: format!("Forward {index}"),
                            team_side: String::from("away"),
                            rating: Some(7.2),
                        },
                    ],
                })
                .collect();
        }
        dashboard
    }
}
