use std::env;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result, WrapErr};
use reqwest::blocking::Client;
use reqwest::Method;
use serde_json::{json, Value};

use crate::domain::VenueId;
use crate::trading_actions::{TradingActionIntent, TradingActionMode, TradingActionSide};

const MATCHBOOK_BASE_URL: &str = "https://api.matchbook.com";
const MATCHBOOK_TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct ExchangeApiExecutionResult {
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchbookOfferRow {
    pub offer_id: String,
    pub runner_id: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub side: String,
    pub status: String,
    pub odds: Option<f64>,
    pub stake: Option<f64>,
    pub remaining_stake: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchbookBetRow {
    pub bet_id: String,
    pub runner_id: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub side: String,
    pub status: String,
    pub odds: Option<f64>,
    pub stake: Option<f64>,
    pub profit_loss: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchbookPositionRow {
    pub runner_id: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub exposure: Option<f64>,
    pub profit_loss: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchbookAccountState {
    pub status_line: String,
    pub balance_label: String,
    pub summary: MatchbookPreflightSummary,
    pub current_offers: Vec<MatchbookOfferRow>,
    pub current_bets: Vec<MatchbookBetRow>,
    pub positions: Vec<MatchbookPositionRow>,
}

#[derive(Debug, Clone)]
pub struct MatchbookApiClient {
    client: Client,
    base_url: String,
    token: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchbookPreflightSummary {
    pub balance: Option<String>,
    pub open_offer_count: usize,
    pub current_bet_count: usize,
    pub matched_bet_count: usize,
    pub position_count: usize,
    pub runner_offer_count: usize,
    pub runner_bet_count: usize,
    pub runner_position_count: usize,
    pub runner_open_stake: Option<f64>,
}

impl MatchbookPreflightSummary {
    pub fn detail_suffix(&self) -> String {
        let mut segments = Vec::new();
        if let Some(balance) = self.balance.as_deref() {
            segments.push(balance.to_string());
        }
        segments.push(format!(
            "offers {}{}",
            self.open_offer_count,
            runner_count_suffix(self.runner_offer_count)
        ));
        if let Some(stake) = self.runner_open_stake {
            if stake > 0.0 {
                segments.push(format!("runner stake {}", format_amount(stake)));
            }
        }
        segments.push(format!(
            "bets {}{}",
            self.current_bet_count,
            runner_count_suffix(self.runner_bet_count)
        ));
        segments.push(format!("matched {}", self.matched_bet_count));
        segments.push(format!(
            "positions {}{}",
            self.position_count,
            runner_count_suffix(self.runner_position_count)
        ));
        if segments.is_empty() {
            String::new()
        } else {
            format!(" • {}", segments.join(" • "))
        }
    }
}

impl MatchbookApiClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(MATCHBOOK_TIMEOUT_SECS))
            .build()
            .wrap_err("failed to build Matchbook HTTP client")?;
        let base_url = env::var("MATCHBOOK_API_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| String::from(MATCHBOOK_BASE_URL));
        let token = matchbook_session_token(&client, &base_url)?;
        Ok(Self {
            client,
            base_url,
            token,
        })
    }

    pub fn account(&self) -> Result<Value> {
        self.get_json("/edge/rest/account", "Matchbook account")
    }

    pub fn balance(&self) -> Result<Value> {
        self.get_json("/edge/rest/account/balance", "Matchbook balance")
    }

    pub fn positions(&self) -> Result<Value> {
        self.get_json("/edge/rest/account/positions", "Matchbook positions")
    }

    pub fn offers(&self) -> Result<Value> {
        self.get_json("/edge/rest/v2/offers", "Matchbook offers")
    }

    pub fn aggregated_matched_bets(&self) -> Result<Value> {
        self.get_json(
            "/edge/rest/v2/matched-bets/aggregated",
            "Matchbook aggregated matched bets",
        )
    }

    pub fn current_offers(&self) -> Result<Value> {
        self.get_json(
            "/edge/rest/reports/v2/offers/current",
            "Matchbook current offers",
        )
    }

    pub fn current_bets(&self) -> Result<Value> {
        self.get_json(
            "/edge/rest/reports/v2/bets/current",
            "Matchbook current bets",
        )
    }

    pub fn submit_offer(&self, intent: &TradingActionIntent) -> Result<Value> {
        let runner_id = matchbook_runner_id(intent)?;
        let payload = matchbook_offer_payload(intent, &runner_id);
        self.send_json(
            Method::POST,
            "/edge/rest/v2/offers",
            Some(&payload),
            "submit Matchbook offer",
        )
    }

    pub fn load_preflight_summary(
        &self,
        intent: &TradingActionIntent,
    ) -> Result<MatchbookPreflightSummary> {
        let runner_id = matchbook_runner_id(intent)?;
        let balance = self.balance().ok();
        let current_offers = self.current_offers().ok();
        let current_bets = self.current_bets().ok();
        let matched_bets = self.aggregated_matched_bets().ok();
        let positions = self.positions().ok();

        Ok(matchbook_preflight_summary_from_values(
            &runner_id,
            balance.as_ref(),
            current_offers.as_ref(),
            current_bets.as_ref(),
            matched_bets.as_ref(),
            positions.as_ref(),
        ))
    }

    fn get_json(&self, path: &str, label: &str) -> Result<Value> {
        self.send_json(Method::GET, path, None, label)
    }

    fn send_json(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
        label: &str,
    ) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let mut request = self
            .client
            .request(method.clone(), url)
            .header("accept", "application/json")
            .header("session-token", &self.token);
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .json(body);
        }
        let response = request
            .send()
            .wrap_err_with(|| format!("failed to {label}"))?;
        let status = response.status();
        let response_body = response
            .text()
            .wrap_err_with(|| format!("failed to read {label} response body"))?;
        if !status.is_success() {
            return Err(eyre!(
                "{label} failed with {}: {}",
                status,
                truncate(&response_body, 220)
            ));
        }
        if response_body.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&response_body)
            .wrap_err_with(|| format!("failed to decode {label} response"))
    }
}

