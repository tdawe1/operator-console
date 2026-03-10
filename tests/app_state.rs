use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use operator_console::app::App;
use operator_console::domain::{WatchRow, WatchSnapshot};
use operator_console::provider::{WatchProvider, WatchRequest};

#[derive(Clone)]
struct StubProvider {
    snapshots: Rc<RefCell<Vec<WatchSnapshot>>>,
}

impl StubProvider {
    fn new(snapshots: Vec<WatchSnapshot>) -> Self {
        Self {
            snapshots: Rc::new(RefCell::new(snapshots)),
        }
    }
}

impl WatchProvider for StubProvider {
    fn load_watch_snapshot(
        &mut self,
        _request: &WatchRequest,
    ) -> color_eyre::Result<WatchSnapshot> {
        self.snapshots.borrow_mut().remove(0).pipe(Ok)
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[test]
fn app_refresh_replaces_watch_rows() {
    let request = WatchRequest {
        payload_path: PathBuf::from("/tmp/open-positions.json"),
        commission_rate: 0.0,
        target_profit: 1.0,
        stop_loss: 1.0,
    };
    let mut app = App::new(
        request,
        StubProvider::new(vec![
            sample_snapshot("Draw", 1.0),
            sample_snapshot("Arsenal", 2.0),
        ]),
    );

    app.refresh().expect("first refresh");
    assert_eq!(app.snapshot().watches[0].contract, "Draw");

    app.refresh().expect("second refresh");
    assert_eq!(app.snapshot().watches[0].contract, "Arsenal");
    assert!((app.snapshot().watches[0].profit_take_back_odds - 2.0).abs() < 0.001);
}

fn sample_snapshot(contract: &str, profit_take_back_odds: f64) -> WatchSnapshot {
    WatchSnapshot {
        position_count: 1,
        watch_count: 1,
        commission_rate: 0.0,
        target_profit: 1.0,
        stop_loss: 1.0,
        watches: vec![WatchRow {
            contract: contract.to_string(),
            market: "Full-time result".to_string(),
            position_count: 1,
            can_trade_out: true,
            total_stake: 10.0,
            total_liability: 23.29,
            current_pnl_amount: -0.31,
            average_entry_lay_odds: 3.35,
            entry_implied_probability: 0.2985,
            profit_take_back_odds,
            profit_take_implied_probability: 1.0 / profit_take_back_odds,
            stop_loss_back_odds: 3.04,
            stop_loss_implied_probability: 1.0 / 3.04,
        }],
    }
}
