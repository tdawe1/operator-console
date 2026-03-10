use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::{Frame, Terminal};

use crate::domain::WatchSnapshot;
use crate::provider::{WatchProvider, WatchRequest};

pub struct App<P> {
    provider: P,
    request: WatchRequest,
    snapshot: WatchSnapshot,
    running: bool,
    status_line: String,
}

impl<P> App<P> {
    pub fn new(request: WatchRequest, provider: P) -> Self {
        Self {
            provider,
            request,
            snapshot: WatchSnapshot::default(),
            running: true,
            status_line: String::from("Press r to refresh. Press q or Esc to quit."),
        }
    }

    pub fn snapshot(&self) -> &WatchSnapshot {
        &self.snapshot
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}

impl<P: WatchProvider> App<P> {
    pub fn refresh(&mut self) -> Result<()> {
        self.snapshot = self.provider.load_watch_snapshot(&self.request)?;
        self.status_line = format!(
            "Loaded {} grouped watches from {} positions.",
            self.snapshot.watch_count, self.snapshot.position_count
        );
        Ok(())
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        while self.running {
            terminal.draw(|frame| self.render(frame))?;

            if event::poll(Duration::from_millis(250))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => self.running = false,
                        KeyCode::Char('r') => {
                            if let Err(error) = self.refresh() {
                                self.status_line = format!("Refresh failed: {error}");
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let layout = Layout::vertical([
            Constraint::Length(5),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(frame.area());

        let summary = Paragraph::new(format!(
            "Payload: {}\nWatches: {}  Positions: {}  Profit target: {:.2}  Stop loss: {:.2}",
            self.request.payload_path.display(),
            self.snapshot.watch_count,
            self.snapshot.position_count,
            self.snapshot.target_profit,
            self.snapshot.stop_loss,
        ))
        .block(
            Block::default()
                .title("Operator Watch")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });
        frame.render_widget(summary, layout[0]);

        let header = Row::new([
            "Contract", "Market", "Pos", "Stake", "PnL", "Profit", "Stop",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));

        let rows = self.snapshot.watches.iter().map(|watch| {
            Row::new([
                Cell::from(watch.contract.clone()),
                Cell::from(watch.market.clone()),
                Cell::from(watch.position_count.to_string()),
                Cell::from(format!("{:.2}", watch.total_stake)),
                Cell::from(format!("{:.2}", watch.current_pnl_amount)),
                Cell::from(format!("{:.2}", watch.profit_take_back_odds)),
                Cell::from(format!("{:.2}", watch.stop_loss_back_odds)),
            ])
        });

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(18),
                Constraint::Percentage(28),
                Constraint::Length(5),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(8),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .title("Trade-Out Thresholds")
                .borders(Borders::ALL),
        );
        frame.render_widget(table, layout[1]);

        let status = Paragraph::new(self.status_line.clone())
            .block(Block::default().title("Status").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        frame.render_widget(status, layout[2]);
    }
}
