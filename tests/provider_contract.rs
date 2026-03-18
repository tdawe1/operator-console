use operator_console::domain::{ExchangePanelSnapshot, WatchSnapshot};

#[test]
fn watch_snapshot_deserializes_grouped_rows() {
    let snapshot: WatchSnapshot = serde_json::from_str(
        r#"{
          "position_count": 3,
          "watch_count": 2,
          "commission_rate": 0.0,
          "target_profit": 1.0,
          "stop_loss": 1.0,
          "watches": [
            {
              "contract": "1 - 1",
              "market": "Correct score",
              "position_count": 2,
              "can_trade_out": true,
              "total_stake": 2.96,
              "total_liability": 18.34,
              "current_pnl_amount": -0.18,
              "current_back_odds": 10.87,
              "average_entry_lay_odds": 7.2,
              "entry_implied_probability": 0.1389,
              "profit_take_back_odds": 10.87,
              "profit_take_implied_probability": 0.092,
              "stop_loss_back_odds": 5.38,
              "stop_loss_implied_probability": 0.1859
            }
          ]
        }"#,
    )
    .expect("snapshot should parse");

    assert_eq!(snapshot.watch_count, 2);
    assert_eq!(snapshot.watches[0].contract, "1 - 1");
    assert_eq!(snapshot.watches[0].market, "Correct score");
    assert_eq!(snapshot.watches[0].current_back_odds, Some(10.87));
}

#[test]
fn exchange_panel_snapshot_deserializes_account_positions_and_other_bets() {
    let snapshot: ExchangePanelSnapshot = serde_json::from_str(
        r#"{
          "worker": {
            "name": "bet-recorder",
            "status": "ready",
            "detail": "Loaded richer snapshot"
          },
          "venues": [
            {
              "id": "smarkets",
              "label": "Smarkets",
              "status": "ready",
              "detail": "Richer snapshot loaded",
              "event_count": 3,
              "market_count": 2
            }
          ],
          "selected_venue": "smarkets",
          "events": [],
          "markets": [],
          "preflight": null,
          "status_line": "Loaded richer snapshot",
          "runtime": {
            "updated_at": "2026-03-11T12:05:00Z",
            "source": "watcher-state",
            "decision_count": 2,
            "watcher_iteration": 7,
            "stale": false
          },
          "account_stats": {
            "available_balance": 120.45,
            "exposure": 41.63,
            "unrealized_pnl": -0.49,
            "currency": "GBP"
          },
          "open_positions": [
            {
              "event": "West Ham vs Man City",
              "event_status": "27'|Premier League",
              "event_url": "https://smarkets.com/football/england-premier-league/2026/03/14/20-00/west-ham-vs-manchester-city/44919693/",
              "contract": "Draw",
              "market": "Full-time result",
              "price": 3.35,
              "stake": 9.91,
              "liability": 23.29,
              "current_value": 9.60,
              "pnl_amount": -0.31,
              "current_back_odds": 2.80,
              "current_implied_probability": 0.3571428571,
              "current_implied_percentage": 35.71428571,
              "current_score": "0-0",
              "current_score_home": 0,
              "current_score_away": 0,
              "can_trade_out": true
            }
          ],
          "historical_positions": [
            {
              "event": "Aston Villa v Chelsea",
              "event_status": "2026-03-03T14:08:00|Football",
              "event_url": "",
              "contract": "Reece James (Chelsea)",
              "market": "Player To Receive A Card",
              "price": 4.50,
              "stake": 2.00,
              "liability": 2.00,
              "current_value": 0.00,
              "pnl_amount": -2.00,
              "current_back_odds": 4.50,
              "current_implied_probability": 0.2222222222,
              "current_implied_percentage": 22.22222222,
              "can_trade_out": false,
              "status": "settled",
              "market_status": "settled"
            }
          ],
          "other_open_bets": [
            {
              "label": "Arsenal",
              "market": "Full-time result",
              "side": "back",
              "odds": 2.12,
              "stake": 5.00,
              "status": "Open"
            }
          ],
          "decisions": [
            {
              "contract": "Draw",
              "market": "Full-time result",
              "status": "take_profit_ready",
              "reason": "current_back_odds",
              "current_pnl_amount": 2.1,
              "current_back_odds": 4.80,
              "profit_take_back_odds": 4.20,
              "stop_loss_back_odds": 2.80
            }
          ],
          "watch": {
            "position_count": 3,
            "watch_count": 2,
            "commission_rate": 0.0,
            "target_profit": 1.0,
            "stop_loss": 1.0,
            "watches": []
          }
        }"#,
    )
    .expect("snapshot should parse");

    assert_eq!(
        snapshot
            .account_stats
            .expect("account stats")
            .available_balance,
        120.45
    );
    assert_eq!(snapshot.open_positions.len(), 1);
    assert_eq!(snapshot.historical_positions.len(), 1);
    assert_eq!(snapshot.other_open_bets.len(), 1);
    assert_eq!(snapshot.other_open_bets[0].label, "Arsenal");
    assert_eq!(snapshot.decisions.len(), 1);
    assert_eq!(snapshot.decisions[0].status, "take_profit_ready");
    assert_eq!(snapshot.open_positions[0].current_back_odds, Some(2.80));
    assert_eq!(snapshot.open_positions[0].current_score, "0-0");
    assert_eq!(
        snapshot.open_positions[0].event_url,
        "https://smarkets.com/football/england-premier-league/2026/03/14/20-00/west-ham-vs-manchester-city/44919693/"
    );
    assert_eq!(snapshot.runtime.expect("runtime").decision_count, 2);
}

