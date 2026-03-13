# Frame Buffer Provider API

This API hides pool/ring internals behind a simple producer/consumer contract so runtime code does not depend on buffer strategy details.

## Goals

1. Keep SDL/render backend code out of worker threads.
2. Let `main` configure buffering policy (2/3/N) without leaking internals.
3. Keep worker code using a stable interface: `get_next_frame_buffer()` + publish.

## Contract

Producer-side:

1. `get_next_frame_buffer()` acquires the next writable frame buffer (if available).
2. Producer writes pixels only to the acquired write frame.
3. Producer may attach raw timing markers using the project timestamp type:
   - `request_sim_time: InputTimestamp`
   - `compute_start: InputTimestamp`
   - `compute_end: InputTimestamp`
3. `publish_frame(write_frame)` marks the frame as latest completed output.

Consumer-side:

1. `get_latest_frame()` returns the latest published frame snapshot.
2. Consumer should treat returned data as immutable.
3. `get_latest_frame_after(last_sequence)` returns only a strictly newer frame, enabling non-blocking "continue while waiting" loops without reprocessing the same frame.
4. Consumers compute deltas/jitter from raw timestamps; provider does not precompute durations.

## Current Rust Types

- `FrameBufferSource` trait in `src/runtime/frame_buffer.rs`
- `FrameBufferPool` implementation with configurable `buffer_count`
- `WriteFrameBuffer` / `ReadFrameBuffer` transport types

## Notes

1. The current `get_latest_frame()` returns an owned copy of pixels for safety and simplicity.
2. Later optimization can switch to zero-copy handles while keeping the same trait contract.
