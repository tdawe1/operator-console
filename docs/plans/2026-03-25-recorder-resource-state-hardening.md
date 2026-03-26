# Recorder Resource State Hardening Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Prevent `sabi` from appearing frozen during recorder startup or background syncs, while preserving the last good provider/Owls/Matchbook data so the live overlay stays populated instead of degrading to partial emptiness.

**Architecture:** Introduce explicit async resource state for the three background feeds (`provider`, `owls`, `matchbook`) instead of treating "in flight" as a bare boolean. Add watchdog expiry so blocked jobs become `Stale`/`Error` rather than holding the UI in a perpetual waiting state. Keep normalized snapshot projection pure and rebuild it from the last good resource payloads instead of replacing good data with partial refresh results.

**Tech Stack:** Rust 2021, Ratatui, std `mpsc`, blocking `reqwest`, existing `cargo test` integration tests.

---

### Task 1: Lock In The Regression With Failing Async-State Tests

**Files:**
- Create: `tests/async_resource_state.rs`
- Modify: `src/app.rs`
- Test: `tests/async_resource_state.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn stuck_matchbook_sync_expires_and_preserves_last_good_data() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut app = test_app_with_snapshot(temp_dir.path().join("recorder.json"));
    app.set_matchbook_state_for_test(sample_matchbook_state("good"));
    app.mark_matchbook_sync_in_flight_for_test(Instant::now() - Duration::from_secs(60));

    app.poll_matchbook_account();

    assert!(!app.matchbook_sync_in_flight_for_test());
    assert_eq!(app.matchbook_status_for_test(), "stale");
    assert_eq!(app.matchbook_account_state().unwrap().status_line, "good");
}

#[test]
fn stuck_owls_sync_expires_and_keeps_live_context_targets() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut app = test_app_with_live_snapshot(temp_dir.path().join("recorder.json"));
    app.set_owls_dashboard_for_test(sample_ready_owls_dashboard());
    app.mark_owls_sync_in_flight_for_test(Instant::now() - Duration::from_secs(60));

    app.poll_owls_dashboard();

    assert!(!app.owls_sync_in_flight_for_test());
    assert!(!app.snapshot().external_live_events.is_empty());
}

#[test]
fn stuck_provider_request_expires_and_allows_next_refresh() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut app = test_app_with_snapshot(temp_dir.path().join("recorder.json"));
    app.mark_provider_in_flight_for_test(Instant::now() - Duration::from_secs(60));

    app.poll_recorder();

    assert!(!app.provider_in_flight_for_test());
    assert!(app.provider_pending_debug_label().is_none());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test async_resource_state -- --nocapture`

Expected: FAIL because the helper methods and watchdog behavior do not exist yet.

**Step 3: Write minimal implementation**

Add the smallest internal test hooks and placeholder expiry checks in `src/app.rs` so the tests compile and fail on the real assertions rather than missing symbols.

```rust
#[cfg(test)]
impl App {
    fn mark_matchbook_sync_in_flight_for_test(&mut self, started_at: Instant) {
        self.matchbook_sync_in_flight = true;
        self.matchbook_sync_started_at = Some(started_at);
    }
}
```

**Step 4: Run test to verify it still fails for the right reason**

Run: `cargo test --test async_resource_state -- --nocapture`

Expected: FAIL on stale-state assertions, not on compilation errors.

**Step 5: Commit**

```bash
git add tests/async_resource_state.rs src/app.rs
git commit -m "test: capture stuck async resource regressions"
```

### Task 2: Add Explicit Resource State And Watchdog Expiry

**Files:**
- Create: `src/resource_state.rs`
- Modify: `src/app.rs`
- Modify: `src/lib.rs`
- Test: `tests/async_resource_state.rs`

**Step 1: Write the failing test**

Add a narrow unit test for the new state type.

```rust
#[test]
fn resource_state_expires_loading_to_stale_without_dropping_last_good() {
    let mut state = ResourceState::ready(String::from("payload"));
    state.begin_refresh(Instant::now() - Duration::from_secs(60));

    state.expire_if_overdue(Duration::from_secs(5), "timeout");

    assert_eq!(state.phase(), ResourcePhase::Stale);
    assert_eq!(state.last_good().map(String::as_str), Some("payload"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test resource_state_expires_loading_to_stale_without_dropping_last_good -- --nocapture`

Expected: FAIL because `ResourceState` and `ResourcePhase` do not exist yet.

**Step 3: Write minimal implementation**

Create `src/resource_state.rs`.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourcePhase {
    Idle,
    Loading,
    Ready,
    Stale,
    Error,
}

