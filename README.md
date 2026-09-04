# music-analyze

Tempo and key analysis you can argue with, and a USB stick built from it.

Most tools print one number. When that number is wrong, and for a track with a half-time
intro or an offbeat bassline it often is, there is nothing to inspect and no way to tell a
bad recording from a bad algorithm. This one writes out the curve the tempo came from, the
salience of every competing tempo, the beat grid drawn over the onsets it was fitted to, and
a list of the reasons it might be wrong. The answer is a claim with its evidence attached.

The same measurements then go onto a stick a player will browse. A Nix pipeline takes the
zips as they were downloaded, analyses every track in parallel, names each file after its
tempo and key, and writes a rekordbox database for Pioneer gear and an Engine Library for
Denon onto a FAT32 image, with the beat grids, cues and waveforms both read.

Keys are Camelot throughout: `8A` rather than `A minor`, on the terminal, in the report, in
the file names and in both databases.

Everything is measured on the file. Nothing is looked up, and the tool never talks to a
network.

---

## In and out

Put the zips where they landed. Get a stick that plays.

```
  archives/*.zip          ┌──────────────────────────────┐        outputs.usb.wav  ──►  wav.img
  audio/*.wav             │  unpack, analyse each track  │        outputs.usb.flac ──►  flac.img
  audio/*.flac      ──►   │  in its own derivation,      │  ──►   outputs.usb.mp3  ──►  mp3.img
  audio/*.mp3             │  name, encode, write both    │        outputs.usb      ──►  all three
                          │  databases, make the image   │
                          └──────────────────────────────┘
```

Each image is one FAT32 filesystem, which is what a CDJ mounts and what a Denon deck reads
too, holding:

| On the image | For |
|---|---|
| `/Contents/138_03A_Artist-Title.flac` | The audio, named after its own tempo and key, zero padded so a plain listing sorts by tempo and then around the wheel. |
| `/PIONEER/rekordbox/export.pdb` | **Pioneer.** The DeviceSQL database a CDJ, XDJ or RX3 browses: tracks, artists, keys, one playlist. |
| `/PIONEER/USBANLZ/…/ANLZ0000.DAT` and `.EXT` | **Pioneer.** Per track: the beat grid, the cue, the monochrome waveforms and the colour pair a Nexus 2 or newer draws. |
| `/Engine Library/Database2/m.db` | **Denon.** Engine schema 2.21.2, which Engine DJ 2 and 3 read: tempo, key, beat grid, cue slots, overview waveform. |

Both databases go on every image. They read different directories, neither player looks at
the other's, and the audio is shared, so one stick works in whichever booth you walk into.

A WAV lands in all three images, encoded to FLAC and MP3 on the way. A FLAC or an MP3 lands
in its own format only, because transcoding a lossy source spends CPU to lose more.

```sh
just usb flac        # or: just usb, for all three
```

`devenv build` prints the store path it wrote, and the image is inside it. Copying one to a
stick is `dd`, so read the disk number twice:

```sh
dd if=/nix/store/…-usb-flac.img/usb-flac.img of=/dev/diskN bs=4m
```

---

## Tutorial

A first pass, from a checkout to a report you can read. Roughly ten minutes, and you need one
WAV file.

1. **Enter the dev shell.** It provides the pinned Rust toolchain, `just`, `treefmt` and
   `crate2nix`.

   ```sh
   direnv allow        # or, without direnv:  devenv shell
   ```

2. **Measure something with a known answer.** The selftest generates a pulse train at a tempo
   you name and runs the whole pipeline over it.

   ```sh
   just selftest --bpm 174
   ```

   ```
   generated 174.00 BPM, measured 173.97 BPM, error 0.026 BPM (0.02%)
   ```

   A failure here is a defect in this repo. Every later surprise is about the track.

