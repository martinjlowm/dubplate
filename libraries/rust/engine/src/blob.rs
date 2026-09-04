//! The binary columns an Engine database stores its analysis in.
//!
//! Five blobs per track, four of them zlib streams with the uncompressed length
//! in front. That framing is Qt's `qCompress`, which is what Engine's own
//! software is written against; the fifth, the loops, is stored raw.
//!
//! Byte order is mixed and not by accident: the outer fields are big-endian and
//! the beat grid markers and the loops are little-endian. Both are as the format
//! is, and reading one the wrong way round gives plausible nonsense rather than
//! an error.
//!
//! Layouts follow the format analysis in
//! <https://github.com/mixxxdj/mixxx/wiki/Engine-Library-Format> and the
//! encoders in libdjinterop, which is where the fields the wiki leaves open are
//! pinned down.

use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::Write;

/// Wrap a buffer the way Engine stores one: four bytes of uncompressed length,
/// then a zlib stream.
fn compress(uncompressed: &[u8]) -> Vec<u8> {
    let mut out = (uncompressed.len() as u32).to_be_bytes().to_vec();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(uncompressed)
        .expect("writing to a vector cannot fail");
    out.extend(encoder.finish().expect("finishing a vector cannot fail"));
    out
}

/// A point on the whole-track waveform: one byte per band.
#[derive(Clone, Copy, Debug, Default)]
pub struct WaveformPoint {
    pub low: u8,
    pub mid: u8,
    pub high: u8,
}

/// One beat grid marker.
///
/// A grid has at least two: the first is beat -4, which sits before the start
/// of the file, and the last is one beat past the end. That is the format's own
/// convention and it is what leaves room to nudge a grid by up to a bar.
#[derive(Clone, Copy, Debug)]
pub struct BeatGridMarker {
    pub sample_offset: f64,
    pub beat_number: i64,
    pub beats_to_next_marker: i32,
}

/// `trackData`: the numbers every other blob is measured against.
pub fn track_data(sample_rate: f64, samples: i64, key: i32, loudness: (f64, f64, f64)) -> Vec<u8> {
    let mut out = Vec::with_capacity(44);
    out.extend_from_slice(&sample_rate.to_be_bytes());
    out.extend_from_slice(&samples.to_be_bytes());
    out.extend_from_slice(&key.to_be_bytes());
    out.extend_from_slice(&loudness.0.to_be_bytes());
    out.extend_from_slice(&loudness.1.to_be_bytes());
    out.extend_from_slice(&loudness.2.to_be_bytes());
    compress(&out)
}

/// `beatData`: the same grid twice, as analysed and as adjusted.
///
/// Writing the analysed grid into both is what "the user has not moved it"
/// looks like in this format.
pub fn beat_data(sample_rate: f64, samples: f64, grid: &[BeatGridMarker]) -> Vec<u8> {
    let mut out = Vec::with_capacity(33 + 48 * grid.len());
    out.extend_from_slice(&sample_rate.to_be_bytes());
    out.extend_from_slice(&samples.to_be_bytes());
    out.push(u8::from(!grid.is_empty()));
    for _ in 0..2 {
        out.extend_from_slice(&(grid.len() as i64).to_be_bytes());
        for marker in grid {
            // Little-endian from here to the end of the marker.
            out.extend_from_slice(&marker.sample_offset.to_le_bytes());
            out.extend_from_slice(&marker.beat_number.to_le_bytes());
            out.extend_from_slice(&marker.beats_to_next_marker.to_le_bytes());
            out.extend_from_slice(&0i32.to_le_bytes());
        }
    }
    compress(&out)
}