pub fn execute_trade(intent: &TradingActionIntent) -> Result<ExchangeApiExecutionResult> {
    match intent.venue {
        VenueId::Matchbook => execute_matchbook_trade(intent),
        VenueId::Betdaq => scaffolded_exchange(
            "Betdaq",
            "BETDAQ API access is scaffolded but not enabled yet. Configure credentials once access is ready.",
        ),
        VenueId::Betfair => scaffolded_exchange(
            "Betfair",
            "Betfair API access is scaffolded but not enabled yet. Configure app key and session flow once access is ready.",
        ),
        venue => Err(eyre!(
            "{} does not have a Rust API execution layer.",
            venue.as_str()
        )),
    }
}

pub fn load_matchbook_account_state() -> Result<MatchbookAccountState> {
    let client = MatchbookApiClient::new()?;
    let account = client.account().ok();
    let balance = client.balance().ok();
    let current_offers = client.current_offers().ok();
    let current_bets = client.current_bets().ok();
    let matched_bets = client.aggregated_matched_bets().ok();
    let positions = client.positions().ok();

    let summary = matchbook_preflight_summary_from_values(
        "",
        balance.as_ref(),
        current_offers.as_ref(),
        current_bets.as_ref(),
        matched_bets.as_ref(),
        positions.as_ref(),
    );
    let account_label = account
        .as_ref()
        .and_then(matchbook_account_label_from_value)
        .unwrap_or_else(|| String::from("account ready"));
    let balance_label = summary
        .balance
        .clone()
        .unwrap_or_else(|| String::from("balance unavailable"));
    let detail_suffix = summary.detail_suffix();

    Ok(MatchbookAccountState {
        status_line: if detail_suffix.is_empty() {
            format!("Matchbook API ready • {account_label}")
        } else {
            format!("Matchbook API ready • {account_label}{detail_suffix}")
        },
        balance_label,
        summary,
        current_offers: parse_matchbook_offer_rows(current_offers.as_ref()),
        current_bets: parse_matchbook_bet_rows(current_bets.as_ref()),
        positions: parse_matchbook_position_rows(positions.as_ref()),
    })
}

