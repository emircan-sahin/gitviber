//! Review notes: comments on lines of a diff, kept per worktree in app storage (never in the repo)
//! and handed to the agent as a prompt. A note is anchored by its lines' content and the lines
//! around them, not only their number, so it follows them as the agent edits around them, tells
//! them from a same-looking line elsewhere (`}`, `return null;`), and turns outdated once they change.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Where a note is: its lines, and what was around them when it was written.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    /// First and last line, 1-based. A note on the files on disk follows its lines as they move.
    pub start: usize,
    pub end: usize,
    /// The lines as they were: all of them, or the first KEEP of a longer run (its last in `tail`).
    pub code: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail: Option<Vec<String>>,
    /// A longer run's lines all hashed: a rewrite between `code` and `tail` changes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Up to CONTEXT lines above and below as they were; None: a note from before these were kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReviewNote {
    pub id: String,
    pub path: String,
    pub body: String,
    #[serde(flatten)]
    pub anchor: Anchor,
    /// On the old side of a diff, removed or replaced lines.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub old: bool,
    /// Written on a commit's version ("commit 1a2b3c4"); None: on the files on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// Its lines on disk changed since: the agent rewrote them, or the file is gone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outdated: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resolved: bool,
}

impl ReviewNote {
    /// A note as stored by this or an older build; anything else there is dropped.
    pub fn read(v: &serde_json::Value) -> Option<ReviewNote> {
        let n: ReviewNote = serde_json::from_value(v.clone()).ok()?;
        (n.anchor.end >= n.anchor.start).then_some(n)
    }
}

const CONTEXT: usize = 2;
// A note on a whole file would keep all of it, in storage every workspace save shares.
const KEEP: usize = 20;

/// FNV-1a over UTF-16 units, as the old app hashed: tells two texts apart cheaply.
pub fn hash(text: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for unit in text.encode_utf16() {
        h = (h ^ unit as u32).wrapping_mul(0x0100_0193);
    }
    base36(h)
}

fn base36(mut n: u32) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn slice(lines: &[String], from: usize, to: usize) -> Vec<String> {
    let to = to.min(lines.len());
    if from >= to {
        return vec![];
    }
    lines[from..to].to_vec()
}

/// A note's anchor on lines `start`–`end` of `lines`.
pub fn anchor_at(lines: &[String], start: usize, end: usize) -> Anchor {
    let all = slice(lines, start - 1, end);
    let long = all.len() > 2 * KEEP;
    Anchor {
        start,
        end,
        code: if long {
            all[..KEEP].to_vec()
        } else {
            all.clone()
        },
        tail: long.then(|| all[all.len() - KEEP..].to_vec()),
        hash: long.then(|| hash(&all.join("\n"))),
        before: Some(slice(lines, (start - 1).saturating_sub(CONTEXT), start - 1)),
        after: Some(slice(lines, end, end + CONTEXT)),
    }
}

/// A note on the files on disk, which an agent's edits can make outdated; the rest are on
/// versions that don't change.
pub fn is_live(n: &ReviewNote) -> bool {
    !n.old && n.at.is_none()
}

/// Where a note should be in a newer version of its file: near `expect`, between lines `lo` and
/// `hi` (neither included).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Near {
    pub expect: f64,
    pub lo: usize,
    pub hi: usize,
}

/// Whether `a`'s own lines, as they were, start at line `s` of `lines` (what's around them aside).
pub fn own_at(lines: &[String], a: &Anchor, s: usize) -> bool {
    let size = a.end - a.start + 1;
    let tail: &[String] = a.tail.as_deref().unwrap_or_default();
    let same = |at: usize, want: &[String]| {
        at >= 1
            && want
                .iter()
                .enumerate()
                .all(|(i, w)| lines.get(at - 1 + i) == Some(w))
    };
    !a.code.is_empty()
        && s >= 1
        && s + size - 1 <= lines.len()
        && same(s, &a.code)
        && (s + size)
            .checked_sub(tail.len())
            .is_some_and(|at| same(at, tail))
        && a.hash
            .as_ref()
            .is_none_or(|h| hash(&lines[s - 1..s - 1 + size].join("\n")) == *h)
}

