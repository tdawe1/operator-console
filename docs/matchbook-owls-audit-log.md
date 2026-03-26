# Matchbook + Owls Audit Log

Last updated: 2026-03-25

## Operating Brief

- Work autonomously until the Matchbook, Owls, and TUI implementations are functional and usable.
- Do not rush. Do not cut corners.
- Keep a running audit log and update it frequently.
- When implementation appears complete, initiate a review pass instead of declaring done.
- If all scoped work is exhausted, perform a fresh-context audit using this log as context.

## Reminder Checklist

- Re-read the user brief at regular intervals.
- Prefer documented behavior over inference.
- Promote typed data models over preview-text matching when practical.
- Verify each material change with targeted tests or focused inspection.
- Preserve unrelated user changes in the repo.

## Current Scope

Target repo: `/home/thomas/projects/sabi/console/operator-console`

Primary areas:

1. Matchbook session/auth/rate-limit handling.
2. Owls sport selection, quote matching, and documented field coverage.
3. Positions live view, stats, markets/live/props panels, and any other TUI surface using Matchbook/Owls data.
4. Review loops after implementation, not just direct patches.

## Findings So Far

### Closed

- Matchbook env loading now falls back to `~/.env`, `~/.env.local`, and ancestor dotenv files.
- Owls sport now auto-switches from the default `nba` context when snapshot data clearly implies `soccer`.
- Positions live view layout is denser and promotes best available exit pricing.
- Hold/lock downside coloring now follows actual P/L sign instead of hardcoded green.
- Matchbook sync worker now reuses a client/session, retries on `401`, and backs off after `429`.
- Background Matchbook polling no longer includes aggregated matched bets on every account refresh.
- Owls odds/realtime parsing now preserves typed quote rows with book, market, selection, decimal price, point, Pinnacle limits, and freshness metadata.
- Sharp/Pinnacle matching for alerts and the positions live overlay no longer depends on preview-text scraping.
- Markets/Live/Props panels now surface quote counts, returned books, and freshness metadata instead of acting like a generic endpoint monitor.
- Owls odds sync no longer hard-filters to `pinnacle,bet365,betmgm`; where the docs allow it, the console now requests the full book surface by default.
- Matchbook stats and action-overlay surfaces now use `current_bets` and `positions` in addition to `current_offers`, reducing obvious underuse of the account-state payload we already fetch.
- Matchbook review/preflight now has an explicit `Get Prices` hook for action flows that already carry `event_id`, `market_id`, and `runner_id` from OddsMatcher-style sources.
- Event, market, and selection matching no longer live in three drifting copies across `app.rs`, `owls.rs`, and `trading_positions.rs`; there is now one shared normalization module for cross-source identity.
- The shared matcher now handles `vs`/`v`/`@` event forms, reversed home-away order, `X`/`Draw` aliases, `1`/`2` vs team-name aliases, and Owls `h2h` vs sportsbook `Full-time result` / `Match Betting`.
- The positions live view now resolves the Malta/Luxembourg-style alias case end to end for Pinnacle/Owls and Matchbook-linked rows instead of silently degrading to blanks.
- Review pass found one remaining alias blind spot in `watch_row_event_name`; sharp-watch alert lookups now use the shared selection aliases there as well.
- Review pass also found a live-overlay hot path: Matchbook and Owls quote scans were being recomputed several times per frame. The overlay now computes those quotes once and reuses them across the boards, reducing freeze risk against large quote surfaces.
- Review pass found a larger runtime regression in the main TUI shell: `ui.rs` was cloning the full snapshot, Owls dashboard, and Matchbook account state on every `Trading > Positions` frame just to satisfy borrow constraints. With typed external quote/live-event payloads in the snapshot, that per-frame cloning was enough to make the live view freeze and crash under real data. Positions rendering now uses a split-borrow render context instead of cloning those payloads every frame.
- Selected-row summary, signal, and overlay rendering now share one matched external-quote cache per frame instead of rescanning `snapshot.external_quotes` separately for Sharp, Matchbook, best-exit, and venue rows.
- Recorder startup now applies a short bootstrap alert mute for noisy background rules such as tracked-bet growth, exit recommendations, snapshot stale, Owls errors, Matchbook failures, and sharp-watch alerts. This prevents pressing `s` from immediately spraying unrelated bootstrap notifications before the first post-start snapshot has settled, while keeping genuine recorder/process failures visible.
- `ExchangePanelSnapshot` now carries typed `external_quotes` and `external_live_events`, so positions/live overlay consumers no longer have to scrape Owls previews or Matchbook side-state directly.
- App-level snapshot enrichment now projects Smarkets snapshot odds, Owls multi-book quotes, and Matchbook account identifiers into one normalized quote surface keyed to the active positions transport model.
- The positions overlay now reads its Matchbook, Smarkets, Betfair, Betdaq, Pinnacle/sharp, best-exit, and liquidity values from `snapshot.external_quotes` instead of mixing direct Owls lookups with `current_offers`-only Matchbook probes.
- Positions action seeds now pick up `event_id`, `market_id`, `selection_id`, and deep-link metadata from the structured external quote surface when those identifiers exist.
- Owls scores parsing now emits typed soccer live events with status, scoreline, match stats, incidents, and player ratings instead of flattening the endpoint into preview text only.
- App-level enrichment now maps those Owls live events onto snapshot rows, updating live score/clock state and exposing structured live context for the overlay.
- Owls now uses the documented team-normalization batch API for soccer/tennis team aliases discovered in quote and live-score payloads, and quote/score matching can fall back to canonicalized team-pair comparison when raw strings differ.

