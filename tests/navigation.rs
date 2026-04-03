use color_eyre::Result;
use crossterm::event::KeyCode;
use operator_console::app::{App, Panel, TradingSection};
use operator_console::domain::{
    ExchangePanelSnapshot, OpenPositionRow, OtherOpenBetRow, VenueId, VenueStatus, VenueSummary,
    WorkerStatus, WorkerSummary,
};
use operator_console::market_intel::{
    MarketIntelDashboard, MarketIntelSourceId, MarketOpportunityRow, MarketQuoteComparisonRow,
    OpportunityKind, SourceHealth, SourceHealthStatus, SourceLoadMode,
};
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use std::time::Duration;

struct StaticProvider {
    snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for StaticProvider {
    fn handle(&mut self, _request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        Ok(self.snapshot.clone())
    }
}

fn sample_market_intel_dashboard() -> MarketIntelDashboard {
    let executable_market = MarketOpportunityRow {
        source: MarketIntelSourceId::Oddsentry,
        kind: OpportunityKind::Market,
        id: String::from("intel-executable"),
        sport: String::from("Soccer"),
        competition_name: String::from("Premier League"),
        event_id: String::from("event-1"),
        event_name: String::from("Arsenal v Everton"),
        market_name: String::from("Match Odds"),
        selection_name: String::from("Arsenal"),
        venue: String::from("bet365"),
        secondary_venue: String::from("smarkets"),
        event_url: String::from("https://oddsentry.example/events/arsenal-everton"),
        deep_link_url: String::from("https://smarkets.example/events/arsenal-everton"),
        quotes: vec![
            MarketQuoteComparisonRow {
                source: MarketIntelSourceId::Oddsentry,
                event_id: String::from("event-1"),
                event_name: String::from("Arsenal v Everton"),
                market_name: String::from("Match Odds"),
                selection_name: String::from("Arsenal"),
                venue: String::from("bet365"),
                price: Some(2.42),
                fair_price: Some(2.26),
                event_url: String::from("https://oddsentry.example/events/arsenal-everton"),
                deep_link_url: String::from("https://bet365.example/events/arsenal-everton"),
                updated_at: String::from("2026-04-03T11:24:00Z"),
                ..MarketQuoteComparisonRow::default()
            },
            MarketQuoteComparisonRow {
                source: MarketIntelSourceId::Oddsentry,
                event_id: String::from("event-1"),
                event_name: String::from("Arsenal v Everton"),
                market_name: String::from("Match Odds"),
                selection_name: String::from("Arsenal"),
                venue: String::from("smarkets"),
                side: String::from("lay"),
                price: Some(2.30),
                event_url: String::from("https://oddsentry.example/events/arsenal-everton"),
                deep_link_url: String::from("https://smarkets.example/events/arsenal-everton"),
                updated_at: String::from("2026-04-03T11:24:00Z"),
                ..MarketQuoteComparisonRow::default()
            },
        ],
        notes: vec![String::from("routeable")],
        ..MarketOpportunityRow::default()
    };

    let missing_lay_value = MarketOpportunityRow {
        source: MarketIntelSourceId::FairOdds,
        kind: OpportunityKind::Value,
        id: String::from("intel-missing-lay"),
        sport: String::from("Basketball"),
        competition_name: String::from("NBA"),
        event_id: String::from("event-2"),
        event_name: String::from("Mavericks v Suns"),
        market_name: String::from("Moneyline"),
        selection_name: String::from("Mavericks"),
        venue: String::from("fanduel"),
        event_url: String::from("https://fairodds.example/value/mavericks-suns"),
        quotes: vec![MarketQuoteComparisonRow {
            source: MarketIntelSourceId::FairOdds,
            event_id: String::from("event-2"),
            event_name: String::from("Mavericks v Suns"),
            market_name: String::from("Moneyline"),
            selection_name: String::from("Mavericks"),
            venue: String::from("fanduel"),
            price: Some(2.34),
            fair_price: Some(2.15),
            event_url: String::from("https://fairodds.example/value/mavericks-suns"),
            updated_at: String::from("2026-04-03T11:24:30Z"),
            ..MarketQuoteComparisonRow::default()
        }],
        notes: vec![String::from("missing lay")],
        ..MarketOpportunityRow::default()
    };

    MarketIntelDashboard {
        refreshed_at: String::from("2026-04-03T11:24:30Z"),
        status_line: String::from("test dashboard"),
        sources: vec![
            SourceHealth {
                source: MarketIntelSourceId::Oddsentry,
                mode: SourceLoadMode::Live,
                status: SourceHealthStatus::Ready,
                detail: String::from("live"),
                refreshed_at: String::from("2026-04-03T11:24:30Z"),
            },
            SourceHealth {
                source: MarketIntelSourceId::FairOdds,
                mode: SourceLoadMode::Fixture,
                status: SourceHealthStatus::Ready,
                detail: String::from("fixture"),
                refreshed_at: String::from("2026-04-03T11:24:30Z"),
            },
        ],
        markets: vec![executable_market],
        value: vec![missing_lay_value],
        ..MarketIntelDashboard::default()
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

    assert_eq!(app.active_trading_section(), TradingSection::Positions);

    app.next_section();
    assert_eq!(app.active_trading_section(), TradingSection::Markets);

    app.previous_section();
    assert_eq!(app.active_trading_section(), TradingSection::Positions);
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

#[test]
fn enter_opens_markets_overlay_and_escape_closes_it() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Markets);

    assert!(!app.markets_overlay_visible());

    app.handle_key(KeyCode::Enter);
    assert!(app.markets_overlay_visible());

    app.handle_key(KeyCode::Esc);
    assert!(!app.markets_overlay_visible());
}

#[test]
fn q_closes_markets_overlay_before_quitting() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Markets);

    app.handle_key(KeyCode::Enter);
    assert!(app.markets_overlay_visible());

    app.handle_key(KeyCode::Char('q'));

    assert!(!app.markets_overlay_visible());
    assert!(app.is_running());
}