/// Where `a`'s lines are in `lines`, or None once they're gone; nearest to where it's expected if
/// there are several. Its own lines must be there as they were, with the lines around them as
/// they were too. Without that, only where it's expected: in the stretch `near` says the edits
/// left it in, else at its own line number. A same-looking line elsewhere never takes it.
pub fn find_note(lines: &[String], a: &Anchor, near: Option<Near>) -> Option<usize> {
    if a.code.is_empty() {
        return None;
    }
    let size = a.end - a.start + 1;
    let expect = near.map_or(a.start as f64, |n| n.expect);
    let same = |at: isize, want: &[String]| {
        at >= 1
            && want
                .iter()
                .enumerate()
                .all(|(i, w)| lines.get(at as usize - 1 + i) == Some(w))
    };
    let own = |s: usize| own_at(lines, a, s);
    let around = |s: usize| {
        a.before
            .as_ref()
            .is_none_or(|b| b.is_empty() || same(s as isize - b.len() as isize, b))
            && a.after
                .as_ref()
                .is_none_or(|af| af.is_empty() || same((s + size) as isize, af))
    };
    let closer = |best: Option<usize>, s: usize| match best {
        Some(b) if (s as f64 - expect).abs() >= (b as f64 - expect).abs() => Some(b),
        _ => Some(s),
    };
    let (mut full, mut inside) = (None, None);
    let mut s = 1;
    while s + size - 1 <= lines.len() {
        if own(s) {
            if around(s) {
                full = closer(full, s);
            }
            if let Some(n) = near
                && s > n.lo
                && s + size - 1 < n.hi
            {
                inside = closer(inside, s);
            }
        }
        s += 1;
    }
    let at_own = a.start;
    full.or(if near.is_some() {
        inside
    } else if own(at_own) {
        Some(at_own)
    } else {
        None
    })
}

/// Where lines `start`–`end` of `prev` went in `next`, by the nearest lines around them that each
/// version has once (as patience diff anchors a diff): the stretch between those, and the shift above.
pub fn shifted(prev: &[String], next: &[String], start: usize, end: usize) -> Near {
    // Each line's index, or None once it's seen twice.
    fn once(ls: &[String]) -> HashMap<&str, Option<usize>> {
        let mut m: HashMap<&str, Option<usize>> = HashMap::new();
        for (i, l) in ls.iter().enumerate() {
            m.entry(l.as_str())
                .and_modify(|v| *v = None)
                .or_insert(Some(i));
        }
        m
    }
    let (a, b) = (once(prev), once(next));
    // The index in `next` of `prev[i]` when both have it once (blank lines tell nothing apart).
    let anchor = |i: usize| -> Option<usize> {
        let l = prev[i].as_str();
        if l.trim().is_empty() || a.get(l).copied().flatten().is_none() {
            return None;
        }
        b.get(l).copied().flatten()
    };
    let mut near = Near {
        expect: start as f64,
        lo: 0,
        hi: next.len() + 1,
    };
    let above = (start - 1).min(prev.len());
    for i in (0..above).rev() {
        if let Some(j) = anchor(i) {
            near.lo = j + 1;
            near.expect = start as f64 + j as f64 - i as f64;
            break;
        }
    }
    for k in end..prev.len() {
        if let Some(m) = anchor(k) {
            near.hi = m + 1;
            // Nothing to go by above: it ends where it did, counted from below.
            if near.lo == 0 {
                near.expect = start as f64 + m as f64 - k as f64;
            }
            break;
        }
    }
    near
}

/// Live notes on `path` checked against its text on disk now (None: it's gone; `prev`: as it
/// was last checked): each moves with its lines, or turns outdated when they're no longer there
/// (and back, if the agent puts them back). None when nothing changed.
pub fn check_notes(
    notes: &[ReviewNote],
    path: &str,
    lines: Option<&[String]>,
    prev: Option<&[String]>,
) -> Option<Vec<ReviewNote>> {
    let mut changed = false;
    let next: Vec<ReviewNote> = notes
        .iter()
        .map(|n| {
            if n.path != path || !is_live(n) || n.resolved {
                return n.clone();
            }
            let start = lines.and_then(|ls| {
                let near = prev.map(|p| shifted(p, ls, n.anchor.start, n.anchor.end));
                find_note(ls, &n.anchor, near)
            });
            let mut moved = n.clone();
            match start {
                None => moved.outdated = true,
                Some(s) => {
                    moved.anchor.end = s + n.anchor.end - n.anchor.start;
                    moved.anchor.start = s;
                    moved.outdated = false;
                }
            }
            if moved.anchor.start == n.anchor.start && moved.outdated == n.outdated {
                return n.clone();
            }
            changed = true;
            moved
        })
        .collect();
    changed.then_some(next)
}

