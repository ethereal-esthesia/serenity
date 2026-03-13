# Serenity

Serenity Engine: indexed-color runtime engine focused on deterministic rendering and decoupled input processing.

Serenity Engine is built around a small, deterministic runtime core with a focus on:

- Accurate motion
- Fluid animation
- Clean visual output
- Transparent surface support

## Vision

Serenity Engine targets low-complexity 3D scenes where clarity and motion quality matter more than heavyweight rendering features, while enforcing input/runtime behavior that is independent of render-loop jitter.

## Core Principles

- Indexed color pipeline
- Stable frame-to-frame behavior
- Transparency as a first-class feature
- Predictable camera and object motion models
- Input processing decoupled from render cadence
- Fail-open input safety under stalls

## Initial Scope

- Simple geometry primitives (points, lines, triangles, boxes)
- Indexed palette management
- Alpha/transparency compositing
- Time-stepped motion update loop
- Fast 16-bit PRNG for simulation/update loops

## Status

Project scaffold is initialized. Runtime engine modules are in progress, including active refactor toward thin-main orchestration and engine-owned input state transitions.

## Setup (macOS)

Bootstrap required dependencies:

```bash
cd /Users/shane/Project/serenity
./scripts/macos/bootstrap.sh
```

Bootstrap without HUD TTF dependency (opt-out path):

```bash
cd /Users/shane/Project/serenity
WITH_TTF=0 ./scripts/macos/bootstrap.sh
```

Validate environment at any time:

```bash
cd /Users/shane/Project/serenity
./scripts/macos/doctor.sh
```

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
- By default, HUD text uses SDL_ttf + Cascadia Mono when the font is present.
- If not found, HUD falls back to built-in 5x7 bitmap text.
- Bundled Cascadia font licensing: SIL OFL 1.1 (see `assets/fonts/CASCADIA-LICENSE.txt`).
- Build with HUD TTF path explicitly:

```bash
cd /Users/shane/Project/serenity
cargo run --features hud_ttf -- --debug
```

Opt-out run path (disable default TTF feature):

```bash
cd /Users/shane/Project/serenity
cargo run --no-default-features -- --debug
```

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
See [`docs/engine-main-separation-flow.md`](docs/engine-main-separation-flow.md) for thin-main engine architecture.
See [`docs/key-input-worker-refactor.md`](docs/key-input-worker-refactor.md) for input worker/fail-open refactor plan.
See [`docs/serenity-engine-checklist.md`](docs/serenity-engine-checklist.md) for the high-effort execution checklist.

## Test

```bash
cd /Users/shane/Project/serenity
cargo test
```

See [TESTING.md](./TESTING.md) for full test and data-dump examples.

## CI Prep

For a future GitHub macOS build, install the same native deps used locally:

```bash
brew install sdl3
# optional, only if building with --features hud_ttf
brew install sdl3_ttf
```
