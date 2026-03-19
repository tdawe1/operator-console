use crate::domain::{ExchangePanelSnapshot, VenueId};

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderRequest {
    LoadDashboard,
    SelectVenue(VenueId),
    Refresh,
    CashOutTrackedBet { bet_id: String },
}

pub trait ExchangeProvider {
    fn handle(&mut self, request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot>;
}

impl<T> ExchangeProvider for Box<T>
where
    T: ExchangeProvider + ?Sized,
{
    fn handle(&mut self, request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot> {
        (**self).handle(request)
    }
}
