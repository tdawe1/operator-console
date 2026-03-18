use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Context, Result};
use reqwest::blocking::Client;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize};

pub const ODDSMATCHER_GRAPHQL_URL: &str =
    "https://api.oddsplatform.profitaccumulator.com/graphql";

pub const GET_BEST_MATCHES_QUERY: &str = r#"
query GetBestMatches(
  $bookmaker: [String!]
  $exchange: [String!]
  $ratingType: String!
  $minRating: String
  $maxRating: String
  $minOdds: String
  $maxOdds: String
  $minLiquidity: String
  $limit: Int
  $skip: Int
  $updatedWithinSeconds: Int
  $excludeDraw: Boolean
  $permittedSports: [String!]
  $permittedMarketGroups: [String!]
  $permittedEventGroups: [String!]
  $permittedCountries: [String!]
  $permittedEventIds: [String!]
) {
  getBestMatches(
    bookmaker: $bookmaker
    exchange: $exchange
    ratingType: $ratingType
    minRating: $minRating
    maxRating: $maxRating
    minOdds: $minOdds
    maxOdds: $maxOdds
    minLiquidity: $minLiquidity
    limit: $limit
    skip: $skip
    updatedWithinSeconds: $updatedWithinSeconds
    excludeDraw: $excludeDraw
    permittedSports: $permittedSports
    permittedMarketGroups: $permittedMarketGroups
    permittedEventGroups: $permittedEventGroups
    permittedCountries: $permittedCountries
    permittedEventIds: $permittedEventIds
  ) {
    eventName
    id
    startAt
    selectionId
    marketId
    eventId
    back {
      updatedAt
      odds
      fetchedAt
      deepLink
      bookmaker {
        active
        code
        displayName
        id
        logo
      }
    }
    lay {
      bookmaker {
        active
        code
        displayName
        id
        logo
      }
      deepLink
      fetchedAt
      updatedAt
      odds
      liquidity
      betSlip {
        marketId
        selectionId
      }
    }
    eventGroup {
      displayName
      id
      sourceName
      sport
    }
    marketGroup {
      displayName
      id
      sport
    }
    marketName
    rating
    selectionName
    snr
    sport {
      displayName
      id
    }
    betRequestId
  }
}
"#;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GetBestMatchesVariables {
    pub bookmaker: Vec<String>,
    pub exchange: Vec<String>,
    pub rating_type: String,
    pub min_rating: Option<String>,
    pub max_rating: Option<String>,
    pub min_odds: Option<String>,
    pub max_odds: Option<String>,
    pub min_liquidity: Option<String>,
    pub limit: usize,
    pub skip: usize,
    pub updated_within_seconds: u64,
    pub exclude_draw: bool,
    pub permitted_sports: Vec<String>,
    pub permitted_market_groups: Vec<String>,
    pub permitted_event_groups: Vec<String>,
    pub permitted_countries: Vec<String>,
    pub permitted_event_ids: Vec<String>,
}

impl Default for GetBestMatchesVariables {
    fn default() -> Self {
        Self {
            bookmaker: vec![String::from("betvictor")],
            exchange: vec![String::from("smarketsexchange")],
            rating_type: String::from("rating"),
            min_rating: None,
            max_rating: Some(String::from("99")),
            min_odds: Some(String::from("2.1")),
            max_odds: Some(String::from("5")),
            min_liquidity: Some(String::from("30")),
            limit: 10,
            skip: 0,
            updated_within_seconds: 21_600,
            exclude_draw: false,
            permitted_sports: vec![String::from("soccer")],
            permitted_market_groups: vec![String::from("match-odds")],
            permitted_event_groups: Vec::new(),
            permitted_countries: Vec::new(),
            permitted_event_ids: Vec::new(),
        }
    }
}

pub fn default_query_path() -> PathBuf {
    if let Some(path) = env::var_os("SABI_ODDSMATCHER_CONFIG_PATH") {
        return PathBuf::from(path);
    }

    let config_root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));

    config_root.join("sabi").join("oddsmatcher.json")
}

pub fn load_query_or_default(path: &Path) -> Result<(GetBestMatchesVariables, String)> {
    if !path.exists() {
        return Ok((
            GetBestMatchesVariables::default(),
            String::from("Using default OddsMatcher config."),
        ));
    }

    let content = fs::read_to_string(path)?;
    let query = serde_json::from_str::<GetBestMatchesVariables>(&content)?;
    Ok((
        query,
        format!("Loaded OddsMatcher config from {}.", path.display()),
    ))
}