#[test]
fn enter_opens_trading_action_overlay_for_active_position() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Positions);

    app.handle_key(KeyCode::Enter);

    let overlay = app
        .trading_action_overlay()
        .expect("positions enter should open trading action overlay");
    assert_eq!(overlay.seed.selection_name, "Selection");
    assert_eq!(overlay.seed.venue, VenueId::Smarkets);
}

#[test]
fn q_closes_trading_action_overlay_before_quitting() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Positions);

    app.handle_key(KeyCode::Enter);
    assert!(app.trading_action_overlay().is_some());

    app.handle_key(KeyCode::Char('q'));

    assert!(app.trading_action_overlay().is_none());
    assert!(app.is_running());
}

#[test]
fn markets_navigation_uses_owls_endpoint_selection() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Markets);

    let first_label = app
        .selected_owls_endpoint()
        .map(|endpoint| endpoint.label.clone())
        .expect("markets should seed the first Owls endpoint");

    app.handle_key(KeyCode::Down);
    let second_label = app
        .selected_owls_endpoint()
        .map(|endpoint| endpoint.label.clone())
        .expect("markets should keep an Owls selection");

    assert_ne!(first_label, second_label);
    assert_eq!(app.selected_open_position_row(), Some(0));
}

#[test]
fn intel_tab_cycles_feature_views() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Intel);

    assert_eq!(app.intel_view().label(), "Markets");

    app.handle_key(KeyCode::Tab);
    assert_eq!(app.intel_view().label(), "Arbitrages");
}

#[test]
fn intel_enter_preloads_calculator_and_p_opens_action_overlay() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    assert!(app.wait_for_async_idle(Duration::from_millis(200)));
    app.set_market_intel_dashboard_for_test(sample_market_intel_dashboard());
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Intel);
    let selected_row_id = app.selected_intel_row().expect("selected Intel row").id;

    app.handle_key(KeyCode::Enter);
    assert_eq!(app.active_trading_section(), TradingSection::Calculator);
    assert!(app.calculator_source().is_some());

    app.set_trading_section(TradingSection::Intel);
    app.handle_key(KeyCode::Char('p'));
    let overlay = app
        .trading_action_overlay()
        .expect("intel p should open trading action overlay");
    assert_eq!(overlay.seed.source_ref, selected_row_id);
}

#[test]
fn intel_action_overlay_uses_sell_only_exchange_quote() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    assert!(app.wait_for_async_idle(Duration::from_millis(200)));
    app.set_market_intel_dashboard_for_test(sample_market_intel_dashboard());
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Intel);

    app.handle_key(KeyCode::Char('p'));

    let overlay = app
        .trading_action_overlay()
        .expect("intel p should open trading action overlay");
    assert_eq!(overlay.seed.buy_price, None);
    assert!(overlay.seed.sell_price.is_some());
    assert!(!overlay.can_cycle_side());
}

#[test]
fn intel_enter_does_not_fabricate_lay_quote_when_data_is_missing() {
    let mut app = App::from_provider(StaticProvider {
        snapshot: positions_snapshot(),
    })
    .expect("app");
    assert!(app.wait_for_async_idle(Duration::from_millis(200)));
    app.set_market_intel_dashboard_for_test(sample_market_intel_dashboard());
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Intel);

    for _ in 0..5 {
        app.handle_key(KeyCode::Tab);
    }
    assert_eq!(app.intel_view().label(), "Value");

    app.handle_key(KeyCode::Enter);

    assert_eq!(app.active_trading_section(), TradingSection::Intel);
    assert!(app.calculator_source().is_none());
    assert!(app.status_message().contains("no lay quote"));
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
            funding_kind: String::from("cash"),
            current_cashout_value: Some(16.16),
            supports_cash_out: true,
        }],
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

fn sample_row(event: &str) -> OpenPositionRow {
    OpenPositionRow {
        event: event.to_string(),
        event_status: String::from("Settled"),
        event_url: String::from("https://smarkets.com/event/active-0"),
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
        overall_pnl_known: true,
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
