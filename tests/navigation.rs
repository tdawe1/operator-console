use color_eyre::Result;
use crossterm::event::KeyCode;
use operator_console::app::{App, Panel, TradingSection};
use operator_console::domain::{
    ExchangePanelSnapshot, OpenPositionRow, OtherOpenBetRow, VenueId, VenueStatus, VenueSummary,
    WorkerStatus, WorkerSummary,
};
use operator_console::provider::{ExchangeProvider, ProviderRequest};

struct StaticProvider {
    snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for StaticProvider {
    fn handle(&mut self, _request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        Ok(self.snapshot.clone())
    }
}

#[test]
fn app_defaults_to_trading_panel() {
    let app = App::default();

    assert_eq!(app.active_panel(), Panel::Trading);
}

#[test]
fn observability_toggle_swaps_between_trading_and_observability() {
    let mut app = App::default();

    app.handle_key(KeyCode::Char('o'));
    assert_eq!(app.active_panel(), Panel::Observability);

    app.handle_key(KeyCode::Char('o'));
    assert_eq!(app.active_panel(), Panel::Trading);
}

#[test]
fn panel_helpers_toggle_between_trading_and_observability() {
    let mut app = App::default();

    app.next_panel();
    assert_eq!(app.active_panel(), Panel::Observability);

    app.previous_panel();
    assert_eq!(app.active_panel(), Panel::Trading);
}

#[test]
fn trading_section_navigation_cycles_inside_trading_module() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);

    assert_eq!(app.active_trading_section(), TradingSection::Accounts);

    app.next_section();
    assert_eq!(app.active_trading_section(), TradingSection::Positions);

    app.previous_section();
    assert_eq!(app.active_trading_section(), TradingSection::Accounts);
}

#[test]
fn tab_switches_positions_focus_to_historical_when_history_exists() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Positions);

    assert_eq!(app.positions_focus().label(), "Active");

    app.handle_key(KeyCode::Tab);

    assert_eq!(app.positions_focus().label(), "Historical");
}

#[test]
fn historical_focus_uses_historical_selection_state() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Positions);
    app.handle_key(KeyCode::Tab);

    assert_eq!(app.selected_historical_position_row(), Some(0));

    app.handle_key(KeyCode::Down);

    assert_eq!(app.selected_historical_position_row(), Some(1));
    assert_eq!(app.selected_open_position_row(), Some(0));
}

#[test]
fn j_and_k_follow_the_same_navigation_paths_as_arrows() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Positions);
    app.handle_key(KeyCode::Tab);

    assert_eq!(app.selected_historical_position_row(), Some(0));

    app.handle_key(KeyCode::Char('j'));
    assert_eq!(app.selected_historical_position_row(), Some(1));

    app.handle_key(KeyCode::Char('k'));
    assert_eq!(app.selected_historical_position_row(), Some(0));
}

#[test]
fn v_toggles_live_view_overlay_in_positions() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Positions);

    assert!(!app.live_view_overlay_visible());

    app.handle_key(KeyCode::Char('v'));
    assert!(app.live_view_overlay_visible());

    app.handle_key(KeyCode::Esc);
    assert!(!app.live_view_overlay_visible());
    assert!(app.is_running());
}

fn positions_snapshot() -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("bet-recorder"),
            status: WorkerStatus::Ready,
            detail: String::from("live"),
        },
        venues: vec![VenueSummary {
            id: VenueId::Smarkets,
            label: String::from("Smarkets"),
            status: VenueStatus::Connected,
            detail: String::from("ready"),
            event_count: 2,
            market_count: 2,
        }],
        selected_venue: Some(VenueId::Smarkets),
        events: Vec::new(),
        markets: Vec::new(),
        preflight: None,
        status_line: String::from("snapshot"),
        runtime: None,
        account_stats: None,
        open_positions: vec![sample_row("Active 0"), sample_row("Active 1")],
        historical_positions: vec![sample_row("Hist 0"), sample_row("Hist 1")],
        ledger_pnl_summary: Default::default(),
        other_open_bets: vec![OtherOpenBetRow {
            venue: String::from("bet365"),
            event: String::from("Active 0"),
            label: String::from("Selection"),
            market: String::from("90 Minutes"),
            side: String::from("back"),
            odds: 2.375,
            stake: 10.0,
            status: String::from("cash_out"),
            current_cashout_value: Some(16.16),
            supports_cash_out: true,
        }],
        decisions: Vec::new(),
        watch: None,
        tracked_bets: Vec::new(),
        exit_policy: Default::default(),
        exit_recommendations: Vec::new(),
    }
}

fn sample_row(event: &str) -> OpenPositionRow {
    OpenPositionRow {
        event: event.to_string(),
        event_status: String::from("Settled"),
        event_url: String::new(),
        contract: String::from("Selection"),
        market: String::from("Match Odds"),
        status: String::from("matched"),
        market_status: String::from("settled"),
        is_in_play: false,
        price: 2.0,
        stake: 5.0,
        liability: 5.0,
        current_value: 0.0,
        pnl_amount: 0.0,
        current_back_odds: Some(2.0),
        current_implied_probability: Some(0.5),
        current_implied_percentage: Some(50.0),
        current_buy_odds: Some(2.0),
        current_buy_implied_probability: Some(0.5),
        current_sell_odds: None,
        current_sell_implied_probability: None,
        current_score: String::new(),
        current_score_home: None,
        current_score_away: None,
        live_clock: String::from("2026-03-18T12:00:00Z"),
        can_trade_out: false,
    }
}
