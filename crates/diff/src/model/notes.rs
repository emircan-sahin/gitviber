//! What a diff says instead of, or above, its lines: a binary or too large file, a change git's
//! text diff can't show (a mode, a submodule's own changes), a newline at the end coming or going.

use super::{Row, SideText};
use crate::source::FileFacts;

fn kind(mode: &str) -> Option<&'static str> {
    match mode {
        "100644" | "100755" => Some("file"),
        "120000" => Some("symlink"),
        "160000" => Some("submodule"),
        _ => None,
    }
}

/// A submodule with changes of its own inside it.
pub fn changed_inside(file: &FileFacts) -> bool {
    file.submodule
        .as_deref()
        .is_some_and(|s| s.get(2..).is_some_and(|rest| rest.contains(['M', 'U'])))
}

/// What a text diff can't show about a change: its mode or type, or a submodule's own changes.
fn file_note(file: &FileFacts) -> Option<String> {
    if let Some(mode) = &file.mode {
        let mut it = mode.split(" → ").map(kind);
        return Some(match (it.next().flatten(), it.next().flatten()) {
            (Some(from), Some(to)) if from != to => {
                format!("Changed from a {from} to a {to} ({mode})")
            }
            _ => format!("File mode changed: {mode}"),
        });
    }
    changed_inside(file)
        .then(|| "This submodule has changes inside it; commit them in the submodule".into())
}

/// Why a diff shows no lines, when it can't or has none to show.
pub fn placeholder(
    old: &SideText,
    new: &SideText,
    rows: &[Row],
    whitespace_hidden: bool,
    file: &FileFacts,
) -> Option<String> {
    if let Some(lfs) = new.lfs_missing.as_ref().or(old.lfs_missing.as_ref()) {
        return Some(lfs.clone());
    }
    if old.binary || new.binary {
        return Some("Binary file".into());
    }
    if old.too_large || new.too_large {
        return Some("File is too large to display".into());
    }
    if rows.iter().all(|r| r.k == 0) {
        // First: an added or renamed file whose mode changed isn't only that.
        if let Some(note) = file_note(file) {
            return Some(note);
        }
        return Some(
            if whitespace_hidden {
                "Only whitespace changed (hidden)"
            } else if old.exists != new.exists {
                // Added or deleted with no line changed: there were none.
                "Empty file"
            } else if file.status == "R" {
                "Renamed without changes"
            } else if file.status == "C" {
                "Copied without changes"
            } else {
                "No textual changes"
            }
            .into(),
        );
    }
    None
}

/// Why a last line shows removed and added unchanged: the newline after it came or went.
fn newline_note(old: &SideText, new: &SideText, rows: &[Row]) -> Option<&'static str> {
    if old.is_empty() || new.is_empty() || old.ends_with_newline == new.ends_with_newline {
        return None;
    }
    // Rows hold every line, so the last one naming the new side is its last line; ignoring
    // whitespace can leave it unchanged.
    let last = rows.iter().rev().find(|r| r.n != 0)?;
    if last.k == 0 {
        return None;
    }
    Some(if new.ends_with_newline {
        "Newline added at end of file"
    } else {
        "No newline at end of file"
    })
}

/// What the header says about a change shown as text.
pub fn header_note(
    old: &SideText,
    new: &SideText,
    rows: &[Row],
    whitespace_hidden: bool,
    eol_only: bool,
    file: &FileFacts,
) -> Option<String> {
    let text = if eol_only {
        Some("Only line endings changed")
    } else {
        newline_note(old, new, rows).or(whitespace_hidden.then_some("Whitespace changes hidden"))
    };
    let parts: Vec<String> = [file_note(file), text.map(str::to_string)]
        .into_iter()
        .flatten()
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(k: u8, o: u32, n: u32) -> Row {
        Row { k, o, n, e: vec![] }
    }

    #[test]
    fn says_why_nothing_shows() {
        let a = SideText::from_text("x\n");
        let missing = SideText::default();
        let facts = FileFacts::default();
        assert_eq!(
            placeholder(&a, &a, &[row(0, 1, 1)], false, &facts).as_deref(),
            Some("No textual changes")
        );
        assert_eq!(
            placeholder(&missing, &SideText::from_text(""), &[], false, &facts).as_deref(),
            Some("Empty file")
        );
        let chmod = FileFacts {
            mode: Some("100644 → 100755".into()),
            ..Default::default()
        };
        assert_eq!(
            placeholder(&a, &a, &[], false, &chmod).as_deref(),
            Some("File mode changed: 100644 → 100755")
        );
        let link = FileFacts {
            mode: Some("100644 → 120000".into()),
            ..Default::default()
        };
        assert_eq!(
            placeholder(&a, &a, &[], false, &link).as_deref(),
            Some("Changed from a file to a symlink (100644 → 120000)")
        );
        assert_eq!(placeholder(&a, &a, &[row(1, 0, 1)], false, &facts), None);
    }

    #[test]
    fn notes_a_newline_at_the_end() {
        let (a, b) = (SideText::from_text("x"), SideText::from_text("x\n"));
        let rows = [row(2, 1, 0), row(1, 0, 1)];
        assert_eq!(
            header_note(&a, &b, &rows, false, false, &FileFacts::default()).as_deref(),
            Some("Newline added at end of file")
        );
        assert_eq!(
            header_note(&b, &a, &rows, true, false, &FileFacts::default()).as_deref(),
            Some("No newline at end of file")
        );
        assert_eq!(
            header_note(&a, &a, &rows, false, true, &FileFacts::default()).as_deref(),
            Some("Only line endings changed")
        );
        let sub = FileFacts {
            submodule: Some("S.M.".into()),
            ..Default::default()
        };
        assert!(changed_inside(&sub));
    }
}
