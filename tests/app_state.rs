use std::cell::RefCell;
use std::rc::Rc;

use operator_console::app::App;
use operator_console::domain::{
    ExchangePanelSnapshot, ExitRecommendation, VenueId, VenueStatus, VenueSummary, WorkerStatus,
    WorkerSummary,
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
        open_positions: Vec::new(),
        historical_positions: Vec::new(),
        other_open_bets: Vec::new(),
        decisions: Vec::new(),
        watch: None,
        tracked_bets: Vec::new(),
        exit_policy: Default::default(),
        exit_recommendations: vec![ExitRecommendation {
            bet_id: String::from("bet-001"),
            action: String::from("cash_out"),
            reason: String::from("hard_margin_call"),
            worst_case_pnl: 3.2,
            cash_out_venue: Some(String::from("smarkets")),
        }],
    }
}
