use operator_console::domain::{ExchangePanelSnapshot, VenueId};
use operator_console::provider::{ExchangeProvider, ProviderRequest};

struct NullProvider;

impl ExchangeProvider for NullProvider {
    fn handle(&mut self, request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot> {
        match request {
            ProviderRequest::LoadDashboard => Ok(ExchangePanelSnapshot::empty()),
            _ => unreachable!("test provider only handles initial load"),
        }
    }
}

#[test]
fn provider_contract_returns_snapshot() {
    let mut provider = NullProvider;
    let snapshot = provider
        .handle(ProviderRequest::LoadDashboard)
        .expect("snapshot");

    assert!(snapshot.venues.is_empty());
    assert_eq!(snapshot.selected_venue, None);
}

#[test]
fn venue_id_display_labels_are_stable() {
    assert_eq!(VenueId::Smarkets.as_str(), "smarkets");
    assert_eq!(VenueId::Bet365.as_str(), "bet365");
    assert_eq!(VenueId::Betfair.as_str(), "betfair");
    assert_eq!(VenueId::Betfred.as_str(), "betfred");
    assert_eq!(VenueId::Betdaq.as_str(), "betdaq");
    assert_eq!(VenueId::Betway.as_str(), "betway");
    assert_eq!(VenueId::Betuk.as_str(), "betuk");
}