### Still Open

- Live view still degrades to `-` when upstream quote matching fails; source failure states need clearer surfacing.
- Need systematic audit of every TUI panel touching `owls_dashboard` or `matchbook_account_state`.
- Need broader review of how much of Owls docs and Matchbook docs are actually reflected in the current data model.
- Need to evaluate whether the current Matchbook polling cadence is still too aggressive even after session reuse and lighter report usage.
- Matchbook panels still underuse `current_bets` and `positions`; most TUI rendering still stops at current offers plus a coarse summary.
- Matchbook still does not call the documented market-data prices endpoint, so “current Matchbook odds” in the positions workflow is not a real market quote yet.
- The positions workflow usually lacks Matchbook market/selection identifiers, so even a prices integration needs transport/model work to become reliable.
- The snapshot transport now has typed external quote/live-event rows, but it still has no no-vig/fair-odds model and no reusable per-book market-state abstraction beyond quoted prices/liquidity.
- Owls still uses REST polling only. The docs expose WebSocket push updates for odds and a dedicated `pinnacle-realtime` stream; the console is not using that yet.

## Docs / Source Notes

### Matchbook official docs

- Login docs: session token should be reused; sessions live for about 6 hours.
- FAQ docs: rate limits include `Security` 200 req/min and `Reports` 40 req/min for standard usage.
- The API reference exposes market-data endpoints such as `Get Prices`, separate from account/reports endpoints.

### Owls official docs

- Odds responses are keyed by book.
- Soccer is a supported sport.
- Pinnacle is a first-class book in odds responses.
- Pinnacle markets can include `limits`.
- Odds responses use normalized multi-book schemas, but prices in examples are American-style values such as `-150` and `130`; the console now converts those into decimal odds for decision logic.
- `books` is an optional filter, not required. Using a fixed subset underutilizes the API.
- `meta` includes `requestedBooks`, `availableBooks`, `booksReturned`, and `freshness`.
- REST rate limits apply per plan, but WebSocket push updates are unlimited once connected.
- A dedicated `pinnacle-realtime` WebSocket event is documented for automatic real-time updates.
- Team normalization API supports single and batch canonicalization for soccer/tennis team aliases; the console now uses the batch endpoint for discovered team names during dashboard sync.
- Live scores for soccer include `matchStats`, `incidents`, and `playerStats`; the console now parses those into typed rows and surfaces them through the live overlay context block.
- Official TypeScript SDK exists, but Context7 did not resolve a usable official library entry in this environment.
- Runtime regression found and fixed in the Positions TUI path: `ui.rs` was cloning the full snapshot, Owls dashboard, and Matchbook state every frame before rendering `Trading > Positions`, which became materially more expensive after `external_quotes` and `external_live_events` were added.
- The Positions render path now uses split-borrow state from `App` and reuses selected-row quote matches plus precomputed half/lock outcomes inside the live overlay instead of rescanning the same enrichment vectors repeatedly during one frame.

## Verification Log

- `cargo test --test trading_panel_ui positions_live_view_overlay_renders_cashout_and_matrix -- --nocapture`
- `cargo test replace_snapshot_auto_switches_default_owls_sport_for_soccer_positions -- --nocapture`
- `cargo test dotenv_value_reader_supports_home_style_env_files -- --nocapture`
- `cargo test matchbook_error_status_detection_matches_embedded_http_codes -- --nocapture`
- `cargo test parse_book_market_summary_extracts_quote_rows_and_metadata -- --nocapture`
- `cargo test live_sharp_opportunity_helper_detects_crossing_threshold -- --nocapture`
- `cargo test --test trading_panel_ui -- --nocapture`
- `cargo test matchbook_event_id_reader_uses_event_path_segment -- --nocapture`
- `cargo test matchbook_best_prices_parser_prefers_best_back_and_lay -- --nocapture`
- `cargo test native_provider_routes_matchbook_actions_to_api_runner -- --nocapture`
- `cargo test event_matching_handles_reversed_home_away_formats -- --nocapture`
- `cargo test live_view_matches_matchbook_and_sharp_quotes_across_source_aliases -- --nocapture`
- `cargo check --tests --quiet`
- `cargo test snapshot_enrichment_projects_owls_quotes_and_live_scores -- --nocapture`
- `cargo test parse_scores_summary_extracts_soccer_live_context -- --nocapture`
- `cargo check --tests --quiet`
- `cargo test --test trading_panel_ui -- --nocapture`
- `cargo test recorder_startup_suppresses_bootstrap_tracked_bet_alerts -- --nocapture`
- `cargo test tracked_bet_alerts_resume_after_recorder_startup_snapshot -- --nocapture`
- `cargo fmt`
- `cargo check --quiet`
- `cargo test --quiet --test trading_panel_ui -- --nocapture`
- `cargo test --quiet live_view_matches_matchbook_and_sharp_quotes_across_source_aliases -- --nocapture`

## Next Pass

1. Audit Matchbook parsing against the official response shapes to confirm we are not still dropping usable identifiers such as event links, richer side/status detail, or additional market metadata.
2. Wire a rate-safe path from Matchbook account rows or persisted action context into real `Get Prices` market quotes so the positions workflow stops relying on Owls-only Matchbook pricing when direct market IDs are available.
3. Add source-health/error rows plus no-vig/fair-odds math to the new external quote transport so blank or partial boards explain exactly what is missing.
4. Revisit Owls transport again after the Matchbook pass and assess whether WebSocket adoption is now the highest-value next step.
