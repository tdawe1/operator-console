use std::env;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result, WrapErr};
use reqwest::blocking::Client;
use serde_json::{json, Value};

use crate::domain::VenueId;
use crate::trading_actions::{TradingActionIntent, TradingActionMode, TradingActionSide};

const MATCHBOOK_BASE_URL: &str = "https://api.matchbook.com";

#[derive(Debug, Clone, PartialEq)]
pub struct ExchangeApiExecutionResult {
    pub detail: String,
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

fn execute_matchbook_trade(intent: &TradingActionIntent) -> Result<ExchangeApiExecutionResult> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .wrap_err("failed to build Matchbook HTTP client")?;
    let base_url = env::var("MATCHBOOK_API_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| String::from(MATCHBOOK_BASE_URL));
    let token = matchbook_session_token(&client, &base_url)?;
    let runner_id = matchbook_runner_id(intent)?;
    let balance = matchbook_balance_summary(&client, &base_url, &token).ok();

    if intent.mode == TradingActionMode::Review {
        return Ok(ExchangeApiExecutionResult {
            detail: format!(
                "Matchbook review ready: {} {} @ {:.2} stake {:.2} runner {}{}",
                matchbook_side_label(intent.side),
                intent.selection_name,
                intent.expected_price,
                intent.stake,
                runner_id,
                balance_suffix(balance.as_deref()),
            ),
        });
    }

    let response = client
        .post(format!("{base_url}/edge/rest/offers"))
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .header("session-token", &token)
        .json(&matchbook_offer_payload(intent, &runner_id))
        .send()
        .wrap_err("failed to submit Matchbook offer")?;
    let status = response.status();
    let body = response
        .text()
        .wrap_err("failed to read Matchbook submit-offer response body")?;
    if !status.is_success() {
        return Err(eyre!(
            "Matchbook offer submission failed with {}: {}",
            status,
            truncate(&body, 220)
        ));
    }
    let value: Value =
        serde_json::from_str(&body).wrap_err("failed to decode Matchbook submit-offer response")?;
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
            balance_suffix(balance.as_deref()),
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

fn matchbook_balance_summary(client: &Client, base_url: &str, token: &str) -> Result<String> {
    let response = client
        .get(format!("{base_url}/edge/rest/account/balance"))
        .header("accept", "application/json")
        .header("session-token", token)
        .send()
        .wrap_err("failed to load Matchbook balance")?;
    let status = response.status();
    let body = response
        .text()
        .wrap_err("failed to read Matchbook balance response body")?;
    if !status.is_success() {
        return Err(eyre!(
            "Matchbook balance request failed with {}: {}",
            status,
            truncate(&body, 180)
        ));
    }
    let value: Value =
        serde_json::from_str(&body).wrap_err("failed to decode Matchbook balance response")?;
    let amount = first_non_empty_string(
        &value,
        &[
            "/available-balance",
            "/balance/available",
            "/balances/available",
            "/data/available-balance",
        ],
    )
    .unwrap_or_else(|| String::from("?"));
    let currency = first_non_empty_string(
        &value,
        &["/currency", "/balance/currency", "/data/currency"],
    )
    .unwrap_or_else(|| String::from("GBP"));
    Ok(format!("balance {amount} {currency}"))
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

fn balance_suffix(balance: Option<&str>) -> String {
    balance
        .map(|balance| format!(" • {balance}"))
        .unwrap_or_default()
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
    use super::{matchbook_offer_payload, matchbook_runner_id, matchbook_side_label};
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
