use std::cmp::Ordering;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::{HorseMatcherQuote, HorseMatcherSnapshot, HorseMatcherSource, VenueId};
use crate::oddsmatcher::{
    BetSlipRef, BookmakerActive, BookmakerSummary, GroupSummary, LayLeg, OddsMatcherRow, PriceLeg,
    SportSummary,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HorseMatcherMode {
    RacesPerOffer,
    OffersPerRace,
}

impl HorseMatcherMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::RacesPerOffer => "Races/Offer",
            Self::OffersPerRace => "Offers/Race",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HorseMatcherQuery {
    pub mode: HorseMatcherMode,
    pub bookmakers: Vec<String>,
    pub exchanges: Vec<String>,
    pub rating_type: String,
    pub min_rating: Option<String>,
    pub min_odds: Option<String>,
    pub search: Vec<String>,
    pub limit: usize,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub offers: Vec<String>,
    pub offer_types: Vec<String>,
}

impl Default for HorseMatcherQuery {
    fn default() -> Self {
        Self {
            mode: HorseMatcherMode::RacesPerOffer,
            bookmakers: vec![String::from("betfred"), String::from("coral")],
            exchanges: vec![String::from("smarkets"), String::from("betdaq")],
            rating_type: String::from("rating"),
            min_rating: Some(String::from("90")),
            min_odds: Some(String::from("2.0")),
            search: Vec::new(),
            limit: 25,
            date_from: None,
            date_to: None,
            offers: Vec::new(),
            offer_types: Vec::new(),
        }
    }
}

pub fn default_query_path() -> PathBuf {
    if let Some(path) = env::var_os("SABI_HORSE_MATCHER_CONFIG_PATH") {
        return PathBuf::from(path);
    }

    let config_root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));

    config_root.join("sabi").join("horsematcher.json")
}

pub fn load_query_or_default(path: &Path) -> Result<(HorseMatcherQuery, String)> {
    if !path.exists() {
        return Ok((
            HorseMatcherQuery::default(),
            String::from("Using default Horse Matcher config."),
        ));
    }

    let content = fs::read_to_string(path)?;
    let query = serde_json::from_str::<HorseMatcherQuery>(&content)?;
    let (query, repaired_fields) = normalize_loaded_query(query);
    let repair_note = if repaired_fields.is_empty() {
        String::new()
    } else {
        format!(" Repaired {}.", repaired_fields.join(", "))
    };
    Ok((
        query,
        format!(
            "Loaded Horse Matcher config from {}.{}",
            path.display(),
            repair_note
        ),
    ))
}