pub fn save_query(path: &Path, query: &GetBestMatchesVariables) -> Result<String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(query)? + "\n")?;
    Ok(format!("Saved OddsMatcher config to {}.", path.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OddsMatcherField {
    Bookmaker,
    Exchange,
    Sport,
    MarketGroup,
    EventGroup,
    Country,
    EventId,
    RatingType,
    MinRating,
    MaxRating,
    MinOdds,
    MaxOdds,
    MinLiquidity,
    Limit,
    Skip,
    UpdatedWithinSeconds,
    ExcludeDraw,
}

impl OddsMatcherField {
    pub const ALL: [Self; 17] = [
        Self::Bookmaker,
        Self::Exchange,
        Self::Sport,
        Self::MarketGroup,
        Self::EventGroup,
        Self::Country,
        Self::EventId,
        Self::RatingType,
        Self::MinRating,
        Self::MaxRating,
        Self::MinOdds,
        Self::MaxOdds,
        Self::MinLiquidity,
        Self::Limit,
        Self::Skip,
        Self::UpdatedWithinSeconds,
        Self::ExcludeDraw,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Bookmaker => "Bookmaker",
            Self::Exchange => "Exchange",
            Self::Sport => "Sport",
            Self::MarketGroup => "Market Group",
            Self::EventGroup => "Event Group",
            Self::Country => "Country",
            Self::EventId => "Event Id",
            Self::RatingType => "Rating Type",
            Self::MinRating => "Min Rating",
            Self::MaxRating => "Max Rating",
            Self::MinOdds => "Min Odds",
            Self::MaxOdds => "Max Odds",
            Self::MinLiquidity => "Min Liquidity",
            Self::Limit => "Limit",
            Self::Skip => "Skip",
            Self::UpdatedWithinSeconds => "Updated Within",
            Self::ExcludeDraw => "Exclude Draw",
        }
    }

    pub fn display_value(self, query: &GetBestMatchesVariables) -> String {
        match self {
            Self::Bookmaker => query.bookmaker.join(","),
            Self::Exchange => query.exchange.join(","),
            Self::Sport => query.permitted_sports.join(","),
            Self::MarketGroup => query.permitted_market_groups.join(","),
            Self::EventGroup => query.permitted_event_groups.join(","),
            Self::Country => query.permitted_countries.join(","),
            Self::EventId => query.permitted_event_ids.join(","),
            Self::RatingType => query.rating_type.clone(),
            Self::MinRating => query.min_rating.clone().unwrap_or_default(),
            Self::MaxRating => query.max_rating.clone().unwrap_or_default(),
            Self::MinOdds => query.min_odds.clone().unwrap_or_default(),
            Self::MaxOdds => query.max_odds.clone().unwrap_or_default(),
            Self::MinLiquidity => query.min_liquidity.clone().unwrap_or_default(),
            Self::Limit => query.limit.to_string(),
            Self::Skip => query.skip.to_string(),
            Self::UpdatedWithinSeconds => query.updated_within_seconds.to_string(),
            Self::ExcludeDraw => query.exclude_draw.to_string(),
        }
    }

    pub fn apply_value(self, query: &mut GetBestMatchesVariables, value: &str) -> Result<()> {
        match self {
            Self::Bookmaker => query.bookmaker = parse_csv_list(value),
            Self::Exchange => query.exchange = parse_csv_list(value),
            Self::Sport => query.permitted_sports = parse_csv_list(value),
            Self::MarketGroup => query.permitted_market_groups = parse_csv_list(value),
            Self::EventGroup => query.permitted_event_groups = parse_csv_list(value),
            Self::Country => query.permitted_countries = parse_csv_list(value),
            Self::EventId => query.permitted_event_ids = parse_csv_list(value),
            Self::RatingType => query.rating_type = value.trim().to_string(),
            Self::MinRating => query.min_rating = parse_optional_string(value),
            Self::MaxRating => query.max_rating = parse_optional_string(value),
            Self::MinOdds => query.min_odds = parse_optional_string(value),
            Self::MaxOdds => query.max_odds = parse_optional_string(value),
            Self::MinLiquidity => query.min_liquidity = parse_optional_string(value),
            Self::Limit => {
                query.limit = value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| eyre!("limit must be a positive integer"))?;
            }
            Self::Skip => {
                query.skip = value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| eyre!("skip must be a non-negative integer"))?;
            }
            Self::UpdatedWithinSeconds => {
                query.updated_within_seconds = value
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| eyre!("updated within must be a non-negative integer"))?;
            }
            Self::ExcludeDraw => {
                query.exclude_draw = value
                    .trim()
                    .parse::<bool>()
                    .map_err(|_| eyre!("exclude draw must be true or false"))?;
            }
        }
        Ok(())
    }

    pub fn suggestions(self) -> Vec<String> {
        match self {
            Self::Bookmaker => vec![String::from("betvictor")],
            Self::Exchange => vec![String::from("smarketsexchange")],
            Self::Sport => vec![String::from("soccer")],
            Self::MarketGroup => vec![String::from("match-odds")],
            Self::EventGroup | Self::Country | Self::EventId | Self::MinRating => Vec::new(),
            Self::RatingType => vec![String::from("rating")],
            Self::MaxRating => vec![String::from("99"), String::from("100")],
            Self::MinOdds => vec![String::from("2.1"), String::from("1.5")],
            Self::MaxOdds => vec![String::from("5"), String::from("10")],
            Self::MinLiquidity => vec![String::from("30"), String::from("100")],
            Self::Limit => vec![String::from("10"), String::from("25"), String::from("50")],
            Self::Skip => vec![String::from("0")],
            Self::UpdatedWithinSeconds => vec![
                String::from("3600"),
                String::from("21600"),
                String::from("86400"),
            ],
            Self::ExcludeDraw => vec![String::from("false"), String::from("true")],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OddsMatcherEditorState {
    selected_field_index: usize,
    pub editing: bool,
    pub buffer: String,
    pub replace_on_input: bool,
}

impl Default for OddsMatcherEditorState {
    fn default() -> Self {
        Self {
            selected_field_index: 0,
            editing: false,
            buffer: String::new(),
            replace_on_input: false,
        }
    }
}

impl OddsMatcherEditorState {
    pub fn selected_field(&self) -> OddsMatcherField {
        OddsMatcherField::ALL[self.selected_field_index]
    }

    pub fn select_next_field(&mut self) {
        self.selected_field_index = (self.selected_field_index + 1) % OddsMatcherField::ALL.len();
    }

    pub fn select_previous_field(&mut self) {
        self.selected_field_index = if self.selected_field_index == 0 {
            OddsMatcherField::ALL.len() - 1
        } else {
            self.selected_field_index - 1
        };
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphQlRequest<T> {
    #[serde(rename = "operationName")]
    pub operation_name: String,
    pub query: String,
    pub variables: T,
}

impl GraphQlRequest<GetBestMatchesVariables> {
    pub fn get_best_matches(variables: GetBestMatchesVariables) -> Self {
        Self {
            operation_name: String::from("GetBestMatches"),
            query: String::from(GET_BEST_MATCHES_QUERY),
            variables,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GraphQlResponse<T> {
    pub data: Option<T>,
    #[serde(default)]
    pub errors: Vec<GraphQlError>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GraphQlError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GetBestMatchesData {
    #[serde(rename = "getBestMatches")]
    pub get_best_matches: Vec<OddsMatcherRow>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OddsMatcherRow {
    #[serde(rename = "eventName")]
    pub event_name: String,
    pub id: String,
    #[serde(rename = "startAt")]
    pub start_at: String,
    #[serde(rename = "selectionId")]
    pub selection_id: String,
    #[serde(rename = "marketId")]
    pub market_id: String,
    #[serde(rename = "eventId")]
    pub event_id: String,
    pub back: PriceLeg,
    pub lay: LayLeg,
    #[serde(rename = "eventGroup")]
    pub event_group: GroupSummary,
    #[serde(rename = "marketGroup")]
    pub market_group: GroupSummary,
    #[serde(rename = "marketName")]
    pub market_name: String,
    #[serde(deserialize_with = "deserialize_f64_from_string_or_number")]
    pub rating: f64,
    #[serde(rename = "selectionName")]
    pub selection_name: String,
    #[serde(default, deserialize_with = "deserialize_optional_f64_from_string_or_number")]
    pub snr: Option<f64>,
    pub sport: SportSummary,
    #[serde(rename = "betRequestId")]
    pub bet_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PriceLeg {
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
    #[serde(deserialize_with = "deserialize_f64_from_string_or_number")]
    pub odds: f64,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: Option<String>,
    #[serde(rename = "deepLink")]
    pub deep_link: Option<String>,
    pub bookmaker: BookmakerSummary,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LayLeg {
    pub bookmaker: BookmakerSummary,
    #[serde(rename = "deepLink")]
    pub deep_link: Option<String>,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: Option<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
    #[serde(deserialize_with = "deserialize_f64_from_string_or_number")]
    pub odds: f64,
    #[serde(default, deserialize_with = "deserialize_optional_f64_from_string_or_number")]
    pub liquidity: Option<f64>,
    #[serde(rename = "betSlip")]
    pub bet_slip: Option<BetSlipRef>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BookmakerSummary {
    #[serde(default)]
    pub active: BookmakerActive,
    pub code: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub id: String,
    pub logo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(untagged)]
pub enum BookmakerActive {
    Bool(bool),
    Labels(Vec<String>),
    #[default]
    Missing,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BetSlipRef {
    #[serde(rename = "marketId")]
    pub market_id: String,
    #[serde(rename = "selectionId")]
    pub selection_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GroupSummary {
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub id: String,
    #[serde(rename = "sourceName")]
    pub source_name: Option<String>,
    pub sport: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SportSummary {
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub id: String,
}

impl OddsMatcherRow {
    pub fn availability_label(&self) -> String {
        self.lay
            .liquidity
            .map(|value| format!("£{value:.2}"))
            .unwrap_or_else(|| String::from("-"))
    }
}

pub fn fetch_best_matches(
    client: &Client,
    variables: &GetBestMatchesVariables,
) -> Result<Vec<OddsMatcherRow>> {
    let payload = GraphQlRequest::get_best_matches(variables.clone());
    let response = client
        .post(ODDSMATCHER_GRAPHQL_URL)
        .json(&payload)
        .send()
        .wrap_err("failed to send OddsMatcher GraphQL request")?;

    let status = response.status();
    let graphql: GraphQlResponse<GetBestMatchesData> = response
        .json()
        .wrap_err("failed to decode OddsMatcher GraphQL response")?;

    if !status.is_success() {
        let detail = graphql
            .errors
            .first()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(eyre!("OddsMatcher GraphQL request failed: {detail}"));
    }

    if !graphql.errors.is_empty() {
        let detail = graphql
            .errors
            .iter()
            .map(|error| error.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(eyre!("OddsMatcher GraphQL returned errors: {detail}"));
    }

    let data = graphql
        .data
        .ok_or_else(|| eyre!("OddsMatcher GraphQL response did not include data"))?;
    Ok(data.get_best_matches)
}

#[cfg(test)]
mod tests {
    use super::{
        fetch_best_matches, load_query_or_default, save_query, GetBestMatchesData,
        GetBestMatchesVariables, GraphQlRequest, GraphQlResponse, OddsMatcherField,
        OddsMatcherRow,
    };

    #[test]
    fn request_payload_uses_captured_defaults() {
        let payload = GraphQlRequest::get_best_matches(GetBestMatchesVariables::default());
        let json = serde_json::to_value(payload).expect("serialize request");

        assert_eq!(json["operationName"], "GetBestMatches");
        assert_eq!(json["variables"]["bookmaker"][0], "betvictor");
        assert_eq!(json["variables"]["exchange"][0], "smarketsexchange");
        assert_eq!(json["variables"]["permittedMarketGroups"][0], "match-odds");
    }

    #[test]
    fn request_variables_serialize_to_graphql_field_names() {
        let variables =
            serde_json::to_value(GetBestMatchesVariables::default()).expect("serialize variables");

        assert_eq!(variables["ratingType"], "rating");
        assert_eq!(variables["updatedWithinSeconds"], 21600);
    }

    #[test]
    fn response_deserializes_captured_oddsmatcher_shape() {
        let response: GraphQlResponse<GetBestMatchesData> = serde_json::from_str(
            r#"{
              "data": {
                "getBestMatches": [
                  {
                    "eventName": "Arsenal v Everton",
                    "id": "match-1",
                    "startAt": "2026-03-14T17:30:00Z",
                    "selectionId": "sel-1",
                    "marketId": "mkt-1",
                    "eventId": "evt-1",
                    "back": {
                      "updatedAt": "2026-03-18T12:00:00Z",
                      "odds": 2.55,
                      "fetchedAt": "2026-03-18T12:00:00Z",
                      "deepLink": "https://bookie.example/bet",
                      "bookmaker": {
                        "active": true,
                        "code": "betvictor",
                        "displayName": "BetVictor",
                        "id": "101",
                        "logo": "/logos/80x30/betvictor.png"
                      }
                    },
                    "lay": {
                      "bookmaker": {
                        "active": true,
                        "code": "smarketsexchange",
                        "displayName": "Smarkets Exchange",
                        "id": "201",
                        "logo": "/logos/80x30/smarkets.png"
                      },
                      "deepLink": "https://smarkets.example/betslip",
                      "fetchedAt": "2026-03-18T12:00:00Z",
                      "updatedAt": "2026-03-18T12:00:00Z",
                      "odds": 2.66,
                      "liquidity": 30.0,
                      "betSlip": {
                        "marketId": "mkt-1",
                        "selectionId": "sel-1"
                      }
                    },
                    "eventGroup": {
                      "displayName": "Premier League",
                      "id": "grp-1",
                      "sourceName": "premier-league",
                      "sport": "soccer"
                    },
                    "marketGroup": {
                      "displayName": "Match Odds",
                      "id": "mg-1",
                      "sourceName": null,
                      "sport": "soccer"
                    },
                    "marketName": "Match Odds",
                    "rating": "95.81",
                    "selectionName": "Arsenal",
                    "snr": "0",
                    "sport": {
                      "displayName": "Soccer",
                      "id": "soccer"
                    },
                    "betRequestId": "req-1"
                  }
                ]
              },
              "errors": []
            }"#,
        )
        .expect("deserialize response");

        let data = response.data.expect("data");
        let row: &OddsMatcherRow = data.get_best_matches.first().expect("row");
        assert_eq!(row.event_name, "Arsenal v Everton");
        assert_eq!(row.back.bookmaker.display_name, "BetVictor");
        assert_eq!(row.back.bookmaker.active, super::BookmakerActive::Bool(true));
        assert_eq!(row.lay.liquidity, Some(30.0));
        assert_eq!(row.market_group.display_name, "Match Odds");
        assert_eq!(row.snr, Some(0.0));
    }

    #[test]
    fn response_deserializes_live_bookmaker_active_labels() {
        let response: GraphQlResponse<GetBestMatchesData> = serde_json::from_str(
            r#"{
              "data": {
                "getBestMatches": [
                  {
                    "eventName": "Tottenham v Atletico Madrid",
                    "id": "match-2",
                    "startAt": "2026-03-18T19:00:00Z",
                    "selectionId": "sel-2",
                    "marketId": "mkt-2",
                    "eventId": "evt-2",
                    "back": {
                      "updatedAt": "2026-03-18T12:00:00Z",
                      "odds": "3.25",
                      "fetchedAt": "2026-03-18T12:00:00Z",
                      "deepLink": "https://bookie.example/bet",
                      "bookmaker": {
                        "active": ["desktop", "mobile"],
                        "code": "betvictor",
                        "displayName": "BetVictor",
                        "id": "101",
                        "logo": "/logos/80x30/betvictor.png"
                      }
                    },
                    "lay": {
                      "bookmaker": {
                        "active": ["desktop"],
                        "code": "smarketsexchange",
                        "displayName": "Smarkets Exchange",
                        "id": "201",
                        "logo": "/logos/80x30/smarkets.png"
                      },
                      "deepLink": "https://smarkets.example/betslip",
                      "fetchedAt": "2026-03-18T12:00:00Z",
                      "updatedAt": "2026-03-18T12:00:00Z",
                      "odds": "3.35",
                      "liquidity": "51.17",
                      "betSlip": {
                        "marketId": "mkt-2",
                        "selectionId": "sel-2"
                      }
                    },
                    "eventGroup": {
                      "displayName": "Club Friendlies",
                      "id": "grp-2",
                      "sourceName": "club-friendlies",
                      "sport": "soccer"
                    },
                    "marketGroup": {
                      "displayName": "Match Odds",
                      "id": "mg-2",
                      "sourceName": null,
                      "sport": "soccer"
                    },
                    "marketName": "Match Odds",
                    "rating": "97.14",
                    "selectionName": "Tottenham",
                    "snr": "0",
                    "sport": {
                      "displayName": "Soccer",
                      "id": "soccer"
                    },
                    "betRequestId": "req-2"
                  }
                ]
              },
              "errors": []
            }"#,
        )
        .expect("deserialize response");

        let data = response.data.expect("data");
        let row: &OddsMatcherRow = data.get_best_matches.first().expect("row");
        assert_eq!(
            row.back.bookmaker.active,
            super::BookmakerActive::Labels(vec![
                String::from("desktop"),
                String::from("mobile"),
            ])
        );
    }

    #[test]
    fn fetch_best_matches_surface_is_linkable_for_live_use() {
        let client = reqwest::blocking::Client::new();
        let function_ptr: fn(&reqwest::blocking::Client, &GetBestMatchesVariables)
            -> color_eyre::Result<Vec<OddsMatcherRow>> = fetch_best_matches;
        let _ = (client, function_ptr);
    }

    #[test]
    fn oddsmatcher_config_round_trips_through_disk() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config_path = temp_dir.path().join("oddsmatcher.json");
        let query = GetBestMatchesVariables {
            bookmaker: vec![String::from("betvictor"), String::from("bet365")],
            exchange: vec![String::from("smarketsexchange")],
            rating_type: String::from("rating"),
            min_rating: Some(String::from("90")),
            max_rating: Some(String::from("99")),
            min_odds: Some(String::from("2.1")),
            max_odds: Some(String::from("6")),
            min_liquidity: Some(String::from("40")),
            limit: 25,
            skip: 10,
            updated_within_seconds: 3600,
            exclude_draw: true,
            permitted_sports: vec![String::from("soccer")],
            permitted_market_groups: vec![String::from("match-odds")],
            permitted_event_groups: vec![String::from("featured")],
            permitted_countries: vec![String::from("uk")],
            permitted_event_ids: vec![String::from("evt-1")],
        };

        let note = save_query(&config_path, &query).expect("save config");
        assert!(note.contains("Saved OddsMatcher config"));

        let (loaded, load_note) = load_query_or_default(&config_path).expect("load config");
        assert_eq!(loaded, query);
        assert!(load_note.contains("Loaded OddsMatcher config"));
    }

    #[test]
    fn oddsmatcher_fields_apply_values_to_query() {
        let mut query = GetBestMatchesVariables::default();

        OddsMatcherField::Bookmaker
            .apply_value(&mut query, "betvictor, bet365")
            .expect("apply bookmakers");
        OddsMatcherField::MinLiquidity
            .apply_value(&mut query, "")
            .expect("clear liquidity");
        OddsMatcherField::Limit
            .apply_value(&mut query, "25")
            .expect("apply limit");
        OddsMatcherField::ExcludeDraw
            .apply_value(&mut query, "true")
            .expect("apply exclude draw");

        assert_eq!(
            query.bookmaker,
            vec![String::from("betvictor"), String::from("bet365")]
        );
        assert_eq!(query.min_liquidity, None);
        assert_eq!(query.limit, 25);
        assert!(query.exclude_draw);
    }
}

fn deserialize_f64_from_string_or_number<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(value) => value
            .parse::<f64>()
            .map_err(|_| D::Error::custom(format!("invalid numeric string: {value}"))),
        serde_json::Value::Number(value) => value
            .as_f64()
            .ok_or_else(|| D::Error::custom("invalid numeric value")),
        _ => Err(D::Error::custom("expected string or number")),
    }
}

fn parse_csv_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_optional_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn deserialize_optional_f64_from_string_or_number<'de, D>(
    deserializer: D,
) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => value
            .parse::<f64>()
            .map(Some)
            .map_err(|_| D::Error::custom(format!("invalid numeric string: {value}"))),
        Some(serde_json::Value::Number(value)) => value
            .as_f64()
            .map(Some)
            .ok_or_else(|| D::Error::custom("invalid numeric value")),
        Some(_) => Err(D::Error::custom("expected string or number")),
    }
}
