mod models;

use std::collections::HashSet;
use std::env;
use std::time::Duration;

use crate::domain::{ExchangePanelSnapshot, ExternalQuoteRow};
use crate::market_normalization::{event_matches, market_matches, selection_matches_with_context};
use color_eyre::eyre::{eyre, WrapErr};
use reqwest::blocking::Client;

pub use models::{
    MarketEventDetail, MarketHistoryPoint, MarketIntelCalculatorSeed, MarketIntelDashboard,
    MarketIntelSourceId, MarketIntelTradingSeed, MarketOpportunityRow, MarketQuoteComparisonRow,
    OperatorActiveResponse, OperatorExecutionAction, OperatorMatchOpportunity, OperatorMatchQuote,
    OperatorStrategyRecommendation, OpportunityKind, SourceHealth, SourceHealthStatus,
    SourceLoadMode,
};

const SABISABI_BASE_URL_ENV: &str = "SABISABI_BASE_URL";
const DEFAULT_SABISABI_BASE_URL: &str = "http://127.0.0.1:4080";

pub fn load_dashboard() -> color_eyre::Result<MarketIntelDashboard> {
    #[cfg(test)]
    {
        Ok(test_dashboard_fixture())
    }

    #[cfg(not(test))]
    load_dashboard_via_backend(true, None)
}

#[cfg_attr(test, allow(dead_code))]
pub fn load_dashboard_with_options(
    refresh: bool,
    sport_key: Option<&str>,
) -> color_eyre::Result<MarketIntelDashboard> {
    #[cfg(test)]
    {
        let _ = (refresh, sport_key);
        Ok(test_dashboard_fixture())
    }

    #[cfg(not(test))]
    load_dashboard_via_backend(refresh, sport_key)
}

#[cfg_attr(test, allow(dead_code))]
fn load_dashboard_via_backend(
    refresh: bool,
    sport_key: Option<&str>,
) -> color_eyre::Result<MarketIntelDashboard> {
    let client = build_backend_client()?;
    let base_url = market_intel_backend_base_url();

    if refresh {
        refresh_backend_dashboard(&client, &base_url)?;
    }
    query_backend_operator_active(&client, &base_url, sport_key)
}

#[cfg_attr(test, allow(dead_code))]
fn build_backend_client() -> color_eyre::Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(15))
        .build()
        .wrap_err("failed to build market-intel backend client")
}

#[cfg_attr(test, allow(dead_code))]
fn market_intel_backend_base_url() -> String {
    env::var(SABISABI_BASE_URL_ENV).unwrap_or_else(|_| String::from(DEFAULT_SABISABI_BASE_URL))
}

fn refresh_backend_dashboard(client: &Client, base_url: &str) -> color_eyre::Result<()> {
    let path = "/api/v1/ingest/market-intel/refresh";
    let response = client
        .post(format!("{}{}", base_url.trim_end_matches('/'), path))
        .send()
        .wrap_err_with(|| format!("request failed for {path}"))?;
    let status = response.status();
    let payload = response
        .text()
        .wrap_err("failed to read market-intel refresh response body")?;
    if !status.is_success() {
        return Err(eyre!(
            "HTTP {} during market-intel refresh: {}",
            status.as_u16(),
            truncate(&payload, 160)
        ));
    }
    Ok(())
}

fn query_backend_operator_active(
    client: &Client,
    base_url: &str,
    sport_key: Option<&str>,
) -> color_eyre::Result<MarketIntelDashboard> {
    let path = "/api/v1/query/operator/active";
    let mut request = client.get(format!("{}{}", base_url.trim_end_matches('/'), path));
    if let Some(sport_key) = sport_key.map(str::trim).filter(|value| !value.is_empty()) {
        request = request.query(&[("sport", sport_key)]);
    }
    let response = request
        .send()
        .wrap_err_with(|| format!("request failed for {path}"))?;
    let status = response.status();
    let payload = response
        .text()
        .wrap_err("failed to read market-intel dashboard response body")?;
    if !status.is_success() {
        return Err(eyre!(
            "HTTP {} during operator-active query: {}",
            status.as_u16(),
            truncate(&payload, 160)
        ));
    }
    let active: OperatorActiveResponse =
        serde_json::from_str(&payload).wrap_err("failed to decode operator-active response")?;
    Ok(operator_active_to_dashboard(active))
}

