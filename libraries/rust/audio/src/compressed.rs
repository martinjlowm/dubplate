//! Decoding the formats a shop actually sends you, in one process.
//!
//! The analyser reads WAV, and the Nix pipeline gets it WAV by running `flac`
//! and `lame` over an archive first: one decoder per format, each of them the
//! reference implementation for it. A browser has no processes to run, so a zip
//! of Beatport downloads arrives as AIFF, MP3 and FLAC with nothing in front of
//! it.
//!
//! Symphonia is that front, in Rust. It sits behind a feature rather than being
//! always on, because turning it on for the CLI would quietly replace two
//! reference decoders with a third implementation, and that is a decision about
//! the measurements rather than about the build.
//!
//! Nothing is resampled here either, which is the reason this exists at all
//! rather than the browser's own `decodeAudioData`: that resamples to the audio
//! context's rate on the way out, so a measurement in a tab would not be the
//! measurement the CLI makes on the same file.

use crate::{Audio, DecodeError};
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

impl Audio {
    /// Decode WAV, AIFF, FLAC or MP3 already in memory, downmixed to mono.
    ///
    /// `file_name` is a hint only. Symphonia probes the bytes; the extension
    /// breaks a tie sooner than the probe does, and a wrong one costs nothing.
    ///
    /// The bytes are taken by value because symphonia reads through a
    /// `MediaSource`, which is `Send + Sync` and so cannot borrow. Handed a
    /// slice, this copied the whole file to satisfy that, and a browser
    /// analysing a ninety-megabyte track paid for it twice: once crossing the
    /// boundary and once here.
    pub fn from_encoded(bytes: Vec<u8>, file_name: &str) -> Result<Self, DecodeError> {
        let mut hint = Hint::new();
        if let Some((_, extension)) = file_name.rsplit_once('.') {
            hint.with_extension(extension);
        }

        let stream =
            MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
        let mut format = symphonia::default::get_probe()
            .probe(
                &hint,
                stream,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| DecodeError::Container(e.to_string()))?;

        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| DecodeError::Container("the file holds no audio track".into()))?;
        let track_id = track.id;
        let parameters = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or_else(|| DecodeError::Container("the audio track names no codec".into()))?
            .clone();

        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&parameters, &AudioDecoderOptions::default())
            .map_err(|e| DecodeError::Container(e.to_string()))?;

        let mut sample_rate = parameters.sample_rate.unwrap_or(0);
        let mut channels = 0usize;
        let mut samples: Vec<f32> = Vec::new();
        let mut interleaved: Vec<f32> = Vec::new();

        while let Some(packet) = format
            .next_packet()
            .map_err(|e| DecodeError::Container(e.to_string()))?
        {
            if packet.track_id != track_id {
                continue;
            }
            let decoded = match decoder.decode(&packet) {
                Ok(decoded) => decoded,
                // A torn packet is one that is skipped, not one that ends the
                // track: a shop's MP3 with a damaged frame still has a tempo.
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(e) => return Err(DecodeError::Container(e.to_string())),
            };
            append_mono(&decoded, &mut interleaved, &mut samples);
            sample_rate = decoded.spec().rate();
            channels = decoded.spec().channels().count();
        }

        if samples.is_empty() || sample_rate == 0 {
            return Err(DecodeError::Empty);
        }

        Ok(Audio {
            sample_rate,
            source_channels: channels.max(1) as u16,
            samples,
        })
    }
}

/// Average one decoded buffer's channels onto the end of `samples`.
///
/// `scratch` is reused across packets: a four-minute track is a few thousand of
/// them, and a fresh allocation each time is the whole cost of the decode.
fn append_mono(
    decoded: &GenericAudioBufferRef<'_>,
    scratch: &mut Vec<f32>,
    samples: &mut Vec<f32>,
) {
    decoded.copy_to_vec_interleaved(scratch);
    let channels = decoded.spec().channels().count();
    match channels {
        0 | 1 => samples.extend_from_slice(scratch),
        count => samples.extend(
            scratch
                .chunks_exact(count)
                .map(|frame| frame.iter().sum::<f32>() / count as f32),
        ),
    }
}
