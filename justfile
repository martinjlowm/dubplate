# Arguments reach a recipe as positional parameters rather than being spliced
# into the command line as text. A track is called
# "Artist,_Other-Title_(Extended_Mix).wav", and interpolating that unquoted
# hands the shell an ampersand and a subshell to parse.
set positional-arguments

_default:
    @just --list

# Build the CLI
build:
    cargo build --release

# Analyse a file: just analyze "track.wav" [--start 60 --duration 60 …]
analyze +ARGS:
    cargo run --release --bin dubplate -- analyze "$@"

# Name files after what was measured in them: just rename <file|dir>... [flags]
rename +ARGS:
    cargo run --release --bin dubplate -- rename "$@"

# Build the renamed library as one Nix output, from ./archives and ./audio
library:
    devenv build outputs.library

# Build FAT32 USB images: just usb (all three) or just usb flac (one)
usb FORMAT="":
    devenv build outputs.usb{{ if FORMAT == "" { "" } else { "." + FORMAT } }}

# Write a device tree from analysed tracks: just export <dir> --audio A --reports R
export OUT +ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    out="$1"
    shift
    cargo run --release --bin dubplate -- export --out "$out" "$@"

# Measure a generated pulse train, which has no ambiguity to hide behind
selftest *ARGS:
    cargo run --release --bin dubplate -- selftest "$@"

# Every test in the workspace
test:
    cargo test --release

# Clippy, with warnings fatal
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Format every language (rustfmt, alejandra) via treefmt
fmt:
    treefmt

# Fail if anything is unformatted
fmt-check:
    treefmt --fail-on-change

# Build the analysis and both exporters for the browser, which is what keeps them
# free of the host.
#
# A `std::fs` call or a clock reads fine on a laptop and panics in a tab, and the
# compiler is the only thing that catches the difference.
#
# The hardening list is nixpkgs minus `zerocallusedregs`. Its clang wrapper adds
# `-fzero-call-used-regs=used-gpr` by default, clang refuses that option for a
# wasm target, and the SQLite the Engine exporter compiles is the C that trips
# over it.
check-wasm:
    NIX_HARDENING_ENABLE="fortify stackprotector pic strictoverflow format relro bindnow" \
        cargo check --release --target wasm32-unknown-unknown -p pipeline -p rekordbox -p engine

# Regenerate .github/workflows from infrastructure/ci-cd/main.ts (cdkactions)
synth-workflows:
    cd infrastructure/ci-cd && bun install --frozen-lockfile && bun main.ts

# Fail if the committed workflows are not what that definition synthesises
check-workflows: synth-workflows
    git diff --exit-code -- .github/workflows/

# Regenerate the crate graph after a dependency change
sync-cargo-nix:
    crate2nix generate -h .crate-hashes.json

# Fail if Cargo.nix no longer matches the manifests
check-cargo-nix: sync-cargo-nix
    git diff --exit-code -- Cargo.nix

# Build the CLI through Nix, against the committed crate graph
build-nix:
    devenv build outputs.dubplate

# All PR gates
check: fmt-check lint test check-wasm check-cargo-nix check-workflows
