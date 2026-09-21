# 03. Report reference

Every command-line flag, every field of `report.json`, and every diagnostic code. Read from
the code, not from memory; a name that appears here exists.

## Commands

```
dubplate analyze  <FILE> [OPTIONS]
dubplate rename   <PATH>... [OPTIONS]
dubplate selftest [OPTIONS]
```

`analyze` reads a WAV file and writes a report directory. `rename` names files after what it
measured in them. `selftest` generates a pulse train at a known tempo, runs the same pipeline
over it, prints the error, and exits non-zero when the error passes 0.5%.

### Input and output

| Flag | Default | Effect |
|---|---|---|
| `<FILE>` | required | WAV file. 16, 24 or 32-bit integer or float, any rate, any channel count. |
| `-o`, `--out <DIR>` | `analysis/<file stem>` | Where the report directory is written. |
| `--start <S>` | `0` | Start of the analysed excerpt, in seconds. |
| `--duration <S>` | to the end | Length of the analysed excerpt, in seconds. |
| `--plot-start <S>` | a quarter of the way in | Start of the window the novelty figure covers. |
| `--plot-window <S>` | `12` | Length of that window. |
| `--no-figures` | off | Write `report.json` only. |
| `--no-trim-lead-in` | off | Analyse from sample zero even when the file opens with silence. Pass it when the lead-in is the thing being looked at. |
| `--print-json` | off | Also print the report to stdout. |

Runs shorter than 30 seconds of audio are refused rather than answered.

### Transform and onsets

| Flag | Default | Effect |
|---|---|---|
| `--window <N>` | `2048` | STFT window in samples. Larger resolves frequency, smaller resolves time. |
| `--hop <N>` | `512` | STFT hop in samples. Sets the frame rate of every novelty curve. |
| `--onset-bands <N>` | `8` | Log-spaced bands between 30 Hz and 16 kHz. |
| `--compression <G>` | `1000` | Gamma of the `ln(1 + G * energy)` compression before the flux. |
| `--local-mean <S>` | `0.5` | Width of the moving average subtracted from the flux. |

### Tempo

| Flag | Default | Effect |
|---|---|---|
| `--min-bpm <N>` | `60` | Slowest tempo searched. |
| `--max-bpm <N>` | `220` | Fastest tempo searched. |
| `--bpm-resolution <N>` | `0.1` | Grid spacing. The answer is refined between grid points, so this sets search cost rather than precision. |
| `--pulses <N>` | `4` | Comb teeth, weighted 1/k. |
| `--comb-penalty <0..1>` | `0` | Weight of the penalty subtracted halfway between teeth. |
| `--metrical-floor <N>` | `80` | Slowest level the answer may be reported at. `0` disables the doubling. |
| `--metrical-floor-ratio <0..1>` | `0.5` | How strong the doubled candidate must be, relative to the original, for the doubling to happen. |
| `--integer-snap <N>` | `0.25` | Gap, in BPM, the rounding to a whole number may close without comment. Every measurement is rounded; a gap wider than this raises `non-integer-tempo`. `0` reports the measurement instead, grid included. |

### Sections and cues

| Flag | Default | Effect |
|---|---|---|
| `--memory-offset-bars <N>` | `16` | Bars between a memory cue and the hot cue it runs into. |
| `--loop-bars <N>` | `4` | Length of the loop pads B and H mark out. |
| `--drop-after-fraction <0..1>` | `0.2` | Share of the track before which a drop is a taste of the hook rather than the drop. |

### Key

| Flag | Default | Effect |
|---|---|---|
| `--key-profile <NAME>` | `temperley` | `temperley` or `krumhansl`. |
| `--tuning-cents <N>` | measured | Override the measured offset from A = 440 Hz. |

### rename

| Flag | Default | Effect |
|---|---|---|
| `<PATH>...` | required | WAV files, or directories of them. Directories are read one level deep, never recursively. |
| `-o`, `--out <DIR>` | none | Where links or copies are written. Required unless the mode is `print`. |
| `--mode <MODE>` | `print` | `print` names the files and changes nothing, `symlink` writes links to the originals, `copy` writes copies. |
| `--reports <DIR>` | none | Also write `report.json` per track under this directory. |

Every `analyze` flag from the transform, onset and tempo tables applies here too. Names go to
stdout, one per line; findings go to stderr, prefixed with the name they belong to.

The name is `<BPM>_<KEY>_<rest>`, where the tempo is rounded and padded to three digits and the
Camelot number is padded to two, as in `126_05A_Artist-Title (Extended Mix).wav`. Padding makes
a text sort a tempo sort. A `NNN_NNX_` prefix this tool wrote earlier is replaced rather than
stacked, so renaming after a flag change is idempotent.

### selftest

