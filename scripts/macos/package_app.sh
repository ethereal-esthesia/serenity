#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
METADATA_FILE="${ROOT_DIR}/packaging/macos/app_metadata.env"

if [[ -f "${METADATA_FILE}" ]]; then
  # shellcheck disable=SC1090
  source "${METADATA_FILE}"
fi

APP_NAME="${APP_NAME:-${SERENITY_APP_NAME:-Serenity}}"
APP_DISPLAY_NAME="${APP_DISPLAY_NAME:-${SERENITY_APP_DISPLAY_NAME:-Serenity}}"
BUNDLE_IDENTIFIER="${BUNDLE_IDENTIFIER:-${SERENITY_BUNDLE_IDENTIFIER:-com.etherealesthesia.serenity}}"
APP_VERSION="${APP_VERSION:-${SERENITY_APP_VERSION:-0.1.0}}"
APP_BUILD="${APP_BUILD:-${SERENITY_APP_BUILD:-1}}"
EXECUTABLE_NAME="${EXECUTABLE_NAME:-${SERENITY_EXECUTABLE_NAME:-serenity}}"

TEMPLATE_PATH="${ROOT_DIR}/packaging/macos/Info.plist.template"
DIST_DIR="${ROOT_DIR}/dist"
APP_DIR="${DIST_DIR}/${APP_NAME}.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

if [[ ! -f "${TEMPLATE_PATH}" ]]; then
  echo "Missing template: ${TEMPLATE_PATH}" >&2
  exit 1
fi

rm -rf "${APP_DIR}"
mkdir -p "${MACOS_DIR}" "${RESOURCES_DIR}"

sed \
  -e "s|@APP_NAME@|${APP_NAME}|g" \
  -e "s|@APP_DISPLAY_NAME@|${APP_DISPLAY_NAME}|g" \
  -e "s|@BUNDLE_IDENTIFIER@|${BUNDLE_IDENTIFIER}|g" \
  -e "s|@APP_VERSION@|${APP_VERSION}|g" \
  -e "s|@APP_BUILD@|${APP_BUILD}|g" \
  -e "s|@EXECUTABLE_NAME@|${EXECUTABLE_NAME}|g" \
  "${TEMPLATE_PATH}" > "${CONTENTS_DIR}/Info.plist"

cat > "${MACOS_DIR}/${EXECUTABLE_NAME}" <<'STUB'
#!/usr/bin/env bash
echo "Serenity app bundle scaffold created."
echo "Replace this stub with the built binary in Step 4."
STUB
chmod +x "${MACOS_DIR}/${EXECUTABLE_NAME}"

echo "Created app scaffold: ${APP_DIR}"
echo "Info.plist: ${CONTENTS_DIR}/Info.plist"
echo "Executable stub: ${MACOS_DIR}/${EXECUTABLE_NAME}"
