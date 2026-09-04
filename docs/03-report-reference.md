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
| `--metrical-floor <N>` | `90` | Slowest level the answer may be reported at. `0` disables the doubling. |
| `--metrical-floor-ratio <0..1>` | `0.5` | How strong the doubled candidate must be, relative to the original, for the doubling to happen. |
| `--integer-snap <N>` | `0.25` | Largest gap, in BPM, the answer may be moved by to land on a whole number. `0.5` rounds every measurement, `0` reports the measurement. A measurement further from an integer than this keeps its decimals and raises `non-integer-tempo`. |
| `--tempo-prior <N>` | off | Centre of a log-normal prior over tempo, in BPM. |
| `--tempo-prior-width <N>` | `0.7` | Width of that prior, in octaves. |

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
| `source.analysed_start_seconds`, `source.analysed_seconds` | The excerpt actually analysed. Every time in the report is relative to its start. |
| `settings.window_size`, `settings.hop` | Transform geometry, in samples. |
| `settings.frame_rate` | Frames per second, so `hop` samples expressed as a rate. |
| `settings.onset_bands` | Band count the flux was computed over. |
| `settings.flux_compression`, `settings.local_mean_seconds` | The two onset knobs. |

### tempo

| Field | Meaning |
|---|---|
| `tempo.bpm` | The reported tempo, and the one the grid, the file name and both device databases take. A whole number unless `--integer-snap 0` was given or the measurement was too far from one to snap. |
| `tempo.bpm_measured` | The tempo as measured, before the snap. Differs from `tempo.bpm` by at most `--integer-snap`. This is what `selftest` checks and what to read when a grid drifts. |
| `tempo.settings.integer_snap_bpm` | The snap tolerance the run used. |
| `tempo.salience` | Comb salience at the reported tempo. Every ratio in the findings is measured against this. |
| `tempo.octave_shift` | `null`, or the doubling the floor applied, with `from_bpm`, `to_bpm` and both saliences. |
| `tempo.candidates[]` | Up to five peaks, ranked. Each has `bpm`, `salience`, `weighted_salience` (after the prior), `autocorrelation` (the plain correlation at that lag, with no comb) and `fourier_salience`. |
| `tempo.candidates_without_prior` | The same ranking with the prior off. Present only when a prior was set. |
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
| `grid-misfit` | warning | Fewer than half the beats found an onset within tolerance. |
| `unstable-tempo` | warning | Window estimates spread over more than 2 BPM. |
| `prior-changed-answer` | warning | With the prior off, a different tempo wins by more than 1 BPM. |
| `range-edge` | warning | The answer sits within 2 BPM of `--min-bpm` or `--max-bpm`. |
| `level-fits-better` | warning | A two-thirds or three-halves level matched more beats than the answer at more than 1.05 times its pulse ratio. Halves and doubles are excluded, because a half grid reads higher on both measures whatever the truth is. |
| `non-integer-tempo` | warning | The measurement sits further from a whole number than `--integer-snap` allows, so it was reported as measured. Produced music is written on integers, so this is a played or ripped source, or a grid that drifts. |
| `metrical-floor-applied` | info | The floor doubled the answer. Carries both saliences. |
| `octave-split-windows` | info | More than 15% of windows chose half or double the reported tempo. |
| `flat-bar-phase` | info | Bar contrast is under 1.1, so beat one is a guess. |

### Key

| Code | Severity | Fires when |
|---|---|---|
| `flat-chroma` | warning | Pitch-class energy is nearly uniform, so the ranking is arbitrary. |
| `key-tie` | warning | The margin to the runner-up is under 5%. |
| `relative-key-tie` | warning | The same, and the runner-up is the relative major or minor. |
| `tuning-offset` | info | The track sits more than 15 cents from A = 440 Hz. |
