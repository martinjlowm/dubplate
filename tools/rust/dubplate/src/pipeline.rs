//! One pass over the audio, feeding every stage that needs a spectrum.
//!
//! The order is fixed and shallow: tuning, then the transform pass, then tempo
//! and key over what it collected. Only the tuning offset needs its own pass,
//! because the chroma mapping is built around it and cannot be corrected after
//! the fold.

use crate::AnalysisOptions;
use anyhow::{Result, bail};
use audio::Audio;
use report::{
    AnalysisReport, AnalysisSettings, BandTempo, Heatmap, SourceInfo, SpectrumSummary, Waveforms,
};
use spectral::{ChromaMapper, LogBands, Stft, TuningEstimator};
use tempo::{FluxAccumulator, Novelty, TempoPrior, TempoSettings};

/// Columns and rows of the spectrogram image. The width is a compromise between
/// a legible page and a pixel per frame; at 1600 columns a seven-minute track
/// averages about thirty frames per column, which keeps a drop visible as an
/// edge.
const IMAGE_WIDTH: usize = 1600;
const IMAGE_HEIGHT: usize = 440;
/// Dynamic range drawn in the spectrogram. Below 80 dB down, a 16-bit master is
/// showing its noise floor.
const IMAGE_RANGE_DB: f32 = 80.0;
/// Frames transformed for the tuning pass. The estimate is an average over the
/// track and settles long before this many.
const TUNING_FRAMES: usize = 2000;

pub struct Source {
    pub path: String,
    pub duration_seconds: f64,
    pub start_seconds: f64,
}

pub struct Outcome {
    pub report: AnalysisReport,
    pub spectrogram: Heatmap,
    /// Long-term average spectrum, as (hertz, decibels).
    pub average_spectrum: Vec<(f64, f64)>,
    /// The curve the tempo estimate was made from.
    pub broadband: Novelty,
}

pub fn run(audio: &Audio, options: &AnalysisOptions, source: Source) -> Result<Outcome> {
    let mut stft = Stft::new(options.window, options.hop);
    let frames = stft.frame_count(audio.samples.len());
    let frame_rate = stft.frame_rate(audio.sample_rate);
    // Two windows of the stability trace. Less than that and the answer rests on
    // one measurement with nothing to compare it against.
    if (frames as f64 / frame_rate) < 30.0 {
        bail!(
            "need at least 30 seconds of audio, got {:.1}",
            frames as f64 / frame_rate
        );
    }

    let tuning_cents = match options.tuning_cents {
        Some(override_value) => override_value,
        None => {
            let mut estimator = TuningEstimator::default();
            let step = (frames / TUNING_FRAMES).max(1);
            stft.for_each_frame_stepped(&audio.samples, step, |_, magnitudes| {
                estimator.push(magnitudes, audio.sample_rate, options.window);
            });
            estimator.cents()
        }
    };

    let nyquist = audio.sample_rate as f64 / 2.0;
    let onset_bands = LogBands::new(
        options.onset_bands,
        30.0,
        16_000.0f64.min(nyquist),
        audio.sample_rate,
        options.window,
    );
    let image_bands = LogBands::new(
        IMAGE_HEIGHT,
        20.0,
        20_000.0f64.min(nyquist),
        audio.sample_rate,
        options.window,
    );
    let chroma_mapper = ChromaMapper::new(audio.sample_rate, options.window, tuning_cents);

    let mut flux = FluxAccumulator::new(onset_bands.len(), options.compression);
    let mut band_energies = vec![0.0f32; onset_bands.len()];
    let mut image_energies = vec![0.0f32; image_bands.len()];
    let mut chroma = [0.0f64; 12];
    let mut spectrum_sum = vec![0.0f64; stft.bin_count()];

    let image_width = IMAGE_WIDTH.min(frames.max(1));
    let mut image = vec![0.0f64; image_width * IMAGE_HEIGHT];
    let mut column_counts = vec![0.0f64; image_width];

    stft.for_each_frame(&audio.samples, |frame, magnitudes| {
        onset_bands.energies(magnitudes, &mut band_energies);
        flux.push(&band_energies);

        chroma_mapper.accumulate(magnitudes, &mut chroma);
        for (slot, magnitude) in spectrum_sum.iter_mut().zip(magnitudes) {
            *slot += *magnitude as f64;
        }

        let column = (frame * image_width / frames.max(1)).min(image_width - 1);
        image_bands.energies(magnitudes, &mut image_energies);
        for (row, energy) in image_energies.iter().enumerate() {
            image[row * image_width + column] += *energy as f64;
        }
        column_counts[column] += 1.0;
    });

    let band_curves: Vec<Novelty> = flux
        .finish()
        .into_iter()
        .map(|curve| Novelty::from_flux(curve, frame_rate, options.local_mean))
        .collect();
    let broadband = Novelty::sum(&band_curves);
    // The bottom two bands reach to about 130 Hz, which is the kick and nothing
    // else in most club productions.
    let low_band = Novelty::sum(&band_curves[..2.min(band_curves.len())]);

    let settings = TempoSettings {
        min_bpm: options.min_bpm,
        max_bpm: options.max_bpm,
        resolution_bpm: options.bpm_resolution,
        pulses: options.pulses,
        penalty: options.comb_penalty,
        prior: options.tempo_prior.map(|centre_bpm| TempoPrior {
            centre_bpm,
            width_octaves: options.tempo_prior_width,
        }),
        floor: (options.metrical_floor > 0.0).then_some(tempo::MetricalFloor {
            bpm: options.metrical_floor,
            min_salience_ratio: options.metrical_floor_ratio,
        }),
        integer_snap_bpm: options.integer_snap,
    };
    let tempo_analysis = tempo::analyze(&broadband, &low_band, &settings);

    let bands = band_curves
        .iter()
        .enumerate()
        .map(|(index, curve)| {
            let (bpm, salience) = tempo::best_bpm(curve, &settings).unwrap_or((f64::NAN, 0.0));
            let (low_hz, high_hz) = onset_bands.range_hz(index);
            BandTempo {
                band: index,
                low_hz,
                high_hz,
                bpm,
                salience,
            }
        })
        .collect();

    let key = key_detect::analyze(chroma, tuning_cents, options.key_profile);

    let average_spectrum: Vec<(f64, f64)> = spectrum_sum
        .iter()
        .enumerate()
        .skip(1)
        .map(|(bin, sum)| {
            let hz = stft.bin_frequency(bin, audio.sample_rate);
            let amplitude = (sum / frames.max(1) as f64) as f32;
            (hz, spectral::to_db(amplitude) as f64)
        })
        .collect();

    let spectrum = summarise_spectrum(&average_spectrum);
    let spectrogram = build_spectrogram(
        image,
        &column_counts,
        image_width,
        &image_bands,
        source.start_seconds,
        frames as f64 / frame_rate,
    );

    let report = AnalysisReport {
        tool: "dubplate",
        version: env!("CARGO_PKG_VERSION"),
        source: SourceInfo {
            path: source.path,
            sample_rate: audio.sample_rate,
            channels: audio.source_channels,
            duration_seconds: source.duration_seconds,
            analysed_start_seconds: source.start_seconds,
            analysed_seconds: audio.duration_seconds(),
        },
        settings: AnalysisSettings {
            window_size: options.window,
            hop: options.hop,
            frame_rate,
            onset_bands: onset_bands.len(),
            flux_compression: options.compression,
            local_mean_seconds: options.local_mean,
        },
        tempo: tempo_analysis,
        key,
        bands,
        spectrum,
        waveforms: Waveforms {
            preview: waveform::preview(&audio.samples, audio.sample_rate),
            detail: waveform::detail(&audio.samples, audio.sample_rate),
        },
    };

    Ok(Outcome {
        report,
        spectrogram,
        average_spectrum,
        broadband,
    })
}

