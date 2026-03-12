#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FAIL=0

check_cmd() {
  local cmd="$1"
  local hint="$2"
  if command -v "${cmd}" >/dev/null 2>&1; then
    echo "[ok] command: ${cmd}"
  else
    echo "[missing] command: ${cmd} (${hint})"
    FAIL=1
  fi
}

check_brew_formula() {
  local formula="$1"
  if ! command -v brew >/dev/null 2>&1; then
    echo "[warn] homebrew not found; cannot check ${formula}"
    return
  fi

  if brew list --versions "${formula}" >/dev/null 2>&1; then
    echo "[ok] brew formula: ${formula}"
  else
    echo "[warn] brew formula not found: ${formula} (run: brew install ${formula})"
  fi
}

echo "[doctor] checking toolchain"
check_cmd cargo "install Rust via rustup.rs"
check_cmd rustc "install Rust via rustup.rs"
check_cmd brew "install Homebrew via brew.sh"

echo "[doctor] checking native dependencies"
check_brew_formula sdl3
check_brew_formula sdl3_ttf

echo "[doctor] checking bundled assets"
if [[ -f "${ROOT_DIR}/assets/fonts/CascadiaMono-Regular.ttf" ]]; then
  echo "[ok] bundled font: assets/fonts/CascadiaMono-Regular.ttf"
else
  echo "[optional-missing] bundled font: assets/fonts/CascadiaMono-Regular.ttf"
fi
if [[ -f "${ROOT_DIR}/assets/fonts/CASCADIA-LICENSE.txt" ]]; then
  echo "[ok] bundled font license: assets/fonts/CASCADIA-LICENSE.txt"
else
  echo "[optional-missing] bundled font license: assets/fonts/CASCADIA-LICENSE.txt"
fi

echo "[doctor] checking cargo build path (default features)"
if (cd "${ROOT_DIR}" && cargo check >/dev/null 2>&1); then
  echo "[ok] cargo check (default)"
else
  echo "[fail] cargo check (default)"
  FAIL=1
fi

echo "[doctor] checking cargo build path (hud_ttf feature)"
if (cd "${ROOT_DIR}" && cargo check --features hud_ttf >/dev/null 2>&1); then
  echo "[ok] cargo check (--features hud_ttf)"
else
  echo "[warn] cargo check (--features hud_ttf) failed (install sdl3_ttf to enable)"
fi

if [[ "${FAIL}" -ne 0 ]]; then
  echo "[doctor] FAIL"
  exit 1
fi

echo "[doctor] PASS"
