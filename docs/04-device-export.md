# 04. Device export reference

What `dubplate export` writes, where it writes it, and which field each
value comes from. For how to build a stick, see the README how-to guides; for
why the writer exists at all, see the README explanation.

## Device layout

```
/Contents/126_05A_Artist-Title.flac          the audio, named by what was measured in it
/PIONEER/rekordbox/export.pdb                the database a Pioneer player browses
/PIONEER/MYSETTING.DAT                       player settings: quantise, hot cues, auto cue
/PIONEER/MYSETTING2.DAT                      display settings: waveform, phrase markings
/PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT  beat grid, cues, monochrome waveforms
/PIONEER/USBANLZ/P000/00000001/ANLZ0000.EXT  the colour waveforms, the extended cues and grid, the phrases
/PIONEER/USBANLZ/P000/00000001/ANLZ0000.2EX  the three-band waveforms an XDJ-RX3 draws
/Engine Library/Database2/m.db               the database a Denon player browses
```

Both databases go on the same device. They read different directories, neither
player looks at the other's, and the audio in `/Contents` is shared.

Every track's analysis directory is hashed from the path of its audio, which is
what a player does to find it. The database stores the path of both the audio
and the analysis file, and `Track::device_path` in the exporter is the single
source of both: nothing else in the pipeline decides where a file lands, and
renaming a track moves its analysis with it.

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
| `--trim` | off | Cut the silence off the head of each file so beat one is sample zero. Needs `--audio-mode copy`, and WAV only. |

Audio and reports are paired by stem: `126_05A_Artist-Title.flac` with
`126_05A_Artist-Title.json`. A file with no report is skipped, with a line on
stderr saying so. It is not exported with an empty beat grid, which would look
analysed on the player and be wrong.

### The browse menu

A player offers a fixed set of categories to browse by, and which ones it offers is table 17
of `export.pdb`. Each row names a column, a position in the menu, and a byte saying whether
the category is hidden.

rekordbox ships eleven categories on and the file name off. This exporter turns the file name
on, in the first free slot, because a dubplate is named `<BPM>_<KEY>_<track> - Artist - Title`
and that is the one axis carrying what was measured:

| Slot | Category |
|---|---|
| 1-10 | Artist, Album, Track, Key, Playlist, History, Search, Matching, Folder, Date added |
| 11 | File name |

Nothing else in the table changes. Browsing by Folder is the one case where a player reads the
file rather than the exported analysis, so a library that can only be found that way loses its
beat grid, its cues and its waveform.

### The seek index

A player scrubbing a variable-bitrate MP3 needs a map from time to byte offset, and that is
what the `PVBR` section of `ANLZ0000.DAT` holds: 400 offsets, one per slice of the track, then
the sample count. `export` builds it by walking the file's frame headers, which is why it
reads each MP3 rather than only stat-ing it.

A WAV needs no such map, since its byte offset is its timestamp times a constant. rekordbox
writes zeros there and so does this.

The offsets are measured from the first frame that carries audio, skipping both the ID3 tag
and `lame`'s own info frame. Checked against an index an XDJ-RX3 wrote for one of these files,
351 of the 401 words match exactly and the rest land one frame, 24 ms, either side.

### Trimming the lead-in

A shop's WAV often opens with a second or more of digital black. Left alone, a
player's auto-cue lands in it and the grid carries the offset into every bar.
`analyze` measures that silence and skips it, so every time in the report
already counts from where the music starts; `source.lead_in_seconds` says how
much, and `lead-in-trimmed` says it happened.

What `export` then does with the audio decides how those times are written:

- Without `--trim`, the file goes across whole and the skipped head is added
  back to every beat and cue, so they line up with the file as it is.
- With `--trim`, the file is cut at `source.trim_to_first_beat_seconds` and the
  times stay counting from there. Beat one is sample zero.

Either way one number decides it, `Track::trim_seconds`, and both the audio and
the grid read it. The source file is never modified: `--trim` writes a new file
under `--out`, which is the same rule `rename` follows.

The cut is byte surgery on the RIFF container rather than a decode and a
re-encode, so the channel count, the bit depth and the sample values come
through untouched. That is also why it is WAV only: cutting a FLAC or an MP3
means decoding it, and the archive pipeline is where those are decoded.

### Artist and title

Both come from the file name, since the tool reads no tags. Two shapes are recognised:

| Name | Artist | Title |
|---|---|---|
| `138_03A_Bryan_Kearney,_Nedea-Back_Once_Again_(Extended_Mix).wav` | `Bryan Kearney, Nedea` | `Back Once Again (Extended Mix)` |
| `112_06A_001 - Beyonce - COZY.mp3` | `Beyonce` | `COZY` |

