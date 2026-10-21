#!/usr/bin/env bash
# usage: scripts/bump-cask.sh 0.1.1 [cask_path]
set -euo pipefail
VER="${1:?usage: scripts/bump-cask.sh <version> [cask_path]}"
CASK="${2:-packaging/homebrew/blitz.rb}"
URL="https://github.com/h3ma209/Blitz/releases/download/v${VER}/Blitz-macos-arm64.dmg"
SHA=$(curl -fsSL "$URL" | shasum -a 256 | cut -d' ' -f1)
sed -i.bak -E "s/^  version \".*\"/  version \"${VER}\"/; s/^  sha256 \".*\"/  sha256 \"${SHA}\"/" "$CASK"
rm -f "$CASK.bak"
echo "cask -> ${VER} ${SHA}"
