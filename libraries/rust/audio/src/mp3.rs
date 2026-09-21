//! The seek index a player needs to scrub a variable-bitrate MP3.
//!
//! Frame headers only. Nothing here decodes audio: the analyser reads WAV and
//! the archive pipeline gets it WAV by running `lame` and `flac` first, and that
//! stays true. What this walks is the container, the way [`crate::trim_wav`]
//! walks RIFF chunks, because where a frame starts is a property of the file
//! rather than of the music in it.
//!
//! A constant-bitrate file needs no index: time and byte offset are the same
//! number twice, and rekordbox writes zeros for WAV. A VBR MP3 is the opposite,
//! and an `ANLZ0000.DAT` whose `PVBR` section is 401 zeros tells a player that
//! every point in the track is at byte zero.

/// Byte offsets the seek index holds, which is also the column count of the
/// preview waveform the index is read alongside.
pub const SEEK_POINTS: usize = 400;

/// Where the audio sits in an MP3, in the two units a player needs.
pub struct SeekTable {
    /// One byte offset per [`SEEK_POINTS`] slice of the track, measured from
    /// the first frame that carries audio.
    pub offsets: Vec<u32>,
    /// Samples in the file, not counting the encoder's own info frame.
    pub total_samples: u32,
}

impl SeekTable {
    /// The 401 big-endian words a `PVBR` section carries: the offsets, then the
    /// sample count.
    pub fn to_words(&self) -> Vec<u32> {
        let mut words = self.offsets.clone();
        words.resize(SEEK_POINTS, 0);
        words.push(self.total_samples);
        words
    }
}

/// One frame of MPEG audio.
struct Frame {
    /// Offset from the start of the file.
    at: usize,
    length: usize,
    samples: u32,
}

/// Samples per frame, by MPEG version. Layer III only, which is what an MP3 is.
const SAMPLES_V1: u32 = 1152;
const SAMPLES_V2: u32 = 576;

const BITRATES_V1: [u32; 16] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
];
const BITRATES_V2: [u32; 16] = [
    0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
];
const RATES_V1: [u32; 3] = [44100, 48000, 32000];
const RATES_V2: [u32; 3] = [22050, 24000, 16000];
const RATES_V25: [u32; 3] = [11025, 12000, 8000];

/// Build the seek index for an MP3.
///
/// Returns `None` for anything that is not a layer III file with frames in it,
/// which is what a caller writes zeros for.
pub fn seek_table(bytes: &[u8]) -> Option<SeekTable> {
    let frames = frames(bytes);
    // The first frame of a `lame` encode carries the Xing or Info header and no
    // audio. A player counts the file without it, which is how the sample count
    // here lands on the one an XDJ-RX3 wrote for the same file.
    let audio = if frames.len() > 1 && is_info_frame(bytes, &frames[0]) {
        &frames[1..]
    } else {
        &frames[..]
    };
    let first = audio.first()?;
    let base = first.at;

    // Where each frame starts, and how many samples came before it.
    let mut starts = Vec::with_capacity(audio.len());
    let mut total_samples: u32 = 0;
    for frame in audio {
        starts.push((total_samples, (frame.at - base) as u32));
        total_samples = total_samples.checked_add(frame.samples)?;
    }
    if total_samples == 0 {
        return None;
    }

    // The frame a player lands on when it seeks to the middle of each slice.
    // The midpoint rather than the edge, and the frame at or after it rather
    // than the one containing it: 351 of the 401 words an XDJ-RX3 wrote for one
    // of this tool's own files come out identical that way, and the rest land
    // one frame, 24 ms, either side.
    let offsets = (0..SEEK_POINTS)
        .map(|slice| {
            let midpoint =
                (2 * slice as u64 + 1) * u64::from(total_samples) / (2 * SEEK_POINTS as u64);
            let at = starts.partition_point(|(sample, _)| u64::from(*sample) < midpoint);
            starts
                .get(at)
                .or_else(|| starts.last())
                .map_or(0, |(_, offset)| *offset)
        })
        .collect();

    Some(SeekTable {
        offsets,
        total_samples,
    })
}

/// Every frame header in the file, in order.
fn frames(bytes: &[u8]) -> Vec<Frame> {
    let mut at = skip_id3(bytes);
    let mut frames = Vec::new();
    while at + 4 <= bytes.len() {
        match parse_header(&bytes[at..]) {
            Some((length, samples)) if at + length <= bytes.len() => {
                frames.push(Frame {
                    at,
                    length,
                    samples,
                });
                at += length;
            }
            // Not a frame here, or one that runs off the end. Either way the
            // next sync word is what matters, and tags and junk sit between
            // frames often enough that giving up on the first miss loses most
            // of the file.
            _ => at += 1,
        }
    }
    frames
}

