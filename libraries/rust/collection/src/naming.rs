//! Naming a file after what was measured in it.
//!
//! `<BPM>_<KEY>_<rest of the original name>`, with the tempo zero-padded to three
//! digits and the Camelot number to two, so a plain alphabetical listing sorts by
//! tempo first and then around the wheel. Sorting is the whole point of the
//! padding: `96` sorts before `138` as text, and `8A` sorts between `12A` and
//! `1A`.
//!
//! Here rather than in the CLI because there are now two callers. The CLI writes
//! these names onto a disk; a browser writes them into a FAT32 image, and both
//! databases store the result. One of them producing `126_5A_` and the other
//! `126_05A_` would be a stick whose listing does not sort.

/// The name a file measured at `bpm` in `camelot` should carry.
///
/// `camelot` is the short form the report prints, such as `8A`. Returns `None`
/// for a tempo or a key that cannot be written, which is a measurement that
/// failed rather than a name that is awkward.
pub fn target_name(file_name: &str, bpm: f64, camelot: &str) -> Option<String> {
    if !bpm.is_finite() || !(1.0..1000.0).contains(&bpm) {
        return None;
    }

    let (stem, extension) = match file_name.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (file_name, String::new()),
    };

    let (number, letter) = camelot.split_at(camelot.len().checked_sub(1)?);
    let number: u32 = number.parse().ok()?;
    if !(1..=12).contains(&number) || !matches!(letter, "A" | "B") {
        return None;
    }

    Some(format!(
        "{:03}_{:02}{}_{}{}",
        bpm.round() as u32,
        number,
        letter,
        strip_prefix(stem),
        extension
    ))
}

/// Drop a `123_08A_` prefix this tool wrote earlier.
///
/// Renaming an already renamed file is the common case: a second run after a
/// flag change should replace the numbers rather than stack another pair in
/// front of them. The exporter needs the same test to keep a prefix out of the
/// title it shows a player, which is why this is one function and not two.
pub fn strip_prefix(stem: &str) -> &str {
    let mut parts = stem.splitn(3, '_');
    let (Some(bpm), Some(key), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return stem;
    };
    let bpm_shaped = bpm.len() == 3 && bpm.bytes().all(|b| b.is_ascii_digit());
    let key_shaped = key.len() == 3
        && key.as_bytes()[..2].iter().all(u8::is_ascii_digit)
        && matches!(key.as_bytes()[2], b'A' | b'B');
    if bpm_shaped && key_shaped { rest } else { stem }
}

#[cfg(test)]
mod tests {
    use super::target_name;

    #[test]
    fn pads_both_numbers_so_a_listing_sorts() {
        assert_eq!(
            target_name("Artist-Title (Extended Mix).wav", 96.4, "8A").unwrap(),
            "096_08A_Artist-Title (Extended Mix).wav"
        );
        assert_eq!(
            target_name("x.wav", 199.84, "12B").unwrap(),
            "200_12B_x.wav"
        );
    }

    #[test]
    fn renaming_twice_replaces_the_prefix_instead_of_stacking_one() {
        let once = target_name("Artist-Title.wav", 138.0, "3A").unwrap();
        assert_eq!(
            target_name(&once, 140.0, "5A").unwrap(),
            "140_05A_Artist-Title.wav"
        );
    }

    #[test]
    fn a_name_that_only_looks_like_a_prefix_is_left_alone() {
        // Three digits and something ending in A, but not a Camelot key.
        assert_eq!(
            target_name("808_MIA_State-Pacific.wav", 120.0, "8B").unwrap(),
            "120_08B_808_MIA_State-Pacific.wav"
        );
    }

    #[test]
    fn an_unmeasurable_tempo_or_key_is_refused() {
        assert!(target_name("x.wav", f64::NAN, "8A").is_none());
        assert!(target_name("x.wav", 128.0, "13A").is_none());
        assert!(target_name("x.wav", 128.0, "8C").is_none());
    }
}
