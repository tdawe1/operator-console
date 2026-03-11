use crate::domain::{ExchangePanelSnapshot, VenueId, WatchSnapshot};
pub use crate::transport::WorkerConfig as WatchRequest;

pub trait WatchProvider {
    fn load_watch_snapshot(&mut self, request: &WatchRequest) -> color_eyre::Result<WatchSnapshot>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderRequest {
    LoadDashboard,
    SelectVenue(VenueId),
    Refresh,
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
