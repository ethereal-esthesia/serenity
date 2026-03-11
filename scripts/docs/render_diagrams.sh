#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DOCS_DIR="${ROOT_DIR}/docs"
STRICT_MODE="${STRICT_MODE:-0}"  # 1 to fail if renderer is unavailable

if [[ ! -d "${DOCS_DIR}" ]]; then
  echo "No docs directory at ${DOCS_DIR}; skipping diagram render."
  exit 0
fi

find_mmd_files() {
  find "${DOCS_DIR}" -maxdepth 1 -type f -name '*.mmd' | sort
}

mmd_files=( $(find_mmd_files) )
if [[ ${#mmd_files[@]} -eq 0 ]]; then
  echo "No .mmd files found in ${DOCS_DIR}; skipping."
  exit 0
fi

run_mmdc() {
  local input="$1"
  local output="$2"
  local log_file
  log_file="$(mktemp)"
  if command -v mmdc >/dev/null 2>&1; then
    if ! mmdc -i "${input}" -o "${output}" -w 2200 -H 1400 -b white >"${log_file}" 2>&1; then
      rm -f "${log_file}"
      return 1
    fi
  elif command -v npx >/dev/null 2>&1; then
    if ! npx -y @mermaid-js/mermaid-cli -i "${input}" -o "${output}" -w 2200 -H 1400 -b white >"${log_file}" 2>&1; then
      rm -f "${log_file}"
      return 1
    fi
  else
    rm -f "${log_file}"
    return 127
  fi
  rm -f "${log_file}"
}

for input in "${mmd_files[@]}"; do
  output="${input%.mmd}.png"
  if [[ -f "${output}" && "${output}" -nt "${input}" ]]; then
    echo "Up-to-date: $(basename "${output}")"
    continue
  fi
  echo "Rendering $(basename "${input}") -> $(basename "${output}")"
  if ! run_mmdc "${input}" "${output}"; then
    if [[ "${STRICT_MODE}" == "1" ]]; then
      echo "Failed to render ${input} and STRICT_MODE=1" >&2
      exit 1
    fi
    echo "Skipping render for ${input}; Mermaid CLI unavailable or failed."
  fi
done
