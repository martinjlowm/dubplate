//! Saying what each stretch of the track is.
//!
//! The segmentation says where the track changes; this says what it changed
//! into. Three measurements decide it, all of them already in the bar features:
//! how much low end the stretch carries, how loud it is across the spectrum,
//! and which way its loudness is moving.
//!
//! The thresholds are relative to the track, never absolute. A quiet master and
//! a loud one have the same shape, and a rule written in decibels from full
//! scale would call every stretch of the quiet one a breakdown.

use crate::features::Bar;
use serde::Serialize;

/// Bands counted as the low end, which is what the kick lives in.
///
/// The onset detector splits 30 Hz to 16 kHz over its bands, so the bottom two
/// of eight reach to about 130 Hz. The tempo stage reads the same two.
const LOW_BANDS: usize = 2;

/// How far below the loudest section's low end a stretch has to sit before its
/// kick counts as gone.
const KICK_GONE_DB: f64 = 9.0;

/// How close to the loudest low end a stretch has to sit for its kick to count
/// as full.
const KICK_FULL_DB: f64 = 3.0;

/// Rise across a stretch, in decibels from its first bar to its last, that
/// counts as a build.
const BUILD_RISE_DB: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Label {
    /// The opening. Named by position, not by sound.
    Intro,
    /// Energy climbing with the kick out or thinned, running into a drop.
    Build,
    /// Full kick and the loudest the track gets.
    Drop,
    /// The kick gone in the middle of the track.
    Breakdown,
    /// The closing. Named by position.
    Outro,
    /// A stretch that is none of the above: the track changed, and what it
    /// changed into is not one of the five shapes this looks for.
    Steady,
}

impl Label {
    pub fn as_str(self) -> &'static str {
        match self {
            Label::Intro => "intro",
            Label::Build => "build",
            Label::Drop => "drop",
            Label::Breakdown => "breakdown",
            Label::Outro => "outro",
            Label::Steady => "steady",
        }
    }
}

/// One stretch of the track, with the measurements the label was read from.
///
/// Every number here is in the report. A label is a claim, and a claim with no
/// evidence beside it is the thing this tool exists not to print.
#[derive(Clone, Debug, Serialize)]
pub struct Section {
    pub label: Label,
    /// How far the measurements sat from the threshold that would have named
    /// this something else, in [0, 1]. Below [`WEAK_CONFIDENCE`] the label is
    /// a coin toss and a finding says so.
    pub confidence: f64,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub start_bar: usize,
    pub bars: usize,
    /// Mean energy below roughly 130 Hz, in decibels.
    pub low_band_db: f64,
    /// Mean energy across every band, in decibels.
    pub broadband_db: f64,
    /// Change in broadband energy from the first bar of the stretch to the
    /// last, in decibels. Positive is rising.
    pub rise_db: f64,
}

/// Below this the label is reported and a finding says not to trust it.
pub const WEAK_CONFIDENCE: f64 = 0.35;

/// Measure each stretch, then name it.
pub fn label(bars: &[Bar], boundaries: &[usize]) -> Vec<Section> {
    let spans = spans(bars.len(), boundaries);
    let mut sections: Vec<Section> = spans
        .iter()
        .map(|(start, end)| measure(bars, *start, *end))
        .collect();
    if sections.is_empty() {
        return sections;
    }

    // Both scales are the track's own. The loudest low end is whatever this
    // track's kick is, and every other stretch is read against it.
    let loudest_low = sections
        .iter()
        .map(|section| section.low_band_db)
        .fold(f64::MIN, f64::max);
    let loudest_broadband = sections
        .iter()
        .map(|section| section.broadband_db)
        .fold(f64::MIN, f64::max);

    let last = sections.len() - 1;
    for (index, section) in sections.iter_mut().enumerate() {
        let (label, confidence) = name(section, index, last, loudest_low, loudest_broadband);
        section.label = label;
        section.confidence = confidence;
    }

    // A build is only a build if something drops after it. A stretch that
    // climbs and then ends is an outro that got louder.
    for index in 0..sections.len() {
        if sections[index].label == Label::Build
            && !sections[index + 1..]
                .iter()
                .any(|later| later.label == Label::Drop)
        {
            sections[index].label = Label::Steady;
            sections[index].confidence = 0.0;
        }
    }
    sections
}

/// Bar ranges between the boundaries, with the ends of the track closing the
/// first and last.
fn spans(bar_count: usize, boundaries: &[usize]) -> Vec<(usize, usize)> {
    if bar_count == 0 {
        return Vec::new();
    }
    let mut edges = vec![0usize];
    edges.extend(boundaries.iter().copied().filter(|bar| *bar < bar_count));
    edges.push(bar_count);
    edges.dedup();
    edges.windows(2).map(|pair| (pair[0], pair[1])).collect()
}

fn measure(bars: &[Bar], start: usize, end: usize) -> Section {
    let span = &bars[start..end];
    let mean = |values: Vec<f64>| {
        if values.is_empty() {
            crate::features::to_db(0.0)
        } else {
            values.iter().sum::<f64>() / values.len() as f64
        }
    };
    let low_band_db = mean(span.iter().map(|bar| bar.low_band_db(LOW_BANDS)).collect());
    let broadband_db = mean(span.iter().map(Bar::broadband_db).collect());

    // First quarter against last quarter rather than first bar against last,
    // so one loud bar at the end of a stretch is not a build.
    let quarter = (span.len() / 4).max(1);
    let opening = mean(span[..quarter].iter().map(Bar::broadband_db).collect());
    let closing = mean(
        span[span.len() - quarter..]
            .iter()
            .map(Bar::broadband_db)
            .collect(),
    );

    Section {
        label: Label::Steady,
        confidence: 0.0,
        start_seconds: span.first().map_or(0.0, |bar| bar.start_seconds),
        end_seconds: span.last().map_or(0.0, |bar| bar.end_seconds),
        start_bar: start,
        bars: end - start,
        low_band_db,
        broadband_db,
        rise_db: closing - opening,
    }
}