/// `quickCues`: the eight hot cue slots, and the main cue.
///
/// The slots are written even when empty, because the player draws eight
/// buttons whatever the database says. An empty slot is a sample offset of -1.
pub fn quick_cues(cues: &[Option<(String, f64, [u8; 4])>], main_cue: f64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(cues.len() as i64).to_be_bytes());
    for cue in cues {
        match cue {
            Some((label, sample_offset, colour)) => {
                out.push(label.len() as u8);
                out.extend_from_slice(label.as_bytes());
                out.extend_from_slice(&sample_offset.to_be_bytes());
                out.extend_from_slice(colour);
            }
            None => {
                out.push(0);
                out.extend_from_slice(&(-1.0f64).to_be_bytes());
                out.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    out.extend_from_slice(&main_cue.to_be_bytes());
    out.push(0); // The main cue is where the analysis put it, not the user.
    out.extend_from_slice(&main_cue.to_be_bytes());
    compress(&out)
}

/// `loops`: eight slots, all empty here, and stored uncompressed.
pub fn loops(count: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + 23 * count);
    // Little-endian, unlike every other count in these blobs.
    out.extend_from_slice(&(count as i64).to_le_bytes());
    for _ in 0..count {
        out.push(0); // label length
        out.extend_from_slice(&(-1.0f64).to_le_bytes()); // start
        out.extend_from_slice(&(-1.0f64).to_le_bytes()); // end
        out.push(0); // start set
        out.push(0); // end set
        out.extend_from_slice(&[0, 0, 0, 0]); // colour
    }
    out
}

/// `overviewWaveFormData`: the whole track in a few hundred points.
pub fn overview_waveform(points: &[WaveformPoint], samples_per_point: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity(27 + 3 * points.len());
    out.extend_from_slice(&(points.len() as i64).to_be_bytes());
    out.extend_from_slice(&(points.len() as i64).to_be_bytes());
    out.extend_from_slice(&samples_per_point.to_be_bytes());
    for point in points {
        out.extend_from_slice(&[point.low, point.mid, point.high]);
    }
    // The loudest point in the track, which the player scales its drawing to.
    let maximum = points
        .iter()
        .fold(WaveformPoint::default(), |max, point| WaveformPoint {
            low: max.low.max(point.low),
            mid: max.mid.max(point.mid),
            high: max.high.max(point.high),
        });
    out.extend_from_slice(&[maximum.low, maximum.mid, maximum.high]);
    compress(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    fn decompress(blob: &[u8]) -> Vec<u8> {
        let declared = u32::from_be_bytes(blob[..4].try_into().unwrap()) as usize;
        let mut out = Vec::new();
        ZlibDecoder::new(&blob[4..]).read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), declared, "the length prefix has to be the truth");
        out
    }

    #[test]
    fn track_data_round_trips_through_the_framing() {
        let blob = track_data(44100.0, 26_460_000, 7, (0.5, 0.4, 0.3));
        let raw = decompress(&blob);
        assert_eq!(raw.len(), 44);
        assert_eq!(f64::from_be_bytes(raw[0..8].try_into().unwrap()), 44100.0);
        assert_eq!(
            i64::from_be_bytes(raw[8..16].try_into().unwrap()),
            26_460_000
        );
        assert_eq!(i32::from_be_bytes(raw[16..20].try_into().unwrap()), 7);
    }

    #[test]
    fn a_beat_grid_is_written_twice_and_little_endian_inside() {
        let grid = [
            BeatGridMarker {
                sample_offset: -88_813.78,
                beat_number: -4,
                beats_to_next_marker: 628,
            },
            BeatGridMarker {
                sample_offset: 17_000_758.37,
                beat_number: 624,
                beats_to_next_marker: 0,
            },
        ];
        let raw = decompress(&beat_data(44100.0, 16_988_686.0, &grid));
        // 17 bytes of header, then two grids of a count and two 24-byte markers.
        assert_eq!(raw.len(), 17 + 2 * (8 + 2 * 24));
        assert_eq!(raw[16], 1, "the grid is set");
        assert_eq!(i64::from_be_bytes(raw[17..25].try_into().unwrap()), 2);
        assert_eq!(
            f64::from_le_bytes(raw[25..33].try_into().unwrap()),
            -88_813.78,
            "marker offsets are little-endian"
        );
        assert_eq!(i64::from_le_bytes(raw[33..41].try_into().unwrap()), -4);

        // The adjusted grid follows, identical.
        let adjusted = 17 + 8 + 2 * 24;
        assert_eq!(&raw[17..adjusted], &raw[adjusted..adjusted + 8 + 2 * 24]);
    }

    #[test]
    fn empty_cue_slots_are_written_as_slots() {
        let empty: Vec<Option<(String, f64, [u8; 4])>> = vec![None; 8];
        let raw = decompress(&quick_cues(&empty, 4410.0));
        assert_eq!(i64::from_be_bytes(raw[0..8].try_into().unwrap()), 8);
        // 8 header bytes, 8 slots of 13, then the main cue fields.
        assert_eq!(raw.len(), 8 + 8 * 13 + 17);
        assert_eq!(
            f64::from_be_bytes(raw[8 + 8 * 13..8 + 8 * 13 + 8].try_into().unwrap()),
            4410.0
        );
    }

    #[test]
    fn loops_are_stored_uncompressed() {
        let blob = loops(8);
        assert_eq!(i64::from_le_bytes(blob[0..8].try_into().unwrap()), 8);
        assert_eq!(blob.len(), 8 + 8 * 23);
    }
}