fn operator_active_to_dashboard(active: OperatorActiveResponse) -> MarketIntelDashboard {
    let mut dashboard = MarketIntelDashboard {
        refreshed_at: active.refreshed_at.clone(),
        status_line: format!(
            "Operator active: {} matches, {} live, {} arbs, {} +EV.",
            active.summary.total_matches,
            active.summary.live_matches,
            active.summary.arbitrage_matches,
            active.summary.positive_ev_matches
        ),
        total_events: active
            .matches
            .iter()
            .map(|m| m.event_id.clone())
                .collect::<HashSet<_>>()
                .len(),
        total_opportunities: active.summary.total_matches,
        sources: inferred_sources_from_operator_active(&active),
        ..MarketIntelDashboard::default()
    };

    for item in active.matches {
        let row = operator_match_to_row(item);
        match row.kind {
            OpportunityKind::Arbitrage => dashboard.arbitrages.push(row),
            OpportunityKind::PositiveEv => dashboard.plus_ev.push(row),
            OpportunityKind::Drop => dashboard.drops.push(row),
            OpportunityKind::Value => dashboard.value.push(row),
            OpportunityKind::Market => dashboard.markets.push(row),
        }
    }

    dashboard
}

fn inferred_sources_from_operator_active(active: &OperatorActiveResponse) -> Vec<SourceHealth> {
    let mut sources = active
        .matches
        .iter()
        .map(|item| item.source.clone())
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| left.key().cmp(right.key()));
    sources.dedup_by(|left, right| left.key() == right.key());

    sources
        .into_iter()
        .map(|source| SourceHealth {
            source,
            mode: SourceLoadMode::Live,
            status: SourceHealthStatus::Ready,
            detail: String::from("operator-active"),
            refreshed_at: active.refreshed_at.clone(),
        })
        .collect()
}

fn operator_match_to_row(item: OperatorMatchOpportunity) -> MarketOpportunityRow {
    let primary_quote = item.execution_plan.primary.clone();
    let secondary_quote = item.execution_plan.secondary.clone();
    let mut quotes = item
        .quotes
        .iter()
        .cloned()
        .map(|mut quote| {
            let mut row = operator_quote_to_quote_row(quote, item.is_live);
            // Populate missing fields from parent context
            row.event_id = item.event_id.clone();
            row.event_name = item.event_name.clone();
            row.market_name = item.market_name.clone();
            row
        })
        .collect::<Vec<_>>();

    if quotes.is_empty() {
        quotes.push(execution_action_to_quote_row(&item, &primary_quote, false));
        if let Some(secondary) = secondary_quote.as_ref() {
            quotes.push(execution_action_to_quote_row(&item, secondary, true));
        }
    }

    let strategy_note = if item.strategy.reasons.is_empty() {
        item.strategy.summary.clone()
    } else {
        format!(
            "{} | {}",
            item.strategy.summary,
            item.strategy.reasons.join(" | ")
        )
    };

    MarketOpportunityRow {
        source: item.source,
        kind: item.kind,
        id: item.id,
        sport: item.sport,
        competition_name: item.competition_name,
        event_id: item.event_id,
        event_name: item.event_name,
        market_name: item.market_name,
        selection_name: item.selection_name,
        secondary_selection_name: String::new(),
        venue: primary_quote.venue,
        secondary_venue: secondary_quote
            .as_ref()
            .map(|item| item.venue.clone())
            .unwrap_or_default(),
        price: primary_quote.price,
        secondary_price: secondary_quote.as_ref().and_then(|item| item.price),
        fair_price: item.fair_price,
        liquidity: quotes.iter().find_map(|quote| quote.liquidity),
        edge_percent: item.edge_percent,
        arbitrage_margin: item.arbitrage_margin,
        stake_hint: item.stake_hint.or(primary_quote.stake_hint),
        start_time: item.start_time,
        updated_at: item.updated_at,
        event_url: String::new(),
        deep_link_url: primary_quote.deep_link_url,
        is_live: item.is_live,
        quotes,
        notes: vec![strategy_note],
        raw_data: serde_json::json!({
            "strategy": item.strategy,
            "execution_plan": item.execution_plan,
            "live_status": item.live_status,
        }),
    }
}

