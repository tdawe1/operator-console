use crossterm::event::KeyCode;
use operator_console::app::{App, OddsMatcherFocus, Panel, TradingSection};
use operator_console::domain::{ExchangePanelSnapshot, WorkerStatus, WorkerSummary};
use operator_console::oddsmatcher::{
    BetSlipRef, BookmakerActive, BookmakerSummary, GroupSummary, LayLeg, OddsMatcherField,
    OddsMatcherRow, PriceLeg, SportSummary,
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
fn trading_navigation_reaches_matcher_section_with_odds_view() {
    let (_temp_dir, mut app) = oddsmatcher_app();
    app.set_active_panel(Panel::Trading);

    app.next_section();
    app.next_section();
    app.next_section();
    app.next_section();

    assert_eq!(app.active_trading_section(), TradingSection::Matcher);
}

#[test]
fn oddsmatcher_selection_moves_with_arrow_keys() {
    let (_temp_dir, mut app) = oddsmatcher_app();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Matcher);
    app.replace_oddsmatcher_rows(sample_rows(), String::from("Loaded test rows."));

    let first = app
        .selected_oddsmatcher_row()
        .expect("first row")
        .id
        .clone();
    app.handle_key(KeyCode::Down);
    let second = app
        .selected_oddsmatcher_row()
        .expect("selected row")
        .id
        .clone();

    assert_ne!(first, second);
}

#[test]
fn oddsmatcher_filters_are_editable_from_the_app() {
    let (temp_dir, mut app) = oddsmatcher_app();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Matcher);

    app.handle_key(KeyCode::Left);
    assert_eq!(app.oddsmatcher_focus(), OddsMatcherFocus::Filters);

    while app.oddsmatcher_selected_field() != OddsMatcherField::Limit {
        app.handle_key(KeyCode::Down);
    }

    app.handle_key(KeyCode::Enter);
    assert!(app.oddsmatcher_is_editing());
    app.handle_key(KeyCode::Char('2'));
    app.handle_key(KeyCode::Char('5'));
    app.handle_key(KeyCode::Enter);

    assert_eq!(app.oddsmatcher_query().limit, 25);

    let (saved_query, _) = operator_console::oddsmatcher::load_query_or_default(
        &temp_dir.path().join("oddsmatcher.json"),
    )
    .expect("load saved query");
    assert_eq!(saved_query.limit, 25);
}

#[test]
fn oddsmatcher_panel_renders_filter_sidebar() {
    let (_temp_dir, mut app) = oddsmatcher_app();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Matcher);

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

    assert!(rendered.contains("OddsMatcher"));
    assert!(rendered.contains("Market Type"));
    assert!(rendered.contains("Timeframe"));
    assert!(rendered.contains("Offers"));
    assert!(rendered.contains("Selection"));
    assert!(rendered.contains("Availability"));
    assert!(rendered.contains("Date"));
    assert!(rendered.contains("Time"));
}

#[test]
fn oddsmatcher_result_can_seed_calculator() {
    let (_temp_dir, mut app) = oddsmatcher_app();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Matcher);
    let rows = sample_rows();
    let row = rows[0].clone();
    app.replace_oddsmatcher_rows(rows, String::from("Loaded test rows."));

    app.handle_key(KeyCode::Enter);

    assert_eq!(app.active_trading_section(), TradingSection::Calculator);
    assert_eq!(app.calculator_back_odds(), row.back.odds);
    assert_eq!(app.calculator_lay_odds(), row.lay.odds);
    assert_eq!(
        app.calculator_source()
            .map(|source| source.selection_name.clone()),
        Some(row.selection_name)
    );
}

#[test]
fn oddsmatcher_place_hotkey_opens_trading_action_overlay() {
    let (_temp_dir, mut app) = oddsmatcher_app();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Matcher);
    app.replace_oddsmatcher_rows(sample_rows(), String::from("Loaded test rows."));

    app.handle_key(KeyCode::Char('p'));

    let overlay = app
        .trading_action_overlay()
        .expect("oddsmatcher place hotkey should open overlay");
    assert_eq!(overlay.seed.selection_name, "Arsenal");
    assert_eq!(overlay.side.label(), "Sell");
}

