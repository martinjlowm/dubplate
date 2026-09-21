//! Where a track changes, what each stretch is, and which pad to put it on.
//!
//! rekordbox does this from a phrase model nobody outside Pioneer has, and the
//! tools that place cues automatically read its answer back out of the `.EXT`
//! file. This tool writes those files rather than reading them, and rule 13
//! says it reads audio and nothing else, so the sections here are measured
//! from the samples: band energies on a coarse grid during the one transform
//! pass, a self-similarity over bars, and peaks of a checkerboard novelty.
//!
//! What that cannot do is find a vocal. Vocal detection means separating the
//! stem, which is out of scope, so the pad rekordbox tooling gives to the first
//! vocal is given here to the build that runs into the drop, and the report
//! says that is what it is.
//!
//! Nothing here changes the tempo, the grid or the key. It reads the grid the
//! tempo stage produced and reports over it.

pub mod cues;
pub mod features;
pub mod label;
pub mod segment;

pub use cues::{Cue, CueKind, CueSettings, Cues, Role};
pub use features::{Grid, SectionFeatures};
pub use label::{Label, Section, WEAK_CONFIDENCE};
pub use segment::Boundary;

use diagnostics::Diagnostic;
use serde::Serialize;

/// Bars in a phrase, which is what a boundary snaps to.
const PHRASE_BARS: usize = 4;

/// Shortest section this will report, in bars. Two phrases: anything shorter is
/// a fill, and a cue on a fill is a cue in the wrong place.
const MINIMUM_SECTION_BARS: usize = 8;

/// Bars needed before a segmentation is worth attempting at all.
const MINIMUM_BARS: usize = 24;

