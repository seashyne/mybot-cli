---
title: Install
permalink: /install/
---

# Installing mybot cli

mybot is a Rust terminal coding agent. It ships as a single binary; the only
runtime dependency is `bash` for the `bash` tool.

## Quick start (Linux / macOS)

### One line from a URL

Downloads the prebuilt binary for your platform into `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh | sh
```

Options are passed after `sh -s --`:

```bash
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh \
  | sh -s -- --prefix /usr/local/bin
# pin a release
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh \
  | sh -s -- --version v0.2.0
# remove it
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh \
  | sh -s -- --uninstall
```

The script verifies `sha256sums.txt` when the release publishes one.

### npm

```bash
npm install -g mybot-cli
mybot
```

The package is a small wrapper that downloads the native binary on first run
(also works with `npx mybot-cli`). Set `MYBOT_VERSION=v0.2.0` to pin a release.

### Build from source

```bash
# Ubuntu/Debian build prerequisites
sudo apt-get update
sudo apt-get install -y build-essential cmake perl pkg-config git

# Rust 1.85+ (edition 2024)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

git clone https://github.com/seashyne/mybot-cli.git
cd mybot-cli
./install.sh
mybot --help
```

`install.sh` in a checkout builds from source; piped from a URL it downloads a
release. Force either mode with `--from-source` / `--from-release`.

Or use cargo, which installs into `~/.cargo/bin`:

```bash
cargo install --path .
# or, without cloning:
cargo install --git https://github.com/seashyne/mybot-cli.git
```

## Requirements

| Requirement | Why |
| --- | --- |
| Rust 1.85+ | The crate uses edition 2024 |
| C toolchain (gcc/clang) | `aws-lc-sys`, the TLS backend pulled in by rustls |
| CMake | `aws-lc-sys` build script |
| Perl | AWS-LC assembly generation on some targets |
| `pkg-config` | Native dependency discovery |
| `git` | Fetching the source |
| `bash` | The `bash` tool runs `bash -lc` |

macOS: install the Xcode command line tools and CMake first.

```bash
xcode-select --install
brew install cmake
```

## Platform support

| Platform | Architecture | Status | Notes |
| --- | --- | --- | --- |
| Linux | x86_64 | Tested | Primary development platform |
| Linux | aarch64 | Tested via Docker/QEMU | Raspberry Pi OS 64-bit, ARM servers |
| macOS | x86_64, arm64 | Expected to work | Build from source; no macOS hardware in this repo's test environment |
| Windows 10/11 | x86_64 | Best effort | Use WSL2 (recommended) or Git Bash; `bash` must be on PATH |
| Android Termux | aarch64 | Experimental | Untested; needs bash and a full Rust toolchain |
| wasm, iOS | — | Not supported | Requires process spawning and a terminal |

Windows caveats:

- The `bash` tool invokes `bash -lc`, so WSL2 or Git Bash is required.
- Process-group termination is a no-op outside Unix (`src/tools/bash.rs`), so a
  timed-out command may leave child processes running.
- Native Windows builds are not part of the tested matrix; WSL2 is the
  recommended path.

## Headless-only build

For servers and CI images that do not need the TUI:

```bash
cargo build --release --no-default-features
```

This drops ratatui and crossterm. `mybot exec` and the provider commands remain
available; running `mybot` without a subcommand prints a usage error.

## ARM64

### Build on the device (recommended)

On a Raspberry Pi 5 or an ARM server running a 64-bit OS:

```bash
sudo apt-get install -y build-essential cmake perl pkg-config git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://github.com/seashyne/mybot-cli.git
cd mybot-cli
cargo build --release
```

With limited RAM, reduce parallelism:

```bash
cargo build --release -j2
```

### Cross-compile from x86_64

```bash
rustup target add aarch64-unknown-linux-gnu
sudo apt-get install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu cmake

export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++

cargo build --release --target aarch64-unknown-linux-gnu
```

The `CC_`/`CXX_` variables are required because `aws-lc-sys` compiles C code for
the target. For a fully static binary, use `aarch64-unknown-linux-musl` and a
matching musl cross toolchain instead.

### Docker/QEMU

Register ARM64 emulation once:

```bash
docker run --privileged --rm tonistiigi/binfmt --install arm64
```

Build and extract the binary:

```bash
docker run --rm --platform linux/arm64 \
  -v "$PWD":/work -v "$PWD/out":/out -w /work \
  -e CARGO_TARGET_DIR=/tmp/target rust:1-bookworm \
  sh -c "cargo build --release && cp /tmp/target/release/mybot /out/mybot-linux-arm64"
```

Run the test suite on ARM64 the same way:

```bash
docker run --rm --platform linux/arm64 \
  -v "$PWD":/work -w /work -e CARGO_TARGET_DIR=/tmp/target \
  rust:1-bookworm sh -c "cargo test && cargo clippy --all-targets"
```

This repository verifies ARM64 with a release build and the full test suite
under Docker 29 with QEMU binfmt (148 tests: 136 unit + 12 integration);
clippy is verified on the host. QEMU is slow; use it for correctness, not for
performance numbers.

## Verify the installation

```bash
mybot --help
mybot provider list
mybot provider test anthropic   # needs ANTHROPIC_API_KEY
```

From a source checkout:

```bash
cargo test
cargo clippy --all-targets
```

## Update

Re-run the installer; it replaces the binary in place:

```bash
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh | sh
```

From a checkout:

```bash
git pull
./install.sh
```

With cargo:

```bash
cargo install --path . --force
```

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/seashyne/mybot-cli/main/install.sh \
  | sh -s -- --uninstall
npm uninstall -g mybot-cli     # if installed through npm
cargo uninstall mybot          # if installed through cargo
rm -rf ~/.mybot                # config, sessions, global AGENTS.md
```

## Troubleshooting

**`error: linker 'cc' not found` or CMake errors**
Install the C toolchain and CMake listed under Requirements.

**`GLIBC_x.y not found` on an older server**
Build on the oldest distribution you need to support, or cross-compile with the
musl target for a static binary.

**`bash: command not found` on Windows**
Use WSL2 or install Git Bash and add it to `PATH`.

**`aws-lc-sys` fails while cross-compiling**
Set `CC_<target>`, `CXX_<target>`, and
`CARGO_TARGET_<TARGET>_LINKER` to the cross tools, as shown above.

**The build runs out of memory on a small ARM board**
Use `cargo build --release -j1` or `-j2`, and add swap if needed.
