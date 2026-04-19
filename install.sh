#!/bin/sh
set -e

REPO="jamesbrayton/code-looper"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
INSTALL_DIR="${INSTALL_DIR%/}"

OS="${OS:-$(uname -s)}"
ARCH="${ARCH:-$(uname -m)}"

case "${OS}/${ARCH}" in
  Linux/x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
  Linux/aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
  Darwin/arm64)  TARGET="aarch64-apple-darwin" ;;
  Darwin/x86_64)
    printf 'macOS Intel (x86_64) is not supported: no pre-built binary is available.\n' >&2
    printf 'Build from source: https://github.com/%s#build--test\n' "$REPO" >&2
    exit 1
    ;;
  *)
    printf 'Unsupported platform: %s/%s\n' "$OS" "$ARCH" >&2
    printf 'Build from source: https://github.com/%s#build--test\n' "$REPO" >&2
    exit 1
    ;;
esac

API_URL="https://api.github.com/repos/${REPO}/releases/latest"
if [ -n "${GITHUB_TOKEN}" ]; then
  VERSION=$(curl -fsSL -H "Authorization: Bearer ${GITHUB_TOKEN}" "$API_URL" \
    | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/' || true)
else
  VERSION=$(curl -fsSL "$API_URL" \
    | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/' || true)
fi

if [ -z "$VERSION" ]; then
  printf 'Failed to resolve latest version from GitHub API.\n' >&2
  printf 'If you are hitting rate limits, set GITHUB_TOKEN and retry.\n' >&2
  exit 1
fi

printf 'Installing code-looper %s for %s...\n' "$VERSION" "$TARGET"

ARCHIVE="code-looper-${VERSION}-${TARGET}.tar.gz"
BINARY_NAME="code-looper-${VERSION}-${TARGET}"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${ARCHIVE}"
tar -xzf "${TMP_DIR}/${ARCHIVE}" -C "$TMP_DIR"

mkdir -p "$INSTALL_DIR"
mv "${TMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/code-looper"
chmod +x "${INSTALL_DIR}/code-looper"

INSTALLED_VERSION=$("${INSTALL_DIR}/code-looper" --version 2>/dev/null || printf '%s' "$VERSION")
printf 'Installed: %s/code-looper (%s)\n' "$INSTALL_DIR" "$INSTALLED_VERSION"

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    printf '\nNote: %s is not in your PATH.\n' "$INSTALL_DIR"
    printf 'Add this to your shell profile:\n'
    printf "  export PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR"
    ;;
esac