/// Length and sample count of the frame starting here.
fn parse_header(bytes: &[u8]) -> Option<(usize, u32)> {
    let header: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    if header[0] != 0xff || header[1] & 0xe0 != 0xe0 {
        return None;
    }
    let version = (header[1] >> 3) & 0x03;
    let layer = (header[1] >> 1) & 0x03;
    // Version 1 is reserved, and layer 1 in this field is layer III.
    if version == 1 || layer != 1 {
        return None;
    }
    let bitrate_index = (header[2] >> 4) as usize & 0x0f;
    let rate_index = (header[2] >> 2) as usize & 0x03;
    let padding = usize::from((header[2] >> 1) & 0x01);
    if bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        return None;
    }

    let (kbps, rate, samples) = match version {
        3 => (BITRATES_V1[bitrate_index], RATES_V1[rate_index], SAMPLES_V1),
        2 => (BITRATES_V2[bitrate_index], RATES_V2[rate_index], SAMPLES_V2),
        _ => (
            BITRATES_V2[bitrate_index],
            RATES_V25[rate_index],
            SAMPLES_V2,
        ),
    };
    if kbps == 0 || rate == 0 {
        return None;
    }

    let length = (samples / 8 * kbps * 1000 / rate) as usize + padding;
    (length >= 4).then_some((length, samples))
}

/// Whether this frame is the encoder's own header rather than audio.
///
/// `lame` writes a Xing or Info tag in the first frame, which carries the
/// track's own duration and no samples anybody hears.
fn is_info_frame(bytes: &[u8], frame: &Frame) -> bool {
    let body = &bytes[frame.at..(frame.at + frame.length).min(bytes.len())];
    body.windows(4)
        .take(48)
        .any(|window| window == b"Xing" || window == b"Info")
}

/// Bytes of ID3v2 tag at the head of the file.
///
/// A shop's MP3 carries the cover art here, which on this library runs to
/// 1.7 MB on one track: starting the frame walk at byte zero would spend the
/// whole file failing to find a sync word inside a PNG.
fn skip_id3(bytes: &[u8]) -> usize {
    if bytes.len() < 10 || &bytes[..3] != b"ID3" {
        return 0;
    }
    // Four seven-bit bytes, so a length can never contain a sync word.
    let size = bytes[6..10].iter().fold(0usize, |total, byte| {
        (total << 7) | usize::from(byte & 0x7f)
    });
    let footer = if bytes[5] & 0x10 != 0 { 10 } else { 0 };
    (10 + size + footer).min(bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of `count` frames at a fixed bitrate, optionally behind an ID3
    /// tag. Constant bitrate, so every offset is predictable arithmetic and a
    /// walk that loses a frame shows up as the wrong number rather than as a
    /// vague drift.
    fn synthetic(count: usize, tag_bytes: usize) -> Vec<u8> {
        // 128 kbps, 44100 Hz, layer III, MPEG-1: 417 bytes and 1152 samples.
        let header = [0xffu8, 0xfb, 0x90, 0x00];
        let frame_length = 417;

        let mut out = Vec::new();
        if tag_bytes > 0 {
            out.extend_from_slice(b"ID3\x03\x00\x00");
            let size = tag_bytes - 10;
            out.push(((size >> 21) & 0x7f) as u8);
            out.push(((size >> 14) & 0x7f) as u8);
            out.push(((size >> 7) & 0x7f) as u8);
            out.push((size & 0x7f) as u8);
            out.resize(tag_bytes, 0);
        }
        for _ in 0..count {
            out.extend_from_slice(&header);
            out.resize(out.len() + frame_length - 4, 0x55);
        }
        out
    }

    #[test]
    fn a_constant_bitrate_file_indexes_to_even_offsets() {
        let bytes = synthetic(800, 0);
        let table = seek_table(&bytes).expect("800 frames is a file");
        assert_eq!(
            table.total_samples,
            800 * 1152,
            "every frame carries 1152 samples"
        );
        assert_eq!(table.offsets.len(), SEEK_POINTS);

        // 800 frames over 400 slices is two frames each, and the midpoint of
        // slice n lands in its second frame.
        assert_eq!(
            table.offsets[0], 417,
            "slice 0 midpoint is the second frame"
        );
        assert_eq!(table.offsets[1], 3 * 417);
        assert!(
            table.offsets.windows(2).all(|pair| pair[0] <= pair[1]),
            "a seek index only moves forward"
        );
        let words = table.to_words();
        assert_eq!(words.len(), SEEK_POINTS + 1);
        assert_eq!(words[SEEK_POINTS], 800 * 1152);
    }

    #[test]
    fn the_cover_art_at_the_head_of_the_file_is_skipped() {
        // A megabyte of tag, with a byte pattern that would look like a frame
        // header to a walk that started at zero.
        let mut tagged = synthetic(400, 1_000_000);
        tagged[500] = 0xff;
        tagged[501] = 0xfb;
        let table = seek_table(&tagged).expect("a tagged file is still a file");
        assert_eq!(
            table.total_samples,
            400 * 1152,
            "the tag carries no audio and must not be walked"
        );
        assert_eq!(table.offsets[0], 417);
    }

    #[test]
    fn a_file_with_no_frames_has_no_index() {
        assert!(seek_table(b"not an mp3 at all").is_none());
        assert!(seek_table(&[]).is_none());
    }
}
