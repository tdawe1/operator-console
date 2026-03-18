use color_eyre::eyre::eyre;

use operator_console::app::App;
use operator_console::domain::{
    ExchangePanelSnapshot, VenueId, VenueStatus, VenueSummary, WorkerStatus, WorkerSummary,
};
use operator_console::provider::{ExchangeProvider, ProviderRequest};

struct FailingRefreshProvider {
    load_snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for FailingRefreshProvider {
    fn handle(&mut self, request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot> {
        match request {
            ProviderRequest::LoadDashboard => Ok(self.load_snapshot.clone()),
            ProviderRequest::Refresh => Err(eyre!("worker session closed")),
            ProviderRequest::SelectVenue(_) => Err(eyre!("selection not used in this test")),
            ProviderRequest::CashOutTrackedBet { .. } => {
                Err(eyre!("cash out not used in this test"))
            }
        }
    }
}

#[test]
fn refresh_failure_marks_worker_error_and_preserves_selection() {
    let mut app = App::from_provider(FailingRefreshProvider {
        load_snapshot: sample_snapshot(),
    })
    .expect("app should load initial snapshot");

    let error = app.refresh().expect_err("refresh should fail");

    assert!(error.to_string().contains("worker session closed"));
    assert_eq!(app.selected_venue(), Some(VenueId::Smarkets));
    assert_eq!(app.snapshot().worker.status, WorkerStatus::Error);
    assert!(app
        .snapshot()
        .worker
        .detail
        .contains("Refresh failed: worker session closed"));
    assert!(app
        .status_message()
        .contains("Refresh failed: worker session closed"));
}

fn sample_snapshot() -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("exchange-browser-worker"),
            status: WorkerStatus::Ready,
            detail: String::from("connected"),
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
        status_line: String::from("Initial dashboard"),
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