fn operator_quote_to_quote_row(
    item: OperatorMatchQuote,
    is_live: bool,
) -> MarketQuoteComparisonRow {
    MarketQuoteComparisonRow {
        source: item.source,
        event_id: String::new(),
        market_id: String::new(),
        selection_id: String::new(),
        event_name: String::new(),
        market_name: String::new(),
        selection_name: item.selection_name,
        side: item.side,
        venue: item.venue,
        price: item.price,
        fair_price: item.fair_price,
        liquidity: item.liquidity,
        event_url: String::new(),
        deep_link_url: item.deep_link_url,
        updated_at: item.updated_at,
        is_live,
        is_sharp: item.is_sharp,
        notes: Vec::new(),
        raw_data: serde_json::Value::Null,
    }
}

fn execution_action_to_quote_row(
    item: &OperatorMatchOpportunity,
    action: &OperatorExecutionAction,
    secondary: bool,
) -> MarketQuoteComparisonRow {
    MarketQuoteComparisonRow {
        source: item.source.clone(),
        event_id: item.event_id.clone(),
        market_id: String::new(),
        selection_id: String::new(),
        event_name: item.event_name.clone(),
        market_name: item.market_name.clone(),
        selection_name: action.selection_name.clone(),
        side: action.side.clone(),
        venue: action.venue.clone(),
        price: action.price,
        fair_price: item.fair_price,
        liquidity: None,
        event_url: String::new(),
        deep_link_url: action.deep_link_url.clone(),
        updated_at: item.updated_at.clone(),
        is_live: item.is_live,
        is_sharp: !secondary,
        notes: Vec::new(),
        raw_data: serde_json::Value::Null,
    }
}

fn truncate(value: &str, limit: usize) -> String {
    let trimmed = value.trim();
    if trimmed.len() <= limit {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..limit])
    }
}

