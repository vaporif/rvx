#!/bin/sh
set -eu

REPO="vaporif/rvx"
BINARY="rvx"
INSTALL_DIR="${RVX_INSTALL_DIR:-$HOME/.local/bin}"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)  os="unknown-linux-gnu" ;;
    Darwin) os="apple-darwin" ;;
    *)      echo "error: unsupported OS: $OS" >&2; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64)   arch="x86_64" ;;
    aarch64|arm64)   arch="aarch64" ;;
    *)               echo "error: unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

TARGET="${arch}-${os}"

# Get latest release tag
LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | head -1 | sed 's/.*: "//;s/".*//')

if [ -z "$LATEST" ]; then
    echo "error: could not determine latest release" >&2
    exit 1
fi

VERSION="${LATEST#v}"

echo "Installing ${BINARY} ${VERSION} for ${TARGET}..."

# Try common archive naming patterns
URL=""
for pattern in \
    "${BINARY}-${TARGET}.tar.gz" \
    "${BINARY}-v${VERSION}-${TARGET}.tar.gz" \
    "${BINARY}-${VERSION}-${TARGET}.tar.gz" \
    "${BINARY}-${TARGET}.tar.xz" \
    "${BINARY}-${TARGET}.zip"; do
    check_url="https://github.com/${REPO}/releases/download/${LATEST}/${pattern}"
    if curl -fsSL --head "$check_url" >/dev/null 2>&1; then
        URL="$check_url"
        break
    fi
done

if [ -z "$URL" ]; then
    echo "error: no binary found for ${TARGET} in release ${LATEST}" >&2
    echo "  Check: https://github.com/${REPO}/releases/tag/${LATEST}" >&2
    exit 1
fi

# Download and extract
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading ${URL}..."
curl -fsSL "$URL" -o "${TMPDIR}/archive"

mkdir -p "$INSTALL_DIR"

case "$URL" in
    *.tar.gz)  tar xzf "${TMPDIR}/archive" -C "$TMPDIR" ;;
    *.tar.xz)  tar xJf "${TMPDIR}/archive" -C "$TMPDIR" ;;
    *.zip)     unzip -q "${TMPDIR}/archive" -d "$TMPDIR" ;;
esac

# Find and install binary
BIN=$(find "$TMPDIR" -name "$BINARY" -type f | head -1)
if [ -z "$BIN" ]; then
    echo "error: binary '${BINARY}' not found in archive" >&2
    exit 1
fi

cp "$BIN" "${INSTALL_DIR}/${BINARY}"
chmod +x "${INSTALL_DIR}/${BINARY}"

echo "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"

# Check PATH
case ":$PATH:" in
    *":${INSTALL_DIR}:"*) ;;
    *) echo "  Add to PATH: export PATH=\"${INSTALL_DIR}:\$PATH\"" ;;
esac
