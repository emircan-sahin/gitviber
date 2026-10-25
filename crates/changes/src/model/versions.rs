//! A conflicted file as each side has it, without the markers: its code is colored from these,
//! so a block is parsed with the code around it, not as a fragment. Ours (the language is told
//! from it), theirs, and the base where one is known.

use super::conflicts::{Choice, Pick};
use super::text::{Block, Parsed, Segment};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    Ours,
    Theirs,
    Base,
}

/// Each segment's first line (from 1) in each version; None where it has no part there.
#[derive(Debug, Default, PartialEq)]
pub struct Lines(Vec<[Option<u32>; 3]>);

impl Lines {
    /// Where line `ix` of segment `seg`'s part in `v` is in that version.
    pub fn at(&self, seg: usize, v: Version, ix: usize) -> Option<u32> {
        Some(self.0.get(seg)?[v as usize]? + u32::try_from(ix).ok()?)
    }
}

/// The three versions' texts, and where each segment is in them. No base is put together
/// when no block's base is known.
pub fn versions(parsed: &Parsed, bases: &[Option<Vec<String>>]) -> ([String; 3], Lines) {
    let with_base = bases.iter().any(Option::is_some);
    let mut texts: [String; 3] = Default::default();
    let mut next = [1u32; 3];
    let mut starts = Vec::with_capacity(parsed.segments.len());
    let mut block = 0;
    for seg in &parsed.segments {
        let parts: [Option<&[String]>; 3] = match seg {
            Segment::Text(lines) => [Some(lines), Some(lines), Some(lines)],
            Segment::Conflict(b) => {
                let base = bases.get(block).and_then(Option::as_deref);
                block += 1;
                [Some(&b.ours), Some(&b.theirs), base]
            }
        };
        let mut at = [None; 3];
        for (v, part) in parts.into_iter().enumerate() {
            let Some(lines) = part.filter(|_| v != 2 || with_base) else {
                continue;
            };
            at[v] = Some(next[v]);
            for l in lines {
                texts[v].push_str(l);
                texts[v].push('\n');
            }
            next[v] += lines.len() as u32;
        }
        starts.push(at);
    }
    (texts, Lines(starts))
}

/// Where line `ix` of a block's choice comes from; None for one written by hand.
pub fn choice_source(c: &Choice, b: &Block, ix: usize) -> Option<(Version, usize)> {
    match c.kind {
        Pick::Ours => Some((Version::Ours, ix)),
        Pick::Theirs => Some((Version::Theirs, ix)),
        Pick::Both if ix < b.ours.len() => Some((Version::Ours, ix)),
        Pick::Both => Some((Version::Theirs, ix - b.ours.len())),
        Pick::Custom => None,
    }
}

/// Where each line `result_lines` gives comes from, as (segment, version, line of its part);
/// None for a marker or a line written by hand.
pub fn result_sources(
    parsed: &Parsed,
    choices: &HashMap<usize, Choice>,
) -> Vec<Option<(usize, Version, usize)>> {
    let mut out = vec![];
    let mut block = 0;
    for (seg, s) in parsed.segments.iter().enumerate() {
        match s {
            Segment::Text(lines) => {
                out.extend((0..lines.len()).map(|ix| Some((seg, Version::Ours, ix))))
            }
            Segment::Conflict(b) => {
                match choices.get(&block) {
                    None => {
                        out.push(None);
                        out.extend((0..b.ours.len()).map(|ix| Some((seg, Version::Ours, ix))));
                        out.push(None);
                        out.extend((0..b.theirs.len()).map(|ix| Some((seg, Version::Theirs, ix))));
                        out.push(None);
                    }
                    Some(c) => out.extend(
                        (0..c.lines.len())
                            .map(|ix| choice_source(c, b, ix).map(|(v, ix)| (seg, v, ix))),
                    ),
                }
                block += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::conflicts::result_lines;
    use crate::model::text::parse_conflicts;

    const FILE: &str = "a\n<<<<<<< HEAD\nb1\nb2\n=======\nB\n>>>>>>> other\nc\n<<<<<<< HEAD\nd\n||||||| base\nD0\n=======\nD\n>>>>>>> other\n";

    fn blocks(p: &Parsed) -> Vec<&Block> {
        p.segments
            .iter()
            .filter_map(|s| match s {
                Segment::Conflict(b) => Some(b),
                Segment::Text(_) => None,
            })
            .collect()
    }

    #[test]
    fn each_side_reads_as_its_own_file() {
        let p = parse_conflicts(FILE).unwrap();
        let bases = vec![None, Some(vec!["D0".to_string()])];
        let (texts, lines) = versions(&p, &bases);
        assert_eq!(texts[0], "a\nb1\nb2\nc\nd\n");
        assert_eq!(texts[1], "a\nB\nc\nD\n");
        // The first block's base isn't known: the base has nothing in its place.
        assert_eq!(texts[2], "a\nc\nD0\n");
        // "c" is the third segment: line 4 of ours, 3 of theirs, 2 of the base.
        assert_eq!(lines.at(2, Version::Ours, 0), Some(4));
        assert_eq!(lines.at(2, Version::Theirs, 0), Some(3));
        assert_eq!(lines.at(2, Version::Base, 0), Some(2));
        assert_eq!(lines.at(1, Version::Ours, 1), Some(3), "b2");
        assert_eq!(lines.at(1, Version::Base, 0), None);
        assert_eq!(lines.at(3, Version::Base, 0), Some(3), "D0");
        assert_eq!(lines.at(9, Version::Ours, 0), None);
        // No base known anywhere: none is parsed.
        let (texts, lines) = versions(&p, &[None, None]);
        assert_eq!(texts[2], "");
        assert_eq!(lines.at(0, Version::Base, 0), None);
    }

    #[test]
    fn a_choice_and_the_preview_take_their_lines_from_the_sides() {
        let p = parse_conflicts(FILE).unwrap();
        let b = blocks(&p);
        let both = Choice::of(b[0], Pick::Both);
        assert_eq!(choice_source(&both, b[0], 1), Some((Version::Ours, 1)));
        assert_eq!(choice_source(&both, b[0], 2), Some((Version::Theirs, 0)));
        assert_eq!(choice_source(&Choice::custom("x"), b[0], 0), None);
        let mut choices = HashMap::new();
        choices.insert(1, Choice::of(b[1], Pick::Theirs));
        let shown = result_lines(&p, &choices, false);
        let from = result_sources(&p, &choices);
        assert_eq!(from.len(), shown.len());
        assert_eq!(
            from,
            [
                Some((0, Version::Ours, 0)),
                None,
                Some((1, Version::Ours, 0)),
                Some((1, Version::Ours, 1)),
                None,
                Some((1, Version::Theirs, 0)),
                None,
                Some((2, Version::Ours, 0)),
                Some((3, Version::Theirs, 0)),
            ]
        );
    }
}