#[derive(Debug, Clone)]
pub struct ResourceState<T> {
    phase: ResourcePhase,
    last_good: Option<T>,
    loading_started_at: Option<Instant>,
    last_error: Option<String>,
}
```

Wire `App` to use `ResourceState<ExchangePanelSnapshot>`, `ResourceState<OwlsDashboard>`, and `ResourceState<MatchbookAccountState>` instead of naked in-flight bookkeeping for lifecycle decisions.

**Step 4: Run tests to verify they pass**

Run: `cargo test --test async_resource_state -- --nocapture`

Expected: PASS for the new state-type tests and the original stuck-job expiry tests.

**Step 5: Commit**

```bash
git add src/resource_state.rs src/app.rs src/lib.rs tests/async_resource_state.rs
git commit -m "feat: add async resource state watchdogs"
```

### Task 3: Preserve Last-Good Matchbook Data On Partial API Failures

**Files:**
- Modify: `src/exchange_api.rs`
- Modify: `src/app.rs`
- Test: `tests/async_resource_state.rs`

**Step 1: Write the failing test**

Add a focused regression test beside existing `exchange_api.rs` tests.

```rust
#[test]
fn matchbook_loader_reports_partial_failure_instead_of_silent_empty_success() {
    let client = stub_matchbook_client()
        .with_account_ok()
        .with_balance_ok()
        .with_current_offers_error("boom")
        .with_current_bets_ok()
        .with_positions_ok();

    let result = load_matchbook_account_state_with_client(&client);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("current offers"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test matchbook_loader_reports_partial_failure_instead_of_silent_empty_success -- --nocapture`

Expected: FAIL because the current loader returns `Ok(...)` with empty vectors.

**Step 3: Write minimal implementation**

Update `src/exchange_api.rs:312` so account-state loading returns a typed error when any required endpoint fails.

```rust
let current_offers = client
    .current_offers()
    .wrap_err("Matchbook current offers failed")?;
let current_bets = client
    .current_bets()
    .wrap_err("Matchbook current bets failed")?;
let positions = client
    .positions()
    .wrap_err("Matchbook positions failed")?;
```

Then update `src/app.rs` so a failed Matchbook sync leaves `last_good` untouched and only updates the resource status/error message.

**Step 4: Run tests to verify they pass**

Run: `cargo test matchbook_loader_reports_partial_failure_instead_of_silent_empty_success -- --nocapture && cargo test --test async_resource_state -- --nocapture`

Expected: PASS, with the app preserving prior Matchbook data after a failed refresh.

**Step 5: Commit**

```bash
git add src/exchange_api.rs src/app.rs tests/async_resource_state.rs
git commit -m "fix: preserve last good matchbook state on sync errors"
```

### Task 4: Move Snapshot Enrichment Behind A Pure Projector And Verify Overlay Inputs

**Files:**
- Create: `src/snapshot_projection.rs`
- Modify: `src/app.rs`
- Modify: `src/panels/trading_positions.rs`
- Test: `tests/trading_panel_ui.rs`
- Test: `tests/async_resource_state.rs`

**Step 1: Write the failing test**

Add an overlay-oriented regression.

```rust
#[test]
fn live_view_overlay_uses_last_good_enrichment_when_background_sync_is_stale() {
    let mut app = build_live_overlay_app();
    app.inject_last_good_owls_dashboard(sample_ready_owls_dashboard());
    app.inject_last_good_matchbook_state(sample_matchbook_state("ready"));
    app.expire_background_resources_for_test();
    app.set_trading_section(TradingSection::Positions);
    app.toggle_live_view_overlay();

    let rendered = render_app_to_string(&mut app);

    assert!(rendered.contains("Live Context"));
    assert!(rendered.contains("Matchbook"));
    assert!(!rendered.contains("No Owls live match context"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test positions_live_view_overlay_uses_last_good_enrichment_when_background_sync_is_stale -- --nocapture`

Expected: FAIL because enrichment is rebuilt ad hoc inside `App` and stale resources are not preserved as first-class inputs.

**Step 3: Write minimal implementation**

Create `src/snapshot_projection.rs` with a pure projector.

```rust
pub fn project_snapshot(
    base: &ExchangePanelSnapshot,
    owls: Option<&OwlsDashboard>,
    matchbook: Option<&MatchbookAccountState>,
) -> ExchangePanelSnapshot {
    let mut snapshot = base.clone();
    populate_snapshot_enrichment(&mut snapshot, owls.unwrap_or(&OwlsDashboard::default()), matchbook);
    snapshot
}
```

Then make `App` keep an authoritative provider snapshot plus independent resource states, and rebuild the rendered snapshot from the projector. Keep `trading_positions.rs` rendering unchanged apart from reading the already-projected snapshot.

**Step 4: Run tests to verify they pass**

Run: `cargo test --test trading_panel_ui -- --nocapture && cargo test --test async_resource_state -- --nocapture`

Expected: PASS, with the overlay still populated from last-good enrichment when refreshes are stale or errored.

**Step 5: Commit**

```bash
git add src/snapshot_projection.rs src/app.rs src/panels/trading_positions.rs tests/trading_panel_ui.rs tests/async_resource_state.rs
git commit -m "refactor: project overlay data from explicit resource state"
```

### Task 5: Final Verification

**Files:**
- Modify: `src/app.rs`
- Test: `tests/async_resource_state.rs`
- Test: `tests/trading_panel_ui.rs`
- Test: `tests/recorder_startup_retry.rs`

**Step 1: Run the focused regression suite**

Run: `cargo test --test async_resource_state -- --nocapture && cargo test --test trading_panel_ui -- --nocapture && cargo test --test recorder_startup_retry -- --nocapture`

Expected: PASS.

**Step 2: Run the broader recorder-focused suite**

Run: `cargo test --test recorder_controls -- --nocapture && cargo test --test recorder_process_supervisor -- --nocapture`

Expected: PASS.

**Step 3: Run formatting and compile checks**

Run: `cargo fmt --check && cargo test poll_recorder_refreshes_running_recorder_automatically -- --nocapture`

Expected: PASS.

**Step 4: Manual verification**

Run: `RUST_BACKTRACE=1 cargo run`

Expected: starting the recorder no longer leaves the app in a stuck in-flight state, and opening the live overlay still shows the last good Owls/Matchbook context when a background fetch times out.

**Step 5: Commit**

```bash
git add src/app.rs tests/async_resource_state.rs tests/trading_panel_ui.rs tests/recorder_startup_retry.rs
git commit -m "fix: harden recorder async state and preserve overlay context"
```
