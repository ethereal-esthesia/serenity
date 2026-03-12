# Serenity

Indexed-color render engine for simple 3D geometries.

Serenity is built around a small, deterministic rendering core with a focus on:

- Accurate motion
- Fluid animation
- Clean visual output
- Transparent surface support

## Vision

Serenity targets low-complexity 3D scenes where clarity and motion quality matter more than heavyweight rendering features.

## Core Principles

- Indexed color pipeline
- Stable frame-to-frame behavior
- Transparency as a first-class feature
- Predictable camera and object motion models

## Initial Scope

- Simple geometry primitives (points, lines, triangles, boxes)
- Indexed palette management
- Alpha/transparency compositing
- Time-stepped motion update loop
- Fast 16-bit PRNG for simulation/update loops

## Status

Project scaffold is initialized. Engine modules and render loop are in progress.

## Run

```bash
cd /Users/shane/Project/serenity
cargo run --release
```

Default run is an animated neon pattern renderer using the project palette and debanding filter pipeline.

For smooth runtime performance, use release mode:

```bash
cd /Users/shane/Project/serenity
cargo run --release
```

Controls:
- `Esc`: quit
- HUD input debug (top-left):
  - `KEYS`: currently pressed non-modifier keys (chord-friendly)
  - `MODS`: currently pressed modifier keys (left/right variants)
  - On macOS, app attempts global key capture via `CGEventTap` (system-wide while running).
  - If macOS permissions are missing, it falls back to local window-focused capture and prints a status line.

Optional screenshot output:

```bash
cd /Users/shane/Project/serenity
cargo run -- --screenshot /tmp/serenity_main.ppm
```

Optional debug output (init + fps):

```bash
cd /Users/shane/Project/serenity
cargo run -- --debug
```

Optional HUD font override (Cascadia Mono):
- Place a TTF file at `assets/fonts/CascadiaMono-Regular.ttf` (or `assets/fonts/CascadiaMono.ttf`).
- If found, HUD text uses SDL_ttf + Cascadia Mono.
- If not found, HUD falls back to built-in 5x7 bitmap text.
- Bundled Cascadia font licensing: SIL OFL 1.1 (see `assets/fonts/CASCADIA-LICENSE.txt`).

Interactive noise test (non-default):

```bash
cd /Users/shane/Project/serenity
cargo run --bin noise_texture_test
```

Optional screenshot output:

```bash
cd /Users/shane/Project/serenity
cargo run --bin noise_texture_test -- --screenshot /tmp/serenity_noise.ppm
```

## Control Flow

See [`docs/runtime-input-control-flow.md`](docs/runtime-input-control-flow.md) for the main runtime/input diagram and walkthrough.
See [`docs/global-input-permission-flow.md`](docs/global-input-permission-flow.md) for the OS permission + silent fallback flow.

## Test

```bash
cd /Users/shane/Project/serenity
cargo test
```

See [TESTING.md](./TESTING.md) for full test and data-dump examples.
