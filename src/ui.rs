use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use tui_big_text::{BigText, PixelSize};

use crate::app::{App, PositionsRenderState};
use crate::domain::WorkerStatus;
use crate::manual_positions::ManualPositionField;
use crate::panels;
use crate::wm::{effective_ratios, LayoutNode, PaneId, SplitDirection};

// root
pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    frame.render_widget(
        Block::default().style(Style::default().bg(shell_background()).fg(text_color())),
        frame.area(),
    );

    let shell = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(10),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(frame.area());

    render_workspace_strip(frame, shell[0], app);
    render_main(frame, shell[1], app);
    render_minimize_strip(frame, shell[2], app);
    render_footer_line(frame, shell[3], app);

    panels::trading_markets::render_overlay(frame, frame.area(), app);
    panels::trading_action_overlay::render(frame, frame.area(), app);
    render_manual_position_overlay(frame, frame.area(), app);
    render_keymap_overlay(frame, frame.area(), app);
    render_notifications_overlay(frame, frame.area(), app);
}

fn render_main(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    if let Some(maximized) = app.wm.maximized_pane {
        render_pane(frame, area, app, maximized);
    } else {
        let root_node = app.wm.current_workspace().root.clone();
        let emphasized_pane = app.wm.current_workspace().emphasized_pane;
        render_layout_node(frame, area, app, &root_node, emphasized_pane);
    }
}

fn render_layout_node(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &mut App,
    node: &LayoutNode,
    emphasized_pane: Option<PaneId>,
) {
    match node {
        LayoutNode::Pane(pane_id) => {
            render_pane(frame, area, app, *pane_id);
        }
        LayoutNode::Split {
            direction,
            ratios,
            children,
        } => {
            let constraints: Vec<Constraint> = effective_ratios(ratios, children, emphasized_pane)
                .into_iter()
                .map(Constraint::Percentage)
                .collect();
            let layout_dir = match direction {
                SplitDirection::Horizontal => ratatui::layout::Direction::Horizontal,
                SplitDirection::Vertical => ratatui::layout::Direction::Vertical,
            };
            let chunks = Layout::default()
                .direction(layout_dir)
                .constraints(constraints)
                .spacing(match direction {
                    SplitDirection::Horizontal => 0,
                    SplitDirection::Vertical => 1,
                })
                .split(area);

            for (i, child) in children.iter().enumerate() {
                if i < chunks.len() {
                    render_layout_node(frame, chunks[i], app, child, emphasized_pane);
                }
            }
        }
    }
}

fn render_pane(frame: &mut Frame<'_>, area: Rect, app: &mut App, pane_id: PaneId) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let is_focused = app.wm.active_pane == Some(pane_id);
    let chrome = Block::default()
        .borders(Borders::ALL)
        .border_style(if is_focused {
            Style::default()
                .fg(accent_cyan())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(muted_text())
        })
        .style(Style::default().bg(shell_background()));
    let chrome_inner = chrome.inner(area);
    frame.render_widget(chrome, area);

    if chrome_inner.width == 0 || chrome_inner.height == 0 {
        return;
    }

    let title_height = 1;
    let title_bar_area = Rect {
        x: chrome_inner.x,
        y: chrome_inner.y,
        width: chrome_inner.width,
        height: title_height,
    };
    let content_area = if chrome_inner.height > title_height {
        Rect {
            x: chrome_inner.x,
            y: chrome_inner.y.saturating_add(title_height),
            width: chrome_inner.width,
            height: chrome_inner.height.saturating_sub(title_height),
        }
    } else {
        chrome_inner
    };

    render_pane_chrome(frame, title_bar_area, app, pane_id, is_focused);
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
                content_area,
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
            panels::exchanges::render(frame, content_area, &snapshot, app.exchange_list_state())
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
                content_area,
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
        PaneId::Markets => panels::trading_markets::render(frame, content_area, app),
        PaneId::Live => panels::trading_markets::render(frame, content_area, app),
        PaneId::Props => panels::trading_markets::render(frame, content_area, app),
        PaneId::Chart => panels::chart::render(frame, content_area, app),
        PaneId::Intel => panels::intel::render(frame, content_area, app),
        PaneId::Matcher => panels::matcher::render(frame, content_area, app),
        PaneId::Stats => panels::trading_stats::render(
            frame,
            content_area,
            app.snapshot(),
            app.matchbook_account_state(),
        ),
        PaneId::Alerts => panels::alerts::render(frame, content_area, app),
        PaneId::Calculator => panels::calculator::render(frame, content_area, app),
        PaneId::Recorder => panels::recorder::render(frame, content_area, app),
        PaneId::Observability => {
            panels::observability::render(
                frame,
                content_area,
                app,
                app.active_observability_section(),
            );
        }
    }
}

