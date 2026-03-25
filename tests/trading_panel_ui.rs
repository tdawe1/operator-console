use color_eyre::Result;
use operator_console::app::{App, TradingSection};
use operator_console::domain::{
    AccountStats, DecisionSummary, ExchangePanelSnapshot, ExitRecommendation, OpenPositionRow,
    OtherOpenBetRow, RecorderEventSummary, RuntimeSummary, TrackedBetRow, TrackedLeg,
    TransportMarkerSummary, VenueId, VenueStatus, VenueSummary, WatchRow, WatchSnapshot,
    WorkerStatus, WorkerSummary,
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
fn positions_panel_renders_actionable_boards() {
    let rendered = render_section(TradingSection::Positions);

    assert!(rendered.contains("Active Positions"));
    assert!(rendered.contains("Historical Positions"));
    assert!(rendered.contains("Signal Board"));
    assert!(rendered.contains("Sharp"));
    assert!(rendered.contains("Watch Plan"));
    assert!(rendered.contains("Operator Feed"));
}

#[test]
fn stats_panel_renders_operating_ratios_and_mix_tables() {
    let rendered = render_section(TradingSection::Stats);

    assert!(rendered.contains("Trading Stats"));
    assert!(rendered.contains("Running P/L"));
    assert!(rendered.contains("Exposure vs Balance"));
    assert!(rendered.contains("Decision Mix"));
    assert!(rendered.contains("Tracked Mix"));
}

#[test]
fn markets_panel_renders_api_surface_board() {
    let rendered = render_section(TradingSection::Markets);

    assert!(rendered.contains("Owls Markets"));
    assert!(rendered.contains("Endpoint Board"));
    assert!(rendered.contains("/api/v1/nba/odds"));
    assert!(rendered.contains("Preview"));
}

#[test]
fn live_and_props_panels_render_dedicated_owls_views() {
    let live = render_section(TradingSection::Live);
    let props = render_section(TradingSection::Props);

    assert!(live.contains("Owls Live"));
    assert!(live.contains("Live Board"));
    assert!(props.contains("Owls Props"));
    assert!(props.contains("Props Board"));
}

#[test]
fn recorder_panel_renders_capture_pipeline_and_evidence() {
    let rendered = render_section(TradingSection::Recorder);

    assert!(rendered.contains("Capture Pipeline"));
    assert!(rendered.contains("Recorder Config"));
    assert!(rendered.contains("Field Detail"));
    assert!(rendered.contains("Recorder Evidence"));
    assert!(rendered.contains("history sync success"));
}

#[test]
fn positions_live_view_overlay_renders_cashout_and_matrix() {
    let mut snapshot = sample_snapshot();
    snapshot.other_open_bets = vec![OtherOpenBetRow {
        venue: String::from("betway"),
        event: String::from("Arsenal v Everton"),
        label: String::from("Arsenal"),
        market: String::from("Match Odds"),
        side: String::from("back"),
        odds: 2.375,
        stake: 10.0,
        status: String::from("cash_out"),
        funding_kind: String::from("cash"),
        current_cashout_value: Some(16.16),
        supports_cash_out: true,
    }];
    snapshot.transport_events = vec![TransportMarkerSummary {
        captured_at: String::from("2026-03-18T12:35:01Z"),
        kind: String::from("interaction_marker"),
        action: String::from("cash_out"),
        phase: String::from("response"),
        request_id: String::new(),
        reference_id: String::from("bet-1"),
        summary: String::from("response cash_out bet-1"),
        detail: String::from("Cash out requested for bet-1."),
    }];
    let mut app = App::from_provider(StaticProvider { snapshot }).expect("app");
    app.set_trading_section(TradingSection::Positions);
    app.toggle_live_view_overlay();

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
    let rendered = lines.join("\n");

    assert!(rendered.contains("Live View"));
    assert!(rendered.contains("Opportunity Lens"));
    assert!(rendered.contains("Decision Matrix"));
    assert!(rendered.contains("Half"));
    assert!(rendered.contains("Execution Trail"));
    assert!(rendered.contains("cash_out bet-1"));
    assert!(rendered.contains("16.16"));
}

#[test]
fn historical_positions_overlay_renders_selected_history_detail() {
    let mut snapshot = sample_snapshot();
    snapshot.historical_positions = vec![OpenPositionRow {
        event: String::from("Aston Villa v Chelsea"),
        event_status: String::from("Settled"),
        event_url: String::new(),
        contract: String::from("jorrel hato (chelsea) - Player To Receive A Card"),
        market: String::from("Player Cards"),
        status: String::from("settled"),
        market_status: String::from("settled"),
        is_in_play: false,
        price: 4.50,
        stake: 2.0,
        liability: 0.0,
        current_value: 0.0,
        pnl_amount: -2.0,
        overall_pnl_known: true,
        current_back_odds: Some(4.5),
        current_implied_probability: Some(1.0 / 4.5),
        current_implied_percentage: Some(100.0 / 4.5),
        current_buy_odds: Some(4.5),
        current_buy_implied_probability: Some(1.0 / 4.5),
        current_sell_odds: None,
        current_sell_implied_probability: None,
        current_score: String::new(),
        current_score_home: None,
        current_score_away: None,
        live_clock: String::from("13:49"),
        can_trade_out: false,
    }];

    let mut app = App::from_provider(StaticProvider { snapshot }).expect("app");
    app.set_trading_section(TradingSection::Positions);
    app.handle_key(crossterm::event::KeyCode::Tab);
    app.toggle_live_view_overlay();

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
    let rendered = lines.join("\n");

    assert!(rendered.contains("History View"));
    assert!(rendered.contains("Comparable History"));
    assert!(rendered.contains("Aston Villa v Chelsea"));
}

#[test]
fn positions_panel_renders_selected_interaction_evidence() {
    let mut snapshot = sample_snapshot();
    snapshot.transport_events = vec![TransportMarkerSummary {
        captured_at: String::from("2026-03-18T12:35:01Z"),
        kind: String::from("interaction_marker"),
        action: String::from("place_bet"),
        phase: String::from("response"),
        request_id: String::from("req-77"),
        reference_id: String::from("bet-1"),
        summary: String::from("response place_bet req-77 bet-1"),
        detail: String::from("loaded in review mode"),
    }];
    snapshot.recorder_events = vec![RecorderEventSummary {
        captured_at: String::from("2026-03-18T12:35:01Z"),
        kind: String::from("operator_interaction"),
        source: String::from("operator_console"),
        page: String::from("worker_request"),
        action: String::from("place_bet"),
        status: String::from("response:submitted"),
        request_id: String::from("req-77"),
        reference_id: String::from("bet-1"),
        summary: String::from("place_bet bet-1 -> response:submitted"),
        detail: String::from("loaded in review mode"),
    }];

    let mut app = App::from_provider(StaticProvider { snapshot }).expect("app");
    app.set_trading_section(TradingSection::Positions);

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
    let rendered = lines.join("\n");

    assert!(rendered.contains("I/O"));
    assert!(rendered.contains("bet subm"));
    assert!(rendered.contains("selected ref bet-1"));
    assert!(rendered.contains("place_bet"));
    assert!(rendered.contains("req-77"));
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
            refresh_kind: String::from("live_capture"),
            worker_reconnect_count: 0,
            decision_count: 2,
            watcher_iteration: Some(42),
            stale: false,
        }),
        account_stats: Some(AccountStats {
            available_balance: 500.0,
            exposure: 120.0,
            unrealized_pnl: 14.25,
            cumulative_pnl: Some(253.69),
            cumulative_pnl_label: String::from("P&L since Jan 2026"),
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
            overall_pnl_known: true,
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
        ledger_pnl_summary: Default::default(),
        other_open_bets: vec![OtherOpenBetRow {
            venue: String::from("bet365"),
            event: String::from("Brumbies v Chiefs"),
            label: String::from("Brumbies"),
            market: String::from("To Win"),
            side: String::from("back"),
            odds: 3.10,
            stake: 10.0,
            status: String::from("cash_out"),
            funding_kind: String::from("cash"),
            current_cashout_value: Some(16.16),
            supports_cash_out: true,
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
        recorder_bundle: Some(operator_console::domain::RecorderBundleSummary {
            run_dir: String::from("/tmp/sabi-smarkets-watcher"),
            event_count: 12,
            latest_event_at: String::from("2026-03-18T12:34:56Z"),
            latest_event_kind: String::from("bookmaker_history_sync"),
            latest_event_summary: String::from("bet365 history sync success (3 row(s))"),
            latest_positions_at: String::from("2026-03-18T12:34:55Z"),
            latest_watch_plan_at: String::from("2026-03-18T12:34:54Z"),
        }),
        recorder_events: vec![
            RecorderEventSummary {
                captured_at: String::from("2026-03-18T12:34:56Z"),
                kind: String::from("bookmaker_history_sync"),
                source: String::from("bet365"),
                page: String::from("my_bets"),
                action: String::from("bet365"),
                status: String::from("success"),
                request_id: String::new(),
                reference_id: String::new(),
                summary: String::from("bet365 history sync success (3 row(s))"),
                detail: String::from("https://www.bet365.com/#/MB/SB"),
            },
            RecorderEventSummary {
                captured_at: String::from("2026-03-18T12:34:57Z"),
                kind: String::from("action_snapshot"),
                source: String::from("smarkets_exchange"),
                page: String::from("watcher_state"),
                action: String::new(),
                status: String::new(),
                request_id: String::new(),
                reference_id: String::new(),
                summary: String::from("watcher iteration captured"),
                detail: String::from("0 watch groups from 0 positions"),
            },
        ],
        transport_summary: None,
        transport_events: Vec::new(),
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
        horse_matcher: None,
    }
}
