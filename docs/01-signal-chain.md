# 01. The signal chain

Why the analysis is built the way it is, and what each stage can get wrong. If you want to
fix a wrong answer rather than understand one, go to
[02-troubleshooting-a-tempo.md](02-troubleshooting-a-tempo.md).

## The shape of it

```
WAV ──► mono f32 ──► STFT ──┬─► log bands ──► spectral flux ──► novelty ──┬─► tempo salience ──► candidates ──► beat grid
                            │                                             └─► per-band tempo
                            ├─► chroma ──► key profiles ──► ranked keys
                            └─► spectrogram, average spectrum
```

One pass of the transform feeds every consumer. A ten-minute track at a 512-sample hop is
about 52 000 frames of 1025 bins, and holding that matrix costs 200 MB that no stage needs
whole, so `Stft::for_each_frame` hands each magnitude spectrum to a callback and keeps
nothing.

The tuning offset is the one measurement that needs its own pass. The chroma mapping is built
around it and cannot be corrected after the fold, so a first strided pass transforms about
2000 frames and estimates the offset before the real pass starts.

## Decoding

Channels are averaged to mono and nothing is resampled. A tempo in beats per minute does not
depend on the sample rate, frequencies are reported in hertz, and a resampler would add a
filter whose ringing shows up in the spectrogram as an artefact of this tool rather than of
the track.

The container is walked chunk by chunk. The tracks this was built against carry a `LIST`/`INFO`
block of artist metadata between `fmt ` and `data`, and a reader that skips a fixed 44-byte
header lands in the middle of it and reads the artist name as audio.

## The transform

Window 2048 samples, hop 512, periodic Hann. At 44.1 kHz that is 21.5 Hz per bin and 86.13
frames per second.

The window size is the one real tension in the tool. Onsets want a short window, because a
long one smears a transient across several frames and blunts the flux. Key wants a long one,
because 21.5 Hz per bin cannot separate two semitones below about 300 Hz. 2048 is a
compromise that suits neither perfectly, and both `--window` and `--hop` are exposed for
when one question matters more than the other.

The window is periodic rather than symmetric. Consecutive frames overlap, and the symmetric
variant leaves a ripple at the hop rate, which lands in the novelty curve as a tone at exactly
the frame rate.

## Onsets

Eight log-spaced bands from 30 Hz to 16 kHz. Log spacing because a linear axis spends nine
tenths of its bands above 2 kHz, where nothing about tempo happens. Each band takes the mean
magnitude of its bins, not the sum, since an upper band covers a hundred times as many bins
as a lower one and summing would rank bands by their width.

Then `ln(1 + 1000 * energy)` per band, and the positive difference between consecutive frames.
The compression is what makes a hi-hat in a quiet intro count as much as a kick in the drop.
Without it the flux follows the mastering loudness curve, and a track that rises through its
last minute reports the tempo of that minute.

Subtracting a moving average is the step that makes autocorrelation work at all. Raw flux
carries a slow swell an order of magnitude larger than the beat-to-beat structure, and
correlating it reports the arrangement, eight-bar phrases and all, rather than the pulse. The
default 0.5-second window removes everything below about 2 Hz, which is under the slowest
tempo searched.

Each band keeps its own curve. The broadband curve is their sum after each is normalised to
unit mean, so a loud bass band does not become the only voice in it. Bands zero and one, which
reach to about 130 Hz, form the low-band curve that decides bar phase, because the broadband
curve is dominated by hats and claps that fall on every beat while the kick pattern is what a
listener hears as beat one.

## Tempo

Two estimators read the same curve.

The autocorrelation comb sums the normalised autocorrelation at whole multiples of a candidate
beat lag, weighted 1/k. Agreement at the beat itself is the claim being tested; agreement four
beats out corroborates it and is worth less. The autocorrelation is normalised by the overlap
rather than by the curve length, because the biased form tapers every candidate towards zero
in proportion to its period, which is a slow-tempo penalty nobody asked for.

The Fourier tempogram takes the magnitude of a discrete transform of the curve at each beat
frequency, over twelve-second windows, averaged. Windowed rather than global: a set recorded
from vinyl drifts by a beat over several minutes, and a global transform smears that into a
plateau with no peak to pick.

Both are interpolated quadratically between grid points. A beat lag is almost never a whole
number of frames, and interpolating the autocorrelation linearly across a peak pulls the
estimate towards the nearer whole frame. At club tempo that bias is about a fifth of a BPM,
which is a beat and a half of drift over a five-minute track.

