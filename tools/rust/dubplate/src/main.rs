//! The command line: one analysis pass, one output directory, one summary.

mod export;
mod rename;
mod summary;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use key_detect::Profile;
use pipeline::AnalysisOptions;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "dubplate",
    version,
    about = "Tempo and key analysis that shows its working"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyse a WAV file and write a report directory.
    Analyze(AnalyzeArgs),
    /// Name files after the tempo and key measured in them.
    ///
    /// Writes links or copies under a directory of your choosing; the files
    /// given are never touched.
    Rename(RenameArgs),
    /// Write a player-readable device from analysed tracks.
    ///
    /// Reads the audio and the reports written by `analyze`, and writes the
    /// databases a Pioneer or Denon player browses the device through.
    Export(ExportArgs),
    /// Analyse a synthesised pulse train at a known tempo.
    ///
    /// Every stage runs, so a run that misses a tempo it generated itself points
    /// at the tool rather than at the track.
    Selftest(SelftestArgs),
}

/// The analysis flags, which are `pipeline::AnalysisOptions` with clap on top.
///
/// Every default reads out of that struct rather than being repeated here, so
/// `--help` and a browser filling the same options in cannot disagree about
/// what the tool does when told nothing.
#[derive(Args, Clone)]
pub struct AnalysisArgs {
    /// STFT window, in samples. Larger resolves frequency, smaller resolves
    /// time; onsets need time and key needs frequency, which is the tension the
    /// default sits in the middle of.
    #[arg(long, default_value_t = AnalysisOptions::default().window)]
    pub window: usize,

    /// STFT hop, in samples. Sets the frame rate of every novelty curve, and so
    /// the finest tempo difference that can be resolved.
    #[arg(long, default_value_t = AnalysisOptions::default().hop)]
    pub hop: usize,

    /// Frequency bands the onset detector splits the spectrum into.
    #[arg(long, default_value_t = AnalysisOptions::default().onset_bands)]
    pub onset_bands: usize,

    /// Gamma of the logarithmic compression applied before the flux.
    #[arg(long, default_value_t = AnalysisOptions::default().compression)]
    pub compression: f32,

    /// Width of the moving average subtracted from the flux, in seconds.
    #[arg(long, default_value_t = AnalysisOptions::default().local_mean)]
    pub local_mean: f64,

    /// Slowest tempo searched. Wide enough that an octave error stays inside
    /// the range and visible, rather than being clipped out of it.
    #[arg(long, default_value_t = AnalysisOptions::default().min_bpm)]
    pub min_bpm: f64,

    /// Fastest tempo searched.
    #[arg(long, default_value_t = AnalysisOptions::default().max_bpm)]
    pub max_bpm: f64,

    /// Spacing of the tempo grid, in BPM. The answer is refined between grid
    /// points, so this sets the cost of the search rather than the precision.
    #[arg(long, default_value_t = AnalysisOptions::default().bpm_resolution)]
    pub bpm_resolution: f64,

    /// Comb teeth used by the tempo salience.
    #[arg(long, default_value_t = AnalysisOptions::default().pulses)]
    pub pulses: usize,

    /// Weight of the penalty applied between comb teeth, in [0, 1]. At 0 the
    /// salience is a plain harmonic sum, which favours slow metrical levels; at
    /// 1 it argues hardest against them, and against any tempo whose offbeats
    /// carry weight.
    #[arg(long, default_value_t = AnalysisOptions::default().comb_penalty)]
    pub comb_penalty: f64,

    /// Slowest metrical level the answer may be reported at, in BPM. Set to 0
    /// to report whatever the salience curve says, subharmonic and all.
    #[arg(long, default_value_t = AnalysisOptions::default().metrical_floor)]
    pub metrical_floor: f64,

    /// How strong the doubled candidate must be, relative to the original, for
    /// the metrical floor to double it.
    #[arg(long, default_value_t = AnalysisOptions::default().metrical_floor_ratio)]
    pub metrical_floor_ratio: f64,

    /// Largest gap, in BPM, the answer may be moved by to reach a whole number.
    /// Produced music is written on integers, so the default closes this tool's
    /// own error and nothing wider. Set to 0.5 to round whatever was measured,
    /// or to 0 to report it as measured.
    #[arg(long, default_value_t = AnalysisOptions::default().integer_snap)]
    pub integer_snap: f64,

    /// Centre of a log-normal tempo prior, in BPM. Off unless given, because a
    /// prior is how a detector reports the tempo it expected.
    #[arg(long)]
    pub tempo_prior: Option<f64>,

    /// Width of the tempo prior, in octaves.
    #[arg(long, default_value_t = AnalysisOptions::default().tempo_prior_width)]
    pub tempo_prior_width: f64,

    /// Key profile to correlate the chroma against.
    #[arg(long, default_value_t = AnalysisOptions::default().key_profile)]
    pub key_profile: Profile,

    /// Override the measured tuning offset, in cents from A = 440 Hz.
    #[arg(long)]
    pub tuning_cents: Option<f64>,
}

