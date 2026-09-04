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
    cargo run --release --bin music-analyze -- analyze "$@"

# Name files after what was measured in them: just rename <file|dir>... [flags]
rename +ARGS:
    cargo run --release --bin music-analyze -- rename "$@"

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
    cargo run --release --bin music-analyze -- export --out "$out" "$@"

# Measure a generated pulse train, which has no ambiguity to hide behind
selftest *ARGS:
    cargo run --release --bin music-analyze -- selftest "$@"

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
    devenv build outputs.music-analyze

# All PR gates
check: fmt-check lint test check-cargo-nix check-workflows
