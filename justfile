_default:
    @just --list

# Build the CLI
build:
    cargo build --release

# Analyse a file: just analyze "track.wav" [--start 60 --duration 60 …]
analyze +ARGS:
    cargo run --release --bin music-analyze -- analyze {{ARGS}}

# Name files after what was measured in them: just rename <file|dir>... [flags]
rename +ARGS:
    cargo run --release --bin music-analyze -- rename {{ARGS}}

# Build the renamed library as one Nix output, from ./archives and ./audio
library:
    devenv build outputs.library

# Build a FAT32 USB image for one format: just usb flac
usb FORMAT:
    devenv build outputs.usb-{{FORMAT}}

# Write a device tree from analysed tracks: just export <dir> --audio A --reports R
export OUT +ARGS:
    cargo run --release --bin music-analyze -- export --out {{OUT}} {{ARGS}}

# Measure a generated pulse train, which has no ambiguity to hide behind
selftest *ARGS:
    cargo run --release --bin music-analyze -- selftest {{ARGS}}

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

# Regenerate the crate graph after a dependency change
sync-cargo-nix:
    #!/usr/bin/env bash
    set -euo pipefail
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    crate2nix generate -h "$tmp/crate-hashes.json"

# Fail if Cargo.nix no longer matches the manifests
check-cargo-nix: sync-cargo-nix
    git diff --exit-code -- Cargo.nix

# Build the CLI through Nix, against the committed crate graph
build-nix:
    devenv build outputs.music-analyze

# All PR gates
check: fmt-check lint test check-cargo-nix
