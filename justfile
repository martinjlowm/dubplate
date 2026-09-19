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

# Build the browser boundary, which is what keeps every library it reaches free
# of the host.
#
# A `std::fs` call or a clock reads fine on a laptop and panics in a tab, and the
# compiler is the only thing that catches the difference.
#
# The C this compiles is the Engine exporter's SQLite, and `devenv.nix` names
# the compiler and archiver it is built with: an unwrapped clang, because the
# one a shell calls `cc` is gcc on Linux and gcc cannot target wasm32. Nothing
# is set here, so the recipe says the same thing from any shell that has them.
#
# A check, not a build. Linking the module leaves one undefined import,
# `env.__stack_chk_fail`, which comes from the C runtime objects inside
# sqlite-wasm-rs rather than from anything set here: that crate's own build
# passes `-fno-stack-protector` and those objects reference the canary anyway.
# Whoever instantiates the module supplies it, because defining it here would
# take a `#[unsafe(no_mangle)]` and this workspace has no unsafe in it.
#
# One crate rather than a list: `dubplate-wasm` depends on every library that has
# to reach a tab, so adding one to it is what puts it under this gate.
check-wasm:
    cargo check --release --target wasm32-unknown-unknown -p dubplate-wasm

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