#[cfg(test)]
fn test_dashboard_fixture() -> MarketIntelDashboard {
    let market_quote = MarketQuoteComparisonRow {
        source: MarketIntelSourceId::oddsentry(),
        event_id: String::from("f5-farmville"),
        market_id: String::from("base-map-1-winner"),
        selection_id: String::from("f5-esports"),
        event_name: String::from("F5 Esports vs FarmVille"),
        market_name: String::from("BASE_MAP_1_WINNER"),
        selection_name: String::from("F5 Esports"),
        side: String::from("back"),
        venue: String::from("matchbook"),
        price: Some(2.82),
        fair_price: Some(2.70),
        liquidity: Some(120.0),
        event_url: String::from("https://example.test/f5-farmville"),
        deep_link_url: String::from("https://example.test/f5-farmville/deep"),
        updated_at: String::from("2026-04-03T11:24:00Z"),
        is_live: true,
        is_sharp: false,
        notes: vec![String::from("routeable")],
        raw_data: serde_json::json!({}),
    };
    let lay_quote = MarketQuoteComparisonRow {
        source: MarketIntelSourceId::oddsentry(),
        event_id: String::from("f5-farmville"),
        market_id: String::from("base-map-1-winner"),
        selection_id: String::from("f5-esports-lay"),
        event_name: String::from("F5 Esports vs FarmVille"),
        market_name: String::from("BASE_MAP_1_WINNER"),
        selection_name: String::from("F5 Esports"),
        side: String::from("lay"),
        venue: String::from("smarkets"),
        price: Some(2.74),
        fair_price: Some(2.70),
        liquidity: Some(210.0),
        event_url: String::from("https://example.test/f5-farmville"),
        deep_link_url: String::from("https://example.test/f5-farmville/deep"),
        updated_at: String::from("2026-04-03T11:24:00Z"),
        is_live: true,
        is_sharp: true,
        notes: vec![String::from("sharp")],
        raw_data: serde_json::json!({}),
    };
    let value_quote = MarketQuoteComparisonRow {
        source: MarketIntelSourceId::fair_odds(),
        event_id: String::from("mavericks-suns"),
        market_id: String::from("moneyline"),
        selection_id: String::from("mavericks"),
        event_name: String::from("Mavericks v Suns"),
        market_name: String::from("Moneyline"),
        selection_name: String::from("Mavericks"),
        side: String::from("back"),
        venue: String::from("fanduel"),
        price: Some(2.34),
        fair_price: Some(2.15),
        liquidity: Some(175.0),
        event_url: String::from("https://example.test/mavericks-suns"),
        deep_link_url: String::from("https://example.test/mavericks-suns/deep"),
        updated_at: String::from("fixture"),
        is_live: false,
        is_sharp: false,
        notes: vec![String::from("fair_source:pinnacle")],
        raw_data: serde_json::json!({}),
    };

    MarketIntelDashboard {
        refreshed_at: String::from("2026-04-03T11:24:00Z"),
        status_line: String::from("Intel ready: 1 markets, 1 arbs, 1 +EV, 1 value, 1 drops."),
        sources: vec![
            SourceHealth {
                source: MarketIntelSourceId::oddsentry(),
                mode: SourceLoadMode::Live,
                status: SourceHealthStatus::Ready,
                detail: String::from("Loaded via backend-shaped fixture."),
                refreshed_at: String::from("2026-04-03T11:24:00Z"),
            },
            SourceHealth {
                source: MarketIntelSourceId::fair_odds(),
                mode: SourceLoadMode::Fixture,
                status: SourceHealthStatus::Ready,
                detail: String::from("Fixture-backed parity slice."),
                refreshed_at: String::from("fixture"),
            },
        ],
        sports: vec![],
        total_events: 3,
        total_opportunities: 5,
        markets: vec![MarketOpportunityRow {
            source: MarketIntelSourceId::oddsentry(),
            kind: OpportunityKind::Market,
            id: String::from("oddsentry:market:f5-farmville"),
            sport: String::from("esports"),
            competition_name: String::from("CS2"),
            event_id: String::from("f5-farmville"),
            event_name: String::from("F5 Esports vs FarmVille"),
            market_name: String::from("BASE_MAP_1_WINNER"),
            selection_name: String::from("F5 Esports"),
            secondary_selection_name: String::new(),
            venue: String::from("matchbook"),
            secondary_venue: String::from("smarkets"),
            price: Some(2.82),
            secondary_price: Some(2.74),
            fair_price: Some(2.70),
            liquidity: Some(120.0),
            edge_percent: Some(4.4),
            arbitrage_margin: None,
            stake_hint: Some(10.0),
            start_time: String::new(),
            updated_at: String::from("2026-04-03T11:24:00Z"),
            event_url: String::from("https://example.test/f5-farmville"),
            deep_link_url: String::from("https://example.test/f5-farmville/deep"),
            is_live: true,
            quotes: vec![market_quote.clone(), lay_quote.clone()],
            notes: vec![String::from("routeable")],
            raw_data: serde_json::json!({}),
        }],
        arbitrages: vec![MarketOpportunityRow {
            source: MarketIntelSourceId::oddsentry(),
            kind: OpportunityKind::Arbitrage,
            id: String::from("oddsentry:arb:chelsea-liverpool"),
            sport: String::from("soccer"),
            competition_name: String::from("Premier League"),
            event_id: String::from("chelsea-liverpool"),
            event_name: String::from("Chelsea v Liverpool"),
            market_name: String::from("Match Odds"),
            selection_name: String::from("Liverpool"),
            secondary_selection_name: String::from("Draw"),
            venue: String::from("bet365"),
            secondary_venue: String::from("matchbook"),
            price: Some(3.55),
            secondary_price: Some(3.30),
            fair_price: None,
            liquidity: Some(310.0),
            edge_percent: Some(2.4),
            arbitrage_margin: Some(2.4),
            stake_hint: Some(25.0),
            start_time: String::new(),
            updated_at: String::from("2026-04-03T11:24:03Z"),
            event_url: String::from("https://example.test/chelsea-liverpool"),
            deep_link_url: String::from("https://example.test/chelsea-liverpool/deep"),
            is_live: false,
            quotes: vec![],
            notes: vec![String::from("cross-book arb")],
            raw_data: serde_json::json!({}),
        }],
        plus_ev: vec![MarketOpportunityRow {
            source: MarketIntelSourceId::oddsentry(),
            kind: OpportunityKind::PositiveEv,
            id: String::from("oddsentry:ev:bayern-dortmund"),
            sport: String::from("soccer"),
            competition_name: String::from("Bundesliga"),
            event_id: String::from("bayern-dortmund"),
            event_name: String::from("Bayern v Dortmund"),
            market_name: String::from("BTTS"),
            selection_name: String::from("Yes"),
            secondary_selection_name: String::new(),
            venue: String::from("williamhill"),
            secondary_venue: String::from("matchbook"),
            price: Some(1.91),
            secondary_price: Some(1.84),
            fair_price: Some(1.78),
            liquidity: Some(205.0),
            edge_percent: Some(7.3),
            arbitrage_margin: None,
            stake_hint: None,
            start_time: String::new(),
            updated_at: String::from("2026-04-03T11:24:06Z"),
            event_url: String::from("https://example.test/bayern-dortmund"),
            deep_link_url: String::from("https://example.test/bayern-dortmund/deep"),
            is_live: false,
            quotes: vec![],
            notes: vec![String::from("positive ev")],
            raw_data: serde_json::json!({}),
        }],
        drops: vec![MarketOpportunityRow {
            source: MarketIntelSourceId::fair_odds(),
            kind: OpportunityKind::Drop,
            id: String::from("fairodds:drop:roma-napoli"),
            sport: String::from("soccer"),
            competition_name: String::from("Serie A"),
            event_id: String::from("roma-napoli"),
            event_name: String::from("Roma v Napoli"),
            market_name: String::from("Drop Watch"),
            selection_name: String::from("Roma"),
            secondary_selection_name: String::new(),
            venue: String::from("bet365"),
            secondary_venue: String::new(),
            price: Some(2.61),
            secondary_price: Some(2.74),
            fair_price: None,
            liquidity: Some(140.0),
            edge_percent: Some(10.5),
            arbitrage_margin: None,
            stake_hint: None,
            start_time: String::new(),
            updated_at: String::from("fixture"),
            event_url: String::new(),
            deep_link_url: String::new(),
            is_live: false,
            quotes: vec![],
            notes: vec![String::from("move:2.74->2.61")],
            raw_data: serde_json::json!({}),
        }],
        value: vec![MarketOpportunityRow {
            source: MarketIntelSourceId::fair_odds(),
            kind: OpportunityKind::Value,
            id: String::from("fairodds:value:mavericks-suns"),
            sport: String::from("nba"),
            competition_name: String::from("NBA"),
            event_id: String::from("mavericks-suns"),
            event_name: String::from("Mavericks v Suns"),
            market_name: String::from("Moneyline"),
            selection_name: String::from("Mavericks"),
            secondary_selection_name: String::new(),
            venue: String::from("fanduel"),
            secondary_venue: String::from("pinnacle"),
            price: Some(2.34),
            secondary_price: Some(2.15),
            fair_price: Some(2.15),
            liquidity: Some(175.0),
            edge_percent: Some(8.8),
            arbitrage_margin: None,
            stake_hint: None,
            start_time: String::new(),
            updated_at: String::from("fixture"),
            event_url: String::from("https://example.test/mavericks-suns"),
            deep_link_url: String::from("https://example.test/mavericks-suns/deep"),
            is_live: false,
            quotes: vec![value_quote.clone()],
            notes: vec![String::from("fair_source:pinnacle")],
            raw_data: serde_json::json!({}),
        }],
        event_detail: Some(MarketEventDetail {
            source: MarketIntelSourceId::oddsentry(),
            event_id: String::from("f5-farmville"),
            sport: String::from("esports"),
            event_name: String::from("F5 Esports vs FarmVille"),
            home_team: String::from("F5 Esports"),
            away_team: String::from("FarmVille"),
            start_time: String::new(),
            is_live: true,
            quotes: vec![market_quote, lay_quote],
            history: vec![MarketHistoryPoint {
                event_id: String::from("f5-farmville"),
                market_name: String::from("BASE_MAP_1_WINNER"),
                selection_name: String::from("F5 Esports"),
                observed_at: String::from("2026-04-03T10:24:00Z"),
                price: 2.76,
            }],
            raw_data: serde_json::json!({}),
        }),
    }
}

