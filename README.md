# operator-console

Rust Ratatui console for Sabi operator workflows.

It is the main operator-facing frontend for positions, calculators, matchers, recorder control, and observability over backend-backed betting data.

## What It Does

- Starts the Sabi terminal UI
- Loads backend-composed operator snapshots through `sabisabi` by default
- Reads market-intel data from `sabisabi`
- Reads Matchbook account-monitor state through the backend-owned operator snapshot surface
- Gives operators a single place to inspect live orders, market views, opportunities, alerts, calculators, and recorder state

## Owns

- Trading-focused operator UX
- Panel routing, keyboard flow, and persisted local UI state
- Rendering provider-backed positions, stats, matcher, calculator, and observability views

## Integrates With

- `../../sabisabi` for operator snapshot control, Matchbook monitor state, execution APIs, and persisted market-intel queries
- `../../bet-recorder` only behind `sabisabi` or explicit legacy provider modes for normalized snapshots and recorder control
- `../../workers/exchange-browser-worker` indirectly through legacy recorder-managed worker flows

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

- If you run the console without explicit payload arguments, it boots from the configured local provider state instead of immediately autostarting the recorder takeover path.
- If you pass `--bet-recorder-payload-path` or `--bet-recorder-run-dir`, it starts with a hybrid `bet-recorder` provider.
- On startup the console checks `SABISABI_BASE_URL`.
- By default the console does not build or start `sabisabi`; it comes up independently and reports backend-backed features as unavailable until the backend is healthy.
- By default the console does not silently fall back to recorder snapshots when the backend path is selected; recorder-backed launch modes remain explicit CLI opt-ins.
- If you explicitly set `OPERATOR_CONSOLE_AUTOSTART_SABISABI=1` and `SABISABI_BASE_URL` points at the default local backend (`http://127.0.0.1:4080` or `http://localhost:4080`), the console will build and start `sabisabi` before entering the TUI.
- Recorder startup remains an explicit operator action from the `Recorder` pane or other dedicated controls.

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

- `SABISABI_BASE_URL` overrides the backend base URL used for snapshot, Matchbook, execution, and market-intel reads.
- `SABISABI_CONTROL_TOKEN` is forwarded to backend control endpoints when configured.
- `OPERATOR_CONSOLE_AUTOSTART_SABISABI=1` restores the old opt-in behavior where the console bootstraps a local `sabisabi` process.
- The console keeps its own local UI, recorder, alerts, and matcher state on disk through crate-managed config files.
- For recorder-backed flows, the console can use native file inputs, a worker client, or a hybrid provider that prefers native data and falls back to the worker path.
- Manual positions load from local config, but legacy example seed entries are filtered so they do not pollute live sportsbook state.

## Operator Notes

- The in-app keymap overlay is the source of truth for navigation keys.
- `Trading > Accounts` is the venue-selection surface. Selecting a non-`smarkets` venue updates focus immediately; use `r` or `R` when you want a fresh live recapture.
- Recorder lifecycle controls are available from the `Recorder` pane.
- Snapshot reads, Matchbook account monitoring, and execution review/submit now go through `sabisabi` by default.
- The TUI no longer runs its own Matchbook polling loop or local snapshot projection path; the provider response is expected to arrive already composed from the backend.
- Recorder data is still a fallback/legacy path where adaptor-backed ingestion is not available and should not be treated as the primary runtime model.

## Test

```bash
cargo test
```

Target a narrower test when possible:

```bash
cargo test --test recorder_controls
cargo test recorder_start_and_stop_are_controllable_from_app
```
