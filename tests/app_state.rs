use std::cell::RefCell;
use std::rc::Rc;

use operator_console::app::App;
use operator_console::domain::{
    ExchangePanelSnapshot, VenueId, VenueStatus, VenueSummary, WorkerStatus, WorkerSummary,
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

    app.refresh().expect("refresh should succeed");
    assert_eq!(app.snapshot().status_line, "Refreshed dashboard");
    assert_eq!(app.snapshot().venues[0].label, "Smarkets");
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
        account_stats: None,
        open_positions: Vec::new(),
        other_open_bets: Vec::new(),
        watch: None,
    }
}
