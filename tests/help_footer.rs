use color_eyre::Result;
use operator_console::app::{App, Panel};
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

    assert!(help.contains("?"));
    assert!(help.contains("n"));
    assert!(help.contains("q"));
    assert!(help.contains("o"));
    assert!(help.contains("r"));
    assert!(help.contains("R"));
    assert!(help.contains("v"));
    assert!(help.contains("s"));
    assert!(help.contains("x"));
    assert!(help.contains("enter"));
    assert!(help.contains("esc"));
    assert!(help.contains("u"));
    assert!(help.contains("D"));
    assert!(help.contains("h/j/k/l panes"));
    assert!(help.contains("alt+1-3 workspaces"));
    assert!(help.contains("ctrl+left/right"));
    assert!(help.contains("[/] cycle sport or suggestions"));
}

#[test]
fn status_bar_renders_latest_timestamp_and_compact_error_summary() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: sample_snapshot("Recorder start failed: watcher timed out"),
    })
    .expect("app should load snapshot");
    app.set_active_panel(Panel::Observability);
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

    assert!(rendered.contains("latest"));
    assert!(rendered.contains("Recorder start failed"));
    assert!(
        rendered.contains("No recent errors")
            || rendered.contains("issue")
            || rendered.contains("[n]")
    );
}

#[test]
fn keymap_overlay_renders_guidance_when_toggled() {
    let mut app = App::default();
    app.toggle_keymap_overlay();
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
    assert!(rendered.contains("Keymap"));
    assert!(rendered.contains("? keymap"));
    assert!(rendered.contains("n error console"));
    assert!(rendered.contains("Controls"));
    assert!(rendered.contains("R live"));
    assert!(rendered.contains("Data"));
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
        recorder_bundle: None,
        recorder_events: Vec::new(),
        transport_summary: None,
        transport_events: Vec::new(),
        tracked_bets: Vec::new(),
        exit_policy: Default::default(),
        exit_recommendations: Vec::new(),
        external_quotes: Vec::new(),
        external_live_events: Vec::new(),
        horse_matcher: None,
    }
}