pub fn save_query(path: &Path, query: &HorseMatcherQuery) -> Result<String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(query)? + "\n")?;
    Ok(format!("Saved Horse Matcher config to {}.", path.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorseMatcherField {
    Mode,
    Bookmaker,
    Exchange,
    Search,
    Offer,
    OfferType,
    RatingType,
    MinRating,
    MinOdds,
    Limit,
    DateFrom,
    DateTo,
}

impl HorseMatcherField {
    pub const ALL: [Self; 12] = [
        Self::Mode,
        Self::Bookmaker,
        Self::Exchange,
        Self::Search,
        Self::Offer,
        Self::OfferType,
        Self::RatingType,
        Self::MinRating,
        Self::MinOdds,
        Self::Limit,
        Self::DateFrom,
        Self::DateTo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mode => "Mode",
            Self::Bookmaker => "Bookmaker",
            Self::Exchange => "Exchange",
            Self::Search => "Race Search",
            Self::Offer => "Offers",
            Self::OfferType => "Offer Types",
            Self::RatingType => "Rating Type",
            Self::MinRating => "Min Rating",
            Self::MinOdds => "Min Odds",
            Self::Limit => "Limit",
            Self::DateFrom => "Date From",
            Self::DateTo => "Date To",
        }
    }

    pub fn display_value(self, query: &HorseMatcherQuery) -> String {
        match self {
            Self::Mode => match query.mode {
                HorseMatcherMode::RacesPerOffer => String::from("races_per_offer"),
                HorseMatcherMode::OffersPerRace => String::from("offers_per_race"),
            },
            Self::Bookmaker => query.bookmakers.join(","),
            Self::Exchange => query.exchanges.join(","),
            Self::Search => query.search.join(","),
            Self::Offer => query.offers.join(","),
            Self::OfferType => query.offer_types.join(","),
            Self::RatingType => query.rating_type.clone(),
            Self::MinRating => query.min_rating.clone().unwrap_or_default(),
            Self::MinOdds => query.min_odds.clone().unwrap_or_default(),
            Self::Limit => query.limit.to_string(),
            Self::DateFrom => query.date_from.clone().unwrap_or_default(),
            Self::DateTo => query.date_to.clone().unwrap_or_default(),
        }
    }

    pub fn apply_value(self, query: &mut HorseMatcherQuery, value: &str) -> Result<()> {
        match self {
            Self::Mode => {
                query.mode = match value.trim() {
                    "races_per_offer" => HorseMatcherMode::RacesPerOffer,
                    "offers_per_race" => HorseMatcherMode::OffersPerRace,
                    _ => return Err(eyre!("mode must be races_per_offer or offers_per_race")),
                };
            }
            Self::Bookmaker => query.bookmakers = parse_csv_list(value),
            Self::Exchange => query.exchanges = parse_csv_list(value),
            Self::Search => query.search = parse_csv_list(value),
            Self::Offer => query.offers = parse_csv_list(value),
            Self::OfferType => query.offer_types = parse_csv_list(value),
            Self::RatingType => query.rating_type = value.trim().to_string(),
            Self::MinRating => query.min_rating = parse_optional_string(value),
            Self::MinOdds => query.min_odds = parse_optional_string(value),
            Self::Limit => {
                query.limit = value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| eyre!("limit must be a positive integer"))?;
            }
            Self::DateFrom => query.date_from = parse_optional_string(value),
            Self::DateTo => query.date_to = parse_optional_string(value),
        }
        Ok(())
    }

    pub fn suggestions(self) -> Vec<String> {
        match self {
            Self::Mode => vec![
                String::from("races_per_offer"),
                String::from("offers_per_race"),
            ],
            Self::Bookmaker => vec![
                String::from("betfred,coral"),
                String::from("betfred,coral,ladbrokes"),
                String::from("betfred,coral,ladbrokes,kwik,bet600"),
            ],
            Self::Exchange => vec![
                String::from("smarkets"),
                String::from("smarkets,betdaq"),
                String::from("smarkets,betdaq,matchbook"),
            ],
            Self::Search => Vec::new(),
            Self::Offer => Vec::new(),
            Self::OfferType => Vec::new(),
            Self::RatingType => vec![String::from("rating"), String::from("snr")],
            Self::MinRating => vec![String::from("90"), String::from("95")],
            Self::MinOdds => vec![String::from("2.0"), String::from("3.0")],
            Self::Limit => vec![String::from("25"), String::from("50"), String::from("100")],
            Self::DateFrom | Self::DateTo => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Default)]
pub struct HorseMatcherEditorState {
    selected_field_index: usize,
    pub editing: bool,
    pub buffer: String,
    pub replace_on_input: bool,
}


impl HorseMatcherEditorState {
    pub fn selected_field(&self) -> HorseMatcherField {
        HorseMatcherField::ALL[self.selected_field_index]
    }

    pub fn select_next_field(&mut self) {
        self.selected_field_index = (self.selected_field_index + 1) % HorseMatcherField::ALL.len();
    }

    pub fn select_previous_field(&mut self) {
        self.selected_field_index = if self.selected_field_index == 0 {
            HorseMatcherField::ALL.len() - 1
        } else {
            self.selected_field_index - 1
        };
    }
}

pub fn build_request_payload(query: &HorseMatcherQuery) -> Value {
    json!({
        "ratingType": query.rating_type,
        "bookmakers": query.bookmakers,
        "exchanges": query.exchanges,
        "minOdds": query.min_odds,
        "minRating": query.min_rating,
        "search": query.search,
        "limit": query.limit,
        "dateFrom": query.date_from,
        "dateTo": query.date_to,
    })
}

pub fn build_rows(
    snapshot: &HorseMatcherSnapshot,
    query: &HorseMatcherQuery,
) -> Result<Vec<OddsMatcherRow>> {
    let sportsbook_filter = lower_set(&query.bookmakers);
    let exchange_filter = lower_set(&query.exchanges);
    let search_filter = query
        .search
        .iter()
        .map(|value| normalize_key(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    let mut best_backs: HashMap<String, AggregatedQuote> = HashMap::new();
    let mut best_lays: HashMap<String, AggregatedQuote> = HashMap::new();

    for source in snapshot
        .sources
        .iter()
        .filter(|source| source.status.eq_ignore_ascii_case("ready"))
    {
        if source.kind.eq_ignore_ascii_case("sportsbook")
            && !sportsbook_filter.is_empty()
            && !sportsbook_filter.contains(&source.venue.as_str().to_ascii_lowercase())
        {
            continue;
        }
        if source.kind.eq_ignore_ascii_case("exchange")
            && !exchange_filter.is_empty()
            && !exchange_filter.contains(&source.venue.as_str().to_ascii_lowercase())
        {
            continue;
        }

        let event_key = normalize_key(&source.event_name);
        if event_key.is_empty() {
            continue;
        }
        if !search_filter.is_empty() && !matches_search_filters(source, &search_filter) {
            continue;
        }
        if !matches_date_filters(source, snapshot, query) {
            continue;
        }

        for quote in &source.quotes {
            if quote.selection_name.trim().is_empty() || quote.odds <= 1.0 {
                continue;
            }
            let quote_side = quote.side.to_ascii_lowercase();
            let selection_key = normalize_key(&quote.selection_name);
            if selection_key.is_empty() {
                continue;
            }
            let aggregate_key = format!(
                "{}::{}::{}",
                event_key,
                normalize_key(&source.market_name),
                selection_key
            );
            let aggregate = AggregatedQuote::new(source, quote, snapshot);

            if source.kind.eq_ignore_ascii_case("sportsbook")
                && (quote_side == "back" || quote_side.is_empty())
            {
                upsert_best_back(&mut best_backs, aggregate_key, aggregate);
            } else if source.kind.eq_ignore_ascii_case("exchange")
                && matches!(quote_side.as_str(), "lay" | "sell" | "back")
            {
                upsert_best_lay(&mut best_lays, aggregate_key, aggregate);
            }
        }
    }

    let mut rows = best_backs
        .into_iter()
        .filter_map(|(key, back)| {
            let lay = best_lays.get(&key)?;
            let rating = compute_rating(
                back.source.venue,
                lay.source.venue,
                back.quote.odds,
                lay.quote.odds,
            );
            if !matches_numeric_filters(query, rating, back.quote.odds) {
                return None;
            }
            Some(build_row(back, lay.clone(), rating))
        })
        .collect::<Vec<_>>();

    sort_rows(&mut rows, query.mode);
    rows.truncate(query.limit);

    if rows.is_empty() {
        let ready_sources = snapshot
            .sources
            .iter()
            .filter(|source| source.status.eq_ignore_ascii_case("ready"))
            .count();
        return Err(eyre!(
            "No internal horse matcher rows were produced from {} readable source(s).",
            ready_sources
        ));
    }

    Ok(rows)
}

#[derive(Debug, Clone)]
struct AggregatedQuote {
    source: HorseMatcherSource,
    quote: HorseMatcherQuote,
    resolved_start_at: String,
}

impl AggregatedQuote {
    fn new(
        source: &HorseMatcherSource,
        quote: &HorseMatcherQuote,
        snapshot: &HorseMatcherSnapshot,
    ) -> Self {
        Self {
            source: source.clone(),
            quote: quote.clone(),
            resolved_start_at: resolve_start_at(source, snapshot),
        }
    }
}

fn lower_set(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize_loaded_query(mut query: HorseMatcherQuery) -> (HorseMatcherQuery, Vec<&'static str>) {
    let defaults = HorseMatcherQuery::default();
    let mut repaired_fields = Vec::new();

    if query.rating_type.trim().is_empty() {
        query.rating_type = defaults.rating_type;
        repaired_fields.push("rating type");
    } else {
        query.rating_type = query.rating_type.trim().to_string();
    }
    if query.limit == 0 {
        query.limit = defaults.limit;
        repaired_fields.push("limit");
    }

    normalize_query_lists(&mut query.bookmakers);
    normalize_query_lists(&mut query.exchanges);
    normalize_query_lists(&mut query.search);
    normalize_query_lists(&mut query.offers);
    normalize_query_lists(&mut query.offer_types);
    normalize_optional_query_string(&mut query.min_rating);
    normalize_optional_query_string(&mut query.min_odds);
    normalize_optional_query_string(&mut query.date_from);
    normalize_optional_query_string(&mut query.date_to);

    (query, repaired_fields)
}

fn normalize_query_lists(values: &mut Vec<String>) {
    *values = values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
}

fn normalize_optional_query_string(value: &mut Option<String>) {
    *value = value
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn matches_search_filters(source: &HorseMatcherSource, filters: &[String]) -> bool {
    let mut haystack = normalize_key(&source.event_name);
    if !source.market_name.is_empty() {
        haystack.push(' ');
        haystack.push_str(&normalize_key(&source.market_name));
    }
    for quote in &source.quotes {
        haystack.push(' ');
        haystack.push_str(&normalize_key(&quote.selection_name));
    }
    filters.iter().any(|filter| haystack.contains(filter))
}

fn matches_date_filters(
    source: &HorseMatcherSource,
    snapshot: &HorseMatcherSnapshot,
    query: &HorseMatcherQuery,
) -> bool {
    let effective_date = resolve_event_date(source, snapshot);
    if let Some(date_from) = &query.date_from {
        if effective_date.as_deref().unwrap_or_default() < date_from.as_str() {
            return false;
        }
    }
    if let Some(date_to) = &query.date_to {
        if effective_date.as_deref().unwrap_or_default() > date_to.as_str() {
            return false;
        }
    }
    true
}

fn resolve_event_date(
    source: &HorseMatcherSource,
    snapshot: &HorseMatcherSnapshot,
) -> Option<String> {
    first_date_component(&source.captured_at)
        .or_else(|| first_date_component(&snapshot.captured_at))
        .map(String::from)
}

fn resolve_start_at(source: &HorseMatcherSource, snapshot: &HorseMatcherSnapshot) -> String {
    let date = resolve_event_date(source, snapshot).unwrap_or_else(|| String::from("1970-01-01"));
    if let Some(time) = first_time_component(&source.start_hint)
        .or_else(|| first_time_component(&source.event_name))
    {
        return format!("{date}T{time}:00Z");
    }
    if let Some(timestamp) =
        first_timestamp(&source.captured_at).or_else(|| first_timestamp(&snapshot.captured_at))
    {
        return timestamp.to_string();
    }
    format!("{date}T00:00:00Z")
}

fn first_date_component(value: &str) -> Option<&str> {
    value
        .find('T')
        .map(|index| &value[..index])
        .filter(|value| value.len() == 10)
}

fn first_time_component(value: &str) -> Option<&str> {
    value.split_whitespace().find(|token| {
        token.len() == 5
            && token.chars().nth(2) == Some(':')
            && token.chars().enumerate().all(|(index, character)| {
                if index == 2 {
                    character == ':'
                } else {
                    character.is_ascii_digit()
                }
            })
    })
}

fn first_timestamp(value: &str) -> Option<&str> {
    value
        .contains('T')
        .then_some(value)
        .filter(|value| value.ends_with('Z'))
}

fn upsert_best_back(
    quotes: &mut HashMap<String, AggregatedQuote>,
    key: String,
    candidate: AggregatedQuote,
) {
    match quotes.get_mut(&key) {
        Some(existing) => {
            if candidate.quote.odds > existing.quote.odds {
                *existing = candidate;
            }
        }
        None => {
            quotes.insert(key, candidate);
        }
    }
}

fn upsert_best_lay(
    quotes: &mut HashMap<String, AggregatedQuote>,
    key: String,
    candidate: AggregatedQuote,
) {
    match quotes.get_mut(&key) {
        Some(existing) => {
            let candidate_liquidity = candidate.quote.liquidity.unwrap_or(0.0);
            let existing_liquidity = existing.quote.liquidity.unwrap_or(0.0);
            if candidate.quote.odds < existing.quote.odds
                || (candidate.quote.odds - existing.quote.odds).abs() < f64::EPSILON
                    && candidate_liquidity > existing_liquidity
            {
                *existing = candidate;
            }
        }
        None => {
            quotes.insert(key, candidate);
        }
    }
}

fn matches_numeric_filters(query: &HorseMatcherQuery, rating: f64, back_odds: f64) -> bool {
    if let Some(min_rating) = parse_optional_f64(query.min_rating.as_deref()) {
        if rating < min_rating {
            return false;
        }
    }
    if let Some(min_odds) = parse_optional_f64(query.min_odds.as_deref()) {
        if back_odds < min_odds {
            return false;
        }
    }
    true
}

fn parse_optional_f64(value: Option<&str>) -> Option<f64> {
    value.and_then(|value| value.trim().parse::<f64>().ok())
}

fn compute_rating(
    bookmaker_venue: VenueId,
    exchange_venue: VenueId,
    back_odds: f64,
    lay_odds: f64,
) -> f64 {
    let commission = exchange_commission(exchange_venue).max(bookmaker_commission(bookmaker_venue));
    let effective_lay = 1.0 + ((lay_odds - 1.0) / (1.0 - commission));
    (back_odds / effective_lay) * 100.0
}

fn bookmaker_commission(_venue: VenueId) -> f64 {
    0.0
}

fn exchange_commission(venue: VenueId) -> f64 {
    match venue {
        VenueId::Smarkets => 0.02,
        VenueId::Betdaq => 0.02,
        _ => 0.0,
    }
}

fn build_row(back: AggregatedQuote, lay: AggregatedQuote, rating: f64) -> OddsMatcherRow {
    let event_key = normalize_key(&back.source.event_name);
    let selection_key = normalize_key(&back.quote.selection_name);
    let market_key = normalize_key(&back.source.market_name);
    let id = format!(
        "internal-horse::{event_key}::{market_key}::{selection_key}::{}::{}",
        back.source.venue.as_str(),
        lay.source.venue.as_str()
    );
    let event_group_name = racecourse_label(&back.source.event_name);
    OddsMatcherRow {
        event_name: back.source.event_name.clone(),
        id,
        start_at: back.resolved_start_at.clone(),
        selection_id: format!("runner::{selection_key}"),
        market_id: format!("market::{event_key}::{market_key}"),
        event_id: format!("event::{event_key}"),
        back: PriceLeg {
            updated_at: Some(back.source.captured_at.clone()),
            odds: back.quote.odds,
            fetched_at: Some(back.source.captured_at.clone()),
            deep_link: Some(back.source.page_url.clone()),
            bookmaker: bookmaker_summary(back.source.venue, &back.source.venue_label),
        },
        lay: LayLeg {
            bookmaker: bookmaker_summary(lay.source.venue, &lay.source.venue_label),
            deep_link: Some(lay.source.page_url.clone()),
            fetched_at: Some(lay.source.captured_at.clone()),
            updated_at: Some(lay.source.captured_at.clone()),
            odds: lay.quote.odds,
            liquidity: lay.quote.liquidity,
            bet_slip: Some(BetSlipRef {
                market_id: format!("market::{event_key}::{market_key}"),
                selection_id: format!("runner::{selection_key}"),
            }),
        },
        event_group: GroupSummary {
            display_name: event_group_name.clone(),
            id: format!("racecourse::{}", normalize_key(&event_group_name)),
            source_name: Some(event_group_name),
            sport: Some(String::from("horse-racing")),
        },
        market_group: GroupSummary {
            display_name: String::from("Win Market"),
            id: String::from("horse-racing-win"),
            source_name: Some(String::from("Win Market")),
            sport: Some(String::from("horse-racing")),
        },
        market_name: String::from("Win"),
        rating,
        selection_name: back.quote.selection_name.clone(),
        snr: None,
        sport: SportSummary {
            display_name: String::from("Horse Racing"),
            id: String::from("horse-racing"),
        },
        bet_request_id: None,
    }
}

fn racecourse_label(event_name: &str) -> String {
    let tokens = event_name.split_whitespace().collect::<Vec<_>>();
    if tokens.len() >= 2 && first_time_component(tokens[0]).is_some() {
        return tokens[1..].join(" ");
    }
    event_name.to_string()
}

fn bookmaker_summary(venue: VenueId, label: &str) -> BookmakerSummary {
    let display_name = if label.trim().is_empty() {
        match venue {
            VenueId::Smarkets => "Smarkets",
            VenueId::Bet10 => "Bet10",
            VenueId::Betdaq => "Betdaq",
            VenueId::Betano => "Betano",
            VenueId::Betfair => "Betfair",
            VenueId::Betfred => "Betfred",
            VenueId::Betmgm => "BetMGM",
            VenueId::Betvictor => "BetVictor",
            VenueId::Boylesports => "BoyleSports",
            VenueId::Coral => "Coral",
            VenueId::Fanteam => "FanTeam",
            VenueId::Ladbrokes => "Ladbrokes",
            VenueId::Kwik => "Kwik",
            VenueId::Bet600 => "Bet600",
            VenueId::Bet365 => "bet365",
            VenueId::Betuk => "BetUK",
            VenueId::Betway => "Betway",
            VenueId::Leovegas => "LeoVegas",
            VenueId::Matchbook => "Matchbook",
            VenueId::Midnite => "Midnite",
            VenueId::Paddypower => "Paddy Power",
            VenueId::Skybet => "Sky Bet",
            VenueId::Sportingindex => "Sporting Index",
            VenueId::Talksportbet => "talkSPORT BET",
            VenueId::Williamhill => "William Hill",
        }
        .to_string()
    } else {
        label.trim().to_string()
    };
    BookmakerSummary {
        active: BookmakerActive::Bool(true),
        code: venue.as_str().to_string(),
        display_name,
        id: venue.as_str().to_string(),
        logo: None,
    }
}

fn sort_rows(rows: &mut [OddsMatcherRow], mode: HorseMatcherMode) {
    rows.sort_by(|left, right| match mode {
        HorseMatcherMode::RacesPerOffer => right
            .rating
            .partial_cmp(&left.rating)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                left.back
                    .bookmaker
                    .display_name
                    .cmp(&right.back.bookmaker.display_name)
            })
            .then_with(|| left.event_name.cmp(&right.event_name)),
        HorseMatcherMode::OffersPerRace => left
            .event_name
            .cmp(&right.event_name)
            .then_with(|| {
                right
                    .rating
                    .partial_cmp(&left.rating)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| left.selection_name.cmp(&right.selection_name)),
    });
}

fn parse_csv_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(String::from)
        .collect()
}

fn parse_optional_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{
        build_request_payload, build_rows, load_query_or_default, save_query, HorseMatcherField,
        HorseMatcherMode, HorseMatcherQuery,
    };
    use crate::domain::{HorseMatcherQuote, HorseMatcherSnapshot, HorseMatcherSource, VenueId};

    #[test]
    fn horse_matcher_config_round_trips_through_disk() {
        let temp_dir = tempdir().expect("tempdir");
        let config_path = temp_dir.path().join("horsematcher.json");
        let query = HorseMatcherQuery {
            mode: HorseMatcherMode::OffersPerRace,
            bookmakers: vec![String::from("betfred"), String::from("coral")],
            exchanges: vec![String::from("smarkets"), String::from("betdaq")],
            rating_type: String::from("snr"),
            min_rating: Some(String::from("94")),
            min_odds: Some(String::from("3.0")),
            search: vec![String::from("Cheltenham 15:20")],
            limit: 50,
            date_from: Some(String::from("2026-03-19")),
            date_to: Some(String::from("2026-03-20")),
            offers: vec![String::from("refund")],
            offer_types: vec![String::from("win")],
        };

        save_query(&config_path, &query).expect("save query");
        let (loaded, _) = load_query_or_default(&config_path).expect("load query");

        assert_eq!(loaded, query);
    }

    #[test]
    fn horse_matcher_fields_apply_values_to_query() {
        let mut query = HorseMatcherQuery::default();
        HorseMatcherField::Mode
            .apply_value(&mut query, "offers_per_race")
            .expect("apply mode");
        HorseMatcherField::Bookmaker
            .apply_value(&mut query, "betfred, coral")
            .expect("apply bookmakers");
        HorseMatcherField::Search
            .apply_value(&mut query, "Cheltenham 15:20, Ascot 16:00")
            .expect("apply search");
        HorseMatcherField::Limit
            .apply_value(&mut query, "50")
            .expect("apply limit");

        assert_eq!(query.mode, HorseMatcherMode::OffersPerRace);
        assert_eq!(
            query.bookmakers,
            vec![String::from("betfred"), String::from("coral")]
        );
        assert_eq!(
            query.search,
            vec![
                String::from("Cheltenham 15:20"),
                String::from("Ascot 16:00")
            ]
        );
        assert_eq!(query.limit, 50);
    }

    #[test]
    fn horse_matcher_load_repairs_blank_required_fields() {
        let temp_dir = tempdir().expect("tempdir");
        let config_path = temp_dir.path().join("horsematcher.json");
        std::fs::write(
            &config_path,
            r#"{
              "mode": "races_per_offer",
              "bookmakers": ["betfred", ""],
              "exchanges": ["smarkets"],
              "rating_type": "",
              "min_rating": "",
              "min_odds": "2.0",
              "search": [""],
              "limit": 0,
              "date_from": "",
              "date_to": null,
              "offers": [""],
              "offer_types": []
            }"#,
        )
        .expect("write query");

        let (loaded, note) = load_query_or_default(&config_path).expect("load query");

        assert_eq!(loaded.rating_type, "rating");
        assert_eq!(loaded.min_rating, None);
        assert_eq!(loaded.limit, HorseMatcherQuery::default().limit);
        assert_eq!(loaded.bookmakers, vec![String::from("betfred")]);
        assert!(loaded.search.is_empty());
        assert!(note.contains("Repaired rating type"));
    }

    #[test]
    fn horse_matcher_request_payload_matches_captured_shape() {
        let payload = build_request_payload(&HorseMatcherQuery::default());

        assert_eq!(payload["ratingType"], "rating");
        assert!(payload.get("mode").is_none());
        assert!(payload.get("offers").is_none());
        assert!(payload.get("offerTypes").is_none());
    }

    #[test]
    fn horse_matcher_builds_rows_from_internal_market_sources() {
        let snapshot = HorseMatcherSnapshot {
            captured_at: String::from("2026-03-19T09:00:00Z"),
            source_count: 2,
            ready_source_count: 2,
            sources: vec![
                HorseMatcherSource {
                    venue: VenueId::Betfred,
                    venue_label: String::from("Betfred"),
                    kind: String::from("sportsbook"),
                    status: String::from("ready"),
                    detail: String::from("Captured"),
                    page_url: String::from("https://betfred.example/race-1"),
                    page_title: String::from("Cheltenham 15:20 - Win"),
                    event_name: String::from("15:20 Cheltenham"),
                    market_name: String::from("Win"),
                    start_hint: String::from("15:20"),
                    captured_at: String::from("2026-03-19T09:00:00Z"),
                    quotes: vec![HorseMatcherQuote {
                        selection_name: String::from("Desert Hero"),
                        side: String::from("back"),
                        odds: 5.2,
                        liquidity: None,
                    }],
                },
                HorseMatcherSource {
                    venue: VenueId::Smarkets,
                    venue_label: String::from("Smarkets"),
                    kind: String::from("exchange"),
                    status: String::from("ready"),
                    detail: String::from("Captured"),
                    page_url: String::from("https://smarkets.example/race-1"),
                    page_title: String::from("Cheltenham 15:20 - Win"),
                    event_name: String::from("15:20 Cheltenham"),
                    market_name: String::from("Win"),
                    start_hint: String::from("15:20"),
                    captured_at: String::from("2026-03-19T09:00:05Z"),
                    quotes: vec![HorseMatcherQuote {
                        selection_name: String::from("Desert Hero"),
                        side: String::from("lay"),
                        odds: 5.4,
                        liquidity: Some(200.0),
                    }],
                },
            ],
        };

        let rows = build_rows(&snapshot, &HorseMatcherQuery::default()).expect("build rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].selection_name, "Desert Hero");
        assert_eq!(rows[0].back.bookmaker.display_name, "Betfred");
        assert_eq!(rows[0].lay.bookmaker.display_name, "Smarkets");
        assert_eq!(rows[0].lay.liquidity, Some(200.0));
    }

    #[test]
    fn horse_matcher_prefers_best_prices_across_internal_sources() {
        let snapshot = HorseMatcherSnapshot {
            captured_at: String::from("2026-03-19T09:00:00Z"),
            source_count: 4,
            ready_source_count: 4,
            sources: vec![
                sample_source(VenueId::Betfred, "Betfred", "sportsbook", "back", 5.2, None),
                sample_source(VenueId::Coral, "Coral", "sportsbook", "back", 5.4, None),
                sample_source(
                    VenueId::Smarkets,
                    "Smarkets",
                    "exchange",
                    "lay",
                    5.6,
                    Some(100.0),
                ),
                sample_source(
                    VenueId::Betdaq,
                    "Betdaq",
                    "exchange",
                    "lay",
                    5.5,
                    Some(40.0),
                ),
            ],
        };

        let rows = build_rows(&snapshot, &HorseMatcherQuery::default()).expect("build rows");
        assert_eq!(rows[0].back.bookmaker.display_name, "Coral");
        assert_eq!(rows[0].back.odds, 5.4);
        assert_eq!(rows[0].lay.bookmaker.display_name, "Betdaq");
        assert_eq!(rows[0].lay.odds, 5.5);
    }

    fn sample_source(
        venue: VenueId,
        label: &str,
        kind: &str,
        side: &str,
        odds: f64,
        liquidity: Option<f64>,
    ) -> HorseMatcherSource {
        HorseMatcherSource {
            venue,
            venue_label: String::from(label),
            kind: String::from(kind),
            status: String::from("ready"),
            detail: String::from("Captured"),
            page_url: format!("https://{}.example/race-1", venue.as_str()),
            page_title: String::from("Cheltenham 15:20 - Win"),
            event_name: String::from("15:20 Cheltenham"),
            market_name: String::from("Win"),
            start_hint: String::from("15:20"),
            captured_at: String::from("2026-03-19T09:00:00Z"),
            quotes: vec![HorseMatcherQuote {
                selection_name: String::from("Desert Hero"),
                side: String::from(side),
                odds,
                liquidity,
            }],
        }
    }
}