fn build_spectrogram(
    accumulated: Vec<f64>,
    column_counts: &[f64],
    width: usize,
    bands: &LogBands,
    start_seconds: f64,
    length_seconds: f64,
) -> Heatmap {
    let height = bands.len();
    let mut values = vec![0.0f32; width * height];
    let mut ceiling = f32::MIN;

    for row in 0..height {
        // Row 0 of the image is the top of the plot, which is the highest band.
        let band = height - 1 - row;
        for column in 0..width {
            let count = column_counts[column].max(1.0);
            let mean = accumulated[band * width + column] / count;
            let db = spectral::to_db(mean as f32);
            values[row * width + column] = db;
            ceiling = ceiling.max(db);
        }
    }

    let (low_hz, _) = bands.range_hz(0);
    let (_, high_hz) = bands.range_hz(height - 1);
    Heatmap {
        title: "Spectrogram".into(),
        width,
        height,
        values,
        floor_db: ceiling - IMAGE_RANGE_DB,
        ceiling_db: ceiling,
        time_range: (start_seconds, start_seconds + length_seconds),
        frequency_range: (low_hz, high_hz),
    }
}

fn summarise_spectrum(spectrum: &[(f64, f64)]) -> SpectrumSummary {
    // Back to linear power: the summary is about energy, and averaging decibels
    // weights a quiet bin as heavily as a loud one.
    let power: Vec<(f64, f64)> = spectrum
        .iter()
        .map(|&(hz, db)| (hz, 10f64.powf(db / 10.0)))
        .collect();
    let total: f64 = power.iter().map(|(_, p)| p).sum();
    let total = total.max(f64::MIN_POSITIVE);

    let peak_hz = spectrum
        .iter()
        .filter(|(hz, _)| *hz >= 20.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(hz, _)| *hz)
        .unwrap_or(0.0);
    let centroid_hz = power.iter().map(|(hz, p)| hz * p).sum::<f64>() / total;

    let mut cumulative = 0.0;
    let mut rolloff_95_hz = 0.0;
    for (hz, p) in &power {
        cumulative += p;
        if cumulative / total >= 0.95 {
            rolloff_95_hz = *hz;
            break;
        }
    }

    let ceiling = spectrum.iter().map(|(_, db)| *db).fold(f64::MIN, f64::max);
    let high_cutoff_hz = spectrum
        .iter()
        .rev()
        .find(|(_, db)| *db >= ceiling - 40.0)
        .map(|(hz, _)| *hz)
        .unwrap_or(0.0);

    let share = |low: f64, high: f64| {
        power
            .iter()
            .filter(|(hz, _)| *hz >= low && *hz < high)
            .map(|(_, p)| p)
            .sum::<f64>()
            / total
    };

    SpectrumSummary {
        peak_hz,
        centroid_hz,
        rolloff_95_hz,
        high_cutoff_hz,
        low_share: share(0.0, 200.0),
        mid_share: share(200.0, 4000.0),
        high_share: share(4000.0, f64::INFINITY),
    }
}