fn execute_matchbook_trade(intent: &TradingActionIntent) -> Result<ExchangeApiExecutionResult> {
    let client = MatchbookApiClient::new()?;
    let runner_id = matchbook_runner_id(intent)?;
    let preflight = client.load_preflight_summary(intent).ok();

    if intent.mode == TradingActionMode::Review {
        return Ok(ExchangeApiExecutionResult {
            detail: format!(
                "Matchbook review ready: {} {} @ {:.2} stake {:.2} runner {}{}",
                matchbook_side_label(intent.side),
                intent.selection_name,
                intent.expected_price,
                intent.stake,
                runner_id,
                preflight
                    .as_ref()
                    .map(MatchbookPreflightSummary::detail_suffix)
                    .unwrap_or_default(),
            ),
        });
    }

    let value = client.submit_offer(intent)?;
    let offer_ref = first_non_empty_string(&value, &["/offers/0/id", "/offers/0/offer-id"])
        .unwrap_or_else(|| String::from("submitted"));
    let offer_status = first_non_empty_string(&value, &["/offers/0/status", "/offers/0/result"])
        .unwrap_or_else(|| String::from("submitted"));

    Ok(ExchangeApiExecutionResult {
        detail: format!(
            "Matchbook {} {} @ {:.2} stake {:.2} offer {}{}",
            offer_status,
            intent.selection_name,
            intent.expected_price,
            intent.stake,
            offer_ref,
            preflight
                .as_ref()
                .map(MatchbookPreflightSummary::detail_suffix)
                .unwrap_or_default(),
        ),
    })
}

fn scaffolded_exchange(label: &str, detail: &str) -> Result<ExchangeApiExecutionResult> {
    Ok(ExchangeApiExecutionResult {
        detail: format!("{label}: {detail}"),
    })
}

fn matchbook_offer_payload(intent: &TradingActionIntent, runner_id: &str) -> Value {
    json!({
        "odds-type": env::var("MATCHBOOK_ODDS_TYPE").ok().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| String::from("DECIMAL")),
        "exchange-type": env::var("MATCHBOOK_EXCHANGE_TYPE").ok().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| String::from("back-lay")),
        "offers": [{
            "runner-id": runner_id,
            "side": matchbook_side_label(intent.side),
            "odds": intent.expected_price,
            "stake": intent.stake,
            "keep-in-play": false
        }]
    })
}

fn matchbook_session_token(client: &Client, base_url: &str) -> Result<String> {
    if let Some(token) = env::var("MATCHBOOK_SESSION_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(token);
    }

    let username = required_env("MATCHBOOK_USERNAME")?;
    let password = required_env("MATCHBOOK_PASSWORD")?;
    let response = client
        .post(format!("{base_url}/bpapi/rest/security/session"))
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&json!({
            "username": username,
            "password": password,
        }))
        .send()
        .wrap_err("failed to create Matchbook session")?;
    let status = response.status();
    let body = response
        .text()
        .wrap_err("failed to read Matchbook session response body")?;
    if !status.is_success() {
        return Err(eyre!(
            "Matchbook session login failed with {}: {}",
            status,
            truncate(&body, 220)
        ));
    }
    let value: Value =
        serde_json::from_str(&body).wrap_err("failed to decode Matchbook session response")?;
    first_non_empty_string(
        &value,
        &[
            "/session-token",
            "/session_token",
            "/token",
            "/data/session-token",
            "/data/session_token",
        ],
    )
    .ok_or_else(|| eyre!("Matchbook session response did not include a session token"))
}

