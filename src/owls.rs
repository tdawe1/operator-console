use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use color_eyre::eyre::{eyre, Result, WrapErr};
use futures_util::FutureExt;
use reqwest::blocking::Client;
use reqwest::Client as AsyncClient;
use reqwest::Url;
use rust_socketio::{asynchronous::ClientBuilder as SocketClientBuilder, Payload, TransportType};
use serde::Deserialize;
use serde_json::Value;

use crate::app_state::TradingSection;
use crate::market_normalization::{
    event_matches, market_matches, normalize_key, selection_matches_with_context,
};

const DEFAULT_BASE_URL: &str = "https://api.owlsinsight.com";
const API_KEY_ENV_NAMES: [&str; 2] = ["OWLS_INSIGHT_API_KEY", "OWLSINSIGHT_API_KEY"];
const SABISABI_BASE_URL_ENV: &str = "SABISABI_BASE_URL";
const DEFAULT_SABISABI_BASE_URL: &str = "http://127.0.0.1:4080";
const DEFAULT_SPORT: &str = "soccer";
const DEFAULT_PLAYER: &str = "LeBron James";
const DEFAULT_PROP_TYPE: &str = "points";
const DEFAULT_BOOK_PROPS_PLAYER: &str = "LeBron";
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
const HOT_SYNC_INTERVAL: Duration = Duration::from_secs(3);
const WARM_SYNC_INTERVAL: Duration = Duration::from_secs(10);
const COOL_SYNC_INTERVAL: Duration = Duration::from_secs(30);
const COLD_SYNC_INTERVAL: Duration = Duration::from_secs(120);
const BACKGROUND_SYNC_BATCH: usize = 3;
const MANUAL_SYNC_BATCH: usize = 6;
pub const SUPPORTED_SPORTS: &[&str] = &[
    "nba", "nfl", "mlb", "nhl", "wnba", "ncaab", "ncaaf", "soccer", "epl", "mma", "tennis", "cs2",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwlsEndpointGroup {
    Odds,
    Props,
    Scores,
    Stats,
    Prediction,
    History,
    Realtime,
}

impl OwlsEndpointGroup {
    pub const ALL: [Self; 7] = [
        Self::Odds,
        Self::Props,
        Self::Scores,
        Self::Stats,
        Self::Prediction,
        Self::History,
        Self::Realtime,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Odds => "Odds",
            Self::Props => "Props",
            Self::Scores => "Scores",
            Self::Stats => "Stats",
            Self::Prediction => "Prediction",
            Self::History => "History",
            Self::Realtime => "Realtime",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::Odds => "O",
            Self::Props => "P",
            Self::Scores => "S",
            Self::Stats => "T",
            Self::Prediction => "M",
            Self::History => "H",
            Self::Realtime => "R",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwlsEndpointId {
    Odds,
    Moneyline,
    Spreads,
    Totals,
    Props,
    FanDuelProps,
    BetMgmProps,
    Bet365Props,
    PropsHistory,
    ScoresAll,
    ScoresSport,
    Stats,
    Averages,
    KalshiMarkets,
    KalshiSeries,
    KalshiSeriesMarkets,
    PolymarketMarkets,
    HistoryGames,
    HistoryOdds,
    HistoryProps,
    HistoryStats,
    HistoryAverages,
    TennisStats,
    Cs2Matches,
    Cs2Match,
    Cs2Players,
    Realtime,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OwlsDashboard {
    pub sport: String,
    pub status_line: String,
    pub refreshed_at: String,
    pub last_sync_mode: String,
    pub sync_checks: usize,
    pub sync_changes: usize,
    pub total_polls: usize,
    pub groups: Vec<OwlsGroupSummary>,
    pub endpoints: Vec<OwlsEndpointSummary>,
    pub team_normalizations: Vec<OwlsTeamNormalization>,
    #[serde(skip)]
    seeds: OwlsSeeds,
}

impl Default for OwlsDashboard {
    fn default() -> Self {
        dashboard_for_sport(DEFAULT_SPORT)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OwlsGroupSummary {
    pub group: OwlsEndpointGroup,
    pub label: String,
    pub ready: usize,
    pub total: usize,
    pub error: usize,
    pub waiting: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OwlsEndpointSummary {
    pub id: OwlsEndpointId,
    pub group: OwlsEndpointGroup,
    pub label: String,
    pub method: String,
    pub path: String,
    pub description: String,
    pub query_hint: String,
    pub status: String,
    pub count: usize,
    pub updated_at: String,
    pub poll_count: usize,
    pub change_count: usize,
    pub detail: String,
    pub preview: Vec<OwlsPreviewRow>,
    pub requested_books: Vec<String>,
    pub available_books: Vec<String>,
    pub books_returned: Vec<String>,
    pub freshness_age_seconds: Option<u64>,
    pub freshness_stale: Option<bool>,
    pub freshness_threshold_seconds: Option<u64>,
    pub quote_count: usize,
    pub market_selections: Vec<OwlsMarketSelection>,
    pub quotes: Vec<OwlsMarketQuote>,
    pub live_scores: Vec<OwlsLiveScoreEvent>,
    #[serde(skip)]
    last_checked_at: Option<Instant>,
}

impl OwlsEndpointSummary {
    fn from_spec(spec: &OwlsEndpointSpec) -> Self {
        Self::from_spec_for_sport(spec, DEFAULT_SPORT)
    }

    fn from_spec_for_sport(spec: &OwlsEndpointSpec, sport: &str) -> Self {
        Self {
            id: spec.id,
            group: spec.group,
            label: spec.label_for_sport(sport),
            method: String::from("GET"),
            path: spec.path.replace("{sport}", sport),
            description: String::from(spec.description),
            query_hint: String::from(spec.query_hint),
            status: String::from("idle"),
            count: 0,
            updated_at: String::new(),
            poll_count: 0,
            change_count: 0,
            detail: String::from("Press r to hydrate"),
            preview: Vec::new(),
            requested_books: Vec::new(),
            available_books: Vec::new(),
            books_returned: Vec::new(),
            freshness_age_seconds: None,
            freshness_stale: None,
            freshness_threshold_seconds: None,
            quote_count: 0,
            market_selections: Vec::new(),
            quotes: Vec::new(),
            live_scores: Vec::new(),
            last_checked_at: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct OwlsPreviewRow {
    pub label: String,
    pub detail: String,
    pub metric: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct OwlsMarketQuote {
    pub book: String,
    pub event: String,
    pub selection: String,
    pub market_key: String,
    pub point: Option<f64>,
    pub decimal_price: Option<f64>,
    pub american_price: Option<f64>,
    pub limit_amount: Option<f64>,
    pub event_link: String,
    pub league: String,
    pub country_code: String,
    pub suspended: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct OwlsMarketSelection {
    pub event: String,
    pub market_key: String,
    pub selection: String,
    pub point: Option<f64>,
    pub league: String,
    pub country_code: String,
    pub quotes: Vec<OwlsMarketQuote>,
}

impl OwlsMarketSelection {
    pub fn quote_count(&self) -> usize {
        self.quotes.len()
    }

    pub fn best_price(&self) -> Option<f64> {
        self.quotes
            .iter()
            .filter_map(|quote| quote.decimal_price)
            .max_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
    }

    pub fn low_price(&self) -> Option<f64> {
        self.quotes
            .iter()
            .filter_map(|quote| quote.decimal_price)
            .min_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
    }

    pub fn books(&self) -> usize {
        self.quotes
            .iter()
            .map(|quote| normalize_key(&quote.book))
            .collect::<BTreeSet<_>>()
            .len()
    }

    pub fn market_label(&self) -> String {
        match self.point {
            Some(point) => format!("{} {point:+}", self.market_key),
            None => self.market_key.clone(),
        }
    }

    pub fn selection_label(&self) -> String {
        if self.selection.trim().is_empty() {
            String::from("-")
        } else {
            self.selection.clone()
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct OwlsTeamNormalization {
    pub input: String,
    pub canonical: String,
    pub simplified: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct OwlsLiveStat {
    pub key: String,
    pub label: String,
    pub home_value: String,
    pub away_value: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct OwlsLiveIncident {
    pub minute: Option<u64>,
    pub incident_type: String,
    pub team_side: String,
    pub player_name: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct OwlsPlayerRating {
    pub player_name: String,
    pub team_side: String,
    pub rating: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct OwlsLiveScoreEvent {
    pub sport: String,
    pub event_id: String,
    pub name: String,
    pub home_team: String,
    pub away_team: String,
    pub home_score: Option<i64>,
    pub away_score: Option<i64>,
    pub status_state: String,
    pub status_detail: String,
    pub display_clock: String,
    pub source_match_id: String,
    pub last_updated: String,
    pub stats: Vec<OwlsLiveStat>,
    pub incidents: Vec<OwlsLiveIncident>,
    pub player_ratings: Vec<OwlsPlayerRating>,
}

#[derive(Debug, Clone)]
struct OwlsEndpointSpec {
    id: OwlsEndpointId,
    group: OwlsEndpointGroup,
    label: &'static str,
    path: &'static str,
    description: &'static str,
    query_hint: &'static str,
}

impl OwlsEndpointSpec {
    fn label_for_sport(&self, sport: &str) -> String {
        match self.id {
            OwlsEndpointId::ScoresSport => format!("Scores {}", sport.to_uppercase()),
            _ => String::from(self.label),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct OwlsSeeds {
    history_event_id: Option<String>,
    tennis_event_id: Option<String>,
    props_game_id: Option<String>,
    props_player: Option<String>,
    props_category: Option<String>,
    kalshi_series_ticker: Option<String>,
    cs2_match_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwlsSyncReason {
    Manual,
    Background,
}

impl OwlsSyncReason {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Background => "monitor",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OwlsSyncOutcome {
    pub dashboard: OwlsDashboard,
    pub checked_count: usize,
    pub changed_count: usize,
    pub changed: bool,
}

pub fn build_client() -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .wrap_err("failed to build Owls Insight HTTP client")
}

pub fn build_async_client() -> Result<AsyncClient> {
    AsyncClient::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .wrap_err("failed to build Owls Insight async HTTP client")
}

pub fn fetch_dashboard(client: &Client) -> OwlsDashboard {
    sync_dashboard(
        client,
        &dashboard_for_sport(DEFAULT_SPORT),
        OwlsSyncReason::Manual,
        None,
    )
    .dashboard
}

pub fn dashboard_for_sport(sport: &str) -> OwlsDashboard {
    let normalized_sport = normalize_sport(sport);
    let endpoints = catalog_specs()
        .iter()
        .map(|spec| OwlsEndpointSummary::from_spec_for_sport(spec, &normalized_sport))
        .collect::<Vec<_>>();
    let groups = build_group_summaries(&endpoints);
    OwlsDashboard {
        sport: normalized_sport,
        status_line: startup_status_line(),
        refreshed_at: String::new(),
        last_sync_mode: String::from("idle"),
        sync_checks: 0,
        sync_changes: 0,
        total_polls: 0,
        groups,
        endpoints,
        team_normalizations: Vec::new(),
        seeds: OwlsSeeds::default(),
    }
}

pub fn dashboard_for_trading_section(sport: &str, section: TradingSection) -> OwlsDashboard {
    let normalized_sport = normalize_sport(sport);
    let endpoints = endpoint_ids_for_trading_section(section)
        .iter()
        .map(|id| OwlsEndpointSummary::from_spec_for_sport(spec_for(*id), &normalized_sport))
        .collect::<Vec<_>>();
    let groups = build_group_summaries(&endpoints);
    OwlsDashboard {
        sport: normalized_sport,
        status_line: startup_status_line(),
        refreshed_at: String::new(),
        last_sync_mode: String::from("idle"),
        sync_checks: 0,
        sync_changes: 0,
        total_polls: 0,
        groups,
        endpoints,
        team_normalizations: Vec::new(),
        seeds: OwlsSeeds::default(),
    }
}

pub fn endpoint_ids_for_trading_section(section: TradingSection) -> &'static [OwlsEndpointId] {
    match section {
        TradingSection::Live => &[OwlsEndpointId::Realtime, OwlsEndpointId::ScoresSport],
        TradingSection::Props => &[OwlsEndpointId::Props, OwlsEndpointId::PropsHistory],
        _ => &[OwlsEndpointId::Odds],
    }
}

pub fn sync_dashboard(
    client: &Client,
    previous: &OwlsDashboard,
    reason: OwlsSyncReason,
    focused: Option<OwlsEndpointId>,
) -> OwlsSyncOutcome {
    let mut dashboard = previous.clone();
    if dashboard.endpoints.is_empty() {
        dashboard = dashboard_for_sport(&dashboard.sport);
    }
    let sport = normalize_sport(&dashboard.sport);
    dashboard.sport = sport.clone();

    let api_key = match load_api_key() {
        Ok(value) => value,
        Err(error) => {
            let previous_dashboard = dashboard.clone();
            let detail = normalize_owls_error(&error.to_string());
            if is_missing_owls_api_key_detail(&detail) {
                mark_all_endpoints_waiting(&mut dashboard, &detail);
                dashboard.status_line = missing_owls_api_key_status_line();
            } else {
                mark_all_endpoints_error(&mut dashboard, &detail);
                dashboard.status_line = format!("Owls unavailable: {detail}");
            }
            dashboard.last_sync_mode = String::from(reason.label());
            dashboard.groups = build_group_summaries(&dashboard.endpoints);
            return OwlsSyncOutcome {
                changed: dashboard_semantically_changed(&previous_dashboard, &dashboard),
                dashboard,
                checked_count: 0,
                changed_count: 0,
            };
        }
    };

    let base_url = env::var("OWLS_INSIGHT_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| String::from(DEFAULT_BASE_URL));
    let due_ids = due_endpoint_ids(&dashboard, reason, focused);

    if due_ids.is_empty() {
        dashboard.last_sync_mode = String::from(reason.label());
        return OwlsSyncOutcome {
            dashboard,
            checked_count: 0,
            changed_count: 0,
            changed: false,
        };
    }

    let previous_dashboard = dashboard.clone();
    let mut checked_count = 0usize;
    let mut changed_count = 0usize;

    for id in due_ids {
        checked_count += 1;
        let summary = fetch_endpoint_summary(
            client,
            &base_url,
            &api_key,
            &sport,
            id,
            &mut dashboard.seeds,
        );
        if merge_endpoint(&mut dashboard, summary) {
            changed_count += 1;
        }
    }

    if sport_supports_team_normalization(&sport) {
        refresh_dashboard_team_normalizations(client, &base_url, &api_key, &sport, &mut dashboard);
    }

    dashboard.groups = build_group_summaries(&dashboard.endpoints);
    dashboard.refreshed_at = dashboard
        .endpoints
        .iter()
        .find_map(|endpoint| {
            (!endpoint.updated_at.is_empty()).then_some(endpoint.updated_at.clone())
        })
        .unwrap_or_else(|| previous_dashboard.refreshed_at.clone());
    dashboard.last_sync_mode = String::from(reason.label());
    dashboard.sync_checks = checked_count;
    dashboard.sync_changes = changed_count;
    dashboard.total_polls += checked_count;
    dashboard.status_line = if changed_count == 0 {
        format!(
            "Owls monitor steady: checked {checked_count} endpoint{} and found no changes.",
            if checked_count == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "Owls {} sync applied {changed_count} endpoint change{} after checking {checked_count}.",
            reason.label(),
            if changed_count == 1 { "" } else { "s" }
        )
    };

    OwlsSyncOutcome {
        changed: dashboard_semantically_changed(&previous_dashboard, &dashboard),
        dashboard,
        checked_count,
        changed_count,
    }
}

pub async fn sync_dashboard_async(
    client: &AsyncClient,
    previous: &OwlsDashboard,
    reason: OwlsSyncReason,
    section: TradingSection,
) -> OwlsSyncOutcome {
    let sport = normalize_sport(&previous.sport);
    let previous_dashboard = previous.clone();
    match fetch_backend_dashboard_async(client, &sabisabi_base_url(), &sport, section).await {
        Ok(mut dashboard) => {
            dashboard.sport = sport;
            dashboard.last_sync_mode = String::from(reason.label());
            dashboard.groups = build_group_summaries(&dashboard.endpoints);
            let checked_count = dashboard.endpoints.len();
            let changed = dashboard_semantically_changed(&previous_dashboard, &dashboard);
            let changed_count = if changed {
                count_changed_endpoints(&previous_dashboard.endpoints, &dashboard.endpoints)
            } else {
                0
            };
            dashboard.sync_checks = checked_count;
            dashboard.sync_changes = changed_count;
            dashboard.total_polls = previous_dashboard.total_polls.saturating_add(checked_count);
            OwlsSyncOutcome {
                changed,
                dashboard,
                checked_count,
                changed_count,
            }
        }
        Err(error) => {
            let mut dashboard = dashboard_for_trading_section(&sport, section);
            let detail = normalize_owls_error(&error.to_string());
            mark_all_endpoints_error(&mut dashboard, &detail);
            dashboard.status_line = format!("Owls backend unavailable: {detail}");
            dashboard.last_sync_mode = String::from(reason.label());
            dashboard.groups = build_group_summaries(&dashboard.endpoints);
            OwlsSyncOutcome {
                changed: dashboard_semantically_changed(&previous_dashboard, &dashboard),
                dashboard,
                checked_count: 0,
                changed_count: 0,
            }
        }
    }
}

pub fn find_pinnacle_quote(
    dashboard: &OwlsDashboard,
    event: &str,
    market: &str,
    selection: &str,
) -> Option<OwlsMarketQuote> {
    matching_market_quotes(dashboard, event, market, selection)
        .into_iter()
        .find(|quote| normalize_key(&quote.book) == "pinnacle")
}

pub fn matching_market_quotes(
    dashboard: &OwlsDashboard,
    event: &str,
    market: &str,
    selection: &str,
) -> Vec<OwlsMarketQuote> {
    let preferred = [
        OwlsEndpointId::Realtime,
        OwlsEndpointId::Moneyline,
        OwlsEndpointId::Odds,
        OwlsEndpointId::Spreads,
        OwlsEndpointId::Totals,
    ];
    let mut matches = Vec::new();
    for endpoint_id in preferred {
        let Some(endpoint) = dashboard
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == endpoint_id)
        else {
            continue;
        };
        if endpoint.quotes.is_empty() {
            for grouped in &endpoint.market_selections {
                if !event_matches_with_team_normalization(dashboard, &grouped.event, event) {
                    continue;
                }
                if !grouped.market_key.trim().is_empty() && !market_matches(&grouped.market_key, market) {
                    continue;
                }
                if !selection_matches_with_context(
                    &grouped.selection,
                    &grouped.event,
                    &grouped.market_key,
                    selection,
                    event,
                    market,
                ) {
                    continue;
                }
                matches.extend(grouped.quotes.iter().cloned());
            }
        } else {
            for quote in &endpoint.quotes {
                if quote_matches_target(dashboard, quote, event, market, selection) {
                    matches.push(quote.clone());
                }
            }
        }
    }
    matches
}

pub fn find_live_score(dashboard: &OwlsDashboard, event: &str) -> Option<OwlsLiveScoreEvent> {
    [OwlsEndpointId::ScoresSport, OwlsEndpointId::ScoresAll]
        .into_iter()
        .find_map(|endpoint_id| {
            dashboard
                .endpoints
                .iter()
                .find(|endpoint| endpoint.id == endpoint_id)
                .and_then(|endpoint| {
                    endpoint
                        .live_scores
                        .iter()
                        .find(|score| score_matches_target_event(dashboard, score, event))
                        .cloned()
                })
        })
}

fn quote_matches_target(
    dashboard: &OwlsDashboard,
    quote: &OwlsMarketQuote,
    event: &str,
    market: &str,
    selection: &str,
) -> bool {
    event_matches_with_team_normalization(dashboard, &quote.event, event)
        && (quote.market_key.trim().is_empty() || market_matches(&quote.market_key, market))
        && selection_matches_with_context(
            &quote.selection,
            &quote.event,
            &quote.market_key,
            selection,
            event,
            market,
        )
}

fn score_matches_target_event(
    dashboard: &OwlsDashboard,
    score: &OwlsLiveScoreEvent,
    event: &str,
) -> bool {
    event_matches_with_team_normalization(dashboard, &score.name, event)
}

fn due_endpoint_ids(
    dashboard: &OwlsDashboard,
    reason: OwlsSyncReason,
    focused: Option<OwlsEndpointId>,
) -> Vec<OwlsEndpointId> {
    if matches!(reason, OwlsSyncReason::Manual) {
        let mut prioritized = dashboard.endpoints.iter().collect::<Vec<_>>();
        prioritized.sort_by_key(|endpoint| manual_priority(endpoint, focused));
        return prioritized
            .into_iter()
            .take(MANUAL_SYNC_BATCH)
            .map(|endpoint| endpoint.id)
            .collect();
    }

    let now = Instant::now();
    let mut due = dashboard
        .endpoints
        .iter()
        .filter(|endpoint| endpoint_due(endpoint, now, focused))
        .collect::<Vec<_>>();
    due.sort_by_key(|endpoint| background_priority(endpoint, focused));
    due.into_iter()
        .take(BACKGROUND_SYNC_BATCH)
        .map(|endpoint| endpoint.id)
        .collect()
}

fn manual_priority(
    endpoint: &OwlsEndpointSummary,
    focused: Option<OwlsEndpointId>,
) -> (usize, usize, usize) {
    let focus_rank = if focused == Some(endpoint.id) { 0 } else { 1 };
    let freshness_rank = if endpoint.last_checked_at.is_none() {
        0
    } else {
        1
    };
    (focus_rank, group_rank(endpoint.group), freshness_rank)
}

fn endpoint_due(
    endpoint: &OwlsEndpointSummary,
    now: Instant,
    focused: Option<OwlsEndpointId>,
) -> bool {
    let Some(last_checked_at) = endpoint.last_checked_at else {
        return true;
    };
    let mut interval = endpoint_interval(endpoint.group);
    if focused == Some(endpoint.id) {
        interval = interval.min(HOT_SYNC_INTERVAL);
    }
    now.duration_since(last_checked_at) >= interval
}

fn endpoint_interval(group: OwlsEndpointGroup) -> Duration {
    match group {
        OwlsEndpointGroup::Realtime | OwlsEndpointGroup::Scores => HOT_SYNC_INTERVAL,
        OwlsEndpointGroup::Odds | OwlsEndpointGroup::Props => WARM_SYNC_INTERVAL,
        OwlsEndpointGroup::Stats | OwlsEndpointGroup::Prediction => COOL_SYNC_INTERVAL,
        OwlsEndpointGroup::History => COLD_SYNC_INTERVAL,
    }
}

fn background_priority(
    endpoint: &OwlsEndpointSummary,
    focused: Option<OwlsEndpointId>,
) -> (usize, usize, usize) {
    let focus_rank = if focused == Some(endpoint.id) { 0 } else { 1 };
    let freshness_rank = if endpoint.last_checked_at.is_none() {
        0
    } else {
        1
    };
    (focus_rank, freshness_rank, group_rank(endpoint.group))
}

fn group_rank(group: OwlsEndpointGroup) -> usize {
    match group {
        OwlsEndpointGroup::Realtime => 0,
        OwlsEndpointGroup::Scores => 1,
        OwlsEndpointGroup::Odds => 2,
        OwlsEndpointGroup::Props => 3,
        OwlsEndpointGroup::Stats => 4,
        OwlsEndpointGroup::Prediction => 5,
        OwlsEndpointGroup::History => 6,
    }
}

fn fetch_endpoint_summary(
    client: &Client,
    base_url: &str,
    api_key: &str,
    sport: &str,
    id: OwlsEndpointId,
    seeds: &mut OwlsSeeds,
) -> OwlsEndpointSummary {
    match id {
        OwlsEndpointId::Odds => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/odds"),
                &[("alternates", "true")],
            ),
            parse_book_market_summary,
        ),
        OwlsEndpointId::Moneyline => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/moneyline"),
                &[],
            ),
            parse_book_market_summary,
        ),
        OwlsEndpointId::Spreads => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/spreads"),
                &[("alternates", "true")],
            ),
            parse_book_market_summary,
        ),
        OwlsEndpointId::Totals => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/totals"),
                &[("alternates", "true")],
            ),
            parse_book_market_summary,
        ),
        OwlsEndpointId::Props => {
            let payload = fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/props"),
                &[("player", DEFAULT_BOOK_PROPS_PLAYER)],
            );
            if let Ok(value) = &payload {
                let (game_id, player, category) = first_props_seed(value);
                seeds.props_game_id = game_id;
                seeds.props_player = player;
                seeds.props_category = category;
            }
            hydrate_result(id, payload, parse_props_summary)
        }
        OwlsEndpointId::FanDuelProps => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/props/fanduel"),
                &[("player", DEFAULT_BOOK_PROPS_PLAYER)],
            ),
            parse_book_props_summary,
        ),
        OwlsEndpointId::BetMgmProps => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/props/betmgm"),
                &[("player", DEFAULT_BOOK_PROPS_PLAYER)],
            ),
            parse_book_props_summary,
        ),
        OwlsEndpointId::Bet365Props => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/props/bet365"),
                &[("player", DEFAULT_BOOK_PROPS_PLAYER)],
            ),
            parse_book_props_summary,
        ),
        OwlsEndpointId::PropsHistory => {
            ensure_props_seed(client, base_url, api_key, sport, seeds);
            match (
                seeds.props_game_id.as_deref(),
                seeds.props_player.as_deref(),
                seeds.props_category.as_deref(),
            ) {
                (Some(game_id), Some(player), Some(category)) => hydrate_result(
                    id,
                    fetch_json(
                        client,
                        base_url,
                        api_key,
                        &format!("/api/v1/{sport}/props/history"),
                        &[
                            ("game_id", game_id),
                            ("player", player),
                            ("category", category),
                            ("hours", "12"),
                        ],
                    ),
                    parse_props_history_summary,
                ),
                _ => waiting_summary(id, "Awaiting a sampled props game, player, and category."),
            }
        }
        OwlsEndpointId::ScoresAll => hydrate_result(
            id,
            fetch_json(client, base_url, api_key, "/api/v1/scores/live", &[]),
            parse_all_scores_summary,
        ),
        OwlsEndpointId::ScoresSport => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/scores/live"),
                &[],
            ),
            parse_scores_summary,
        ),
        OwlsEndpointId::Stats => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/stats"),
                &[("player", DEFAULT_PLAYER)],
            ),
            parse_stats_summary,
        ),
        OwlsEndpointId::Averages => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/stats/averages"),
                &[("playerName", DEFAULT_PLAYER)],
            ),
            parse_averages_summary,
        ),
        OwlsEndpointId::KalshiMarkets => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/kalshi/{sport}/markets"),
                &[("limit", "5")],
            ),
            parse_prediction_summary,
        ),
        OwlsEndpointId::KalshiSeries => {
            let payload = fetch_json(client, base_url, api_key, "/api/v1/kalshi/series", &[]);
            if let Ok(value) = &payload {
                seeds.kalshi_series_ticker = first_non_empty_string(
                    value.pointer("/data/0"),
                    &["series_ticker", "ticker", "seriesTicker"],
                );
            }
            hydrate_result(id, payload, parse_prediction_summary)
        }
        OwlsEndpointId::KalshiSeriesMarkets => {
            ensure_kalshi_series_seed(client, base_url, api_key, seeds);
            match seeds.kalshi_series_ticker.as_deref() {
                Some(series_ticker) => hydrate_result(
                    id,
                    fetch_json(
                        client,
                        base_url,
                        api_key,
                        &format!("/api/v1/kalshi/series/{series_ticker}/markets"),
                        &[("limit", "5")],
                    ),
                    parse_prediction_summary,
                ),
                None => waiting_summary(id, "Awaiting a Kalshi series ticker."),
            }
        }
        OwlsEndpointId::PolymarketMarkets => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/polymarket/{sport}/markets"),
                &[],
            ),
            parse_prediction_summary,
        ),
        OwlsEndpointId::HistoryGames => {
            let payload = fetch_json(
                client,
                base_url,
                api_key,
                "/api/v1/history/games",
                &[("sport", sport), ("limit", "1")],
            );
            if let Ok(value) = &payload {
                seeds.history_event_id = first_history_event_id(value);
            }
            hydrate_result(id, payload, parse_history_games_summary)
        }
        OwlsEndpointId::HistoryOdds => {
            ensure_history_seed(client, base_url, api_key, sport, seeds);
            match seeds.history_event_id.as_deref() {
                Some(event_id) => hydrate_result(
                    id,
                    fetch_json(
                        client,
                        base_url,
                        api_key,
                        "/api/v1/history/odds",
                        &[("eventId", event_id), ("limit", "5")],
                    ),
                    parse_history_snapshot_summary,
                ),
                None => waiting_summary(id, "Awaiting a sampled history event id."),
            }
        }
        OwlsEndpointId::HistoryProps => {
            ensure_history_seed(client, base_url, api_key, sport, seeds);
            match seeds.history_event_id.as_deref() {
                Some(event_id) => hydrate_result(
                    id,
                    fetch_json(
                        client,
                        base_url,
                        api_key,
                        "/api/v1/history/props",
                        &[
                            ("eventId", event_id),
                            ("playerName", DEFAULT_PLAYER),
                            ("propType", DEFAULT_PROP_TYPE),
                            ("limit", "5"),
                        ],
                    ),
                    parse_history_snapshot_summary,
                ),
                None => waiting_summary(id, "Awaiting a sampled history event id."),
            }
        }
        OwlsEndpointId::HistoryStats => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                "/api/v1/history/stats",
                &[
                    ("playerName", DEFAULT_PLAYER),
                    ("sport", sport),
                    ("limit", "5"),
                ],
            ),
            parse_history_stats_summary,
        ),
        OwlsEndpointId::HistoryAverages => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                "/api/v1/history/stats/averages",
                &[("playerName", DEFAULT_PLAYER), ("sport", sport)],
            ),
            parse_averages_summary,
        ),
        OwlsEndpointId::TennisStats => {
            ensure_tennis_seed(client, base_url, api_key, seeds);
            match seeds.tennis_event_id.as_deref() {
                Some(event_id) => hydrate_result(
                    id,
                    fetch_json(
                        client,
                        base_url,
                        api_key,
                        "/api/v1/history/tennis-stats",
                        &[("eventId", event_id)],
                    ),
                    parse_tennis_stats_summary,
                ),
                None => waiting_summary(id, "Awaiting a sampled tennis history event id."),
            }
        }
        OwlsEndpointId::Cs2Matches => {
            let payload = fetch_json(
                client,
                base_url,
                api_key,
                "/api/v1/history/cs2/matches",
                &[("limit", "1")],
            );
            if let Ok(value) = &payload {
                seeds.cs2_match_id = first_non_empty_string(
                    value
                        .pointer("/data/matches/0")
                        .or_else(|| value.pointer("/data/0")),
                    &["matchId", "id", "slug"],
                );
            }
            hydrate_result(id, payload, parse_cs2_matches_summary)
        }
        OwlsEndpointId::Cs2Match => {
            ensure_cs2_seed(client, base_url, api_key, seeds);
            match seeds.cs2_match_id.as_deref() {
                Some(match_id) => hydrate_result(
                    id,
                    fetch_json(
                        client,
                        base_url,
                        api_key,
                        &format!("/api/v1/history/cs2/matches/{match_id}"),
                        &[],
                    ),
                    parse_cs2_match_summary,
                ),
                None => waiting_summary(id, "Awaiting a sampled CS2 match id."),
            }
        }
        OwlsEndpointId::Cs2Players => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                "/api/v1/history/cs2/players",
                &[("limit", "5")],
            ),
            parse_cs2_players_summary,
        ),
        OwlsEndpointId::Realtime => hydrate_result(
            id,
            fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/realtime"),
                &[],
            ),
            parse_realtime_summary,
        ),
    }
}

