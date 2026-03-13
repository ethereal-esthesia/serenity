#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

echo "[test_interpolation] running interpolation unit tests"
cargo test --lib frame_interpolator -- --nocapture

echo "[test_interpolation] running interpolation integration tests"
cargo test --test interpolation_sim -- --nocapture

echo "[test_interpolation] done"
