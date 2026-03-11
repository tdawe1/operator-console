use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Frame;

use crate::app::{App, Panel};
use crate::panels;

pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(3),
    ])
    .split(frame.area());

    let titles = ["Dashboard", "Exchanges", "Recorder"];
    let selected = match app.active_panel() {
        Panel::Dashboard => 0,
        Panel::Exchanges => 1,
        Panel::Recorder => 2,
    };

    let header = Tabs::new(titles)
        .select(selected)
        .block(
            Block::default()
                .title("Operator Console")
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(header, layout[0]);

    match app.active_panel() {
        Panel::Dashboard => panels::dashboard::render(frame, layout[1], app.snapshot()),
        Panel::Exchanges => {
            let snapshot = app.snapshot().clone();
            panels::exchanges::render(frame, layout[1], &snapshot, app.exchange_list_state())
        }
        Panel::Recorder => panels::recorder::render(frame, layout[1], app),
    }

    let footer = Paragraph::new(vec![
        Line::raw(app.help_text()),
        Line::raw(app.status_message().to_string()),
    ])
    .block(Block::default().title("Status").borders(Borders::ALL));
    frame.render_widget(footer, layout[2]);
}
