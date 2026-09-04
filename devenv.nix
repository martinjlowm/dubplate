{
  pkgs,
  config,
  inputs,
  ...
}: let
  # The pinned generator, on PATH in the shell, so regenerating the graph does
  # not depend on the caller's flake registry.
  crate2nix = inputs.crate2nix.packages.${pkgs.stdenv.hostPlatform.system}.default;

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

  # The crate graph is pre-resolved by `crate2nix generate` into a committed
  # Cargo.nix and consumed here through buildRustCrate. Reading a committed
  # graph keeps evaluation pure. Nothing fetches or resolves at eval time,
  # which is the point, and also why it goes stale silently unless a manifest
  # change regenerates it (the cargoNixSync hook below).
  buildRustCrateForPkgs = crossPkgs:
  # Every crate in the graph is pure Rust. rustfft, hound, png and the serde
  # stack pull no C libraries, so there are no crate overrides for native
  # dependencies here. Adding a dependency that needs one is the moment to
  # add its override.
    crossPkgs.buildRustCrate.override {
      rustc = toolchain;
      cargo = toolchain;
    };

  workspace = import ./Cargo.nix {
    pkgs = rustPkgs;
    inherit buildRustCrateForPkgs;
    release = true;
  };

  music-analyze = workspace.workspaceMembers."music-analyze".build;

  # ./archives holds zip downloads and ./audio holds loose files. Both are
  # gitignored, both may be symlinks to wherever the files already live, and Nix
  # imports whatever they point at.
  mkLibrary = import ./nix/library.nix {
    inherit (pkgs) lib runCommand unzip flac lame;
    inherit music-analyze;
  };

  library = mkLibrary {
    archives = ./archives;
    audio = ./audio;
  };

  device = import ./nix/device.nix {
    inherit (pkgs) lib runCommand dosfstools mtools;
    inherit music-analyze;
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

  # `treefmt` on PATH in the shell. The formatter set lives in ./treefmt.nix and
  # is imported rather than restated, so the shell and CI cannot drift apart.
  treefmt = {
    enable = true;
    config.imports = [./treefmt.nix];
  };

  git-hooks.hooks.treefmt.enable = true;

  # Cargo.nix is the pre-resolved crate graph `devenv build` reads. Nothing
  # regenerates it at eval time, so a dependency change has to regenerate it
  # here or the Nix build keeps compiling the previous dependency set.
  #
  # `-h` names a gitignored file rather than a temp one, and the path matters:
  # crate2nix records the arguments it was called with in a comment at the top
  # of Cargo.nix, so a temp path there makes the file differ on every run and
  # the staleness check can never pass. The hashes themselves are redundant,
  # since registry hashes come from Cargo.lock and are baked into Cargo.nix.
  #
  # Git exports GIT_DIR and friends to hooks, which nix-prefetch-git would
  # inherit and use to reinitialise this repo instead of its own scratch
  # directory, so they are unset.
  git-hooks.hooks.cargoNixSync = {
    enable = true;
    name = "cargo-nix-sync";
    entry = ''
      ${pkgs.bash}/bin/bash -euo pipefail -c '
        cd "${config.devenv.root}"
        unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_COMMON_DIR GIT_PREFIX
        ${crate2nix}/bin/crate2nix generate -h .crate-hashes.json
      '
    '';
    files = "Cargo\\.(toml|lock|nix)$";
    pass_filenames = false;
    stages = ["pre-commit" "manual"];
  };

  # `devenv build outputs.music-analyze` builds the CLI through the crate graph
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
    inherit music-analyze library usb;
  };
}
