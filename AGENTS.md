# Agent instructions: dubplate

A Rust CLI that measures tempo and key in a WAV file and writes out the evidence: the novelty
curve, the tempo salience of every competing candidate, the beat grid over the onsets, a
spectrogram, and a list of the reasons the answer might be wrong.

The reason it exists is troubleshooting. Other tools print a number; when that number is wrong
there is nothing to inspect. Any change that makes this one more accurate and less inspectable
is the wrong trade.

## 1. Where things are

- `libraries/rust/audio` decoding, mono downmix, excerpting, and the synthesised signals the
  tests measure against.
- `libraries/rust/spectral` the STFT and its two reductions, log-spaced band energies and
  chroma, plus the tuning estimator.
- `libraries/rust/tempo` novelty curves, tempo salience, beat grid, and the rules in
  `diagnostics.rs` that name a doubtful answer.
- `libraries/rust/key-detect` chroma to key by profile correlation, with Camelot notation.
- `libraries/rust/diagnostics` the finding type every stage reports doubts in.
- `libraries/rust/energy` how hard a track hits, from brightness, crest factor, onset
  density and the share above 4 kHz. Tempo is deliberately not among them.
- `libraries/rust/report` the JSON report, the SVG plots, the PNG spectrogram, the HTML page.
- `libraries/rust/pipeline` the pass itself: `lib.rs` runs every stage over one transform,
  `figures.rs` draws, `options.rs` holds the settings and their defaults. Samples in, a report
  and a set of named artefacts out, with no file, clock or environment touched.
- `tools/rust/dubplate` the CLI. `main.rs` parses flags and writes what the pipeline returned,
  `summary.rs` prints, `rename.rs` builds the `<BPM>_<KEY>_<rest>` name, `export.rs` writes a
  device.
- `libraries/rust/collection` the device-neutral track model, and the reader that builds one
  from a report. Neither exporter knows how the numbers were measured.
- `libraries/rust/waveform` the columns both target formats draw, computed once.
- `libraries/rust/rekordbox` `export.pdb` and the `ANLZ` files, with rekordcrate as the test
  oracle.
- `libraries/rust/engine` the Engine Library database, schema 2.21.2, with Denon's own DDL as
  the check.
- `nix/library.nix` the archive pipeline: one derivation per track, three format outputs.
- `nix/device.nix` the device tree and the FAT32 image built from it.
- `infrastructure/ci-cd` the CI workflows as a cdkactions app; `.github/workflows` is its
  output.

A crate's home follows from its role. Reusable stages go in `libraries/rust/`, things you run
go in `tools/rust/`. Both are globs in the workspace manifest, so adding a crate needs no
edit to `Cargo.toml`.

## 2. Non-negotiable constraints (load-bearing, do not simplify away)

1. **No stage silently corrects another.** Two rules change the reported number and both say
   so. The metrical floor emits `metrical-floor-applied` with both saliences every time it
   fires. The rounding to a whole number keeps the measurement in `tempo.bpm_measured` and
   prints it on the headline whenever it moved the answer. It rounds whatever the gap, because
   the grid, the file name and both device databases take the reported number and none of them
   has a use for 122.66; a gap wider than `--integer-snap` raises `non-integer-tempo`, which
   is what says the grid under that answer drifts. A third such rule needs the
   same treatment or it does not go in: a detector that quietly fixes itself is what this
   tool exists to troubleshoot.
2. **Diagnostics never change the answer.** They read what the stage already produced. A rule
   that adjusts a result is not a diagnostic.
3. **The caller never names a tempo.** Leaning the search toward a tempo improves the
   average case and is exactly how a 174 BPM track gets reported as 87, so no flag takes a
   tempo: a number a person picks is a guess about the answer, and this tool exists because
   guesses about the answer are what go wrong. The one lean that is applied is chosen by a
   measurement, and it is called a tempo weighting rather than a prior, because nobody
   supplies it and `TempoWeighting` says what it does to the salience curve. The energy in
   `libraries/rust/energy` puts a track in one of three bands and each band names a centre,
   and its four inputs read nothing the tempo stage produces, because a track reported at
   twice its tempo would otherwise carry that error into the measurement meant to catch it.
   Nothing switches it off, because the report says what it did: `energy-band-applied` names
   the band and the centre, `bpm_unweighted` carries the answer the track would have had
   without the weighting, and `weighting-changed-answer` fires when those two differ. On the working set
   the bands correct two tracks, change nothing among the other twenty-one and leave the
   selftest untouched. The band centres were chosen by sweeping against the three tracks
   whose tempo anybody has checked, so they are fitted to three examples and not a proof;
   the twenty others are assumed right rather than verified.
4. **The comb penalty defaults to 0.** Subtracting the autocorrelation between comb teeth
   rejects subharmonics, and it fires just as hard on the true tempo of anything with offbeat
   movement. At full weight it reports 106.64 BPM for a 160 BPM hardstyle track. Do not
   restore a non-zero default without measuring the whole working set again.