/// Name one stretch, and say how far it sat from being named something else.
fn name(
    section: &Section,
    index: usize,
    last: usize,
    loudest_low: f64,
    loudest_broadband: f64,
) -> (Label, f64) {
    let below_loudest_low = loudest_low - section.low_band_db;
    let below_loudest_broadband = loudest_broadband - section.broadband_db;

    // Distance from a threshold, scaled so a stretch sitting one whole
    // threshold clear of it reads as certain.
    let margin = |distance: f64, scale: f64| (distance.abs() / scale).clamp(0.0, 1.0);

    if index == 0 {
        // The opening is the opening whatever it sounds like, so the only
        // doubt is whether it is long enough to be a section at all.
        return (Label::Intro, margin(section.bars as f64, 8.0));
    }
    if index == last {
        return (Label::Outro, margin(section.bars as f64, 8.0));
    }

    let kick_full = below_loudest_low <= KICK_FULL_DB;
    let kick_gone = below_loudest_low >= KICK_GONE_DB;
    let loud = below_loudest_broadband <= KICK_FULL_DB;

    if kick_full && loud {
        let confidence = margin(KICK_FULL_DB - below_loudest_low, KICK_FULL_DB)
            .min(margin(KICK_FULL_DB - below_loudest_broadband, KICK_FULL_DB));
        return (Label::Drop, confidence);
    }
    if section.rise_db >= BUILD_RISE_DB && !kick_full {
        let confidence = margin(section.rise_db - BUILD_RISE_DB, BUILD_RISE_DB);
        return (Label::Build, confidence);
    }
    if kick_gone {
        let confidence = margin(below_loudest_low - KICK_GONE_DB, KICK_GONE_DB);
        return (Label::Breakdown, confidence);
    }
    // Nothing claimed it. The confidence is how clear it is that none of the
    // three rules nearly fired, which is what makes a steady stretch either
    // plainly ordinary or a near miss worth reading.
    let nearest = (KICK_FULL_DB - below_loudest_low)
        .abs()
        .min((below_loudest_low - KICK_GONE_DB).abs())
        .min((section.rise_db - BUILD_RISE_DB).abs());
    (Label::Steady, margin(nearest, KICK_FULL_DB))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A run of bars. `gain` multiplies the last bar's energies against the
    /// first, so a stretch can climb the way a build does.
    fn run(count: usize, energies: [f64; 4], gain: f64) -> Vec<(usize, [f64; 4])> {
        (0..count)
            .map(|index| {
                let along = if count > 1 {
                    index as f64 / (count - 1) as f64
                } else {
                    0.0
                };
                let scale = 1.0 + along * (gain - 1.0);
                (
                    index,
                    [
                        energies[0] * scale,
                        energies[1] * scale,
                        energies[2] * scale,
                        energies[3] * scale,
                    ],
                )
            })
            .collect()
    }

    fn bars_of(runs: &[Vec<(usize, [f64; 4])>]) -> Vec<Bar> {
        let mut bars = Vec::new();
        for stretch in runs {
            for (_, energies) in stretch {
                let start = bars.len() as f64 * 2.0;
                bars.push(Bar {
                    start_seconds: start,
                    end_seconds: start + 2.0,
                    energies: energies.to_vec(),
                });
            }
        }
        bars
    }

    /// Quiet opening, a climb with no kick, a full drop, the kick gone, a
    /// second drop, a fade. The shape every club record has.
    fn a_dance_track() -> Vec<Bar> {
        bars_of(&[
            run(16, [0.02, 0.02, 0.02, 0.01], 1.0),
            // The climb: no kick, and four times the energy by the end of it.
            run(16, [0.02, 0.02, 0.10, 0.10], 4.0),
            run(32, [0.50, 0.40, 0.30, 0.20], 1.0),
            run(16, [0.01, 0.01, 0.10, 0.10], 1.0),
            run(32, [0.50, 0.40, 0.30, 0.20], 1.0),
            run(16, [0.05, 0.05, 0.05, 0.03], 1.0),
        ])
    }

    #[test]
    fn the_five_shapes_are_named_from_the_measurements() {
        let bars = a_dance_track();
        let sections = label(&bars, &[16, 32, 64, 80, 112]);
        let names: Vec<&str> = sections
            .iter()
            .map(|section| section.label.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["intro", "build", "drop", "breakdown", "drop", "outro"],
            "sections measured {:?}",
            sections
                .iter()
                .map(|s| (s.label.as_str(), s.low_band_db, s.broadband_db, s.rise_db))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_climb_that_leads_nowhere_is_not_a_build() {
        // The same opening and climb, then nothing louder after it.
        let bars = bars_of(&[
            run(16, [0.02, 0.02, 0.02, 0.01], 1.0),
            run(16, [0.02, 0.02, 0.10, 0.10], 4.0),
            run(16, [0.02, 0.02, 0.10, 0.10], 4.0),
        ]);
        let sections = label(&bars, &[16, 32]);
        assert_ne!(
            sections[1].label,
            Label::Build,
            "a climb with no drop after it is not a build, got {:?}",
            sections.iter().map(|s| s.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_track_with_no_boundary_is_one_section() {
        let bars = bars_of(&[run(32, [0.3, 0.3, 0.3, 0.3], 1.0)]);
        let sections = label(&bars, &[]);
        assert_eq!(sections.len(), 1, "no boundary means one section");
        assert_eq!(sections[0].label, Label::Intro, "the only section opens");
    }
}
