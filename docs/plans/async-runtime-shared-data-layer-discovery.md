# Async Runtime Shared Data Layer Discovery

## Current Worker Entry Points

- `src/app.rs`
  - startup creates provider, OddsMatcher, Owls, and Matchbook worker channels
  - watchdog paths restart provider, Owls, and Matchbook workers
- `src/app.rs`
  - result draining for provider, Owls, and Matchbook happens on the UI thread

## Current Blocking Boundaries

- `src/provider.rs`
  - `ExchangeProvider::handle(...)` is synchronous and stateful
- `src/owls.rs`
  - Owls sync uses `reqwest::blocking::Client`
- `src/exchange_api.rs`
  - Matchbook sync uses `reqwest::blocking::Client`
- `src/recorder.rs`
  - recorder lifecycle is an external process boundary, not an in-process async worker

## Freeze-Relevant Observations

- current worker implementation uses detached `std::thread::spawn` + `std::sync::mpsc`
- watchdog recovery can replace channels and mark state stale, but cannot truly cancel already-running blocking work
- UI state, worker lifecycle, and persistence are still tightly coupled in `src/app.rs`

## Migration Seam For Sprint 1

- keep UI behavior unchanged
- introduce a runtime abstraction that owns current worker startup wiring
- add lifecycle tracing around dispatch, restart, and result delivery before the Tokio migration begins
