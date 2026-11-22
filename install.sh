#!/usr/bin/env sh
# Build and install mybot cli.
#
# From a checkout (builds from source):
#   ./install.sh
#   ./install.sh --prefix /usr/local/bin
#   ./install.sh --headless
#   ./install.sh --uninstall
#
# From a URL (downloads the latest release binary):
#   curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh | sh
#   curl -fsSL ... | sh -s -- --version v0.2.0 --prefix ~/bin
#
# Requirements: source mode needs a Rust toolchain (https://rustup.rs), a C
# toolchain, CMake and Perl. Release mode needs curl or wget and tar.
set -eu

PROG=mybot
REPO=seashyne/mybot-cli
PREFIX="${HOME}/.local/bin"
HEADLESS=0
UNINSTALL=0
MODE=auto
VERSION=latest

usage() {
    cat <<EOF
Install $PROG.

Usage: install.sh [options]

Options:
  --prefix DIR     Install directory (default: \$HOME/.local/bin)
  --headless       Build without the interactive TUI (source mode only)
  --uninstall      Remove an installed binary instead of installing
  --from-release   Download a prebuilt binary from GitHub Releases
  --from-source    Build from the current checkout
  --version TAG    Release tag to install (default: latest)
  -h, --help       Show this help
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            [ $# -ge 2 ] || { echo "error: --prefix needs a directory" >&2; exit 2; }
            PREFIX="$2"
            shift 2
            ;;
        --headless) HEADLESS=1; shift ;;
        --uninstall) UNINSTALL=1; shift ;;
        --from-release) MODE=release; shift ;;
        --from-source) MODE=source; shift ;;
        --version)
            [ $# -ge 2 ] || { echo "error: --version needs a tag" >&2; exit 2; }
            VERSION="$2"
            shift 2
            ;;
        -h|--help) usage; exit 0 ;;
        *) echo "error: unknown option '$1'" >&2; usage >&2; exit 2 ;;
    esac
done

TARGET_PATH="${PREFIX}/${PROG}"

if [ "$UNINSTALL" -eq 1 ]; then
    if [ -e "$TARGET_PATH" ]; then
        rm -f "$TARGET_PATH"
        echo "removed $TARGET_PATH"
    else
        echo "nothing to remove at $TARGET_PATH"
    fi
    exit 0
fi

# Decide how to install: a checkout has Cargo.toml next to this script, a piped
# invocation does not.
SELF_DIR=""
if [ -f "$0" ]; then
    SELF_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" 2>/dev/null && pwd || echo "")
fi
if [ "$MODE" = auto ]; then
    if [ -n "$SELF_DIR" ] && [ -f "$SELF_DIR/Cargo.toml" ]; then
        MODE=source
    else
        MODE=release
    fi
fi

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Linux)
            case "$arch" in
                x86_64|amd64) echo "x86_64-unknown-linux-gnu" ;;
                aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
                *) echo "error: unsupported Linux architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        Darwin)
            case "$arch" in
                x86_64) echo "x86_64-apple-darwin" ;;
                arm64) echo "aarch64-apple-darwin" ;;
                *) echo "error: unsupported macOS architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        *)
            echo "error: unsupported operating system: $os (try 'cargo install --git https://github.com/$REPO')" >&2
            exit 1
            ;;
    esac
}

fetch() {
    url="$1"
    dest="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$dest"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$dest" "$url"
    else
        echo "error: need curl or wget to download releases" >&2
        exit 1
    fi
}

install_binary() {
    src="$1"
    mkdir -p "$PREFIX"
    # Replace atomically so a running instance keeps its inode.
    cp "$src" "${TARGET_PATH}.new"
    chmod 755 "${TARGET_PATH}.new"
    mv -f "${TARGET_PATH}.new" "$TARGET_PATH"
    echo
    echo "installed $TARGET_PATH"
    "$TARGET_PATH" --version
}

if [ "$MODE" = release ]; then
    TARGET=$(detect_target)
    if [ "$VERSION" = latest ]; then
        BASE="${MYBOT_BASE_URL:-https://github.com/${REPO}/releases/latest/download}"
    else
        BASE="${MYBOT_BASE_URL:-https://github.com/${REPO}/releases/download/${VERSION}}"
    fi

    TMP=$(mktemp -d)
    trap 'rm -rf "$TMP"' EXIT

    echo "downloading ${PROG} for ${TARGET} (${VERSION})…"
    fetch "${BASE}/${PROG}-${TARGET}" "$TMP/${PROG}"

    if fetch "${BASE}/sha256sums.txt" "$TMP/sums" 2>/dev/null; then
        expected=$(grep " ${PROG}-${TARGET}\$" "$TMP/sums" | awk '{print $1}' || true)
        if [ -n "$expected" ]; then
            actual=$(sha256sum "$TMP/${PROG}" | awk '{print $1}')
            if [ "$expected" != "$actual" ]; then
                echo "error: checksum mismatch for ${PROG}-${TARGET}" >&2
                exit 1
            fi
            echo "checksum ok"
        fi
    fi

    install_binary "$TMP/${PROG}"
    exit 0
fi

# ---- source mode ----
need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "error: '$1' not found. $2" >&2
        exit 1
    }
}

need cargo "Install Rust from https://rustup.rs and try again."
need cc "Install a C toolchain (build-essential / xcode-select --install)."

if ! command -v cmake >/dev/null 2>&1; then
    echo "warning: cmake not found; the build fails if aws-lc-sys is not cached yet." >&2
fi

cd "$SELF_DIR"

echo "building $PROG (release)…"
if [ "$HEADLESS" -eq 1 ]; then
    cargo build --release --no-default-features
else
    cargo build --release
fi

BIN="target/release/${PROG}"
[ -x "$BIN" ] || { echo "error: expected binary at $BIN" >&2; exit 1; }

install_binary "$BIN"

case ":${PATH}:" in
    *":${PREFIX}:"*) ;;
    *)
        echo
        echo "note: ${PREFIX} is not on your PATH. Add it with:"
        echo "  echo 'export PATH=\"${PREFIX}:\$PATH\"' >> ~/.profile"
        ;;
esac
