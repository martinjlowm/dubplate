//! The settings files, against the ones a rekordbox export wrote.
//!
//! There is no reference parser for these, so the check is the one rule 14 of
//! `AGENTS.md` asks for: write the values a real file encodes and compare the
//! bytes. That covers the envelope, the field order, the constants nobody has
//! explained and both forms of the checksum at once, which no round trip
//! through this crate's own writer would.
//!
//! The fixtures in `tests/settings/` are the four files a rekordbox 7 export
//! left on a stick. They are settings, not a library: no track, no path and no
//! name is in them.

use rekordbox::settings::*;

/// The settings the fixture files encode, which are not this crate's defaults.
///
/// Read out of the fixtures by hand. Reproducing them is what proves the
/// writer, so they are spelled out here rather than derived from anything.
fn as_the_fixtures_have_them() -> Settings {
    Settings {
        quantize: Toggle::On,
        quantize_beat_value: QuantizeBeat::Beat,
        hotcue_autoload: HotCueAutoload::On,
        hotcue_colour: Toggle::Off,
        auto_cue: Toggle::On,
        auto_cue_level: AutoCueLevel::Memory,
        time_mode: TimeMode::Remain,
        jog_mode: JogMode::Vinyl,
        tempo_range: TempoRange::Ten,
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

/// Compare two settings files and say where they first differ.
#[track_caller]
fn same(name: &str, wrote: &[u8], fixture: &[u8]) {
    assert_eq!(
        wrote.len(),
        fixture.len(),
        "{name} is {} bytes, rekordbox writes {}",
        wrote.len(),
        fixture.len()
    );
    let differing: Vec<String> = wrote
        .iter()
        .zip(fixture)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(at, (a, b))| format!("  0x{at:03x}: wrote 0x{a:02x}, rekordbox 0x{b:02x}"))
        .collect();
    assert!(
        differing.is_empty(),
        "{name} differs from the one rekordbox writes at {} bytes:\n{}",
        differing.len(),
        differing.join("\n")
    );
}

#[test]
fn my_setting_is_byte_for_byte_the_one_rekordbox_writes() {
    same(
        "MYSETTING.DAT",
        &my_setting(&as_the_fixtures_have_them()),
        include_bytes!("settings/MYSETTING.DAT"),
    );
}

#[test]
fn my_setting_2_is_byte_for_byte_the_one_rekordbox_writes() {
    same(
        "MYSETTING2.DAT",
        &my_setting_2(&as_the_fixtures_have_them()),
        include_bytes!("settings/MYSETTING2.DAT"),
    );
}

/// `DEVSETTING.DAT` and `DJMMYSETTING.DAT` carry nothing this crate decides, so
/// reproducing them exactly is the whole of what they have to do.
///
/// `DJMMYSETTING.DAT` is also the one file whose checksum covers the header as
/// well as the data, so this is where that difference is caught.
#[test]
fn the_files_this_crate_has_no_opinion_about_are_copied_exactly() {
    same(
        "DEVSETTING.DAT",
        &dev_setting(),
        include_bytes!("settings/DEVSETTING.DAT"),
    );
    same(
        "DJMMYSETTING.DAT",
        &djm_my_setting(),
        include_bytes!("settings/DJMMYSETTING.DAT"),
    );
}

/// The defaults are the ones that make a player use what this tool measured.
///
/// A stick is written once and played from for years, so the settings that
/// decide whether the cues and phrases are used at all are worth stating rather
/// than inheriting from whoever touched the deck last.
#[test]
fn the_defaults_switch_on_what_the_analysis_is_for() {
    let settings = Settings::default();
    assert_eq!(settings.hotcue_autoload, HotCueAutoload::Rekordbox);
    assert_eq!(settings.hotcue_colour, Toggle::On);
    assert_eq!(settings.quantize, Toggle::On);
    assert_eq!(settings.quantize_beat_value, QuantizeBeat::Beat);
    assert_eq!(settings.auto_cue_level, AutoCueLevel::Memory);
    assert_eq!(settings.waveform_divisions, WaveformDivisions::Phrase);
    assert_eq!(
        settings.sync,
        Toggle::Off,
        "a measured grid is a claim about the track, not licence to move it"
    );

    // Whatever the settings, the file is the length rekordbox writes.
    assert_eq!(my_setting(&settings).len(), 148);
    assert_eq!(my_setting_2(&settings).len(), 148);
}
