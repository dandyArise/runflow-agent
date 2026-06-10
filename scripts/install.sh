#!/usr/bin/env sh
set -eu

VERSION="${VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-"$HOME/.runflow-agent/bin"}"
REPO="dandyArise/runflow-agent"

if [ "$VERSION" = "latest" ]; then
  VERSION="$(curl -fsSL -H "User-Agent: runflow-agent-installer" "https://api.github.com/repos/$REPO/releases/latest" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
fi

case "$VERSION" in
  v*) ;;
  *) VERSION="v$VERSION" ;;
esac

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS:$ARCH" in
  Linux:x86_64) PLATFORM="linux-x64" ;;
  Darwin:x86_64) PLATFORM="macos-x64" ;;
  Darwin:arm64) PLATFORM="macos-arm64" ;;
  Darwin:aarch64) PLATFORM="macos-arm64" ;;
  *) echo "unsupported platform: $OS $ARCH" >&2; exit 1 ;;
esac

ASSET="runflow-agent-$VERSION-$PLATFORM.zip"
URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET"
WORK="${TMPDIR:-/tmp}/runflow-agent-install-$VERSION"
ZIP="$WORK/$ASSET"
EXTRACT="$WORK/extract"

mkdir -p "$WORK" "$EXTRACT" "$INSTALL_DIR"
curl -fL -H "User-Agent: runflow-agent-installer" -o "$ZIP" "$URL"
if command -v unzip >/dev/null 2>&1; then
  unzip -q -o "$ZIP" -d "$EXTRACT"
elif command -v python3 >/dev/null 2>&1; then
  python3 -c 'import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])' "$ZIP" "$EXTRACT"
else
  echo "install requires unzip or python3" >&2
  exit 1
fi

BINARY="$(find "$EXTRACT" -type f -name runflow-agent | head -n 1)"
if [ -z "$BINARY" ]; then
  echo "archive did not contain runflow-agent" >&2
  exit 1
fi

cp "$BINARY" "$INSTALL_DIR/runflow-agent"
chmod +x "$INSTALL_DIR/runflow-agent"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Add this to PATH if needed: export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac

"$INSTALL_DIR/runflow-agent" self version 2>/dev/null || "$INSTALL_DIR/runflow-agent" --help | head -n 3