async fn fetch_endpoint_summary_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    sport: &str,
    id: OwlsEndpointId,
    seeds: &mut OwlsSeeds,
) -> OwlsEndpointSummary {
    match id {
        OwlsEndpointId::Odds => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/odds"),
                &[("alternates", "true")],
            )
            .await,
            parse_book_market_summary,
        ),
        OwlsEndpointId::Moneyline => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/moneyline"),
                &[],
            )
            .await,
            parse_book_market_summary,
        ),
        OwlsEndpointId::Spreads => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/spreads"),
                &[("alternates", "true")],
            )
            .await,
            parse_book_market_summary,
        ),
        OwlsEndpointId::Totals => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/totals"),
                &[("alternates", "true")],
            )
            .await,
            parse_book_market_summary,
        ),
        OwlsEndpointId::Props => {
            let payload = fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/props"),
                &[("player", DEFAULT_BOOK_PROPS_PLAYER)],
            )
            .await;
            if let Ok(value) = &payload {
                let (game_id, player, category) = first_props_seed(value);
                seeds.props_game_id = game_id;
                seeds.props_player = player;
                seeds.props_category = category;
            }
            hydrate_result(id, payload, parse_props_summary)
        }
        OwlsEndpointId::FanDuelProps => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/props/fanduel"),
                &[("player", DEFAULT_BOOK_PROPS_PLAYER)],
            )
            .await,
            parse_book_props_summary,
        ),
        OwlsEndpointId::BetMgmProps => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/props/betmgm"),
                &[("player", DEFAULT_BOOK_PROPS_PLAYER)],
            )
            .await,
            parse_book_props_summary,
        ),
        OwlsEndpointId::Bet365Props => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/props/bet365"),
                &[("player", DEFAULT_BOOK_PROPS_PLAYER)],
            )
            .await,
            parse_book_props_summary,
        ),
        OwlsEndpointId::PropsHistory => {
            ensure_props_seed_async(client, base_url, api_key, sport, seeds).await;
            match (
                seeds.props_game_id.as_deref(),
                seeds.props_player.as_deref(),
                seeds.props_category.as_deref(),
            ) {
                (Some(game_id), Some(player), Some(category)) => hydrate_result(
                    id,
                    fetch_json_async(
                        client,
                        base_url,
                        api_key,
                        &format!("/api/v1/{sport}/props/history"),
                        &[
                            ("game_id", game_id),
                            ("player", player),
                            ("category", category),
                            ("hours", "12"),
                        ],
                    )
                    .await,
                    parse_props_history_summary,
                ),
                _ => waiting_summary(id, "Awaiting a sampled props game, player, and category."),
            }
        }
        OwlsEndpointId::ScoresAll => hydrate_result(
            id,
            fetch_json_async(client, base_url, api_key, "/api/v1/scores/live", &[]).await,
            parse_all_scores_summary,
        ),
        OwlsEndpointId::ScoresSport => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/scores/live"),
                &[],
            )
            .await,
            parse_scores_summary,
        ),
        OwlsEndpointId::Stats => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/stats"),
                &[("player", DEFAULT_PLAYER)],
            )
            .await,
            parse_stats_summary,
        ),
        OwlsEndpointId::Averages => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/stats/averages"),
                &[("playerName", DEFAULT_PLAYER)],
            )
            .await,
            parse_averages_summary,
        ),
        OwlsEndpointId::KalshiMarkets => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/kalshi/{sport}/markets"),
                &[("limit", "5")],
            )
            .await,
            parse_prediction_summary,
        ),
        OwlsEndpointId::KalshiSeries => {
            let payload =
                fetch_json_async(client, base_url, api_key, "/api/v1/kalshi/series", &[]).await;
            if let Ok(value) = &payload {
                seeds.kalshi_series_ticker = first_non_empty_string(
                    value.pointer("/data/0"),
                    &["series_ticker", "ticker", "seriesTicker"],
                );
            }
            hydrate_result(id, payload, parse_prediction_summary)
        }
        OwlsEndpointId::KalshiSeriesMarkets => {
            ensure_kalshi_series_seed_async(client, base_url, api_key, seeds).await;
            match seeds.kalshi_series_ticker.as_deref() {
                Some(series_ticker) => hydrate_result(
                    id,
                    fetch_json_async(
                        client,
                        base_url,
                        api_key,
                        &format!("/api/v1/kalshi/series/{series_ticker}/markets"),
                        &[("limit", "5")],
                    )
                    .await,
                    parse_prediction_summary,
                ),
                None => waiting_summary(id, "Awaiting a Kalshi series ticker."),
            }
        }
        OwlsEndpointId::PolymarketMarkets => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                &format!("/api/v1/polymarket/{sport}/markets"),
                &[],
            )
            .await,
            parse_prediction_summary,
        ),
        OwlsEndpointId::HistoryGames => {
            let payload = fetch_json_async(
                client,
                base_url,
                api_key,
                "/api/v1/history/games",
                &[("sport", sport), ("limit", "1")],
            )
            .await;
            if let Ok(value) = &payload {
                seeds.history_event_id = first_history_event_id(value);
            }
            hydrate_result(id, payload, parse_history_games_summary)
        }
        OwlsEndpointId::HistoryOdds => {
            ensure_history_seed_async(client, base_url, api_key, sport, seeds).await;
            match seeds.history_event_id.as_deref() {
                Some(event_id) => hydrate_result(
                    id,
                    fetch_json_async(
                        client,
                        base_url,
                        api_key,
                        "/api/v1/history/odds",
                        &[("eventId", event_id), ("limit", "5")],
                    )
                    .await,
                    parse_history_snapshot_summary,
                ),
                None => waiting_summary(id, "Awaiting a sampled history event id."),
            }
        }
        OwlsEndpointId::HistoryProps => {
            ensure_history_seed_async(client, base_url, api_key, sport, seeds).await;
            match seeds.history_event_id.as_deref() {
                Some(event_id) => hydrate_result(
                    id,
                    fetch_json_async(
                        client,
                        base_url,
                        api_key,
                        "/api/v1/history/props",
                        &[
                            ("eventId", event_id),
                            ("playerName", DEFAULT_PLAYER),
                            ("propType", DEFAULT_PROP_TYPE),
                            ("limit", "5"),
                        ],
                    )
                    .await,
                    parse_history_snapshot_summary,
                ),
                None => waiting_summary(id, "Awaiting a sampled history event id."),
            }
        }
        OwlsEndpointId::HistoryStats => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                "/api/v1/history/stats",
                &[
                    ("playerName", DEFAULT_PLAYER),
                    ("sport", sport),
                    ("limit", "5"),
                ],
            )
            .await,
            parse_history_stats_summary,
        ),
        OwlsEndpointId::HistoryAverages => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                "/api/v1/history/stats/averages",
                &[("playerName", DEFAULT_PLAYER), ("sport", sport)],
            )
            .await,
            parse_averages_summary,
        ),
        OwlsEndpointId::TennisStats => {
            ensure_tennis_seed_async(client, base_url, api_key, seeds).await;
            match seeds.tennis_event_id.as_deref() {
                Some(event_id) => hydrate_result(
                    id,
                    fetch_json_async(
                        client,
                        base_url,
                        api_key,
                        "/api/v1/history/tennis-stats",
                        &[("eventId", event_id)],
                    )
                    .await,
                    parse_tennis_stats_summary,
                ),
                None => waiting_summary(id, "Awaiting a sampled tennis history event id."),
            }
        }
        OwlsEndpointId::Cs2Matches => {
            let payload = fetch_json_async(
                client,
                base_url,
                api_key,
                "/api/v1/history/cs2/matches",
                &[("limit", "1")],
            )
            .await;
            if let Ok(value) = &payload {
                seeds.cs2_match_id = first_non_empty_string(
                    value
                        .pointer("/data/matches/0")
                        .or_else(|| value.pointer("/data/0")),
                    &["matchId", "id", "slug"],
                );
            }
            hydrate_result(id, payload, parse_cs2_matches_summary)
        }
        OwlsEndpointId::Cs2Match => {
            ensure_cs2_seed_async(client, base_url, api_key, seeds).await;
            match seeds.cs2_match_id.as_deref() {
                Some(match_id) => hydrate_result(
                    id,
                    fetch_json_async(
                        client,
                        base_url,
                        api_key,
                        &format!("/api/v1/history/cs2/matches/{match_id}"),
                        &[],
                    )
                    .await,
                    parse_cs2_match_summary,
                ),
                None => waiting_summary(id, "Awaiting a sampled CS2 match id."),
            }
        }
        OwlsEndpointId::Cs2Players => hydrate_result(
            id,
            fetch_json_async(
                client,
                base_url,
                api_key,
                "/api/v1/history/cs2/players",
                &[("limit", "5")],
            )
            .await,
            parse_cs2_players_summary,
        ),
        OwlsEndpointId::Realtime => hydrate_result(
            id,
            fetch_realtime_payload_async(client, base_url, api_key, sport).await,
            parse_realtime_summary,
        ),
    }
}

