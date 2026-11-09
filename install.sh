#!/usr/bin/env bash
set -euo pipefail

# Blitz instant installer for macOS & Linux
REPO="h3ma209/Blitz"
APP_NAME="Blitz"
VERSION="${1:-latest}"

if [ "$VERSION" = "latest" ]; then
  echo "⚡ Fetching latest release tag of Blitz..."
  VERSION=$(curl -fsSL -H "User-Agent: Blitz-Installer" "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name":' | head -1 | sed -E 's/.*"([^"]+)".*/\1/' || true)
  if [ -z "$VERSION" ]; then
    VERSION="v0.2.6"
  fi
fi

BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
echo "⚡ Installing Blitz ${VERSION}..."

OS="$(uname -s)"
ARCH="$(uname -m)"

TMP_DIR=$(mktemp -d)
MOUNT_DIR=""

cleanup() {
  if [ -n "$MOUNT_DIR" ] && [ -d "$MOUNT_DIR" ]; then
    hdiutil detach "$MOUNT_DIR" -quiet 2>/dev/null || true
    rm -rf "$MOUNT_DIR" 2>/dev/null || true
  fi
  rm -rf "$TMP_DIR" 2>/dev/null || true
}
trap cleanup EXIT

case "$OS" in
  Darwin)
    if [ "$ARCH" != "arm64" ]; then
      echo "Notice: macOS Intel builds run via Rosetta 2 or build from source."
    fi
    DMG_URL="${BASE_URL}/Blitz-macos-arm64.dmg"
    echo "Downloading ${DMG_URL}..."
    if ! curl -fsSL --retry 3 --retry-delay 2 --progress-bar -o "${TMP_DIR}/Blitz.dmg" "$DMG_URL"; then
      echo "CDN gateway error. Retrying directly via API stream..."
      curl -fsSL --retry 3 --retry-delay 2 -H "Accept: application/octet-stream" -H "User-Agent: Blitz-Installer" --progress-bar -o "${TMP_DIR}/Blitz.dmg" "https://api.github.com/repos/${REPO}/releases/assets/723604328"
    fi
    
    echo "Mounting DMG..."
    MOUNT_DIR=$(mktemp -d)
    hdiutil attach "${TMP_DIR}/Blitz.dmg" -mountpoint "$MOUNT_DIR" -nobrowse -quiet
    
    echo "Installing to /Applications/Blitz.app..."
    rm -rf "/Applications/Blitz.app"
    cp -R "${MOUNT_DIR}/Blitz.app" "/Applications/"
    
    # Strip Gatekeeper quarantine automatically
    echo "Bypassing macOS Gatekeeper quarantine..."
    xattr -cr "/Applications/Blitz.app" 2>/dev/null || true
    xattr -rd com.apple.quarantine "/Applications/Blitz.app" 2>/dev/null || true

    # Link CLI into ~/.local/bin
    mkdir -p "${HOME}/.local/bin"
    ln -sf "/Applications/Blitz.app/Contents/MacOS/Blitz" "${HOME}/.local/bin/blitz"
    
    hdiutil detach "$MOUNT_DIR" -quiet
    MOUNT_DIR=""
    
    echo ""
    echo "✓ Blitz successfully installed to /Applications/Blitz.app!"
    echo "  Gatekeeper quarantine removed: xattr -cr /Applications/Blitz.app"
    echo "  CLI linked to ~/.local/bin/blitz"
    echo "  You can open it from Spotlight, Launchpad, or by typing: open /Applications/Blitz.app"
    ;;

  Linux)
    TAR_URL="${BASE_URL}/blitz-linux-x86_64.tar.gz"
    INSTALL_DIR="${HOME}/.local/bin"
    mkdir -p "$INSTALL_DIR"
    echo "Downloading ${TAR_URL}..."
    curl -fL --progress-bar "$TAR_URL" | tar -xz -C "$INSTALL_DIR"
    
    echo "Configuring permissions..."
    chmod +x "${INSTALL_DIR}/blitz"
    
    # Mark as trusted in desktop environments (GNOME/KDE)
    gio set "${INSTALL_DIR}/blitz" metadata::trusted true 2>/dev/null || true

    # Create desktop shortcut for application menu
    DESKTOP_DIR="${HOME}/.local/share/applications"
    if [ -d "$DESKTOP_DIR" ] || mkdir -p "$DESKTOP_DIR" 2>/dev/null; then
      cat > "${DESKTOP_DIR}/blitz.desktop" <<EOF
[Desktop Entry]
Name=Blitz
Comment=High-performance API Client
Exec=${INSTALL_DIR}/blitz
Icon=utilities-terminal
Terminal=false
Type=Application
Categories=Development;
EOF
      chmod +x "${DESKTOP_DIR}/blitz.desktop" 2>/dev/null || true
      gio set "${DESKTOP_DIR}/blitz.desktop" metadata::trusted true 2>/dev/null || true
      update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
    fi

    echo ""
    echo "✓ Blitz successfully installed to ${INSTALL_DIR}/blitz!"
    echo "  Execution permissions set: chmod +x"
    echo "  Desktop entry created: ~/.local/share/applications/blitz.desktop"
    if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
      echo "  Note: Add ${INSTALL_DIR} to your PATH if not already present:"
      echo "        export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
    ;;

  MINGW*|MSYS*|CYGWIN*)
    ZIP_URL="${BASE_URL}/blitz-windows-x86_64.zip"
    INSTALL_DIR="${LOCALAPPDATA:-$HOME/AppData/Local}/Blitz"
    mkdir -p "$INSTALL_DIR"
    TMP_ZIP="${TMP_DIR}/blitz.zip"
    echo "Downloading ${ZIP_URL}..."
    curl -fL --progress-bar -o "$TMP_ZIP" "$ZIP_URL"
    unzip -o -q "$TMP_ZIP" -d "$INSTALL_DIR"
    chmod +x "${INSTALL_DIR}/blitz.exe" 2>/dev/null || true

    # Strip Windows SmartScreen Mark-of-the-Web quarantine
    echo "Bypassing Windows SmartScreen Mark-of-the-Web..."
    WIN_EXE_PATH=$(cygpath -w "${INSTALL_DIR}/blitz.exe" 2>/dev/null || echo "${INSTALL_DIR}\\blitz.exe")
    powershell.exe -NoProfile -Command "Unblock-File -Path '${WIN_EXE_PATH}'" 2>/dev/null || true

    echo ""
    echo "✓ Blitz successfully installed to ${INSTALL_DIR}/blitz.exe"
    echo "  Windows SmartScreen quarantine unblocked: Unblock-File"
    echo "  Launch it by typing: ${INSTALL_DIR}/blitz.exe"
    ;;

  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac
