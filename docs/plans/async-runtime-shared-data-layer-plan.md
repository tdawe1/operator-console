# Plan: Async Runtime Shared Data Layer

**Generated**: 2026-03-25
**Estimated Complexity**: High

## Overview
Refactor `console/operator-console` so background provider work no longer relies on detached `std::thread` + `std::sync::mpsc` workers that cannot be cancelled once hung. Introduce a Tokio-backed async worker runtime with explicit task ownership, cancellation, timeouts, and tracing. Layer a SQLite-backed shared data store underneath the live UI so the app can survive restarts, preserve last-good state, and expose queryable state to other tools.

Because external access requirements are still evolving, this plan starts with a **single-writer model**: `operator-console` owns writes to SQLite, while other processes/tools read from exported tables/views. The schema is designed so a later multi-writer migration can happen without rewriting the UI projection model.

## Prerequisites
- Current target repo: `console/operator-console`
- Existing hardening work in:
  - `src/resource_state.rs`
  - `src/snapshot_projection.rs`
  - `tests/async_resource_state.rs`
- Add current-library dependencies after implementation begins:
  - `tokio`
  - `tokio-util`
  - `sqlx` with SQLite + migrations
  - `tracing` / `tracing-subscriber`
- Keep `bet-recorder` as an external process in this phase; do not fold it into the same runtime yet.

## Sprint 1: Prove The Freeze Boundary And Create Async Seams
**Goal**: Make the current freeze observable and isolate all background work behind explicit runtime-owned interfaces.

**Demo/Validation**:
- Run `cargo test --test async_resource_state -- --nocapture`
- Run a new focused runtime smoke test that starts, cancels, and restarts a provider job without touching the TUI main loop.
- Verify trace logs identify task lifecycle: spawn, cancel, timeout, complete.

### Task 1.1: Inventory all current worker entry points
- **Location**: `src/app.rs`, `src/recorder.rs`, `src/exchange_api.rs`
- **Description**: Document every `thread::spawn`, blocking HTTP call, channel boundary, watchdog path, and restart path currently used for provider/Owls/Matchbook work.
- **Dependencies**: None
- **Acceptance Criteria**:
  - Every worker entry point is listed with caller and result consumer.
  - The list distinguishes cancellable vs non-cancellable paths.
- **Validation**:
  - Save notes in the plan or implementation scratch doc; no code changes required.

### Task 1.2: Add runtime tracing around current job lifecycle
- **Location**: `src/app.rs`, `src/recorder.rs`
- **Description**: Add minimal tracing spans/events for job submission, worker restart, timeout expiry, result delivery, and snapshot projection refresh.
- **Dependencies**: Task 1.1
- **Acceptance Criteria**:
  - Provider/Owls/Matchbook jobs emit a request id or job id.
  - Timeouts and worker restarts are visible in logs without opening the TUI.
- **Validation**:
  - `cargo test recorder_config_edit_restarts_running_recorder --test recorder_controls -- --nocapture`
  - Manual run with tracing enabled shows ordered lifecycle events.

### Task 1.3: Introduce async-facing runtime abstraction without changing behavior yet
- **Location**: Create `src/runtime.rs`, modify `src/app.rs`, `src/lib.rs`
- **Description**: Define a small boundary object for background jobs (`AppRuntime`, `RuntimeCommand`, `RuntimeEvent`) so `App` stops owning raw worker channel details directly.
- **Dependencies**: Task 1.1
- **Acceptance Criteria**:
  - `App` submits commands through an abstraction rather than directly to per-worker channels.
  - Existing tests still pass against the adapter layer.
- **Validation**:
  - `cargo test --test async_resource_state -- --nocapture`
  - `cargo test --test recorder_controls -- --nocapture`

## Sprint 2: Move Provider / Owls / Matchbook To Tokio Tasks
**Goal**: Replace detached thread workers with cancellable Tokio tasks and bounded async channels.

**Demo/Validation**:
- A stuck provider job can be cancelled and replaced cleanly.
- Restarting the recorder does not leave orphaned in-flight state.
- New runtime tests prove task restart instead of channel swapping only.

### Task 2.1: Add Tokio runtime host for the TUI app
- **Location**: `Cargo.toml`, create `src/runtime.rs`, modify `src/main.rs` or runtime bootstrap path in `src/app.rs`
- **Description**: Add a Tokio runtime owned by the application process. Decide whether to embed a multi-thread runtime behind a handle or run the app under `#[tokio::main]` while keeping Ratatui input handling predictable.
- **Dependencies**: Sprint 1
- **Acceptance Criteria**:
  - Runtime starts once per app process.
  - TUI code can submit async jobs without blocking input/render loops.
- **Validation**:
  - New runtime smoke test starts the runtime and submits a no-op command.

### Task 2.2: Replace provider worker with cancellable task supervisor
- **Location**: `src/runtime.rs`, `src/app.rs`, create tests in `tests/async_runtime.rs`
- **Description**: Convert provider background work from `thread::spawn` to Tokio task execution using bounded `mpsc` channels, `JoinHandle`, and `CancellationToken`.
- **Dependencies**: Task 2.1
- **Acceptance Criteria**:
  - Provider restart cancels the old task and awaits task termination.
  - A new provider task starts with fresh channels/state.
  - Pending provider work is replayed after restart.
