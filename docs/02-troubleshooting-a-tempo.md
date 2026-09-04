# 02. Troubleshooting a tempo

A tempo came out wrong, or you do not trust it. This is the order to check things in. Each
step ends in a decision, and most tracks are settled by step 3.

You need `report.json` and `report.html` from the run in question. If you passed
`--no-figures`, rerun without it.

## 1. Is the tempo wrong, or is the metrical level wrong?

Divide and multiply the reported tempo by 2, by 3/2 and by 2/3. If one of those is the answer
you expected, this is a metrical-level question and steps 2 and 3 apply. If none of them is,
skip to step 4.

The `octave_relatives` table in the report scores exactly those tempi:

```sh
jq '.tempo.octave_relatives' analysis/*/report.json
```

## 2. Did the metrical floor move the answer?

Look for `metrical-floor-applied` in the findings. It reports both saliences:

```
the strongest candidate was 63.02 BPM (salience 0.887); the answer was doubled to
126.02 BPM (salience 0.854) to clear the metrical floor
```

The rule is a convention about how music is counted, not a measurement. Undo it with
`--metrical-floor 0` and the salience curve's own answer comes back. If you work on music
counted below 90 BPM, lower the floor rather than turning it off, or every subharmonic
becomes a candidate answer again.

## 3. Force the competing tempo and compare grid fits

This is the step that settles octaves. Salience ranks candidates within one run and cannot be
compared across runs. Grid fit can.

```sh
just analyze track.wav --min-bpm 80 --max-bpm 88     # a competing level inside a range
just analyze track.wav --metrical-floor 0            # the level the floor rejected
```

Compare against the original run:

| Field | Meaning | Better is |
|---|---|---|
| `tempo.grid.pulse_ratio` | novelty on the grid over the track mean | higher |
| `tempo.grid.matched_fraction` | beats with an onset within 50 ms | higher |
| `tempo.stability.agreeing_fraction` | windows within 1% of the answer | higher |

The pulse ratio separates a two-thirds or three-halves relative cleanly. On a 126 BPM track,
forcing 84 gives 83% of beats matched at a ratio of 5.72, against 92% at 8.27 for the answer:
two beats in three of an 84 BPM grid land on a real beat and the third lands on nothing.

It does not separate a half or a double at all, and it is worth knowing which way it fails.
Forcing 63 BPM on that same track matches the same 92% of beats at a ratio of 8.81, higher
than the answer's, because every beat of a 63 BPM grid is a beat of the 126 one and half as
many beats are being asked to find an onset. Reading the ratio alone would pick the wrong
level.

Two other numbers decide a half. The window trace collapses: at 126 BPM the twenty-second
estimates spread over 0.01 BPM with 88% agreeing, at 63 they spread over 63 BPM with 74%
agreeing, which is one track being measured as two different things depending on the window.
And `tempo.candidates[].fourier_salience` gives 126 BPM 1.000 against 0.178 for 63, because it
measures energy at a beat frequency and the track carries none at 63.

A split verdict, where one level wins the ratio and another wins the agreement, usually means
the track has two sections at different feels. Step 6 covers that.

Then look at `novelty.svg` for both runs. Grid lines standing on the peaks are a fit. Lines
that start on the peaks and drift off them across the window are a tempo that is close and
wrong, usually by less than a BPM.

## 4. Is the pulse in the novelty curve at all?

If no metrical level fits, the question moves upstream. Read `novelty.svg`. You are looking
for regular peaks, not for a shape.

`weak-pulse` in the findings means the grid sits on novelty less than 1.5 times the track
mean. `grid-misfit` means fewer than half the beats found an onset. Either way the onset
detector, not the tempo estimator, is what to change:

```sh
# Shorter window, finer time resolution: for tracks whose onsets are soft
just analyze track.wav --window 1024 --hop 256

# More compression: for a track whose quiet sections carry the pulse
just analyze track.wav --compression 5000

# A wider moving average: for slow tempi where 0.5 s eats the beat itself
just analyze track.wav --local-mean 1.5
```

