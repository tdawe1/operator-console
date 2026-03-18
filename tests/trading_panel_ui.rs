use color_eyre::Result;
use operator_console::app::{App, TradingSection};
use operator_console::domain::{
    AccountStats, DecisionSummary, ExchangePanelSnapshot, ExitRecommendation, OpenPositionRow,
    OtherOpenBetRow, RuntimeSummary, TrackedBetRow, TrackedLeg, VenueId, VenueStatus,
    VenueSummary, WatchRow, WatchSnapshot, WorkerStatus, WorkerSummary,
};
use operator_console::provider::{ExchangeProvider, ProviderRequest};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

struct StaticProvider {
    snapshot: ExchangePanelSnapshot,
}

impl ExchangeProvider for StaticProvider {
    fn handle(&mut self, _request: ProviderRequest) -> Result<ExchangePanelSnapshot> {
        Ok(self.snapshot.clone())
    }
}

#[test]
fn accounts_panel_renders_multi_venue_board() {
    let rendered = render_section(TradingSection::Accounts);

    assert!(rendered.contains("Venue Board"));
    assert!(rendered.contains("Live Feed Preview"));
    assert!(rendered.contains("Bet365"));
    assert!(rendered.contains("Connected Venues"));
}

#[test]
fn stats_panel_renders_operating_ratios_and_mix_tables() {
    let rendered = render_section(TradingSection::Stats);

    assert!(rendered.contains("Trading Stats"));
    assert!(rendered.contains("Exposure vs Balance"));
    assert!(rendered.contains("Decision Mix"));
    assert!(rendered.contains("Tracked Mix"));
}

#[test]
fn recorder_panel_renders_capture_pipeline_and_runbook() {
    let rendered = render_section(TradingSection::Recorder);

    assert!(rendered.contains("Capture Pipeline"));
    assert!(rendered.contains("Recorder Config"));
    assert!(rendered.contains("Field Detail"));
    assert!(rendered.contains("Recorder Runbook"));
}

