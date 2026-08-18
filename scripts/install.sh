#!/bin/sh
# Comind installer. Downloads the right prebuilt binary from GitHub Releases.
#
#   curl -LsSf https://raw.githubusercontent.com/sayef/comind/main/scripts/install.sh | sh
#
# Env:
#   COMIND_VERSION   tag to install (default: latest)
#   COMIND_BIN_DIR   install dir (default: $HOME/.local/bin)
#   GITHUB_TOKEN     optional (higher GitHub API rate limits, or installing from a private fork)
set -eu

REPO="sayef/comind"
VERSION="${COMIND_VERSION:-latest}"
BIN_DIR="${COMIND_BIN_DIR:-$HOME/.local/bin}"

# --- detect target triple ---------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Linux)  os_part="unknown-linux-gnu" ;;
  Darwin) os_part="apple-darwin" ;;
  *) echo "comind: unsupported OS: $os" >&2; exit 1 ;;
esac
case "$arch" in
  x86_64|amd64) arch_part="x86_64" ;;
  arm64|aarch64) arch_part="aarch64" ;;
  *) echo "comind: unsupported arch: $arch" >&2; exit 1 ;;
esac
target="${arch_part}-${os_part}"
asset="comind-${target}.tar.gz"

# --- resolve download URL ---------------------------------------------------
auth=""
[ -n "${GITHUB_TOKEN:-}" ] && auth="-H \"Authorization: Bearer $GITHUB_TOKEN\""
if [ "$VERSION" = "latest" ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi
url="$base/$asset"

echo "comind: installing $target from $url"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# shellcheck disable=SC2086
eval curl -LsSf $auth "$url" -o "$tmp/$asset"
tar -C "$tmp" -xzf "$tmp/$asset"

mkdir -p "$BIN_DIR"
mv "$tmp/comind" "$BIN_DIR/comind"
chmod +x "$BIN_DIR/comind"

echo "comind: installed to $BIN_DIR/comind"
case ":$PATH:" in
  *":$BIN_DIR:"*) : ;;
  *) echo "comind: add $BIN_DIR to your PATH (e.g. echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.profile)" ;;
esac
"$BIN_DIR/comind" --version
