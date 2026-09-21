//! Turning sections into the eight pads a player has.
//!
//! The layout is fixed: pad A is always the first beat, pad D is always the
//! drop. A scheme that moved with the track would be more faithful to what was
//! measured and useless under the hands, because the hands learn the pad and
//! not the track.
//!
//! A pad whose section was never found stays empty rather than being filled
//! with the nearest thing, and [`Cues::missing`] names it. A drop cue on a
//! track with no drop is worse than no drop cue.

use crate::label::{Label, Section};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CueKind {
    Hot,
    Memory,
}

/// One cue, on one pad, at one bar.
#[derive(Clone, Debug, Serialize)]
pub struct Cue {
    pub kind: CueKind,
    /// 1 to 8. Hot cue 1 is pad A; memory cue 1 is the first in the list.
    pub number: u8,
    pub name: &'static str,
    /// The colour rekordbox gives this pad. Carried for the report and the
    /// page; the exporter does not write it, because a cue colour lives in the
    /// `PCO2` section this tool does not yet produce.
    pub colour: &'static str,
    pub time_seconds: f64,
    pub bar: usize,
    /// Which section put the cue here, as an index into the section list.
    pub from_section: Option<usize>,
}

/// Where one pad landed: the bar, and the section that put it there.
type Placement = Option<(usize, Option<usize>)>;

/// The eight roles, in pad order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    FirstBeat,
    LoopIn,
    Buildup,
    Drop,
    Breakdown,
    Special,
    Outro,
    LoopOut,
}

impl Role {
    const ALL: [Role; 8] = [
        Role::FirstBeat,
        Role::LoopIn,
        Role::Buildup,
        Role::Drop,
        Role::Breakdown,
        Role::Special,
        Role::Outro,
        Role::LoopOut,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Role::FirstBeat => "First beat",
            Role::LoopIn => "Loop in",
            Role::Buildup => "Buildup",
            Role::Drop => "Drop",
            Role::Breakdown => "Breakdown",
            Role::Special => "Special",
            Role::Outro => "Outro",
            Role::LoopOut => "Loop out",
        }
    }

    pub fn colour(self) -> &'static str {
        match self {
            Role::FirstBeat | Role::LoopIn => "green",
            Role::Buildup => "yellow",
            Role::Drop => "red",
            Role::Breakdown => "blue",
            Role::Special => "purple",
            Role::Outro => "cyan",
            Role::LoopOut => "orange",
        }
    }

    pub fn pad(self) -> char {
        (b'A' + Role::ALL.iter().position(|r| *r == self).unwrap_or(0) as u8) as char
    }

    /// Whether the memory cue for this role sits where the hot cue does, or a
    /// run-up ahead of it.
    fn memory_leads_in(self) -> bool {
        !matches!(self, Role::FirstBeat | Role::LoopIn | Role::LoopOut)
    }
}

/// What the scheme produced, and what it could not fill.
#[derive(Clone, Debug, Serialize)]
pub struct Cues {
    pub cues: Vec<Cue>,
    /// Pads with no section to put on them, as pad letters.
    pub missing: Vec<String>,
    /// Set when no drop sat past the opening fraction and the pad took an
    /// earlier one instead. The rule still applied; the caller says so rather
    /// than reporting a drop cue that quietly broke it.
    pub drop_rule_relaxed: bool,
}

/// How the cues are placed. Every field is a flag on the CLI.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct CueSettings {
    /// Bars between a memory cue and the hot cue it runs into.
    pub memory_offset_bars: usize,
    /// Length of the loop pad B and H mark out.
    pub loop_bars: usize,
    /// A drop before this share of the track is a false drop, not the drop.
    /// rekordbox users call it the twenty per cent rule and djcues applies it
    /// too: an eight-bar taste of the hook at 0:30 is not where you mix in.
    pub drop_after_fraction: f64,
}

impl Default for CueSettings {
    fn default() -> Self {
        CueSettings {
            memory_offset_bars: 16,
            loop_bars: 4,
            drop_after_fraction: 0.2,
        }
    }
}

