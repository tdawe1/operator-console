use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use color_eyre::eyre::{eyre, Result, WrapErr};
use reqwest::blocking::Client;
use serde_json::Value;

const DEFAULT_BASE_URL: &str = "https://api.owlsinsight.com";
const API_KEY_ENV_NAMES: [&str; 2] = ["OWLS_INSIGHT_API_KEY", "OWLSINSIGHT_API_KEY"];
const DEFAULT_SPORT: &str = "nba";
const DEFAULT_PLAYER: &str = "LeBron James";
const DEFAULT_PROP_TYPE: &str = "points";
const DEFAULT_BOOKS: &str = "pinnacle,bet365,betmgm";
const DEFAULT_BOOK_PROPS_PLAYER: &str = "LeBron";
const HOT_SYNC_INTERVAL: Duration = Duration::from_secs(3);
const WARM_SYNC_INTERVAL: Duration = Duration::from_secs(10);
const COOL_SYNC_INTERVAL: Duration = Duration::from_secs(30);
const COLD_SYNC_INTERVAL: Duration = Duration::from_secs(120);
const BACKGROUND_SYNC_BATCH: usize = 3;
pub const SUPPORTED_SPORTS: &[&str] = &[
    "nba", "nfl", "mlb", "nhl", "wnba", "ncaab", "ncaaf", "epl", "mma", "tennis", "cs2",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
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
    seeds: OwlsSeeds,
}

impl Default for OwlsDashboard {
    fn default() -> Self {
        dashboard_for_sport(DEFAULT_SPORT)
    }
}

#[derive(Debug, Clone)]
pub struct OwlsGroupSummary {
    pub group: OwlsEndpointGroup,
    pub label: String,
    pub ready: usize,
    pub total: usize,
    pub error: usize,
    pub waiting: usize,
}

#[derive(Debug, Clone)]
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
            last_checked_at: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OwlsPreviewRow {
    pub label: String,
    pub detail: String,
    pub metric: String,
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
        status_line: String::from("Owls catalog loaded. Press r to hydrate the API surface."),
        refreshed_at: String::new(),
        last_sync_mode: String::from("idle"),
        sync_checks: 0,
        sync_changes: 0,
        total_polls: 0,
        groups,
        endpoints,
        seeds: OwlsSeeds::default(),
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
            mark_all_endpoints_error(&mut dashboard, &error.to_string());
            dashboard.status_line = format!("Owls unavailable: {error}");
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

fn due_endpoint_ids(
    dashboard: &OwlsDashboard,
    reason: OwlsSyncReason,
    focused: Option<OwlsEndpointId>,
) -> Vec<OwlsEndpointId> {
    if matches!(reason, OwlsSyncReason::Manual) {
        return dashboard
            .endpoints
            .iter()
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
                &[("books", DEFAULT_BOOKS), ("alternates", "true")],
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
                &[("books", DEFAULT_BOOKS)],
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
                &[("books", DEFAULT_BOOKS), ("alternates", "true")],
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
                &[("books", DEFAULT_BOOKS), ("alternates", "true")],
            ),
            parse_book_market_summary,
        ),
        OwlsEndpointId::Props => {
            let payload = fetch_json(
                client,
                base_url,
                api_key,
                &format!("/api/v1/{sport}/props"),
                &[
                    ("player", DEFAULT_BOOK_PROPS_PLAYER),
                    ("books", DEFAULT_BOOKS),
                ],
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
        &[
            ("player", DEFAULT_BOOK_PROPS_PLAYER),
            ("books", DEFAULT_BOOKS),
        ],
    );
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
    current.endpoints.len() != next.endpoints.len()
        || current
            .endpoints
            .iter()
            .zip(next.endpoints.iter())
            .any(|(left, right)| endpoint_semantically_changed(left, right))
}

fn hydrate_result(
    id: OwlsEndpointId,
    payload: Result<Value>,
    parser: fn(OwlsEndpointId, &Value) -> OwlsEndpointSummary,
) -> OwlsEndpointSummary {
    match payload {
        Ok(value) => parser(id, &value),
        Err(error) => error_summary(id, &error.to_string()),
    }
}

fn error_summary(id: OwlsEndpointId, detail: &str) -> OwlsEndpointSummary {
    let mut summary = OwlsEndpointSummary::from_spec(spec_for(id));
    summary.status = String::from("error");
    summary.detail = truncate(detail, 88);
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

fn parse_book_market_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let books = value
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut preview = Vec::new();
    let mut event_count = 0usize;
    for (book, events) in books {
        let Some(rows) = events.as_array() else {
            continue;
        };
        event_count += rows.len();
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
    summary.detail = format!(
        "books {} • market {}",
        books_len(value),
        first_pointer_string(value, &["/meta/market"]).if_empty("filtered")
    );
    summary.preview = preview;
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
    let rows = extract_array(value, &["/data/history", "/data/snapshots", "/data"]);
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
    summary.count = rows.len();
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

    for (sport, rows) in sports {
        let Some(events) = rows.as_array() else {
            continue;
        };
        total += events.len();
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
    summary
}

fn parse_scores_summary(id: OwlsEndpointId, value: &Value) -> OwlsEndpointSummary {
    let games = value
        .pointer(&format!("/data/sports/{DEFAULT_SPORT}"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
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
    summary.detail = String::from("live feed");
    summary.preview = preview;
    summary
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
    summary.detail = format!(
        "age {}s",
        first_pointer_string(value, &["/meta/freshness/ageSeconds"]).if_empty("-")
    );
    summary.preview = preview;
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
    let Some(market) = event
        .pointer("/bookmakers/0/markets/0/outcomes/0")
        .or_else(|| event.pointer("/markets/0/outcomes/0"))
    else {
        return String::from("-");
    };
    format!(
        "{} {}",
        stringify(market.get("name")).if_empty("-"),
        stringify(market.get("price")).if_empty("-")
    )
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

fn load_api_key() -> Result<String> {
    for name in API_KEY_ENV_NAMES {
        if let Some(value) = env::var_os(name) {
            let trimmed = value.to_string_lossy().trim().to_string();
            if !trimmed.is_empty() {
                return Ok(trimmed);
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
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if API_KEY_ENV_NAMES.contains(&key.trim()) {
                let parsed = value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if !parsed.is_empty() {
                    return Ok(parsed);
                }
            }
        }
    }
    Err(eyre!("missing OWLS_INSIGHT_API_KEY / OWLSINSIGHT_API_KEY"))
}

fn dotenv_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        paths.push(home_path.join(".env"));
        paths.push(home_path.join(".env.local"));
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
    use super::*;

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
}
