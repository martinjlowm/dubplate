{
  pkgs,
  config,
  inputs,
  ...
}: let
  # The pinned generator, on PATH in the shell, so regenerating the graph does
  # not depend on the caller's flake registry.
  crate2nix = inputs.crate2nix.packages.${pkgs.stdenv.hostPlatform.system}.default;

  # A compiler that can target wasm32, and the archiver that goes with it.
  #
  # cc-rs picks the C compiler for whatever target it is building for, and the
  # SQLite the Engine exporter compiles is C. Left to find one itself it takes
  # whatever the shell calls `cc`: clang on a macOS shell, which can target
  # wasm32, and gcc on a Linux one, which cannot. gcc pulls in the host's glibc
  # headers and dies on `__GLIBC_USE`, which is why `just check-wasm` passed on a
  # laptop and had never once passed in CI.
  #
  # Unwrapped, on the wrapper's own advice when it is handed a target that is
  # not the host: "cc-wrapper is currently not designed with multi-target
  # compilers in mind. You may want to use an un-wrapped compiler instead."
  # Unwrapped also means no hardening flags reach a build that refuses several
  # of them, which is what the recipe used to work around by hand.
  wasmClang = pkgs.llvmPackages.clang-unwrapped;
  wasmBintools = pkgs.llvmPackages.bintools-unwrapped;

  # nixpkgs with rust-overlay, which is what supplies `rust-bin`. devenv's own
  # `pkgs` has no overlay applied, so the toolchain is resolved through this one.
  rustPkgs = import inputs.nixpkgs {
    inherit (pkgs.stdenv.hostPlatform) system;
    overlays = [(import inputs.rust-overlay)];
  };

  # One compiler for the shell and for the Nix build, read from
  # rust-toolchain.toml. Two toolchains would mean `cargo test` and
  # `devenv build` disagreeing about what compiles.
  toolchain = rustPkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

  # Only the manifests and the Rust trees. The repo root would pull in
  # ./archives and ./audio, which may be symlinks to a whole music library.
  rustSrc = pkgs.lib.fileset.toSource {
    root = ./.;
    fileset = pkgs.lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./libraries/rust
      ./tools/rust
    ];
  };

  # The crate graph is pre-resolved by `crate2nix generate --format json` into
  # a committed Cargo.json and read here through crate2nix's build-from-json
  # consumer. Reading a committed graph keeps evaluation pure. Nothing fetches
  # or resolves at eval time, which is the point, and also why it goes stale
  # silently unless a manifest change regenerates it (the cargoJsonSync hook
  # below).
  buildRustCrateForPkgs = crossPkgs:
  # Every crate in the graph is pure Rust. rustfft, hound, png and the serde
  # stack pull no C libraries, so there are no crate overrides for native
  # dependencies here. Adding a dependency that needs one is the moment to
  # add its override.
    crossPkgs.buildRustCrate.override {
      rustc = toolchain;
      cargo = toolchain;
    };

  workspace = import "${inputs.crate2nix}/lib/build-from-json.nix" {
    pkgs = rustPkgs;
    src = rustSrc;
    resolvedJson = ./Cargo.json;
    inherit buildRustCrateForPkgs;
  };

  dubplate = workspace.workspaceMembers."dubplate".build;

  # ./archives holds zip downloads and ./audio holds loose files. Both are
  # gitignored, both may be symlinks to wherever the files already live, and Nix
  # imports whatever they point at.
  mkLibrary = import ./nix/library.nix {
    inherit (pkgs) lib runCommand unzip flac lame;
    inherit dubplate;
  };

  library = mkLibrary {
    archives = ./archives;
    audio = ./audio;
  };

  device = import ./nix/device.nix {
    inherit (pkgs) lib runCommand dosfstools mtools;
    inherit dubplate;
  };

  # One USB image per format, each carrying that format's files and both
  # databases built from the same analysis.
  imageFor = format:
    device.image {
      name = "usb-${format}";
      audio = library.${format};
      inherit (library) analysis;
      label = "MUSIC";
    };

  usb = device.imageSet {
    images = {
      wav = imageFor "wav";
      flac = imageFor "flac";
      mp3 = imageFor "mp3";
    };
  };
in {
  packages = [
    toolchain
    crate2nix
    pkgs.just
    # The GitHub Actions workflows are generated from infrastructure/ci-cd by
    # cdkactions, which is a TypeScript library, so a Rust repository carries a
    # JavaScript runtime for that one job. Bun rather than node plus a package
    # manager: there are two dependencies, both public, and it runs the
    # definition directly.
    pkgs.bun
  ];

  # Named per target, so they steer the wasm build and leave every native build
  # to the shell's own compiler.
  env = {
    CC_wasm32_unknown_unknown = "${wasmClang}/bin/clang";
    AR_wasm32_unknown_unknown = "${wasmBintools}/bin/llvm-ar";
  };

  # `treefmt` on PATH in the shell. The formatter set lives in ./treefmt.nix and
  # is imported rather than restated, so the shell and CI cannot drift apart.
  treefmt = {
    enable = true;
    config.imports = [./treefmt.nix];
  };

  git-hooks.hooks.treefmt.enable = true;

  # Cargo.json is the pre-resolved crate graph `devenv build` reads. Nothing
  # regenerates it at eval time, so a dependency change has to regenerate it
  # here or the Nix build keeps compiling the previous dependency set.
  #
  # `-h` points at a temp file. The JSON format records no arguments, so the
  # path never reaches Cargo.json, and the hashes it would hold are already in
  # Cargo.json: registry hashes come from Cargo.lock and git hashes are
  # prefetched into it.
  #
  # Git exports GIT_DIR and friends to hooks, which nix-prefetch-git would
  # inherit and use to reinitialise this repo instead of its own scratch
  # directory, so they are unset.
  git-hooks.hooks.cargoJsonSync = {
    enable = true;
    name = "cargo-json-sync";
    entry = ''
      ${pkgs.bash}/bin/bash -euo pipefail -c '
        cd "${config.devenv.root}"
        unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_COMMON_DIR GIT_PREFIX
        tmp=$(mktemp -d)
        trap "rm -rf $tmp" EXIT
        ${crate2nix}/bin/crate2nix generate --format json -o Cargo.json -h "$tmp/crate-hashes.json"
      '
    '';
    files = "Cargo\\.(toml|lock|json)$";
    pass_filenames = false;
    stages = ["pre-commit" "manual"];
  };

  # `devenv build outputs.dubplate` builds the CLI through the crate graph
  # rather than through the local cargo cache, which is what CI checks and what
  # anyone with Nix can reproduce.
  # Both trees nest the same way: build the whole thing, or one format of it.
  #
  #   outputs.library         every track named <BPM>_<KEY>_<original name>,
  #                           the three formats side by side
  #   outputs.library.flac    that one format, and .analysis for the reports
  #   outputs.usb             a FAT32 image per format, side by side
  #   outputs.usb.flac        that one image, to copy to a stick with dd
  #
  # library is one derivation with several outputs; usb is three derivations
  # behind one, since each image is built from different audio.
  outputs = {
    inherit dubplate library usb;
  };
}