/// Place the eight pads over the sections.
///
/// `bar_starts` carries the time of every bar, so a cue is a bar number and the
/// time follows from the grid rather than from arithmetic on the tempo.
pub fn place(
    sections: &[Section],
    bar_starts: &[f64],
    duration_seconds: f64,
    settings: &CueSettings,
) -> Cues {
    let bar_time = |bar: usize| bar_starts.get(bar).copied();
    let first = |wanted: Label, after_bar: usize| {
        sections
            .iter()
            .enumerate()
            .find(|(_, section)| section.label == wanted && section.start_bar >= after_bar)
    };

    // The drop is the first one past the opening fraction of the track. A
    // shorter track has a shorter run-up, so the rule is a fraction and not a
    // number of bars.
    let earliest_drop_bar = bar_starts
        .iter()
        .position(|time| *time >= duration_seconds * settings.drop_after_fraction)
        .unwrap_or(0);

    let late_drop = first(Label::Drop, earliest_drop_bar);
    let drop = late_drop.or_else(|| first(Label::Drop, 0));
    let drop_rule_relaxed = late_drop.is_none() && drop.is_some();
    let buildup = drop.and_then(|(drop_index, _)| {
        sections[..drop_index]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, section)| section.label == Label::Build)
    });
    let breakdown = first(Label::Breakdown, 0);
    let outro = sections
        .iter()
        .enumerate()
        .rev()
        .find(|(_, section)| section.label == Label::Outro);

    // The loop lives where the track first holds a steady kick, which on a club
    // edit is the intro's second half and is what a DJ loops to mix in. Failing
    // that, the first bar.
    let loop_in_bar = sections
        .iter()
        .find(|section| matches!(section.label, Label::Drop | Label::Steady))
        .map(|section| section.start_bar)
        .or_else(|| sections.first().map(|section| section.start_bar))
        .unwrap_or(0);

    // Whatever changed hardest that no other pad already marks.
    let taken: Vec<usize> = [drop, buildup, breakdown, outro]
        .into_iter()
        .flatten()
        .map(|(index, _)| index)
        .collect();
    let special = sections
        .iter()
        .enumerate()
        .filter(|(index, section)| {
            *index > 0 && !taken.contains(index) && section.label != Label::Intro
        })
        .max_by(|a, b| a.1.broadband_db.total_cmp(&b.1.broadband_db));

    let placed: Vec<(Role, Placement)> = vec![
        (Role::FirstBeat, Some((0, None))),
        (Role::LoopIn, Some((loop_in_bar, None))),
        (
            Role::Buildup,
            buildup.map(|(index, section)| (section.start_bar, Some(index))),
        ),
        (
            Role::Drop,
            drop.map(|(index, section)| (section.start_bar, Some(index))),
        ),
        (
            Role::Breakdown,
            breakdown.map(|(index, section)| (section.start_bar, Some(index))),
        ),
        (
            Role::Special,
            special.map(|(index, section)| (section.start_bar, Some(index))),
        ),
        (
            Role::Outro,
            outro.map(|(index, section)| (section.start_bar, Some(index))),
        ),
        (
            Role::LoopOut,
            Some((loop_in_bar + settings.loop_bars, None)),
        ),
    ];

    let mut cues = Vec::new();
    let mut missing = Vec::new();
    let mut memory_number = 0u8;
    let mut memory_bars: Vec<usize> = Vec::new();
    for (pad, (role, where_at)) in placed.into_iter().enumerate() {
        let Some((bar, from_section)) = where_at else {
            missing.push(role.pad().to_string());
            continue;
        };
        let Some(time_seconds) = bar_time(bar) else {
            missing.push(role.pad().to_string());
            continue;
        };

        cues.push(Cue {
            kind: CueKind::Hot,
            number: pad as u8 + 1,
            name: role.name(),
            colour: role.colour(),
            time_seconds,
            bar,
            from_section,
        });

        // The memory cue either shares the hot cue's bar or sits a run-up
        // ahead of it, which is where a mix actually starts.
        let memory_bar = if role.memory_leads_in() {
            bar.saturating_sub(settings.memory_offset_bars)
        } else {
            bar
        };
        // A run-up that ran off the front of the track lands on bar zero, where
        // the first beat already is. The memory cues are a list a player
        // scrolls rather than pads it lights, so three of them on one bar is
        // three rows saying the same thing.
        if memory_bars.contains(&memory_bar) {
            continue;
        }
        if let Some(time_seconds) = bar_time(memory_bar) {
            memory_bars.push(memory_bar);
            memory_number += 1;
            cues.push(Cue {
                kind: CueKind::Memory,
                number: memory_number,
                name: role.name(),
                colour: role.colour(),
                time_seconds,
                bar: memory_bar,
                from_section,
            });
        }
    }

    Cues {
        cues,
        missing,
        drop_rule_relaxed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::label::Section;

    fn section(label: Label, start_bar: usize, bars: usize, broadband_db: f64) -> Section {
        Section {
            label,
            confidence: 1.0,
            start_seconds: start_bar as f64 * 2.0,
            end_seconds: (start_bar + bars) as f64 * 2.0,
            start_bar,
            bars,
            low_band_db: -10.0,
            broadband_db,
            rise_db: 0.0,
        }
    }

    fn grid(bars: usize) -> Vec<f64> {
        (0..=bars).map(|bar| bar as f64 * 2.0).collect()
    }

    fn a_dance_track() -> Vec<Section> {
        vec![
            section(Label::Intro, 0, 16, -30.0),
            section(Label::Build, 16, 16, -20.0),
            section(Label::Drop, 32, 32, -6.0),
            section(Label::Breakdown, 64, 16, -24.0),
            section(Label::Drop, 80, 32, -6.5),
            section(Label::Outro, 112, 16, -18.0),
        ]
    }

    #[test]
    fn every_pad_lands_on_the_section_it_names() {
        let sections = a_dance_track();
        let placed = place(&sections, &grid(128), 256.0, &CueSettings::default());
        let hot: Vec<(u8, &str, usize)> = placed
            .cues
            .iter()
            .filter(|cue| cue.kind == CueKind::Hot)
            .map(|cue| (cue.number, cue.name, cue.bar))
            .collect();
        assert_eq!(
            hot,
            vec![
                (1, "First beat", 0),
                (2, "Loop in", 32),
                (3, "Buildup", 16),
                (4, "Drop", 32),
                (5, "Breakdown", 64),
                (6, "Special", 80),
                (7, "Outro", 112),
                (8, "Loop out", 36),
            ]
        );
        assert!(placed.missing.is_empty(), "every pad had a section");
    }

    #[test]
    fn a_memory_cue_runs_into_its_hot_cue() {
        let sections = a_dance_track();
        let placed = place(&sections, &grid(128), 256.0, &CueSettings::default());
        let memory_bar = |name: &str| {
            placed
                .cues
                .iter()
                .find(|cue| cue.kind == CueKind::Memory && cue.name == name)
                .map(|cue| cue.bar)
        };
        // Sixteen bars ahead of the drop at bar 32.
        assert_eq!(memory_bar("Drop"), Some(16));
        // The first beat and the loop pads share their hot cue's bar.
        assert_eq!(memory_bar("First beat"), Some(0));
        assert_eq!(memory_bar("Loop out"), Some(36));
    }

    #[test]
    fn a_taste_of_the_hook_early_on_is_not_the_drop() {
        let mut sections = a_dance_track();
        // Eight bars of the hook at bar 8, well inside the opening fifth.
        sections.insert(1, section(Label::Drop, 8, 8, -6.0));
        let placed = place(&sections, &grid(128), 256.0, &CueSettings::default());
        let drop = placed
            .cues
            .iter()
            .find(|cue| cue.kind == CueKind::Hot && cue.name == "Drop")
            .expect("a drop pad");
        assert_eq!(
            drop.bar, 32,
            "the drop pad took the false drop at bar 8 instead of the real one at 32"
        );
    }

    #[test]
    fn a_pad_with_no_section_is_left_empty_and_named() {
        // No breakdown and no build anywhere in the track.
        let sections = vec![
            section(Label::Intro, 0, 16, -30.0),
            section(Label::Drop, 16, 64, -6.0),
            section(Label::Outro, 80, 16, -18.0),
        ];
        let placed = place(&sections, &grid(96), 192.0, &CueSettings::default());
        // F too: with no build and no breakdown, every section that is not the
        // intro is already on a pad, so nothing is left to be the special one.
        assert_eq!(placed.missing, vec!["C", "E", "F"]);
        assert!(
            !placed.cues.iter().any(|cue| cue.name == "Buildup"),
            "a track with no build gets no buildup cue"
        );
    }
}
