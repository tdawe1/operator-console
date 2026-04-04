use crate::domain::{ExchangePanelSnapshot, VenueId};
use crate::horse_matcher::HorseMatcherQuery;
use crate::trading_actions::TradingActionIntent;

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderRequest {
    LoadDashboard,
    SelectVenue(VenueId),
    RefreshCached,
    RefreshLive,
    CashOutTrackedBet { bet_id: String },
    ExecuteTradingAction { intent: Box<TradingActionIntent> },
    LoadHorseMatcher { query: Box<HorseMatcherQuery> },
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