fn ensure_history_seed(
    client: &Client,
    base_url: &str,
    api_key: &str,
    sport: &str,
    seeds: &mut OwlsSeeds,
) {
    if seeds.history_event_id.is_some() {
        return;
    }
    let payload = fetch_json(
        client,
        base_url,
        api_key,
        "/api/v1/history/games",
        &[("sport", sport), ("limit", "1")],
    );
    if let Ok(value) = payload {
        seeds.history_event_id = first_history_event_id(&value);
    }
}

async fn ensure_history_seed_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    sport: &str,
    seeds: &mut OwlsSeeds,
) {
    if seeds.history_event_id.is_some() {
        return;
    }
    let payload = fetch_json_async(
        client,
        base_url,
        api_key,
        "/api/v1/history/games",
        &[("sport", sport), ("limit", "1")],
    )
    .await;
    if let Ok(value) = payload {
        seeds.history_event_id = first_history_event_id(&value);
    }
}

fn ensure_tennis_seed(client: &Client, base_url: &str, api_key: &str, seeds: &mut OwlsSeeds) {
    if seeds.tennis_event_id.is_some() {
        return;
    }
    let payload = fetch_json(
        client,
        base_url,
        api_key,
        "/api/v1/history/games",
        &[("sport", "tennis"), ("limit", "1")],
    );
    if let Ok(value) = payload {
        seeds.tennis_event_id = first_history_event_id(&value);
    }
}

async fn ensure_tennis_seed_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    seeds: &mut OwlsSeeds,
) {
    if seeds.tennis_event_id.is_some() {
        return;
    }
    let payload = fetch_json_async(
        client,
        base_url,
        api_key,
        "/api/v1/history/games",
        &[("sport", "tennis"), ("limit", "1")],
    )
    .await;
    if let Ok(value) = payload {
        seeds.tennis_event_id = first_history_event_id(&value);
    }
}

fn ensure_props_seed(
    client: &Client,
    base_url: &str,
    api_key: &str,
    sport: &str,
    seeds: &mut OwlsSeeds,
) {
    if seeds.props_game_id.is_some()
        && seeds.props_player.is_some()
        && seeds.props_category.is_some()
    {
        return;
    }
    let payload = fetch_json(
        client,
        base_url,
        api_key,
        &format!("/api/v1/{sport}/props"),
        &[("player", DEFAULT_BOOK_PROPS_PLAYER)],
    );
    if let Ok(value) = payload {
        let (game_id, player, category) = first_props_seed(&value);
        seeds.props_game_id = game_id;
        seeds.props_player = player;
        seeds.props_category = category;
    }
}