3. **Analyse a track.**

   ```sh
   just analyze "Ferry_Corsten,_Kosheen-Catch_(Extended_Mix).wav"
   ```

   ```
   137.99 BPM   3A   7:03 analysed
     grid      99% of beats within 50 ms, pulse 4.98x the mean, bar phase 1 (contrast 1.60)
     windows   median 137.99 BPM, spread 0.01 BPM, 98% agree
     also      68.97 (0.88), 91.98 (0.73), 184.14 (0.44)
     ! octave-ambiguity: 91.99 BPM (two-thirds) scores 87% of 137.99 BPM, so the octave is
       decided by a margin too small to defend
     - metrical-floor-applied: the strongest candidate was 68.97 BPM (salience 0.882); the
       answer was doubled to 137.99 BPM (salience 0.835) to clear the metrical floor
     wrote     analysis/Ferry_Corsten,_Kosheen-Catch_(Extended_Mix)/report.html
   ```

   Read that second line first. 99% of beats landed within 50 ms of an onset and the grid sits
   on novelty five times the track mean, so 138 is not a guess. The `!` line is the tool
   disagreeing with itself, and step 5 is where you settle it.

4. **Open the report.**

   ```sh
   open analysis/*/report.html
   ```

   Seven figures, each captioned with what it would look like if the stage above it had gone
   wrong. Start with the novelty plot. Grid lines sitting on the peaks mean the tempo is
   right; lines drifting off them across the window mean it is close and wrong.

5. **Settle the ambiguity yourself.** The warning says 91.99 BPM scores 87% of the winner.
   Force it and compare the grid fits:

   ```sh
   just analyze "Ferry_Corsten,_Kosheen-Catch_(Extended_Mix).wav" --min-bpm 88 --max-bpm 96
   ```

   ```
   91.98 BPM   3A   7:03 analysed
     grid      91% of beats within 50 ms, pulse 2.26x the mean, bar phase 1 (contrast 2.48)
   ```

   91% of beats still land on an onset, because 92 and 138 are a two-to-three relation and
   half of a 92 BPM grid falls on beats of the 138 one. The number that separates them is the
   pulse ratio: 4.98 against 2.26. The 138 grid sits on novelty five times the track mean, the
   92 grid on barely twice it. 138 stands, and that comparison rather than the salience
   ranking is what decides an octave.

6. **See a knob change the answer.** The metrical floor is what turned 68.97 into 137.99.
   Turn it off:

   ```sh
   just analyze "Ferry_Corsten,_Kosheen-Catch_(Extended_Mix).wav" --metrical-floor 0
   ```

   The tool now reports 68.97 BPM, which is what the salience curve actually says. Neither run
   is lying. One of them applies a rule about how dance music is counted, and it says so in
   the findings.

7. **Put it on a stick.** The same analysis, run over a whole library and written where a
   player looks for it:

   ```sh
   mkdir -p audio
   cp "Ferry_Corsten,_Kosheen-Catch_(Extended_Mix).wav" audio/
   just usb flac
   ```

   Nix analyses each track in its own derivation, using the tool you just ran by hand, and
   builds a FAT32 image holding `Contents/138_03A_Ferry_Corsten,_Kosheen-Catch_(Extended_Mix).flac`,
   a rekordbox database and an Engine Library. Copy it over with `dd` and the deck reads the
   tempo, the key, the grid and the waveform without analysing anything itself.

