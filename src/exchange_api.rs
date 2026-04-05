use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use color_eyre::eyre::{eyre, Result, WrapErr};
use reqwest::blocking::Client;
use reqwest::Client as AsyncClient;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::VenueId;
use crate::trading_actions::{TradingActionIntent, TradingActionMode, TradingActionSide};

const MATCHBOOK_BASE_URL: &str = "https://api.matchbook.com";
const MATCHBOOK_TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct ExchangeApiExecutionResult {
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MatchbookOfferRow {
    pub offer_id: String,
    pub event_id: String,
    pub market_id: String,
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

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MatchbookBetRow {
    pub bet_id: String,
    pub event_id: String,
    pub market_id: String,
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

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MatchbookPositionRow {
    pub event_id: String,
    pub market_id: String,
    pub runner_id: String,
    pub event_name: String,
    pub market_name: String,
    pub selection_name: String,
    pub exposure: Option<f64>,
    pub profit_loss: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
pub struct AsyncMatchbookApiClient {
    client: AsyncClient,
    base_url: String,
    token: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
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
    pub runner_best_back_odds: Option<f64>,
    pub runner_best_back_liquidity: Option<f64>,
    pub runner_best_lay_odds: Option<f64>,
    pub runner_best_lay_liquidity: Option<f64>,
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
        if self.runner_best_back_odds.is_some() || self.runner_best_lay_odds.is_some() {
            segments.push(format!(
                "prices back {} • lay {}",
                format_price_liquidity(self.runner_best_back_odds, self.runner_best_back_liquidity),
                format_price_liquidity(self.runner_best_lay_odds, self.runner_best_lay_liquidity)
            ));
        }
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
        let base_url = env_or_dotenv("MATCHBOOK_API_BASE_URL")
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

        let mut summary = matchbook_preflight_summary_from_values(
            &runner_id,
            balance.as_ref(),
            current_offers.as_ref(),
            current_bets.as_ref(),
            matched_bets.as_ref(),
            positions.as_ref(),
        );
        if let Some((event_id, market_id)) = matchbook_price_lookup_ids(intent) {
            if let Ok(prices) = self.runner_prices(&event_id, &market_id, &runner_id) {
                let (back, lay) = matchbook_best_prices_from_value(&prices);
                summary.runner_best_back_odds = back.0;
                summary.runner_best_back_liquidity = back.1;
                summary.runner_best_lay_odds = lay.0;
                summary.runner_best_lay_liquidity = lay.1;
            }
        }
        Ok(summary)
    }

    pub fn runner_prices(&self, event_id: &str, market_id: &str, runner_id: &str) -> Result<Value> {
        self.get_json(
            &format!("/edge/rest/events/{event_id}/markets/{market_id}/runners/{runner_id}/prices"),
            "Matchbook runner prices",
        )
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

impl AsyncMatchbookApiClient {
    pub async fn new() -> Result<Self> {
        let client = AsyncClient::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(MATCHBOOK_TIMEOUT_SECS))
            .build()
            .wrap_err("failed to build Matchbook HTTP client")?;
        let base_url = env_or_dotenv("MATCHBOOK_API_BASE_URL")
            .unwrap_or_else(|| String::from(MATCHBOOK_BASE_URL));
        let token = matchbook_session_token_async(&client, &base_url).await?;
        Ok(Self {
            client,
            base_url,
            token,
        })
    }

    pub async fn account(&self) -> Result<Value> {
        self.get_json("/edge/rest/account", "Matchbook account")
            .await
    }

    pub async fn balance(&self) -> Result<Value> {
        self.get_json("/edge/rest/account/balance", "Matchbook balance")
            .await
    }

    pub async fn positions(&self) -> Result<Value> {
        self.get_json("/edge/rest/account/positions", "Matchbook positions")
            .await
    }

    pub async fn current_offers(&self) -> Result<Value> {
        self.get_json(
            "/edge/rest/reports/v2/offers/current",
            "Matchbook current offers",
        )
        .await
    }

    pub async fn current_bets(&self) -> Result<Value> {
        self.get_json(
            "/edge/rest/reports/v2/bets/current",
            "Matchbook current bets",
        )
        .await
    }

    async fn get_json(&self, path: &str, label: &str) -> Result<Value> {
        self.send_json(Method::GET, path, None, label).await
    }

    async fn send_json(
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
            .await
            .wrap_err_with(|| format!("failed to {label}"))?;
        let status = response.status();
        let response_body = response
            .text()
            .await
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
    load_matchbook_account_state_with_client(&client)
}

pub fn load_matchbook_account_state_with_client(
    client: &MatchbookApiClient,
) -> Result<MatchbookAccountState> {
    let account = client.account().wrap_err("Matchbook account failed")?;
    let balance = client.balance().wrap_err("Matchbook balance failed")?;
    let current_offers = client
        .current_offers()
        .wrap_err("Matchbook current offers failed")?;
    let current_bets = client
        .current_bets()
        .wrap_err("Matchbook current bets failed")?;
    let positions = client.positions().wrap_err("Matchbook positions failed")?;

    let summary = matchbook_preflight_summary_from_values(
        "",
        Some(&balance),
        Some(&current_offers),
        Some(&current_bets),
        None,
        Some(&positions),
    );
    let account_label = matchbook_account_label_from_value(&account)
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
        current_offers: parse_matchbook_offer_rows(Some(&current_offers)),
        current_bets: parse_matchbook_bet_rows(Some(&current_bets)),
        positions: parse_matchbook_position_rows(Some(&positions)),
    })
}

pub async fn load_matchbook_account_state_with_async_client(
    client: &AsyncMatchbookApiClient,
) -> Result<MatchbookAccountState> {
    let account = client
        .account()
        .await
        .wrap_err("Matchbook account failed")?;
    let balance = client
        .balance()
        .await
        .wrap_err("Matchbook balance failed")?;
    let current_offers = client
        .current_offers()
        .await
        .wrap_err("Matchbook current offers failed")?;
    let current_bets = client
        .current_bets()
        .await
        .wrap_err("Matchbook current bets failed")?;
    let positions = client
        .positions()
        .await
        .wrap_err("Matchbook positions failed")?;

    let summary = matchbook_preflight_summary_from_values(
        "",
        Some(&balance),
        Some(&current_offers),
        Some(&current_bets),
        None,
        Some(&positions),
    );
    let account_label = matchbook_account_label_from_value(&account)
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
        current_offers: parse_matchbook_offer_rows(Some(&current_offers)),
        current_bets: parse_matchbook_bet_rows(Some(&current_bets)),
        positions: parse_matchbook_position_rows(Some(&positions)),
    })
}

pub async fn run_matchbook_sync_job_async(
    client: &mut Option<AsyncMatchbookApiClient>,
    rate_limited_until: &mut Option<Instant>,
) -> Result<MatchbookAccountState> {
    if let Some(until) = rate_limited_until {
        if Instant::now() < *until {
            let remaining_secs = until.saturating_duration_since(Instant::now()).as_secs();
            return Err(eyre!(
                "Matchbook API remains rate limited; retry after {}s",
                remaining_secs
            ));
        }
        *rate_limited_until = None;
    }

    if client.is_none() {
        *client = Some(AsyncMatchbookApiClient::new().await?);
    }

    match load_matchbook_account_state_with_async_client(
        client.as_ref().expect("client should be initialized"),
    )
    .await
    {
        Ok(state) => Ok(state),
        Err(error) if matchbook_error_has_status(&error, 401) => {
            *client = Some(AsyncMatchbookApiClient::new().await?);
            load_matchbook_account_state_with_async_client(
                client.as_ref().expect("client refreshed"),
            )
            .await
        }
        Err(error) if matchbook_error_has_status(&error, 429) => {
            *client = None;
            *rate_limited_until = Some(Instant::now() + Duration::from_secs(10 * 60));
            Err(error)
        }
        Err(error) => Err(error),
    }
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
        "odds-type": env_or_dotenv("MATCHBOOK_ODDS_TYPE").unwrap_or_else(|| String::from("DECIMAL")),
        "exchange-type": env_or_dotenv("MATCHBOOK_EXCHANGE_TYPE").unwrap_or_else(|| String::from("back-lay")),
        "offers": [{
            "runner-id": runner_id,
            "side": matchbook_side_label(intent.side),
            "odds": intent.expected_price,
            "stake": intent.stake,
            "keep-in-play": false
        }]
    })
}

