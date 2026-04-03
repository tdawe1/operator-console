# console/operator-console/src/

## Responsibility

Implements the operator console runtime and process bootstrap: crate module exports, tracing initialization, provider abstraction, recorder supervision, panel rendering, and supporting domain/projection modules.

## Design

- `lib.rs` exposes the main boundaries: `app`, `provider`, `runtime`, `recorder`, `worker_client`, `native_provider`, `panels`, `tracing_setup`, and domain/helper modules.
- `app.rs` is the state machine. It owns the current `ExchangePanelSnapshot`, panel selection, overlay state, table state, alerts, notifications, manual positions, and worker channel handles.
- `app_state.rs` defines the navigation model (`Panel`, `TradingSection`, `ObservabilitySection`, `PositionsFocus`, calculator/trading overlay fields).
- `provider.rs` reduces the data boundary to `ProviderRequest -> ExchangePanelSnapshot` so UI code stays transport-agnostic.
- `runtime.rs` runs four background lanes: provider jobs, OddsMatcher refresh, Owls sync, and Matchbook sync.
- `tracing_setup.rs` owns tracing bootstrap. It builds a shared subscriber with `EnvFilter`, writes to stderr, keeps event targets, disables ANSI, omits timestamps, and defaults the filter to `info` when `RUST_LOG` is unset.

## Flow

1. `main.rs` installs `color-eyre`, calls `init_tracing()`, parses CLI args into `LaunchMode`, and builds the chosen provider.
2. `main.rs` calls `App::from_provider`; `app.rs` immediately loads a snapshot through `ProviderRequest::LoadDashboard`.
3. `normalize_snapshot`/projection helpers shape raw provider data plus manual positions into a renderable snapshot.
4. `ui.rs` renders shell chrome and dispatches to `panels::*` based on `Panel` and `TradingSection`.
5. Interactive actions queue provider work (`RefreshCached`, `RefreshLive`, `SelectVenue`, `CashOutTrackedBet`, `ExecuteTradingAction`, `LoadHorseMatcher`) or side workers.
6. `drain_*_results` methods fold async worker results back into `App`, update `ResourceState`, refresh enrichment, and surface status/alerts.

## Integration

- `native_provider.rs` reads watcher-state/run-dir artifacts and handles native execution; `worker_client.rs` bridges to the external worker session; `stub_provider.rs` supplies fixture data.
- `trading_actions.rs` is consumed by positions/matcher panels and the trading action overlay to build `TradingActionSeed`, risk reports, and final `TradingActionIntent` values.
- `recorder.rs` is consumed by `app.rs` and `panels/recorder.rs` for config editing, autostart/start/stop, and process attachment.
- `owls.rs`, `oddsmatcher.rs`, `horse_matcher.rs`, and `exchange_api.rs` feed the specialized panel data shown by the TUI.
- `main.rs` depends on `tracing_setup::init_tracing` for process-wide logging before any provider or app work begins.
