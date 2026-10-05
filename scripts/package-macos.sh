#!/usr/bin/env bash

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: macOS bundles must be built on macOS" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${MACOS_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
APP_PATH="${MACOS_APP_PATH:-${ROOT_DIR}/dist/ImageMin.app}"
TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT_DIR}/target}"
SIGNING_IDENTITY="${MACOS_SIGNING_IDENTITY:--}"

if [[ "${APP_PATH}" != /* ]]; then
  APP_PATH="${ROOT_DIR}/${APP_PATH}"
fi
if [[ "${APP_PATH}" != *.app || "${APP_PATH}" == "/" ]]; then
  echo "error: MACOS_APP_PATH must point to an .app bundle" >&2
  exit 1
fi
if [[ "${TARGET_DIR}" != /* ]]; then
  TARGET_DIR="${ROOT_DIR}/${TARGET_DIR}"
fi

case "${TARGET}" in
  aarch64-apple-darwin|x86_64-apple-darwin) ;;
  *)
    echo "error: unsupported macOS target '${TARGET}'" >&2
    exit 1
    ;;
esac

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "${ROOT_DIR}/Cargo.toml" | head -n 1)"
if [[ -z "${VERSION}" ]]; then
  echo "error: could not read the package version from Cargo.toml" >&2
  exit 1
fi

echo "Building ImageMin ${VERSION} for ${TARGET}..."
cd "${ROOT_DIR}"
cargo build --manifest-path "${ROOT_DIR}/Cargo.toml" --release --locked --target "${TARGET}"

BINARY_PATH="${TARGET_DIR}/${TARGET}/release/image-min"
if [[ ! -x "${BINARY_PATH}" ]]; then
  echo "error: release binary not found at ${BINARY_PATH}" >&2
  exit 1
fi

rm -rf "${APP_PATH}"
mkdir -p "${APP_PATH}/Contents/MacOS" "${APP_PATH}/Contents/Resources"
install -m 755 "${BINARY_PATH}" "${APP_PATH}/Contents/MacOS/image-min"
sed "s/@VERSION@/${VERSION}/g" \
  "${ROOT_DIR}/packaging/macos/Info.plist" > "${APP_PATH}/Contents/Info.plist"
printf 'APPL????' > "${APP_PATH}/Contents/PkgInfo"
plutil -lint "${APP_PATH}/Contents/Info.plist" >/dev/null

if [[ "${SIGNING_IDENTITY}" == "-" ]]; then
  codesign --force --deep --sign - "${APP_PATH}"
else
  codesign --force --deep --options runtime --timestamp \
    --sign "${SIGNING_IDENTITY}" "${APP_PATH}"
fi
codesign --verify --deep --strict "${APP_PATH}"

echo "Created ${APP_PATH}"
