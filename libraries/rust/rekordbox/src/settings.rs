//! The four settings files rekordbox leaves in `/PIONEER/`.
//!
//! These are player preferences rather than anything about a track: what a deck
//! quantises to, whether it loads hot cues on its own, which waveform it draws.
//! A stick without them leaves every deck on whatever the last DJ set, which is
//! how a cue that this tool placed on a bar line gets played back unquantised.
//!
//! All four share one envelope: three fixed-width strings naming who wrote the
//! file, a length, the settings themselves, then a checksum. The checksum is
//! CRC-16/XMODEM, over the data block alone except in `DJMMYSETTING.DAT`, where
//! it covers the whole file up to itself. Both forms were measured against the
//! four files a rekordbox 7 export wrote, and `tests/settings.rs` reproduces
//! those files byte for byte from the values they encode.
//!
//! Field names and option lists follow `pyrekordbox`, which is the only open
//! implementation of these files.

use deku::prelude::*;

/// Every layout here is fixed, so a write cannot fail for anything a caller
/// could fix.
fn bytes(layout: &impl DekuContainerWrite) -> Vec<u8> {
    layout.to_bytes().expect("a fixed layout with no counts")
}

/// A setting that is either on or off. Every such field in these files stores
/// 0x80 for off and 0x81 for on, so the two are one type rather than sixteen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Toggle {
    Off = 0x80,
    On = 0x81,
}

/// What a player snaps a cue or a loop to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum QuantizeBeat {
    Beat = 0x80,
    Half = 0x81,
    Quarter = 0x82,
    Eighth = 0x83,
}

/// Whether a deck loads a track's hot cues when the track loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HotCueAutoload {
    Off = 0x80,
    On = 0x81,
    /// Load them, and take the colours from the analysis rather than the deck.
    Rekordbox = 0x82,
}

/// How quiet a passage has to be before auto cue calls it the start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AutoCueLevel {
    Minus36dB = 0x80,
    Minus42dB = 0x81,
    Minus48dB = 0x82,
    Minus54dB = 0x83,
    Minus60dB = 0x84,
    Minus66dB = 0x85,
    Minus72dB = 0x86,
    Minus78dB = 0x87,
    /// Use the track's first memory cue, which is the one this tool places on
    /// beat one.
    Memory = 0x88,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TimeMode {
    Elapsed = 0x80,
    Remain = 0x81,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum JogMode {
    Cdj = 0x80,
    Vinyl = 0x81,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TempoRange {
    Six = 0x80,
    Ten = 0x81,
    Sixteen = 0x82,
    Wide = 0x83,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayMode {
    Continue = 0x80,
    Single = 0x81,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PhaseMeter {
    Type1 = 0x80,
    Type2 = 0x81,
}

/// The language a deck labels its own menus in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Language {
    English = 0x81,
    French = 0x82,
    German = 0x83,
    Italian = 0x84,
    Dutch = 0x85,
    Spanish = 0x86,
    Russian = 0x87,
    Korean = 0x88,
    ChineseSimplified = 0x89,
    ChineseTraditional = 0x8A,
    Japanese = 0x8B,
    Portuguese = 0x8C,
    Swedish = 0x8D,
    Czech = 0x8E,
    Hungarian = 0x8F,
    Danish = 0x90,
    Greek = 0x91,
    Turkish = 0x92,
}

/// What the main display draws. Not a colour scheme: which of the waveforms in
/// the analysis a deck picks is decided by which ones it finds, richest first,
/// and this exporter writes all three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WaveformView {
    Waveform = 0x80,
    PhaseMeter = 0x81,
}

/// What the marks along the waveform count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WaveformDivisions {
    TimeScale = 0x80,
    /// Draw the phrases, which is what `PSSI` carries.
    Phrase = 0x81,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum JogDisplay {
    Auto = 0x80,
    Info = 0x81,
    Simple = 0x82,
    Artwork = 0x83,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BeatJump {
    Half = 0x80,
    One = 0x81,
    Two = 0x82,
    Four = 0x83,
    Eight = 0x84,
    Sixteen = 0x85,
    ThirtyTwo = 0x86,
    SixtyFour = 0x87,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VinylSpeedAdjust {
    TouchRelease = 0x80,
    Touch = 0x81,
    Release = 0x82,
}

/// A brightness, on the one-to-five scale the display settings use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Brightness {
    One = 0x81,
    Two = 0x82,
    Three = 0x83,
    Four = 0x84,
    Five = 0x85,
}

/// The settings a stick carries, as a DJ would describe them.
///
/// Only the fields worth deciding are here. The rest of each file is written
/// with the values a rekordbox export carries, because a field nobody has
/// explained is not a field to invent a value for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub quantize: Toggle,
    pub quantize_beat_value: QuantizeBeat,
    pub hotcue_autoload: HotCueAutoload,
    pub hotcue_colour: Toggle,
    pub auto_cue: Toggle,
    pub auto_cue_level: AutoCueLevel,
    pub time_mode: TimeMode,
    pub jog_mode: JogMode,
    pub tempo_range: TempoRange,
    pub master_tempo: Toggle,
    pub sync: Toggle,
    pub play_mode: PlayMode,
    pub phase_meter: PhaseMeter,
    pub language: Language,
    pub lcd_brightness: Brightness,
    pub waveform: WaveformView,
    pub waveform_divisions: WaveformDivisions,
    pub jog_display_mode: JogDisplay,
    pub beat_jump: BeatJump,
    pub vinyl_speed_adjust: VinylSpeedAdjust,
    pub jog_lcd_brightness: Brightness,
}

impl Default for Settings {
    /// Defaults chosen for what this tool writes, not for what a deck ships
    /// with.
    ///
    /// The analysis places cues on bar lines and names phrases, so the settings
    /// that make a deck use them are on: hot cues load with their own colours,
    /// quantise snaps to the beat, auto cue takes the first memory cue this tool
    /// put on beat one rather than hunting for silence, and the waveform is
    /// divided by phrase so `PSSI` shows. Sync stays off, because a grid this
    /// tool measured is a claim about the track and not licence to move it.
    fn default() -> Self {
        Settings {
            quantize: Toggle::On,
            quantize_beat_value: QuantizeBeat::Beat,
            hotcue_autoload: HotCueAutoload::Rekordbox,
            hotcue_colour: Toggle::On,
            auto_cue: Toggle::On,
            auto_cue_level: AutoCueLevel::Memory,
            time_mode: TimeMode::Remain,
            jog_mode: JogMode::Vinyl,
            tempo_range: TempoRange::Sixteen,
            master_tempo: Toggle::Off,
            sync: Toggle::Off,
            play_mode: PlayMode::Single,
            phase_meter: PhaseMeter::Type1,
            language: Language::English,
            lcd_brightness: Brightness::Three,
            waveform: WaveformView::Waveform,
            waveform_divisions: WaveformDivisions::Phrase,
            jog_display_mode: JogDisplay::Auto,
            beat_jump: BeatJump::Sixteen,
            vinyl_speed_adjust: VinylSpeedAdjust::Touch,
            jog_lcd_brightness: Brightness::Three,
        }
    }
}

/// The envelope every settings file shares: who wrote it, then how much follows.
///
/// `len_strings` counts the three fixed-width strings rather than the header, so
/// it is 0x60 in all four files whatever the strings say.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct FileHeader {
    len_strings: u32,
    brand: [u8; 32],
    software: [u8; 32],
    version: [u8; 32],
    len_data: u32,
}

/// `MYSETTING.DAT`: what the player does, as opposed to what it shows.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct MySettingBody {
    /// Eight bytes every export carries here, purpose unrecorded.
    marker: [u8; 8],
    on_air_display: u8,
    lcd_brightness: u8,
    quantize: u8,
    auto_cue_level: u8,
    language: u8,
    unknown1: u8,
    jog_ring_brightness: u8,
    jog_ring_indicator: u8,
    slip_flashing: u8,
    unknown2: [u8; 3],
    disc_slot_illumination: u8,
    eject_lock: u8,
    sync: u8,
    play_mode: u8,
    quantize_beat_value: u8,
    hotcue_autoload: u8,
    hotcue_colour: u8,
    unknown3: u16,
    needle_lock: u8,
    unknown4: u16,
    time_mode: u8,
    jog_mode: u8,
    auto_cue: u8,
    master_tempo: u8,
    tempo_range: u8,
    phase_meter: u8,
    unknown5: u16,
}

/// `MYSETTING2.DAT`: what the player draws.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct MySetting2Body {
    vinyl_speed_adjust: u8,
    jog_display_mode: u8,
    pad_button_brightness: u8,
    jog_lcd_brightness: u8,
    waveform_divisions: u8,
    unknown1: [u8; 5],
    waveform: u8,
    /// 0x81 in every export examined.
    unknown2: u8,
    beat_jump_beat_value: u8,
    unknown3: [u8; 27],
}