5. **`report.json` is the contract.** Every terminal line, every table and every plot is
   derived from it. Nothing is computed for display only.
6. **Chroma folds spectral peaks, not every bin.** An FFT bin is a fixed width in hertz and a
   semitone is not, so mapping every bin bakes the transform geometry into the result. The
   version that did reported F major for sixteen of seventeen tracks.
7. **One transform pass, nothing kept whole.** A ten-minute track is about 52 000 frames of
   1025 bins. `Stft::for_each_frame` hands each spectrum to a callback and reuses the buffer.
   Only the tuning estimate gets a second, strided pass, because the chroma mapping is built
   around it.
8. **The moving average subtracted from the flux must stay wider than a beat period.** At
   0.5 s and 60 BPM it removes the pulse being measured. If a default changes, check the slow
   end of the tempo range.
9. **Tests measure against synthesised signals** from `audio::synth`, never against a track. A
   fixture whose true tempo is an assumption fails for two reasons and cannot separate them.
10. **Audio never enters the repository.** `.gitignore` covers `*.wav` and the rest. The
    working set sits untracked in the repo root.
11. **`Cargo.nix` is generated.** Fix the manifest and regenerate with `just sync-cargo-nix`.
    The pre-commit hook does it for you; CI fails when the graph and the manifests disagree.
12. **Renaming never touches the source.** `rename` writes links or copies under a directory
    you name, and the default mode changes nothing at all. The Nix pipeline is the same rule
    at scale: the archives are inputs, the named collection is an output.
13. **The tool reads audio and nothing else.** No network, no metadata tags, no online
    lookup. Every number is measured from the samples. Artist and title come from the file
    name, which is what the shop wrote.
14. **The exporters are verified against rekordcrate, and it stays pinned to a commit at
    least as current as the format analysis.** Any change to a row or section layout keeps
    the round-trip tests passing. The published 0.3.0 is two years behind and reads a cue
    point's type as 0 where the current analysis uses 1; that is why the dependency is a git
    revision and not a version range.
15. **Fields the format analysis calls unknown carry the constants real exports carry.**
    They are not padding. Zeroing one because nobody has explained it is how a stick becomes
    unreadable on a player nobody here owns. Every layout is a `deku` struct with its field
    widths, endianness and bit order declared, so an unknown field is a named field with a
    comment rather than a number in a byte stream. A new layout is declared the same way; the
    offset arrays and the page packing stay hand-written, because where a row lands depends on
    what came before it and a derive macro cannot say that.
16. **`Track::device_path` is the only place a file's location on the device is decided.**
    The database, the analysis files and the image builder all read it. Two of them computing
    a path separately is two of them disagreeing.
17. **The Engine schema is Denon's and is not tidied.** Column names, the misspelt
    `currentPlayedIndiciator` and `isPerfomanceDataOfPackedTrackChanged` among them, are what a
    player looks for. The triggers do work on insert, so rows are written with the columns
    they fill left alone. `PerformanceData` is a view, and the analysis blobs reach the track
    row through its `INSTEAD OF` trigger, which is why this file is built by SQLite rather than
    assembled: `engine::build` opens an in-memory database, runs the DDL and the inserts, and
    returns `sqlite3_serialize` of the result, so the bytes on the stick are the pages SQLite
    wrote. `write_device` is that plus a write.
18. **Dates come from `SOURCE_DATE_EPOCH` when it is set.** Otherwise the same library
    exports to two different images, which makes every Nix rebuild copy gigabytes for a field
    nobody can hear.
19. **The analysis and both exporters touch no file, clock or environment variable.**
    `libraries/rust/pipeline` takes samples and returns a report and named artefacts, and both
    exporters return bytes; opening the file, writing what came back and deciding what today
    is belong to the CLI. That is what lets the same code run in a browser, where `std::fs`
    compiles and then panics, and `just check-wasm` is the gate that catches a regression:
    `cargo check --target wasm32-unknown-unknown` over the pipeline and both exporters. A
    `SystemTime::now()` added to a library reads fine on a laptop and takes the wasm build with
    it. Two things that build needs, both recorded where they are used: rusqlite 0.40 or newer,
    whose default `ffi-sqlite-wasm-rs` swaps the bundled SQLite for one that compiles against a
    wasm shim, and the unwrapped clang `devenv.nix` names in
    `CC_wasm32_unknown_unknown`. cc-rs otherwise takes whatever the shell calls `cc`, which is
    clang on macOS and gcc on Linux, and gcc cannot target wasm32; that is why this gate passed
    on a laptop and failed in CI for every run the repository had.

## 3. What a change to the algorithm has to show

Any edit to onset detection, tempo salience or key correlation is measured before it is
committed:

```sh
just selftest --bpm 100    # and 128, 145, 174, 200
for f in *.wav; do just analyze "$f" --no-figures -o /tmp/sweep; done
```

The selftest error stays under 0.5% at every tempo, and it reads `bpm_measured`, so a rounded
answer cannot hide a regression from it.

