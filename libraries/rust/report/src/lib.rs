//! What a run of the tool leaves behind: one JSON document, a set of plots, and
//! an HTML page that puts the plots next to the numbers they came from.
//!
//! The JSON is the contract. Every figure printed to the terminal or drawn in a
//! plot is in it, so a disagreement about an answer is settled by reading the
//! run rather than by running it again with different flags.

pub mod heatmap;
pub mod html;
pub mod plot;

pub use heatmap::Heatmap;
pub use plot::{BarChart, LinePlot, Marker, Series};

use diagnostics::{Diagnostic, Severity};
use key_detect::KeyAnalysis;
use serde::Serialize;
use tempo::TempoAnalysis;
use waveform::Waveform;

#[derive(Clone, Debug, Serialize)]
pub struct SourceInfo {
    pub path: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_seconds: f64,
    /// Where the analysed excerpt starts and how long it is. Equal to the whole
    /// file unless `--start` or `--duration` narrowed it, and always printed,
    /// because an answer about 30 seconds of a track is a different claim from
    /// an answer about the track.
    pub analysed_start_seconds: f64,
    pub analysed_seconds: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnalysisSettings {
    pub window_size: usize,
    pub hop: usize,
    pub frame_rate: f64,
    pub onset_bands: usize,
    pub flux_compression: f32,
    pub local_mean_seconds: f64,
}

/// Tempo measured from one frequency band on its own.
///
/// A kick that says 150 and hats that say 100 is a polyrhythm or a shuffle, and
/// it is the fastest way to see why a broadband estimate landed between them.
#[derive(Clone, Debug, Serialize)]
pub struct BandTempo {
    pub band: usize,
    pub low_hz: f64,
    pub high_hz: f64,
    pub bpm: f64,
    pub salience: f64,
}

/// Long-term properties of the spectrum, independent of tempo and key.
#[derive(Clone, Debug, Serialize)]
pub struct SpectrumSummary {
    pub peak_hz: f64,
    /// Energy-weighted mean frequency: where the track sits, in one number.
    pub centroid_hz: f64,
    /// Frequency below which 95% of the energy lies.
    pub rolloff_95_hz: f64,
    /// Highest frequency still within 40 dB of the peak. A track cut from a
    /// lossy source shows a hard ceiling here, usually at 16 kHz or 19 kHz,
    /// whatever the file extension says.
    pub high_cutoff_hz: f64,
    /// Share of total energy below 200 Hz, from 200 Hz to 4 kHz, and above.
    pub low_share: f64,
    pub mid_share: f64,
    pub high_share: f64,
}

/// The two waveforms a player draws, at the resolutions their formats read.
///
/// Here rather than only in an exporter because they are measurements of the
/// audio like everything else in the report, and because the export step reads
/// this file instead of decoding the track a second time.
#[derive(Clone, Debug, Serialize)]
pub struct Waveforms {
    /// 400 columns spanning the whole track.
    pub preview: Waveform,
    /// 150 columns per second.
    pub detail: Waveform,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnalysisReport {
    pub tool: &'static str,
    pub version: &'static str,
    pub source: SourceInfo,
    pub settings: AnalysisSettings,
    pub tempo: TempoAnalysis,
    pub key: KeyAnalysis,
    pub bands: Vec<BandTempo>,
    pub spectrum: SpectrumSummary,
    pub waveforms: Waveforms,
}

impl AnalysisReport {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Every diagnostic from every stage, warnings first.
    pub fn diagnostics(&self) -> Vec<&Diagnostic> {
        let mut all: Vec<&Diagnostic> = self
            .tempo
            .diagnostics
            .iter()
            .chain(self.key.diagnostics.iter())
            .collect();
        all.sort_by_key(|d| match d.severity {
            Severity::Warning => 0,
            Severity::Info => 1,
        });
        all
    }
}
