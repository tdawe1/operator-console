use color_eyre::eyre::{ContextCompat, Result};

use crate::domain::{ExchangePanelSnapshot, VenueId};
use crate::provider::{ExchangeProvider, ProviderRequest};
use crate::transport::WorkerResponseEnvelope;

const SNAPSHOT_FIXTURE: &str = include_str!("../fixtures/exchange_panel_snapshot.json");

#[derive(Debug, Clone)]
pub struct StubExchangeProvider {
    snapshot: ExchangePanelSnapshot,
}

impl Default for StubExchangeProvider {
    fn default() -> Self {
        let response: WorkerResponseEnvelope =
            serde_json::from_str(SNAPSHOT_FIXTURE).expect("exchange stub fixture should parse");
        Self {
            snapshot: response.snapshot,
        }
    }
}

impl ExchangeProvider for StubExchangeProvider {
    fn handle(&mut self, request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        match request {
            ProviderRequest::LoadDashboard | ProviderRequest::Refresh => Ok(self.snapshot.clone()),
            ProviderRequest::SelectVenue(venue) => {
                self.select_venue(venue)?;
                Ok(self.snapshot.clone())
            }
            ProviderRequest::CashOutTrackedBet { bet_id } => {
                self.snapshot.status_line = format!("Cash out requested for {bet_id}.");
                Ok(self.snapshot.clone())
            }
        }
    }
}

impl StubExchangeProvider {
    fn select_venue(&mut self, venue: VenueId) -> Result<()> {
        let selected = self
            .snapshot
            .venues
            .iter()
            .find(|candidate| candidate.id == venue)
            .with_context(|| format!("unknown stub venue: {}", venue.as_str()))?;

        self.snapshot.selected_venue = Some(venue);
        self.snapshot.status_line = format!("Selected {}.", selected.label);
        Ok(())
    }
}