fn matchbook_runner_id(intent: &TradingActionIntent) -> Result<String> {
    intent
        .betslip_selection_id
        .clone()
        .or_else(|| note_value(&intent.notes, "runner_id"))
        .or_else(|| note_value(&intent.notes, "selection_id"))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| eyre!("Matchbook execution requires a runner/selection id"))
}

fn matchbook_preflight_summary_from_values(
    runner_id: &str,
    balance: Option<&Value>,
    current_offers: Option<&Value>,
    current_bets: Option<&Value>,
    matched_bets: Option<&Value>,
    positions: Option<&Value>,
) -> MatchbookPreflightSummary {
    let offer_rows = rows_from_value(
        current_offers,
        &[
            "/offers",
            "/current-offers",
            "/data/offers",
            "/data/current-offers",
        ],
    );
    let bet_rows = rows_from_value(
        current_bets,
        &["/bets", "/current-bets", "/data/bets", "/data/current-bets"],
    );
    let matched_rows = rows_from_value(
        matched_bets,
        &[
            "/matched-bets",
            "/aggregated-matched-bets",
            "/data/matched-bets",
            "/data/aggregated-matched-bets",
        ],
    );
    let position_rows = rows_from_value(positions, &["/positions", "/data/positions"]);

    MatchbookPreflightSummary {
        balance: balance.and_then(matchbook_balance_summary_from_value),
        open_offer_count: offer_rows.len(),
        current_bet_count: bet_rows.len(),
        matched_bet_count: matched_rows.len(),
        position_count: position_rows.len(),
        runner_offer_count: count_runner_rows(&offer_rows, runner_id),
        runner_bet_count: count_runner_rows(&bet_rows, runner_id),
        runner_position_count: count_runner_rows(&position_rows, runner_id),
        runner_open_stake: sum_runner_numeric(
            &offer_rows,
            runner_id,
            &[
                "/remaining-stake",
                "/remaining_stake",
                "/unmatched-stake",
                "/unmatched_stake",
                "/stake",
            ],
        ),
    }
}

fn matchbook_balance_summary_from_value(value: &Value) -> Option<String> {
    let amount = first_non_empty_string(
        value,
        &[
            "/available-balance",
            "/balance/available",
            "/balances/available",
            "/data/available-balance",
        ],
    )?;
    let currency =
        first_non_empty_string(value, &["/currency", "/balance/currency", "/data/currency"])
            .unwrap_or_else(|| String::from("GBP"));
    Some(format!("balance {amount} {currency}"))
}

fn matchbook_account_label_from_value(value: &Value) -> Option<String> {
    first_non_empty_string(
        value,
        &[
            "/username",
            "/user-name",
            "/display-name",
            "/display_name",
            "/email",
            "/account-name",
            "/account_name",
        ],
    )
}

fn rows_from_value<'a>(value: Option<&'a Value>, paths: &[&str]) -> Vec<&'a Value> {
    let Some(value) = value else {
        return Vec::new();
    };
    for path in paths {
        if let Some(items) = value.pointer(path).and_then(Value::as_array) {
            return items.iter().collect();
        }
    }
    Vec::new()
}