async fn ensure_props_seed_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    sport: &str,
    seeds: &mut OwlsSeeds,
) {
    if seeds.props_game_id.is_some()
        && seeds.props_player.is_some()
        && seeds.props_category.is_some()
    {
        return;
    }
    let payload = fetch_json_async(
        client,
        base_url,
        api_key,
        &format!("/api/v1/{sport}/props"),
        &[("player", DEFAULT_BOOK_PROPS_PLAYER)],
    )
    .await;
    if let Ok(value) = payload {
        let (game_id, player, category) = first_props_seed(&value);
        seeds.props_game_id = game_id;
        seeds.props_player = player;
        seeds.props_category = category;
    }
}

fn normalize_sport(value: &str) -> String {
    let trimmed = value.trim().to_lowercase();
    if trimmed.is_empty() {
        String::from(DEFAULT_SPORT)
    } else {
        trimmed
    }
}

fn sport_supports_team_normalization(sport: &str) -> bool {
    matches!(normalize_key(sport).as_str(), "soccer" | "tennis")
}

fn refresh_dashboard_team_normalizations(
    client: &Client,
    base_url: &str,
    api_key: &str,
    sport: &str,
    dashboard: &mut OwlsDashboard,
) {
    let team_names = collect_dashboard_team_names(dashboard);
    if team_names.is_empty() {
        return;
    }
    let known = dashboard
        .team_normalizations
        .iter()
        .map(|item| normalize_key(&item.input))
        .collect::<BTreeSet<_>>();
    let missing = team_names
        .into_iter()
        .filter(|name| !known.contains(&normalize_key(name)))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return;
    }
    if let Ok(fetched) = fetch_team_normalizations(client, base_url, api_key, sport, &missing) {
        for item in fetched {
            if item.input.trim().is_empty() {
                continue;
            }
            let key = normalize_key(&item.input);
            if dashboard
                .team_normalizations
                .iter()
                .any(|existing| normalize_key(&existing.input) == key)
            {
                continue;
            }
            dashboard.team_normalizations.push(item);
        }
    }
}

async fn refresh_dashboard_team_normalizations_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    sport: &str,
    dashboard: &mut OwlsDashboard,
) {
    let team_names = collect_dashboard_team_names(dashboard);
    if team_names.is_empty() {
        return;
    }
    let known = dashboard
        .team_normalizations
        .iter()
        .map(|item| normalize_key(&item.input))
        .collect::<BTreeSet<_>>();
    let missing = team_names
        .into_iter()
        .filter(|name| !known.contains(&normalize_key(name)))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return;
    }
    if let Ok(fetched) =
        fetch_team_normalizations_async(client, base_url, api_key, sport, &missing).await
    {
        for item in fetched {
            if item.input.trim().is_empty() {
                continue;
            }
            let key = normalize_key(&item.input);
            if dashboard
                .team_normalizations
                .iter()
                .any(|existing| normalize_key(&existing.input) == key)
            {
                continue;
            }
            dashboard.team_normalizations.push(item);
        }
    }
}

fn collect_dashboard_team_names(dashboard: &OwlsDashboard) -> Vec<String> {
    let mut names = BTreeSet::new();
    for endpoint in &dashboard.endpoints {
        for quote in &endpoint.quotes {
            if let Some((left, right)) = split_event_teams(&quote.event) {
                names.insert(left);
                names.insert(right);
            }
        }
        for score in &endpoint.live_scores {
            if !score.home_team.trim().is_empty() {
                names.insert(score.home_team.clone());
            }
            if !score.away_team.trim().is_empty() {
                names.insert(score.away_team.clone());
            }
            if let Some((left, right)) = split_event_teams(&score.name) {
                names.insert(left);
                names.insert(right);
            }
        }
    }
    names.into_iter().collect()
}

fn fetch_team_normalizations(
    client: &Client,
    base_url: &str,
    api_key: &str,
    sport: &str,
    team_names: &[String],
) -> Result<Vec<OwlsTeamNormalization>> {
    let mut rows = Vec::new();
    for chunk in team_names.chunks(25) {
        let joined = chunk
            .iter()
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        if joined.is_empty() {
            continue;
        }
        let query = [("names", joined.as_str()), ("sport", sport)];
        let payload = fetch_json(client, base_url, api_key, "/api/v1/normalize/batch", &query)?;
        rows.extend(parse_team_normalizations(&payload));
    }
    Ok(rows)
}

async fn fetch_team_normalizations_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    sport: &str,
    team_names: &[String],
) -> Result<Vec<OwlsTeamNormalization>> {
    let mut rows = Vec::new();
    for chunk in team_names.chunks(25) {
        let joined = chunk
            .iter()
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        if joined.is_empty() {
            continue;
        }
        let query = [("names", joined.as_str()), ("sport", sport)];
        let payload =
            fetch_json_async(client, base_url, api_key, "/api/v1/normalize/batch", &query).await?;
        rows.extend(parse_team_normalizations(&payload));
    }
    Ok(rows)
}

fn parse_team_normalizations(value: &Value) -> Vec<OwlsTeamNormalization> {
    let rows = extract_array(value, &["/results", "/data/results", ""]);
    if rows.is_empty() {
        return first_non_empty_string(Some(value), &["input"]).map_or_else(Vec::new, |_| {
            vec![OwlsTeamNormalization {
                input: first_non_empty(value, &["input"]),
                canonical: first_non_empty(value, &["canonical"]),
                simplified: first_non_empty(value, &["simplified"]),
            }]
        });
    }
    rows.into_iter()
        .map(|row| OwlsTeamNormalization {
            input: first_non_empty(&row, &["input"]),
            canonical: first_non_empty(&row, &["canonical"]),
            simplified: first_non_empty(&row, &["simplified"]),
        })
        .collect()
}

fn event_matches_with_team_normalization(
    dashboard: &OwlsDashboard,
    left: &str,
    right: &str,
) -> bool {
    event_matches(left, right)
        || match (
            canonical_event_pair(dashboard, left),
            canonical_event_pair(dashboard, right),
        ) {
            (Some(left_pair), Some(right_pair)) => left_pair == right_pair,
            _ => false,
        }
}

fn canonical_event_pair(dashboard: &OwlsDashboard, event: &str) -> Option<(String, String)> {
    let (left, right) = split_event_teams(event)?;
    let mut pair = [
        canonical_team_key(dashboard, &left),
        canonical_team_key(dashboard, &right),
    ];
    pair.sort();
    Some((pair[0].clone(), pair[1].clone()))
}

fn canonical_team_key(dashboard: &OwlsDashboard, name: &str) -> String {
    let normalized = normalize_key(name);
    dashboard
        .team_normalizations
        .iter()
        .find(|item| {
            normalize_key(&item.input) == normalized
                || normalize_key(&item.canonical) == normalized
                || normalize_key(&item.simplified) == normalized
        })
        .map(|item| {
            if !item.canonical.trim().is_empty() {
                simple_team_key(&item.canonical)
            } else if !item.simplified.trim().is_empty() {
                simple_team_key(&item.simplified)
            } else {
                simple_team_key(name)
            }
        })
        .unwrap_or_else(|| simple_team_key(name))
}

