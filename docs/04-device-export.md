# 04. Device export reference

What `music-analyze export` writes, where it writes it, and which field each
value comes from. For how to build a stick, see the README how-to guides; for
why the writer exists at all, see the README explanation.

## Device layout

```
/Contents/138_03A_Artist-Title.flac          the audio, named by what was measured in it
/PIONEER/rekordbox/export.pdb                the database a Pioneer player browses
/PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT  beat grid, cues, preview waveforms
/PIONEER/USBANLZ/P000/00000001/ANLZ0000.EXT  the scrolling waveform
```

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
| `--target <NAME>` | `rekordbox` | `rekordbox`, `engine` or `both`. Only `rekordbox` is implemented. |
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

## What is not written

- **Colour waveforms.** A player from the Nexus 2 generation onward draws
  `PWV4`/`PWV5` if they are present and falls back to the monochrome `PWAV` and
  `PWV3` if they are not. Only the monochrome pair is written.
- **Album art.** The `Artwork` table exists and is empty; the artwork id on every
  track is zero.
- **Hot cues.** The hot cue list is written and empty. Only one memory cue, on
  the first beat, is set.
- **Index pages.** The database carries data pages only. Browsing by title or by
  artist is built from them by the player.
- **Song structure, phrase analysis, `PSSI`.** Not written.
- **The Engine Library.** `--target engine` is accepted and does nothing yet.

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

What that does not establish is that a player accepts the result. No CDJ or XDJ
has read one of these sticks. The fields whose purpose nobody has established
are written with the constants that appear in real exports, and they are marked
as such in `rows.rs` and `anlz.rs`.

## Images

`devenv build outputs.usb-flac` (or `usb-wav`, `usb-mp3`) writes a FAT32 image
holding that format's files and a database built from the same analysis.

| Property | Value |
|---|---|
| Filesystem | FAT32, which is what a CDJ mounts |
| Volume label | `MUSIC` |
| Volume id | fixed, so the same library builds the same image |
| Size | payload plus 12% plus 64 MB, and at least 128 MB |

Written with `mkfs.vfat` and `mtools`, so no mounting and no root. `fsck.vfat`
reads the result back at the end of the build.

Copy one to a stick with `dd if=result/usb-flac.img of=/dev/diskN bs=4m`, having
checked twice which disk that is.
