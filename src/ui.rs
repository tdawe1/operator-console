use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap};
use ratatui::Frame;
use crate::app::{App, PositionsRenderState};
use crate::domain::WorkerStatus;
use crate::manual_positions::ManualPositionField;
use crate::panels;
use crate::wm::PaneId;

// root
pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    frame.render_widget(
        Block::default().style(Style::default().bg(shell_background()).fg(text_color())),
        frame.area(),
    );

    let shell = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(10),
        Constraint::Length(1),
    ])
    .split(frame.area());

    render_top_bar(frame, shell[0], app);
    render_main(frame, shell[1], app);
    render_bottom_bar(frame, shell[2], app);

    panels::trading_markets::render_overlay(frame, frame.area(), app);
    panels::trading_action_overlay::render(frame, frame.area(), app);
    render_manual_position_overlay(frame, frame.area(), app);
    render_keymap_overlay(frame, frame.area(), app);
    render_notifications_overlay(frame, frame.area(), app);
}

fn render_main(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    if let Some(maximized) = app.wm.maximized_pane {
        let inner = area.inner(shell_margin(area));
        render_pane(frame, inner, app, maximized);
    } else {
        draw_main_content(frame, area, app);
    }
}

fn draw_main_content(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let area = area.inner(shell_margin(area));
    if area.width == 0 || area.height == 0 {
        return;
    }

    let column_gap = shell_column_gap(area.width);

    match app.wm.active_workspace {
        0 => {
            let cols = Layout::horizontal([
                Constraint::Percentage(20),
                Constraint::Length(column_gap),
                Constraint::Percentage(55),
                Constraint::Length(column_gap),
                Constraint::Percentage(25),
            ])
            .split(area);
            render_trading_workspace(frame, cols[0], cols[2], cols[4], app);
        }
        1 => {
            let cols = Layout::horizontal([
                Constraint::Percentage(22),
                Constraint::Length(column_gap),
                Constraint::Percentage(48),
                Constraint::Length(column_gap),
                Constraint::Percentage(30),
            ])
            .split(area);
            render_markets_workspace(frame, cols[0], cols[2], cols[4], app);
        }
        2 => {
            let cols = Layout::horizontal([
                Constraint::Percentage(20),
                Constraint::Length(column_gap),
                Constraint::Percentage(55),
                Constraint::Length(column_gap),
                Constraint::Percentage(25),
            ])
            .split(area);
            render_control_workspace(frame, cols[0], cols[2], cols[4], app);
        }
        _ => {
            let cols = Layout::horizontal([
                Constraint::Percentage(20),
                Constraint::Length(column_gap),
                Constraint::Percentage(55),
                Constraint::Length(column_gap),
                Constraint::Percentage(25),
            ])
            .split(area);
            render_trading_workspace(frame, cols[0], cols[2], cols[4], app);
        }
    }
}

fn render_trading_workspace(
    frame: &mut Frame<'_>,
    left_col: Rect,
    center_col: Rect,
    right_col: Rect,
    app: &mut App,
) {
    let row_gap = shell_row_gap(left_col.height);
    let split_gap = shell_column_gap(center_col.width);
    let left = Layout::vertical([
        Constraint::Percentage(60),
        Constraint::Length(row_gap),
        Constraint::Percentage(40),
    ])
    .split(left_col);
    let center = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(row_gap),
        Constraint::Percentage(45),
        Constraint::Length(row_gap),
        Constraint::Percentage(25),
        Constraint::Length(row_gap),
        Constraint::Percentage(25),
        Constraint::Length(row_gap),
        Constraint::Length(3),
    ])
    .split(center_col);
    let right = Layout::vertical([
        Constraint::Percentage(25),
        Constraint::Length(row_gap),
        Constraint::Percentage(65),
        Constraint::Length(row_gap),
        Constraint::Percentage(10),
    ])
    .split(right_col);
    let analyst = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Length(split_gap),
        Constraint::Percentage(50),
    ])
    .split(center[6]);

    let center_pane = match app.active_trading_section() {
        crate::app_state::TradingSection::Live => PaneId::Live,
        crate::app_state::TradingSection::Props => PaneId::Props,
        _ => PaneId::Markets,
    };

    render_pane(frame, left[0], app, PaneId::Intel);
    render_pane(frame, left[2], app, PaneId::Accounts);
    render_market_strip(frame, center[0], app);
    render_pane(frame, center[2], app, PaneId::Chart);
    render_pane(frame, center[4], app, center_pane);
    render_pane(frame, analyst[0], app, PaneId::History);
    render_pane(frame, analyst[2], app, PaneId::Stats);
    render_action_strip(frame, center[8], app);
    render_pane(frame, right[0], app, PaneId::Positions);
    render_pane(frame, right[2], app, PaneId::Matcher);
    render_summary_strip(frame, right[4], app);
}

fn render_markets_workspace(
    frame: &mut Frame<'_>,
    left_col: Rect,
    center_col: Rect,
    right_col: Rect,
    app: &mut App,
) {
    let row_gap = shell_row_gap(left_col.height);
    let left = Layout::vertical([Constraint::Min(12)]).split(left_col);
    let center = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(row_gap),
        Constraint::Length(12),
        Constraint::Length(row_gap),
        Constraint::Min(18),
        Constraint::Length(row_gap),
        Constraint::Length(3),
    ])
    .split(center_col);
    let right = Layout::vertical([Constraint::Min(12)]).split(right_col);

    render_pane(frame, left[0], app, PaneId::Intel);
    render_market_strip(frame, center[0], app);
    render_pane(frame, center[2], app, PaneId::Markets);
    render_pane(frame, center[4], app, PaneId::Chart);
    render_action_strip(frame, center[6], app);
    render_pane(frame, right[0], app, PaneId::Matcher);
}

fn render_control_workspace(
    frame: &mut Frame<'_>,
    left_col: Rect,
    center_col: Rect,
    right_col: Rect,
    app: &mut App,
) {
    let row_gap = shell_row_gap(left_col.height);
    let split_gap = shell_column_gap(center_col.width);
    let left = Layout::vertical([
        Constraint::Percentage(60),
        Constraint::Length(row_gap),
        Constraint::Percentage(40),
    ])
    .split(left_col);
    let center = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(row_gap),
        Constraint::Percentage(65),
        Constraint::Length(row_gap),
        Constraint::Percentage(25),
        Constraint::Length(row_gap),
        Constraint::Length(3),
    ])
    .split(center_col);
    let center_lower = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Length(split_gap),
        Constraint::Percentage(50),
    ])
    .split(center[4]);
    let right = Layout::vertical([
        Constraint::Percentage(25),
        Constraint::Length(row_gap),
        Constraint::Percentage(65),
        Constraint::Length(row_gap),
        Constraint::Percentage(10),
    ])
    .split(right_col);

    render_pane(frame, left[0], app, PaneId::Accounts);
    render_pane(frame, left[2], app, PaneId::Recorder);
    render_market_strip(frame, center[0], app);
    render_pane(frame, center[2], app, PaneId::Observability);
    render_pane(frame, center_lower[0], app, PaneId::Alerts);
    render_pane(frame, center_lower[2], app, PaneId::Calculator);
    render_action_strip(frame, center[6], app);
    render_pane(frame, right[0], app, PaneId::Stats);
    render_pane(frame, right[2], app, PaneId::Positions);
    render_summary_strip(frame, right[4], app);
}

