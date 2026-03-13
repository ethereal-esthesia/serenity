# Serenity Engine Checklist

Status: high-effort architectural refactor (not a low-effort cleanup)

## A) Architecture Boundary (Main vs Engine)
- [ ] `main.rs` is orchestration-only (init, pump, tick, render, present, shutdown)
- [ ] No input-domain mutation logic remains in `main.rs`
- [ ] Introduce `RuntimeEngine`/`AppEngine` facade for loop-facing operations
- [ ] Add a CI/assertion check to prevent architecture regressions

## B) Input Pipeline Decoupling
- [x] Local SDL keydown probe facts are queued from main
- [x] Probe evaluation/lock runs off main thread
- [ ] Eliminate probe-gate dependency on short timing windows
- [ ] Emit unresolved events immediately, backfill alias asynchronously
- [ ] Guarantee monotonic sanitized event stream with duplicate suppression

## C) Safety and Fail-Open Behavior
- [x] Watchdog + heartbeat mechanism exists
- [x] Fail-open path clears held state on stale consumer heartbeat
- [ ] Add explicit diagnostics for fail-open enter/exit in HUD/panel
- [ ] Add recovery tests proving passthrough resumes correctly after stall

## D) Threading and Atomicity
- [ ] Single writer model for input state transitions is enforced
- [ ] All shared state mutations route through engine/input worker transitions
- [ ] Main thread consumes snapshots/events as read-only
- [ ] Add stress tests for lock contention/interleaving ordering

## E) Test and Verification
- [ ] Deterministic replay tests for key-down/up/modifier ordering
- [ ] Probe timeout/late SDL arrival regression test
- [ ] Watchdog stale-heartbeat regression test
- [ ] End-to-end test proving escape/exit path is independent of render cadence

## F) Docs and Maintainability
- [x] Engine/main separation flow documented
- [x] Input worker refactor document exists
- [ ] Keep README architecture section aligned with actual implementation
- [ ] Keep diagrams/checklists updated with each completed phase
