//! Decoding and excerpting, plus the synthetic signals the tests measure against.
//!
//! Everything downstream works on one mono `f32` buffer at the file's own sample
//! rate. Nothing here resamples: a tempo expressed in beats per minute is
//! independent of the rate, and the frequency axis is reported in hertz, so
//! resampling would only add a filter whose ringing shows up in the spectrogram
//! as an artefact of this tool rather than of the track.

pub mod synth;

use std::error::Error;
use std::fmt;
use std::path::Path;

#[cfg(feature = "compressed")]
mod compressed;

pub mod mp3;

/// Cut the head off a WAV without decoding it.
///
/// Byte surgery on the container rather than a decode and a re-encode: the
/// samples that survive are the bytes that were there, so the channel count,
/// the bit depth and the sample values all come through untouched. Decoding
/// would hand back the mono downmix this crate analyses, and a stereo track
/// written back from that has lost the width a mix is made of.
///
/// Returns the whole file when `start_seconds` is zero or negative.
pub fn trim_wav(wav: &[u8], start_seconds: f64) -> Result<Vec<u8>, DecodeError> {
    let layout = WavLayout::read(wav)?;
    let frames = (start_seconds.max(0.0) * f64::from(layout.sample_rate)).round() as usize;
    let skip = (frames * layout.bytes_per_frame()).min(layout.data_len);
    let kept = &wav[layout.data_at + skip..layout.data_at + layout.data_len];

    // A fresh header rather than a patched one: the source may carry LIST or
    // INFO chunks whose offsets a shortened data chunk would invalidate.
    let mut out = Vec::with_capacity(44 + kept.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + kept.len()) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&layout.channels.to_le_bytes());
    out.extend_from_slice(&layout.sample_rate.to_le_bytes());
    let bytes_per_second = layout.sample_rate * layout.bytes_per_frame() as u32;
    out.extend_from_slice(&bytes_per_second.to_le_bytes());
    out.extend_from_slice(&(layout.bytes_per_frame() as u16).to_le_bytes());
    out.extend_from_slice(&layout.bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(kept.len() as u32).to_le_bytes());
    out.extend_from_slice(kept);
    Ok(out)
}

/// Where the samples are in a WAV and what shape they are.
struct WavLayout {
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
    data_at: usize,
    data_len: usize,
}

impl WavLayout {
    fn bytes_per_frame(&self) -> usize {
        usize::from(self.channels) * usize::from(self.bits_per_sample).div_ceil(8)
    }

    /// Walk the RIFF chunks. The sample tracks carry a LIST/INFO chunk between
    /// `fmt ` and `data`, which is why this walks rather than assuming 44.
    fn read(wav: &[u8]) -> Result<Self, DecodeError> {
        let malformed = || DecodeError::Container("not a RIFF/WAVE file".into());
        if wav.len() < 12 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
            return Err(malformed());
        }
        let word = |at: usize| -> Result<u32, DecodeError> {
            wav.get(at..at + 4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                .ok_or_else(malformed)
        };

        let (mut channels, mut sample_rate, mut bits_per_sample) = (0u16, 0u32, 0u16);
        let mut at = 12;
        while at + 8 <= wav.len() {
            let kind = &wav[at..at + 4];
            let length = word(at + 4)? as usize;
            let body = at + 8;
            match kind {
                b"fmt " if body + 16 <= wav.len() => {
                    channels = u16::from_le_bytes(wav[body + 2..body + 4].try_into().unwrap());
                    sample_rate = word(body + 4)?;
                    bits_per_sample =
                        u16::from_le_bytes(wav[body + 14..body + 16].try_into().unwrap());
                }
                b"data" => {
                    if channels == 0 || sample_rate == 0 || bits_per_sample == 0 {
                        return Err(DecodeError::Container(
                            "the data chunk came before the fmt chunk".into(),
                        ));
                    }
                    return Ok(WavLayout {
                        channels,
                        sample_rate,
                        bits_per_sample,
                        data_at: body,
                        // Clamped to what is actually there: a truncated
                        // download states a length its file does not carry.
                        data_len: length.min(wav.len().saturating_sub(body)),
                    });
                }
                _ => {}
            }
            // Chunks are padded to an even length, and the pad byte is not
            // counted in the length field.
            at = body + length + (length & 1);
        }
        Err(malformed())
    }
}

/// A decoded track: mono samples in `[-1.0, 1.0]` at `sample_rate`.
#[derive(Clone, Debug)]
pub struct Audio {
    pub sample_rate: u32,
    /// Channel count of the file, kept because a mono source and a downmixed
    /// stereo source behave differently in the stereo-wide parts of a mix.
    pub source_channels: u16,
    pub samples: Vec<f32>,
}

#[derive(Debug)]
pub enum DecodeError {
    Wav(hound::Error),
    /// Anything the compressed-format decoder refused, as it described it.
    /// A string rather than the crate's own error, so the type does not change
    /// shape with a feature.
    Container(String),
    Empty,
    /// The requested excerpt starts at or past the end of the file.
    ExcerptOutOfRange {
        start: f64,
        duration: f64,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Wav(e) => write!(f, "cannot read WAV: {e}"),
            DecodeError::Container(e) => write!(f, "cannot decode: {e}"),
            DecodeError::Empty => write!(f, "file decodes to zero samples"),
            DecodeError::ExcerptOutOfRange { start, duration } => write!(
                f,
                "excerpt starts at {start:.3}s but the track is {duration:.3}s long"
            ),
        }
    }
}

impl Error for DecodeError {}

impl From<hound::Error> for DecodeError {
    fn from(e: hound::Error) -> Self {
        DecodeError::Wav(e)
    }
}

impl Audio {
    /// Decode a WAV file and downmix to mono by averaging channels.
    ///
    /// The container is walked chunk by chunk rather than by skipping a fixed
    /// 44-byte header: the sample tracks carry a `LIST`/`INFO` block of track
    /// metadata between `fmt ` and `data`, and a fixed-offset reader lands in the
    /// middle of it and reads the artist name as audio.
    pub fn from_wav(path: &Path) -> Result<Self, DecodeError> {
        Self::decode(hound::WavReader::open(path)?)
    }

    /// Decode a WAV already in memory.
    ///
    /// The same decoder as `from_wav`, for callers that never had a path: a
    /// browser is handed an `ArrayBuffer`, and the archive pipeline reads a zip
    /// entry. `from_wav` is this plus opening the file.
    pub fn from_wav_bytes(wav: &[u8]) -> Result<Self, DecodeError> {
        Self::decode(hound::WavReader::new(std::io::Cursor::new(wav))?)
    }

    fn decode<R: std::io::Read>(mut reader: hound::WavReader<R>) -> Result<Self, DecodeError> {
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;

        let interleaved: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
            hound::SampleFormat::Int => {
                // Full-scale for a signed integer of this width. 24-bit files are
                // read as i32 by hound with the sample already sign-extended.
                let scale = 1.0f32 / (1i64 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 * scale))
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        if interleaved.is_empty() {
            return Err(DecodeError::Empty);
        }

        let samples = if channels == 1 {
            interleaved
        } else {
            interleaved
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                .collect()
        };

        Ok(Audio {
            sample_rate: spec.sample_rate,
            source_channels: spec.channels,
            samples,
        })
    }

    pub fn duration_seconds(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }

    /// Seconds of near-silence before the track starts.
    ///
    /// A shop's WAV often opens with a second or more of digital black, and a
    /// player told to auto-cue lands in it: the waveform shows a flat run, the
    /// first beat is nowhere near the start of the file, and a grid laid from
    /// sample zero carries that offset into every bar.
    ///
    /// Measured against the track's own peak rather than an absolute floor, so
    /// a quiet master is not read as one long lead-in. The threshold is 60 dB
    /// down, which is below the noise floor of anything mastered and above the
    /// dither of a silent passage.
    pub fn lead_in_seconds(&self) -> f64 {
        const WINDOW_SECONDS: f64 = 0.01;
        const BELOW_PEAK_DB: f64 = 60.0;

        let window = ((self.sample_rate as f64 * WINDOW_SECONDS) as usize).max(1);
        let levels: Vec<f32> = self
            .samples
            .chunks(window)
            .map(|chunk| {
                chunk
                    .iter()
                    .fold(0.0f32, |loudest, sample| loudest.max(sample.abs()))
            })
            .collect();

        let peak = levels.iter().copied().fold(0.0f32, f32::max);
        if peak <= 0.0 {
            return 0.0;
        }
        let threshold = peak * 10.0f32.powf(-(BELOW_PEAK_DB as f32) / 20.0);
        let first = levels.iter().position(|level| *level >= threshold);
        match first {
            Some(0) | None => 0.0,
            Some(index) => index as f64 * window as f64 / self.sample_rate as f64,
        }
    }

    /// A copy of `duration` seconds starting at `start`.
    ///
    /// Analysing an excerpt is the first move when a full-track estimate looks
    /// wrong: an intro that is half-time against the body of the track produces a
    /// confident answer for a section nobody dances to.
    pub fn excerpt(&self, start: f64, duration: Option<f64>) -> Result<Audio, DecodeError> {
        let first = (start.max(0.0) * self.sample_rate as f64).round() as usize;
        if first >= self.samples.len() {
            return Err(DecodeError::ExcerptOutOfRange {
                start,
                duration: self.duration_seconds(),
            });
        }
        let last = match duration {
            Some(d) => (first + (d.max(0.0) * self.sample_rate as f64).round() as usize)
                .min(self.samples.len()),
            None => self.samples.len(),
        };
        Ok(Audio {
            sample_rate: self.sample_rate,
            source_channels: self.source_channels,
            samples: self.samples[first..last].to_vec(),
        })
    }
}
