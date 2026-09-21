//! Finding where the track changes, from how little each bar resembles the
//! bars around it.
//!
//! The method is Foote's: build a self-similarity matrix over bars, slide a
//! checkerboard kernel down its diagonal, and read the peaks. A kernel over a
//! stretch that sounds the same throughout sees four quadrants that all match
//! and scores nothing; a kernel straddling a boundary sees two that match and
//! two that do not, and scores.
//!
//! Bars rather than beats, because a section boundary in dance music is a bar
//! boundary and comparing beats finds the backbeat instead.

use crate::features::Bar;

/// Bars either side of the boundary the kernel looks at.
///
/// Eight bars is a phrase, which is the shortest thing anybody would call a
/// section. Wider smooths over a four-bar fill; narrower fires on one.
const KERNEL_BARS: usize = 8;

/// How strong a peak has to be, in standard deviations above the mean novelty,
/// before it is called a boundary.
const PEAK_THRESHOLD_SIGMA: f64 = 0.8;

/// A change this tool found, and how sure the curve was about it.
#[derive(Clone, Copy, Debug)]
pub struct Boundary {
    pub bar: usize,
    pub novelty: f64,
    /// Novelty in standard deviations above the mean, which is what the
    /// threshold was applied to and what the report prints.
    pub strength: f64,
}

/// The novelty curve over bars, and the boundaries picked off it.
pub struct Segmentation {
    pub novelty: Vec<f64>,
    pub boundaries: Vec<Boundary>,
}

/// Cosine similarity between every pair of bars, convolved with a checkerboard.
///
/// The matrix is never built: each kernel position needs only the block of
/// similarities around its own bar, so the cost is bars times kernel squared
/// rather than bars squared, and nothing the size of a track is held.
pub fn novelty(bars: &[Bar]) -> Vec<f64> {
    let shapes: Vec<Vec<f64>> = bars.iter().map(Bar::shape).collect();
    let similarity = |a: usize, b: usize| -> f64 {
        shapes[a]
            .iter()
            .zip(&shapes[b])
            .map(|(x, y)| x * y)
            .sum::<f64>()
    };

    let mean_over = |rows: std::ops::Range<usize>, columns: std::ops::Range<usize>| {
        let count = rows.len() * columns.len();
        if count == 0 {
            return 0.0;
        }
        let total: f64 = rows
            .flat_map(|a| columns.clone().map(move |b| (a, b)))
            .map(|(a, b)| similarity(a, b))
            .sum();
        total / count as f64
    };

    (0..bars.len())
        .map(|centre| {
            // A kernel that runs off either end of the track has nothing to
            // compare on that side, and scoring it anyway makes the first and
            // last bars look like the biggest changes in the track.
            if centre < KERNEL_BARS || centre + KERNEL_BARS > bars.len() {
                return 0.0;
            }
            let before = centre - KERNEL_BARS..centre;
            let after = centre..centre + KERNEL_BARS;

            // Two quadrants comparing a side with itself, two crossing the
            // boundary. A stretch that sounds the same throughout scores the
            // same in all four and nets nothing.
            let same = (mean_over(before.clone(), before.clone())
                + mean_over(after.clone(), after.clone()))
                / 2.0;
            let across = mean_over(before, after);
            (same - across).max(0.0)
        })
        .collect()
}