fn render_pane_chrome(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &mut App,
    pane_id: PaneId,
    is_focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let min_label = " 󰖭 ";
    let max_label = if app.wm.maximized_pane == Some(pane_id) {
        " 󰖰 "
    } else {
        " 󰖯 "
    };
    let title = pane_id.title();
    let min_width = min_label.chars().count() as u16;
    let max_width = max_label.chars().count() as u16;
    let controls_width = min_width + 1 + max_width;
    let [title_area, controls_area] = Layout::horizontal([
        Constraint::Min(4),
        Constraint::Length(controls_width.min(area.width)),
    ])
    .areas(area);

    let title_style = if is_focused {
        Style::default()
            .fg(accent_cyan())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(muted_text())
    };

    // Reverting to normal text for titles, as tui-big-text was too large.
    // Instead, just increase padding and ensure clear visual separation.
    frame.render_widget(
        Paragraph::new(Span::styled(format!(" {} ", title), title_style))
            .style(Style::default().bg(shell_background())),
        title_area,
    );

    let min_style = if app.can_minimize_pane(pane_id) {
        Style::default()
            .fg(text_color())
            .bg(panel_background())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(muted_text())
            .add_modifier(Modifier::DIM)
    };
    let max_style = Style::default()
        .fg(text_color())
        .bg(panel_background())
        .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(min_label, min_style),
            Span::raw(" "),
            Span::styled(max_label, max_style),
        ]))
        .alignment(ratatui::layout::Alignment::Right)
        .style(Style::default().bg(shell_background())),
        controls_area,
    );

    let controls_start_x = controls_area.x + controls_area.width.saturating_sub(controls_width);
    if app.can_minimize_pane(pane_id) {
        app.register_pane_minimize_target(
            Rect {
                x: controls_start_x,
                y: controls_area.y,
                width: min_width,
                height: 1,
            },
            pane_id,
        );
    }
    app.register_pane_toggle_maximize_target(
        Rect {
            x: controls_start_x + min_width + 1,
            y: controls_area.y,
            width: max_width,
            height: 1,
        },
        pane_id,
    );
}

