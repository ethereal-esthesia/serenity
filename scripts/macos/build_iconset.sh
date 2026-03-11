#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC_PNG="${1:-${ROOT_DIR}/assets/icon/serenity-icon-1024.png}"
ICONSET_DIR="${ROOT_DIR}/assets/icon/Serenity.iconset"
ICNS_OUT="${ROOT_DIR}/assets/icon/Serenity.icns"

if [[ ! -f "${SRC_PNG}" ]]; then
  echo "Missing source PNG: ${SRC_PNG}" >&2
  echo "Provide a 1024x1024 PNG as first arg or at assets/icon/serenity-icon-1024.png" >&2
  exit 1
fi

mkdir -p "${ICONSET_DIR}"

make_icon() {
  local size="$1"
  local name="$2"
  sips -z "${size}" "${size}" "${SRC_PNG}" --out "${ICONSET_DIR}/${name}" >/dev/null
}

make_icon 16 icon_16x16.png
make_icon 32 icon_16x16@2x.png
make_icon 32 icon_32x32.png
make_icon 64 icon_32x32@2x.png
make_icon 128 icon_128x128.png
make_icon 256 icon_128x128@2x.png
make_icon 256 icon_256x256.png
make_icon 512 icon_256x256@2x.png
make_icon 512 icon_512x512.png
make_icon 1024 icon_512x512@2x.png

iconutil -c icns "${ICONSET_DIR}" -o "${ICNS_OUT}"

echo "Generated iconset: ${ICONSET_DIR}"
echo "Generated icns: ${ICNS_OUT}"
