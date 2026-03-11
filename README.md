# operator-console

Protocol-first Rust TUI for the `sabi` operator shell.

Current scope:
- Ratatui shell with `Dashboard` and `Exchanges` panels
- generic exchange-domain snapshot models
- typed provider contract
- transport-shaped stub exchange provider
- typed worker transport and persistent stdio session management
- optional `bet-recorder`-backed Exchanges provider over that worker session

## Run

```bash
cd /home/thomas/projects/sabi/console/operator-console
cargo run
```

Recorder-backed Exchanges data:

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

Expected Exchanges panel content in recorder-backed mode:
- account stats
- raw open positions
- grouped watch rows
- other open bets

`--bet-recorder-account-path` and `--bet-recorder-open-bets-path` still exist as optional fallback
inputs, but the normal path is now either:
- a richer `open_positions` payload that already contains those sections in its captured text
- a `bet-recorder` run bundle whose latest `positions_snapshot` event contains that richer capture

If you also pass `--bet-recorder-session`, the worker captures the current Smarkets
`open_positions` page into that run bundle before each `LoadDashboard` or `Refresh`.

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

Core keys:
- `q` quit
- `tab` switch panel
- `j`/`k` move in the exchanges list
- `r` refresh

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

That keeps the Rust side aligned with the longer-term architecture:
- Rust owns terminal lifecycle, panels, routing, and operator state
- a worker-backed provider owns live data acquisition
- the Python exchange browser worker can replace the stub provider without a UI rewrite
