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

use deku::prelude::*;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::Write;

/// Every layout here is fixed, so a write cannot fail for anything a caller
/// could fix. Asserted once rather than at each of the dozen places a struct
/// becomes bytes.
fn bytes(layout: &impl DekuContainerWrite) -> Vec<u8> {
    layout.to_bytes().expect("a fixed layout with no counts")
}

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

/// `trackData`, uncompressed: 44 bytes, all big-endian.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct TrackData {
    sample_rate: f64,
    samples: i64,
    /// Engine's own key numbering, 0 for 8B through 23 for 7A.
    key: i32,
    average_loudness: f64,
    peak_loudness: f64,
    perceived_loudness: f64,
}

/// `trackData`: the numbers every other blob is measured against.
pub fn track_data(sample_rate: f64, samples: i64, key: i32, loudness: (f64, f64, f64)) -> Vec<u8> {
    compress(&bytes(&TrackData {
        sample_rate,
        samples,
        key,
        average_loudness: loudness.0,
        peak_loudness: loudness.1,
        perceived_loudness: loudness.2,
    }))
}

/// `beatData`: the same grid twice, as analysed and as adjusted.
///
/// Writing the analysed grid into both is what "the user has not moved it"
/// looks like in this format.
pub fn beat_data(sample_rate: f64, samples: f64, grid: &[BeatGridMarker]) -> Vec<u8> {
    let mut out = bytes(&BeatDataHeader {
        sample_rate,
        samples,
        is_beat_grid_set: u8::from(!grid.is_empty()),
    });
    for _ in 0..2 {
        out.extend(bytes(&MarkerCount {
            markers: grid.len() as i64,
        }));
        for marker in grid {
            out.extend(bytes(&Marker {
                sample_offset: marker.sample_offset,
                beat_number: marker.beat_number,
                beats_to_next_marker: marker.beats_to_next_marker,
                unknown: 0,
            }));
        }
    }
    compress(&out)
}

/// `beatData`, the fields before the first grid. Big-endian.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct BeatDataHeader {
    sample_rate: f64,
    samples: f64,
    is_beat_grid_set: u8,
}

/// The count in front of each of the two grids, big-endian like the header and
/// unlike the markers it counts.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct MarkerCount {
    markers: i64,
}

/// One beat grid marker, little-endian in the middle of a big-endian blob.
///
/// The mixed order is the format's, not a mistake being preserved: reading a
/// marker the wrong way round gives a sample offset of about 1e-300 rather than
/// an error, so this is the field group most worth having declared.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct Marker {
    sample_offset: f64,
    beat_number: i64,
    beats_to_next_marker: i32,
    /// Zero in every export examined, and libdjinterop writes zero too.
    unknown: i32,
}

/// `quickCues`: the eight hot cue slots, and the main cue.
///
/// The slots are written even when empty, because the player draws eight
/// buttons whatever the database says. An empty slot is a sample offset of -1.
pub fn quick_cues(cues: &[Option<(String, f64, [u8; 4])>], main_cue: f64) -> Vec<u8> {
    let mut out = bytes(&QuickCueCount {
        cues: cues.len() as i64,
    });
    for cue in cues {
        let (label, sample_offset, colour) = match cue {
            Some((label, offset, colour)) => (label.as_bytes().to_vec(), *offset, *colour),
            // An empty slot is a label of nothing and a sample offset of -1.
            None => (Vec::new(), -1.0, [0, 0, 0, 0]),
        };
        out.extend(bytes(&QuickCue {
            len_label: label.len() as u8,
            label,
            sample_offset,
            colour,
        }));
    }
    out.extend(bytes(&MainCue {
        sample_offset: main_cue,
        // The main cue is where the analysis put it, not where a user did.
        is_set_by_user: 0,
        sample_offset_again: main_cue,
    }));
    compress(&out)
}

/// The count in front of the eight `quickCues` slots.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct QuickCueCount {
    cues: i64,
}

/// One hot cue slot. The label is as long as its own length field says.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct QuickCue {
    len_label: u8,
    label: Vec<u8>,
    sample_offset: f64,
    colour: [u8; 4],
}

/// The main cue, written twice with a flag between saying who put it there.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct MainCue {
    sample_offset: f64,
    is_set_by_user: u8,
    sample_offset_again: f64,
}

/// `loops`: eight slots, all empty here, and stored uncompressed.
pub fn loops(count: usize) -> Vec<u8> {
    // Little-endian, unlike every other count in these blobs.
    let mut out = bytes(&LoopCount {
        loops: count as i64,
    });
    for _ in 0..count {
        out.extend(bytes(&EmptyLoop {
            len_label: 0,
            start: -1.0,
            end: -1.0,
            is_start_set: 0,
            is_end_set: 0,
            colour: [0, 0, 0, 0],
        }));
    }
    out
}

/// The count in front of the loop slots, little-endian unlike every other
/// count in these blobs.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct LoopCount {
    loops: i64,
}

/// One empty loop slot: no label, both ends unset at -1.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct EmptyLoop {
    len_label: u8,
    start: f64,
    end: f64,
    is_start_set: u8,
    is_end_set: u8,
    colour: [u8; 4],
}

/// `overviewWaveFormData`: the whole track in a few hundred points.
pub fn overview_waveform(points: &[WaveformPoint], samples_per_point: f64) -> Vec<u8> {
    let mut out = bytes(&OverviewHeader {
        points: points.len() as i64,
        points_again: points.len() as i64,
        samples_per_point,
    });
    for point in points {
        out.extend(bytes(&Point {
            low: point.low,
            mid: point.mid,
            high: point.high,
        }));
    }
    // The loudest point in the track, which the player scales its drawing to.
    let maximum = points
        .iter()
        .fold(WaveformPoint::default(), |max, point| WaveformPoint {
            low: max.low.max(point.low),
            mid: max.mid.max(point.mid),
            high: max.high.max(point.high),
        });
    out.extend(bytes(&Point {
        low: maximum.low,
        mid: maximum.mid,
        high: maximum.high,
    }));
    compress(&out)
}

/// `overviewWaveFormData`, the fields before the points. The count is written
/// twice and nobody has established why.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct OverviewHeader {
    points: i64,
    points_again: i64,
    samples_per_point: f64,
}

/// One point of a waveform: a byte per band. The last one in the blob is the
/// loudest point rather than a point in the track.
#[derive(DekuWrite)]
struct Point {
    low: u8,
    mid: u8,
    high: u8,
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