- **Validation**:
  - `cargo test provider_watchdog_expiry_allows_follow_up_refresh_to_complete --test async_resource_state -- --nocapture`
  - New runtime cancellation test passes.

### Task 2.3: Convert Owls and Matchbook workers to Tokio task supervisors
- **Location**: `src/runtime.rs`, `src/app.rs`, `src/exchange_api.rs`, `src/owls.rs`
- **Description**: Move Owls and Matchbook sync loops to the same runtime model with per-task cancellation tokens and timeouts.
- **Dependencies**: Task 2.2
- **Acceptance Criteria**:
  - Owls/Matchbook tasks can be cancelled, restarted, and timed out without leaving zombie work attached to the app.
  - `ResourceState` transitions remain the source of truth for `Loading/Ready/Stale/Error`.
- **Validation**:
  - `cargo test --test async_resource_state -- --nocapture`
  - New task cancellation tests for Owls and Matchbook pass.

### Task 2.4: Remove remaining direct `std::sync::mpsc` worker control from `App`
- **Location**: `src/app.rs`
- **Description**: Make `App` consume runtime events and submit runtime commands only; raw worker handles live in the runtime layer.
- **Dependencies**: Tasks 2.2 and 2.3
- **Acceptance Criteria**:
  - `App` no longer owns per-worker sender/receiver pairs for provider/Owls/Matchbook.
  - Watchdog logic requests runtime cancellation/restart rather than manually swapping channels.
- **Validation**:
  - `cargo test --test recorder_controls -- --nocapture`
  - `cargo test --test recorder_startup_retry -- --nocapture`

## Sprint 3: Add SQLite Shared Data Store
**Goal**: Persist snapshots, sync status, and event history so state survives restarts and becomes queryable by external tools.

**Demo/Validation**:
- Starting the app with no live workers still loads last-good state from SQLite.
- External queries can read current projected state from the DB.
- Recorder startup updates tables without requiring TUI overlay rendering.

### Task 3.1: Define shared schema and migration strategy
- **Location**: Create `migrations/`, `src/store/schema.rs` or `src/store/mod.rs`, `docs/plans/async-runtime-shared-data-layer-plan.md`
- **Description**: Define SQLite tables for:
  - `sync_runs`
  - `provider_snapshots`
  - `owls_snapshots`
  - `matchbook_snapshots`
  - `projected_snapshot_cache`
  - `sync_errors`
  - optional `event_log`
- **Dependencies**: Sprint 2
- **Acceptance Criteria**:
  - Schema supports append-only history plus a fast current-state projection.
  - Tables clearly distinguish raw payloads from derived/projected state.
  - Schema assumes single writer now but leaves room for future `writer_id` / `source` fields.
- **Validation**:
  - Migration test creates a DB from scratch and verifies expected tables.

### Task 3.2: Add store layer and typed persistence API
- **Location**: Create `src/store/mod.rs`, `src/store/models.rs`, `src/store/projection.rs`, modify `src/lib.rs`
- **Description**: Add a typed SQLite access layer using SQLx, with methods for writing raw snapshots, recording sync status/errors, and reading current projected state.
- **Dependencies**: Task 3.1
- **Acceptance Criteria**:
  - Store API is isolated from UI code.
  - DB writes are atomic per sync cycle.
  - Reads can fetch last-good provider/Owls/Matchbook state independently.
- **Validation**:
  - New store tests against temp SQLite DB pass.

### Task 3.3: Persist runtime events and last-good resource payloads
- **Location**: `src/runtime.rs`, `src/app.rs`, `src/store/*`
- **Description**: On each successful or failed sync, write raw result metadata and update the last-good snapshot/projection tables.
- **Dependencies**: Task 3.2
- **Acceptance Criteria**:
  - Matchbook partial failures are persisted as errors without deleting the last good row.
  - Provider/Owls/Matchbook last-good state remains queryable after app restart.
- **Validation**:
  - New persistence regression tests pass.
  - Manual inspection of SQLite file shows updated rows after a simulated sync.

### Task 3.4: Hydrate app state from SQLite on startup
- **Location**: `src/app.rs`, `src/runtime.rs`, `src/store/*`
- **Description**: Load last-good persisted state before live syncs begin, then let runtime refresh it.
- **Dependencies**: Task 3.3
- **Acceptance Criteria**:
  - App starts with meaningful positions/overlay context even if live workers have not completed yet.
  - Startup state is marked stale vs fresh correctly.
- **Validation**:
  - New startup hydration test passes.
  - `cargo test --test recorder_startup_retry -- --nocapture`

## Sprint 4: Expose Queryable Shared State To Other Tools
**Goal**: Make the data usable elsewhere without forcing other tools to scrape the TUI or share in-memory structures.

**Demo/Validation**:
- A non-TUI consumer can query current state from SQLite.
- Overlay-relevant rows can be read without launching `sabi`.

