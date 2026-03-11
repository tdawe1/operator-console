use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let config = app.recorder_config();
    let body = Paragraph::new(vec![
        Line::raw(format!("Status: {:?}", app.recorder_status())),
        Line::raw(format!("Command: {}", config.command.display())),
        Line::raw(format!("Run Dir: {}", config.run_dir.display())),
        Line::raw(format!("Session: {}", config.session)),
        Line::raw(format!("Interval: {}s", config.interval_seconds)),
        Line::raw(format!(
            "Targets: commission {} | profit {} | stop {}",
            config.commission_rate, config.target_profit, config.stop_loss
        )),
        Line::raw(""),
        Line::raw("s start recorder"),
        Line::raw("x stop recorder"),
        Line::raw("r refresh exchanges from current provider"),
    ])
    .block(Block::default().title("Recorder").borders(Borders::ALL))
    .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}
