//! Placing a beat grid on the novelty curve, and measuring how badly it fits.
//!
//! The fit is the point. A tempo estimate on its own is unfalsifiable; a grid
//! that lands 40 ms early on every beat of the second half says the track is not
//! at a constant tempo, and a grid that matches a third of its beats says the
//! candidate is wrong however confident its salience looked.

use crate::novelty::Novelty;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct BeatGrid {
    pub bpm: f64,
    /// Where the first beat sits, in seconds from the start of the analysed
    /// audio (not of the file, when an excerpt was analysed).
    pub offset_seconds: f64,
    pub beats_seconds: Vec<f64>,
    /// Mean novelty on the grid divided by mean novelty everywhere. At 1.0 the
    /// grid is no better than an arbitrary one; a clean four-to-the-floor track
    /// lands between 2 and 4.
    pub pulse_ratio: f64,
    /// Fraction of beats with a novelty peak inside the tolerance.
    pub matched_fraction: f64,
    /// Mean distance from a beat to its nearest peak, over matched beats.
    pub mean_absolute_error_ms: f64,
    pub tolerance_ms: f64,
}

/// Best-fitting grid at a fixed tempo.
///
/// Only the phase is searched. The tempo is an input, because a search over
/// both at once finds a slightly wrong tempo that fits one section beautifully
/// and reports it with high confidence.
pub fn align(novelty: &Novelty, bpm: f64, tolerance_ms: f64) -> BeatGrid {
    let lag = 60.0 * novelty.frame_rate / bpm;
    let beat_count = ((novelty.values.len() as f64 - 1.0) / lag).floor().max(0.0) as usize;

    let mut best_offset = 0.0;
    let mut best_score = f64::MIN;
    let steps = (lag * 10.0).ceil() as usize;
    for step in 0..steps.max(1) {
        let offset = step as f64 * lag / steps as f64;
        let score: f64 = (0..=beat_count)
            .map(|k| novelty.at(offset + k as f64 * lag) as f64)
            .sum();
        if score > best_score {
            best_score = score;
            best_offset = offset;
        }
    }

    let beats_frames: Vec<f64> = (0..=beat_count)
        .map(|k| best_offset + k as f64 * lag)
        .collect();
    let mean = novelty.mean() as f64;
    let pulse_ratio = if mean > 0.0 && !beats_frames.is_empty() {
        (best_score / beats_frames.len() as f64) / mean
    } else {
        0.0
    };

    let peaks = peak_frames(novelty);
    let tolerance_frames = tolerance_ms / 1000.0 * novelty.frame_rate;
    let mut matched = 0usize;
    let mut error_total = 0.0;
    for &beat in &beats_frames {
        if let Some(distance) = nearest_distance(&peaks, beat)
            && distance <= tolerance_frames
        {
            matched += 1;
            error_total += distance;
        }
    }

    BeatGrid {
        bpm,
        offset_seconds: best_offset / novelty.frame_rate,
        beats_seconds: beats_frames
            .iter()
            .map(|f| f / novelty.frame_rate)
            .collect(),
        pulse_ratio,
        matched_fraction: if beats_frames.is_empty() {
            0.0
        } else {
            matched as f64 / beats_frames.len() as f64
        },
        mean_absolute_error_ms: if matched == 0 {
            f64::NAN
        } else {
            error_total / matched as f64 / novelty.frame_rate * 1000.0
        },
        tolerance_ms,
    }
}

/// Which beat of the bar carries the emphasis.
#[derive(Clone, Debug, Serialize)]
pub struct BarPhase {
    pub beats_per_bar: usize,
    /// Index in `0..beats_per_bar` of the beat that starts the bar.
    pub phase: usize,
    /// Emphasis on that beat over the mean of the others. Near 1.0 the bar line
    /// is a guess: every beat is equally loud, which is common in a loop and in
    /// anything without a backbeat.
    pub contrast: f64,
}

/// Bar phase from a low-band curve.
///
/// Deliberately a separate input: the broadband novelty is dominated by hats and
/// claps, which are on every beat, while the kick pattern is what a listener
/// hears as beat one.
pub fn bar_phase(grid: &BeatGrid, low_band: &Novelty, beats_per_bar: usize) -> BarPhase {
    let mut totals = vec![0.0f64; beats_per_bar];
    let mut counts = vec![0.0f64; beats_per_bar];
    for (index, &time) in grid.beats_seconds.iter().enumerate() {
        let slot = index % beats_per_bar;
        totals[slot] += low_band.at(time * low_band.frame_rate) as f64;
        counts[slot] += 1.0;
    }
    let means: Vec<f64> = totals
        .iter()
        .zip(&counts)
        .map(|(t, c)| if *c > 0.0 { t / c } else { 0.0 })
        .collect();

    let (phase, &strongest) = means
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap_or((0, &0.0));
    let others: f64 = means
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != phase)
        .map(|(_, v)| *v)
        .sum::<f64>()
        / (beats_per_bar.saturating_sub(1)).max(1) as f64;

    BarPhase {
        beats_per_bar,
        phase,
        contrast: if others > 0.0 {
            strongest / others
        } else {
            0.0
        },
    }
}

/// Frame positions of novelty peaks above the curve's mean.
fn peak_frames(novelty: &Novelty) -> Vec<f64> {
    let mean = novelty.mean();
    let mut peaks = Vec::new();
    for i in 1..novelty.values.len().saturating_sub(1) {
        let (previous, here, next) = (
            novelty.values[i - 1],
            novelty.values[i],
            novelty.values[i + 1],
        );
        if here > previous && here >= next && here > mean {
            let denominator = previous - 2.0 * here + next;
            let offset = if denominator.abs() > f32::EPSILON {
                (0.5 * (previous - next) / denominator).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            peaks.push(i as f64 + offset as f64);
        }
    }
    peaks
}

/// Distance from `position` to the nearest peak, in frames.
fn nearest_distance(peaks: &[f64], position: f64) -> Option<f64> {
    if peaks.is_empty() {
        return None;
    }
    let index = peaks.partition_point(|&p| p < position);
    let before = index.checked_sub(1).map(|i| position - peaks[i]);
    let after = peaks.get(index).map(|&p| p - position);
    match (before, after) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}