### Task 4.1: Define external read contract
- **Location**: Create `docs/shared-state.md`, `src/store/query.rs`
- **Description**: Define stable read models for current provider snapshot, current overlay inputs, recent sync errors, and event history.
- **Dependencies**: Sprint 3
- **Acceptance Criteria**:
  - Contract distinguishes stable/public read models from internal raw payload storage.
  - Contract explicitly says writes are owned by `operator-console` in this phase.
- **Validation**:
  - Review generated docs and example queries.

### Task 4.2: Add internal query helpers or a small CLI for external consumers
- **Location**: Create `src/bin/sabi-state.rs` or add a small read-only command path, modify `Cargo.toml`
- **Description**: Provide a read-only interface to fetch current projected snapshot, overlay context, and sync health from SQLite.
- **Dependencies**: Task 4.1
- **Acceptance Criteria**:
  - Command can print current state without launching the TUI.
  - Output is structured (JSON recommended).
- **Validation**:
  - New CLI smoke test passes.
  - Manual run returns the last-good projected state.

### Task 4.3: Route UI reads through the same projection contract where practical
- **Location**: `src/app.rs`, `src/snapshot_projection.rs`, `src/store/projection.rs`
- **Description**: Reduce drift by making in-memory UI projection and persisted projection share the same rules or helper functions.
- **Dependencies**: Tasks 4.1 and 4.2
- **Acceptance Criteria**:
  - Projection logic is not duplicated in incompatible forms.
  - Overlay fields come from the same normalized projection path used for persistence.
- **Validation**:
  - `cargo test --test trading_panel_ui -- --nocapture`
  - Added parity tests between in-memory and persisted projection output.

## Sprint 5: Prepare Multi-Writer Migration Path Without Enabling It Yet
**Goal**: Avoid boxing the system into a single-process design while keeping the first implementation safe.

**Demo/Validation**:
- The schema and runtime docs explain how `bet-recorder` or another process could become a writer later.
- No current code path requires multiple writers to work now.

### Task 5.1: Add writer/source metadata to schema and events
- **Location**: `migrations/`, `src/store/models.rs`
- **Description**: Include `source`, `writer_id`, and monotonic timestamps/sequence fields where needed.
- **Dependencies**: Sprint 3
- **Acceptance Criteria**:
  - Later multi-writer ingestion can distinguish producer origin.
  - Single-writer mode remains simple.
- **Validation**:
  - Migration tests updated.

### Task 5.2: Document multi-writer upgrade path
- **Location**: `docs/shared-state.md`, this plan file
- **Description**: Document the next-step architecture if `bet-recorder` or another process must write directly (e.g. append-only event ingestion, leader writer service, or WAL-backed serialized writes).
- **Dependencies**: Task 5.1
- **Acceptance Criteria**:
  - Clear non-goals for the first rollout.
  - Clear upgrade path for future direct writers.
- **Validation**:
  - Manual review of docs.

## Testing Strategy
- Preserve and extend current focused suites:
  - `cargo test --test async_resource_state -- --nocapture`
  - `cargo test --test recorder_controls -- --nocapture`
  - `cargo test --test recorder_startup_retry -- --nocapture`
  - `cargo test --test recorder_process_supervisor -- --nocapture`
  - `cargo test --test trading_panel_ui -- --nocapture`
- Add new suites:
  - `tests/async_runtime.rs` for cancellation/restart semantics
  - `tests/store_sqlite.rs` for migrations, persistence, and startup hydration
  - optional CLI/query smoke tests for external state access
- Add at least one end-to-end test per sprint that demonstrates a runnable increment.
- During manual validation, capture tracing output and SQLite contents from a real recorder start attempt.

## Potential Risks & Gotchas
- **Tokio does not magically cancel blocking work**: `spawn_blocking` tasks cannot be force-aborted once running. Avoid treating blocking adapters as cancellable; prefer async HTTP and short timeouts at the source.
- **Ratatui + Tokio integration**: be careful not to block the input/render loop while waiting on async tasks. Keep UI rendering synchronous and runtime interaction message-based.
- **SQLite write contention**: even in single-writer mode, long transactions will stall reads. Keep writes short and use a projection/cache table for fast reads.
- **Schema drift**: if UI projection and persisted projection diverge, external consumers will see inconsistent state. Share projection helpers where possible.
- **Recorder boundary confusion**: this phase does not eliminate the external `bet-recorder` process. The runtime refactor is inside `operator-console`; recorder process supervision remains separate.
- **Multi-writer temptation**: do not let future-reader access turn into ad hoc direct writes before sequence/ownership rules exist.

## Rollback Plan
- Keep the current `std::thread` worker implementation behind a temporary feature flag or adapter during the migration.
- Land Tokio runtime introduction before deleting the old worker code paths.
- Keep SQLite hydration optional until parity tests pass.
- If the shared data layer causes instability, disable startup hydration first while retaining persistence for debugging.
- If the async runtime refactor destabilizes recorder startup, revert only the runtime adapter layer and keep the `ResourceState` / projection fixes already landed.