/// Live notes on `path` once its text, typed into, is saved as `lines`: `at` has the line each
/// one's last line is on now, as the editor carried it through the typing. Each takes its lines
/// there, with what's around them now, or turns outdated where they were rewritten. None when
/// nothing changed.
pub fn follow_notes(
    notes: &[ReviewNote],
    path: &str,
    lines: &[String],
    at: &HashMap<String, usize>,
) -> Option<Vec<ReviewNote>> {
    let mut changed = false;
    let next: Vec<ReviewNote> = notes
        .iter()
        .map(|n| {
            let Some(&end) = at.get(&n.id) else {
                return n.clone();
            };
            if n.path != path || !is_live(n) || n.resolved {
                return n.clone();
            }
            let size = n.anchor.end - n.anchor.start + 1;
            let start = (end + 1).saturating_sub(size).max(1);
            let mut moved = n.clone();
            if own_at(lines, &n.anchor, start) {
                moved.anchor = anchor_at(lines, start, start + size - 1);
                moved.outdated = false;
            } else {
                moved.outdated = true;
            }
            changed |= moved != *n;
            moved
        })
        .collect();
    changed.then_some(next)
}

/// A note where a file on show has its lines: on the old side or the new, from `start` to `end`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedNote {
    pub note: ReviewNote,
    pub old: bool,
    pub start: usize,
    pub end: usize,
}

/// A file on show: each side's path and lines (None: that side doesn't exist).
pub struct Shown<'a> {
    pub path: &'a str,
    pub old_path: &'a str,
    pub old_lines: Option<&'a [String]>,
    pub new_lines: Option<&'a [String]>,
}

/// The open notes on a file on show, where its versions have their lines.
pub fn place_notes(notes: &[ReviewNote], file: &Shown) -> Vec<PlacedNote> {
    notes
        .iter()
        .filter_map(|note| {
            let lines = if note.old {
                file.old_lines
            } else {
                file.new_lines
            };
            let path = if note.old { file.old_path } else { file.path };
            if note.resolved || note.path != path {
                return None;
            }
            let start = find_note(lines?, &note.anchor, None)?;
            Some(PlacedNote {
                note: note.clone(),
                old: note.old,
                start,
                end: start + note.anchor.end - note.anchor.start,
            })
        })
        .collect()
}

/// A fence the code can't close early: one backtick longer than any run of them in it.
fn fence(code: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in code.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

/// "line 12" or "lines 12–14".
pub fn note_lines(n: &ReviewNote) -> String {
    if n.anchor.end > n.anchor.start {
        format!("lines {}–{}", n.anchor.start, n.anchor.end)
    } else {
        format!("line {}", n.anchor.start)
    }
}

/// Where a note is, as an agent reads a location: `path:12` or `path:12-14`.
pub fn note_location(n: &ReviewNote) -> String {
    if n.anchor.end > n.anchor.start {
        format!("{}:{}-{}", n.path, n.anchor.start, n.anchor.end)
    } else {
        format!("{}:{}", n.path, n.anchor.start)
    }
}

/// The note's code as the prompt quotes it: a long run's start and end, with how much is left out.
fn quoted(n: &ReviewNote) -> String {
    let Some(tail) = &n.anchor.tail else {
        return n.anchor.code.join("\n");
    };
    let left = (n.anchor.end - n.anchor.start + 1).saturating_sub(n.anchor.code.len() + tail.len());
    let mut all = n.anchor.code.clone();
    all.push(format!("⋯ {left} more lines ⋯"));
    all.extend(tail.iter().cloned());
    all.join("\n")
}

/// The notes as a prompt for the agent: each one's place, its code quoted, then what was said.
pub fn notes_prompt(notes: &[ReviewNote]) -> String {
    notes
        .iter()
        .map(|n| {
            let mut about = vec![];
            if n.old {
                about.push("the old version".to_string());
            }
            if let Some(at) = &n.at {
                about.push(format!("in {at}"));
            }
            if n.outdated {
                about.push("outdated: these lines have changed since".into());
            }
            let about = if about.is_empty() {
                String::new()
            } else {
                format!(" ({})", about.join("; "))
            };
            let code = quoted(n);
            let f = fence(&code);
            let name = n.path.rsplit('/').next().unwrap_or(&n.path);
            let ext = name
                .rsplit_once('.')
                .map(|(_, e)| e)
                .filter(|e| {
                    !e.is_empty()
                        && e.chars()
                            .all(|c| c.is_ascii_alphanumeric() || "_+-".contains(c))
                })
                .unwrap_or("");
            format!(
                "`{}`{about}\n{f}{ext}\n{code}\n{f}\n{}",
                note_location(n),
                n.body.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Text to paste into a terminal: no control characters (an ESC could end the bracketed paste
/// early and run what follows), and no line break at the end, which a program would take as Enter.
pub fn for_terminal(text: &str) -> String {
    let unix = text.replace("\r\n", "\n").replace('\r', "\n");
    let kept: String = unix
        .chars()
        .filter(|&c| !matches!(c, '\x00'..='\x08' | '\x0b'..='\x1f' | '\x7f'))
        .collect();
    kept.trim_end_matches('\n').to_string()
}

#[cfg(test)]
mod tests;
