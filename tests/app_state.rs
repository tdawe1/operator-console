use std::cell::RefCell;
use std::rc::Rc;

use operator_console::app::App;
use operator_console::domain::{
    ExchangePanelSnapshot, ExitPolicySummary, ExitRecommendation, OpenPositionRow, TrackedBetRow,
    TrackedLeg, ValueMetric, VenueId, VenueStatus, VenueSummary, WorkerStatus, WorkerSummary,
};
use operator_console::provider::{ExchangeProvider, ProviderRequest};

#[derive(Clone)]
struct StubProvider {
    snapshots: Rc<RefCell<Vec<ExchangePanelSnapshot>>>,
}

impl StubProvider {
    fn new(snapshots: Vec<ExchangePanelSnapshot>) -> Self {
        Self {
            snapshots: Rc::new(RefCell::new(snapshots)),
        }
    }
}

impl ExchangeProvider for StubProvider {
    fn handle(&mut self, request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot> {
        match request {
            ProviderRequest::LoadDashboard | ProviderRequest::Refresh => {
                Ok(self.snapshots.borrow_mut().remove(0))
            }
            ProviderRequest::SelectVenue(_) => unreachable!("selection not used in this test"),
            ProviderRequest::CashOutTrackedBet { .. } => Ok(self.snapshots.borrow_mut().remove(0)),
        }
    }
}

#[test]
fn app_refresh_replaces_exchange_snapshot() {
    let mut app = App::from_provider(StubProvider::new(vec![
        sample_snapshot("Initial dashboard"),
        sample_snapshot("Refreshed dashboard"),
    ]))
    .expect("app should load initial snapshot");

    assert_eq!(app.snapshot().status_line, "Initial dashboard");
    assert_eq!(app.status_message(), "Initial dashboard");

    app.refresh().expect("refresh should succeed");
    assert_eq!(app.snapshot().status_line, "Refreshed dashboard");
    assert_eq!(app.status_message(), "Refreshed dashboard");
    assert_eq!(app.snapshot().venues[0].label, "Smarkets");
}

#[test]
fn app_cash_out_uses_provider_action_and_replaces_snapshot() {
    let actionable = sample_snapshot("Actionable dashboard");
    let mut cash_out_result = sample_snapshot("Cash out requested");
    cash_out_result.worker.detail = String::from("Cash out requested for bet-001");
    cash_out_result.exit_recommendations.clear();

    let mut app = App::from_provider(StubProvider::new(vec![actionable, cash_out_result]))
        .expect("app should load initial snapshot");

    app.cash_out_next_actionable_bet()
        .expect("cash out should succeed");

    assert_eq!(app.snapshot().status_line, "Cash out requested");
    assert_eq!(app.status_message(), "Cash out requested");
    assert_eq!(
        app.snapshot().worker.detail,
        "Cash out requested for bet-001"
    );
}

fn sample_snapshot(status_line: &str) -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("exchange-browser-worker"),
            status: WorkerStatus::Ready,
            detail: String::from("stub"),
        },
        venues: vec![VenueSummary {
            id: VenueId::Smarkets,
            label: String::from("Smarkets"),
            status: VenueStatus::Connected,
            detail: String::from("Browser ready"),
            event_count: 3,
            market_count: 18,
        }],
        selected_venue: Some(VenueId::Smarkets),
        events: Vec::new(),
        markets: Vec::new(),
        preflight: None,
        status_line: status_line.to_string(),
        runtime: None,
        account_stats: None,
        open_positions: vec![OpenPositionRow {
            event: String::from("Arsenal v Everton"),
            event_status: String::from("27'|Premier League"),
            event_url: String::from(
                "https://smarkets.com/football/england-premier-league/2026/03/14/20-00/arsenal-vs-everton/44919693/",
            ),
            contract: String::from("Draw"),
            market: String::from("Full-time result"),
            status: String::from("Order filled"),
            market_status: String::from("tradable"),
            is_in_play: true,
            price: 3.35,
            stake: 9.91,
            liability: 23.29,
            current_value: 6.64,
            pnl_amount: 3.27,
            current_back_odds: Some(5.0),
            current_implied_probability: Some(0.2),
            current_implied_percentage: Some(20.0),
            current_buy_odds: Some(5.0),
            current_buy_implied_probability: Some(0.2),
            current_sell_odds: Some(5.1),
            current_sell_implied_probability: Some(1.0 / 5.1),
            current_score: String::from("0-0"),
            current_score_home: Some(0),
            current_score_away: Some(0),
            live_clock: String::from("27'"),
            can_trade_out: true,
        }],
        historical_positions: Vec::new(),
        ledger_pnl_summary: Default::default(),
        other_open_bets: Vec::new(),
        decisions: Vec::new(),
        watch: None,
        tracked_bets: vec![TrackedBetRow {
            bet_id: String::from("bet-001"),
            group_id: String::from("group-arsenal-everton"),
            event: String::from("Arsenal v Everton"),
            market: String::from("Full-time result"),
            selection: String::from("Draw"),
            status: String::from("open"),
            placed_at: String::from("2026-03-13T10:30:00Z"),
            settled_at: String::new(),
            platform: String::from("bet365"),
            platform_kind: String::from("sportsbook"),
            exchange: Some(String::from("smarkets")),
            sport_key: String::from("soccer_epl"),
            sport_name: String::from("Premier League"),
            bet_type: String::from("single"),
            market_family: String::from("match_odds"),
            selection_line: None,
            currency: String::from("GBP"),
            stake_gbp: Some(2.0),
            potential_returns_gbp: Some(4.24),
            payout_gbp: None,
            realised_pnl_gbp: None,
            back_price: Some(2.12),
            lay_price: Some(3.35),
            spread: None,
            expected_ev: ValueMetric::default(),
            realised_ev: ValueMetric::default(),
            activities: Vec::new(),
            parse_confidence: String::from("high"),
            notes: String::new(),
            legs: vec![
                TrackedLeg {
                    venue: String::from("smarkets"),
                    outcome: String::from("Draw"),
                    side: String::from("lay"),
                    odds: 3.35,
                    stake: 9.91,
                    status: String::from("open"),
                    market: String::from("Full-time result"),
                    market_family: String::from("match_odds"),
                    line: None,
                    liability: None,
                    commission_rate: Some(0.0),
                    exchange: Some(String::from("smarkets")),
                    placed_at: String::new(),
                    settled_at: String::new(),
                },
                TrackedLeg {
                    venue: String::from("bet365"),
                    outcome: String::from("Draw"),
                    side: String::from("back"),
                    odds: 2.12,
                    stake: 2.0,
                    status: String::from("matched"),
                    market: String::from("Full-time result"),
                    market_family: String::from("match_odds"),
                    line: None,
                    liability: None,
                    commission_rate: None,
                    exchange: None,
                    placed_at: String::new(),
                    settled_at: String::new(),
                },
            ],
        }],
        exit_policy: ExitPolicySummary {
            target_profit: 5.0,
            stop_loss: 5.0,
            hard_margin_call_profit_floor: Some(1.0),
            warn_only_default: true,
        },
        exit_recommendations: vec![ExitRecommendation {
            bet_id: String::from("bet-001"),
            action: String::from("cash_out"),
            reason: String::from("hard_margin_call"),
            worst_case_pnl: 3.2,
            cash_out_venue: Some(String::from("smarkets")),
        }],
    }
}
