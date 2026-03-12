#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WITH_TTF="${WITH_TTF:-0}" # 1 => install SDL3_ttf for --features hud_ttf builds
SKIP_BREW_UPDATE="${SKIP_BREW_UPDATE:-0}"

if ! command -v brew >/dev/null 2>&1; then
  cat <<'EOF' >&2
Homebrew is required for native dependencies.
Install from: https://brew.sh
Then rerun: scripts/macos/bootstrap.sh
EOF
  exit 1
fi

if [[ "${SKIP_BREW_UPDATE}" != "1" ]]; then
  echo "[bootstrap] brew update"
  brew update
fi

echo "[bootstrap] install required dependencies: sdl3"
brew install sdl3

if [[ "${WITH_TTF}" == "1" ]]; then
  echo "[bootstrap] install optional HUD font dependency: sdl3_ttf"
  brew install sdl3_ttf
fi

if ! command -v cargo >/dev/null 2>&1; then
  cat <<'EOF' >&2
Rust toolchain missing.
Install from: https://rustup.rs
Then rerun: scripts/macos/bootstrap.sh
EOF
  exit 1
fi

echo "[bootstrap] running environment doctor"
"${ROOT_DIR}/scripts/macos/doctor.sh"

echo "[bootstrap] done"
if [[ "${WITH_TTF}" == "1" ]]; then
  echo "[bootstrap] TTF path enabled; use: cargo run --features hud_ttf -- --debug"
else
  echo "[bootstrap] default path enabled; use: cargo run -- --debug"
  echo "[bootstrap] to enable HUD TTF support later:"
  echo "  WITH_TTF=1 scripts/macos/bootstrap.sh"
fi
