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
ICON_FILE="${ICON_FILE:-${SERENITY_ICON_FILE:-Serenity.icns}}"
BUILD_RELEASE="${BUILD_RELEASE:-1}"
COPY_BINARY="${COPY_BINARY:-1}"
SIGN_MODE="${SIGN_MODE:-adhoc}"            # none | adhoc | developerid
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-}" # required when SIGN_MODE=developerid
RUN_VERIFY="${RUN_VERIFY:-1}"              # 1 to run codesign/spctl checks

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
  -e "s|@ICON_FILE@|${ICON_FILE}|g" \
  "${TEMPLATE_PATH}" > "${CONTENTS_DIR}/Info.plist"

ICON_SRC="${ROOT_DIR}/assets/icon/${ICON_FILE}"
if [[ -f "${ICON_SRC}" ]]; then
  cp "${ICON_SRC}" "${RESOURCES_DIR}/${ICON_FILE}"
fi

BIN_SRC="${ROOT_DIR}/target/release/${EXECUTABLE_NAME}"
if [[ "${BUILD_RELEASE}" == "1" ]]; then
  (cd "${ROOT_DIR}" && cargo build --release)
fi

if [[ "${COPY_BINARY}" == "1" && -f "${BIN_SRC}" ]]; then
  cp "${BIN_SRC}" "${MACOS_DIR}/${EXECUTABLE_NAME}"
  chmod +x "${MACOS_DIR}/${EXECUTABLE_NAME}"
else
  cat > "${MACOS_DIR}/${EXECUTABLE_NAME}" <<'STUB'
#!/usr/bin/env bash
echo "Serenity app bundle scaffold created."
echo "Built binary missing; run scripts/macos/package_app.sh with BUILD_RELEASE=1."
STUB
  chmod +x "${MACOS_DIR}/${EXECUTABLE_NAME}"
fi

case "${SIGN_MODE}" in
  none)
    echo "Signing skipped (SIGN_MODE=none)."
    ;;
  adhoc)
    codesign --force --deep --sign - "${APP_DIR}"
    echo "Ad-hoc signed app: ${APP_DIR}"
    ;;
  developerid)
    if [[ -z "${CODESIGN_IDENTITY}" ]]; then
      echo "SIGN_MODE=developerid requires CODESIGN_IDENTITY." >&2
      exit 1
    fi
    codesign --force --deep --options runtime --sign "${CODESIGN_IDENTITY}" "${APP_DIR}"
    echo "Developer ID signed app: ${APP_DIR}"
    ;;
  *)
    echo "Unsupported SIGN_MODE: ${SIGN_MODE} (expected none|adhoc|developerid)" >&2
    exit 1
    ;;
esac

echo "Created app scaffold: ${APP_DIR}"
echo "Info.plist: ${CONTENTS_DIR}/Info.plist"
if [[ -f "${BIN_SRC}" ]]; then
  echo "Executable bundled: ${MACOS_DIR}/${EXECUTABLE_NAME}"
else
  echo "Executable stub: ${MACOS_DIR}/${EXECUTABLE_NAME}"
fi
if [[ -f "${RESOURCES_DIR}/${ICON_FILE}" ]]; then
  echo "Icon copied: ${RESOURCES_DIR}/${ICON_FILE}"
else
  echo "Icon missing (expected for Step 3 until generated): ${ICON_SRC}"
fi

if [[ "${RUN_VERIFY}" == "1" ]]; then
  echo "Verification: codesign"
  codesign --verify --deep --strict --verbose=2 "${APP_DIR}"
  echo "Verification: spctl"
  if ! spctl --assess --type execute --verbose "${APP_DIR}"; then
    echo "spctl assessment failed (this can happen for ad-hoc signatures)." >&2
  fi
fi