| Flag | Default | Effect |
|---|---|---|
| `--bpm <N>` | `174` | Tempo of the generated pulse train. |
| `--seconds <S>` | `60` | Length of the generated signal. |

Every `analyze` flag from the transform, onset and tempo tables applies to `selftest` too.

## Output files

| File | Contents |
|---|---|
| `report.json` | Everything below. The source of every other file. |
| `report.html` | Headline, findings, five tables, seven figures. No script, no external asset. |
| `spectrogram.png` | 1600 columns at most, 440 log-spaced rows from 20 Hz, 80 dB of range. |
| `spectrogram.svg` | Axes around the PNG, which it references by name. |
| `spectrum.svg` | Long-term average spectrum, log frequency axis. |
| `novelty.svg` | Onset curve over the plot window, beat grid drawn over it. |
| `tempo-salience.svg` | Both estimators across the BPM range, each scaled to its own peak. |
| `tempo-over-time.svg` | One estimate per twenty-second window. |
| `chroma.svg` | Pitch-class energy, tonic highlighted. |
| `key-correlations.svg` | The eight best-scoring keys, in Camelot notation. |

## report.json

### source and settings

| Field | Meaning |
|---|---|
| `tool`, `version` | Name and crate version that produced the file. |
| `source.path` | Path as given on the command line. |
| `source.sample_rate`, `source.channels` | As found in the file. Channels are averaged to mono before analysis. |
| `source.duration_seconds` | Length of the whole file. |
| `source.analysed_start_seconds`, `source.analysed_seconds` | The excerpt actually analysed, `--start` and the lead-in trim together. Every time in the report is relative to its start. |
| `source.lead_in_seconds` | Near-silence measured at the head of the file: the run of 10 ms windows whose peak stays more than 60 dB below the track's own peak. Reported whether or not it was cut, and 0 when `--no-trim-lead-in` was given. |
| `source.trim_to_first_beat_seconds` | Seconds to cut from the head of the source so beat one lands on sample zero: the lead-in plus `tempo.grid.offset_seconds`. `export --trim` cuts here and shifts every time it writes by the same number. |
| `settings.window_size`, `settings.hop` | Transform geometry, in samples. |
| `settings.frame_rate` | Frames per second, so `hop` samples expressed as a rate. |
| `settings.onset_bands` | Band count the flux was computed over. |
| `settings.flux_compression`, `settings.local_mean_seconds` | The two onset knobs. |

### tempo

| Field | Meaning |
|---|---|
| `tempo.bpm` | The reported tempo, and the one the grid, the file name and both device databases take. A whole number unless `--integer-snap 0` was given. |
| `tempo.bpm_measured` | The tempo as measured, before the rounding. Differs from `tempo.bpm` by at most half a BPM, and by more than `--integer-snap` only where `non-integer-tempo` fires. This is what `selftest` checks and what to read when a grid drifts. |
| `tempo.settings.integer_snap_bpm` | The tolerance the run used. |
| `tempo.salience` | Comb salience at the reported tempo. Every ratio in the findings is measured against this. |
| `tempo.octave_shift` | `null`, or the doubling the floor applied, with `from_bpm`, `to_bpm` and both saliences. |
| `tempo.candidates[]` | Up to five peaks, ranked. Each has `bpm`, `salience`, `weighted_salience` (after the tempo weighting), `autocorrelation` (the plain correlation at that lag, with no comb) and `fourier_salience`. |
| `tempo.candidates_unweighted` | The same ranking with the tempo weighting off. Present only when the energy band named a centre. |
| `tempo.octave_relatives[]` | One row per metrical level: `half`, `two-thirds`, `candidate`, `three-halves` and `double` of the reported tempo. Each carries `salience` and `fourier_salience` at that tempo, plus `matched_fraction` and `pulse_ratio` from a grid fitted there, which is what a rerun forcing that tempo would measure. The `candidate` row is the run's own grid. A half reads high on `pulse_ratio` whatever the truth is, since every one of its beats is a beat of the real grid and half as many have to find an onset. |
| `tempo.grid.offset_seconds` | Where the first beat sits. |
| `tempo.grid.beats_seconds[]` | Every beat time. |
| `tempo.grid.pulse_ratio` | Mean novelty on the grid over the track mean. 1.0 is no better than an arbitrary grid; a clean four-to-the-floor track lands between 2 and 4. |
| `tempo.grid.matched_fraction` | Fraction of beats with a novelty peak within `tolerance_ms`. |
| `tempo.grid.mean_absolute_error_ms` | Mean distance to the nearest peak, over matched beats. `null` when nothing matched. |
| `tempo.grid.tolerance_ms` | 50 ms, the usual bound on where a listener stops noticing. |
| `tempo.bar.phase` | Which beat of the bar carries the low-end weight, counted from 0. |
| `tempo.bar.contrast` | That beat's low-band novelty over the mean of the others. Near 1.0 the bar line is a guess. |
| `tempo.over_time[]` | `start_seconds`, `bpm` and `salience` per twenty-second window, hopped every ten. |
| `tempo.stability.median_bpm` | Median of those windows. |
| `tempo.stability.interquartile_range_bpm` | Their spread. Under 1 BPM is a steady track. |
| `tempo.stability.agreeing_fraction` | Fraction within 1% of the reported tempo. |
| `tempo.stability.octave_split_fraction` | Fraction that chose half or double it. |
| `tempo.diagnostics[]` | Findings from this stage. |