fn simple_team_key(name: &str) -> String {
    normalize_key(name)
        .split_whitespace()
        .filter(|token| {
            !matches!(
                *token,
                "fc" | "sc" | "cf" | "afc" | "bk" | "fk" | "club" | "de" | "ac"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_event_teams(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim();
    let lowercase = trimmed.to_ascii_lowercase();
    for separator in [" @ ", "@", " at ", " vs ", " v "] {
        if let Some(index) = lowercase.find(separator) {
            let left = trimmed[..index].trim();
            let right = trimmed[index + separator.len()..].trim();
            if !left.is_empty() && !right.is_empty() {
                return Some((left.to_string(), right.to_string()));
            }
        }
    }
    None
}

fn ensure_kalshi_series_seed(
    client: &Client,
    base_url: &str,
    api_key: &str,
    seeds: &mut OwlsSeeds,
) {
    if seeds.kalshi_series_ticker.is_some() {
        return;
    }
    let payload = fetch_json(client, base_url, api_key, "/api/v1/kalshi/series", &[]);
    if let Ok(value) = payload {
        seeds.kalshi_series_ticker = first_non_empty_string(
            value.pointer("/data/0"),
            &["series_ticker", "ticker", "seriesTicker"],
        );
    }
}

async fn ensure_kalshi_series_seed_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    seeds: &mut OwlsSeeds,
) {
    if seeds.kalshi_series_ticker.is_some() {
        return;
    }
    let payload = fetch_json_async(client, base_url, api_key, "/api/v1/kalshi/series", &[]).await;
    if let Ok(value) = payload {
        seeds.kalshi_series_ticker = first_non_empty_string(
            value.pointer("/data/0"),
            &["series_ticker", "ticker", "seriesTicker"],
        );
    }
}

fn ensure_cs2_seed(client: &Client, base_url: &str, api_key: &str, seeds: &mut OwlsSeeds) {
    if seeds.cs2_match_id.is_some() {
        return;
    }
    let payload = fetch_json(
        client,
        base_url,
        api_key,
        "/api/v1/history/cs2/matches",
        &[("limit", "1")],
    );
    if let Ok(value) = payload {
        seeds.cs2_match_id = first_non_empty_string(
            value
                .pointer("/data/matches/0")
                .or_else(|| value.pointer("/data/0")),
            &["matchId", "id", "slug"],
        );
    }
}

async fn ensure_cs2_seed_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    seeds: &mut OwlsSeeds,
) {
    if seeds.cs2_match_id.is_some() {
        return;
    }
    let payload = fetch_json_async(
        client,
        base_url,
        api_key,
        "/api/v1/history/cs2/matches",
        &[("limit", "1")],
    )
    .await;
    if let Ok(value) = payload {
        seeds.cs2_match_id = first_non_empty_string(
            value
                .pointer("/data/matches/0")
                .or_else(|| value.pointer("/data/0")),
            &["matchId", "id", "slug"],
        );
    }
}

fn merge_endpoint(dashboard: &mut OwlsDashboard, mut summary: OwlsEndpointSummary) -> bool {
    let checked_at = Instant::now();
    if let Some(slot) = dashboard
        .endpoints
        .iter_mut()
        .find(|endpoint| endpoint.id == summary.id)
    {
        summary.label = slot.label.clone();
        summary.path = slot.path.clone();
        let changed = endpoint_semantically_changed(slot, &summary);
        summary.poll_count = slot.poll_count + 1;
        summary.change_count = slot.change_count + usize::from(changed);
        summary.last_checked_at = Some(checked_at);
        if !changed {
            summary.preview = slot.preview.clone();
            summary.detail = slot.detail.clone();
            summary.count = slot.count;
            summary.status = slot.status.clone();
            summary.updated_at = slot.updated_at.clone();
            summary.requested_books = slot.requested_books.clone();
            summary.available_books = slot.available_books.clone();
            summary.books_returned = slot.books_returned.clone();
            summary.freshness_age_seconds = slot.freshness_age_seconds;
            summary.freshness_stale = slot.freshness_stale;
            summary.freshness_threshold_seconds = slot.freshness_threshold_seconds;
            summary.quote_count = slot.quote_count;
            summary.market_selections = slot.market_selections.clone();
            summary.quotes = slot.quotes.clone();
            summary.live_scores = slot.live_scores.clone();
        }
        *slot = summary;
        return changed;
    }
    summary.poll_count = 1;
    summary.change_count = 1;
    summary.last_checked_at = Some(checked_at);
    dashboard.endpoints.push(summary);
    true
}

fn endpoint_semantically_changed(
    current: &OwlsEndpointSummary,
    next: &OwlsEndpointSummary,
) -> bool {
    current.status != next.status
        || current.count != next.count
        || current.updated_at != next.updated_at
        || current.detail != next.detail
        || current.requested_books != next.requested_books
        || current.available_books != next.available_books
        || current.books_returned != next.books_returned
        || current.freshness_age_seconds != next.freshness_age_seconds
        || current.freshness_stale != next.freshness_stale
        || current.freshness_threshold_seconds != next.freshness_threshold_seconds
        || current.quote_count != next.quote_count
        || current.market_selections != next.market_selections
        || current.quotes != next.quotes
        || current.live_scores != next.live_scores
        || current.preview.len() != next.preview.len()
        || current
            .preview
            .iter()
            .zip(next.preview.iter())
            .any(|(left, right)| {
                left.label != right.label
                    || left.detail != right.detail
                    || left.metric != right.metric
            })
}

fn dashboard_semantically_changed(current: &OwlsDashboard, next: &OwlsDashboard) -> bool {
    current.team_normalizations != next.team_normalizations
        || current.endpoints.len() != next.endpoints.len()
        || current
            .endpoints
            .iter()
            .zip(next.endpoints.iter())
            .any(|(left, right)| endpoint_semantically_changed(left, right))
}

fn count_changed_endpoints(
    current_endpoints: &[OwlsEndpointSummary],
    next_endpoints: &[OwlsEndpointSummary],
) -> usize {
    current_endpoints
        .iter()
        .zip(next_endpoints.iter())
        .filter(|(left, right)| endpoint_semantically_changed(left, right))
        .count()
}

fn hydrate_result(
    id: OwlsEndpointId,
    payload: Result<Value>,
    parser: fn(OwlsEndpointId, &Value) -> OwlsEndpointSummary,
) -> OwlsEndpointSummary {
    match payload {
        Ok(value) => parser(id, &value),
        Err(error) => error_summary(id, &normalize_owls_error(&error.to_string())),
    }
}

fn error_summary(id: OwlsEndpointId, detail: &str) -> OwlsEndpointSummary {
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("error");
    summary.detail = truncate(&normalize_owls_error(detail), 88);
    summary
}

fn waiting_summary(id: OwlsEndpointId, detail: &str) -> OwlsEndpointSummary {
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("waiting");
    summary.detail = String::from(detail);
    summary
}

fn mark_all_endpoints_error(dashboard: &mut OwlsDashboard, detail: &str) {
    for endpoint in &mut dashboard.endpoints {
        endpoint.status = String::from("error");
        endpoint.detail = truncate(detail, 88);
    }
}

fn mark_all_endpoints_waiting(dashboard: &mut OwlsDashboard, detail: &str) {
    for endpoint in &mut dashboard.endpoints {
        endpoint.status = String::from("waiting");
        endpoint.detail = truncate(detail, 88);
    }
}

fn fetch_json(
    client: &Client,
    base_url: &str,
    api_key: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<Value> {
    let response = client
        .get(format!("{}{}", base_url.trim_end_matches('/'), path))
        .bearer_auth(api_key)
        .query(query)
        .send()
        .wrap_err_with(|| format!("request failed for {path}"))?;
    let status = response.status();
    let payload = response
        .text()
        .wrap_err("failed to read Owls response body")?;
    if !status.is_success() {
        return Err(eyre!(
            "HTTP {}: {}",
            status.as_u16(),
            truncate(&payload, 120)
        ));
    }
    serde_json::from_str(&payload).wrap_err("failed to decode Owls response")
}

static BACKEND_UNHEALTHY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

async fn fetch_json_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<Value> {
    if api_key.trim().is_empty() {
        if !BACKEND_UNHEALTHY.load(std::sync::atomic::Ordering::Relaxed) {
            match fetch_backend_json_async(client, base_url, path, query).await {
                Ok(value) => return Ok(value),
                Err(backend_error) => {
                    tracing::warn!("owls backend fetch failed for {path}: {backend_error}");
                    BACKEND_UNHEALTHY.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        let direct_api_key = load_api_key()?;
        let direct_base_url = env::var("OWLS_INSIGHT_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| String::from(DEFAULT_BASE_URL));
        return fetch_upstream_json_async(client, &direct_base_url, &direct_api_key, path, query)
            .await
            .wrap_err_with(|| format!("backend request failed for {path}"));
    }
    fetch_upstream_json_async(client, base_url, api_key, path, query).await
}

async fn fetch_upstream_json_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<Value> {
    let response = client
        .get(format!("{}{}", base_url.trim_end_matches('/'), path))
        .bearer_auth(api_key)
        .query(query)
        .send()
        .await
        .wrap_err_with(|| format!("request failed for {path}"))?;
    let status = response.status();
    let payload = response
        .text()
        .await
        .wrap_err("failed to read Owls response body")?;
    if !status.is_success() {
        return Err(eyre!(
            "HTTP {}: {}",
            status.as_u16(),
            truncate(&payload, 120)
        ));
    }
    serde_json::from_str(&payload).wrap_err("failed to decode Owls response")
}

async fn fetch_backend_dashboard_async(
    client: &AsyncClient,
    base_url: &str,
    sport: &str,
    section: TradingSection,
) -> Result<OwlsDashboard> {
    let response = client
        .get(format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            owls_backend_dashboard_path(sport)
        ))
        .query(&[("section", owls_section_key(section))])
        .send()
        .await
        .wrap_err_with(|| format!("backend dashboard request failed for {sport}"))?;
    let status = response.status();
    let payload = response
        .text()
        .await
        .wrap_err("failed to read backend Owls dashboard body")?;
    if !status.is_success() {
        return Err(eyre!(
            "HTTP {}: {}",
            status.as_u16(),
            truncate(&payload, 120)
        ));
    }
    serde_json::from_str(&payload).wrap_err("failed to decode backend Owls dashboard")
}

async fn fetch_backend_json_async(
    client: &AsyncClient,
    base_url: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<Value> {
    let response = client
        .get(format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            owls_backend_path(path)
        ))
        .query(query)
        .send()
        .await
        .wrap_err_with(|| format!("backend request failed for {path}"))?;
    let status = response.status();
    let payload = response
        .text()
        .await
        .wrap_err("failed to read backend Owls response body")?;
    if !status.is_success() {
        return Err(eyre!(
            "HTTP {}: {}",
            status.as_u16(),
            truncate(&payload, 120)
        ));
    }
    serde_json::from_str(&payload).wrap_err("failed to decode backend Owls response")
}

async fn fetch_realtime_payload_async(
    client: &AsyncClient,
    base_url: &str,
    api_key: &str,
    sport: &str,
) -> Result<Value> {
    fetch_json_async(
        client,
        base_url,
        api_key,
        &format!("/api/v1/{sport}/realtime"),
        &[],
    )
    .await
}

fn owls_backend_path(path: &str) -> String {
    let suffix = path.strip_prefix("/api/v1").unwrap_or(path);
    format!("/api/v1/owls{suffix}")
}

fn owls_backend_dashboard_path(sport: &str) -> String {
    format!("/api/v1/owls/dashboard/{sport}")
}

fn owls_section_key(section: TradingSection) -> &'static str {
    match section {
        TradingSection::Live => "live",
        TradingSection::Props => "props",
        _ => "markets",
    }
}

fn socketio_connect_url(base_url: &str, api_key: &str) -> Result<String> {
    let mut url = Url::parse(base_url).wrap_err("invalid Owls Socket.IO base URL")?;
    url.query_pairs_mut().append_pair("apiKey", api_key);
    Ok(url.to_string())
}

async fn fetch_socketio_realtime_payload(
    base_url: &str,
    api_key: &str,
    sport: &str,
) -> Result<Value> {
    let sport = sport.to_string();
    let payload_cell = Arc::new(Mutex::new(None::<Value>));
    let error_cell = Arc::new(Mutex::new(None::<String>));
    let socket_url = socketio_connect_url(base_url, api_key)?;

    let mut builder = SocketClientBuilder::new(socket_url)
        .transport_type(TransportType::Websocket)
        .reconnect(false)
        .namespace("/");

    for event_name in ["odds-update", "pinnacle-realtime", "ps3838-realtime"] {
        let payload_cell = Arc::clone(&payload_cell);
        let event_sport = sport.clone();
        builder = builder.on(event_name, move |payload: Payload, _| {
            let payload_cell = Arc::clone(&payload_cell);
            let event_sport = event_sport.clone();
            async move {
                if let Some(value) = first_json_value(payload)
                    .and_then(|value| normalize_socketio_realtime_payload(&value, &event_sport))
                {
                    if let Ok(mut slot) = payload_cell.lock() {
                        if slot.is_none() {
                            *slot = Some(value);
                        }
                    }
                }
            }
            .boxed()
        });
    }

    for event_name in ["error", "connect_error"] {
        let error_cell = Arc::clone(&error_cell);
        builder = builder.on(event_name, move |payload: Payload, _| {
            let error_cell = Arc::clone(&error_cell);
            async move {
                let detail = payload_debug_string(payload);
                if let Ok(mut slot) = error_cell.lock() {
                    if slot.is_none() {
                        *slot = Some(detail);
                    }
                }
            }
            .boxed()
        });
    }

    let socket = builder
        .connect()
        .await
        .wrap_err("socket.io connect failed")?;

    socket
        .emit(
            "subscribe",
            serde_json::json!({
                "sports": [sport],
                "alternates": true
            }),
        )
        .await
        .wrap_err("socket.io subscribe failed")?;

    let started = Instant::now();
    loop {
        if started.elapsed() >= SOCKET_TIMEOUT {
            break;
        }
        let maybe_payload = payload_cell.lock().ok().and_then(|mut slot| slot.take());
        if let Some(value) = maybe_payload {
            let _ = socket.disconnect().await;
            return Ok(value);
        }
        let maybe_error = error_cell.lock().ok().and_then(|slot| slot.clone());
        if let Some(error) = maybe_error {
            let _ = socket.disconnect().await;
            return Err(eyre!(normalize_owls_error(&error)));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let _ = socket.disconnect().await;
    Err(eyre!("socket.io timed out waiting for realtime payload"))
}

#[allow(deprecated)]
fn first_json_value(payload: Payload) -> Option<Value> {
    match payload {
        Payload::Text(values) => values.into_iter().next(),
        Payload::String(value) => serde_json::from_str(&value).ok(),
        Payload::Binary(_) => None,
    }
}

#[allow(deprecated)]
fn payload_debug_string(payload: Payload) -> String {
    match payload {
        Payload::Text(values) => serde_json::to_string(&values)
            .unwrap_or_else(|_| String::from("socket.io text payload")),
        Payload::String(value) => value,
        Payload::Binary(bytes) => format!("binary payload ({} bytes)", bytes.len()),
    }
}

fn normalize_socketio_realtime_payload(value: &Value, sport: &str) -> Option<Value> {
    if value.pointer("/data").is_some() {
        return Some(value.clone());
    }
    if let Some(events) = value
        .pointer(&format!("/sports/{sport}"))
        .and_then(Value::as_array)
    {
        return Some(serde_json::json!({
            "data": events,
            "meta": {
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "freshness": {
                    "ageSeconds": 0,
                    "stale": false,
                    "threshold": 3
                },
                "transport": "socket.io"
            }
        }));
    }
    if let Some(events) = value.as_array() {
        return Some(serde_json::json!({
            "data": events,
            "meta": {
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "freshness": {
                    "ageSeconds": 0,
                    "stale": false,
                    "threshold": 3
                },
                "transport": "socket.io"
            }
        }));
    }
    None
}

fn parse_book_market_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let books = value
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut preview = Vec::new();
    let mut event_count = 0usize;
    let mut quotes = Vec::new();
    for (book, events) in books {
        let Some(rows) = events.as_array() else {
            continue;
        };
        event_count += rows.len();
        for event in rows {
            quotes.extend(extract_market_quotes(event, Some(&book)));
        }
        if let Some(event) = rows.first() {
            preview.push(OwlsPreviewRow {
                label: matchup_label(event),
                detail: format!("{book} {}", stringify(event.get("commence_time"))),
                metric: first_market_price(event),
            });
        }
    }

    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = event_count;
    summary.updated_at = first_pointer_string(value, &["/meta/timestamp"]);
    summary.requested_books = string_array_at(value, "/meta/requestedBooks");
    summary.available_books = string_array_at(value, "/meta/availableBooks");
    summary.books_returned = string_array_at(value, "/meta/booksReturned");
    summary.freshness_age_seconds = unsigned_number_at(value, "/meta/freshness/ageSeconds");
    summary.freshness_stale = value
        .pointer("/meta/freshness/stale")
        .and_then(Value::as_bool);
    summary.freshness_threshold_seconds = unsigned_number_at(value, "/meta/freshness/threshold");
    summary.quote_count = quotes.len();
    summary.detail = format!(
        "books {} • market {} • age {}s{}",
        books_returned_len(value),
        first_pointer_string(value, &["/meta/market"]).if_empty("filtered"),
        summary
            .freshness_age_seconds
            .map(|value| value.to_string())
            .unwrap_or_else(|| String::from("-")),
        if summary.freshness_stale.unwrap_or(false) {
            " stale"
        } else {
            ""
        }
    );
    summary.preview = preview;
    summary.market_selections = build_market_selections(&quotes);
    summary
}

fn parse_props_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let meta = value.get("meta").unwrap_or(&Value::Null);
    let count = meta
        .get("propsReturned")
        .and_then(Value::as_u64)
        .unwrap_or_default() as usize;
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = count;
    summary.updated_at = stringify(meta.get("timestamp"));
    summary.detail = format!(
        "games {}",
        meta.get("gamesReturned")
            .and_then(Value::as_u64)
            .unwrap_or_default()
    );
    summary.preview = collect_book_event_preview(value, 4);
    summary
}

fn parse_book_props_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let games = extract_array(value, &["/data"]);
    let mut preview = Vec::new();
    let mut prop_count = 0usize;

    for game in games.iter().take(4) {
        let books = extract_array(game, &["/books"]);
        for book in books {
            let props = extract_array(&book, &["/props"]);
            prop_count += props.len();
            if let Some(first_prop) = props.first() {
                preview.push(OwlsPreviewRow {
                    label: format!(
                        "{} @ {}",
                        stringify(game.get("awayTeam")).if_empty("-"),
                        stringify(game.get("homeTeam")).if_empty("-")
                    ),
                    detail: format!(
                        "{} {}",
                        stringify(book.get("key")).if_empty("-"),
                        stringify(first_prop.get("playerName")).if_empty("-")
                    ),
                    metric: format!(
                        "{} {}",
                        stringify(first_prop.get("category")).if_empty("-"),
                        stringify(first_prop.get("line")).if_empty("-")
                    ),
                });
            }
        }
    }

    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = prop_count;
    summary.updated_at = first_pointer_string(value, &["/meta/timestamp"]);
    summary.detail = format!("games {}", games.len());
    summary.preview = preview;
    summary
}

fn parse_props_history_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let rows = ["/data/history", "/data/snapshots", "/data"]
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_array))
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let preview = rows
        .iter()
        .take(4)
        .map(|row| OwlsPreviewRow {
            label: first_non_empty(row, &["player", "playerName", "name", "category"]),
            detail: first_non_empty(row, &["category", "book", "timestamp", "recordedAt"]),
            metric: first_non_empty(row, &["odds", "price", "line", "overPrice", "underPrice"]),
        })
        .collect::<Vec<_>>();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = unsigned_number_at(value, "/meta/totalCount")
        .map(|count| count as usize)
        .unwrap_or(rows.len());
    summary.updated_at = first_pointer_string(value, &["/meta/timestamp", "/data/timestamp"]);
    summary.detail =
        first_pointer_string(value, &["/meta/game_id", "/data/gameId"]).if_empty("line history");
    summary.preview = preview;
    summary
}

fn parse_all_scores_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let sports = value
        .pointer("/data/sports")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut preview = Vec::new();
    let mut total = 0usize;
    let mut live_scores = Vec::new();

    for (sport, rows) in sports {
        let Some(events) = rows.as_array() else {
            continue;
        };
        total += events.len();
        live_scores.extend(
            events
                .iter()
                .map(|event| parse_live_score_event(&sport, event))
                .collect::<Vec<_>>(),
        );
        if let Some(event) = events.first() {
            preview.push(OwlsPreviewRow {
                label: sport,
                detail: stringify(event.get("name")),
                metric: format!(
                    "{}-{}",
                    stringify(event.pointer("/away/score")).if_empty("-"),
                    stringify(event.pointer("/home/score")).if_empty("-")
                ),
            });
        }
    }

    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = total;
    summary.updated_at = first_pointer_string(value, &["/data/timestamp"]);
    summary.detail = format!("sports {}", preview.len());
    summary.preview = preview;
    summary.live_scores = live_scores;
    summary
}

fn parse_scores_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let (sport, games) = extract_sport_score_games(value);
    let preview = games
        .iter()
        .take(4)
        .map(|event| OwlsPreviewRow {
            label: stringify(event.get("name")),
            detail: stringify(event.pointer("/status/detail")),
            metric: format!(
                "{}-{}",
                stringify(event.pointer("/away/score")).if_empty("-"),
                stringify(event.pointer("/home/score")).if_empty("-")
            ),
        })
        .collect();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = games.len();
    summary.updated_at = first_pointer_string(value, &["/data/timestamp"]);
    summary.detail = format!("live feed {}", sport.clone().if_empty("-"));
    summary.preview = preview;
    summary.live_scores = games
        .iter()
        .map(|event| parse_live_score_event(&sport, event))
        .collect();
    summary
}