fn render_section(section: TradingSection) -> String {
    let mut app = App::from_provider(StaticProvider {
        snapshot: sample_snapshot(),
    })
    .expect("app");
    app.set_trading_section(section);

    let backend = TestBackend::new(160, 40);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| operator_console::ui::render(frame, &mut app))
        .expect("draw ui");

    let buffer = terminal.backend().buffer().clone();
    let area = buffer.area;
    let mut lines = Vec::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buffer.cell((x, y)).expect("cell").symbol());
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn sample_snapshot() -> ExchangePanelSnapshot {
    ExchangePanelSnapshot {
        worker: WorkerSummary {
            name: String::from("bet-recorder"),
            status: WorkerStatus::Ready,
            detail: String::from("connected to live browser session"),
        },
        venues: vec![
            VenueSummary {
                id: VenueId::Smarkets,
                label: String::from("Smarkets"),
                status: VenueStatus::Connected,
                detail: String::from("watcher ready"),
                event_count: 3,
                market_count: 9,
            },
            VenueSummary {
                id: VenueId::Bet365,
                label: String::from("Bet365"),
                status: VenueStatus::Connected,
                detail: String::from("cdp tab visible"),
                event_count: 1,
                market_count: 2,
            },
            VenueSummary {
                id: VenueId::Betway,
                label: String::from("Betway"),
                status: VenueStatus::Error,
                detail: String::from("login required"),
                event_count: 0,
                market_count: 0,
            },
        ],
        selected_venue: Some(VenueId::Smarkets),
        events: vec![operator_console::domain::EventCandidateSummary {
            id: String::from("evt-1"),
            label: String::from("Arsenal v Everton"),
            competition: String::from("Premier League"),
            start_time: String::from("2026-03-18T20:00:00Z"),
            url: String::new(),
        }],
        markets: vec![operator_console::domain::MarketSummary {
            name: String::from("Match Odds"),
            contract_count: 3,
        }],
        preflight: None,
        status_line: String::from("Live recorder snapshot loaded."),
        runtime: Some(RuntimeSummary {
            updated_at: String::from("2026-03-18T12:34:56Z"),
            source: String::from("bet-recorder"),
            decision_count: 2,
            watcher_iteration: Some(42),
            stale: false,
        }),
        account_stats: Some(AccountStats {
            available_balance: 500.0,
            exposure: 120.0,
            unrealized_pnl: 14.25,
            currency: String::from("GBP"),
        }),
        open_positions: vec![OpenPositionRow {
            event: String::from("Arsenal v Everton"),
            event_status: String::from("1H | 12:15"),
            event_url: String::new(),
            contract: String::from("Arsenal"),
            market: String::from("Match Odds"),
            status: String::from("matched"),
            market_status: String::from("active"),
            is_in_play: true,
            price: 2.8,
            stake: 20.0,
            liability: 36.0,
            current_value: 22.5,
            pnl_amount: 2.5,
            current_back_odds: Some(2.4),
            current_implied_probability: Some(0.416),
            current_implied_percentage: Some(41.6),
            current_buy_odds: Some(2.42),
            current_buy_implied_probability: Some(0.413),
            current_sell_odds: Some(2.46),
            current_sell_implied_probability: Some(0.406),
            current_score: String::from("1-0"),
            current_score_home: Some(1),
            current_score_away: Some(0),
            live_clock: String::from("12:15"),
            can_trade_out: true,
        }],
        historical_positions: Vec::new(),
        other_open_bets: vec![OtherOpenBetRow {
            label: String::from("Brumbies"),
            market: String::from("To Win"),
            side: String::from("back"),
            odds: 3.10,
            stake: 10.0,
            status: String::from("cash_out"),
        }],
        decisions: vec![
            DecisionSummary {
                contract: String::from("Arsenal"),
                market: String::from("Match Odds"),
                status: String::from("take_profit_ready"),
                reason: String::from("target hit"),
                current_pnl_amount: 2.5,
                current_back_odds: Some(2.4),
                profit_take_back_odds: 2.5,
                stop_loss_back_odds: 3.0,
            },
            DecisionSummary {
                contract: String::from("Arsenal"),
                market: String::from("Match Odds"),
                status: String::from("hold"),
                reason: String::from("watching"),
                current_pnl_amount: 2.5,
                current_back_odds: Some(2.4),
                profit_take_back_odds: 2.5,
                stop_loss_back_odds: 3.0,
            },
        ],
        watch: Some(WatchSnapshot {
            position_count: 1,
            watch_count: 1,
            commission_rate: 0.0,
            target_profit: 1.0,
            stop_loss: 1.0,
            watches: vec![WatchRow {
                contract: String::from("Arsenal"),
                market: String::from("Match Odds"),
                position_count: 1,
                can_trade_out: true,
                total_stake: 20.0,
                total_liability: 36.0,
                current_pnl_amount: 2.5,
                current_back_odds: Some(2.4),
                average_entry_lay_odds: 2.8,
                profit_take_back_odds: 2.5,
                stop_loss_back_odds: 3.0,
                entry_implied_probability: 0.357,
                profit_take_implied_probability: 0.400,
                stop_loss_implied_probability: 0.333,
            }],
        }),
        tracked_bets: vec![TrackedBetRow {
            bet_id: String::from("bet-1"),
            group_id: String::from("grp-1"),
            event: String::from("Arsenal v Everton"),
            market: String::from("Match Odds"),
            selection: String::from("Arsenal"),
            status: String::from("open"),
            platform: String::from("bet365"),
            legs: vec![TrackedLeg {
                venue: String::from("bet365"),
                outcome: String::from("Arsenal"),
                side: String::from("back"),
                odds: 3.10,
                stake: 10.0,
                status: String::from("open"),
                ..TrackedLeg::default()
            }],
            ..TrackedBetRow::default()
        }],
        exit_policy: operator_console::domain::ExitPolicySummary {
            target_profit: 1.0,
            stop_loss: 1.0,
            hard_margin_call_profit_floor: None,
            warn_only_default: true,
        },
        exit_recommendations: vec![ExitRecommendation {
            bet_id: String::from("bet-1"),
            action: String::from("cash_out"),
            reason: String::from("profit target"),
            worst_case_pnl: 1.5,
            cash_out_venue: Some(String::from("smarkets")),
        }],
    }
}