### What the comb cannot decide

At half the true tempo, every comb tooth still lands on a beat, so a harmonic sum scores the
subharmonic nearly as highly as the truth. The textbook fix subtracts the autocorrelation
halfway between the teeth, and it works against the subharmonic. It also fires on the true
tempo of any track with offbeat movement, because those offbeats sit in exactly the gaps
being penalised. On a 160 BPM hardstyle track with reverse bass on every offbeat, the penalty
at full weight reports 106.64 BPM. So it is off by default, and `--comb-penalty` exposes it.

What decides the metrical level instead is one stated rule: double the answer until it clears
90 BPM, provided the doubled candidate still scores at least half of the original. It is a
convention about how dance music is counted, not a measurement, and the report says every time
it fires.

### From a measurement to a tempo

The interpolated peak is a real number and the tempo of a produced track is not. Somebody
typed 126 into a sequencer, so a measurement of 126.02 is a measurement of 126 whose last two
digits belong to this tool rather than to the track. The reported tempo is therefore the
nearest whole number, provided it is within `--integer-snap` of what was measured, and
`bpm_measured` keeps the measurement.

The default tolerance is a quarter of a BPM. That is wider than the 0.1 BPM candidate grid and
wider than the 0.065 BPM by which the selftest misses a generated pulse train at 200 BPM, so
it closes this tool's error. It is much narrower than half a BPM, which would round everything
and would move the grid of a track that really does sit between two integers, a played set or
a tape rip, by enough to drift a beat every couple of minutes. Those keep their measurement
and raise `non-integer-tempo`.

The snap happens before the grid is fitted, so the grid, the file name and both device
databases state one number. Across the seventeen-track working set it moved every answer onto
an integer by at most 0.164 BPM, and the grid fit improved on eleven tracks, held on three and
lost a point or two on three: the largest gain took a 125 BPM deep house track from 88% of
beats matched to 100%, and its grid from twice the track novelty to six and a half times it.
Tracks are written on integers, and a grid fitted to one is measurably the better grid.

### The grid

Only the phase is searched, at a tenth of a frame. The tempo is an input, because searching
both at once finds a slightly wrong tempo that fits one section beautifully and reports it with
high confidence.

The fit is then measured against the curve it was fitted to: how much novelty sits on the grid
compared to the track mean, and how many beats have a novelty peak within 50 ms. That figure
is the tool's most useful number. A salience is a ranking within one run, while a grid fit
compares runs, which is why forcing a competing tempo and comparing fits is the way to settle
an octave.

## Key

Chroma folds spectral peaks onto twelve pitch classes, using peaks rather than every bin. An
FFT bin is a fixed width in hertz and a semitone is not, so mapping every bin assigns four
times as many of them to the top pitch class of an octave as to the bottom one. Broadband
energy then lands in a pattern set by the transform geometry, identical for every track. The
first version of this tool did exactly that and reported F major for sixteen tracks out of
seventeen.

Peaks are interpolated quadratically, discarded when more than a third of a semitone from
equal temperament, and squared before accumulating, so a partial 20 dB down contributes a
hundredth rather than a tenth. Each frame is normalised by its own total, so a drop is worth
no more than a breakdown of the same length.

The twelve-element result is correlated against major and minor profiles in all twelve
rotations. Two profiles ship. Krumhansl and Kessler's comes from probe-tone ratings on
classical stimuli, and its minor profile weights the sixth and seventh degrees heavily enough
to read a natural-minor loop as its relative major. Temperley's is fitted to note counts in a
corpus of scores, is flatter, and is the better default for music built on a repeating chord
loop. The choice changes the answer on real tracks, most often between a key and its relative,
so it is named in the report.

## Visualisation

The spectrogram is a PNG because a megapixel of magnitudes is a raster, and drawing it as SVG
rectangles produces a file no browser will open. Its axes are a separate SVG that references
the PNG, so the labels stay selectable text and a frequency can be copied out of a report. The
HTML page embeds that SVG inline rather than linking it, because an SVG loaded through an
`img` element may not fetch anything and would render as empty axes.

Every other figure is hand-written SVG. The plots are one shape each, and a plotting crate
would pull a font stack and a raster pipeline into a build that otherwise needs neither.

Curves are max-pooled, not sampled, when there are more points than pixels. The peaks are the
signal, and a decimated novelty curve that drops them looks like a track with no onsets.
