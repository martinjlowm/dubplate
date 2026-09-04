//! Key ranking and the notation it is reported in.

use diagnostics::Severity;
use key_detect::{Key, Mode, Profile, analyze};

#[test]
fn camelot_numbers_follow_the_wheel() {
    let cases = [
        (0, Mode::Major, "8B"),
        (9, Mode::Minor, "8A"),
        (11, Mode::Major, "1B"),
        (6, Mode::Minor, "11A"),
        (3, Mode::Major, "5B"),
    ];
    for (tonic, mode, expected) in cases {
        let key = Key { tonic, mode };
        assert_eq!(key.camelot(), expected, "{}", key.name());
    }
    // A key and its relative share a number and differ in letter.
    let a_minor = Key {
        tonic: 9,
        mode: Mode::Minor,
    };
    assert_eq!(
        a_minor.relative(),
        Key {
            tonic: 0,
            mode: Mode::Major
        }
    );
    assert_eq!(a_minor.camelot(), "8A");
}

#[test]
fn a_chroma_shaped_like_a_key_ranks_that_key_first() {
    for tonic in 0..12u8 {
        for mode in [Mode::Major, Mode::Minor] {
            let profile = Profile::Temperley.rotated(mode, tonic);
            let analysis = analyze(profile, 0.0, Profile::Temperley);
            assert_eq!(
                analysis.key,
                Key { tonic, mode },
                "ranked {} first instead",
                analysis.name
            );
            assert!(analysis.ranked.len() == 24);
        }
    }
}

#[test]
fn a_flat_chroma_is_reported_as_meaningless() {
    let analysis = analyze([1.0; 12], 0.0, Profile::Temperley);
    let codes: Vec<&str> = analysis
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|d| d.code)
        .collect();
    assert!(codes.contains(&"flat-chroma"), "got {codes:?}");
}