pub fn project_external_quote_rows(
    snapshot: &ExchangePanelSnapshot,
    dashboard: &MarketIntelDashboard,
) -> Vec<ExternalQuoteRow> {
    let targets = snapshot_market_targets(snapshot);
    if targets.is_empty() {
        return Vec::new();
    }

    let mut rows = Vec::new();
    for target in &targets {
        for quote in dashboard.quote_rows() {
            if !market_intel_quote_matches_target(quote, target) {
                continue;
            }
            rows.push(ExternalQuoteRow {
                provider: format!("market_intel:{}", quote.source.key()),
                venue: quote.venue.clone(),
                event: quote.event_name.clone(),
                market: quote.market_name.clone(),
                selection: quote.selection_name.clone(),
                side: if quote.side.trim().is_empty() {
                    String::from("back")
                } else {
                    quote.side.clone()
                },
                event_url: quote.event_url.clone(),
                deep_link_url: quote.deep_link_url.clone(),
                event_id: quote.event_id.clone(),
                market_id: quote.market_id.clone(),
                selection_id: quote.selection_id.clone(),
                price: quote.price,
                liquidity: quote.liquidity,
                is_sharp: quote.is_sharp,
                updated_at: quote.updated_at.clone(),
                status: if quote.is_live {
                    String::from("live")
                } else {
                    String::from("ready")
                },
            });
        }
    }
    rows
}