The separator is ` - ` when the name carries one and a bare `-` otherwise, and a leading run
of digits is a track number or a shop's id rather than an artist. Getting that wrong is not
cosmetic: a library whose every artist is its track number has nothing a player can browse by,
which pushes a DJ into the Folder menu, and loading a track from there is the one case where a
CDJ ignores the exported analysis and reads the file itself.

### The phrases

`PSSI` is where rekordbox puts its phrase analysis, and it is what a player
lights a track by and what other tooling reads instead of running an analysis of
its own. This writes the stretches `report.json` already carries, one phrase
each, in the high mood, whose vocabulary the six labels fit:

| Label | Phrase kind |
|---|---|
| `intro` | 1, Intro |
| `build` | 2, Up |
| `steady` | 2, Up |
| `breakdown` | 3, Down |
| `drop` | 5, Chorus |
| `outro` | 6, Outro |

`steady` has no word of its own in this vocabulary. It goes down as Up rather
than leaving a hole in the middle of the track, which is the one place the
mapping loses something.

These are coarser than rekordbox's. On the same track rekordbox wrote 31
phrases where the structure stage found four stretches: it cuts at every
sixteen or thirty-two bars, and this cuts where the track changed. Both are
claims, and these are the ones with `low_band_db`, `broadband_db`, `rise_db` and
a confidence beside them in `report.json`.

Every export since rekordbox 6 masks the section, and so does this: each byte
from the mood onwards is XORed with a nineteen-byte pattern whose every byte has
the phrase count added to it. A player that reads the mask reads these.

The `k1`, `k2` and `k3` flags that subdivide a high-mood phrase into "Up 1" and
"Up 2", and the `beat2` to `beat4` marks for a change inside a phrase, are
written zero. They are zero on all 31 phrases of the export measured against
too, and nothing here measures what they would say.

### The extended beat grid

`PQT2` is the same beats as `PQTZ` to the microsecond: a header naming the first
and last beat and the beat count, then one big-endian word per beat holding the
microseconds past the whole millisecond `PQTZ` rounds down to. On the export
measured against, `PQTZ` time plus that word over 1000 fits a straight line
through all 923 beats to within 0.046 ms, where the whole milliseconds alone sit
0.527 ms off it.

`PQTZ` rounds a beat down rather than to the nearest millisecond, which is what
lets the remainder be positive. That is what the export does, and changing it
here moved no beat by more than a millisecond.

One word of the header is unexplained. The export carries 0x0cdcb1f5 there,
which is not that track's length, beat count, tempo or sample count, so it reads
as track-specific rather than as a constant and is written zero rather than
copied.

## Where each database field comes from

| Database field | Source |
|---|---|
| tempo | `tempo.bpm`, in centi-BPM. A whole number, so a CDJ shows 126.00 rather than 126.02 |
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
| `bpmAnalyzed`, `bpm` | `tempo.bpm` as a real and as an integer. Both carry the reported whole number; `tempo.bpm_measured` is not exported, since a player uses the tempo to beat match rather than to audit the analysis |
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

## Where a player looks for the analysis

`export.pdb` gives every track an `analyze_path`, and this exporter fills it with the
directory it wrote the files into: `/PIONEER/USBANLZ/P000/00000001` for the first track,
counting up. Every tool that reads these exports takes that field as the answer.

An XDJ-RX3 does not. It works out a directory of its own and reads that, so it has never
opened a file this exporter wrote. The test that shows it: give a track the *other* track's
analysis at the directory `analyze_path` names, and its own analysis at the directory the
player picked. Both tracks draw their own waveform. The two differ by 203 beats, so reading
the wrong one would be obvious.

When the player cannot find a track's analysis where it expects it, it writes its own
`ANLZ0000.DAT` there: the path, and a beat grid of zero beats. From then on it reads that
file. This is why a stick keeps testing as broken after a fix, and why any test needs a file
name the player has never seen.

The directory is a hash of the device path, and this exporter computes the same one:

```
h = 0
for each UTF-16 code unit c of /Contents/126_05A_Artist-Title.flac:
    h = h * 23497 + c        wrapping at 32 bits
    h = h * 37813 + c
index  = h % 200003
bucket = bits 0, 2, 6, 7, 9, 13 and 16 of index, packed in that order
        /PIONEER/USBANLZ/P{bucket:03X}/{index:08X}/ANLZ0000.DAT
```

The constants are Pioneer's, read out of `analyzer::CreateAnlzFileFolderPath` in rekordbox 7.
No ordinary hash produces them: 26 640 combinations of function, encoding and truncation were
tried against 34 known directories and none matched. This one reproduces every directory of
two rekordbox exports and the three an XDJ-RX3 chose for itself, which is what says the player
runs the same function rather than reading the field.

