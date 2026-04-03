mod models;

use std::env;
use std::time::Duration;

use crate::domain::{ExchangePanelSnapshot, ExternalQuoteRow};
use crate::market_normalization::{event_matches, market_matches, selection_matches_with_context};
use color_eyre::eyre::{eyre, WrapErr};
use reqwest::blocking::Client;

pub use models::{
    MarketEventDetail, MarketHistoryPoint, MarketIntelCalculatorSeed, MarketIntelDashboard,
    MarketIntelSourceId, MarketIntelTradingSeed, MarketOpportunityRow, MarketQuoteComparisonRow,
    OpportunityKind, SourceHealth, SourceHealthStatus, SourceLoadMode,
};

const SABISABI_BASE_URL_ENV: &str = "SABISABI_BASE_URL";
const DEFAULT_SABISABI_BASE_URL: &str = "http://127.0.0.1:4080";

pub fn load_dashboard() -> color_eyre::Result<MarketIntelDashboard> {
    #[cfg(test)]
    {
        Ok(test_dashboard_fixture())
    }

    #[cfg(not(test))]
    load_dashboard_via_backend()
}

#[cfg_attr(test, allow(dead_code))]
fn load_dashboard_via_backend() -> color_eyre::Result<MarketIntelDashboard> {
    let client = build_backend_client()?;
    let base_url = market_intel_backend_base_url();

    refresh_backend_dashboard(&client, &base_url)?;
    query_backend_dashboard(&client, &base_url)
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

fn query_backend_dashboard(
    client: &Client,
    base_url: &str,
) -> color_eyre::Result<MarketIntelDashboard> {
    let path = "/api/v1/query/market-intel/dashboard";
    let response = client
        .get(format!("{}{}", base_url.trim_end_matches('/'), path))
        .send()
        .wrap_err_with(|| format!("request failed for {path}"))?;
    let status = response.status();
    let payload = response
        .text()
        .wrap_err("failed to read market-intel dashboard response body")?;
    if !status.is_success() {
        return Err(eyre!(
            "HTTP {} during market-intel query: {}",
            status.as_u16(),
            truncate(&payload, 160)
        ));
    }
    serde_json::from_str(&payload).wrap_err("failed to decode market-intel dashboard response")
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
        source: MarketIntelSourceId::Oddsentry,
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
        source: MarketIntelSourceId::Oddsentry,
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
        source: MarketIntelSourceId::FairOdds,
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
                source: MarketIntelSourceId::Oddsentry,
                mode: SourceLoadMode::Live,
                status: SourceHealthStatus::Ready,
                detail: String::from("Loaded via backend-shaped fixture."),
                refreshed_at: String::from("2026-04-03T11:24:00Z"),
            },
            SourceHealth {
                source: MarketIntelSourceId::FairOdds,
                mode: SourceLoadMode::Fixture,
                status: SourceHealthStatus::Ready,
                detail: String::from("Fixture-backed parity slice."),
                refreshed_at: String::from("fixture"),
            },
        ],
        markets: vec![MarketOpportunityRow {
            source: MarketIntelSourceId::Oddsentry,
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
            source: MarketIntelSourceId::Oddsentry,
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
            source: MarketIntelSourceId::Oddsentry,
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
            source: MarketIntelSourceId::FairOdds,
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
            source: MarketIntelSourceId::FairOdds,
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
            source: MarketIntelSourceId::Oddsentry,
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

    use super::{load_dashboard, project_external_quote_rows, test_dashboard_fixture};
    use crate::domain::{ExchangePanelSnapshot, OpenPositionRow};

    #[test]
    fn combined_dashboard_contains_both_sources() {
        let dashboard = load_dashboard().expect("combined market intel dashboard");
        assert_eq!(dashboard.sources.len(), 2);
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
        let dashboard = test_dashboard_fixture();
        let body = serde_json::to_string(&dashboard).expect("serialize dashboard");
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
        let fetched = super::query_backend_dashboard(&client, &base_url).expect("query backend");

        assert_eq!(fetched.sources.len(), dashboard.sources.len());
        assert_eq!(fetched.markets.len(), dashboard.markets.len());
        assert_eq!(fetched.arbitrages.len(), dashboard.arbitrages.len());
        assert_eq!(fetched.plus_ev.len(), dashboard.plus_ev.len());
        assert_eq!(fetched.value.len(), dashboard.value.len());
        assert_eq!(fetched.drops.len(), dashboard.drops.len());

        server.join().expect("server join");
    }
}