fn render_workspace_strip(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let [brand_area, status_area, nav_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let latest_event_label = app.last_event_at_label().unwrap_or("--:--:--");
    let latest_event_spans = if let Some((prefix, seconds)) = latest_event_label.rsplit_once(':') {
        vec![
            Span::styled(format!("{prefix}:"), Style::default().fg(muted_text())),
            Span::styled(seconds.to_string(), Style::default().fg(text_color())),
        ]
    } else {
        vec![Span::styled(
            latest_event_label.to_string(),
            Style::default().fg(text_color()),
        )]
    };

    if brand_area.height > 0 {
        frame.render_widget(
            BigText::builder()
                .pixel_size(PixelSize::Octant)
                .style(
                    Style::default()
                        .fg(accent_cyan())
                        .bg(shell_background())
                        .add_modifier(Modifier::BOLD),
                )
                .lines(vec!["sabi".into()])
                .build(),
            brand_area,
        );
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " latest ",
                Style::default().fg(accent_gold()).bg(panel_background()),
            ),
            Span::raw(" "),
            latest_event_spans[0].clone(),
            latest_event_spans
                .get(1)
                .cloned()
                .unwrap_or_else(|| Span::raw("")),
        ]))
        .style(Style::default().bg(shell_background()))
        .wrap(Wrap { trim: true }),
        status_area,
    );

    let active_label = active_context_label(app);
    let hide_label = " hide ";
    let full_label = if app.wm.maximized_pane.is_some() {
        " restore 󰖯 "
    } else {
        " full  "
    };
    let right_width = (active_label.len() + 3 + hide_label.len() + 1 + full_label.len()) as u16;
    let control_width = right_width.min(nav_area.width);
    let side_width = control_width.min(nav_area.width.saturating_sub(control_width));
    let [left_area, center_area, right_area] = Layout::horizontal([
        Constraint::Length(side_width),
        Constraint::Min(10),
        Constraint::Length(control_width),
    ])
    .areas(nav_area);

    let mut spans = Vec::new();
    let workspace_names = app
        .wm
        .workspaces
        .iter()
        .map(|workspace| workspace.name.clone())
        .collect::<Vec<_>>();
    let workspace_width = workspace_names
        .iter()
        .enumerate()
        .map(|(index, workspace_name)| {
            workspace_name.len() as u16 + 2 + if index > 0 { 3 } else { 0 }
        })
        .sum::<u16>();
    let mut x = center_area
        .x
        .saturating_add(center_area.width.saturating_sub(workspace_width) / 2);
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
                y: nav_area.y,
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
        center_area,
    );

    let (ticker_kind, ticker_body) = app.top_bar_ticker_parts();
    let ticker_label = format!(" {ticker_kind} ");
    let ticker_body_width = left_area
        .width
        .saturating_sub(ticker_label.chars().count() as u16 + 1)
        as usize;
    let ticker_body = ticker_window(
        &ticker_body,
        ticker_body_width,
        ticker_scroll_offset_chars(),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                ticker_label,
                Style::default().fg(accent_blue()).bg(panel_background()),
            ),
            Span::raw(" "),
            Span::styled(ticker_body, Style::default().fg(muted_text())),
        ]))
        .style(Style::default().bg(shell_background()))
        .wrap(Wrap { trim: true }),
        left_area,
    );

    let controls = Paragraph::new(Line::from(vec![
        Span::styled(&active_label, Style::default().fg(text_color())),
        Span::styled(" | ", Style::default().fg(muted_text())),
        Span::styled(
            hide_label,
            if app.can_minimize_active_pane() {
                Style::default()
                    .fg(text_color())
                    .bg(panel_background())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(muted_text())
                    .add_modifier(Modifier::DIM)
            },
        ),
        Span::styled(" ", Style::default().fg(muted_text())),
        Span::styled(
            full_label,
            Style::default()
                .fg(text_color())
                .bg(panel_background())
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .alignment(ratatui::layout::Alignment::Right)
    .style(Style::default().bg(shell_background()));
    frame.render_widget(controls, right_area);

    let controls_start_x = right_area.x + right_area.width.saturating_sub(control_width);
    let labels_offset = (active_label.len() + 3) as u16;

    if app.can_minimize_active_pane() {
        app.register_minimize_active_pane_target(Rect {
            x: controls_start_x + labels_offset,
            y: right_area.y,
            width: hide_label.len() as u16,
            height: 1,
        });
    }
    app.register_toggle_maximize_target(Rect {
        x: controls_start_x + labels_offset + hide_label.len() as u16 + 1,
        y: right_area.y,
        width: full_label.len() as u16,
        height: 1,
    });
}

fn render_footer_line(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let runtime = app.snapshot().runtime.as_ref();
    let fresh = if runtime.map(|summary| summary.stale).unwrap_or(false) {
        "stale"
    } else {
        "fresh"
    };
    let [status_area, error_area] =
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(area);
    let worker = worker_status_label(app);
    let refresh_kind = refresh_kind_label(app);
    let latest_status = footer_status_detail(app, status_area.width);

    let line = Paragraph::new(Line::from(vec![
        Span::styled(worker, Style::default().fg(worker_status_color(app))),
        Span::styled(" | ", Style::default().fg(muted_text())),
        Span::styled(refresh_kind, Style::default().fg(accent_gold())),
        Span::styled(" | ", Style::default().fg(muted_text())),
        Span::styled(
            fresh,
            Style::default().fg(if fresh == "fresh" {
                accent_green()
            } else {
                accent_red()
            }),
        ),
        Span::styled(" | ", Style::default().fg(muted_text())),
        Span::styled(last_refresh_label(app), Style::default().fg(muted_text())),
        Span::styled(" | ", Style::default().fg(muted_text())),
        Span::styled(latest_status, Style::default().fg(text_color())),
    ]))
    .style(Style::default().bg(shell_background()));
    frame.render_widget(line, status_area);

    let error_line = if let Some(entry) = app.latest_problem_notification() {
        let level_color = match entry.level {
            crate::alerts::NotificationLevel::Info => accent_blue(),
            crate::alerts::NotificationLevel::Warning => accent_gold(),
            crate::alerts::NotificationLevel::Critical => accent_red(),
        };
        let issue_count = app.problem_notifications().len();
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{} ", entry.level.label().to_uppercase()),
                Style::default()
                    .fg(level_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{} issue{}",
                    issue_count,
                    if issue_count == 1 { "" } else { "s" }
                ),
                Style::default().fg(text_color()),
            ),
            Span::styled("  [n]", Style::default().fg(muted_text())),
        ]))
        .alignment(ratatui::layout::Alignment::Right)
        .style(Style::default().bg(shell_background()))
    } else {
        Paragraph::new(Line::from(vec![
            Span::styled(
                "OK ",
                Style::default()
                    .fg(accent_green())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("No recent errors", Style::default().fg(muted_text())),
            Span::styled("  [n]", Style::default().fg(muted_text())),
        ]))
        .alignment(ratatui::layout::Alignment::Right)
        .style(Style::default().bg(shell_background()))
    };
    frame.render_widget(error_line, error_area);
}

fn render_minimize_strip(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let panes = app.current_minimized_panes().to_vec();
    if panes.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(" ", Style::default().bg(shell_background()))),
            area,
        );
        return;
    }

    let mut spans = vec![Span::styled(
        " hidden panes, click to restore ",
        Style::default()
            .fg(muted_text())
            .add_modifier(Modifier::DIM),
    )];

    for pane in &panes {
        spans.push(Span::styled(" | ", Style::default().fg(muted_text())));
        spans.push(Span::styled(
            format!(" 󰖰 {} ", pane.title().to_uppercase()),
            Style::default()
                .fg(text_color())
                .bg(panel_background())
                .add_modifier(Modifier::BOLD),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(shell_background())),
        area,
    );

    let mut x = area.x.saturating_add(31);
    for pane in panes {
        x = x.saturating_add(3);
        let width = pane.title().len() as u16 + 5;
        app.register_minimized_pane_target(
            Rect {
                x,
                y: area.y,
                width,
                height: 1,
            },
            pane,
        );
        x = x.saturating_add(width);
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
            Line::raw("Left/Right switch sections"),
            Line::raw("Up/Down navigate inside pane"),
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

fn ticker_scroll_offset_chars() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_millis() / 1000) as usize)
        .unwrap_or(0)
}

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