#[test]
fn exchange_panel_snapshot_tolerates_null_legacy_position_strings() {
    let snapshot: ExchangePanelSnapshot = serde_json::from_str(
        r#"{
          "worker": {
            "name": "bet-recorder",
            "status": "ready",
            "detail": "legacy watcher state"
          },
          "venues": [],
          "selected_venue": "smarkets",
          "events": [],
          "markets": [],
          "preflight": null,
          "status_line": "legacy watcher state",
          "runtime": {
            "updated_at": "2026-03-15T09:00:00Z",
            "source": "watcher-state",
            "decision_count": 1,
            "watcher_iteration": 12,
            "stale": false
          },
          "account_stats": null,
          "open_positions": [
            {
              "contract": "Man City",
              "market": "Full-time result",
              "event": null,
              "event_status": null,
              "event_url": null,
              "status": "Order filled",
              "market_status": null,
              "is_in_play": false,
              "price": 1.69,
              "stake": 22.55,
              "liability": 15.56,
              "current_value": 22.55,
              "pnl_amount": 0.0,
              "current_back_odds": null,
              "current_implied_probability": null,
              "current_implied_percentage": null,
              "current_buy_odds": null,
              "current_buy_implied_probability": null,
              "current_sell_odds": null,
              "current_sell_implied_probability": null,
              "current_score": null,
              "current_score_home": null,
              "current_score_away": null,
              "live_clock": null,
              "can_trade_out": false
            }
          ],
          "other_open_bets": [],
          "decisions": [],
          "watch": {
            "position_count": 1,
            "watch_count": 1,
            "commission_rate": 0.0,
            "target_profit": 1.0,
            "stop_loss": 1.0,
            "watches": []
          },
          "tracked_bets": [],
          "exit_policy": {
            "target_profit": 1.0,
            "stop_loss": 1.0,
            "hard_margin_call_profit_floor": null,
            "warn_only_default": true
          },
          "exit_recommendations": []
        }"#,
    )
    .expect("snapshot should parse");

    assert_eq!(snapshot.open_positions[0].event, "");
    assert_eq!(snapshot.open_positions[0].event_status, "");
    assert_eq!(snapshot.open_positions[0].event_url, "");
    assert_eq!(snapshot.open_positions[0].current_score, "");
    assert_eq!(snapshot.open_positions[0].live_clock, "");
}

#[test]
fn exchange_panel_snapshot_deserializes_tracked_bets_and_exit_sections() {
    let snapshot: ExchangePanelSnapshot = serde_json::from_str(
        r#"{
          "worker": {
            "name": "bet-recorder",
            "status": "ready",
            "detail": "Loaded ledger snapshot"
          },
          "venues": [],
          "selected_venue": "smarkets",
          "events": [],
          "markets": [],
          "preflight": null,
          "status_line": "Loaded ledger snapshot",
          "runtime": null,
          "account_stats": null,
          "open_positions": [],
          "other_open_bets": [],
          "decisions": [],
          "watch": null,
          "tracked_bets": [
            {
              "bet_id": "bet-001",
              "group_id": "group-arsenal-everton",
              "platform": "bet365",
              "exchange": "smarkets",
              "sport_key": "soccer_epl",
              "sport_name": "Premier League",
              "bet_type": "single",
              "market_family": "match_odds",
              "back_price": 2.12,
              "lay_price": 3.35,
              "expected_ev": {
                "gbp": 0.42,
                "pct": 0.21,
                "method": "fair_price",
                "source": "local_formula",
                "status": "calculated"
              },
              "event": "Arsenal v Everton",
              "market": "Full-time result",
              "selection": "Draw",
              "status": "open",
              "legs": [
                {
                  "venue": "smarkets",
                  "outcome": "Draw",
                  "side": "lay",
                  "odds": 3.35,
                  "stake": 9.91,
                  "status": "open"
                },
                {
                  "venue": "bet365",
                  "outcome": "Arsenal",
                  "side": "back",
                  "odds": 2.12,
                  "stake": 5.0,
                  "status": "matched"
                }
              ]
            }
          ],
          "exit_policy": {
            "target_profit": 1.0,
            "stop_loss": 1.0,
            "hard_margin_call_profit_floor": null,
            "warn_only_default": true
          },
          "exit_recommendations": [
            {
              "bet_id": "bet-001",
              "action": "warn",
              "reason": "target not reached",
              "worst_case_pnl": 0.82,
              "cash_out_venue": "smarkets"
            }
          ]
        }"#,
    )
    .expect("snapshot should parse");

    assert_eq!(snapshot.tracked_bets.len(), 1);
    assert_eq!(snapshot.tracked_bets[0].bet_id, "bet-001");
    assert_eq!(snapshot.tracked_bets[0].platform, "bet365");
    assert_eq!(
        snapshot.tracked_bets[0].exchange.as_deref(),
        Some("smarkets")
    );
    assert_eq!(snapshot.tracked_bets[0].expected_ev.gbp, Some(0.42));
    assert_eq!(snapshot.tracked_bets[0].legs.len(), 2);
    assert!(snapshot.exit_policy.warn_only_default);
    assert_eq!(snapshot.exit_recommendations.len(), 1);
    assert_eq!(snapshot.exit_recommendations[0].action, "warn");
}