fn render_pane(frame: &mut Frame<'_>, area: Rect, app: &mut App, pane_id: PaneId) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    app.register_pane_target(area, pane_id);

    match pane_id {
        PaneId::Positions => {
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
            panels::trading_positions::render_live_pane(
                frame,
                area,
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
        PaneId::Accounts => {
            let snapshot = app.snapshot().clone();
            panels::exchanges::render(frame, area, &snapshot, app.exchange_list_state())
        }
        PaneId::History => {
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
            panels::trading_positions::render_history_pane(
                frame,
                area,
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
        PaneId::Markets => panels::trading_markets::render_for_section(
            frame,
            area,
            app,
            crate::app_state::TradingSection::Markets,
        ),
        PaneId::Live => panels::trading_markets::render_for_section(
            frame,
            area,
            app,
            crate::app_state::TradingSection::Live,
        ),
        PaneId::Props => panels::trading_markets::render_for_section(
            frame,
            area,
            app,
            crate::app_state::TradingSection::Props,
        ),
        PaneId::Chart => panels::chart::render(frame, area, app),
        PaneId::Intel => panels::intel::render(frame, area, app),
        PaneId::Matcher => panels::matcher::render(frame, area, app),
        PaneId::Stats => panels::trading_stats::render(
            frame,
            area,
            app.snapshot(),
            app.matchbook_account_state(),
        ),
        PaneId::Alerts => panels::alerts::render(frame, area, app),
        PaneId::Calculator => panels::calculator::render(frame, area, app),
        PaneId::Recorder => panels::recorder::render(frame, area, app),
        PaneId::Observability => {
            panels::observability::render(
                frame,
                area,
                app,
                app.active_observability_section(),
            );
        }
    }
}

fn render_top_bar(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bar_area = area.inner(Margin {
        horizontal: if area.width > 48 { 1 } else { 0 },
        vertical: 0,
    });
    if bar_area.width == 0 {
        return;
    }

    let [status_area, workspace_area, meta_area] = Layout::horizontal([
        Constraint::Length(42),
        Constraint::Min(24),
        Constraint::Length(34),
    ])
    .areas(bar_area);
    let latest_event_label = app.last_event_at_label().unwrap_or("--:--:--");
    let status_line = vec![
        Span::styled(
            " SABI ",
            Style::default()
                .fg(accent_gold())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("│ ", Style::default().fg(border_color())),
        Span::styled(
            worker_status_label(app),
            Style::default()
                .fg(worker_status_color(app))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(border_color())),
        Span::styled(
            format!("latest {latest_event_label}"),
            Style::default().fg(accent_cyan()),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Line::from(status_line))
            .style(Style::default().bg(shell_background()))
            .wrap(Wrap { trim: true }),
        status_area,
    );

    let workspace_names = app
        .wm
        .workspaces
        .iter()
        .map(|workspace| workspace.name.clone())
        .collect::<Vec<_>>();
    let mut spans = Vec::new();
    let workspace_width = workspace_names.iter().fold(0_u16, |acc, name| {
        acc + name.len() as u16 + 2 + if acc == 0 { 0 } else { 3 }
    });
    let mut x = workspace_area
        .x
        .saturating_add(workspace_area.width.saturating_sub(workspace_width) / 2);
    for (index, workspace_name) in workspace_names.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" | ", Style::default().fg(muted_text())));
            x = x.saturating_add(3);
        }
        let style = if index == app.wm.active_workspace {
            Style::default()
                .fg(crate::theme::selected_text())
                .bg(selected_background())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(muted_text())
        };
        spans.push(Span::styled(format!(" {} ", workspace_name), style));

        let width = workspace_name.len() as u16 + 2;
        app.register_workspace_target(
            Rect {
                x,
                y: bar_area.y,
                width,
                height: 1,
            },
            index,
        );
        x = x.saturating_add(width);
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().bg(shell_background())),
        workspace_area,
    );

    let now = chrono::Local::now();
    let date = now.format("%b %d").to_string();
    let clock = now.format("%H:%M:%S").to_string();
    let section = app.active_trading_section().label();
    let section_style = if app.active_panel() == crate::app_state::Panel::Trading {
        Style::default()
            .fg(selected_text())
            .bg(selected_background())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(muted_text())
    };
    let meta_line = Line::from(vec![
        Span::styled(
            format!(" {} ", section),
            section_style,
        ),
        Span::styled(" │ ", Style::default().fg(border_color())),
        Span::styled(
            truncate_line(&active_context_label(app), 14),
            Style::default().fg(text_color()),
        ),
        Span::styled(" │ ", Style::default().fg(border_color())),
        Span::styled(format!("{date} {clock}"), Style::default().fg(accent_cyan())),
    ]);
    frame.render_widget(
        Paragraph::new(meta_line)
            .alignment(ratatui::layout::Alignment::Right)
            .style(Style::default().bg(shell_background())),
        meta_area,
    );
}