fn matchbook_error_has_status(error: &color_eyre::Report, status_code: u16) -> bool {
    error.to_string().contains(&format!(" {status_code}:"))
}

fn matchbook_session_token(client: &Client, base_url: &str) -> Result<String> {
    if let Some(token) = env_or_dotenv("MATCHBOOK_SESSION_TOKEN") {
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

async fn matchbook_session_token_async(client: &AsyncClient, base_url: &str) -> Result<String> {
    if let Some(token) = env_or_dotenv("MATCHBOOK_SESSION_TOKEN") {
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
        .await
        .wrap_err("failed to create Matchbook session")?;
    let status = response.status();
    let body = response
        .text()
        .await
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
        runner_best_back_odds: None,
        runner_best_back_liquidity: None,
        runner_best_lay_odds: None,
        runner_best_lay_liquidity: None,
    }
}

fn matchbook_price_lookup_ids(intent: &TradingActionIntent) -> Option<(String, String)> {
    let event_id = intent
        .betslip_event_id
        .clone()
        .or_else(|| note_value(&intent.notes, "event_id"))
        .or_else(|| matchbook_event_id_from_url(intent.deep_link_url.as_deref()))
        .filter(|value| !value.trim().is_empty())?;
    let market_id = intent
        .betslip_market_id
        .clone()
        .or_else(|| note_value(&intent.notes, "market_id"))
        .filter(|value| !value.trim().is_empty())?;
    Some((event_id, market_id))
}

fn matchbook_event_id_from_url(url: Option<&str>) -> Option<String> {
    let url = url?.trim();
    if url.is_empty() {
        return None;
    }
    let marker = "/events/";
    let start = url.find(marker)? + marker.len();
    let tail = &url[start..];
    let event_id = tail
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '-')
        .collect::<String>();
    (!event_id.is_empty()).then_some(event_id)
}

fn matchbook_best_prices_from_value(
    value: &Value,
) -> ((Option<f64>, Option<f64>), (Option<f64>, Option<f64>)) {
    let rows = rows_from_value(
        Some(value),
        &[
            "/prices",
            "/data/prices",
            "/data/runner/prices",
            "/runner/prices",
        ],
    );
    let mut best_back = (None, None);
    let mut best_lay = (None, None);
    for row in rows {
        let side = first_non_empty_string(row, &["/side", "/type"]).unwrap_or_default();
        let odds = first_numeric(row, &["/decimal-odds", "/decimal_odds", "/odds", "/price"]);
        let liquidity = first_numeric(
            row,
            &[
                "/available-amount",
                "/available_amount",
                "/available",
                "/amount",
                "/stake",
            ],
        );
        match side.to_ascii_lowercase().as_str() {
            "back" => {
                if odds > best_back.0 {
                    best_back = (odds, liquidity);
                }
            }
            "lay" => {
                if best_lay.0.is_none() || odds < best_lay.0 {
                    best_lay = (odds, liquidity);
                }
            }
            _ => {}
        }
    }
    (best_back, best_lay)
}

fn format_price_liquidity(price: Option<f64>, liquidity: Option<f64>) -> String {
    match (price, liquidity) {
        (Some(price), Some(liquidity)) => format!("{price:.2}/{liquidity:.2}"),
        (Some(price), None) => format!("{price:.2}"),
        _ => String::from("-"),
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
        event_id: first_non_empty_string(row, &["/event-id", "/event_id", "/event/id"])
            .unwrap_or_default(),
        market_id: first_non_empty_string(row, &["/market-id", "/market_id", "/market/id"])
            .unwrap_or_default(),
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
        event_id: first_non_empty_string(row, &["/event-id", "/event_id", "/event/id"])
            .unwrap_or_default(),
        market_id: first_non_empty_string(row, &["/market-id", "/market_id", "/market/id"])
            .unwrap_or_default(),
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
            event_id: first_non_empty_string(row, &["/event-id", "/event_id", "/event/id"])
                .unwrap_or_default(),
            market_id: first_non_empty_string(row, &["/market-id", "/market_id", "/market/id"])
                .unwrap_or_default(),
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
    env_or_dotenv(name).ok_or_else(|| eyre!("missing {name}"))
}

fn env_or_dotenv(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| dotenv_value_from_paths(name, &dotenv_candidates()))
}

fn dotenv_value_from_paths(name: &str, paths: &[PathBuf]) -> Option<String> {
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if key.trim() != name {
                continue;
            }
            let parsed = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !parsed.is_empty() {
                return Some(parsed);
            }
        }
    }
    None
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
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    use reqwest::blocking::Client;
    use serde_json::json;

    use super::{
        dotenv_value_from_paths, load_matchbook_account_state_with_client,
        matchbook_best_prices_from_value, matchbook_event_id_from_url, matchbook_offer_payload,
        matchbook_preflight_summary_from_values, matchbook_runner_id, matchbook_side_label,
        MatchbookApiClient,
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

    #[test]
    fn dotenv_value_reader_supports_home_style_env_files() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let dotenv_path = temp_dir.path().join(".env");
        fs::write(
            &dotenv_path,
            "MATCHBOOK_USERNAME=alice\nMATCHBOOK_PASSWORD=\"secret\"\n",
        )
        .expect("write dotenv");

        assert_eq!(
            dotenv_value_from_paths("MATCHBOOK_USERNAME", &[dotenv_path.clone()]),
            Some(String::from("alice"))
        );
        assert_eq!(
            dotenv_value_from_paths("MATCHBOOK_PASSWORD", &[dotenv_path]),
            Some(String::from("secret"))
        );
    }

    #[test]
    fn matchbook_event_id_reader_uses_event_path_segment() {
        assert_eq!(
            matchbook_event_id_from_url(Some("https://www.matchbook.com/events/12345?foo=bar")),
            Some(String::from("12345"))
        );
        assert_eq!(matchbook_event_id_from_url(Some("")), None);
    }

    #[test]
    fn matchbook_best_prices_parser_prefers_best_back_and_lay() {
        let prices = json!({
            "prices": [
                {"side": "back", "decimal-odds": 3.10, "available-amount": 40.0},
                {"side": "back", "decimal-odds": 3.25, "available-amount": 12.0},
                {"side": "lay", "decimal-odds": 3.40, "available-amount": 28.0},
                {"side": "lay", "decimal-odds": 3.32, "available-amount": 9.0}
            ]
        });

        let (back, lay) = matchbook_best_prices_from_value(&prices);

        assert_eq!(back, (Some(3.25), Some(12.0)));
        assert_eq!(lay, (Some(3.32), Some(9.0)));
    }

    #[test]
    fn matchbook_loader_reports_partial_failure_instead_of_silent_empty_success() {
        let server = spawn_matchbook_test_server(vec![
            http_ok(r#"{"name":"Test Account"}"#),
            http_ok(r#"{"available-balance":128.42,"currency":"GBP"}"#),
            http_error(500, "boom"),
        ]);
        let client = MatchbookApiClient {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(1))
                .timeout(Duration::from_secs(1))
                .build()
                .expect("client"),
            base_url: server,
            token: String::from("test-token"),
        };

        let result = load_matchbook_account_state_with_client(&client);

        assert!(result.is_err());
        let error = result.expect_err("partial failure should error");
        assert!(error.to_string().contains("Matchbook current offers"));
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
            betslip_event_id: Some(String::from("event-1")),
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
            betslip_event_id: Some(String::from("event-1")),
            betslip_market_id: Some(String::from("market-1")),
            betslip_selection_id: Some(String::from("runner-7")),
            execution_policy: TradingExecutionPolicy::new(TradingTimeInForce::GoodTilCancel),
            risk_report: TradingRiskReport::default(),
            source_context: Default::default(),
            notes: vec![String::from("runner_id:runner-7")],
        })
    }

    fn spawn_matchbook_test_server(responses: Vec<String>) -> String {
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

    fn http_error(status: u16, body: &str) -> String {
        format!(
            "HTTP/1.1 {} ERROR\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            status,
            body.len(),
            body
        )
    }
}