### key

| Field | Meaning |
|---|---|
| `key.profile` | `temperley` or `krumhansl`. |
| `key.chroma[12]` | Pitch-class energy, C first, summing to 1. |
| `key.tuning_cents` | Measured offset from A = 440 Hz, which the chroma mapping was shifted by. |
| `key.key.tonic`, `key.key.mode` | 0 = C through 11 = B, and `major` or `minor`. |
| `key.name`, `key.camelot` | The same key as text and on the Camelot wheel. |
| `key.margin` | Gap to the runner-up as a fraction of the winner's correlation. Under 0.05 the two are indistinguishable. |
| `key.ranked[]` | All 24 keys with their Pearson correlations, best first. |
| `key.diagnostics[]` | Findings from this stage. |

### waveforms

Two pictures of the track, at the resolutions the device formats read them at.
The export step reads these rather than decoding the audio a second time.

| Field | Meaning |
|---|---|
| `waveforms.preview` | 1200 columns spanning the whole track. |
| `waveforms.detail` | 150 columns per second. |
| `.columns_per_second` | What it says. The detail waveform is exactly 150; the preview follows from the length of the track. |
| `.columns` | Base64 of two bytes per column: the first byte carries the height in its low five bits and the low band in its top three, the second the mid band in its low three bits and the high band in the three above. Every level is 0 to 7, scaled so the loudest band of the column reads 7; the height is 0 to 31. |

Colour is a balance rather than a level, which is why the bands are scaled
per column: the level is the height. Bands are split at 200 Hz and 2 kHz.

### structure

Where the track changes, what each stretch is, and the cues that follow. Measured over the
beat grid from band energies on a tenth-of-a-second grid, so it is empty when no grid was
found. Nothing here changes the tempo, the grid or the key.

| Field | Meaning |
|---|---|
| `structure.sections[]` | One entry per stretch, in order. |
| `.label` | `intro`, `build`, `drop`, `breakdown`, `outro` or `steady`. The first stretch is always `intro` and the last always `outro`, by position rather than by sound. |
| `.confidence` | 0 to 1: how far the measurements sat from the threshold that would have named the stretch something else. Under 0.35 raises `weak-section-label`. |
| `.start_seconds`, `.end_seconds`, `.start_bar`, `.bars` | Where the stretch runs, in both units. |
| `.low_band_db` | Mean energy in the bottom two onset bands, which reach to about 130 Hz and in a club production are the kick. Read against the loudest section's, never against full scale. |
| `.broadband_db` | Mean energy across every band. |
| `.rise_db` | Broadband energy of the last quarter of the stretch minus the first quarter. 4 dB or more with the kick thinned is what makes a `build`. |
| `structure.boundaries[]` | Every peak the novelty curve offered, with `bar`, `time_seconds` and `strength` in standard deviations above the curve's mean. A section list with no curve behind it cannot be argued with. |
| `structure.cues[]` | The cues placed, hot and memory. |
| `.kind` | `hot` or `memory`. |
| `.number` | 1 to 8. Hot cue 1 is pad A and the pad is fixed, so a track with no build has no cue 3. Memory cues are numbered in the order they were placed. |
| `.name`, `.colour` | The role and the colour rekordbox gives that pad. The colour is not written to the device: a cue colour lives in the `PCO2` section this tool does not yet produce. |
| `.time_seconds`, `.bar` | Where the cue sits. Always a bar line. |
| `.from_section` | Index into `sections[]`, or null for the first-beat and loop pads, which are placed by position rather than by a section. |
| `structure.missing_pads[]` | Pad letters no section could fill. Left empty rather than filled with the nearest thing. |
| `structure.settings` | `phrase_bars` (4, what a boundary snaps to), `minimum_section_bars` (8), and the three flags: `memory_offset_bars`, `loop_bars`, `drop_after_fraction`. |

The pads, in order:

| Pad | Role | Placed at |
|---|---|---|
| A | First beat | Bar 0. |
| B | Loop in | First `drop` or `steady` stretch, else bar 0. |
| C | Buildup | Last `build` before the drop. |
| D | Drop | First `drop` starting past `drop_after_fraction` of the track; if every drop is earlier, the first one, and `drop-rule-relaxed` fires. |
| E | Breakdown | First `breakdown`. |
| F | Special | Loudest stretch no other pad marks. |
| G | Outro | Last stretch. |
| H | Loop out | Pad B plus `loop_bars`. |

Memory cues repeat those roles. A, B and H share their hot cue's bar; the rest sit
`memory_offset_bars` earlier, which is where a mix starts rather than where the section does.
A run-up that falls off the front of the track lands on bar 0, and a memory cue that would
duplicate one already there is dropped rather than repeated.

### bands and spectrum

| Field | Meaning |
|---|---|
| `bands[]` | One entry per onset band: `low_hz`, `high_hz`, and the `bpm` and `salience` that band alone reports. |
| `spectrum.peak_hz` | Loudest frequency in the long-term average, above 20 Hz. |
| `spectrum.centroid_hz` | Energy-weighted mean frequency. |
| `spectrum.rolloff_95_hz` | Frequency below which 95% of the energy lies. |
| `spectrum.high_cutoff_hz` | Highest frequency within 40 dB of the peak. A hard ceiling here is a lossy source. |
| `spectrum.low_share`, `mid_share`, `high_share` | Energy below 200 Hz, from 200 Hz to 4 kHz, and above. |

## Diagnostic codes

Codes are stable once published; the wording lives in `message`. `severity` is `warning` when
the answer is unsafe to use without reading the evidence, and `info` when it is worth knowing
and changes nothing.

### Tempo

| Code | Severity | Fires when |
|---|---|---|
| `no-candidate` | warning | The salience curve has no peak in the searched range. |
| `octave-ambiguity` | warning | An octave relative scores 85% or more of the reported tempo. Relatives below the metrical floor are excluded, since the floor already ruled them out. |
| `close-runner-up` | warning | The second candidate scores 85% or more of the first and is not an octave relative of the answer. |
| `estimators-disagree` | warning | The Fourier tempogram peaks more than 1% away from the autocorrelation answer. |
| `weak-pulse` | warning | `grid.pulse_ratio` is under 1.5. |
| `energy-band-applied` | info | The energy band chose the tempo weighting. Carries the score, the band and the centre. |
| `loose-grid` | warning | Between half and 80% of beats found an onset within the tolerance: the answer stands but the grid fits loosely. |
| `grid-misfit` | warning | Fewer than half the beats found an onset within tolerance. |
| `unstable-tempo` | warning | Window estimates spread over more than 2 BPM. |
| `range-edge` | warning | The answer sits within 2 BPM of `--min-bpm` or `--max-bpm`. |
| `level-fits-better` | warning | A two-thirds or three-halves level matched more beats than the answer at more than 1.05 times its pulse ratio. Halves and doubles are excluded, because a half grid reads higher on both measures whatever the truth is. |
| `non-integer-tempo` | warning | The measurement sits further from the whole number it was rounded onto than `--integer-snap` accounts for, so the grid under the answer drifts. Produced music is written on integers, so this is a played or ripped source, or a grid that drifts. |
| `weighting-changed-answer` | warning | The band's tempo weighting, not the track, picked the level. Names `bpm_unweighted`. |
| `metrical-floor-applied` | info | The floor doubled the answer. Carries both saliences. |
| `octave-split-windows` | info | More than 15% of windows chose half or double the reported tempo. |
| `flat-bar-phase` | info | Bar contrast is under 1.1, so beat one is a guess. |

### Structure

| Code | Severity | Fires when |
|---|---|---|
| `track-too-short-to-segment` | warning | The grid is under 24 bars. No sections are reported and only the first beat is cued. |
| `no-section-boundaries` | warning | No bar changed enough from its neighbours to be a boundary, so the whole track is one section. |
| `weak-section-label` | warning | A section's `confidence` is under 0.35: a different label was nearly as good. Names the sections and their times. |
| `no-drop-found` | info | No stretch carried both a full kick and the loudest the track gets, so pad D is empty. |
| `drop-rule-relaxed` | warning | Every drop started inside `drop_after_fraction` of the track, so pad D took the earliest one rather than staying empty. |
| `cue-pads-empty` | info | Names the pads left empty. |

### Key

| Code | Severity | Fires when |
|---|---|---|
| `flat-chroma` | warning | Pitch-class energy is nearly uniform, so the ranking is arbitrary. |
| `key-tie` | warning | The margin to the runner-up is under 5%. |
| `relative-key-tie` | warning | The same, and the runner-up is the relative major or minor. |
| `tuning-offset` | info | The track sits more than 15 cents from A = 440 Hz. |
| `lead-in-trimmed` | info | The file opened with near-silence, which was skipped before the grid was laid. Reported under tempo, because the grid is what it moves. |