fn workspace_sections(app: &App) -> Vec<crate::app_state::TradingSection> {
    use crate::app_state::TradingSection;

    match app.wm.active_workspace {
        0 => vec![
            TradingSection::Positions,
            TradingSection::Accounts,
            TradingSection::Stats,
            TradingSection::Alerts,
        ],
        1 => vec![
            TradingSection::Markets,
            TradingSection::Live,
            TradingSection::Props,
            TradingSection::Intel,
            TradingSection::Matcher,
        ],
        2 => vec![
            TradingSection::Recorder,
            TradingSection::Calculator,
            TradingSection::Alerts,
        ],
        _ => TradingSection::ALL.to_vec(),
    }
}

fn render_bottom_bar(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let bar_area = area.inner(Margin {
        horizontal: if area.width > 48 { 1 } else { 0 },
        vertical: 0,
    });
    if bar_area.width == 0 {
        return;
    }
    let runtime = app.snapshot().runtime.as_ref();
    let fresh = if runtime.map(|summary| summary.stale).unwrap_or(false) {
        "stale"
    } else {
        "fresh"
    };
    let issues = app.problem_notifications().len();
    let latest_status = footer_status_detail(app, area.width);
    let minimized = app.current_minimized_panes();
    let minimized_label = if minimized.is_empty() {
        String::from("none")
    } else {
        minimized
            .iter()
            .map(|pane| pane.title())
            .collect::<Vec<_>>()
            .join(",")
    };
    let line = Line::from(vec![
        Span::styled(" operator-console ", Style::default().fg(muted_text())),
        Span::styled("│", Style::default().fg(border_color())),
        Span::styled(format!(" {} ", refresh_kind_label(app)), Style::default().fg(accent_gold())),
        Span::styled("│", Style::default().fg(border_color())),
        Span::styled(format!(" {} ", fresh), Style::default().fg(if fresh == "fresh" { accent_green() } else { accent_red() })),
        Span::styled("│", Style::default().fg(border_color())),
        Span::styled(format!(" latest {} ", last_refresh_label(app)), Style::default().fg(accent_cyan())),
        Span::styled("│", Style::default().fg(border_color())),
        Span::styled(
            if issues == 0 {
                String::from(" No recent errors ")
            } else {
                format!(" [n] {issues} issues ")
            },
            Style::default().fg(if issues == 0 { muted_text() } else { accent_red() }),
        ),
        Span::styled("│", Style::default().fg(border_color())),
        Span::styled(format!(" hidden {} ", minimized_label), Style::default().fg(muted_text())),
        Span::styled("│", Style::default().fg(border_color())),
        Span::styled(truncate_line(&latest_status, 28), Style::default().fg(text_color())),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(shell_background())),
        bar_area,
    );
}

