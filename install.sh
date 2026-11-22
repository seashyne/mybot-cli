#!/usr/bin/env sh
# Build and install mybot cli.
#
#   ./install.sh                 install to ~/.local/bin
#   ./install.sh --prefix /usr/local/bin
#   ./install.sh --headless      build without the TUI (smaller, no crossterm)
#   ./install.sh --uninstall     remove the installed binary
#
# Requirements: a Rust toolchain (https://rustup.rs) plus a C toolchain,
# CMake and Perl (rustls uses aws-lc-sys).
set -eu

PROG=mybot
PREFIX="${HOME}/.local/bin"
HEADLESS=0
UNINSTALL=0

usage() {
    cat <<EOF
Install $PROG from source.

Usage: ./install.sh [options]

Options:
  --prefix DIR   Install directory (default: \$HOME/.local/bin)
  --headless     Build without the interactive TUI
  --uninstall    Remove an installed binary instead of installing
  -h, --help     Show this help
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
        -h|--help) usage; exit 0 ;;
        *) echo "error: unknown option '$1'" >&2; usage >&2; exit 2 ;;
    esac
done

TARGET="${PREFIX}/${PROG}"

if [ "$UNINSTALL" -eq 1 ]; then
    if [ -e "$TARGET" ]; then
        rm -f "$TARGET"
        echo "removed $TARGET"
    else
        echo "nothing to remove at $TARGET"
    fi
    exit 0
fi

need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "error: '$1' not found. $2" >&2
        exit 1
    }
}

need cargo "Install Rust from https://rustup.rs and try again."
need cc "Install a C toolchain (build-essential / xcode-select --install)."

# rustls' aws-lc-sys build needs cmake; skip the check when it is absent but
# the dependency is already compiled into the cargo cache.
if ! command -v cmake >/dev/null 2>&1; then
    echo "warning: cmake not found; the build fails if aws-lc-sys is not cached yet." >&2
fi

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$SCRIPT_DIR"

echo "building $PROG (release)…"
if [ "$HEADLESS" -eq 1 ]; then
    cargo build --release --no-default-features
else
    cargo build --release
fi

BIN="target/release/${PROG}"
[ -x "$BIN" ] || { echo "error: expected binary at $BIN" >&2; exit 1; }

mkdir -p "$PREFIX"
# Replace atomically so a running instance keeps its inode.
cp "$BIN" "${TARGET}.new"
chmod 755 "${TARGET}.new"
mv -f "${TARGET}.new" "$TARGET"

echo
echo "installed $TARGET"
"$TARGET" --version

case ":${PATH}:" in
    *":${PREFIX}:"*) ;;
    *)
        echo
        echo "note: ${PREFIX} is not on your PATH. Add it with:"
        echo "  echo 'export PATH=\"${PREFIX}:\$PATH\"' >> ~/.profile"
        ;;
esac
