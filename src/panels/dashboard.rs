use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::domain::{ExchangePanelSnapshot, VenueStatus};
use crate::recorder::RecorderStatus;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let layout = Layout::vertical([Constraint::Length(7), Constraint::Min(10)]).split(area);
    let top = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .split(layout[0]);
    let bottom = Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)])
        .split(layout[1]);

    render_block(
        frame,
        top[0],
        "Operator",
        operator_lines(app.snapshot(), app.recorder_status()),
    );
    render_block(frame, top[1], "Trading", trading_lines(app.snapshot()));
    render_block(frame, top[2], "Runtime", runtime_lines(app.snapshot()));
    render_block(frame, bottom[0], "Attention", attention_lines(app));
    render_block(frame, bottom[1], "Modules", module_lines());
}

fn operator_lines(
    snapshot: &ExchangePanelSnapshot,
    recorder_status: &RecorderStatus,
) -> Vec<Line<'static>> {
    let connected_count = snapshot
        .venues
        .iter()
        .filter(|venue| matches!(venue.status, VenueStatus::Connected))
        .count();
    let source_mode = source_mode(snapshot, recorder_status);

    vec![
        Line::raw(format!(
            "Worker: {} [{:?}]",
            snapshot.worker.name, snapshot.worker.status
        )),
        Line::raw(format!(
            "Source mode: {} | Recorder: {:?}",
            source_mode, recorder_status
        )),
        Line::raw(format!(
            "Venues: {} total | {} connected",
            snapshot.venues.len(),
            connected_count
        )),
        Line::raw(format!(
            "Selected venue: {}",
            snapshot
                .selected_venue
                .map(|venue| venue.as_str().to_string())
                .unwrap_or_else(|| String::from("none"))
        )),
    ]
}

fn trading_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
    let account_line = snapshot
        .account_stats
        .as_ref()
        .map(|stats| {
            format!(
                "Balance {:.2} {} | Exposure {:.2}",
                stats.available_balance, stats.currency, stats.exposure
            )
        })
        .unwrap_or_else(|| String::from("No trading account summary loaded"));

    vec![
        Line::raw(account_line),
        Line::raw(format!(
            "Open positions: {} | Open bets: {}",
            snapshot.open_positions.len(),
            snapshot.other_open_bets.len(),
        )),
        Line::raw(format!(
            "Watch groups: {} | Markets: {}",
            snapshot
                .watch
                .as_ref()
                .map(|watch| watch.watch_count)
                .unwrap_or(0),
            snapshot.markets.len(),
        )),
        Line::raw(format!("Active decisions: {}", snapshot.decisions.len())),
        Line::raw(format!("Events in view: {}", snapshot.events.len())),
    ]
}

fn runtime_lines(snapshot: &ExchangePanelSnapshot) -> Vec<Line<'static>> {
    let runtime = snapshot.runtime.as_ref();
    vec![
        Line::raw(format!(
            "Updated: {}",
            runtime
                .map(|summary| summary.updated_at.as_str())
                .unwrap_or("unknown")
        )),
        Line::raw(format!(
            "Source: {} | Decisions: {}",
            runtime
                .map(|summary| summary.source.as_str())
                .unwrap_or("snapshot"),
            runtime.map(|summary| summary.decision_count).unwrap_or(0),
        )),
        Line::raw(format!(
            "Iteration: {} | Stale: {}",
            runtime
                .and_then(|summary| summary.watcher_iteration)
                .map(|value| value.to_string())
                .unwrap_or_else(|| String::from("-")),
            runtime
                .map(|summary| yes_no(summary.stale))
                .unwrap_or("unknown"),
        )),
        Line::raw(snapshot.status_line.clone()),
    ]
}

fn attention_lines(app: &App) -> Vec<Line<'static>> {
    let runtime = app.snapshot().runtime.as_ref();
    let mut rows = Vec::new();

    if app.snapshot().worker.detail.contains("Stub demo") {
        rows.push(Line::raw("Stub/demo data"));
    }

    if matches!(
        app.snapshot().worker.status,
        crate::domain::WorkerStatus::Error
    ) {
        rows.push(Line::raw("Worker error"));
    }
    if runtime.map(|summary| summary.stale).unwrap_or(false) {
        rows.push(Line::raw("Snapshot stale"));
    }
    if app
        .snapshot()
        .watch
        .as_ref()
        .map(|watch| watch.watch_count)
        .unwrap_or(0)
        == 0
    {
        rows.push(Line::raw("No watch groups"));
    }
    let urgent_decisions = app
        .snapshot()
        .decisions
        .iter()
        .filter(|decision| {
            decision.status == "take_profit_ready" || decision.status == "stop_loss_ready"
        })
        .count();
    if urgent_decisions > 0 {
        rows.push(Line::raw(format!(
            "{urgent_decisions} action-ready decision(s)"
        )));
    }
    if rows.is_empty() {
        rows.push(Line::raw("No active alerts"));
    }
    rows
}