/// What the structure stage reports.
#[derive(Clone, Debug, Serialize)]
pub struct StructureAnalysis {
    pub sections: Vec<Section>,
    pub cues: Vec<Cue>,
    /// Pads no section could fill.
    pub missing_pads: Vec<String>,
    /// Every boundary the novelty curve offered, including the ones that became
    /// section edges. Here because a section list with no curve behind it
    /// cannot be argued with.
    pub boundaries: Vec<BoundaryReport>,
    pub settings: StructureSettings,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BoundaryReport {
    pub bar: usize,
    pub time_seconds: f64,
    /// Novelty in standard deviations above the mean of the curve.
    pub strength: f64,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct StructureSettings {
    pub phrase_bars: usize,
    pub minimum_section_bars: usize,
    pub memory_offset_bars: usize,
    pub loop_bars: usize,
    pub drop_after_fraction: f64,
}

/// Measure the structure and place the cues.
///
/// `bar_starts` is the time of every bar plus the end of the last, which is
/// what the tempo stage's grid folds down to.
pub fn analyse(
    grid: &Grid,
    bar_starts: &[f64],
    duration_seconds: f64,
    settings: &CueSettings,
) -> StructureAnalysis {
    let reported = StructureSettings {
        phrase_bars: PHRASE_BARS,
        minimum_section_bars: MINIMUM_SECTION_BARS,
        memory_offset_bars: settings.memory_offset_bars,
        loop_bars: settings.loop_bars,
        drop_after_fraction: settings.drop_after_fraction,
    };
    let mut diagnostics = Vec::new();

    let bars = grid.per_bar(bar_starts);
    if bars.len() < MINIMUM_BARS {
        diagnostics.push(Diagnostic::warning(
            "track-too-short-to-segment",
            format!(
                "{} bars of grid is too little to find sections in; {MINIMUM_BARS} is the minimum, so no cues were placed beyond the first beat",
                bars.len()
            ),
        ));
        return StructureAnalysis {
            sections: Vec::new(),
            cues: first_beat_only(bar_starts),
            missing_pads: Vec::new(),
            boundaries: Vec::new(),
            settings: reported,
            diagnostics,
        };
    }

    let found = segment::segment(&bars, PHRASE_BARS, MINIMUM_SECTION_BARS);
    let boundaries: Vec<BoundaryReport> = found
        .boundaries
        .iter()
        .map(|boundary| BoundaryReport {
            bar: boundary.bar,
            time_seconds: bar_starts.get(boundary.bar).copied().unwrap_or(0.0),
            strength: boundary.strength,
        })
        .collect();

    if found.boundaries.is_empty() {
        diagnostics.push(Diagnostic::warning(
            "no-section-boundaries",
            "no bar changed enough from its neighbours to be a section boundary, so the whole track is one section and only the first beat and the loop pads are placed",
        ));
    }

    let edges: Vec<usize> = found.boundaries.iter().map(|b| b.bar).collect();
    let sections = label::label(&bars, &edges);

    let weak: Vec<String> = sections
        .iter()
        .filter(|section| section.confidence < WEAK_CONFIDENCE)
        .map(|section| {
            format!(
                "{} at {}",
                section.label.as_str(),
                format_time(section.start_seconds)
            )
        })
        .collect();
    if !weak.is_empty() {
        diagnostics.push(Diagnostic::warning(
            "weak-section-label",
            format!(
                "{} of {} sections sat close enough to the threshold that a different label was nearly as good: {}",
                weak.len(),
                sections.len(),
                weak.join(", ")
            ),
        ));
    }

    if !sections.iter().any(|section| section.label == Label::Drop) {
        diagnostics.push(Diagnostic::info(
            "no-drop-found",
            "no stretch carried both a full kick and the loudest the track gets, so the drop pad is empty",
        ));
    }

    let placed = cues::place(&sections, bar_starts, duration_seconds, settings);
    if placed.drop_rule_relaxed {
        diagnostics.push(Diagnostic::warning(
            "drop-rule-relaxed",
            format!(
                "every drop started inside the opening {:.0}% of the track, so the drop pad took the earliest one rather than staying empty",
                settings.drop_after_fraction * 100.0
            ),
        ));
    }
    if !placed.missing.is_empty() {
        diagnostics.push(Diagnostic::info(
            "cue-pads-empty",
            format!(
                "pads {} had no section to mark and were left empty rather than filled with the nearest thing",
                placed.missing.join(", ")
            ),
        ));
    }

    StructureAnalysis {
        sections,
        cues: placed.cues,
        missing_pads: placed.missing,
        boundaries,
        settings: reported,
        diagnostics,
    }
}

/// A player with no cue at all parks at the start of the file, which on a track
/// with a silent lead-in is the wrong place. So even a track too short to
/// segment gets its first beat marked.
fn first_beat_only(bar_starts: &[f64]) -> Vec<Cue> {
    bar_starts
        .first()
        .map(|time_seconds| {
            [CueKind::Hot, CueKind::Memory]
                .into_iter()
                .map(|kind| Cue {
                    kind,
                    number: 1,
                    name: Role::FirstBeat.name(),
                    colour: Role::FirstBeat.colour(),
                    time_seconds: *time_seconds,
                    bar: 0,
                    from_section: None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Minutes and seconds, which is how a finding about a position reads.
pub fn format_time(seconds: f64) -> String {
    let whole = seconds.max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// Fold a beat grid into the time of every bar, plus the end of the last one.
///
/// The bar phase says which beat carries the low end; bars start there, and the
/// beats before the first downbeat are not a bar.
pub fn bar_starts(beats_seconds: &[f64], beats_per_bar: usize, phase: usize) -> Vec<f64> {
    if beats_per_bar == 0 {
        return Vec::new();
    }
    let first_downbeat = phase % beats_per_bar;
    beats_seconds
        .iter()
        .skip(first_downbeat)
        .step_by(beats_per_bar)
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_start_on_the_downbeat_the_phase_names() {
        let beats: Vec<f64> = (0..16).map(|beat| beat as f64 * 0.5).collect();
        assert_eq!(bar_starts(&beats, 4, 0), vec![0.0, 2.0, 4.0, 6.0]);
        // A phase of 2 means beat one is the third beat of the file.
        assert_eq!(bar_starts(&beats, 4, 2), vec![1.0, 3.0, 5.0, 7.0]);
    }

    #[test]
    fn a_track_with_almost_no_grid_says_so_and_still_marks_the_first_beat() {
        let mut features = SectionFeatures::new(4, 100.0);
        for frame in 0..500 {
            features.push(frame, &[0.2, 0.2, 0.2, 0.2]);
        }
        let grid = features.finish();
        let bars: Vec<f64> = (0..6).map(|bar| bar as f64 * 2.0).collect();
        let analysis = analyse(&grid, &bars, 10.0, &CueSettings::default());

        assert!(analysis.sections.is_empty(), "six bars is not a structure");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == "track-too-short-to-segment"),
            "a track too short to segment has to say so"
        );
        assert_eq!(
            analysis.cues.len(),
            2,
            "the first beat is still marked, hot and memory"
        );
    }
}
