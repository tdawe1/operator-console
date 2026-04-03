use crossterm::event::KeyCode;
use operator_console::app::{App, OddsMatcherFocus, Panel, TradingSection};
use operator_console::domain::{ExchangePanelSnapshot, WorkerStatus, WorkerSummary};
use operator_console::horse_matcher::HorseMatcherField;
use operator_console::oddsmatcher::{
    BetSlipRef, BookmakerActive, BookmakerSummary, GroupSummary, LayLeg, OddsMatcherRow, PriceLeg,
    SportSummary,
};
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use operator_console::recorder::{RecorderConfig, RecorderStatus, RecorderSupervisor};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

struct StaticProvider {
    snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for StaticProvider {
    fn handle(&mut self, _request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot> {
        Ok(self.snapshot.clone())
    }
}

struct DisabledSupervisor;

impl RecorderSupervisor for DisabledSupervisor {
    fn start(&mut self, _config: &RecorderConfig) -> color_eyre::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> color_eyre::Result<()> {
        Ok(())
    }

    fn poll_status(&mut self) -> RecorderStatus {
        RecorderStatus::Disabled
    }
}

#[test]
fn trading_navigation_reaches_matcher_section_and_can_switch_to_horse_view() {
    let (_temp_dir, mut app) = horse_matcher_app();
    app.set_active_panel(Panel::Trading);

    while app.active_trading_section() != TradingSection::Matcher {
        app.next_section();
    }
    app.cycle_matcher_view(true);

    assert_eq!(app.active_trading_section(), TradingSection::Matcher);
}

#[test]
fn horse_matcher_filters_are_editable_from_the_app() {
    let (temp_dir, mut app) = horse_matcher_app();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Matcher);
    app.cycle_matcher_view(true);

    app.handle_key(KeyCode::Left);
    assert_eq!(app.horse_matcher_focus(), OddsMatcherFocus::Filters);

    while app.horse_matcher_selected_field() != HorseMatcherField::Limit {
        app.handle_key(KeyCode::Down);
    }

    app.handle_key(KeyCode::Enter);
    assert!(app.horse_matcher_is_editing());
    app.handle_key(KeyCode::Backspace);
    app.handle_key(KeyCode::Backspace);
    app.handle_key(KeyCode::Char('5'));
    app.handle_key(KeyCode::Char('0'));
    app.handle_key(KeyCode::Enter);

    assert_eq!(app.horse_matcher_query().limit, 50);

    let (saved_query, _) = operator_console::horse_matcher::load_query_or_default(
        &temp_dir.path().join("horsematcher.json"),
    )
    .expect("load saved query");
    assert_eq!(saved_query.limit, 50);
}

#[test]
fn horse_matcher_result_can_seed_calculator() {
    let (_temp_dir, mut app) = horse_matcher_app();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Matcher);
    app.cycle_matcher_view(true);
    let rows = sample_rows();
    let row = rows[0].clone();
    app.replace_horse_matcher_rows(rows, String::from("Loaded horse rows."));

    app.handle_key(KeyCode::Enter);

    assert_eq!(app.active_trading_section(), TradingSection::Calculator);
    assert_eq!(app.calculator_back_odds(), row.back.odds);
    assert_eq!(app.calculator_lay_odds(), row.lay.odds);
}

#[test]
fn horse_matcher_place_hotkey_opens_trading_action_overlay() {
    let (_temp_dir, mut app) = horse_matcher_app();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Matcher);
    app.cycle_matcher_view(true);
    app.replace_horse_matcher_rows(sample_rows(), String::from("Loaded horse rows."));

    app.handle_key(KeyCode::Char('p'));

    let overlay = app
        .trading_action_overlay()
        .expect("horse matcher place hotkey should open overlay");
    assert_eq!(overlay.seed.selection_name, "Desert Hero");
    assert_eq!(
        overlay.seed.source,
        operator_console::trading_actions::TradingActionSource::HorseMatcher
    );
}