fn render_market_strip(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let title = match app.wm.active_workspace {
        2 => " Control Strip ",
        _ => " Market Index Strip ",
    };
    let content = Line::from(vec![
        Span::styled(
            format!(" {} ", app.wm.current_workspace().name),
            Style::default().fg(accent_blue()).add_modifier(Modifier::BOLD),
        ),
        Span::styled("│", Style::default().fg(border_color())),
        Span::styled(
            format!(" {} ", app.active_trading_section().label()),
            Style::default().fg(text_color()),
        ),
        Span::styled("│", Style::default().fg(border_color())),
        Span::styled(
            format!(" {} ", app.owls_dashboard().sport),
            Style::default().fg(accent_gold()),
        ),
        Span::styled("│", Style::default().fg(border_color())),
        Span::styled(
            truncate_line(app.snapshot().status_line.as_str(), area.width.saturating_sub(24) as usize),
            Style::default().fg(muted_text()),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(content).block(block_with_title(title)),
        area,
    );
}

fn render_action_strip(frame: &mut Frame<'_>, area: Rect, _app: &mut App) {
    let content = Line::from(vec![
        Span::styled(" [Enter] ", Style::default().fg(selected_text()).bg(selected_background()).add_modifier(Modifier::BOLD)),
        Span::styled("inspect", Style::default().fg(text_color())),
        Span::raw("   "),
        Span::styled(" [r] ", Style::default().fg(selected_text()).bg(selected_background()).add_modifier(Modifier::BOLD)),
        Span::styled("refresh", Style::default().fg(text_color())),
        Span::raw("   "),
        Span::styled(" [/] ", Style::default().fg(selected_text()).bg(selected_background()).add_modifier(Modifier::BOLD)),
        Span::styled("cycle sport", Style::default().fg(text_color())),
        Span::raw("   "),
        Span::styled(" [f] ", Style::default().fg(selected_text()).bg(selected_background()).add_modifier(Modifier::BOLD)),
        Span::styled("maximize", Style::default().fg(text_color())),
    ]);
    frame.render_widget(
        Paragraph::new(content).block(block_with_title(" Order Entry Strip ")),
        area,
    );
}

fn render_summary_strip(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let issues = app.problem_notifications().len();
    let text = vec![
        Line::from(vec![
            Span::styled(" Context: ", Style::default().fg(muted_text())),
            Span::styled(active_context_label(app), Style::default().fg(text_color())),
        ]),
        Line::from(vec![
            Span::styled(" Issues: ", Style::default().fg(muted_text())),
            Span::styled(
                if issues == 0 {
                    String::from("none")
                } else {
                    issues.to_string()
                },
                Style::default().fg(if issues == 0 { accent_green() } else { accent_red() }),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(text).block(block_with_title(" Day P&L ")), area);
}

fn block_with_title<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color()))
        .title(title)
        .title_style(Style::default().fg(muted_text()))
        .padding(Padding::new(1, 1, 0, 0))
        .style(Style::default().bg(shell_background()))
}

fn shell_margin(area: Rect) -> Margin {
    Margin {
        horizontal: if area.width >= 160 {
            3
        } else if area.width >= 110 {
            2
        } else {
            1
        },
        vertical: if area.height >= 28 { 1 } else { 0 },
    }
}

fn shell_column_gap(width: u16) -> u16 {
    if width >= 150 {
        3
    } else if width >= 110 {
        2
    } else {
        1
    }
}

fn shell_row_gap(height: u16) -> u16 {
    if height >= 34 {
        2
    } else if height >= 22 {
        1
    } else {
        0
    }
}

fn render_keymap_overlay(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if !app.keymap_overlay_visible() {
        return;
    }

    let popup = popup_area(area, 96, 64);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().style(Style::default().bg(shell_background())),
        popup,
    );

    let shell = shell_block("󰘳 Keymap", accent_gold());
    let inner = shell.inner(popup);
    frame.render_widget(shell, popup);

    let [hero_area, body_area] =
        Layout::vertical([Constraint::Length(4), Constraint::Min(10)]).areas(inner);
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .areas(body_area);
    let [left_top, left_bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(left_area);
    let [right_top, right_bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
            .areas(right_area);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Latest ", Style::default().fg(accent_gold())),
                Span::styled(
                    truncate_line(app.status_message(), 72),
                    Style::default().fg(text_color()),
                ),
            ]),
            Line::from(vec![
                Span::styled("Workspace ", Style::default().fg(accent_blue())),
                Span::styled(
                    app.wm.current_workspace().name.clone(),
                    Style::default().fg(text_color()),
                ),
                Span::raw("   "),
                Span::styled("Pane ", Style::default().fg(accent_cyan())),
                Span::styled(active_context_label(app), Style::default().fg(text_color())),
            ]),
        ])
        .block(shell_block("󰖟 Current Context", accent_blue()))
        .wrap(Wrap { trim: true }),
        hero_area,
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("Movement", Style::default().add_modifier(Modifier::BOLD)),
            Line::raw("1-3 and Alt+1-3 switch workspaces"),
            Line::raw("h/j/k/l focus panes"),
            Line::raw("Left/Right focus panes"),
            Line::raw("Up/Down navigate inside pane"),
            Line::raw("Ctrl+Left/Right switch sections"),
            Line::raw("Alt+arrows also focus panes"),
        ])
        .block(shell_block("󰹑 Navigation", accent_cyan()))
        .wrap(Wrap { trim: true }),
        left_top,
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("Actions", Style::default().add_modifier(Modifier::BOLD)),
            Line::raw("enter open/edit/apply"),
            Line::raw("tab rotate pane tool/view"),
            Line::raw("shift+tab reverse where supported"),
            Line::raw("p place action"),
            Line::raw("a manual position"),
            Line::raw("c cash out first actionable"),
            Line::raw("v live overlay"),
        ])
        .block(shell_block("󰐕 Trading", accent_green()))
        .wrap(Wrap { trim: true }),
        left_bottom,
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("Refresh", Style::default().add_modifier(Modifier::BOLD)),
            Line::raw("r cache"),
            Line::raw("R live"),
            Line::raw("[ / ] cycle Owls sport or suggestions"),
            Line::raw("u reload config"),
            Line::raw("D defaults"),
        ])
        .block(shell_block("󰑐 Data", accent_pink()))
        .wrap(Wrap { trim: true }),
        right_top,
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("Global", Style::default().add_modifier(Modifier::BOLD)),
            Line::raw("? keymap"),
            Line::raw("n problems / events"),
            Line::raw("f maximize pane"),
            Line::raw("o observability"),
            Line::raw("s start recorder"),
            Line::raw("x stop recorder"),
            Line::raw("q quit"),
            Line::raw("esc cancel"),
            Line::raw("b cycle calc type"),
            Line::raw("m toggle mode"),
        ])
        .block(shell_block("󰌌 Controls", accent_gold()))
        .wrap(Wrap { trim: true }),
        right_bottom,
    );
}