fn parse_matchbook_offer_rows(value: Option<&Value>) -> Vec<MatchbookOfferRow> {
    rows_from_value(
        value,
        &[
            "/offers",
            "/current-offers",
            "/data/offers",
            "/data/current-offers",
        ],
    )
    .into_iter()
    .map(|row| MatchbookOfferRow {
        offer_id: first_non_empty_string(row, &["/id", "/offer-id", "/offer_id"])
            .unwrap_or_else(|| String::from("-")),
        runner_id: first_non_empty_string(
            row,
            &["/runner-id", "/runner_id", "/selection-id", "/selection_id"],
        )
        .unwrap_or_default(),
        event_name: first_non_empty_string(
            row,
            &["/event-name", "/event/name", "/event_name", "/name"],
        )
        .unwrap_or_else(|| String::from("-")),
        market_name: first_non_empty_string(row, &["/market-name", "/market/name", "/market_name"])
            .unwrap_or_else(|| String::from("-")),
        selection_name: first_non_empty_string(
            row,
            &[
                "/runner-name",
                "/runner_name",
                "/selection-name",
                "/selection_name",
            ],
        )
        .unwrap_or_else(|| String::from("-")),
        side: first_non_empty_string(row, &["/side"]).unwrap_or_else(|| String::from("-")),
        status: first_non_empty_string(row, &["/status"]).unwrap_or_else(|| String::from("-")),
        odds: first_numeric(row, &["/odds", "/price"]),
        stake: first_numeric(row, &["/stake"]),
        remaining_stake: first_numeric(
            row,
            &[
                "/remaining-stake",
                "/remaining_stake",
                "/unmatched-stake",
                "/unmatched_stake",
            ],
        ),
    })
    .collect()
}

fn parse_matchbook_bet_rows(value: Option<&Value>) -> Vec<MatchbookBetRow> {
    rows_from_value(
        value,
        &["/bets", "/current-bets", "/data/bets", "/data/current-bets"],
    )
    .into_iter()
    .map(|row| MatchbookBetRow {
        bet_id: first_non_empty_string(row, &["/id", "/bet-id", "/bet_id"])
            .unwrap_or_else(|| String::from("-")),
        runner_id: first_non_empty_string(
            row,
            &["/runner-id", "/runner_id", "/selection-id", "/selection_id"],
        )
        .unwrap_or_default(),
        event_name: first_non_empty_string(
            row,
            &["/event-name", "/event/name", "/event_name", "/name"],
        )
        .unwrap_or_else(|| String::from("-")),
        market_name: first_non_empty_string(row, &["/market-name", "/market/name", "/market_name"])
            .unwrap_or_else(|| String::from("-")),
        selection_name: first_non_empty_string(
            row,
            &[
                "/runner-name",
                "/runner_name",
                "/selection-name",
                "/selection_name",
            ],
        )
        .unwrap_or_else(|| String::from("-")),
        side: first_non_empty_string(row, &["/side"]).unwrap_or_else(|| String::from("-")),
        status: first_non_empty_string(row, &["/status"]).unwrap_or_else(|| String::from("-")),
        odds: first_numeric(row, &["/odds", "/price"]),
        stake: first_numeric(row, &["/stake"]),
        profit_loss: first_numeric(
            row,
            &[
                "/profit-loss",
                "/profit_loss",
                "/net-profit-loss",
                "/net_profit_loss",
            ],
        ),
    })
    .collect()
}

fn parse_matchbook_position_rows(value: Option<&Value>) -> Vec<MatchbookPositionRow> {
    rows_from_value(value, &["/positions", "/data/positions"])
        .into_iter()
        .map(|row| MatchbookPositionRow {
            runner_id: first_non_empty_string(
                row,
                &["/runner-id", "/runner_id", "/selection-id", "/selection_id"],
            )
            .unwrap_or_default(),
            event_name: first_non_empty_string(
                row,
                &["/event-name", "/event/name", "/event_name", "/name"],
            )
            .unwrap_or_else(|| String::from("-")),
            market_name: first_non_empty_string(
                row,
                &["/market-name", "/market/name", "/market_name"],
            )
            .unwrap_or_else(|| String::from("-")),
            selection_name: first_non_empty_string(
                row,
                &[
                    "/runner-name",
                    "/runner_name",
                    "/selection-name",
                    "/selection_name",
                ],
            )
            .unwrap_or_else(|| String::from("-")),
            exposure: first_numeric(
                row,
                &["/exposure", "/net-exposure", "/net_exposure", "/stake"],
            ),
            profit_loss: first_numeric(
                row,
                &[
                    "/profit-loss",
                    "/profit_loss",
                    "/net-profit-loss",
                    "/net_profit_loss",
                ],
            ),
        })
        .collect()
}

