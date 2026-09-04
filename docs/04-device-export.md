# 04. Device export reference

What `dubplate export` writes, where it writes it, and which field each
value comes from. For how to build a stick, see the README how-to guides; for
why the writer exists at all, see the README explanation.

## Device layout

```
/Contents/138_03A_Artist-Title.flac          the audio, named by what was measured in it
/PIONEER/rekordbox/export.pdb                the database a Pioneer player browses
/PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT  beat grid, cues, monochrome waveforms
/PIONEER/USBANLZ/P000/00000001/ANLZ0000.EXT  the scrolling and colour waveforms
/Engine Library/Database2/m.db               the database a Denon player browses
```

Both databases go on the same device. They read different directories, neither
player looks at the other's, and the audio in `/Contents` is shared.

Every track's analysis directory is derived from its row id, not hashed, so the
same collection exports to the same paths. The database stores the path of both
the audio and the analysis file, and `Track::device_path` in the exporter is the
single source of both: nothing else in the pipeline decides where a file lands.

`export` writes the databases only. The audio is placed by whoever assembles the
device, which is the image builder in the Nix pipeline and `--audio-mode` when
running by hand.

## `export` flags

| Flag | Default | Effect |
|---|---|---|
| `--audio <DIR>` | required | Directory of audio files, read one level deep. |
| `--reports <DIR>` | required | Directory of `<stem>.json` reports, one per audio file. |
| `-o`, `--out <DIR>` | required | Device root to write into. |
| `--target <NAME>` | `rekordbox` | `rekordbox`, `engine` or `both`. The Nix images build with `both`. |
| `--audio-mode <MODE>` | `none` | `none`, `copy` or `symlink`. |
| `--playlist <NAME>` | `All tracks` | Name of the playlist holding every track. |
| `--date <YYYY-MM-DD>` | `SOURCE_DATE_EPOCH`, else today | Written as each track's added and analysed date. |

Audio and reports are paired by stem: `138_03A_Artist-Title.flac` with
`138_03A_Artist-Title.json`. A file with no report is skipped, with a line on
stderr saying so. It is not exported with an empty beat grid, which would look
analysed on the player and be wrong.

## Where each database field comes from

| Database field | Source |
|---|---|
| tempo | `tempo.bpm`, in centi-BPM |
| key | `key.name`, shared by every track in that key |
| duration | `source.duration_seconds`, rounded |
| sample rate, bit depth | `source.sample_rate`, 16 |
| bitrate | sample rate times depth times channels for a lossless file; file size over duration for a lossy one |
| file size | the file on disk |
| file type | the extension: 1 for MP3, 5 for FLAC, 0xb for WAV, 0xc for AIFF |
| artist, title | the file name, split on the first hyphen after the tempo and key prefix |
| genre, album, comment, colour, rating | not set |
| beat grid | `tempo.grid.beats_seconds`, with beat one taken from `tempo.bar.phase` |
| memory cue | the first beat |
| preview waveform | `waveforms.preview`, 400 columns |
| detail waveform | `waveforms.detail`, 150 columns per second |

Nothing reads tags. A WAV carries none worth trusting and the name is what the
shop wrote, so the name is what the database says.

## The Engine Library

Schema 2.21.2, which is the `Database2` layout Engine DJ 2 and 3 read. The
earlier `m.db` plus `p.db` pair from Engine Prime and the first SC5000 firmware
is not written.

| Column or blob | Source |
|---|---|
| `bpmAnalyzed`, `bpm` | `tempo.bpm`, exact and rounded |
| `key` | the Camelot key as Engine numbers it, 0 for 8B through 23 for 7A |
| `length` | duration in whole seconds |
| `path` | `../Contents/<file>`, relative to the Engine Library directory |
| `bitrate`, `fileBytes`, `fileType` | the file |
| `title`, `artist`, `genre` | the file name and the report |
| `trackData` | sample rate, length in samples, key, and average loudness per band |
| `beatData` | two grid markers, beat -4 and one past the end, written as both the analysed and the adjusted grid |
| `quickCues` | eight empty slots, and the main cue on the first beat |
| `loops` | eight empty slots |
| `overviewWaveFormData` | 1024 points of low, mid and high |
| `Playlist`, `PlaylistEntity` | one playlist, entries as the linked list the format stores |

The scrolling waveform column, `highResolutionWaveFormData`, is left empty: no
open description of its layout exists, and a player analyses the track on load
rather than refusing it.

The schema itself is Denon's, transcribed from libdjinterop, which is the open
record of each Engine schema version. A player checks what it finds, so the
statements are the format rather than a design.

## What is not written

- **Album art.** The `Artwork` table exists and is empty; the artwork id on every
  track is zero.
- **Hot cues.** The hot cue list is written and empty. Only one memory cue, on
  the first beat, is set.
- **Index pages.** The database carries data pages only. Browsing by title or by
  artist is built from them by the player.
- **Song structure, phrase analysis, `PSSI`.** Not written.
- **Engine's scrolling waveform.** See above.
- **Engine crates and smartlists.** One playlist per device, and no crates.

## How much of this is verified

Every file this exporter writes is parsed back by
[rekordcrate](https://github.com/Holzhaus/rekordcrate), the reference parser for
the format, in the tests of `libraries/rust/rekordbox`. That covers:

- a database of fifty tracks, which crosses several pages and row groups
- a non-ASCII title, which takes the UTF-16 string encoding and the four-byte
  alignment that goes with it
- artists, genres and keys shared between tracks by id
- a playlist and its entries, in order
- both analysis files, section by section, including the beat grid and the cue
  list

The parser is pinned to an upstream commit rather than the published 0.3.0,
which reads a cue point's type as 0 where the current analysis and rekordbox use
1.

The colour waveform sections are checked the same way: the red, green, blue and
height of each column come back from the parser exactly as written.

The Engine database has no reference parser to check it against, so the check
there is the database itself. Its schema carries constraints, foreign keys and
triggers, and every row this exporter writes has to satisfy them; the tests then
read the values back through `PerformanceData`, which is the view a player reads
the analysis through, and walk the playlist as a player walks it.

What none of this establishes is that a player accepts the result. No CDJ, XDJ
or Denon deck has read one of these sticks. The fields whose purpose nobody has
established are written with the constants that appear in real exports, and they
are marked as such in `rows.rs`, `anlz.rs` and `blob.rs`.

## Images

`devenv build outputs.usb` writes a FAT32 image per format, each holding that
format's files and both databases built from the same analysis.
`devenv build outputs.usb.flac` writes just that one, and the same nesting holds
for `outputs.library`.

| Property | Value |
|---|---|
| Filesystem | FAT32, which is what a CDJ mounts |
| Volume label | `MUSIC` |
| Volume id | fixed, so the same library builds the same image |
| Size | payload plus 12% plus 64 MB, and at least 128 MB |

Written with `mkfs.vfat` and `mtools`, so no mounting and no root. `fsck.vfat`
reads the result back at the end of the build.

`devenv build` prints the store path it wrote rather than leaving a `result`
symlink. The image is the `.img` file inside that path, so copying one to a
stick reads:

```sh
dd if=/nix/store/…-usb-flac.img/usb-flac.img of=/dev/diskN bs=4m
```

Check twice which disk that is. The combined output holds `wav.img`, `flac.img`
and `mp3.img` as links, so the same command works from there with the name
changed.
