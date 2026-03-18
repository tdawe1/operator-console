# operator-console

Protocol-first Rust TUI for the `Sabi` operator shell.

Current scope:
- Ratatui shell with top-level `Dashboard`, `Trading`, `Banking`, and `Observability` modules
- generic exchange-domain snapshot models
- typed provider contract
- transport-shaped stub exchange provider
- typed worker transport and persistent stdio session management
- optional `bet-recorder`-backed trading data over that worker session
- in-TUI recorder lifecycle control and config editing under `Trading > Recorder`
- unified tracked-bet rows and deterministic exit recommendations in `Trading > Positions`

## Run

```bash
cd /home/thomas/projects/sabi/console/operator-console
cargo run
```

One-word launcher:

```bash
sabi
```

Recorder-backed Trading data:

```bash
cargo run -- \
  --bet-recorder-payload-path /home/thomas/projects/sabi/console/operator-console/fixtures/smarkets-open-positions.json
```

Recorder-backed Exchanges data from a live `bet-recorder` run bundle:

```bash
cargo run -- \
  --bet-recorder-run-dir /tmp/bet-recorder-demo/captures/smarkets_exchange/2026/2026-03-11/run-20260311T110500Z
```

Live capture on each refresh from the current `agent-browser` session:

```bash
cargo run -- \
  --bet-recorder-run-dir /tmp/bet-recorder-demo/captures/smarkets_exchange/2026/2026-03-11/run-20260311T110500Z \
  --bet-recorder-session helium-copy
```

Override the recorder executable explicitly:

```bash
cargo run -- \
  --bet-recorder-command /home/thomas/projects/sabi/bet-recorder/bin/bet-recorder \
  --bet-recorder-run-dir /tmp/bet-recorder-demo/captures/smarkets_exchange/2026/2026-03-11/run-20260311T110500Z \
  --bet-recorder-session helium-copy
```

Expected `Trading` module content in recorder-backed mode:
- account stats
- raw open positions
- grouped watch rows
- other open bets
- tracked bets
- exit recommendations

The `Trading > Positions` panel now keeps the raw Smarkets state visible and adds a derived layer:
- `Tracked Bets` shows canonical bet rows loaded through the worker snapshot
- `Exit Recommendations` shows the current deterministic `hold` / `warn` / `cash_out` decision state

The worker config now carries additional ledger/policy fields over the stdio transport:
- `companion_legs_path` optional static input for non-Smarkets companion legs
- `hard_margin_call_profit_floor` optional hard auto-exit threshold
- `warn_only_default` boolean policy flag for non-hard-threshold exits

`--bet-recorder-account-path` and `--bet-recorder-open-bets-path` still exist as optional fallback
inputs, but the normal path is now either:
- a richer `open_positions` payload that already contains those sections in its captured text
- a `bet-recorder` run bundle whose latest `positions_snapshot` event contains that richer capture

If you also pass `--bet-recorder-session`, the worker captures the current Smarkets
`open_positions` page into that run bundle before each `LoadDashboard` or `Refresh`.

The long-running watcher path no longer takes a screenshot on every poll. It captures page state
without screenshots during the loop and still records screenshots for explicit capture/action flows.

Example real-data flow:

```bash
cd /home/thomas/projects/sabi/bet-recorder

./bin/bet-recorder init-run \
  --source smarkets_exchange \
  --root-dir /tmp/bet-recorder-demo \
  --started-at 2026-03-11T11:05:00Z \
  --collector-version dev \
  --browser-profile-used helium-copy

./bin/bet-recorder record-page \
  --source smarkets_exchange \
  --run-dir /tmp/bet-recorder-demo/captures/smarkets_exchange/2026/2026-03-11/run-20260311T110500Z \
  --payload-path /tmp/smarkets-open-positions.json

cd /home/thomas/projects/sabi/console/operator-console

cargo run -- \
  --bet-recorder-run-dir /tmp/bet-recorder-demo/captures/smarkets_exchange/2026/2026-03-11/run-20260311T110500Z \
  --bet-recorder-session helium-copy
```

## Navigation

Top-level modules:
- `Dashboard`
- `Trading`
- `Banking`
- `Observability`

Trading sub-sections:
- `Accounts`
- `Positions`
- `Markets`
- `Stats`
- `Recorder`

Core keys:
- `q` quit
- `j` / `k` switch module
- `h` / `l` switch section within the active module
- `up` / `down` move the venue list in `Trading > Accounts`
- `up` / `down` move the selected config field in `Trading > Recorder`
- `tab` / `shift-tab` still cycle modules as an alternate path
- `r` refresh the current provider
- `s` start recorder from `Trading > Recorder`
- `x` stop recorder from `Trading > Recorder`
- `u` reload saved recorder config
- `D` reset recorder config to defaults
- `[` / `]` cycle common recorder field suggestions
- `enter` begin/apply recorder field edits
- `esc` cancel recorder edit or quit from the shell

## Test

```bash
cd /home/thomas/projects/sabi/console/operator-console
cargo test
```

## Architecture

The shell does not read fixture files directly. The UI talks to a provider boundary, and the
default `StubExchangeProvider` simply loads a transport-shaped snapshot from
`fixtures/exchange_panel_snapshot.json`. When `--bet-recorder-payload-path` or
`--bet-recorder-run-dir` is provided, the shell instead boots a worker-backed provider that routes
typed requests through
`worker_client.rs`, speaks newline-delimited JSON over a persistent stdio session to
`bet-recorder exchange-worker-session`, sends the recorder config in the first
`LoadDashboard` request, and maps the resulting worker snapshot into the Exchanges panel. In
run-bundle mode, the Python worker resolves the latest `positions_snapshot` event from
`events.jsonl` on each refresh. When `--bet-recorder-session` is set, the Python worker first
captures the current `open_positions` page from that `agent-browser` session into the run bundle,
then reads the newest recorded snapshot.

If a worker refresh or venue sync fails, the shell keeps the last good snapshot visible, marks
the worker as `Error`, and surfaces the transport failure in the status footer. If the Python
worker has simply died, the client will respawn it, replay the last `LoadDashboard` bootstrap,
and retry the in-flight request once.

The transport layer also includes a narrow `CashOutTrackedBet` request for Smarkets-only action
scaffolding. At the moment it validates the request path end-to-end and refuses execution until a
captured Smarkets trade-out submission contract is implemented; it does not place bets.

That keeps the Rust side aligned with the longer-term architecture:
- Rust owns terminal lifecycle, panels, routing, and operator state
- a worker-backed provider owns live data acquisition
- the Python exchange browser worker can replace the stub provider without a UI rewrite

The current IA is intentionally broader than betting:
- `Dashboard` is cross-domain overview
- `Trading` is the home for bookmaker and exchange workflows
- `Banking` is reserved for cash-management workflows
- `Observability` is the home for worker, watcher, config, log, and health views