fn extract_sport_score_games(value: &Value) -> (String, Vec<Value>) {
    let sports = value
        .pointer("/data/sports")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    sports
        .into_iter()
        .find_map(|(sport, rows)| rows.as_array().map(|items| (sport, items.clone())))
        .unwrap_or_else(|| (String::new(), Vec::new()))
}

fn parse_live_score_event(sport: &str, value: &Value) -> OwlsLiveScoreEvent {
    let home_team = first_non_empty_string(value.pointer("/home/team"), &["displayName", "name"])
        .unwrap_or_default();
    let away_team = first_non_empty_string(value.pointer("/away/team"), &["displayName", "name"])
        .unwrap_or_default();
    OwlsLiveScoreEvent {
        sport: sport.to_string(),
        event_id: first_non_empty(value, &["id"]),
        name: first_non_empty(value, &["name"]),
        home_team,
        away_team,
        home_score: integer_value(value.pointer("/home/score")),
        away_score: integer_value(value.pointer("/away/score")),
        status_state: first_non_empty_string(value.pointer("/status"), &["state"])
            .unwrap_or_default(),
        status_detail: first_non_empty_string(value.pointer("/status"), &["detail"])
            .unwrap_or_default(),
        display_clock: first_non_empty_string(value.pointer("/status"), &["displayClock"])
            .unwrap_or_default(),
        source_match_id: first_non_empty(value, &["sourceMatchId"]),
        last_updated: first_non_empty(value, &["lastUpdated"]),
        stats: parse_live_stats(value.pointer("/matchStats")),
        incidents: extract_array(value, &["/incidents"])
            .into_iter()
            .map(|row| OwlsLiveIncident {
                minute: row.get("minute").and_then(Value::as_u64),
                incident_type: first_non_empty(&row, &["type"]),
                team_side: first_non_empty(&row, &["teamSide"]),
                player_name: first_non_empty(&row, &["playerName"]),
                detail: first_non_empty(
                    &row,
                    &["assistPlayerName", "playerOut", "detail", "description"],
                ),
            })
            .collect(),
        player_ratings: extract_array(value, &["/playerStats"])
            .into_iter()
            .map(|row| OwlsPlayerRating {
                player_name: first_non_empty(&row, &["playerName"]),
                team_side: first_non_empty(&row, &["teamSide"]),
                rating: numeric_value(row.get("rating")),
            })
            .collect(),
    }
}

fn parse_live_stats(value: Option<&Value>) -> Vec<OwlsLiveStat> {
    let Some(stats) = value.and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for (key, entry) in stats {
        let home_value = stringify(entry.get("home"));
        let away_value = stringify(entry.get("away"));
        if home_value.is_empty() && away_value.is_empty() {
            continue;
        }
        rows.push(OwlsLiveStat {
            key: key.clone(),
            label: live_stat_label(key),
            home_value: home_value.if_empty("-"),
            away_value: away_value.if_empty("-"),
        });
    }
    rows
}

fn live_stat_label(key: &str) -> String {
    match key {
        "shotsOnTarget" => String::from("Shots OT"),
        "shotsOffTarget" => String::from("Shots Off"),
        "expectedGoals" => String::from("xG"),
        "bigChances" => String::from("Big Ch"),
        "yellowCards" => String::from("Yellow"),
        "redCards" => String::from("Red"),
        "goalkeeperSaves" => String::from("Saves"),
        "shotsInsideBox" => String::from("In Box"),
        "shotsOutsideBox" => String::from("Out Box"),
        "freeKicks" => String::from("FK"),
        "throwIns" => String::from("Throw"),
        other => other
            .chars()
            .enumerate()
            .fold(String::new(), |mut label, (index, character)| {
                if index > 0 && character.is_ascii_uppercase() {
                    label.push(' ');
                }
                label.push(character);
                label
            }),
    }
}

fn parse_stats_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let stats = extract_array(value, &["/data/stats"]);
    let preview = stats
        .iter()
        .take(4)
        .map(|row| OwlsPreviewRow {
            label: stringify(row.pointer("/player/name")),
            detail: stringify(row.pointer("/team/name")),
            metric: format!(
                "pts {} reb {} ast {}",
                stringify(row.pointer("/stats/points")).if_empty("-"),
                stringify(row.pointer("/stats/rebounds")).if_empty("-"),
                stringify(row.pointer("/stats/assists")).if_empty("-")
            ),
        })
        .collect();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = stats.len();
    summary.updated_at = first_pointer_string(value, &["/meta/timestamp"]);
    summary.detail = String::from(DEFAULT_PLAYER);
    summary.preview = preview;
    summary
}

fn parse_averages_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let data = value.get("data").unwrap_or(&Value::Null);
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = data.as_object().map(|item| item.len()).unwrap_or(0);
    summary.updated_at = first_pointer_string(value, &["/meta/timestamp"]);
    summary.detail = format!(
        "player {}",
        stringify(data.get("playerName")).if_empty(DEFAULT_PLAYER)
    );
    summary.preview = vec![OwlsPreviewRow {
        label: stringify(data.get("playerName")).if_empty(DEFAULT_PLAYER),
        detail: format!("games {}", stringify(data.get("games")).if_empty("-")),
        metric: format!(
            "pts {} reb {} ast {}",
            stringify(data.get("points")).if_empty("-"),
            stringify(data.get("rebounds")).if_empty("-"),
            stringify(data.get("assists")).if_empty("-")
        ),
    }];
    summary
}

fn parse_prediction_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let markets = extract_array(value, &["/data/markets", "/data/events", "/data"]);
    let preview = markets
        .iter()
        .take(4)
        .map(|row| OwlsPreviewRow {
            label: first_non_empty(
                row,
                &[
                    "title",
                    "question",
                    "name",
                    "ticker",
                    "marketTitle",
                    "eventTitle",
                ],
            ),
            detail: first_non_empty(row, &["status", "subtitle", "seriesTicker", "slug"]),
            metric: first_non_empty(row, &["ticker", "volume", "liquidity", "id"]),
        })
        .collect::<Vec<_>>();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = markets.len();
    summary.updated_at = first_pointer_string(value, &["/meta/timestamp"]);
    summary.detail = String::from("markets");
    summary.preview = preview;
    summary
}

fn parse_history_games_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let games = extract_array(value, &["/data/games"]);
    let preview = games
        .iter()
        .take(4)
        .map(|row| OwlsPreviewRow {
            label: format!(
                "{} @ {}",
                stringify(row.get("awayTeam")).if_empty("-"),
                stringify(row.get("homeTeam")).if_empty("-")
            ),
            detail: stringify(row.get("gameDate")),
            metric: format!(
                "snaps {}",
                stringify(row.get("oddsSnapshots")).if_empty("-")
            ),
        })
        .collect();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = games.len();
    summary.detail = String::from("archive");
    summary.preview = preview;
    summary
}

fn parse_history_snapshot_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let snapshots = extract_array(value, &["/data/snapshots", "/data"]);
    let preview = snapshots
        .iter()
        .take(4)
        .map(|row| OwlsPreviewRow {
            label: first_non_empty(row, &["playerName", "book", "side", "market"]),
            detail: first_non_empty(row, &["market", "propType", "book", "side"]),
            metric: first_non_empty(
                row,
                &["price", "line", "overPrice", "underPrice", "recordedAt"],
            ),
        })
        .collect();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = snapshots.len();
    summary.updated_at = first_pointer_string(value, &["/data/timeRange/end"]);
    summary.detail = format!(
        "event {}",
        first_pointer_string(value, &["/data/eventId"]).if_empty("-")
    );
    summary.preview = preview;
    summary
}

fn parse_history_stats_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let stats = extract_array(value, &["/data/stats", "/data"]);
    let preview = stats
        .iter()
        .take(4)
        .map(|row| OwlsPreviewRow {
            label: stringify(row.pointer("/player/name")).if_empty(DEFAULT_PLAYER),
            detail: stringify(row.get("gameDate")).if_empty("-"),
            metric: format!(
                "pts {}",
                stringify(row.pointer("/stats/points")).if_empty("-")
            ),
        })
        .collect();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = stats.len();
    summary.detail = String::from(DEFAULT_PLAYER);
    summary.preview = preview;
    summary
}

fn parse_tennis_stats_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let stats = extract_array(value, &["/data/stats"]);
    let preview = stats
        .iter()
        .take(4)
        .map(|row| OwlsPreviewRow {
            label: stringify(row.get("scope")).if_empty("match"),
            detail: format!(
                "aces {}-{}",
                stringify(row.pointer("/aces/home")).if_empty("-"),
                stringify(row.pointer("/aces/away")).if_empty("-")
            ),
            metric: format!(
                "points {}-{}",
                stringify(row.pointer("/totalPointsWon/home")).if_empty("-"),
                stringify(row.pointer("/totalPointsWon/away")).if_empty("-")
            ),
        })
        .collect();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = stats.len();
    summary.updated_at = first_pointer_string(value, &["/data/stats/0/recordedAt"]);
    summary.detail = first_pointer_string(value, &["/data/eventId"]).if_empty("tennis");
    summary.preview = preview;
    summary
}

fn parse_cs2_matches_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let matches = extract_array(value, &["/data/matches", "/data"]);
    let preview = matches
        .iter()
        .take(4)
        .map(|row| OwlsPreviewRow {
            label: first_non_empty(row, &["matchName", "name", "slug", "id"]),
            detail: first_non_empty(row, &["event", "tournament", "stars", "startDate"]),
            metric: first_non_empty(row, &["matchId", "id", "bestOf", "result"]),
        })
        .collect();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = matches.len();
    summary.detail = String::from("hltv archive");
    summary.preview = preview;
    summary
}

fn parse_cs2_match_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let data = value.get("data").unwrap_or(value);
    let mut preview = Vec::new();
    let maps = extract_array(data, &["/maps"]);
    if maps.is_empty() {
        preview.push(OwlsPreviewRow {
            label: first_non_empty(data, &["matchName", "name", "slug", "id"]),
            detail: first_non_empty(data, &["event", "tournament", "startDate"]),
            metric: first_non_empty(data, &["result", "bestOf", "status"]),
        });
    } else {
        for map in maps.iter().take(4) {
            preview.push(OwlsPreviewRow {
                label: first_non_empty(map, &["name", "mapName", "map"]),
                detail: first_non_empty(map, &["winner", "pickedBy", "status"]),
                metric: format!(
                    "{}-{}",
                    first_non_empty(map, &["team1Score", "homeScore", "leftScore"]).if_empty("-"),
                    first_non_empty(map, &["team2Score", "awayScore", "rightScore"]).if_empty("-")
                ),
            });
        }
    }

    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = preview.len();
    summary.updated_at = first_pointer_string(data, &["/startDate", "/date"]);
    summary.detail = first_non_empty(data, &["matchId", "id", "slug"]).if_empty("match detail");
    summary.preview = preview;
    summary
}

fn parse_cs2_players_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let rows = extract_array(value, &["/data/players", "/data/stats", "/data"]);
    let preview = rows
        .iter()
        .take(4)
        .map(|row| OwlsPreviewRow {
            label: first_non_empty(row, &["playerName", "name", "player"]),
            detail: first_non_empty(row, &["team", "event", "mapName"]),
            metric: format!(
                "rating {}",
                first_non_empty(row, &["rating", "rating2", "kills"]).if_empty("-")
            ),
        })
        .collect();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = rows.len();
    summary.detail = String::from("player archive");
    summary.preview = preview;
    summary
}

fn parse_realtime_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let events = extract_array(value, &["/data"]);
    let preview = events
        .iter()
        .take(4)
        .map(|event| OwlsPreviewRow {
            label: matchup_label(event),
            detail: String::from("pinnacle"),
            metric: first_market_price(event),
        })
        .collect();
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("ready");
    summary.count = events.len();
    summary.updated_at = first_pointer_string(value, &["/meta/timestamp"]);
    summary.books_returned = vec![String::from("pinnacle")];
    summary.freshness_age_seconds = unsigned_number_at(value, "/meta/freshness/ageSeconds");
    summary.freshness_stale = value
        .pointer("/meta/freshness/stale")
        .and_then(Value::as_bool);
    summary.freshness_threshold_seconds = unsigned_number_at(value, "/meta/freshness/threshold");
    let transport = first_pointer_string(value, &["/meta/transport"]).if_empty("rest");
    summary.detail = format!(
        "pinnacle realtime via {} • age {}s{}",
        transport,
        summary
            .freshness_age_seconds
            .map(|value| value.to_string())
            .unwrap_or_else(|| String::from("-")),
        if summary.freshness_stale.unwrap_or(false) {
            " stale"
        } else {
            ""
        }
    );
    summary.preview = preview;
    summary.quotes = events
        .iter()
        .flat_map(|event| extract_market_quotes(event, Some("pinnacle")))
        .collect();
    summary
}

fn collect_book_event_preview(value: &Value, limit: usize) -> Vec<OwlsPreviewRow> {
    let mut preview = Vec::new();
    if let Some(books) = value.get("data").and_then(Value::as_object) {
        for (book, events) in books {
            let Some(rows) = events.as_array() else {
                continue;
            };
            for row in rows.iter().take(1) {
                preview.push(OwlsPreviewRow {
                    label: matchup_label(row),
                    detail: book.clone(),
                    metric: first_market_price(row),
                });
                if preview.len() >= limit {
                    return preview;
                }
            }
        }
    }
    preview
}

