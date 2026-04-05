use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result, WrapErr};
use reqwest::Client as AsyncClient;

use crate::exchange_api::MatchbookAccountState;

const SABISABI_BASE_URL_ENV: &str = "SABISABI_BASE_URL";
const SABISABI_CONTROL_TOKEN_ENV: &str = "SABISABI_CONTROL_TOKEN";
const DEFAULT_SABISABI_BASE_URL: &str = "http://127.0.0.1:4080";

pub async fn load_account_state(force_refresh: bool) -> Result<MatchbookAccountState> {
    load_account_state_from_base_url(
        &sabisabi_base_url(),
        force_refresh,
        sabisabi_control_token(),
    )
    .await
}

async fn load_account_state_from_base_url(
    base_url: &str,
    force_refresh: bool,
    control_token: Option<String>,
) -> Result<MatchbookAccountState> {
    let client = AsyncClient::builder()
        .connect_timeout(Duration::from_millis(750))
        .timeout(Duration::from_secs(8))
        .build()
        .wrap_err("failed to build backend Matchbook client")?;

    let path = if force_refresh {
        "/api/v1/control/operator/matchbook/account/refresh"
    } else {
        "/api/v1/query/operator/matchbook/account"
    };
    let mut request = if force_refresh {
        client.post(format!("{}{}", base_url.trim_end_matches('/'), path))
    } else {
        client.get(format!("{}{}", base_url.trim_end_matches('/'), path))
    };
    if force_refresh {
        if let Some(token) = control_token {
            request = request.bearer_auth(token);
        }
    }
    let response = request
        .send()
        .await
        .wrap_err_with(|| format!("request failed for {path}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(eyre!(
            "HTTP {} during Matchbook backend sync: {}",
            status.as_u16(),
            truncate(&body, 200)
        ));
    }
    serde_json::from_str::<MatchbookAccountState>(&body)
        .wrap_err("failed to decode Matchbook backend account state")
}

fn sabisabi_base_url() -> String {
    env::var(SABISABI_BASE_URL_ENV).unwrap_or_else(|_| String::from(DEFAULT_SABISABI_BASE_URL))
}

fn sabisabi_control_token() -> Option<String> {
    env::var(SABISABI_CONTROL_TOKEN_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| dotenv_value_from_paths(SABISABI_CONTROL_TOKEN_ENV, &dotenv_candidates()))
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

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::load_account_state_from_base_url;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn backend_matchbook_query_decodes_account_state() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        let server = format!("http://{}", address);
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).expect("read request");
            let request = String::from_utf8_lossy(&buffer[..read]);
            assert!(request.starts_with("GET /api/v1/query/operator/matchbook/account "));
            let body = serde_json::json!({
                "status_line": "ready",
                "balance_label": "balance 12.00 GBP",
                "summary": {
                    "balance": "balance 12.00 GBP",
                    "open_offer_count": 0,
                    "current_bet_count": 0,
                    "matched_bet_count": 0,
                    "position_count": 0,
                    "runner_offer_count": 0,
                    "runner_bet_count": 0,
                    "runner_position_count": 0,
                    "runner_open_stake": null,
                    "runner_best_back_odds": null,
                    "runner_best_back_liquidity": null,
                    "runner_best_lay_odds": null,
                    "runner_best_lay_liquidity": null
                },
                "current_offers": [],
                "current_bets": [],
                "positions": []
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write response");
        });

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let state = runtime
            .block_on(load_account_state_from_base_url(&server, false, None))
            .expect("backend state");
        assert_eq!(state.status_line, "ready");
    }
}