You now know both loops: read the headline, read the findings, force the competing tempo and
compare grid fits; then let the pipeline do the same to everything and write the result where
a player reads it. The [how-to guides](#how-to-guides) cover the rest of the tasks,
[docs/02-troubleshooting-a-tempo.md](docs/02-troubleshooting-a-tempo.md) is the decision
procedure keyed on the diagnostic codes, and
[docs/04-device-export.md](docs/04-device-export.md) says exactly what lands on the stick.

---

## How-to guides

### Analyse one section instead of the whole track

An intro at half time, or a breakdown with no drums, drags the whole-track average with it.
Cut to the part you care about:

```sh
just analyze track.wav --start 90 --duration 60
```

Beat times in `report.json` are then relative to the excerpt, and `source.analysed_start_seconds`
records where it began.

### Find out which frequencies decided the tempo

The report has a tempo per onset band. A kick at 150 and hats at 100 is a shuffle or a
polyrhythm, and the broadband estimate lands between them:

```sh
jq '.bands[] | "\(.low_hz | round)-\(.high_hz | round) Hz: \(.bpm)"' analysis/*/report.json
```

Then narrow the onset detector to the band that agrees with your ears with `--onset-bands`,
and rerun.

### Force a metrical level

To hear the tool out on a tempo it rejected, restrict the search range rather than arguing
with the salience:

```sh
just analyze track.wav --min-bpm 155 --max-bpm 165
```

Compare `grid.matched_fraction` and `grid.pulse_ratio` against the original run. Higher on
both is the right level, whatever the salience curve ranked first.

### See what a tempo prior would do

Detectors that report a plausible answer to everything usually have a prior. This one has
none unless you ask:

```sh
just analyze track.wav --tempo-prior 128 --tempo-prior-width 0.4
```

The report then carries `candidates_without_prior`, and a `prior-changed-answer` finding
fires when the prior, rather than the track, picked the winner.

### Check whether a file came from a lossy source

`spectrum.high_cutoff_hz` is the highest frequency still within 40 dB of the peak. A hard
ceiling at 16 kHz or 19 kHz is a transcode, whatever the extension says:

```sh
jq '.spectrum' analysis/*/report.json
```

The spectrogram shows the same thing as a flat black band across the top.

### Trust a key estimate, or not

Read `key.tuning_cents` first. Beyond about 15 cents the track is not at A = 440 Hz, the
chroma mapping was shifted to compensate, and any tool that skipped that step read a
different set of notes. Then read `key.margin`. Under 0.05 the winner and the runner-up are
indistinguishable, and they are usually a key and its relative, which share every note and,
on the wheel, share a number: `8A` against `8B`.

Keys are reported in Camelot. `key.name` in the report and the second column of the key table
on the page carry the same key as notes, for when that is what you want.

Both profiles are available, and they disagree on tracks built from a repeating loop:

```sh
just analyze track.wav --key-profile krumhansl
```

### Name a library after what is in it

`rename` prints the name a file should carry: the tempo padded to three digits, the Camelot
key padded to two, then the name the file already had.

```sh
just rename "Ferry_Corsten,_Kosheen-Catch_(Extended_Mix).wav"
```

```
138_03A_Ferry_Corsten,_Kosheen-Catch_(Extended_Mix).wav
```

Padding is the point. A plain `ls` then sorts by tempo and, within a tempo, around the Camelot
wheel, which is the order you want when building a set. Nothing is renamed in place: pass
`--out` with `--mode symlink` or `--mode copy` to write a named collection somewhere else, and
run it again after a flag change to replace the prefix rather than stack a second one.

### Build a whole library from the zips you downloaded

```sh
mkdir -p archives                       # or symlink it at wherever the zips live
cp ~/Downloads/beatport_tracks_*.zip archives/
just library
```

That gives the named files without building an image, which is what you want when the
destination is a hard drive rather than a stick. Take all three formats or one:

```sh
devenv build outputs.library         # wav/ flac/ mp3/ side by side
devenv build outputs.library.flac    # one format, plus .analysis for the reports
```

Each track inside each archive becomes its own derivation, so Nix analyses as many at once as
it builds anything else, and one bad file fails one track rather than the batch. Loose files
work too: put them in `audio/` instead of zipping them.

### Build a USB stick for a CDJ or a Denon deck

```sh
just usb flac                   # or wav, or mp3, or `just usb` for all three
```

devenv prints the store path it built. The image is the `.img` file inside it, so copying to
a stick reads:

```sh
dd if=/nix/store/…-usb-flac.img/usb-flac.img of=/dev/diskN bs=4m   # check the disk twice
```

Building `outputs.usb` instead gives one directory of all three, named `wav.img`, `flac.img`
and `mp3.img`. Beside each single image is `device`, a link to the databases on their own,
which is what to read when a player refuses a stick.

The databases and the audio are separate derivations, so changing a database field does not
recopy several gigabytes of audio.

### Look at a device tree without building an image

```sh
just export /tmp/device --audio path/to/audio --reports path/to/reports
```

Writes `PIONEER/rekordbox/export.pdb` and `PIONEER/USBANLZ/…` and nothing else. Reports come
from `just analyze`; audio and reports are paired by file stem. Everything the databases
contain, and everything they do not, is in
[docs/04-device-export.md](docs/04-device-export.md).

### Add a dependency

```sh
just sync-cargo-nix     # regenerate the crate graph
just check              # includes the staleness check CI runs
```

The pre-commit hook regenerates `Cargo.nix` for you when a manifest changes. CI fails if the
committed graph and the manifests disagree.

---

## Reference

### Repository layout

| Path | What it is |
|---|---|
| `libraries/rust/audio/` | WAV decode, mono downmix, excerpting, and the synthesised signals the tests measure against. |
| `libraries/rust/spectral/` | The STFT, log-spaced frequency bands, chroma folding, tuning estimation. |
| `libraries/rust/tempo/` | Novelty curves, tempo salience, beat grid fitting, and the rules that name a doubtful answer. |
| `libraries/rust/key-detect/` | Chroma to key by profile correlation, in Camelot notation. |
| `libraries/rust/diagnostics/` | The finding type every stage reports its doubts in. |
| `libraries/rust/report/` | The JSON report, the SVG plots, the PNG spectrogram, the HTML page. |
| `tools/rust/music-analyze/` | The CLI: one analysis pass, one output directory. |
| `libraries/rust/collection/` | The device-neutral track model both exporters read, and the reader that builds it from a report. |
| `libraries/rust/waveform/` | The two waveforms a player draws, at the resolutions their formats read. |
| `libraries/rust/rekordbox/` | `export.pdb` and the `ANLZ` files, with the reference parser as the test oracle. |
| `libraries/rust/engine/` | The Engine Library database Denon players read, schema 2.21.2. |
| `nix/library.nix` | The archive pipeline: zips in, three named format directories out. |
| `nix/device.nix` | The device tree and the FAT32 image built from it. |
| `infrastructure/ci-cd/` | The CI workflows, as a cdkactions app. `.github/workflows/` is generated from it. |
| `docs/` | `01-signal-chain`, `02-troubleshooting-a-tempo`, `03-report-reference`. |
| `Cargo.nix` | The crate graph, generated by crate2nix. Never hand-edited. |
| `devenv.nix`, `treefmt.nix` | The dev shell, the Nix build, and the formatter set. |

Packages are grouped by structural role first and by language second, so a crate's home
follows from what it is rather than from what it is about. Reusable analysis stages go in
`libraries/rust/`, things you run go in `tools/rust/`. Both are globs in the workspace
manifest, so adding a crate does not mean editing `Cargo.toml`.

### Commands

| Command | Does |
|---|---|
| `just check` | The full PR gate: formatting, clippy, tests, crate-graph staleness. |
| `just analyze <file> [flags]` | Analyse a file and write a report directory. |
| `just rename <path>...` | Print, link or copy files named after what was measured in them. |
| `just library` | Build every archive in `archives/` into `wav/`, `flac/` and `mp3/`. |
| `just usb [format]` | Build FAT32 images, all three or one, databases and all. |
| `just export <dir>` | Write a device tree from a directory of audio and reports. |
| `just selftest [--bpm N]` | Measure a generated pulse train end to end. |
| `just test` / `lint` / `fmt` | `cargo test` / clippy with warnings fatal / `treefmt`. |
| `just sync-cargo-nix` | Regenerate `Cargo.nix` after a dependency change. |
| `just synth-workflows` | Regenerate `.github/workflows/` from `infrastructure/ci-cd`. |
| `just build-nix` | Build the CLI through Nix, against the committed crate graph. |

### Output files

A run writes one directory per track. `report.json` is the contract: every number in the
terminal summary, in the HTML page and in the plots comes from it.

| File | What it holds |
|---|---|
| `report.json` | Every measurement, every candidate, every finding. |
| `report.html` | The page: headline, findings, four tables, seven figures. |
| `spectrogram.png` + `.svg` | Time against log frequency. The SVG carries the axes. |
| `spectrum.svg` | Long-term average spectrum on a log frequency axis. |
| `novelty.svg` | The onset curve with the beat grid drawn over it. |
| `tempo-salience.svg` | Both estimators across the BPM range, candidates marked. |
| `tempo-over-time.svg` | One independent estimate per twenty-second window. |
| `chroma.svg`, `key-correlations.svg` | Pitch-class energy and the ranked keys. |

Every flag, every JSON field and every diagnostic code is in
[docs/03-report-reference.md](docs/03-report-reference.md); everything the device export
writes is in [docs/04-device-export.md](docs/04-device-export.md).

### Nix outputs

Both trees nest, so a build takes the lot or one format of it.

| Output | What it is |
|---|---|
| `outputs.music-analyze` | The CLI, built through the committed crate graph. |
| `outputs.library` | Every track named `<BPM>_<KEY>_<original name>`, the three formats side by side. |
| `outputs.library.flac` | One format. `.wav` and `.mp3` likewise; `.analysis` holds the reports. |
| `outputs.usb` | A FAT32 image per format, side by side as `wav.img`, `flac.img`, `mp3.img`. |
| `outputs.usb.flac` | One image, with `device` beside it linking the databases on their own. |

`library` is one derivation with several outputs; `usb` is three derivations behind one,
since each image is built from different audio.

### Input format

The analyser reads WAV: any sample rate, any channel count, 16, 24 or 32-bit integer or float.
Channels are averaged to mono and nothing is resampled. It refuses anything shorter than 30
seconds rather than reporting a tempo it measured once.

The pipeline takes WAV, FLAC and MP3 out of the archives and decodes the last two with `flac`
and `lame` before the analyser sees them, which keeps one decoder per format and each of them
the reference implementation for it.

---

## Explanation

### Everything hangs off the novelty curve

The pipeline is short on purpose. One pass of the short-time Fourier transform produces
energy in eight log-spaced bands per frame; the difference between consecutive frames, log
compressed and with a moving average subtracted, is the novelty curve; every tempo number in
the report is a statement about that one curve. So when a tempo is wrong there are only two
possibilities, and the novelty plot separates them. Either the pulse is not in the curve,
which is a problem with onset detection or with the track, or it is in the curve and the
estimator picked the wrong period.

Keeping the pipeline shallow costs accuracy that a deeper model would win back. It buys the
thing this tool exists for, which is that every intermediate value has a plot and a name.

### Two estimators, kept separate

Autocorrelation asks how self-similar the curve is at a lag of one beat. The Fourier
tempogram asks how much energy sits at a beat frequency. They fail differently. Autocorrelation
is confused by a swung or shuffled pulse, and the Fourier tempogram is confused by a tempo
that drifts. Neither is corrected against the other, and neither vote is averaged in. When
they disagree the report says so, with both numbers, because a disagreement names the thing
to look at next and an average hides it.

### The octave problem, and the one rule that resolves it

A harmonic sum scores half the true tempo almost as highly as the truth, since every one of
its comb teeth still lands on a beat. Subtracting the autocorrelation halfway between the
teeth argues against that, and it is the standard fix, but it argues just as hard against the
true tempo of anything with offbeat movement. On a 160 BPM hardstyle track whose reverse bass
sits on every offbeat, that penalty at full weight reports 106.64 BPM, which is not even an
octave relative of the truth. So the penalty is off by default and `--comb-penalty` exposes
it.

What resolves the octave instead is a stated rule rather than a hidden one. The reported
tempo is doubled until it clears 90 BPM, provided the doubled candidate still scores at least
half of the original. Nothing in club music is counted below that; a track that measures at 70
is being counted in half bars. Across the seventeen-track working set this doubling fires four
times, and every time it does the report carries a `metrical-floor-applied` finding with both
saliences, so the decision is visible and `--metrical-floor 0` undoes it.

### No tempo prior unless you ask

Most detectors weight candidates towards 120 BPM. It improves the average case and it is
exactly how a 174 BPM drum and bass track gets reported as 87. Here the prior is off, and
switching it on with `--tempo-prior` makes the report carry the ranking it would have produced
without one. When the prior rather than the track picked the winner, that is a finding.

### The findings are not warnings about the tool

A finding is a measured disagreement, not a confidence score. `grid-misfit` means fewer than
half the beats landed near an onset, which is a statement about whether a constant tempo
describes the track at all. `flat-chroma` means pitch-class energy is nearly uniform, so the
key ranking below it is arbitrary however good its correlations look. Six of the seventeen
tracks in the working set raise nothing. That is the point of not raising something on every
one.

### Writing a player's database

A CDJ does not read a folder of files. It reads `export.pdb`, a DeviceSQL database of pages
and row groups, and two analysis files per track holding the beat grid, the cues and the
waveforms. No open tool writes them. rekordcrate, which is the reference implementation of
the format analysis, parses a database and can modify one, but keeps every row field private,
so it can check this output and cannot produce it.

So the writer here is ours, and rekordcrate is the oracle: the tests write a database and read
it back through its own parser, across page boundaries, through the UTF-16 string encoding,
and into the playlist entries. That is the strongest check available without hardware. It says
the file is well formed and the values survive; it does not say a player accepts it, and no
CDJ has read one of these sticks yet. The fields whose purpose nobody has established carry the
constants that appear in real exports, and they are named as unknown where they are written.

Denon's side is easier and harder. Easier because the database is SQLite and its schema is
recorded in libdjinterop, so the file is checked by its own constraints and triggers as it is
written. Harder because there is no parser to disagree with: the analysis lives in five
compressed binary columns, and a mistake inside one of them is a blob that decompresses to the
wrong numbers rather than a database that fails to open. The tests decode each blob back and
check the framing, the byte order and the values.

### One derivation per track

The archive pipeline could be a shell script over a directory. It is a derivation per track
instead, for three reasons that a script gives up. Nix schedules the analyses the way it
schedules any other build, so the parallelism is whatever the machine is already set to. A
track that fails to decode fails alone and names itself. And a rebuild after a flag change
reanalyses nothing that did not change, which matters at a second per track across a library
of hundreds.

The cost is import from derivation. Nothing in Nix can list the contents of a zip without
unpacking one, so the archive is imported into the store and a manifest is built and read
during evaluation. On a gigabyte of downloads the first run says nothing for as long as that
copy takes.

### Keys are Camelot

`8A` says two things a DJ acts on: which key, and which keys mix with it. `A minor` says one
of them and leaves the other as a piece of theory to do in your head at the wrong moment. So
the wheel is what the terminal prints, what the file names carry, what the rekordbox database
stores for a player to display and sort by, and what Engine gets as its key number.

The musical name is still measured and still in the report, as `key.name` and as a column on
the page. Nothing is lost; the reading order is just the one that matches how the answer gets
used.

### Why Rust, and why the graph is committed

The hot loops are a transform over tens of millions of samples and an autocorrelation over
tens of thousands of frames. A seven-minute track analyses in about a second, which is what
makes a flag sweep across a library a thing you actually do rather than a thing you plan.

`Cargo.nix` is the crate graph, resolved ahead of time by crate2nix and committed. Nix reads
it and builds every dependency with the compiler pinned in `rust-toolchain.toml`, so nothing
resolves or fetches while the build is being evaluated. The cost is that the file goes stale
silently, which is why a pre-commit hook regenerates it whenever a manifest changes and CI
fails when the two disagree.
