# console/operator-console/src/panels/

## Responsibility

Contains the concrete Ratatui views for each console surface: trading boards, recorder controls, calculator/matcher screens, observability pages, and modal overlays.

## Design

- `mod.rs` registers the active panel modules used by `ui.rs`.
- `trading_positions.rs` is the main trading board for open/historical positions. It joins exchange rows, tracked bets, sportsbook bets, watch thresholds, and live quote context into one operator view.
- `trading_markets.rs` renders Owls endpoint boards for `Markets`, `Live`, and `Props` sections without cloning the selected endpoint per frame.
- `trading_action_overlay.rs` renders the bet ticket and risk tape for `TradingActionSeed`/`TradingRiskReport` produced in `app.rs` + `trading_actions.rs`.
- `recorder.rs` exposes recorder runtime/config state as an editable control panel backed by `RecorderConfig` and recorder evidence from the snapshot bundle.

## Flow

1. `ui::render` selects a panel module from current `Panel`/`TradingSection`.
2. Panel renderers consume immutable snapshot data plus mutable selection state owned by `App`.
3. Position and matcher panels surface executable selections; `App` converts those to a trading-action overlay, which can then submit `ProviderRequest::ExecuteTradingAction`.
4. Recorder panel reflects `App` recorder state while start/stop/config edits are executed in `app.rs` via `RecorderSupervisor` and provider worker restarts.

## Integration

- Primary consumer is `ui.rs`; primary upstream producer is `app.rs`.
- `trading_positions.rs` consumes domain rows, Owls enrichment, Matchbook state, and trading-action helpers.
- `trading_markets.rs` consumes `owls.rs` endpoint summaries filtered by current trading section.
- `trading_action_overlay.rs` consumes overlay state from `app_state.rs`/`app.rs` and risk data from `trading_actions.rs`.
- `recorder.rs` consumes recorder config/status from `recorder.rs` and recorder bundle/events attached by `native_provider.rs` snapshots.
