use std::cell::RefCell;
use std::rc::Rc;

use operator_console::app::{App, Panel, TradingSection};
use operator_console::domain::{
    ExchangePanelSnapshot, RuntimeSummary, VenueId, VenueStatus, VenueSummary, WorkerStatus,
    WorkerSummary,
};
use operator_console::provider::{ExchangeProvider, ProviderRequest};

struct WorkflowProvider {
    requests: Rc<RefCell<Vec<ProviderRequest>>>,
    selected_venue: VenueId,
}

impl WorkflowProvider {
    fn new(requests: Rc<RefCell<Vec<ProviderRequest>>>) -> Self {
        Self {
            requests,
            selected_venue: VenueId::Smarkets,
        }
    }
}

impl ExchangeProvider for WorkflowProvider {
    fn handle(&mut self, request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot> {
        self.requests.borrow_mut().push(request.clone());

        let snapshot = match request {
            ProviderRequest::LoadDashboard => workflow_snapshot(
                "Loaded Smarkets dashboard.",
                VenueId::Smarkets,
                "bootstrap",
                WorkerStatus::Ready,
            ),
            ProviderRequest::SelectVenue(venue) => {
                self.selected_venue = venue;
                match venue {
                    VenueId::Betway => workflow_snapshot(
                        "Captured betway live tab.",
                        VenueId::Betway,
                        "live_capture",
                        WorkerStatus::Ready,
                    ),
                    _ => workflow_snapshot(
                        "Returned to Smarkets.",
                        VenueId::Smarkets,
                        "cached",
                        WorkerStatus::Ready,
                    ),
                }
            }
            ProviderRequest::RefreshCached => workflow_snapshot(
                match self.selected_venue {
                    VenueId::Betway => "Reused cached betway snapshot.",
                    _ => "Reused cached Smarkets snapshot.",
                },
                self.selected_venue,
                "cached",
                WorkerStatus::Ready,
            ),
            ProviderRequest::RefreshLive => workflow_snapshot(
                match self.selected_venue {
                    VenueId::Betway => "betway live tab unavailable.",
                    _ => "Captured live Smarkets snapshot.",
                },
                self.selected_venue,
                "live_capture",
                match self.selected_venue {
                    VenueId::Betway => WorkerStatus::Error,
                    _ => WorkerStatus::Ready,
                },
            ),
            ProviderRequest::CashOutTrackedBet { .. }
            | ProviderRequest::ExecuteTradingAction { .. }
            | ProviderRequest::LoadHorseMatcher { .. } => {
                unreachable!("workflow test only covers dashboard and venue refresh flow")
            }
        };

        Ok(snapshot)
    }
}

#[test]
fn venue_switch_flow_distinguishes_cached_and_live_refreshes() {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let mut app = App::from_provider(WorkflowProvider::new(requests.clone()))
        .expect("workflow provider should initialize");

    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Positions);

    app.select_next_exchange_row();
    app.select_next_exchange_row();

    assert_eq!(app.selected_venue(), Some(VenueId::Betway));
    assert_eq!(app.recorder_snapshot_mode(), "live");
    assert_eq!(app.snapshot().worker.status, WorkerStatus::Ready);
    assert_eq!(app.snapshot().status_line, "Captured betway live tab.");

    app.refresh().expect("cached refresh should succeed");

    assert_eq!(app.recorder_snapshot_mode(), "cached");
    assert_eq!(app.snapshot().status_line, "Reused cached betway snapshot.");
    assert_eq!(app.snapshot().worker.status, WorkerStatus::Ready);

    app.refresh_live().expect("live refresh should succeed");

    assert_eq!(app.recorder_snapshot_mode(), "live");
    assert_eq!(app.snapshot().worker.status, WorkerStatus::Error);
    assert!(app.snapshot().status_line.contains("unavailable"));

    app.select_previous_exchange_row();

    assert_eq!(app.selected_venue(), Some(VenueId::Smarkets));
    assert_eq!(app.recorder_snapshot_mode(), "cached");
    assert_eq!(app.snapshot().worker.status, WorkerStatus::Ready);
    assert_eq!(app.snapshot().status_line, "Returned to Smarkets.");
    assert_eq!(
        requests.borrow().clone(),
        vec![
            ProviderRequest::LoadDashboard,
            ProviderRequest::SelectVenue(VenueId::Smarkets),
            ProviderRequest::SelectVenue(VenueId::Betway),
            ProviderRequest::RefreshCached,
            ProviderRequest::RefreshLive,
            ProviderRequest::SelectVenue(VenueId::Smarkets),
        ]
    );
}

fn workflow_snapshot(
    status_line: &str,
    selected_venue: VenueId,
    refresh_kind: &str,
    worker_status: WorkerStatus,
) -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("bet-recorder"),
            status: worker_status,
            detail: String::from(status_line),
        },
        venues: vec![
            VenueSummary {
                id: VenueId::Smarkets,
                label: String::from("Smarkets"),
                status: VenueStatus::Connected,
                detail: String::from("exchange ready"),
                event_count: 1,
                market_count: 2,
            },
            VenueSummary {
                id: VenueId::Betway,
                label: String::from("betway"),
                status: VenueStatus::Connected,
                detail: String::from("sportsbook ready"),
                event_count: 1,
                market_count: 1,
            },
        ],
        selected_venue: Some(selected_venue),
        status_line: String::from(status_line),
        runtime: Some(RuntimeSummary {
            updated_at: String::from("2026-03-20T11:30:00Z"),
            source: String::from("bet-recorder"),
            refresh_kind: String::from(refresh_kind),
            worker_reconnect_count: 0,
            decision_count: 1,
            watcher_iteration: Some(12),
            stale: false,
        }),
        ..ExchangePanelSnapshot::empty()
    }
}
