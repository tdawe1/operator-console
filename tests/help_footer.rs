use color_eyre::Result;
use operator_console::app::App;
use operator_console::domain::{
    ExchangePanelSnapshot, VenueId, VenueStatus, VenueSummary, WorkerStatus, WorkerSummary,
};
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

struct StaticProvider {
    snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for StaticProvider {
    fn handle(&mut self, _request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        Ok(self.snapshot.clone())
    }
}

#[test]
fn help_text_mentions_core_operator_keys() {
    let app = App::default();
    let help = app.help_text();

    assert!(help.contains("q"));
    assert!(help.contains("o"));
    assert!(help.contains("r"));
    assert!(help.contains("v"));
    assert!(help.contains("s"));
    assert!(help.contains("x"));
    assert!(help.contains("enter"));
    assert!(help.contains("esc"));
    assert!(help.contains("u"));
    assert!(help.contains("D"));
    assert!(help.contains("j/k"));
    assert!(help.contains("[/] cycle suggestions"));
}

#[test]
fn footer_renders_status_message_visibly() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: sample_snapshot("Recorder start failed: watcher timed out"),
    })
    .expect("app should load snapshot");
    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| operator_console::ui::render(frame, &mut app))
        .expect("draw ui");

    let buffer = terminal.backend().buffer().clone();
    let area = buffer.area;
    let mut lines = Vec::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buffer.cell((x, y)).expect("cell").symbol());
        }
        lines.push(line);
    }
    let rendered = lines.join("\n");

    assert!(rendered.contains("Recorder start failed: watcher timed out"));
}

#[test]
fn startup_footer_guidance_stays_visible_at_standard_terminal_size() {
    let mut app = App::default();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| operator_console::ui::render(frame, &mut app))
        .expect("draw ui");

    let buffer = terminal.backend().buffer().clone();
    let area = buffer.area;
    let mut lines = Vec::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buffer.cell((x, y)).expect("cell").symbol());
        }
        lines.push(line);
    }
    let rendered = lines.join("\n");

    assert!(rendered.contains("press s for live data."));
    assert!(rendered.contains("o observability"));
    assert!(rendered.contains("v live view"));
    assert!(rendered.contains("s start recorder"));
}

fn sample_snapshot(status_line: &str) -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("bet-recorder"),
            status: WorkerStatus::Error,
            detail: String::from(status_line),
        },
        venues: vec![VenueSummary {
            id: VenueId::Smarkets,
            label: String::from("Smarkets"),
            status: VenueStatus::Error,
            detail: String::from(status_line),
            event_count: 0,
            market_count: 0,
        }],
        selected_venue: Some(VenueId::Smarkets),
        events: Vec::new(),
        markets: Vec::new(),
        preflight: None,
        status_line: String::from(status_line),
        runtime: None,
        account_stats: None,
        open_positions: Vec::new(),
        historical_positions: Vec::new(),
        ledger_pnl_summary: Default::default(),
        other_open_bets: Vec::new(),
        decisions: Vec::new(),
        watch: None,
        tracked_bets: Vec::new(),
        exit_policy: Default::default(),
        exit_recommendations: Vec::new(),
    }
}