impl From<&AnalysisArgs> for AnalysisOptions {
    fn from(args: &AnalysisArgs) -> Self {
        AnalysisOptions {
            window: args.window,
            hop: args.hop,
            onset_bands: args.onset_bands,
            compression: args.compression,
            local_mean: args.local_mean,
            min_bpm: args.min_bpm,
            max_bpm: args.max_bpm,
            bpm_resolution: args.bpm_resolution,
            pulses: args.pulses,
            comb_penalty: args.comb_penalty,
            metrical_floor: args.metrical_floor,
            metrical_floor_ratio: args.metrical_floor_ratio,
            integer_snap: args.integer_snap,
            tempo_prior: args.tempo_prior,
            tempo_prior_width: args.tempo_prior_width,
            key_profile: args.key_profile,
            tuning_cents: args.tuning_cents,
        }
    }
}

#[derive(Args)]
struct AnalyzeArgs {
    /// WAV file to analyse.
    file: PathBuf,

    /// Directory for report.json, report.html and the figures.
    #[arg(short, long)]
    out: Option<PathBuf>,

    /// Start of the excerpt to analyse, in seconds.
    #[arg(long, default_value_t = 0.0)]
    start: f64,

    /// Length of the excerpt to analyse, in seconds.
    #[arg(long)]
    duration: Option<f64>,

    /// Start of the window the novelty figure covers, in seconds from the start
    /// of the file. Defaults to a quarter of the way into what was analysed,
    /// which for most tracks is past the intro.
    #[arg(long)]
    plot_start: Option<f64>,

    /// Length of the window the novelty figure covers, in seconds.
    #[arg(long, default_value_t = 12.0)]
    plot_window: f64,

    /// Write report.json only.
    #[arg(long)]
    no_figures: bool,

    /// Print the report JSON to stdout as well.
    #[arg(long)]
    print_json: bool,

    #[command(flatten)]
    options: AnalysisArgs,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum RenameMode {
    /// Print what the files would be called and change nothing.
    Print,
    /// Write symbolic links pointing at the originals.
    Symlink,
    /// Write copies, leaving the originals in place.
    Copy,
}

#[derive(Args)]
struct RenameArgs {
    /// WAV files, or directories of them. Directories are read one level deep.
    paths: Vec<PathBuf>,

    /// Where the links or copies are written. Required unless printing.
    #[arg(short, long)]
    out: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = RenameMode::Print)]
    mode: RenameMode,

    /// Also write a report directory per track, under this directory.
    #[arg(long)]
    reports: Option<PathBuf>,

    #[command(flatten)]
    options: AnalysisArgs,
}

#[derive(Args)]
pub struct ExportArgs {
    /// Directory of audio files, read one level deep.
    #[arg(long)]
    pub audio: PathBuf,

    /// Directory of `<stem>.json` reports, one per audio file.
    #[arg(long)]
    pub reports: PathBuf,

    /// Device root to write into.
    #[arg(short, long)]
    pub out: PathBuf,

    /// Which databases to write.
    #[arg(long, value_enum, default_value_t = export::Target::Rekordbox)]
    pub target: export::Target,

    /// What to do with the audio files. The default writes databases only,
    /// because the pipeline that builds a disk image places the audio itself.
    #[arg(long, value_enum, default_value_t = export::AudioMode::None)]
    pub audio_mode: export::AudioMode,

    /// Name of the playlist holding every track.
    #[arg(long, default_value = "All tracks")]
    pub playlist: String,

    /// Date recorded against every track, as YYYY-MM-DD. Defaults to
    /// SOURCE_DATE_EPOCH when set, and to today otherwise.
    #[arg(long)]
    pub date: Option<String>,
}

#[derive(Args)]
struct SelftestArgs {
    /// Tempo of the generated pulse train.
    #[arg(long, default_value_t = 174.0)]
    bpm: f64,

    /// Length of the generated signal, in seconds.
    #[arg(long, default_value_t = 60.0)]
    seconds: f64,

    #[command(flatten)]
    options: AnalysisArgs,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Analyze(args) => analyze(args),
        Command::Rename(args) => rename_files(args),
        Command::Export(args) => export::run(args),
        Command::Selftest(args) => selftest(args),
    }
}

fn analyze(args: AnalyzeArgs) -> Result<()> {
    let decoded = audio::Audio::from_wav(&args.file)
        .with_context(|| format!("decoding {}", args.file.display()))?;
    let duration = decoded.duration_seconds();
    let excerpt = decoded.excerpt(args.start, args.duration)?;

    let outcome = pipeline::run(
        &excerpt,
        &AnalysisOptions::from(&args.options),
        pipeline::Source {
            path: args.file.display().to_string(),
            duration_seconds: duration,
            start_seconds: args.start,
        },
    )?;

    let out = args.out.unwrap_or_else(|| {
        PathBuf::from("analysis").join(
            args.file
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "track".into()),
        )
    });
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;

    let json = outcome.report.to_json()?;
    std::fs::write(out.join("report.json"), &json)?;
    if args.print_json {
        println!("{json}");
    }

    if !args.no_figures {
        // Both the novelty axis and the beat times are absolute seconds in the
        // file, so this window is too. A window measured from zero on an
        // excerpt that starts at 96 s contains none of its beats, and the
        // figure then draws a grid line for none of them.
        let source = &outcome.report.source;
        let plot_start = args
            .plot_start
            .unwrap_or(source.analysed_start_seconds + source.analysed_seconds * 0.25);
        write_artefacts(
            &out,
            pipeline::figures(&outcome, plot_start, args.plot_window),
        )?;
    }

    // The summary goes to stderr when the JSON is on stdout, so a pipe into jq
    // gets JSON and the person running it still sees the findings.
    if args.print_json {
        summary::print(
            &outcome.report,
            &out,
            args.no_figures,
            &mut std::io::stderr(),
        );
    } else {
        summary::print(
            &outcome.report,
            &out,
            args.no_figures,
            &mut std::io::stdout(),
        );
    }
    Ok(())
}

fn rename_files(args: RenameArgs) -> Result<()> {
    if args.paths.is_empty() {
        anyhow::bail!("give at least one file or directory to name");
    }
    if args.mode != RenameMode::Print && args.out.is_none() {
        anyhow::bail!("--out names the directory the links or copies go into");
    }
    if let Some(out) = &args.out {
        std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    }

    let mut files = Vec::new();
    for path in &args.paths {
        files.extend(rename::wav_files(path)?);
    }

    for file in files {
        let decoded = audio::Audio::from_wav(&file)
            .with_context(|| format!("decoding {}", file.display()))?;
        let duration = decoded.duration_seconds();
        let outcome = pipeline::run(
            &decoded,
            &AnalysisOptions::from(&args.options),
            pipeline::Source {
                path: file.display().to_string(),
                duration_seconds: duration,
                start_seconds: 0.0,
            },
        )?;
        let report = &outcome.report;
        let name = rename::target_name(&file, report.tempo.bpm, &report.key.camelot)?;

        if let Some(reports) = &args.reports {
            let directory = reports.join(
                file.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "track".into()),
            );
            std::fs::create_dir_all(&directory)?;
            std::fs::write(directory.join("report.json"), report.to_json()?)?;
        }

        match (args.mode, &args.out) {
            (RenameMode::Print, _) => println!("{name}"),
            (RenameMode::Symlink, Some(out)) => {
                let link = out.join(&name);
                // Absolute, so the link still resolves from wherever the output
                // directory is later read.
                let target = std::fs::canonicalize(&file)?;
                let _ = std::fs::remove_file(&link);
                std::os::unix::fs::symlink(&target, &link)
                    .with_context(|| format!("linking {}", link.display()))?;
                println!("{name}");
            }
            (RenameMode::Copy, Some(out)) => {
                let destination = out.join(&name);
                std::fs::copy(&file, &destination)
                    .with_context(|| format!("copying to {}", destination.display()))?;
                println!("{name}");
            }
            (_, None) => unreachable!("checked above"),
        }

        // The findings belong on stderr: stdout is the list of names, which is
        // what a script downstream reads.
        for finding in report.diagnostics() {
            eprintln!("{name}: {} {}", finding.code, finding.message);
        }
    }
    Ok(())
}

/// Write what `pipeline::figures` drew into a directory.
///
/// The names are the ones the HTML references, so writing them side by side is
/// the whole of what turning artefacts into a readable report directory takes.
fn write_artefacts(out: &std::path::Path, artefacts: pipeline::Artefacts) -> Result<()> {
    for (name, bytes) in artefacts.binary {
        std::fs::write(out.join(&name), bytes).with_context(|| format!("writing {name}"))?;
    }
    for (name, text) in artefacts.text {
        std::fs::write(out.join(&name), text).with_context(|| format!("writing {name}"))?;
    }
    Ok(())
}

fn selftest(args: SelftestArgs) -> Result<()> {
    let generated = audio::synth::click_train(args.bpm, args.seconds, 44100);
    let outcome = pipeline::run(
        &generated,
        &AnalysisOptions::from(&args.options),
        pipeline::Source {
            path: format!("synthetic pulse train at {:.2} BPM", args.bpm),
            duration_seconds: args.seconds,
            start_seconds: 0.0,
        },
    )?;

    // The measurement, not the reported answer: a tempo snapped to a whole
    // number would report zero error against a generated 174 BPM and hide the
    // thing this command exists to measure.
    let measured = outcome.report.tempo.bpm_measured;
    let error = (measured - args.bpm).abs();
    println!(
        "generated {:.2} BPM, measured {:.2} BPM, error {:.3} BPM ({:.2}%)",
        args.bpm,
        measured,
        error,
        error / args.bpm * 100.0
    );
    for finding in outcome.report.diagnostics() {
        println!("  {} {}", finding.code, finding.message);
    }

    // A pulse train is the easiest possible input, so the bar is tight: a
    // failure here is a defect in this repo, not an ambiguous track.
    if error / args.bpm > 0.005 {
        anyhow::bail!("tempo stage missed a synthetic pulse train by more than 0.5%");
    }
    Ok(())
}