fn first_history_event_id(value: &Value) -> Option<String> {
    value
        .pointer("/data/games/0/eventId")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn first_props_seed(value: &Value) -> (Option<String>, Option<String>, Option<String>) {
    let Some(game) = value.pointer("/data/0") else {
        return (None, None, None);
    };
    let game_id = game
        .get("gameId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let first_prop = game.pointer("/books/0/props/0");
    let player = first_prop
        .and_then(|item| item.get("playerName"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let category = first_prop
        .and_then(|item| item.get("category"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| Some(String::from(DEFAULT_PROP_TYPE)));
    (game_id, player, category)
}

fn books_len(value: &Value) -> usize {
    value
        .get("data")
        .and_then(Value::as_object)
        .map(|item| item.len())
        .unwrap_or(0)
}

fn books_returned_len(value: &Value) -> usize {
    let returned = string_array_at(value, "/meta/booksReturned");
    if returned.is_empty() {
        books_len(value)
    } else {
        returned.len()
    }
}

fn extract_array(value: &Value, paths: &[&str]) -> Vec<Value> {
    for path in paths {
        let current = if path.is_empty() {
            Some(value)
        } else {
            value.pointer(path)
        };
        if let Some(items) = current.and_then(Value::as_array) {
            return items.clone();
        }
    }
    Vec::new()
}

fn matchup_label(event: &Value) -> String {
    let away = first_non_empty(event, &["away_team", "awayTeam", "name"]);
    let home = first_non_empty(event, &["home_team", "homeTeam"]);
    if !away.is_empty() && !home.is_empty() {
        format!("{away} @ {home}")
    } else {
        first_non_empty(event, &["name", "id"])
    }
}

fn first_market_price(event: &Value) -> String {
    let Some(outcome) = event
        .pointer("/bookmakers/0/markets/0/outcomes/0")
        .or_else(|| event.pointer("/markets/0/outcomes/0"))
    else {
        return String::from("-");
    };
    format_outcome_price(outcome)
}

fn extract_market_quotes(event: &Value, book_hint: Option<&str>) -> Vec<OwlsMarketQuote> {
    let event_name = matchup_label(event);
    let league = first_non_empty(event, &["league"]);
    let country_code = first_non_empty(event, &["country_code", "countryCode"]);

    let bookmakers = event
        .get("bookmakers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| {
            book_hint
                .map(|book| vec![serde_json::json!({ "key": book, "markets": event.get("markets").cloned().unwrap_or(Value::Null) })])
                .unwrap_or_default()
        });
    let mut quotes = Vec::new();

    for bookmaker in bookmakers {
        let book =
            first_non_empty(&bookmaker, &["key", "title"]).if_empty(book_hint.unwrap_or("unknown"));
        let event_link = first_non_empty(&bookmaker, &["event_link", "eventLink", "link"]);
        for market in extract_array(&bookmaker, &["/markets"]) {
            let market_key = first_non_empty(&market, &["key", "market"]);
            let suspended = market
                .get("suspended")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let limit_amount = extract_array(&market, &["/limits"])
                .into_iter()
                .filter_map(|limit| numeric_value(limit.get("amount")))
                .max_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
            for outcome in extract_array(&market, &["/outcomes"]) {
                let american_price = numeric_value(outcome.get("price"));
                quotes.push(OwlsMarketQuote {
                    book: book.clone(),
                    event: event_name.clone(),
                    selection: first_non_empty(&outcome, &["name", "label", "selection"]),
                    market_key: market_key.clone(),
                    point: numeric_value(outcome.get("point")),
                    decimal_price: american_price.and_then(normalize_odds_price),
                    american_price,
                    limit_amount,
                    event_link: event_link.clone(),
                    league: league.clone(),
                    country_code: country_code.clone(),
                    suspended,
                });
            }
        }
    }

    quotes
}

pub fn build_market_selections(quotes: &[OwlsMarketQuote]) -> Vec<OwlsMarketSelection> {
    let mut grouped =
        std::collections::BTreeMap::<(String, String, String, String), OwlsMarketSelection>::new();

    for quote in quotes.iter().filter(|quote| quote.decimal_price.is_some()) {
        let key = (
            normalize_key(&quote.event),
            normalize_key(&quote.market_key),
            normalize_key(&quote.selection),
            quote
                .point
                .map(|value| format!("{value:.3}"))
                .unwrap_or_default(),
        );
        let entry = grouped.entry(key).or_insert_with(|| OwlsMarketSelection {
            event: quote.event.clone(),
            market_key: quote.market_key.clone(),
            selection: quote.selection.clone(),
            point: quote.point,
            league: quote.league.clone(),
            country_code: quote.country_code.clone(),
            quotes: Vec::new(),
        });
        entry.quotes.push(quote.clone());
    }

    let mut rows = grouped.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .books()
            .cmp(&left.books())
            .then_with(|| {
                right
                    .best_price()
                    .partial_cmp(&left.best_price())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.event.cmp(&right.event))
            .then_with(|| left.market_key.cmp(&right.market_key))
            .then_with(|| left.selection.cmp(&right.selection))
    });

    for row in &mut rows {
        row.quotes.sort_by(|left, right| {
            right
                .decimal_price
                .unwrap_or_default()
                .partial_cmp(&left.decimal_price.unwrap_or_default())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.book.cmp(&right.book))
        });
    }

    rows
}

fn format_outcome_price(outcome: &Value) -> String {
    let selection = first_non_empty(outcome, &["name", "label", "selection"]).if_empty("-");
    let point = numeric_value(outcome.get("point"));
    let decimal_price = numeric_value(outcome.get("price")).and_then(normalize_odds_price);
    match (point, decimal_price) {
        (Some(point), Some(price)) => format!("{selection} {point:+} @ {price:.2}"),
        (None, Some(price)) => format!("{selection} {price:.2}"),
        (Some(point), None) => format!("{selection} {point:+}"),
        (None, None) => selection,
    }
}

fn normalize_odds_price(price: f64) -> Option<f64> {
    if price.abs() >= 100.0 {
        american_to_decimal(price)
    } else if price > 1.0 {
        Some(price)
    } else {
        None
    }
}

fn american_to_decimal(price: f64) -> Option<f64> {
    if price > 0.0 {
        Some(1.0 + (price / 100.0))
    } else if price < 0.0 {
        Some(1.0 + (100.0 / price.abs()))
    } else {
        None
    }
}

fn numeric_value(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn integer_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number.as_i64(),
        Some(Value::String(text)) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn unsigned_number_at(value: &Value, path: &str) -> Option<u64> {
    value.pointer(path).and_then(|item| match item {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    })
}

fn string_array_at(value: &Value, path: &str) -> Vec<String> {
    value
        .pointer(path)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| stringify(Some(item)))
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn first_non_empty(value: &Value, keys: &[&str]) -> String {
    for key in keys {
        let current = stringify(value.get(*key));
        if !current.is_empty() {
            return current;
        }
    }
    String::new()
}

fn first_non_empty_string(value: Option<&Value>, keys: &[&str]) -> Option<String> {
    let value = value?;
    for key in keys {
        let current = stringify(value.get(*key));
        if !current.is_empty() {
            return Some(current);
        }
    }
    None
}

fn first_pointer_string(value: &Value, paths: &[&str]) -> String {
    for path in paths {
        let current = stringify(value.pointer(path));
        if !current.is_empty() {
            return current;
        }
    }
    String::new()
}

fn stringify(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(item)) => item.trim().to_string(),
        Some(Value::Number(item)) => item.to_string(),
        Some(Value::Bool(item)) => item.to_string(),
        Some(other) if !other.is_null() => other.to_string(),
        _ => String::new(),
    }
}

fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    format!("{}...", &value[..limit.saturating_sub(3)])
}

fn sabisabi_base_url() -> String {
    env::var(SABISABI_BASE_URL_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| String::from(DEFAULT_SABISABI_BASE_URL))
}

fn load_api_key() -> Result<String> {
    load_api_key_with_source().map(|(value, _)| value)
}

fn load_api_key_with_source() -> Result<(String, String)> {
    for name in API_KEY_ENV_NAMES {
        if let Some(value) = env::var_os(name) {
            let trimmed = value.to_string_lossy().trim().to_string();
            if !trimmed.is_empty() {
                return Ok((trimmed, format!("env:{name}")));
            }
        }
    }
    for path in dotenv_candidates() {
        if !path.is_file() {
            continue;
        }
        let content = fs::read_to_string(&path)
            .wrap_err_with(|| format!("failed to read {}", path.display()))?;
        for line in content.lines() {
            if let Some(parsed) = parse_api_key_from_line(line) {
                return Ok((parsed, path.display().to_string()));
            }
        }
    }
    Err(eyre!("missing OWLS_INSIGHT_API_KEY / OWLSINSIGHT_API_KEY"))
}

fn parse_api_key_from_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let candidate = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (key, value) = candidate.split_once('=')?;
    if !API_KEY_ENV_NAMES.contains(&key.trim()) {
        return None;
    }

    let parsed = value
        .trim()
        .trim_end_matches(';')
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    (!parsed.is_empty()).then_some(parsed)
}

fn startup_status_line() -> String {
    match load_api_key_with_source() {
        Ok((key, source)) => format!(
            "Owls catalog loaded. Key detected from {} ({}). Press r to hydrate the API surface.",
            source,
            mask_key_hint(&key)
        ),
        Err(_) => missing_owls_api_key_status_line(),
    }
}

fn missing_owls_api_key_status_line() -> String {
    String::from(
        "Owls catalog ready in offline mode. Add OWLS_INSIGHT_API_KEY to hydrate live endpoints.",
    )
}

fn mask_key_hint(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.len() <= 8 {
        return String::from("********");
    }
    format!("{}...{}", &trimmed[..4], &trimmed[trimmed.len() - 4..])
}

fn normalize_owls_error(detail: &str) -> String {
    let lowered = detail.to_ascii_lowercase();
    if lowered.contains("error code: 1010")
        || (lowered.contains("http 403") && lowered.contains("1010"))
    {
        return String::from(
            "Cloudflare blocked this client (HTTP 403 / 1010). Verify Owls allowlisting or zone access for this machine.",
        );
    }
    detail.to_string()
}

fn is_missing_owls_api_key_detail(detail: &str) -> bool {
    detail.contains("OWLS_INSIGHT_API_KEY") || detail.contains("OWLSINSIGHT_API_KEY")
}

fn dotenv_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        paths.push(home_path.join(".env"));
        paths.push(home_path.join(".env.local"));
        paths.push(home_path.join(".zshenv"));
        paths.push(home_path.join(".zshrc"));
        paths.push(home_path.join(".bashrc"));
        paths.push(home_path.join(".profile"));
    }
    if let Ok(current_dir) = env::current_dir() {
        for ancestor in current_dir.ancestors() {
            paths.push(ancestor.join(".env"));
            paths.push(ancestor.join(".env.local"));
        }
    }
    paths
}

fn build_group_summaries(endpoints: &[OwlsEndpointSummary]) -> Vec<OwlsGroupSummary> {
    OwlsEndpointGroup::ALL
        .iter()
        .map(|group| {
            let total = endpoints.iter().filter(|item| item.group == *group).count();
            let ready = endpoints
                .iter()
                .filter(|item| item.group == *group && item.status == "ready")
                .count();
            let error = endpoints
                .iter()
                .filter(|item| item.group == *group && item.status == "error")
                .count();
            let waiting = endpoints
                .iter()
                .filter(|item| item.group == *group && item.status == "waiting")
                .count();
            OwlsGroupSummary {
                group: *group,
                label: String::from(group.label()),
                ready,
                total,
                error,
                waiting,
            }
        })
        .collect()
}

fn spec_for(id: OwlsEndpointId) -> &'static OwlsEndpointSpec {
    catalog_specs()
        .iter()
        .find(|spec| spec.id == id)
        .expect("every Owls endpoint id must exist in the catalog")
}

fn catalog_specs() -> &'static [OwlsEndpointSpec] {
    &[
        OwlsEndpointSpec {
            id: OwlsEndpointId::Odds,
            group: OwlsEndpointGroup::Odds,
            label: "Odds",
            path: "/api/v1/{sport}/odds",
            description: "All pregame odds grouped by book.",
            query_hint: "books, alternates, league",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Moneyline,
            group: OwlsEndpointGroup::Odds,
            label: "Moneyline",
            path: "/api/v1/{sport}/moneyline",
            description: "Moneyline-only price board.",
            query_hint: "books, league",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Spreads,
            group: OwlsEndpointGroup::Odds,
            label: "Spreads",
            path: "/api/v1/{sport}/spreads",
            description: "Spread-only market feed.",
            query_hint: "books, alternates, league",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Totals,
            group: OwlsEndpointGroup::Odds,
            label: "Totals",
            path: "/api/v1/{sport}/totals",
            description: "Totals and alternate totals feed.",
            query_hint: "books, alternates, league",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Props,
            group: OwlsEndpointGroup::Props,
            label: "Props",
            path: "/api/v1/{sport}/props",
            description: "Aggregated player props across books.",
            query_hint: "game_id, player, category, books",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::FanDuelProps,
            group: OwlsEndpointGroup::Props,
            label: "FanDuel Props",
            path: "/api/v1/{sport}/props/fanduel",
            description: "FanDuel player props feed.",
            query_hint: "game_id, player, category",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::BetMgmProps,
            group: OwlsEndpointGroup::Props,
            label: "BetMGM Props",
            path: "/api/v1/{sport}/props/betmgm",
            description: "BetMGM player props feed.",
            query_hint: "game_id, player, category",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Bet365Props,
            group: OwlsEndpointGroup::Props,
            label: "Bet365 Props",
            path: "/api/v1/{sport}/props/bet365",
            description: "Bet365 player props feed.",
            query_hint: "game_id, player, category",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::PropsHistory,
            group: OwlsEndpointGroup::Props,
            label: "Props History",
            path: "/api/v1/{sport}/props/history",
            description: "Line movement for a single player prop.",
            query_hint: "game_id, player, category, hours",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::ScoresAll,
            group: OwlsEndpointGroup::Scores,
            label: "Scores All",
            path: "/api/v1/scores/live",
            description: "Live scores across supported sports.",
            query_hint: "none",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::ScoresSport,
            group: OwlsEndpointGroup::Scores,
            label: "Scores NBA",
            path: "/api/v1/{sport}/scores/live",
            description: "Live scorecard for the active sport.",
            query_hint: "sport path only",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Stats,
            group: OwlsEndpointGroup::Stats,
            label: "Stats",
            path: "/api/v1/{sport}/stats",
            description: "Game-level player stats feed.",
            query_hint: "date, player",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Averages,
            group: OwlsEndpointGroup::Stats,
            label: "Averages",
            path: "/api/v1/{sport}/stats/averages",
            description: "Rolling player averages.",
            query_hint: "playerName, opponent",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::KalshiMarkets,
            group: OwlsEndpointGroup::Prediction,
            label: "Kalshi Markets",
            path: "/api/v1/kalshi/{sport}/markets",
            description: "Kalshi markets for a sport.",
            query_hint: "status, limit, cursor, eventTicker",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::KalshiSeries,
            group: OwlsEndpointGroup::Prediction,
            label: "Kalshi Series",
            path: "/api/v1/kalshi/series",
            description: "Kalshi series registry.",
            query_hint: "none",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::KalshiSeriesMarkets,
            group: OwlsEndpointGroup::Prediction,
            label: "Kalshi Series Mkts",
            path: "/api/v1/kalshi/series/{ticker}/markets",
            description: "Markets for a single Kalshi series.",
            query_hint: "status, limit, cursor, eventTicker",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::PolymarketMarkets,
            group: OwlsEndpointGroup::Prediction,
            label: "Polymarket",
            path: "/api/v1/polymarket/{sport}/markets",
            description: "Polymarket markets for a sport.",
            query_hint: "sport path only",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::HistoryGames,
            group: OwlsEndpointGroup::History,
            label: "History Games",
            path: "/api/v1/history/games",
            description: "Historical event catalog.",
            query_hint: "sport, startDate, endDate, limit, offset",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::HistoryOdds,
            group: OwlsEndpointGroup::History,
            label: "History Odds",
            path: "/api/v1/history/odds",
            description: "Historical odds snapshots.",
            query_hint: "eventId, book, market, side, time range",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::HistoryProps,
            group: OwlsEndpointGroup::History,
            label: "History Props",
            path: "/api/v1/history/props",
            description: "Historical props snapshots.",
            query_hint: "eventId, playerName, propType, book, time range",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::HistoryStats,
            group: OwlsEndpointGroup::History,
            label: "History Stats",
            path: "/api/v1/history/stats",
            description: "Historical player stat rows.",
            query_hint: "eventId or playerName, sport, position, dates",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::HistoryAverages,
            group: OwlsEndpointGroup::History,
            label: "History Avg",
            path: "/api/v1/history/stats/averages",
            description: "Historical rolling averages.",
            query_hint: "playerName, sport, opponent",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::TennisStats,
            group: OwlsEndpointGroup::History,
            label: "Tennis Stats",
            path: "/api/v1/history/tennis-stats",
            description: "Historical tennis match and set stats.",
            query_hint: "eventId",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Cs2Matches,
            group: OwlsEndpointGroup::History,
            label: "CS2 Matches",
            path: "/api/v1/history/cs2/matches",
            description: "CS2 match archive.",
            query_hint: "team, event, stars, dates, limit, offset",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Cs2Match,
            group: OwlsEndpointGroup::History,
            label: "CS2 Match",
            path: "/api/v1/history/cs2/matches/{id}",
            description: "Single CS2 match detail.",
            query_hint: "match id path only",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Cs2Players,
            group: OwlsEndpointGroup::History,
            label: "CS2 Players",
            path: "/api/v1/history/cs2/players",
            description: "CS2 player stat archive.",
            query_hint: "playerName, team, event, mapName, dates",
        },
        OwlsEndpointSpec {
            id: OwlsEndpointId::Realtime,
            group: OwlsEndpointGroup::Realtime,
            label: "Realtime",
            path: "/api/v1/{sport}/realtime",
            description: "Low-latency Pinnacle realtime feed.",
            query_hint: "league",
        },
    ]
}

