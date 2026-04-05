# operator-console

Rust Ratatui console for Sabi operator workflows.

It is the main operator-facing frontend for positions, calculators, matchers, recorder control, and observability over worker-backed betting data.

## What It Does

- Starts the Sabi terminal UI
- Loads either a stub provider or a `bet-recorder`-backed provider depending on CLI arguments
- Reads market-intel data from `sabisabi`
- Gives operators a single place to inspect live orders, market views, opportunities, alerts, calculators, and recorder state

## Owns

- Trading-focused operator UX
- Panel routing, keyboard flow, and persisted local UI state
- Rendering recorder-backed positions, stats, matcher, calculator, and observability views

## Integrates With

- `../../bet-recorder` for normalized snapshots and recorder control
- `../../sabisabi` for persisted market intelligence and query APIs
- `../../workers/exchange-browser-worker` indirectly through recorder-managed worker flows

Treat this crate as the top-level client in the active Sabi runtime path, not as a standalone app.

If a change alters transport messages, recorder snapshot shape, or live recorder behavior, verify the console against the recorder change rather than treating this repo as isolated.

## Current Surface

The console currently exposes two top-level workspaces:

- `Trading`
- `Observability`

The active pane set includes:

- `Live Orders`
- `Accounts`
- `History`
- `Markets`
- `Live`
- `Props`
- `Chart`
- `Opportunities`
- `Matcher`
- `Stats`
- `Alerts`
- `Calc`
- `Recorder`
- `Observability`

The exact layout is managed in the window manager and can be changed without changing the launch surface.

## Startup Behavior

- If you run the console without a recorder payload or run directory, it starts in stub mode.
- If you pass `--bet-recorder-payload-path` or `--bet-recorder-run-dir`, it starts with a hybrid `bet-recorder` provider.
- On startup the console checks `SABISABI_BASE_URL`.
- If `SABISABI_BASE_URL` is the default local backend (`http://127.0.0.1:4080` or `http://localhost:4080`) and the service is not healthy, the console builds and starts `sabisabi` automatically before entering the TUI.

## Run

Default launch:

```bash
cargo run
```

Show CLI help:

```bash
cargo run -- --help
```

List themes:

```bash
cargo run -- --list-themes
```

Load from a recorder run bundle:

```bash
cargo run -- --bet-recorder-run-dir /path/to/run-dir
```

Load from captured payloads:

```bash
cargo run -- --bet-recorder-payload-path /path/to/positions.json
```

Useful options:

- `--theme <name>`
- `--bet-recorder-payload-path <path>`
- `--bet-recorder-run-dir <path>`
- `--bet-recorder-account-path <path>`
- `--bet-recorder-open-bets-path <path>`
- `--bet-recorder-session <name>`
- `--bet-recorder-command <path>`
- `--bet-recorder-python <path>`
- `--bet-recorder-root <path>`
- `--commission-rate <value>`
- `--target-profit <value>`
- `--stop-loss <value>`

## Configuration Notes

- `SABISABI_BASE_URL` overrides the backend base URL used for market-intel reads.
- The console keeps its own local UI, recorder, alerts, and matcher state on disk through crate-managed config files.
- For recorder-backed flows, the console can use native file inputs, a worker client, or a hybrid provider that prefers native data and falls back to the worker path.

## Operator Notes

- The in-app keymap overlay is the source of truth for navigation keys.
- `Trading > Accounts` is the venue-selection surface. Selecting a non-`smarkets` venue updates focus immediately; use `r` or `R` when you want a fresh live recapture.
- Recorder lifecycle controls are available from the `Recorder` pane.
- Market-intel data is read from `sabisabi`; recorder data is still a fallback/legacy path where adaptor-backed ingestion is not available.

## Test

```bash
cargo test
```

Target a narrower test when possible:

```bash
cargo test --test recorder_controls
cargo test recorder_start_and_stop_are_controllable_from_app
```
