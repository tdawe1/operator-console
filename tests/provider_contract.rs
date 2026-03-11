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
          "account_stats": {
            "available_balance": 120.45,
            "exposure": 41.63,
            "unrealized_pnl": -0.49,
            "currency": "GBP"
          },
          "open_positions": [
            {
              "contract": "Draw",
              "market": "Full-time result",
              "price": 3.35,
              "stake": 9.91,
              "liability": 23.29,
              "current_value": 9.60,
              "pnl_amount": -0.31,
              "can_trade_out": true
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
    assert_eq!(snapshot.other_open_bets.len(), 1);
    assert_eq!(snapshot.other_open_bets[0].label, "Arsenal");
}