/// The checksum both forms of these files carry: CRC-16/XMODEM, seeded at zero.
fn checksum(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Pad a name into the fixed width these headers store it at.
fn fixed(text: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    let bytes = text.as_bytes();
    let take = bytes.len().min(out.len() - 1);
    out[..take].copy_from_slice(&bytes[..take]);
    out
}

/// Wrap a data block in its envelope and close it with the checksum.
///
/// `whole_file` is what `DJMMYSETTING.DAT` needs: its checksum covers the header
/// as well, which is the one way the four files differ from each other.
fn file(brand: &str, version: &str, data: &[u8], whole_file: bool) -> Vec<u8> {
    let mut out = bytes(&FileHeader {
        len_strings: 0x60,
        brand: fixed(brand),
        software: fixed("rekordbox"),
        version: fixed(version),
        len_data: data.len() as u32,
    });
    out.extend_from_slice(data);

    let covered = if whole_file { &out[..] } else { data };
    out.extend_from_slice(&checksum(covered).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// `MYSETTING.DAT`, from the settings a caller chose.
pub fn my_setting(settings: &Settings) -> Vec<u8> {
    let body = MySettingBody {
        marker: [0x78, 0x56, 0x34, 0x12, 0x02, 0x00, 0x00, 0x00],
        on_air_display: Toggle::On as u8,
        lcd_brightness: settings.lcd_brightness as u8,
        quantize: settings.quantize as u8,
        auto_cue_level: settings.auto_cue_level as u8,
        language: settings.language as u8,
        unknown1: 0x01,
        jog_ring_brightness: 0x82,
        jog_ring_indicator: Toggle::On as u8,
        slip_flashing: Toggle::On as u8,
        unknown2: [0x01, 0x01, 0x01],
        disc_slot_illumination: 0x82,
        eject_lock: 0x81,
        sync: settings.sync as u8,
        play_mode: settings.play_mode as u8,
        quantize_beat_value: settings.quantize_beat_value as u8,
        hotcue_autoload: settings.hotcue_autoload as u8,
        hotcue_colour: settings.hotcue_colour as u8,
        unknown3: 0,
        needle_lock: 0x81,
        unknown4: 0,
        time_mode: settings.time_mode as u8,
        jog_mode: settings.jog_mode as u8,
        auto_cue: settings.auto_cue as u8,
        master_tempo: settings.master_tempo as u8,
        tempo_range: settings.tempo_range as u8,
        phase_meter: settings.phase_meter as u8,
        unknown5: 0,
    };
    file("PIONEER", "0.001", &bytes(&body), false)
}

/// `MYSETTING2.DAT`, from the settings a caller chose.
pub fn my_setting_2(settings: &Settings) -> Vec<u8> {
    let body = MySetting2Body {
        vinyl_speed_adjust: settings.vinyl_speed_adjust as u8,
        jog_display_mode: settings.jog_display_mode as u8,
        pad_button_brightness: Brightness::Three as u8,
        jog_lcd_brightness: settings.jog_lcd_brightness as u8,
        waveform_divisions: settings.waveform_divisions as u8,
        unknown1: [0; 5],
        waveform: settings.waveform as u8,
        unknown2: 0x81,
        beat_jump_beat_value: settings.beat_jump as u8,
        unknown3: [0; 27],
    };
    file("PIONEER", "0.001", &bytes(&body), false)
}

/// `DEVSETTING.DAT`, which carries twenty-four bytes nobody has decoded.
///
/// Written with the values the export measured against carries. A player reads
/// this file whether or not anything here understands it, so leaving it out and
/// zeroing it are both worse than copying what works.
pub fn dev_setting() -> Vec<u8> {
    let mut data = vec![0x78, 0x56, 0x34, 0x12, 0x01, 0x00, 0x00, 0x00];
    data.extend_from_slice(&[0x01, 0x02, 0x03, 0x01, 0x02, 0x01]);
    data.resize(0x20, 0);
    file("PIONEER DJ", "6.7.7", &data, false)
}

/// `DJMMYSETTING.DAT`: the mixer's own settings, none of which this tool has an
/// opinion about, written as the export measured against carries them.
pub fn djm_my_setting() -> Vec<u8> {
    let mut data = vec![
        0x78, 0x56, 0x34, 0x12, 0x01, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00,
    ];
    data.extend_from_slice(&[
        0x81, 0x82, 0x80, 0x80, 0x81, 0x81, 0x80, 0x81, 0x80, 0x80, 0x85, 0x82, 0x80, 0x80, 0x82,
        0x84, 0x81, 0x81,
    ]);
    data.resize(0x34, 0);
    file("PioneerDJ", "1.000", &data, true)
}
