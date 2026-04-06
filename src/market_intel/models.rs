use crate::market_normalization::normalize_key;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MarketIntelSourceId(String);

impl MarketIntelSourceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(normalize_market_intel_source_key(&value.into()))
    }

    pub fn owls() -> Self {
        Self::new("owls")
    }

    pub fn oddsentry() -> Self {
        Self::new("oddsentry")
    }

    pub fn fair_odds() -> Self {
        Self::new("fair_odds")
    }

    pub fn odds_api() -> Self {
        Self::new("odds_api")
    }

    pub fn label(&self) -> &str {
        match self.key() {
            "owls" => "Owls",
            "oddsentry" => "Oddsentry",
            "fair_odds" => "FairOdds",
            "odds_api" => "The Odds API",
            other => other,
        }
    }

    pub fn key(&self) -> &str {
        &self.0
    }
}

fn normalize_market_intel_source_key(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "owls" | "owlsinsight" | "owls_insight" => String::from("owls"),
        "oddsapi" | "odds_api" | "the-odds-api" => String::from("odds_api"),
        "fairodds" | "fair_odds" => String::from("fair_odds"),
        "oddsentry" => String::from("oddsentry"),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum OpportunityKind {
    #[default]
    Market,
    Arbitrage,
    PositiveEv,
    Drop,
    Value,
}

impl OpportunityKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Market => "Markets",
            Self::Arbitrage => "Arbitrages",
            Self::PositiveEv => "Plus EV",
            Self::Drop => "Drops",
            Self::Value => "Value",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SourceLoadMode {
    #[default]
    Fixture,
    Live,
}

impl SourceLoadMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixture => "fixture",
            Self::Live => "live",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SourceHealthStatus {
    #[default]
    Ready,
    Degraded,
    Error,
}

impl SourceHealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceHealth {
    pub source: MarketIntelSourceId,
    pub mode: SourceLoadMode,
    pub status: SourceHealthStatus,
    pub detail: String,
    pub refreshed_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SportDashboard {
    pub sport_key: String,
    pub sport_title: String,
    pub group_name: String,
    pub active: bool,
    pub primary_source: MarketIntelSourceId,
    pub primary_refreshed_at: Option<String>,
    pub fallback_available: bool,
    pub event_count: usize,
    pub quote_count: usize,
    pub arbitrage_count: usize,
    pub positive_ev_count: usize,
    pub value_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MarketQuoteComparisonRow {
    pub source: MarketIntelSourceId,
    pub event_id: String,
    pub market_id: String,
    pub selection_id: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub side: String,
    pub venue: String,
    pub price: Option<f64>,
    pub fair_price: Option<f64>,
    pub liquidity: Option<f64>,
    pub event_url: String,
    pub deep_link_url: String,
    pub updated_at: String,
    pub is_live: bool,
    pub is_sharp: bool,
    pub notes: Vec<String>,
    #[serde(default)]
    pub raw_data: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MarketHistoryPoint {
    pub event_id: String,
    pub market_name: String,
    pub selection_name: String,
    pub observed_at: String,
    pub price: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MarketEventDetail {
    pub source: MarketIntelSourceId,
    pub event_id: String,
    pub sport: String,
    pub event_name: String,
    pub home_team: String,
    pub away_team: String,
    pub start_time: String,
    pub is_live: bool,
    pub quotes: Vec<MarketQuoteComparisonRow>,
    pub history: Vec<MarketHistoryPoint>,
    #[serde(default)]
    pub raw_data: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketIntelCalculatorSeed {
    pub event_name: String,
    pub selection_name: String,
    pub competition_name: String,
    pub rating: f64,
    pub bookmaker_name: String,
    pub exchange_name: String,
    pub back_odds: f64,
    pub lay_odds: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketIntelTradingSeed {
    pub source_ref: String,
    pub venue_name: String,
    pub preferred_side: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub event_url: String,
    pub deep_link_url: String,
    pub event_id: String,
    pub market_id: String,
    pub selection_id: String,
    pub buy_price: Option<f64>,
    pub sell_price: Option<f64>,
    pub default_stake: Option<f64>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MarketOpportunityRow {
    pub source: MarketIntelSourceId,
    pub kind: OpportunityKind,
    pub id: String,
    pub sport: String,
    pub competition_name: String,
    pub event_id: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub secondary_selection_name: String,
    pub venue: String,
    pub secondary_venue: String,
    pub price: Option<f64>,
    pub secondary_price: Option<f64>,
    pub fair_price: Option<f64>,
    pub liquidity: Option<f64>,
    pub edge_percent: Option<f64>,
    pub arbitrage_margin: Option<f64>,
    pub stake_hint: Option<f64>,
    pub start_time: String,
    pub updated_at: String,
    pub event_url: String,
    pub deep_link_url: String,
    pub is_live: bool,
    pub quotes: Vec<MarketQuoteComparisonRow>,
    pub notes: Vec<String>,
    #[serde(default)]
    pub raw_data: serde_json::Value,
}

impl MarketOpportunityRow {
    pub fn primary_quote(&self) -> Option<&MarketQuoteComparisonRow> {
        self.quotes
            .first()
            .filter(|quote| quote.price.unwrap_or_default() > 1.0)
    }

    pub fn secondary_quote(&self) -> Option<&MarketQuoteComparisonRow> {
        self.quotes
            .iter()
            .skip(1)
            .find(|quote| quote.price.unwrap_or_default() > 1.0)
    }

    pub fn calculator_seed(&self) -> Option<MarketIntelCalculatorSeed> {
        let primary = self.primary_quote()?;
        let secondary = self.secondary_quote()?;
        let back_odds = primary.price?;
        let lay_odds = secondary.price?;
        if back_odds <= 1.0 || lay_odds <= 1.0 {
            return None;
        }

        Some(MarketIntelCalculatorSeed {
            event_name: self.event_name.clone(),
            selection_name: if self.selection_name.trim().is_empty() {
                primary.selection_name.clone()
            } else {
                self.selection_name.clone()
            },
            competition_name: if self.competition_name.trim().is_empty() {
                self.source.label().to_string()
            } else {
                self.competition_name.clone()
            },
            rating: self.edge_percent.unwrap_or_default(),
            bookmaker_name: primary.venue.clone(),
            exchange_name: secondary.venue.clone(),
            back_odds,
            lay_odds,
        })
    }

    pub fn trading_seed(&self) -> Option<MarketIntelTradingSeed> {
        let quote = self.primary_quote()?;
        let price = quote.price?;
        if price <= 1.0 {
            return None;
        }

        let preferred_side = if normalize_key(&quote.side).contains("lay")
            || normalize_key(&quote.side).contains("sell")
        {
            String::from("sell")
        } else {
            String::from("buy")
        };
        let (buy_price, sell_price) = if preferred_side == "sell" {
            (None, Some(price))
        } else {
            (
                Some(price),
                self.secondary_quote().and_then(|item| item.price),
            )
        };

        Some(MarketIntelTradingSeed {
            source_ref: if self.id.trim().is_empty() {
                format!(
                    "{}:{}:{}:{}",
                    self.source.key(),
                    self.kind.label(),
                    self.event_name,
                    quote.selection_name
                )
            } else {
                self.id.clone()
            },
            venue_name: quote.venue.clone(),
            preferred_side,
            event_name: self.event_name.clone(),
            market_name: if self.market_name.trim().is_empty() {
                quote.market_name.clone()
            } else {
                self.market_name.clone()
            },
            selection_name: if self.selection_name.trim().is_empty() {
                quote.selection_name.clone()
            } else {
                self.selection_name.clone()
            },
            event_url: if quote.event_url.trim().is_empty() {
                self.event_url.clone()
            } else {
                quote.event_url.clone()
            },
            deep_link_url: if quote.deep_link_url.trim().is_empty() {
                self.deep_link_url.clone()
            } else {
                quote.deep_link_url.clone()
            },
            event_id: if quote.event_id.trim().is_empty() {
                self.event_id.clone()
            } else {
                quote.event_id.clone()
            },
            market_id: quote.market_id.clone(),
            selection_id: quote.selection_id.clone(),
            buy_price,
            sell_price,
            default_stake: self.stake_hint,
            notes: self.notes.clone(),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MarketIntelDashboard {
    pub refreshed_at: String,
    pub status_line: String,
    pub sources: Vec<SourceHealth>,
    #[serde(default)]
    pub sports: Vec<SportDashboard>,
    #[serde(default)]
    pub total_events: usize,
    #[serde(default)]
    pub total_opportunities: usize,
    pub markets: Vec<MarketOpportunityRow>,
    pub arbitrages: Vec<MarketOpportunityRow>,
    pub plus_ev: Vec<MarketOpportunityRow>,
    pub drops: Vec<MarketOpportunityRow>,
    pub value: Vec<MarketOpportunityRow>,
    pub event_detail: Option<MarketEventDetail>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OperatorActiveResponse {
    pub refreshed_at: String,
    pub generated_at: String,
    pub summary: OperatorSummary,
    pub matches: Vec<OperatorMatchOpportunity>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OperatorSummary {
    pub total_matches: usize,
    pub live_matches: usize,
    pub arbitrage_matches: usize,
    pub positive_ev_matches: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OperatorMatchOpportunity {
    pub id: String,
    pub source: MarketIntelSourceId,
    pub kind: OpportunityKind,
    pub event_id: String,
    pub sport: String,
    pub competition_name: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub is_live: bool,
    pub live_status: Option<String>,
    pub start_time: String,
    pub updated_at: String,
    pub edge_percent: Option<f64>,
    pub arbitrage_margin: Option<f64>,
    pub fair_price: Option<f64>,
    pub stake_hint: Option<f64>,
    #[serde(default)]
    pub quotes: Vec<OperatorMatchQuote>,
    pub execution_plan: OperatorExecutionPlan,
    pub strategy: OperatorStrategyRecommendation,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OperatorMatchQuote {
    pub source: MarketIntelSourceId,
    pub venue: String,
    pub selection_name: String,
    pub side: String,
    pub price: Option<f64>,
    pub fair_price: Option<f64>,
    pub liquidity: Option<f64>,
    pub updated_at: String,
    pub is_sharp: bool,
    pub deep_link_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OperatorExecutionPlan {
    pub executor: String,
    pub status: String,
    pub primary: OperatorExecutionAction,
    pub secondary: Option<OperatorExecutionAction>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OperatorExecutionAction {
    pub venue: String,
    pub selection_name: String,
    pub side: String,
    pub price: Option<f64>,
    pub stake_hint: Option<f64>,
    pub deep_link_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OperatorStrategyRecommendation {
    pub action: String,
    pub confidence: String,
    pub summary: String,
    pub stale: bool,
    #[serde(default)]
    pub reasons: Vec<String>,
}

impl MarketIntelDashboard {
    pub fn quote_rows(&self) -> Vec<&MarketQuoteComparisonRow> {
        let mut rows = Vec::new();
        for group in [
            &self.markets,
            &self.arbitrages,
            &self.plus_ev,
            &self.drops,
            &self.value,
        ] {
            for row in group.iter() {
                rows.extend(row.quotes.iter());
            }
        }
        if let Some(event_detail) = self.event_detail.as_ref() {
            rows.extend(event_detail.quotes.iter());
        }
        rows
    }
}