/// Peaks of the novelty curve, snapped to the phrase grid.
///
/// Snapping is what turns a boundary that is musically right but a bar late
/// into one a player can cue to. Dance music is written in four-bar phrases and
/// a cue half a bar out is audible; the snap moves a peak at most half a phrase
/// and the report carries the bar it moved from.
pub fn boundaries(novelty: &[f64], phrase_bars: usize, minimum_spacing: usize) -> Vec<Boundary> {
    if novelty.is_empty() {
        return Vec::new();
    }
    let mean = novelty.iter().sum::<f64>() / novelty.len() as f64;
    let variance =
        novelty.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / novelty.len() as f64;
    let deviation = variance.sqrt();
    let strength = |value: f64| {
        if deviation > 0.0 {
            (value - mean) / deviation
        } else {
            0.0
        }
    };

    let mut found: Vec<Boundary> = Vec::new();
    for (bar, value) in novelty.iter().enumerate() {
        let rising = bar == 0 || *value >= novelty[bar - 1];
        let falling = bar + 1 == novelty.len() || *value >= novelty[bar + 1];
        if !(rising && falling) || strength(*value) < PEAK_THRESHOLD_SIGMA {
            continue;
        }
        let snapped = snap(bar, phrase_bars);
        if snapped == 0 || snapped >= novelty.len() {
            continue;
        }
        found.push(Boundary {
            bar: snapped,
            novelty: *value,
            strength: strength(*value),
        });
    }

    // Strongest first, then drop anything too close to something already kept,
    // so a section is never shorter than a phrase or two.
    found.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    let mut kept: Vec<Boundary> = Vec::new();
    for candidate in found {
        if kept
            .iter()
            .any(|other| other.bar.abs_diff(candidate.bar) < minimum_spacing)
        {
            continue;
        }
        kept.push(candidate);
    }
    kept.sort_by_key(|boundary| boundary.bar);
    kept
}

/// The nearest multiple of the phrase length.
fn snap(bar: usize, phrase_bars: usize) -> usize {
    if phrase_bars == 0 {
        return bar;
    }
    let below = bar / phrase_bars * phrase_bars;
    let above = below + phrase_bars;
    if bar - below <= above - bar {
        below
    } else {
        above
    }
}

pub fn segment(bars: &[Bar], phrase_bars: usize, minimum_spacing: usize) -> Segmentation {
    let novelty = novelty(bars);
    let boundaries = boundaries(&novelty, phrase_bars, minimum_spacing);
    Segmentation {
        novelty,
        boundaries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(start: f64, energies: Vec<f64>) -> Bar {
        Bar {
            start_seconds: start,
            end_seconds: start + 2.0,
            energies,
        }
    }

    /// Two stretches that sound nothing alike, joined at a known bar.
    fn two_halves(bars_each: usize) -> Vec<Bar> {
        (0..bars_each * 2)
            .map(|index| {
                let energies = if index < bars_each {
                    vec![0.5, 0.4, 0.01, 0.01]
                } else {
                    vec![0.01, 0.01, 0.4, 0.5]
                };
                bar(index as f64 * 2.0, energies)
            })
            .collect()
    }

    #[test]
    fn the_novelty_peaks_where_the_track_changes() {
        let bars = two_halves(16);
        let curve = novelty(&bars);
        let peak = curve
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(bar, _)| bar)
            .expect("a curve over 32 bars has a maximum");
        assert!(
            peak.abs_diff(16) <= 1,
            "the change is at bar 16 and the novelty peaked at {peak}"
        );
    }

    #[test]
    fn a_boundary_lands_on_the_phrase_grid() {
        let bars = two_halves(16);
        let found = segment(&bars, 4, 8);
        assert!(
            found.boundaries.iter().any(|b| b.bar == 16),
            "expected a boundary at bar 16, found {:?}",
            found.boundaries.iter().map(|b| b.bar).collect::<Vec<_>>()
        );
        for boundary in &found.boundaries {
            assert_eq!(
                boundary.bar % 4,
                0,
                "boundary at bar {} is not on the four-bar phrase grid",
                boundary.bar
            );
        }
    }

    #[test]
    fn a_track_that_never_changes_has_no_boundary() {
        let bars: Vec<Bar> = (0..48)
            .map(|index| bar(index as f64 * 2.0, vec![0.4, 0.3, 0.2, 0.1]))
            .collect();
        let found = segment(&bars, 4, 8);
        assert!(
            found.boundaries.is_empty(),
            "a bar that repeats 48 times has no boundary, found {:?}",
            found.boundaries.iter().map(|b| b.bar).collect::<Vec<_>>()
        );
    }

    #[test]
    fn snapping_moves_a_bar_no_further_than_half_a_phrase() {
        for bar in 0..64usize {
            let snapped = snap(bar, 4);
            assert_eq!(snapped % 4, 0, "bar {bar} snapped to {snapped}");
            assert!(
                snapped.abs_diff(bar) <= 2,
                "bar {bar} snapped to {snapped}, which is more than half a phrase"
            );
        }
    }
}