#[derive(Clone)]
struct SnapshotMarketTarget {
    event: String,
    market: String,
    selection: String,
}

fn snapshot_market_targets(snapshot: &ExchangePanelSnapshot) -> Vec<SnapshotMarketTarget> {
    let mut targets = Vec::new();
    for open_position in &snapshot.open_positions {
        targets.push(SnapshotMarketTarget {
            event: open_position.event.clone(),
            market: open_position.market.clone(),
            selection: open_position.contract.clone(),
        });
    }
    for tracked_bet in &snapshot.tracked_bets {
        targets.push(SnapshotMarketTarget {
            event: tracked_bet.event.clone(),
            market: tracked_bet.market.clone(),
            selection: tracked_bet.selection.clone(),
        });
        for leg in &tracked_bet.legs {
            targets.push(SnapshotMarketTarget {
                event: tracked_bet.event.clone(),
                market: leg.market.clone(),
                selection: leg.outcome.clone(),
            });
        }
    }
    targets
}

fn market_intel_quote_matches_target(
    quote: &MarketQuoteComparisonRow,
    target: &SnapshotMarketTarget,
) -> bool {
    event_matches(&quote.event_name, &target.event)
        && market_matches(&quote.market_name, &target.market)
        && selection_matches_with_context(
            &quote.selection_name,
            &quote.event_name,
            &quote.market_name,
            &target.selection,
            &target.event,
            &target.market,
        )
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use reqwest::blocking::Client;

    use super::{load_dashboard, project_external_quote_rows};
    use crate::domain::{ExchangePanelSnapshot, OpenPositionRow};

    #[test]
    fn combined_dashboard_contains_both_sources() {
        let dashboard = load_dashboard().expect("combined market intel dashboard");
        assert!(dashboard.sources.len() >= 2);
        assert!(!dashboard.markets.is_empty());
        assert!(!dashboard.arbitrages.is_empty());
        assert!(!dashboard.plus_ev.is_empty());
        assert!(!dashboard.value.is_empty());
        assert!(!dashboard.drops.is_empty());
    }

    #[test]
    fn quote_projection_matches_positions_context() {
        let dashboard = load_dashboard().expect("combined market intel dashboard");
        let mut snapshot = ExchangePanelSnapshot::default();
        snapshot.open_positions = vec![OpenPositionRow {
            event: String::from("F5 Esports vs FarmVille"),
            event_status: String::new(),
            contract: String::from("F5 Esports"),
            market: String::from("BASE_MAP_1_WINNER"),
            status: String::from("open"),
            market_status: String::from("open"),
            is_in_play: false,
            price: 2.82,
            stake: 10.0,
            liability: 10.0,
            current_value: 0.0,
            pnl_amount: 0.0,
            overall_pnl_known: true,
            current_back_odds: Some(2.82),
            current_implied_probability: None,
            current_implied_percentage: None,
            current_buy_odds: Some(2.82),
            current_buy_implied_probability: None,
            current_sell_odds: None,
            current_sell_implied_probability: None,
            current_score: String::new(),
            current_score_home: None,
            current_score_away: None,
            live_clock: String::new(),
            can_trade_out: false,
            event_url: String::new(),
        }];

        let quotes = project_external_quote_rows(&snapshot, &dashboard);
        assert!(quotes.iter().any(|quote| quote.selection == "F5 Esports"));
    }

    #[test]
    fn backend_http_contract_round_trips_dashboard_shape() {
        let body = serde_json::json!({
            "refreshed_at": "2026-04-03T11:24:00Z",
            "generated_at": "2026-04-03T11:24:01Z",
            "summary": {
                "total_matches": 2,
                "live_matches": 1,
                "arbitrage_matches": 1,
                "positive_ev_matches": 1
            },
            "matches": [
                {
                    "id": "oddsentry:arb:chelsea-liverpool",
                    "source": "oddsentry",
                    "kind": "arbitrage",
                    "event_id": "chelsea-liverpool",
                    "sport": "soccer",
                    "competition_name": "Premier League",
                    "event_name": "Chelsea v Liverpool",
                    "market_name": "Match Odds",
                    "selection_name": "Liverpool",
                    "is_live": true,
                    "live_status": null,
                    "start_time": "2026-04-03T13:00:00Z",
                    "updated_at": "2026-04-03T11:24:03Z",
                    "edge_percent": 2.4,
                    "arbitrage_margin": 2.4,
                    "fair_price": null,
                    "stake_hint": 25.0,
                    "quotes": [
                        {
                            "source": "oddsentry",
                            "venue": "bet365",
                            "selection_name": "Liverpool",
                            "side": "back",
                            "price": 3.55,
                            "fair_price": null,
                            "liquidity": 310.0,
                            "updated_at": "2026-04-03T11:24:03Z",
                            "is_sharp": false,
                            "deep_link_url": "https://example.test/chelsea-liverpool/deep"
                        },
                        {
                            "source": "oddsentry",
                            "venue": "matchbook",
                            "selection_name": "Draw",
                            "side": "lay",
                            "price": 3.30,
                            "fair_price": null,
                            "liquidity": 240.0,
                            "updated_at": "2026-04-03T11:24:03Z",
                            "is_sharp": true,
                            "deep_link_url": "https://example.test/chelsea-liverpool/deep"
                        }
                    ],
                    "execution_plan": {
                        "executor": "matchbook",
                        "status": "ready",
                        "primary": {
                            "venue": "bet365",
                            "selection_name": "Liverpool",
                            "side": "back",
                            "price": 3.55,
                            "stake_hint": 25.0,
                            "deep_link_url": "https://example.test/chelsea-liverpool/deep"
                        },
                        "secondary": {
                            "venue": "matchbook",
                            "selection_name": "Draw",
                            "side": "lay",
                            "price": 3.30,
                            "stake_hint": 25.0,
                            "deep_link_url": "https://example.test/chelsea-liverpool/deep"
                        },
                        "notes": ["paired execution required"]
                    },
                    "strategy": {
                        "action": "enter",
                        "confidence": "high",
                        "summary": "Execute both legs while the spread is available.",
                        "stale": false,
                        "reasons": ["arbitrage margin positive"]
                    }
                },
                {
                    "id": "oddsentry:ev:bayern-dortmund",
                    "source": "oddsentry",
                    "kind": "positive_ev",
                    "event_id": "bayern-dortmund",
                    "sport": "soccer",
                    "competition_name": "Bundesliga",
                    "event_name": "Bayern v Dortmund",
                    "market_name": "BTTS",
                    "selection_name": "Yes",
                    "is_live": false,
                    "live_status": null,
                    "start_time": "2026-04-03T15:00:00Z",
                    "updated_at": "2026-04-03T11:24:06Z",
                    "edge_percent": 7.3,
                    "arbitrage_margin": null,
                    "fair_price": 1.78,
                    "stake_hint": null,
                    "quotes": [
                        {
                            "source": "oddsentry",
                            "venue": "williamhill",
                            "selection_name": "Yes",
                            "side": "back",
                            "price": 1.91,
                            "fair_price": 1.78,
                            "liquidity": 205.0,
                            "updated_at": "2026-04-03T11:24:06Z",
                            "is_sharp": false,
                            "deep_link_url": "https://example.test/bayern-dortmund/deep"
                        },
                        {
                            "source": "oddsentry",
                            "venue": "matchbook",
                            "selection_name": "Yes",
                            "side": "lay",
                            "price": 1.84,
                            "fair_price": 1.78,
                            "liquidity": 180.0,
                            "updated_at": "2026-04-03T11:24:06Z",
                            "is_sharp": true,
                            "deep_link_url": "https://example.test/bayern-dortmund/deep"
                        }
                    ],
                    "execution_plan": {
                        "executor": "matchbook",
                        "status": "ready",
                        "primary": {
                            "venue": "williamhill",
                            "selection_name": "Yes",
                            "side": "back",
                            "price": 1.91,
                            "stake_hint": null,
                            "deep_link_url": "https://example.test/bayern-dortmund/deep"
                        },
                        "secondary": null,
                        "notes": []
                    },
                    "strategy": {
                        "action": "enter",
                        "confidence": "medium",
                        "summary": "Enter with the indicated stake and monitor for a hedge.",
                        "stale": false,
                        "reasons": ["edge exceeds threshold"]
                    }
                }
            ]
        })
        .to_string();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("server address");

        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept connection");
                let mut buffer = [0_u8; 4096];
                let _ = stream.read(&mut buffer).expect("read request");
                let request = String::from_utf8_lossy(&buffer);
                let response = if request.starts_with("POST /api/v1/ingest/market-intel/refresh") {
                    "HTTP/1.1 202 Accepted\r\nContent-Length: 2\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}".to_string()
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                stream
                    .write_all(response.as_bytes())
                    .expect("write response");
                stream.flush().expect("flush response");
            }
        });

        let client = Client::builder().build().expect("client");
        let base_url = format!("http://{address}");
        super::refresh_backend_dashboard(&client, &base_url).expect("refresh via backend");
        let fetched = super::query_backend_operator_active(&client, &base_url, None)
            .expect("query operator backend");

        assert_eq!(fetched.arbitrages.len(), 1);
        assert_eq!(fetched.plus_ev.len(), 1);
        assert_eq!(fetched.total_opportunities, 2);
        assert_eq!(fetched.markets.len(), 0);
        assert!(fetched.arbitrages[0]
            .quotes
            .iter()
            .all(|quote| quote.is_live));

        server.join().expect("server join");
    }

    #[test]
    fn dashboard_deserializes_new_source_and_summary_fields() {
        let payload = serde_json::json!({
            "refreshed_at": "2026-04-04T12:00:00Z",
            "status_line": "ok",
            "sources": [
                {
                    "source": "odds_api",
                    "mode": "live",
                    "status": "ready",
                    "detail": "loaded",
                    "refreshed_at": "2026-04-04T12:00:00Z"
                }
            ],
            "sports": [
                {
                    "sport_key": "soccer_epl",
                    "sport_title": "Soccer",
                    "group_name": "Premier League",
                    "active": true,
                    "primary_source": "odds_api",
                    "primary_refreshed_at": "2026-04-04T12:00:00Z",
                    "fallback_available": true,
                    "event_count": 4,
                    "quote_count": 12,
                    "arbitrage_count": 1,
                    "positive_ev_count": 2,
                    "value_count": 0
                }
            ],
            "total_events": 4,
            "total_opportunities": 3,
            "markets": [],
            "arbitrages": [],
            "plus_ev": [],
            "drops": [],
            "value": [],
            "event_detail": null
        });

        let dashboard: crate::market_intel::MarketIntelDashboard =
            serde_json::from_value(payload).expect("decode dashboard");

        assert_eq!(dashboard.sources.len(), 1);
        assert_eq!(dashboard.sources[0].source.key(), "odds_api");
        assert_eq!(dashboard.sports.len(), 1);
        assert_eq!(dashboard.sports[0].primary_source.key(), "odds_api");
        assert_eq!(dashboard.total_events, 4);
        assert_eq!(dashboard.total_opportunities, 3);
    }
}