#[test]
#[ignore = "hits live OddsMatcher API"]
fn oddsmatcher_refresh_smoke_loads_live_rows() {
    let (_temp_dir, mut app) = oddsmatcher_app();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Matcher);

    app.handle_key(KeyCode::Char('r'));

    assert!(!app.oddsmatcher_rows().is_empty());
    assert!(
        app.status_message().contains("OddsMatcher") || app.status_message().contains("Loaded")
    );
}

fn oddsmatcher_app() -> (tempfile::TempDir, App) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let app = App::with_dependencies_and_storage_paths(
        Box::new(StaticProvider {
            snapshot: sample_snapshot("Stub dashboard"),
        }),
        Box::new(|| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Stub dashboard"),
            }) as Box<dyn ExchangeProvider>
        }),
        Box::new(|_| {
            Box::new(StaticProvider {
                snapshot: sample_snapshot("Recorder dashboard"),
            }) as Box<dyn ExchangeProvider>
        }),
        Box::new(DisabledSupervisor),
        RecorderConfig::default(),
        temp_dir.path().join("recorder.json"),
        String::from("test"),
        temp_dir.path().join("oddsmatcher.json"),
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
    vec![
        sample_row(
            "match-1",
            "Arsenal",
            "Arsenal v Everton",
            2.84,
            2.9,
            97.4,
            120.0,
        ),
        sample_row(
            "match-2",
            "Liverpool",
            "Liverpool v Chelsea",
            3.10,
            3.2,
            96.9,
            210.0,
        ),
    ]
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
        start_at: String::from("2026-03-18T20:00:00Z"),
        selection_id: format!("{id}-selection"),
        market_id: format!("{id}-market"),
        event_id: format!("{id}-event"),
        back: PriceLeg {
            updated_at: Some(String::from("2026-03-18T19:55:00Z")),
            odds: back_odds,
            fetched_at: Some(String::from("2026-03-18T19:55:05Z")),
            deep_link: Some(String::from("https://bookmaker.test/bet")),
            bookmaker: BookmakerSummary {
                active: BookmakerActive::Bool(true),
                code: String::from("betvictor"),
                display_name: String::from("BetVictor"),
                id: String::from("bookmaker-1"),
                logo: None,
            },
        },
        lay: LayLeg {
            bookmaker: BookmakerSummary {
                active: BookmakerActive::Bool(true),
                code: String::from("smarketsexchange"),
                display_name: String::from("Smarkets"),
                id: String::from("exchange-1"),
                logo: None,
            },
            deep_link: Some(String::from("https://exchange.test/lay")),
            fetched_at: Some(String::from("2026-03-18T19:55:06Z")),
            updated_at: Some(String::from("2026-03-18T19:55:01Z")),
            odds: lay_odds,
            liquidity: Some(liquidity),
            bet_slip: Some(BetSlipRef {
                market_id: format!("{id}-betslip-market"),
                selection_id: format!("{id}-betslip-selection"),
            }),
        },
        event_group: GroupSummary {
            display_name: String::from("Premier League"),
            id: String::from("event-group-1"),
            source_name: Some(String::from("soccer")),
            sport: Some(String::from("soccer")),
        },
        market_group: GroupSummary {
            display_name: String::from("Match Odds"),
            id: String::from("market-group-1"),
            source_name: Some(String::from("match-odds")),
            sport: Some(String::from("soccer")),
        },
        market_name: String::from("Match Odds"),
        rating,
        selection_name: String::from(selection_name),
        snr: None,
        sport: SportSummary {
            display_name: String::from("Soccer"),
            id: String::from("soccer"),
        },
        bet_request_id: Some(format!("{id}-request")),
    }
}
