#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DOCS_DIR="${ROOT_DIR}/docs"

if [[ ! -d "${DOCS_DIR}" ]]; then
  echo "docs/ missing at ${DOCS_DIR}" >&2
  exit 1
fi

missing=0
while IFS= read -r -d '' mmd; do
  md="${mmd%.mmd}.md"
  if [[ ! -f "${md}" ]]; then
    echo "Missing markdown companion for $(basename "${mmd}"): expected $(basename "${md}")" >&2
    missing=1
  fi
done < <(find "${DOCS_DIR}" -maxdepth 1 -type f -name '*.mmd' -print0 | sort -z)

if command -v git >/dev/null 2>&1 && [[ -d "${ROOT_DIR}/.git" ]]; then
  tracked_pngs="$(git -C "${ROOT_DIR}" ls-files 'docs/*.png' || true)"
  if [[ -n "${tracked_pngs}" ]]; then
    echo "Tracked docs PNGs found (should be generated-only):" >&2
    echo "${tracked_pngs}" >&2
    missing=1
  fi
fi

if [[ "${missing}" != "0" ]]; then
  exit 1
fi

echo "Docs policy check passed."
