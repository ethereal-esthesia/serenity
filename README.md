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
cargo run
```

Controls:
- `Esc`: quit
- `Space`: toggle noise mode (`Linear` / `Gaussian`)

## Test

```bash
cd /Users/shane/Project/serenity
cargo test
```

See [TESTING.md](./TESTING.md) for full test and data-dump examples.
