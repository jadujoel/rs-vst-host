#!/usr/bin/env bash
# Build a macOS .app bundle for RS VST Host.
#
# Usage: ./scripts/bundle-macos.sh [--release]
#
# The bundle is created at target/{debug|release}/RS VST Host.app

set -euo pipefail

PROFILE="debug"
CARGO_FLAGS=""
if [[ "${1:-}" == "--release" ]]; then
    PROFILE="release"
    CARGO_FLAGS="--release"
fi

APP_NAME="RS VST Host"
BINARY="rs-vst-host"
TARGET_DIR="target/${PROFILE}"
BUNDLE_DIR="${TARGET_DIR}/${APP_NAME}.app"

echo "Building ${APP_NAME} (${PROFILE})..."
cargo build ${CARGO_FLAGS}

echo "Creating app bundle at ${BUNDLE_DIR}..."
mkdir -p "${BUNDLE_DIR}/Contents/MacOS"
mkdir -p "${BUNDLE_DIR}/Contents/Resources"

cp "${TARGET_DIR}/${BINARY}" "${BUNDLE_DIR}/Contents/MacOS/"

cat > "${BUNDLE_DIR}/Contents/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>${BINARY}</string>
  <key>CFBundleIdentifier</key>
  <string>com.rs-vst-host.app</string>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleVersion</key>
  <string>0.25.0</string>
  <key>CFBundleShortVersionString</key>
  <string>0.25.0</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSMicrophoneUsageDescription</key>
  <string>RS VST Host needs microphone access for audio input processing.</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSSupportsAutomaticGraphicsSwitching</key>
  <true/>
</dict>
</plist>
EOF

echo "✓ Bundle created: ${BUNDLE_DIR}"
echo "  To run: open '${BUNDLE_DIR}'"
