use operator_console::domain::WatchSnapshot;

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