fn module_lines() -> Vec<Line<'static>> {
    vec![
        Line::raw("Dashboard"),
        Line::raw("Trading"),
        Line::raw("Banking"),
        Line::raw("Observability"),
    ]
}

fn source_mode(snapshot: &ExchangePanelSnapshot, recorder_status: &RecorderStatus) -> &'static str {
    if snapshot.worker.detail.contains("Stub demo") {
        "stub/demo"
    } else if *recorder_status == RecorderStatus::Running || snapshot.worker.name == "bet-recorder"
    {
        "recorder-backed"
    } else {
        "provider-backed"
    }
}

fn render_block(frame: &mut Frame<'_>, area: Rect, title: &str, rows: Vec<Line<'static>>) {
    let body = Paragraph::new(rows)
        .block(
            Block::default()
                .title(Span::styled(
                    title.to_string(),
                    Style::default()
                        .fg(block_accent(title))
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .style(
                    Style::default()
                        .bg(Color::Rgb(16, 22, 30))
                        .fg(Color::Rgb(234, 240, 246)),
                )
                .border_style(Style::default().fg(Color::Rgb(74, 88, 104))),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn block_accent(title: &str) -> Color {
    match title {
        "Operator" => Color::Rgb(109, 180, 255),
        "Trading" => Color::Rgb(94, 234, 212),
        "Runtime" => Color::Rgb(134, 239, 172),
        "Attention" => Color::Rgb(248, 113, 113),
        "Modules" => Color::Rgb(244, 143, 177),
        _ => Color::Rgb(234, 240, 246),
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
fn dashboard_text(snapshot: &ExchangePanelSnapshot) -> Vec<String> {
    operator_lines(snapshot, &RecorderStatus::Disabled)
        .into_iter()
        .chain(trading_lines(snapshot))
        .chain(runtime_lines(snapshot))
        .map(|line| line.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::dashboard_text;
    use crate::domain::{
        AccountStats, ExchangePanelSnapshot, RuntimeSummary, VenueId, VenueStatus, VenueSummary,
        WorkerStatus, WorkerSummary,
    };

    #[test]
    fn dashboard_summary_mentions_trading_and_runtime_context() {
        let snapshot = ExchangePanelSnapshot {
            worker: WorkerSummary {
                name: String::from("bet-recorder"),
                status: WorkerStatus::Ready,
                detail: String::from("live"),
            },
            venues: vec![VenueSummary {
                id: VenueId::Smarkets,
                label: String::from("Smarkets"),
                status: VenueStatus::Connected,
                detail: String::from("watching"),
                event_count: 2,
                market_count: 3,
            }],
            selected_venue: Some(VenueId::Smarkets),
            status_line: String::from("watcher healthy"),
            runtime: Some(RuntimeSummary {
                updated_at: String::from("2026-03-11T14:00:00Z"),
                source: String::from("watcher-state"),
                decision_count: 2,
                watcher_iteration: Some(8),
                stale: false,
            }),
            account_stats: Some(AccountStats {
                available_balance: 144.5,
                exposure: 23.0,
                unrealized_pnl: 1.2,
                currency: String::from("GBP"),
            }),
            open_positions: vec![],
            historical_positions: vec![],
            other_open_bets: vec![],
            decisions: vec![],
            markets: vec![],
            events: vec![],
            preflight: None,
            watch: None,
            tracked_bets: vec![],
            exit_policy: Default::default(),
            exit_recommendations: vec![],
        };

        let text = dashboard_text(&snapshot);

        assert!(text
            .iter()
            .any(|line| line.contains("Worker: bet-recorder")));
        assert!(text.iter().any(|line| line.contains("Balance 144.50 GBP")));
        assert!(text.iter().any(|line| line.contains("watcher-state")));
    }
}