fn render_notifications_overlay(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if !app.notifications_overlay_visible() {
        return;
    }

    let popup = popup_area(area, 78, 64);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().style(Style::default().bg(shell_background())),
        popup,
    );

    let [summary_area, problems_area, events_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Percentage(58),
        Constraint::Percentage(42),
    ])
    .areas(popup);

    let summary = Paragraph::new(Line::from(vec![
        Span::styled(
            "Problems ",
            Style::default()
                .fg(accent_red())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(app.problem_notifications().len().to_string()),
        Span::raw("  "),
        Span::styled(
            "Unread ",
            Style::default()
                .fg(accent_gold())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(app.unread_notification_count().to_string()),
        Span::raw("  "),
        Span::styled(
            "Events ",
            Style::default()
                .fg(accent_blue())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(app.recent_events().len().to_string()),
    ]))
    .block(shell_block("󰚌 Error Console", accent_red()));
    frame.render_widget(summary, summary_area);

    let problem_lines = if app.problem_notifications().is_empty() {
        vec![Line::raw("No warnings or critical failures captured.")]
    } else {
        app.problem_notifications()
            .into_iter()
            .take(10)
            .flat_map(|entry| {
                let level_color = match entry.level {
                    crate::alerts::NotificationLevel::Info => accent_blue(),
                    crate::alerts::NotificationLevel::Warning => accent_gold(),
                    crate::alerts::NotificationLevel::Critical => accent_red(),
                };
                vec![
                    Line::from(vec![
                        Span::styled(
                            format!(
                                "{} {} ",
                                entry.created_at,
                                entry.level.label().to_uppercase()
                            ),
                            Style::default()
                                .fg(level_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            truncate_line(&entry.title, 30),
                            Style::default().fg(text_color()),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("  ", Style::default().fg(muted_text())),
                        Span::raw(truncate_line(&entry.detail, 86)),
                    ]),
                    Line::raw(""),
                ]
            })
            .collect()
    };
    let problems = Paragraph::new(problem_lines)
        .block(shell_block("Problems", accent_gold()))
        .wrap(Wrap { trim: true });
    frame.render_widget(problems, problems_area);

    let event_lines = if app.recent_events().is_empty() {
        vec![Line::raw("No recent runtime events.")]
    } else {
        app.recent_events()
            .into_iter()
            .take(10)
            .map(|event| Line::raw(truncate_line(event, 92)))
            .collect()
    };
    let events = Paragraph::new(event_lines)
        .block(shell_block("Recent Events", accent_blue()))
        .wrap(Wrap { trim: true });
    frame.render_widget(events, events_area);
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
        .block(shell_block("󰍉 Manual Position", accent_gold()))
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
            format!(" {} ", title),
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
    crate::theme::shell_background()
}

fn panel_background() -> Color {
    crate::theme::panel_background()
}

fn text_color() -> Color {
    crate::theme::text_color()
}

fn muted_text() -> Color {
    crate::theme::muted_text()
}

fn border_color() -> Color {
    crate::theme::border_color()
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
    let pane = app.active_pane().map(PaneId::title).unwrap_or("Overview");
    format!("{} / {}", app.wm.current_workspace().name, pane)
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

fn footer_status_detail(app: &App, width: u16) -> String {
    let worker = worker_status_label(app);
    let refresh_kind = refresh_kind_label(app);
    let last_refresh = last_refresh_label(app);
    let reserved =
        worker.chars().count() + refresh_kind.chars().count() + last_refresh.chars().count() + 15;
    let max_chars = usize::from(width).saturating_sub(reserved).max(12);
    truncate_line(app.status_message(), max_chars)
}

fn worker_status_label(app: &App) -> String {
    format!("{:?}", app.snapshot().worker.status)
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

#[cfg(test)]
fn ticker_scroll_offset_chars() -> usize {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| (duration.as_millis() / 1000) as usize)
        .unwrap_or(0)
}

#[cfg(test)]
fn ticker_window(text: &str, width: usize, offset: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let glyphs = text.chars().collect::<Vec<_>>();
    if glyphs.len() <= width {
        return text.to_string();
    }

    let mut looped = glyphs;
    looped.extend("      ".chars());
    let len = looped.len();
    let start = offset % len;

    (0..width)
        .map(|index| looped[(start + index) % len])
        .collect()
}

#[cfg(test)]
mod tests {
    use color_eyre::Result;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::{render, ticker_window};
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
    fn live_panel_render_preserves_selected_endpoint() {
        let mut app = App::from_provider(StaticProvider).expect("app");
        app.set_trading_section(TradingSection::Live);
        app.set_owls_dashboard_for_test(large_soccer_dashboard());
        assert!(app.wait_for_async_idle(std::time::Duration::from_millis(200)));

        let selected_index = app
            .visible_owls_endpoints()
            .iter()
            .position(|endpoint| endpoint.id == OwlsEndpointId::ScoresSport)
            .expect("scores endpoint visible");
        app.owls_endpoint_table_state().select(Some(selected_index));
        let selected_before = app.selected_owls_endpoint().map(|endpoint| endpoint.id);

        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(frame, &mut app))
            .expect("draw ui");

        assert_eq!(
            app.selected_owls_endpoint().map(|endpoint| endpoint.id),
            selected_before
        );
    }

    #[test]
    fn ticker_window_stays_static_when_text_fits() {
        assert_eq!(
            ticker_window("Arsenal vs Everton", 32, 5),
            "Arsenal vs Everton"
        );
    }

    #[test]
    fn ticker_window_scrolls_long_text() {
        assert_eq!(ticker_window("abcdef", 4, 0), "abcd");
        assert_eq!(ticker_window("abcdef", 4, 2), "cdef");
        assert_eq!(ticker_window("abcdef", 4, 6), "    ");
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