#[test]
fn horse_matcher_panel_renders_core_sections() {
    let (_temp_dir, mut app) = horse_matcher_app();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Matcher);
    app.cycle_matcher_view(true);
    app.replace_horse_matcher_rows(sample_rows(), String::from("Loaded horse rows."));

    let backend = TestBackend::new(160, 36);
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

    assert!(rendered.contains("HorseMatcher"));
    assert!(rendered.contains("Racing Rows"));
    assert!(rendered.contains("Coverage"));
    assert!(rendered.contains("Details"));
}

fn horse_matcher_app() -> (tempfile::TempDir, App) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let app = App::with_dependencies_and_storage_matcher_paths(
        Box::new(StaticProvider {
            snapshot: sample_snapshot("Stub dashboard"),
        }),
        Box::new(|| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Stub dashboard"),
            }) as Box<dyn ExchangeProvider + Send>
        }),
        Box::new(|_| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Recorder dashboard"),
            }) as Box<dyn ExchangeProvider + Send>
        }),
        Box::new(DisabledSupervisor),
        RecorderConfig::default(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
        temp_dir.path().join("oddsmatcher.json"),
        temp_dir.path().join("horsematcher.json"),
    )
    .expect("app");

    (temp_dir, app)
}

fn sample_snapshot(status_line: &str) -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("provider"),
            status: WorkerStatus::Ready,
            detail: String::from("ok"),
        },
        status_line: String::from(status_line),
        ..ExchangePanelSnapshot::empty()
    }
}

fn sample_rows() -> Vec<OddsMatcherRow> {
    vec![sample_row(
        "race-1",
        "Desert Hero",
        "Cheltenham 15:20",
        5.2,
        5.4,
        96.4,
        200.0,
    )]
}

fn sample_row(
    id: &str,
    selection_name: &str,
    event_name: &str,
    back_odds: f64,
    lay_odds: f64,
    rating: f64,
    liquidity: f64,
) -> OddsMatcherRow {
    OddsMatcherRow {
        event_name: String::from(event_name),
        id: String::from(id),
        start_at: String::from("2026-03-19T15:20:00Z"),
        selection_id: String::from("runner-1"),
        market_id: String::from("market-1"),
        event_id: String::from("event-1"),
        back: PriceLeg {
            updated_at: Some(String::from("2026-03-19T09:00:00Z")),
            odds: back_odds,
            fetched_at: Some(String::from("2026-03-19T09:00:00Z")),
            deep_link: Some(String::from("https://bookie.example/race-1")),
            bookmaker: BookmakerSummary {
                active: BookmakerActive::Bool(true),
                code: String::from("betfred"),
                display_name: String::from("Betfred"),
                id: String::from("bookie-1"),
                logo: Some(String::new()),
            },
        },
        lay: LayLeg {
            bookmaker: BookmakerSummary {
                active: BookmakerActive::Bool(true),
                code: String::from("smarketsexchange"),
                display_name: String::from("Smarkets"),
                id: String::from("exchange-1"),
                logo: Some(String::new()),
            },
            deep_link: Some(String::from("https://exchange.example/race-1")),
            fetched_at: Some(String::from("2026-03-19T09:00:00Z")),
            updated_at: Some(String::from("2026-03-19T09:00:00Z")),
            odds: lay_odds,
            liquidity: Some(liquidity),
            bet_slip: Some(BetSlipRef {
                market_id: String::from("m-1"),
                selection_id: String::from("s-1"),
            }),
        },
        event_group: GroupSummary {
            display_name: String::from("Cheltenham"),
            id: String::from("group-1"),
            source_name: Some(String::from("Cheltenham")),
            sport: Some(String::from("horse-racing")),
        },
        market_group: GroupSummary {
            display_name: String::from("Win Market"),
            id: String::from("market-group-1"),
            source_name: None,
            sport: Some(String::from("horse-racing")),
        },
        market_name: String::from("Win"),
        rating,
        selection_name: String::from(selection_name),
        snr: Some(0.0),
        sport: SportSummary {
            display_name: String::from("Horse Racing"),
            id: String::from("horse-racing"),
        },
        bet_request_id: Some(String::from("req-1")),
    }
}