fn count_runner_rows(rows: &[&Value], runner_id: &str) -> usize {
    rows.iter()
        .filter(|row| row_matches_runner_id(row, runner_id))
        .count()
}

fn sum_runner_numeric(rows: &[&Value], runner_id: &str, paths: &[&str]) -> Option<f64> {
    let mut matched = false;
    let mut sum = 0.0;
    for row in rows {
        if row_matches_runner_id(row, runner_id) {
            if let Some(value) = first_numeric(row, paths) {
                matched = true;
                sum += value;
            }
        }
    }
    matched.then_some(sum)
}

fn row_matches_runner_id(value: &Value, runner_id: &str) -> bool {
    first_non_empty_string(
        value,
        &[
            "/runner-id",
            "/runner_id",
            "/selection-id",
            "/selection_id",
            "/id",
        ],
    )
    .map(|value| value == runner_id)
    .unwrap_or(false)
}

fn note_value(notes: &[String], prefix: &str) -> Option<String> {
    notes.iter().find_map(|note| {
        note.strip_prefix(&format!("{prefix}:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn matchbook_side_label(side: TradingActionSide) -> &'static str {
    match side {
        TradingActionSide::Buy => "back",
        TradingActionSide::Sell => "lay",
    }
}

fn required_env(name: &str) -> Result<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| eyre!("missing {name}"))
}

fn first_non_empty_string(value: &Value, paths: &[&str]) -> Option<String> {
    for path in paths {
        let current = value.pointer(path)?;
        let as_text = match current {
            Value::String(item) => item.trim().to_string(),
            Value::Number(item) => item.to_string(),
            Value::Bool(item) => item.to_string(),
            _ => continue,
        };
        if !as_text.is_empty() {
            return Some(as_text);
        }
    }
    None
}

fn first_numeric(value: &Value, paths: &[&str]) -> Option<f64> {
    for path in paths {
        let current = value.pointer(path)?;
        let parsed = match current {
            Value::Number(item) => item.as_f64(),
            Value::String(item) => item.trim().parse::<f64>().ok(),
            _ => None,
        };
        if parsed.is_some() {
            return parsed;
        }
    }
    None
}

fn runner_count_suffix(count: usize) -> String {
    if count == 0 {
        String::new()
    } else {
        format!(" (runner {count})")
    }
}

fn format_amount(value: f64) -> String {
    format!("{value:.2}")
}

fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        value.to_string()
    } else {
        format!("{}...", &value[..limit.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        matchbook_offer_payload, matchbook_preflight_summary_from_values, matchbook_runner_id,
        matchbook_side_label,
    };
    use crate::domain::VenueId;
    use crate::trading_actions::{
        TradingActionIntent, TradingActionKind, TradingActionMode, TradingActionSeed,
        TradingExecutionPolicy, TradingRiskReport, TradingTimeInForce,
    };

    #[test]
    fn matchbook_payload_uses_lay_side_for_sell_intent() {
        let intent = sample_intent();
        let payload = matchbook_offer_payload(&intent, "12345");
        assert_eq!(
            payload
                .pointer("/offers/0/runner-id")
                .and_then(|value| value.as_str()),
            Some("12345")
        );
        assert_eq!(
            payload
                .pointer("/offers/0/side")
                .and_then(|value| value.as_str()),
            Some("lay")
        );
    }

    #[test]
    fn matchbook_runner_id_uses_betslip_selection_id_first() {
        let intent = sample_intent();
        assert_eq!(matchbook_runner_id(&intent).expect("runner id"), "runner-7");
    }

    #[test]
    fn matchbook_side_label_maps_buy_to_back() {
        assert_eq!(
            matchbook_side_label(crate::trading_actions::TradingActionSide::Buy),
            "back"
        );
    }

    #[test]
    fn matchbook_preflight_summary_counts_account_state() {
        let balance = json!({
            "available-balance": 128.42,
            "currency": "GBP"
        });
        let current_offers = json!({
            "offers": [
                {"runner-id": "runner-7", "remaining-stake": 12.0},
                {"runner-id": "runner-9", "remaining-stake": 4.0}
            ]
        });
        let current_bets = json!({
            "bets": [
                {"runner-id": "runner-7"},
                {"runner-id": "runner-7"},
                {"runner-id": "runner-1"}
            ]
        });
        let matched_bets = json!({
            "matched-bets": [
                {"runner-id": "runner-7"},
                {"runner-id": "runner-9"}
            ]
        });
        let positions = json!({
            "positions": [
                {"runner-id": "runner-7"},
                {"runner-id": "runner-4"}
            ]
        });

        let summary = matchbook_preflight_summary_from_values(
            "runner-7",
            Some(&balance),
            Some(&current_offers),
            Some(&current_bets),
            Some(&matched_bets),
            Some(&positions),
        );

        assert_eq!(summary.balance.as_deref(), Some("balance 128.42 GBP"));
        assert_eq!(summary.open_offer_count, 2);
        assert_eq!(summary.current_bet_count, 3);
        assert_eq!(summary.matched_bet_count, 2);
        assert_eq!(summary.position_count, 2);
        assert_eq!(summary.runner_offer_count, 1);
        assert_eq!(summary.runner_bet_count, 2);
        assert_eq!(summary.runner_position_count, 1);
        assert_eq!(summary.runner_open_stake, Some(12.0));
        assert!(summary.detail_suffix().contains("offers 2 (runner 1)"));
        assert!(summary.detail_suffix().contains("runner stake 12.00"));
    }

    fn sample_intent() -> TradingActionIntent {
        let seed = TradingActionSeed {
            source: crate::trading_actions::TradingActionSource::OddsMatcher,
            venue: VenueId::Matchbook,
            source_ref: String::from("row-1"),
            event_name: String::from("Arsenal v Everton"),
            market_name: String::from("Match Odds"),
            selection_name: String::from("Arsenal"),
            event_url: Some(String::from("https://matchbook.example/event")),
            deep_link_url: Some(String::from("https://matchbook.example/bet")),
            betslip_market_id: Some(String::from("market-1")),
            betslip_selection_id: Some(String::from("runner-7")),
            buy_price: Some(2.14),
            sell_price: Some(2.16),
            default_side: crate::trading_actions::TradingActionSide::Sell,
            default_stake: Some(10.0),
            source_context: Default::default(),
            notes: vec![String::from("runner_id:runner-7")],
        };
        seed.build_intent(
            &Default::default(),
            String::from("req-1"),
            crate::trading_actions::TradingActionSide::Sell,
            TradingActionMode::Confirm,
            10.0,
            TradingTimeInForce::GoodTilCancel,
        )
        .unwrap_or_else(|_| TradingActionIntent {
            action_kind: TradingActionKind::PlaceBet,
            source: crate::trading_actions::TradingActionSource::OddsMatcher,
            venue: VenueId::Matchbook,
            mode: TradingActionMode::Confirm,
            side: crate::trading_actions::TradingActionSide::Sell,
            request_id: String::from("req-1"),
            source_ref: String::from("row-1"),
            event_name: String::from("Arsenal v Everton"),
            market_name: String::from("Match Odds"),
            selection_name: String::from("Arsenal"),
            stake: 10.0,
            expected_price: 2.16,
            event_url: Some(String::from("https://matchbook.example/event")),
            deep_link_url: Some(String::from("https://matchbook.example/bet")),
            betslip_market_id: Some(String::from("market-1")),
            betslip_selection_id: Some(String::from("runner-7")),
            execution_policy: TradingExecutionPolicy::new(TradingTimeInForce::GoodTilCancel),
            risk_report: TradingRiskReport::default(),
            source_context: Default::default(),
            notes: vec![String::from("runner_id:runner-7")],
        })
    }
}
