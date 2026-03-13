# Key Input Worker Refactor (Quick Plan)

Status: high-effort architecture refactor (engine boundary + input pipeline ownership)

## Goal

Make input state updates thread-safe and atomic in a dedicated input worker, so render/main-thread cadence does not control key-resolution logic.

## Why This Matters

This is a primary motivation for the library:

1. Game/local lag spikes should not distort input semantics (stuck/late keys causing exaggerated motion).
2. Input must remain robust if render/update slows down.
3. If the app hangs/crashes, users must still be able to exit/control the OS (no key capture deadlock).

## Problem

Current probe lock path depends on SDL keydown handling in the main loop. Under low FPS / heavy load, local SDL events arrive too late for a 4ms probe window, causing repeated probe timeout drops.

## Non-Negotiable Guarantees

1. Input processing is decoupled from render cadence.
2. Input worker is the only writer of input state (atomic transitions).
3. Main thread is read-only for input state.
4. Capture/swallow is fail-open:
   - if consumer stalls or process state is unhealthy, stop swallowing and allow OS passthrough.
5. Escape/exit-critical handling must not depend on render timing.

## Target Architecture

1. CG/HID callback thread captures raw events.
2. SDL main-thread pump only publishes local SDL keydown facts into a queue.
3. Input worker thread consumes queued local facts and updates shared input state atomically.
4. Main thread only reads snapshots/events and renders HUD.

## Phase 1 (this change)

1. Add `local_sdl_keydown` queue in `GlobalInputCapture`.
2. Move `on_local_sdl_keydown_for_probe` probe-eval logic into a background resolver thread.
3. Keep current event model and probe gate semantics unchanged for now.

Status:
- Implemented queue + resolver thread.
- Main thread now enqueues SDL keydown probe facts; probe lock state mutation runs off main.

## Phase 2

1. Remove probe-gate dependency on tight time windows.
2. Emit unresolved keys immediately and backfill aliases asynchronously.
3. Stop blocking/hiding key stream while alias resolution is pending.
4. Ensure sanitized event stream remains monotonic and duplicate-safe.

## Phase 3

1. Consolidate all mutation paths through worker-owned state transitions.
2. Keep shared state read-mostly for snapshots/events.
3. Add deterministic test harness with simulated interleavings/load.
4. Add watchdog/heartbeat:
   - worker tracks consumer liveness
   - on stale consumer, disable swallow and clear held state (fail-open path).

Status:
- Heartbeat + watchdog fail-open is now implemented:
  - main reports consumer heartbeat each frame
  - watchdog trips fail-open on stale heartbeat and clears runtime-held input state
  - capture resumes automatically when heartbeat recovers

## Execution Checklist

- [x] Queue local SDL probe facts from main (non-blocking publish only)
- [x] Resolve probe lock in dedicated worker path
- [x] Add heartbeat/watchdog fail-open safety path
- [ ] Remove short-window probe-gate coupling
- [ ] Emit unresolved events immediately and backfill aliases asynchronously
- [ ] Make engine/input worker the only writer of input state
- [ ] Add deterministic interleaving/stall regression tests