Two consequences. The analysis directory follows the audio path, so renaming a track moves its
analysis; `Track::device_path` decides both. And `analyze_path` still gets written, with the
same value, because every other reader of these exports does use it.

## Player settings

Four files in `/PIONEER/` hold what a deck does rather than anything about a track. Without
them a stick inherits whatever the last DJ left on the player, which is how a cue placed on a
bar line gets played back unquantised.

All four are three fixed-width strings, a length, the settings, then a CRC-16/XMODEM checksum.
The checksum covers the data block alone, except in `DJMMYSETTING.DAT` where it covers the
whole file. `libraries/rust/rekordbox/tests/settings.rs` reproduces a rekordbox 7 export's four
files byte for byte, which is what checks the envelope, the field order, the constants and both
checksum forms together.

The defaults are chosen so a player uses what was measured:

| Setting | Default | Why |
|---|---|---|
| `hotcue_autoload` | `rekordbox` | load the cue pads on track load, with their own colours |
| `hotcue_colour` | on | otherwise the pads are all one colour |
| `quantize`, `quantize_beat_value` | on, 1 beat | the cues sit on bar lines already |
| `auto_cue`, `auto_cue_level` | on, memory | cue to the first memory cue, which is beat one, rather than hunting for silence |
| `waveform_divisions` | phrase | draws the `PSSI` phrases along the waveform |
| `sync` | **off** | a measured grid is a claim about the track, not licence to move it |
| `tempo_range` | 16% | |
| `time_mode`, `jog_mode` | remain, vinyl | |

Everything else a DJ might want is on `settings::Settings`: language, brightnesses, play mode,
master tempo, phase meter, beat jump, jog display, vinyl speed adjust.

Two things people expect to find here and will not. **There is no waveform colour palette** —
which waveform a deck draws is decided by which sections the analysis carries, and it takes the
richest it finds, so writing `PWV7`/`PWV6` is what gets the three-band view. The only related
setting is `waveform`, full waveform against phase meter. And **cue colours are per cue**, held
in `PCO2` as an index into the colour table, not chosen here.

`DEVSETTING.DAT` and `DJMMYSETTING.DAT` carry nothing this exporter decides. They are written
with the values a real export carries, because a file a player reads is worse absent than
copied.

## What is not written

- **Album art.** The `Artwork` table exists and is empty; the artwork id on every
  track is zero.
- **Cue colours.** `PCO2` carries a colour per cue and this writes none, so every
  cue goes out in the player's default. The name does go across: a Nexus 2 reads
  "Drop" off the pad rather than a number.
- **Index pages.** The database carries data pages only. Browsing by title or by
  artist is built from them by the player.
- **Vocal detection, `PVDI`.** Not written, and not measurable here: it needs the
  vocal separated from the mix, which section 4 of `AGENTS.md` puts out of scope.
  Tooling that places a cue on the first vocal has nothing to read.
- **`PCP2`'s last twenty bytes.** Five words rekordcrate reads as unknown and the
  format analysis does not reach. They go out zero. Every populated extended cue
  list read here has been one this tool wrote.
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

The colour waveform sections are the one place the parser is not the oracle.
rekordcrate reads a `PWV5` column's two bytes from the low end up, and this
exporter wrote what it read until an XDJ-RX3 drew no scrolling waveform from a
stick. In the `PWV5` rekordbox wrote for a track this tool also analysed, the
height sits in bits six to two of the second byte, where it matches that file's
`PWV3` height column for column at 0.97 across all 66 395 columns; read one bit
lower, where rekordcrate puts it, it matches at 0.51. So the column is packed
from the high bit of the first byte down, low band first, and the test asserts
those bit positions against the layout measured off the export rather than
against the parser.

The Engine database has no reference parser to check it against, so the check
there is the database itself. Its schema carries constraints, foreign keys and
triggers, and every row this exporter writes has to satisfy them; the tests then
read the values back through `PerformanceData`, which is the view a player reads
the analysis through, and walk the playlist as a player walks it.

What none of this establishes is that a player draws the result. An XDJ-RX3
browses these sticks and plays from them; what it did not draw was the colour
waveform, and the parser said nothing about that because the parser and the
exporter agreed with each other and not with rekordbox. Where a layout is in
doubt now, the check is a rekordbox export of a track this tool also analysed,
put side by side column for column. No Denon deck has read one of these sticks.
The fields whose purpose nobody has established are written with the constants
that appear in real exports, and they are marked as such in `rows.rs`, `anlz.rs`
and `blob.rs`.

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
