#!/bin/sh
set -e

REPO="jamesbrayton/code-looper"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
INSTALL_DIR="${INSTALL_DIR%/}"

OS="${OS:-$(uname -s)}"
ARCH="${ARCH:-$(uname -m)}"

case "${OS}/${ARCH}" in
  Linux/x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
  Linux/aarch64|Linux/arm64) TARGET="aarch64-unknown-linux-gnu" ;;
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
    | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
else
  VERSION=$(curl -fsSL "$API_URL" \
    | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
fi

if [ -z "$VERSION" ]; then
  printf 'Failed to resolve latest version. Check your network connection.\n' >&2
  printf 'If hitting GitHub API rate limits, set GITHUB_TOKEN and retry.\n' >&2
  exit 1
fi

printf 'Installing code-looper %s for %s...\n' "$VERSION" "$TARGET"

ARCHIVE="code-looper-${VERSION}-${TARGET}.tar.gz"
BINARY_NAME="code-looper-${VERSION}-${TARGET}"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"
CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${VERSION}/checksums.txt"

TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t code-looper 2>/dev/null || printf '')
if [ -z "$TMP_DIR" ] || [ ! -d "$TMP_DIR" ]; then
  printf 'Failed to create a temporary directory for installation.\n' >&2
  exit 1
fi
trap 'rm -rf "$TMP_DIR"' EXIT

curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${ARCHIVE}"
curl -fsSL "$CHECKSUMS_URL" -o "${TMP_DIR}/checksums.txt"

EXPECTED_CHECKSUM=$(grep "  ${ARCHIVE}$" "${TMP_DIR}/checksums.txt" | awk '{print $1}' || true)
if [ -z "$EXPECTED_CHECKSUM" ]; then
  printf 'Failed to find checksum for %s in checksums.txt.\n' "$ARCHIVE" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_CHECKSUM=$(sha256sum "${TMP_DIR}/${ARCHIVE}" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_CHECKSUM=$(shasum -a 256 "${TMP_DIR}/${ARCHIVE}" | awk '{print $1}')
else
  printf 'Neither sha256sum nor shasum is available to verify the download.\n' >&2
  exit 1
fi

if [ "$ACTUAL_CHECKSUM" != "$EXPECTED_CHECKSUM" ]; then
  printf 'Checksum verification failed for %s.\n' "$ARCHIVE" >&2
  exit 1
fi

tar -xzf "${TMP_DIR}/${ARCHIVE}" -C "$TMP_DIR"

mkdir -p "$INSTALL_DIR"
mv "${TMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/code-looper"
chmod +x "${INSTALL_DIR}/code-looper"

if ! INSTALLED_VERSION=$("${INSTALL_DIR}/code-looper" --version 2>&1); then
  printf 'Binary was installed to %s/code-looper but failed to execute:\n%s\n' "$INSTALL_DIR" "$INSTALLED_VERSION" >&2
  printf 'The binary may be incompatible with this system (wrong architecture, noexec mount, etc.).\n' >&2
  exit 1
fi
printf 'Installed: %s/code-looper (%s)\n' "$INSTALL_DIR" "$INSTALLED_VERSION"

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    printf '\nNote: %s is not in your PATH.\n' "$INSTALL_DIR"
    printf 'Add this to your shell profile:\n'
    printf "  export PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR"
    ;;
esac