The moving average is the one that bites most often. It must stay comfortably wider than a
beat period, so at 60 BPM a 0.5-second window is attenuating exactly what is being measured.

## 5. Which frequencies disagree?

The report has one tempo per onset band:

```sh
jq -r '.bands[] | "\(.low_hz|round)-\(.high_hz|round) Hz  \(.bpm)  \(.salience)"' analysis/*/report.json
```

A kick band at 150 and a hat band at 100 is a shuffle or a polyrhythm, and the broadband
answer will sit somewhere between them. Narrow the detector to the band that matches what you
hear with `--onset-bands`, or accept that the track genuinely has two pulses and say which one
you mean.

## 6. Does the tempo hold across the track?

`tempo-over-time.svg` is one independent estimate per twenty-second window. `unstable-tempo`
fires when those estimates spread over more than 2 BPM.

A line that steps between two values an octave apart is a whole-track average hiding two
feels: a half-time intro, or a breakdown counted differently. Analyse the sections separately:

```sh
just analyze track.wav --start 0   --duration 60
just analyze track.wav --start 120 --duration 60
```

A line that drifts smoothly is a real tempo change, and no single number describes the track.
A line that is flat except for a few outliers is fine; those windows are breakdowns with no
drums.

## 7. When the two estimators disagree

`estimators-disagree` means the autocorrelation comb and the Fourier tempogram peaked more
than 1% apart. They read the same curve, so one of them is being misled by the shape of the
pulse rather than by its period.

Read `tempo-salience.svg`. Autocorrelation is the one confused by a swung or shuffled pulse,
where the second eighth of each beat arrives late. The Fourier tempogram is the one confused
by drift, since it measures energy at a frequency and a drifting tempo has none. Whichever
explanation matches what you hear tells you which line to believe.

## 8. Is the answer a whole number, and should it be?

The headline is a whole number and `bpm_measured` is what was measured. When the two differ,
the headline says by how much:

```
126 BPM   5A   5:55 analysed
  measured  126.02 BPM, snapped -0.02 BPM to a whole number
```

A snap of a few hundredths is this tool's own error being closed, and there is nothing to
check. A snap near the tolerance, above roughly 0.15 BPM, is worth a look at
`tempo-over-time.svg`: a track whose windows sit either side of an integer is being measured
well, and one whose windows drift across the track was rounded to a number it only passes
through.

`non-integer-tempo` is the opposite case. The measurement was too far from any integer to
snap, so it stands as the answer:

This is `just selftest --bpm 127.5`, a pulse train generated exactly half way between two
integers, which is the one place the finding can be produced on demand:

```
generated 127.50 BPM, measured 127.50 BPM, error 0.002 BPM (0.00%)
  non-integer-tempo 127.50 BPM sits 0.50 BPM from the nearest whole number, further than the
  0.25 BPM the snap allows, so the measurement is the answer
```

Produced music is written on integers, so that means one of three things. The track was
played rather than rendered, and a DJ set or a live recording has no single tempo to find.
The file is a rip whose speed is off, which `--integer-snap 0.5` will round for you if you
want the nominal tempo. Or the grid genuinely drifts, which step 6 is how you tell.

To see the measurement everywhere instead of the rounded answer, turn the snap off:

```sh
just analyze track.wav --integer-snap 0
```

## 9. When nothing above resolves it

Cut to twenty seconds you are certain about and run that alone:

```sh
just analyze track.wav --start 90 --duration 30 --plot-window 8
```

Then count beats by hand against `novelty.svg`. Eight seconds at 140 BPM is about 19 beats,
which is countable from the plot. If the tool agrees with your count on that window and
disagrees over the track, the track is the problem, not the estimator.

## Reading the findings

Every code, and what it means, is in [03-report-reference.md](03-report-reference.md#diagnostic-codes).
Two habits are worth keeping. A finding is a measured disagreement rather than a confidence
score, so `octave-ambiguity` on a track you know is fine means the ambiguity is real and the
tool resolved it correctly. And an empty findings list is not proof of a right answer; it means
nothing the tool measures disagreed with anything else it measures.