trait EmptyFallback {
    fn if_empty(self, fallback: &str) -> String;
}

impl EmptyFallback for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.trim().is_empty() {
            return String::from(fallback);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::app_state::TradingSection;

    use super::*;

    #[test]
    fn missing_api_key_offline_state_is_waiting_not_error() {
        let mut dashboard = dashboard_for_sport("nba");

        mark_all_endpoints_waiting(
            &mut dashboard,
            "missing OWLS_INSIGHT_API_KEY / OWLSINSIGHT_API_KEY",
        );

        assert!(missing_owls_api_key_status_line().contains("offline mode"));
        assert!(dashboard
            .endpoints
            .iter()
            .all(|endpoint| endpoint.status == "waiting"));
        assert!(is_missing_owls_api_key_detail(
            "missing OWLS_INSIGHT_API_KEY / OWLSINSIGHT_API_KEY"
        ));
    }

    #[test]
    fn parse_api_key_from_plain_env_line() {
        assert_eq!(
            parse_api_key_from_line("OWLSINSIGHT_API_KEY=secret-value"),
            Some(String::from("secret-value"))
        );
    }

    #[test]
    fn parse_api_key_from_exported_shell_line() {
        assert_eq!(
            parse_api_key_from_line("export OWLSINSIGHT_API_KEY=\"secret-value\""),
            Some(String::from("secret-value"))
        );
    }
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn fetch_json_async_reads_success_payload() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let base_url = spawn_http_server(vec![http_ok(r#"{"ok":true}"#)]);
        let client = build_async_client().expect("async client");

        let value = runtime
            .block_on(fetch_json_async(
                &client,
                &base_url,
                "token",
                "/status",
                &[],
            ))
            .expect("async json payload");

        assert_eq!(value["ok"], serde_json::json!(true));
    }

    #[test]
    fn owls_backend_path_rewrites_upstream_api_prefix() {
        assert_eq!(
            owls_backend_path("/api/v1/nba/props/history"),
            "/api/v1/owls/nba/props/history"
        );
        assert_eq!(
            owls_backend_path("/api/v1/normalize/batch"),
            "/api/v1/owls/normalize/batch"
        );
    }

    #[test]
    fn background_due_ids_prioritize_hot_feeds() {
        let mut dashboard = OwlsDashboard::default();
        let now = Instant::now();
        for endpoint in &mut dashboard.endpoints {
            endpoint.last_checked_at = Some(now);
        }
        dashboard
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == OwlsEndpointId::Realtime)
            .expect("realtime endpoint")
            .last_checked_at = Some(now - HOT_SYNC_INTERVAL - Duration::from_millis(1));
        dashboard
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == OwlsEndpointId::HistoryGames)
            .expect("history endpoint")
            .last_checked_at = Some(now - HOT_SYNC_INTERVAL - Duration::from_millis(1));

        let due_ids = due_endpoint_ids(&dashboard, OwlsSyncReason::Background, None);

        assert!(due_ids.contains(&OwlsEndpointId::Realtime));
        assert!(!due_ids.contains(&OwlsEndpointId::HistoryGames));
        assert!(due_ids.len() <= BACKGROUND_SYNC_BATCH);
    }

    #[test]
    fn focused_endpoint_uses_hot_interval_even_for_cold_groups() {
        let mut dashboard = OwlsDashboard::default();
        let history = dashboard
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == OwlsEndpointId::HistoryGames)
            .expect("history endpoint");
        history.last_checked_at =
            Some(Instant::now() - HOT_SYNC_INTERVAL - Duration::from_millis(1));

        let due_ids = due_endpoint_ids(
            &dashboard,
            OwlsSyncReason::Background,
            Some(OwlsEndpointId::HistoryGames),
        );

        assert!(due_ids.contains(&OwlsEndpointId::HistoryGames));
    }

    #[test]
    fn first_background_sync_is_batched_instead_of_loading_everything() {
        let dashboard = OwlsDashboard::default();

        let due_ids = due_endpoint_ids(&dashboard, OwlsSyncReason::Background, None);

        assert_eq!(due_ids.len(), BACKGROUND_SYNC_BATCH);
        assert!(due_ids.contains(&OwlsEndpointId::Realtime));
    }

    #[test]
    fn manual_sync_is_batched_and_prioritizes_the_focused_endpoint() {
        let dashboard = OwlsDashboard::default();

        let due_ids = due_endpoint_ids(
            &dashboard,
            OwlsSyncReason::Manual,
            Some(OwlsEndpointId::HistoryGames),
        );

        assert_eq!(due_ids.len(), 6);
        assert_eq!(due_ids.first(), Some(&OwlsEndpointId::HistoryGames));
    }

    #[test]
    fn socketio_connect_url_preserves_existing_query_and_adds_api_key() {
        let url = socketio_connect_url(
            "https://api.owlsinsight.com/socket.io?transport=websocket",
            "secret:value",
        )
        .expect("socket url");

        let parsed = Url::parse(&url).expect("parsed url");
        let query_pairs = parsed.query_pairs().collect::<Vec<_>>();

        assert!(query_pairs
            .iter()
            .any(|(key, value)| { key == "transport" && value == "websocket" }));
        assert!(query_pairs
            .iter()
            .any(|(key, value)| { key == "apiKey" && value == "secret:value" }));
    }

    #[test]
    fn merge_endpoint_skips_semantic_noops() {
        let mut dashboard = OwlsDashboard::default();
        let first = summary_fixture(OwlsEndpointId::Realtime, "ready", 12, "live");
        assert!(merge_endpoint(&mut dashboard, first));
        let slot = dashboard
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == OwlsEndpointId::Realtime)
            .expect("realtime endpoint");
        assert_eq!(slot.poll_count, 1);
        assert_eq!(slot.change_count, 1);

        let second = summary_fixture(OwlsEndpointId::Realtime, "ready", 12, "live");
        assert!(!merge_endpoint(&mut dashboard, second));
        let slot = dashboard
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == OwlsEndpointId::Realtime)
            .expect("realtime endpoint");
        assert_eq!(slot.poll_count, 2);
        assert_eq!(slot.change_count, 1);
    }

    #[test]
    fn trading_section_dashboard_limits_markets_to_odds_endpoints() {
        let dashboard = dashboard_for_trading_section("soccer", TradingSection::Markets);
        let ids = dashboard
            .endpoints
            .iter()
            .map(|endpoint| endpoint.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![OwlsEndpointId::Odds]);
    }

    #[test]
    fn parse_book_market_summary_extracts_quote_rows_and_metadata() {
        let value = serde_json::json!({
            "data": {
                "pinnacle": [{
                    "away_team": "Arsenal",
                    "home_team": "Everton",
                    "league": "England - Premier League",
                    "country_code": "GB",
                    "bookmakers": [{
                        "key": "pinnacle",
                        "event_link": "https://www.pinnacle.com/event/1",
                        "markets": [{
                            "key": "h2h",
                            "limits": [{"type": "maxRiskStake", "amount": 1500}],
                            "outcomes": [
                                {"name": "Arsenal", "price": -150},
                                {"name": "Draw", "price": 220},
                                {"name": "Everton", "price": 430}
                            ]
                        }]
                    }]
                }],
                "bet365": [{
                    "away_team": "Arsenal",
                    "home_team": "Everton",
                    "bookmakers": [{
                        "key": "bet365",
                        "markets": [{
                            "key": "h2h",
                            "outcomes": [{"name": "Draw", "price": 3.30}]
                        }]
                    }]
                }]
            },
            "meta": {
                "market": "moneyline",
                "requestedBooks": ["pinnacle", "bet365"],
                "availableBooks": ["pinnacle", "bet365", "betmgm"],
                "booksReturned": ["pinnacle", "bet365"],
                "timestamp": "2026-03-25T01:02:03Z",
                "freshness": {"ageSeconds": 2, "stale": false, "threshold": 90}
            }
        });

        let summary = parse_book_market_summary(OwlsEndpointId::Moneyline, &value);
        assert_eq!(summary.books_returned, vec!["pinnacle", "bet365"]);
        assert_eq!(
            summary.available_books,
            vec!["pinnacle", "bet365", "betmgm"]
        );
        assert_eq!(summary.requested_books, vec!["pinnacle", "bet365"]);
        assert_eq!(summary.freshness_age_seconds, Some(2));
        assert!(!summary.freshness_stale.unwrap_or(true));
        assert_eq!(summary.quote_count, 4);
        assert!(summary.quotes.is_empty());
        assert_eq!(summary.market_selections.len(), 3);
        let draw = summary
            .market_selections
            .iter()
            .find(|selection| selection.selection == "Draw")
            .expect("draw market selection");
        assert_eq!(draw.event, "Arsenal @ Everton");
        assert_eq!(draw.market_key, "h2h");
        assert_eq!(draw.books(), 2);
        assert_eq!(draw.best_price().map(|value| value.round() as i64), Some(3));
        assert!(summary.detail.contains("age 2s"));
    }

    #[test]
    fn parse_scores_summary_extracts_soccer_live_context() {
        let value = serde_json::json!({
            "success": true,
            "data": {
                "sports": {
                    "soccer": [{
                        "id": "soccer:Malta@Luxembourg-20260325",
                        "sport": "soccer",
                        "name": "Malta at Luxembourg",
                        "status": {
                            "state": "in",
                            "detail": "72'",
                            "displayClock": "72"
                        },
                        "home": {
                            "team": {"displayName": "Luxembourg"},
                            "score": 1
                        },
                        "away": {
                            "team": {"displayName": "Malta"},
                            "score": 2
                        },
                        "sourceMatchId": "mb-1",
                        "matchStats": {
                            "possession": {"home": 40, "away": 60},
                            "shotsOnTarget": {"home": 3, "away": 7},
                            "expectedGoals": {"home": 0.8, "away": 1.7}
                        },
                        "incidents": [{
                            "minute": 58,
                            "type": "goal",
                            "playerName": "Attard",
                            "teamSide": "away"
                        }],
                        "playerStats": [{
                            "playerName": "Teuma",
                            "teamSide": "away",
                            "rating": 8.4
                        }],
                        "lastUpdated": "2026-03-25T11:12:13Z"
                    }]
                },
                "timestamp": "2026-03-25T11:12:13Z"
            }
        });

        let summary = parse_scores_summary(OwlsEndpointId::ScoresSport, &value);

        assert_eq!(summary.count, 1);
        assert!(summary.detail.contains("soccer"));
        assert_eq!(summary.live_scores.len(), 1);
        assert_eq!(summary.live_scores[0].sport, "soccer");
        assert_eq!(summary.live_scores[0].home_team, "Luxembourg");
        assert_eq!(summary.live_scores[0].away_team, "Malta");
        assert_eq!(summary.live_scores[0].display_clock, "72");
        assert_eq!(summary.live_scores[0].incidents.len(), 1);
        assert_eq!(summary.live_scores[0].player_ratings.len(), 1);
        assert!(summary.live_scores[0]
            .stats
            .iter()
            .any(|stat| stat.label == "xG" && stat.away_value == "1.7"));
    }

    fn summary_fixture(
        id: OwlsEndpointId,
        status: &str,
        count: usize,
        detail: &str,
    ) -> OwlsEndpointSummary {
        let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
        summary.status = String::from(status);
        summary.count = count;
        summary.updated_at = String::from("2026-03-24T11:00:00Z");
        summary.detail = String::from(detail);
        summary.preview = vec![OwlsPreviewRow {
            label: String::from("fixture"),
            detail: String::from("detail"),
            metric: String::from("metric"),
        }];
        summary
    }

    fn spawn_http_server(responses: Vec<String>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let address = listener.local_addr().expect("local addr");
        thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept connection");
                let mut buffer = [0_u8; 4096];
                let _ = stream.read(&mut buffer);
                stream
                    .write_all(response.as_bytes())
                    .expect("write response");
                stream.flush().expect("flush response");
            }
        });
        format!("http://{}", address)
    }

    fn http_ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }
}