In the sweep, read `bpm_measured` rather than `bpm`: the reported tempo is snapped to a whole
number and would look right while the measurement behind it drifted. Produced electronic music
is written at integer tempi, so the number that matters is the gap between the measurement and
its nearest integer. Across the working set that gap is at most 0.164 BPM, at 200 BPM. A change
that widens it, or that makes `non-integer-tempo` fire on a track that used to snap, is a
regression whatever the headline says:

```sh
for f in *.wav; do just analyze "$f" --no-figures -o /tmp/sweep/"${f%.wav}"; done
jq -r '[.tempo.bpm_measured, (.tempo.bpm - .tempo.bpm_measured)] | @tsv' /tmp/sweep/*/report.json
```

Say in the pull request what moved.

## 4. Out of scope

Formats other than WAV in the Rust code, tag writing, playlists, stem separation, and anything
that plays audio. The archive pipeline handles other formats by decoding them with `flac` and
`lame` before the analyser sees them, which keeps one decoder per format and each of them the
reference implementation. If in-process decoding is ever wanted, it belongs behind the same
`Audio` type in `libraries/rust/audio` and nothing downstream should notice.

## 5. Code conventions

### Comments

Prefer a descriptive name to a comment. Keep comments that explain a why, a guard, or a
non-obvious consequence, and the ones that record a measurement, such as why the comb penalty
is off. Drop comments that restate the code. The comments naming failure modes in
`tempo/src/` and `spectral/src/chroma.rs` are load-bearing.

### Tests

As few as possible without losing coverage. One integration test per crate boundary, plus the
CLI test that runs the binary. Assertions carry the measured value in the failure message, so
a failure reads as a number rather than as `assertion failed`.

### Rust

- `snake_case` for variables and functions, `PascalCase` for types.
- Files and directories are `kebab-case`.
- No `unsafe`, no panics on user input. The CLI returns `anyhow::Result` and the libraries
  return typed errors.
- Clippy with `-D warnings` is a gate, not advice.

### Prose

Documentation follows four modes and keeps them apart. A tutorial teaches one path that works,
a how-to gets a competent reader to a goal, a reference describes the machinery, and an
explanation says why the code is shaped this way. `README.md` carries all four in that order,
`docs/01` explains, `docs/02` is a how-to, `docs/03` is reference. A reference entry is read
out of the code before it is written down.

No em dashes. Sentence case in headings. Name the actor rather than writing in the passive.

## 6. Tooling

- **just** runs everything: `just check` is the PR gate, `just analyze` and `just selftest`
  are the two things you run while working. Recipes take their arguments as positional
  parameters (`set positional-arguments`, then `"$@"`), never as `{{ARGS}}` spliced into the
  command line: a track is called `Artist,_Other-Title_(Extended_Mix).wav`, and interpolating
  that unquoted hands the shell an ampersand and a subshell to parse.
- **devenv** provides the shell: the toolchain pinned in `rust-toolchain.toml`, `just`,
  `treefmt`, `crate2nix`. `direnv allow`, or `devenv shell`.
- **deku** carries every device layout: `#[deku(endian, bits, bit_order)]` on a struct instead
  of `extend_from_slice(&x.to_be_bytes())`. It refuses a value too wide for its field rather
  than truncating it, which is why the packed waveform columns pass through `Column::clamped`
  first. It costs about 110 ms per track in the export step, which the volume here can afford.
- **crate2nix** resolves the crate graph ahead of time into `Cargo.nix`, which
  `devenv build outputs.dubplate` reads. Nothing fetches during evaluation.
- **outputs.library** builds every track in `archives/` and `audio/` into `wav/`, `flac/` and
  `mp3/`, and **outputs.usb** builds an image from each. Both nest: `outputs.library.flac` is
  one output of a multi-output derivation, `outputs.usb.flac` is one derivation hanging off
  another's passthru. Adding a variant means adding it in one place, not three. The library
  lists archive contents through import from derivation, so evaluation builds the manifest
  before it knows what the tracks are.
- **treefmt** formats Rust and Nix from one definition in `treefmt.nix`, used by the shell,
  the git hook and CI.
- **cdkactions** generates `.github/workflows/*.yaml` from `infrastructure/ci-cd/main.ts`.
  Never edit the YAML: it carries a "Do not modify" header, `just synth-workflows`
  regenerates it, and both `just check` and a CI job fail on any difference. Bun runs it,
  which is the only reason this repository has a JavaScript runtime.

## 7. Git and pull requests

Commit messages and pull request titles follow `<type>(<scope>): <description>`.

- `<type>` is one of `feat`, `fix`, `chore`, `docs`.
- `<scope>` is the crate or area: `tempo`, `spectral`, `key-detect`, `report`, `cli`, `docs`,
  `ci`.
- Multiple scopes read `feat(tempo,cli): ...`; repository-wide changes read `chore(*): ...`.

A pull request that changes an algorithm carries the sweep numbers from section 3.
