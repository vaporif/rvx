#!/bin/sh
set -eu

REPO="vaporif/rvx"
BINARY="rvx"
INSTALL_DIR="${RVX_INSTALL_DIR:-$HOME/.local/bin}"
ENV_FILE="$HOME/.rvx/env"

# GitHub API auth (optional, avoids rate limits)
AUTH_HEADER=""
if [ -n "${GITHUB_TOKEN:-}" ]; then
    AUTH_HEADER="Authorization: Bearer ${GITHUB_TOKEN}"
fi

github_curl() {
    if [ -n "$AUTH_HEADER" ]; then
        curl -fsSL -H "$AUTH_HEADER" "$@"
    else
        curl -fsSL "$@"
    fi
}

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)  os="unknown-linux-musl" ;;
    Darwin) os="apple-darwin" ;;
    *)      echo "error: unsupported OS: $OS" >&2; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64)  arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *)             echo "error: unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

TARGET="${arch}-${os}"

# Get latest release tag (tries stable first, falls back to any release including prereleases)
LATEST=$(github_curl "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
    | grep '"tag_name"' | head -1 | sed 's/.*: "//;s/".*//')

if [ -z "$LATEST" ]; then
    LATEST=$(github_curl "https://api.github.com/repos/${REPO}/releases" \
        | grep '"tag_name"' | head -1 | sed 's/.*: "//;s/".*//')
fi

if [ -z "$LATEST" ]; then
    echo "error: could not determine latest release. Set GITHUB_TOKEN if rate-limited." >&2
    exit 1
fi

# Archive naming: rvx-<target>-<tag>.tar.gz
ARCHIVE="${BINARY}-${TARGET}-${LATEST}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${LATEST}/${ARCHIVE}"

echo "Installing ${BINARY} ${LATEST} for ${TARGET}..."

# Download and extract
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading ${URL}..."
curl -fsSL "$URL" -o "${TMPDIR}/archive.tar.gz"

tar xzf "${TMPDIR}/archive.tar.gz" -C "$TMPDIR"

# Find and install binary
BIN=$(find "$TMPDIR" -name "$BINARY" -type f | head -1)
if [ -z "$BIN" ]; then
    echo "error: binary '${BINARY}' not found in archive" >&2
    exit 1
fi

mkdir -p "$INSTALL_DIR"
cp "$BIN" "${INSTALL_DIR}/${BINARY}"
chmod +x "${INSTALL_DIR}/${BINARY}"

echo "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"

# Create env file for PATH setup
mkdir -p "$(dirname "$ENV_FILE")"
cat > "$ENV_FILE" <<'ENVEOF'
#!/bin/sh
# rvx shell setup
case ":${PATH}:" in
    *":$HOME/.local/bin:"*) ;;
    *) export PATH="$HOME/.local/bin:$PATH" ;;
esac
ENVEOF

# Add to all shell profile files
SOURCE_LINE=". \"$ENV_FILE\""

for rc in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.zshenv"; do
    # Only modify files that already exist, except .profile which we always set up
    if [ -f "$rc" ] || [ "$rc" = "$HOME/.profile" ]; then
        if ! grep -qF "$ENV_FILE" "$rc" 2>/dev/null; then
            echo "$SOURCE_LINE" >> "$rc"
        fi
    fi
done

# Fish uses a different syntax
FISH_CONFIG="$HOME/.config/fish/conf.d/rvx.fish"
if [ -d "$HOME/.config/fish" ]; then
    mkdir -p "$HOME/.config/fish/conf.d"
    echo "fish_add_path $INSTALL_DIR" > "$FISH_CONFIG"
fi

echo "Added ${INSTALL_DIR} to PATH via ${ENV_FILE}"
echo "To use now, run: . \"${ENV_FILE}\""
