use crate::domain::{ExchangePanelSnapshot, VenueId};
use crate::horse_matcher::HorseMatcherQuery;
use crate::exchange_api::MatchbookAccountState;
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderSnapshot {
    pub snapshot: ExchangePanelSnapshot,
    pub matchbook_account_state: Option<MatchbookAccountState>,
}

impl ProviderSnapshot {
    pub fn from_snapshot(snapshot: ExchangePanelSnapshot) -> Self {
        Self {
            snapshot,
            matchbook_account_state: None,
        }
    }
}

pub trait ExchangeProvider {
    fn handle(&mut self, request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot>;

    fn handle_with_metadata(
        &mut self,
        request: ProviderRequest,
    ) -> color_eyre::Result<ProviderSnapshot> {
        self.handle(request).map(ProviderSnapshot::from_snapshot)
    }
}

impl<T> ExchangeProvider for Box<T>
where
    T: ExchangeProvider + ?Sized,
{
    fn handle(&mut self, request: ProviderRequest) -> color_eyre::Result<ExchangePanelSnapshot> {
        (**self).handle(request)
    }

    fn handle_with_metadata(
        &mut self,
        request: ProviderRequest,
    ) -> color_eyre::Result<ProviderSnapshot> {
        (**self).handle_with_metadata(request)
    }
}
