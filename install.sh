#!/usr/bin/env bash
set -euo pipefail

# Blitz instant installer for macOS & Linux
REPO="h3ma209/Blitz"
APP_NAME="Blitz"

echo "⚡ Fetching latest release of Blitz..."
RELEASE_JSON=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest")
TAG=$(echo "$RELEASE_JSON" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$TAG" ]; then
  echo "Error: Could not determine latest release tag." >&2
  exit 1
fi

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    if [ "$ARCH" != "arm64" ]; then
      echo "Notice: macOS Intel builds run via Rosetta 2 or build from source."
    fi
    DMG_URL="https://github.com/${REPO}/releases/download/${TAG}/Blitz-macos-arm64.dmg"
    TMP_DIR=$(mktemp -d)
    echo "Downloading ${DMG_URL}..."
    curl -fsSL -o "${TMP_DIR}/Blitz.dmg" "$DMG_URL"
    
    echo "Mounting DMG..."
    MOUNT_DIR=$(mktemp -d)
    hdiutil attach "${TMP_DIR}/Blitz.dmg" -mountpoint "$MOUNT_DIR" -nobrowse -quiet
    
    echo "Installing to /Applications/Blitz.app..."
    rm -rf "/Applications/Blitz.app"
    cp -R "${MOUNT_DIR}/Blitz.app" "/Applications/"
    
    # Strip Gatekeeper quarantine automatically
    xattr -cr "/Applications/Blitz.app" 2>/dev/null || true
    
    hdiutil detach "$MOUNT_DIR" -quiet
    rm -rf "$TMP_DIR" "$MOUNT_DIR"
    
    echo ""
    echo "✓ Blitz successfully installed to /Applications/Blitz.app!"
    echo "  You can open it from Spotlight, Launchpad, or by typing: open /Applications/Blitz.app"
    ;;

  Linux)
    TAR_URL="https://github.com/${REPO}/releases/download/${TAG}/blitz-linux-x86_64.tar.gz"
    INSTALL_DIR="${HOME}/.local/bin"
    mkdir -p "$INSTALL_DIR"
    echo "Downloading ${TAR_URL}..."
    curl -fsSL "$TAR_URL" | tar -xz -C "$INSTALL_DIR"
    chmod +x "${INSTALL_DIR}/blitz"
    echo ""
    echo "✓ Blitz CLI installed to ${INSTALL_DIR}/blitz"
    ;;

  MINGW*|MSYS*|CYGWIN*)
    ZIP_URL="https://github.com/${REPO}/releases/download/${TAG}/blitz-windows-x86_64.zip"
    INSTALL_DIR="${LOCALAPPDATA:-$HOME/AppData/Local}/Blitz"
    mkdir -p "$INSTALL_DIR"
    TMP_ZIP=$(mktemp --suffix=.zip 2>/dev/null || mktemp)
    echo "Downloading ${ZIP_URL}..."
    curl -fsSL -o "$TMP_ZIP" "$ZIP_URL"
    unzip -o -q "$TMP_ZIP" -d "$INSTALL_DIR"
    rm -f "$TMP_ZIP"
    chmod +x "${INSTALL_DIR}/blitz.exe" 2>/dev/null || true
    echo ""
    echo "✓ Blitz successfully installed to ${INSTALL_DIR}/blitz.exe"
    echo "  Launch it by typing: ${INSTALL_DIR}/blitz.exe"
    ;;

  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac
