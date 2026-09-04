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
