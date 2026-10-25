//! The code view's editing commands, worked out on the text alone: ⌘/ comments by the language's
//! own style, ⇧⌘K, ⌥↑/⌥↓, ⇧⌥↑/⇧⌥↓, ⌘↵/⇧⌘↵ on whole lines, ⌘L, ⌘D, and the bracket that matches
//! the one at the caret (⇧⌘\ jumps to it). Each edit is one replacement of a run of whole lines,
//! so it undoes in one step, with every selection carried along.

use std::ops::Range;

use gpui_kit::component::input::{Rope, RopeExt as _};

/// How a language comments a line out: a token before it, or a pair around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Comment {
    Line(&'static str),
    Block(&'static str, &'static str),
}

/// By the highlighter's language name; None where there are no comments (text, diff).
pub(crate) fn comment_for(lang: &str) -> Option<Comment> {
    Some(match lang {
        "c" | "cpp" | "csharp" | "go" | "java" | "javascript" | "json" | "kotlin" | "php"
        | "proto" | "rust" | "scala" | "swift" | "tsx" | "typescript" | "zig" => {
            Comment::Line("//")
        }
        "bash" | "cmake" | "elixir" | "graphql" | "make" | "python" | "ruby" | "toml" | "yaml" => {
            Comment::Line("#")
        }
        "lua" => Comment::Line("--"),
        "css" => Comment::Block("/*", "*/"),
        "astro" | "ejs" | "erb" | "html" | "markdown" | "svelte" => Comment::Block("<!--", "-->"),
        _ => return None,
    })
}

/// One replacement of whole lines: `span` of the text becomes `text`, then `selections` are
/// set, the active one first.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LineEdit {
    pub span: Range<usize>,
    pub text: String,
    pub selections: Vec<Range<usize>>,
}

/// The rows a selection covers; one ending at a line's start leaves that line out.
fn rows_of(text: &Rope, sel: &Range<usize>) -> Range<usize> {
    let first = text.offset_to_point(sel.start).row;
    let end = text.offset_to_point(sel.end);
    let last = if sel.end > sel.start && end.column == 0 && end.row > first {
        end.row - 1
    } else {
        end.row
    };
    first..last + 1
}

/// The rows the selections cover, as runs of adjacent rows in order.
fn row_runs(text: &Rope, selections: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut rows: Vec<Range<usize>> = selections.iter().map(|s| rows_of(text, s)).collect();
    rows.sort_by_key(|r| r.start);
    let mut runs: Vec<Range<usize>> = Vec::new();
    for r in rows {
        match runs.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => runs.push(r),
        }
    }
    runs
}

fn line(text: &Rope, row: usize) -> String {
    text.slice_line(row).to_string()
}

fn indent_len(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// Where `offset` goes once `edits` (sorted, apart, each a range and its new length) are made.
/// Sticky right, it ends past what's inserted at it, as typing does; left, before it.
fn map(offset: usize, edits: &[(Range<usize>, usize)], right: bool) -> usize {
    let mut delta = 0isize;
    for (r, n) in edits {
        let n = *n as isize;
        if r.is_empty() {
            if offset < r.start || (offset == r.start && !right) {
                break;
            }
            delta += n;
            continue;
        }
        if offset <= r.start {
            break;
        }
        if offset >= r.end {
            delta += n - r.len() as isize;
            continue;
        }
        // Inside what was replaced: to its start, or past its replacement.
        return (r.start as isize + delta + if right { n } else { 0 }) as usize;
    }
    (offset as isize + delta) as usize
}

/// `edits` (sorted, apart) made as one replacement of what they span, the selections mapped.
fn line_edit(
    text: &Rope,
    edits: Vec<(Range<usize>, String)>,
    selections: &[Range<usize>],
) -> Option<LineEdit> {
    let span = edits.first()?.0.start..edits.last()?.0.end;
    let mut out = String::new();
    let mut at = span.start;
    for (r, new) in &edits {
        out.push_str(&text.slice(at..r.start).to_string());
        out.push_str(new);
        at = r.end;
    }
    let lens: Vec<(Range<usize>, usize)> =
        edits.iter().map(|(r, n)| (r.clone(), n.len())).collect();
    let selections = selections
        .iter()
        .map(|s| {
            if s.is_empty() {
                let at = map(s.start, &lens, true);
                at..at
            } else {
                map(s.start, &lens, false)..map(s.end, &lens, true)
            }
        })
        .collect();
    Some(LineEdit {
        span,
        text: out,
        selections,
    })
}

/// ⌘/: comments the selected lines out, or back in when every one is commented. Blank lines are
/// left alone unless all are blank; the token goes at the least indentation, as Monaco's does.
pub(crate) fn toggle_comment(
    text: &Rope,
    selections: &[Range<usize>],
    comment: Comment,
) -> Option<LineEdit> {
    let rows: Vec<usize> = row_runs(text, selections).into_iter().flatten().collect();
    let lines: Vec<(usize, String)> = rows.iter().map(|&r| (r, line(text, r))).collect();
    let all_blank = lines.iter().all(|(_, l)| l.trim().is_empty());
    let picked: Vec<&(usize, String)> = lines
        .iter()
        .filter(|(_, l)| all_blank || !l.trim().is_empty())
        .collect();
    let commented = |l: &str| {
        let t = l.trim();
        match comment {
            Comment::Line(token) => t.starts_with(token),
            Comment::Block(open, close) => {
                t.len() >= open.len() + close.len() && t.starts_with(open) && t.ends_with(close)
            }
        }
    };
    let undo = picked.iter().all(|(_, l)| commented(l));
    let indent = picked.iter().map(|(_, l)| indent_len(l)).min()?;
    let mut edits = Vec::new();
    for (row, l) in picked {
        let start = text.line_start_offset(*row);
        let lead = indent_len(l);
        match (comment, undo) {
            (Comment::Line(token), true) => {
                let after = lead + token.len();
                let space = usize::from(l[after..].starts_with(' '));
                edits.push((start + lead..start + after + space, String::new()));
            }
            (Comment::Line(token), false) => {
                edits.push((start + indent..start + indent, format!("{token} ")));
            }
            (Comment::Block(open, close), true) => {
                let end = l.trim_end().len();
                let inner = lead + open.len()..end - close.len();
                let space_before = usize::from(l[inner.clone()].starts_with(' '));
                let space_after = usize::from(
                    inner.end > inner.start + space_before && l[inner.clone()].ends_with(' '),
                );
                edits.push((
                    start + lead..start + inner.start + space_before,
                    String::new(),
                ));
                edits.push((start + inner.end - space_after..start + end, String::new()));
            }
            (Comment::Block(open, close), false) => {
                edits.push((start + indent..start + indent, format!("{open} ")));
                let end = start + l.len();
                edits.push((end..end, format!(" {close}")));
            }
        }
    }
    line_edit(text, edits, selections)
}

/// ⇧⌘K: the selected lines go; each caret lands on the line that follows (the one before at the
/// end), at its column as far as that line reaches.
pub(crate) fn delete_lines(text: &Rope, selections: &[Range<usize>]) -> Option<LineEdit> {
    let runs = row_runs(text, selections);
    let rows = text.lines_len();
    let edits: Vec<(Range<usize>, String)> = runs
        .iter()
        .map(|run| {
            let range = if run.end < rows {
                text.line_start_offset(run.start)..text.line_start_offset(run.end)
            } else if run.start > 0 {
                text.line_end_offset(run.start - 1)..text.len()
            } else {
                0..text.len()
            };
            (range, String::new())
        })
        .collect();
    let mut edit = line_edit(text, edits.clone(), &[])?;
    let lens: Vec<(Range<usize>, usize)> = edits.iter().map(|(r, _)| (r.clone(), 0)).collect();
    edit.selections = selections
        .iter()
        .map(|s| {
            let caret = text.offset_to_point(s.end);
            let run = runs.iter().find(|r| r.contains(&rows_of(text, s).start));
            let row = match run {
                Some(r) if r.end < rows => r.end,
                Some(r) if r.start > 0 => r.start - 1,
                _ => return 0..0,
            };
            let at = map(text.line_start_offset(row), &lens, false)
                + caret.column.min(text.line_len(row));
            at..at
        })
        .collect();
    Some(edit)
}

/// ⌥↑ / ⌥↓: the selected lines swap with the one above (`up`) or below, selections along.
/// Nothing moves when a run is already at that end.
pub(crate) fn move_lines(text: &Rope, selections: &[Range<usize>], up: bool) -> Option<LineEdit> {
    let runs = row_runs(text, selections);
    let rows = text.lines_len();
    if runs.is_empty() || (up && runs[0].start == 0) || (!up && runs.last()?.end >= rows) {
        return None;
    }
    let mut edits = Vec::new();
    // How far each run's text moves.
    let mut shifts = Vec::new();
    for run in &runs {
        let block = text
            .slice(text.line_start_offset(run.start)..text.line_end_offset(run.end - 1))
            .to_string();
        if up {
            let other = line(text, run.start - 1);
            let range = text.line_start_offset(run.start - 1)..text.line_end_offset(run.end - 1);
            edits.push((range, format!("{block}\n{other}")));
            shifts.push(-(other.len() as isize + 1));
        } else {
            let other = line(text, run.end);
            let range = text.line_start_offset(run.start)..text.line_end_offset(run.end);
            edits.push((range, format!("{other}\n{block}")));
            shifts.push(other.len() as isize + 1);
        }
    }
    let mut edit = line_edit(text, edits, &[])?;
    // A selection taking the last line's newline along has none left at the end.
    let len = text.len();
    edit.selections = selections
        .iter()
        .map(|s| {
            let row = rows_of(text, s).start;
            let i = runs.iter().position(|r| r.contains(&row)).unwrap_or(0);
            let at = |o: usize| ((o as isize + shifts[i]) as usize).min(len);
            at(s.start)..at(s.end)
        })
        .collect();
    Some(edit)
}

/// ⇧⌥↑ / ⇧⌥↓: the selected lines copied above (`up`) or below; selections on the copy, which
/// is the upper one going up and the lower one going down.
pub(crate) fn copy_lines(text: &Rope, selections: &[Range<usize>], up: bool) -> Option<LineEdit> {
    let runs = row_runs(text, selections);
    let mut edits = Vec::new();
    let mut shifts = Vec::new();
    // What earlier copies add in front of a run.
    let mut before = 0;
    for run in &runs {
        let (start, end) = (
            text.line_start_offset(run.start),
            text.line_end_offset(run.end - 1),
        );
        let block = text.slice(start..end).to_string();
        let added = block.len() + 1;
        if up {
            edits.push((start..start, format!("{block}\n")));
            shifts.push(before);
        } else {
            edits.push((end..end, format!("\n{block}")));
            shifts.push(before + added);
        }
        before += added;
    }
    let mut edit = line_edit(text, edits, &[])?;
    edit.selections = selections
        .iter()
        .map(|s| {
            let row = rows_of(text, s).start;
            let i = runs.iter().position(|r| r.contains(&row)).unwrap_or(0);
            s.start + shifts[i]..s.end + shifts[i]
        })
        .collect();
    Some(edit)
}

/// ⌘↵ / ⇧⌘↵: a new line below (`below`) or above each caret's line, indented as that line;
/// the carets go to it.
pub(crate) fn insert_line(
    text: &Rope,
    selections: &[Range<usize>],
    below: bool,
) -> Option<LineEdit> {
    let mut rows: Vec<usize> = selections
        .iter()
        .map(|s| {
            text.offset_to_point(if below { s.end } else { s.start })
                .row
        })
        .collect();
    rows.sort_unstable();
    rows.dedup();
    let edits: Vec<(Range<usize>, String)> = rows
        .iter()
        .map(|&row| {
            let l = line(text, row);
            let indent = &l[..indent_len(&l)];
            if below {
                let end = text.line_end_offset(row);
                (end..end, format!("\n{indent}"))
            } else {
                let start = text.line_start_offset(row);
                (start..start, format!("{indent}\n"))
            }
        })
        .collect();
    let mut edit = line_edit(text, edits.clone(), &[])?;
    // Each caret at the end of its new line's indentation.
    let mut added = 0;
    edit.selections = edits
        .iter()
        .map(|(r, new)| {
            let at = r.start + added + if below { new.len() } else { new.len() - 1 };
            added += new.len();
            at..at
        })
        .collect();
    Some(edit)
}

/// ⌘L: each selection grows to its whole lines, and by the next line once it is that.
pub(crate) fn select_lines(text: &Rope, selections: &[Range<usize>]) -> Vec<Range<usize>> {
    let rows = text.lines_len();
    let end_of = |row: usize| {
        if row + 1 < rows {
            text.line_start_offset(row + 1)
        } else {
            text.len()
        }
    };
    selections
        .iter()
        .map(|s| {
            let r = rows_of(text, s);
            let whole = text.line_start_offset(r.start)..end_of(r.end - 1);
            if *s == whole && !s.is_empty() {
                whole.start..end_of(r.end.min(rows - 1))
            } else {
                whole
            }
        })
        .collect()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// What ⌘D does next.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Next {
    /// The caret had no selection: the word around it, selected alone.
    Word(Range<usize>),
    /// The next place the selected text is, after the last selection and round from the top.
    Add(Range<usize>),
}

/// ⌘D: the word at the caret, then each next place it is: case-sensitive, and whole words only
/// when the selection is one.
pub(crate) fn next_occurrence(text: &Rope, selections: &[Range<usize>]) -> Option<Next> {
    let active = selections.first()?;
    if active.is_empty() {
        let at = active.start;
        let word = text
            .word_range(at)
            .or_else(|| at.checked_sub(1).and_then(|a| text.word_range(a)))?;
        return Some(Next::Word(word));
    }
    let all = text.to_string();
    let needle = &all[active.clone()];
    let whole = needle.chars().all(is_word)
        && !all[..active.start].chars().next_back().is_some_and(is_word)
        && !all[active.end..].chars().next().is_some_and(is_word);
    let from = selections.iter().map(|s| s.end).max()?;
    let fits = |at: usize| {
        let end = at + needle.len();
        !whole
            || (!all[..at].chars().next_back().is_some_and(is_word)
                && !all[end..].chars().next().is_some_and(is_word))
    };
    let taken = |r: &Range<usize>| {
        selections
            .iter()
            .any(|s| s.start < r.end && r.start < s.end)
    };
    let after = all[from..].match_indices(needle).map(|(i, _)| from + i);
    let before = all[..from].match_indices(needle).map(|(i, _)| i);
    after
        .chain(before)
        .filter(|&at| fits(at))
        .map(|at| at..at + needle.len())
        .find(|r| !taken(r))
        .map(Next::Add)
}

/// Brackets are matched this far from the caret at most.
const BRACKET_REACH: usize = 200_000;

const PAIRS: [(u8, u8); 3] = [(b'(', b')'), (b'[', b']'), (b'{', b'}')];

/// The bracket of `pair` matching the one at `at`, scanning forward from an opener or back from
/// a closer; brackets where `code` says no (in a string or comment) don't count.
fn match_at(text: &Rope, at: usize, code: &dyn Fn(usize) -> bool) -> Option<usize> {
    let b = text.get_byte(at)?;
    let &(open, close) = PAIRS.iter().find(|(o, c)| *o == b || *c == b)?;
    if !code(at) {
        return None;
    }
    let mut depth = 0usize;
    if b == open {
        let end = (at + 1 + BRACKET_REACH).min(text.len());
        let window = text.slice(at + 1..end).to_string();
        for (i, &c) in window.as_bytes().iter().enumerate() {
            if (c == open || c == close) && code(at + 1 + i) {
                if c == open {
                    depth += 1;
                } else if depth == 0 {
                    return Some(at + 1 + i);
                } else {
                    depth -= 1;
                }
            }
        }
    } else {
        let start = at.saturating_sub(BRACKET_REACH);
        let window = text.slice(start..at).to_string();
        for (i, &c) in window.as_bytes().iter().enumerate().rev() {
            if (c == open || c == close) && code(start + i) {
                if c == close {
                    depth += 1;
                } else if depth == 0 {
                    return Some(start + i);
                } else {
                    depth -= 1;
                }
            }
        }
    }
    None
}

/// The bracket beside the caret (the one before it first) and the one matching it.
pub(crate) fn bracket_pair(
    text: &Rope,
    caret: usize,
    code: &dyn Fn(usize) -> bool,
) -> Option<(usize, usize)> {
    caret
        .checked_sub(1)
        .and_then(|b| Some((b, match_at(text, b, code)?)))
        .or_else(|| Some((caret, match_at(text, caret, code)?)))
}

/// ⇧⌘\: where the caret goes. Beside a bracket, to the other side of its match (pressed again,
/// back); elsewhere, to the closing bracket of the pair around it.
pub(crate) fn jump_to_bracket(
    text: &Rope,
    caret: usize,
    code: &dyn Fn(usize) -> bool,
) -> Option<usize> {
    if let Some(before) = caret.checked_sub(1)
        && let Some(m) = match_at(text, before, code)
    {
        return Some(m + 1);
    }
    if let Some(m) = match_at(text, caret, code) {
        return Some(m);
    }
    // The innermost opener before the caret left unclosed.
    let start = caret.saturating_sub(BRACKET_REACH);
    let window = text.slice(start..caret).to_string();
    let mut depth = [0usize; 3];
    for (i, &c) in window.as_bytes().iter().enumerate().rev() {
        let Some(k) = PAIRS.iter().position(|(o, cl)| *o == c || *cl == c) else {
            continue;
        };
        if !code(start + i) {
            continue;
        }
        if c == PAIRS[k].1 {
            depth[k] += 1;
        } else if depth[k] > 0 {
            depth[k] -= 1;
        } else {
            return match_at(text, start + i, code);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text with `|` carets and `[…]` selections taken out, and those selections.
    fn parse(marked: &str) -> (Rope, Vec<Range<usize>>) {
        let mut text = String::new();
        let mut sels = Vec::new();
        let mut open = None;
        for c in marked.chars() {
            match c {
                '|' => sels.push(text.len()..text.len()),
                '[' => open = Some(text.len()),
                ']' => sels.push(open.take().unwrap()..text.len()),
                c => text.push(c),
            }
        }
        (Rope::from(text.as_str()), sels)
    }

    /// The edit made, with its selections marked back in.
    fn apply(text: &Rope, edit: &LineEdit) -> String {
        let mut out = text.to_string();
        out.replace_range(edit.span.clone(), &edit.text);
        let mut marks: Vec<(usize, char)> = Vec::new();
        for s in &edit.selections {
            if s.is_empty() {
                marks.push((s.start, '|'));
            } else {
                marks.push((s.end, ']'));
                marks.push((s.start, '['));
            }
        }
        marks.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (at, c) in marks {
            out.insert(at, c);
        }
        out
    }

    fn run(marked: &str, f: impl Fn(&Rope, &[Range<usize>]) -> Option<LineEdit>) -> String {
        let (text, sels) = parse(marked);
        match f(&text, &sels) {
            Some(edit) => apply(&text, &edit),
            None => "none".into(),
        }
    }

    #[test]
    fn comments_go_in_at_the_least_indent_and_come_out() {
        let rust = |t: &Rope, s: &[Range<usize>]| toggle_comment(t, s, Comment::Line("//"));
        assert_eq!(
            run("fn a() {\n    [let x = 1;\n\n  let y]= 2;\n}", rust),
            "fn a() {\n  //   [let x = 1;\n\n  // let y]= 2;\n}"
        );
        assert_eq!(
            run("    [// let x = 1;\n\n  //let y]= 2;", rust),
            "    [let x = 1;\n\n  let y]= 2;"
        );
        // A selection ending at a line's start leaves that line out.
        assert_eq!(run("[a\n]b", rust), "[// a\n]b");
        // A caret on a blank line comments it.
        assert_eq!(run("a\n  |\nb", rust), "a\n  // |\nb");
        // Two carets, two lines apart: only theirs.
        assert_eq!(run("|a\nb\n|c", rust), "// |a\nb\n// |c");
        let html =
            |t: &Rope, s: &[Range<usize>]| toggle_comment(t, s, Comment::Block("<!--", "-->"));
        assert_eq!(run("  <p>|hi</p>", html), "  <!-- <p>|hi</p> -->");
        assert_eq!(run("  <!-- <p>|hi</p> -->", html), "  <p>|hi</p>");
        assert_eq!(comment_for("python"), Some(Comment::Line("#")));
        assert_eq!(comment_for("text"), None);
    }

    #[test]
    fn delete_move_copy_and_insert_lines() {
        assert_eq!(run("a\nb|c\nd", delete_lines), "a\nd|");
        assert_eq!(run("a\nbbb|\nc", delete_lines), "a\nc|");
        assert_eq!(run("a\nb|", delete_lines), "a|");
        assert_eq!(run("only|", delete_lines), "|");
        assert_eq!(run("|a\nb\n|c\nd", delete_lines), "|b\n|d");

        assert_eq!(run("a\nb|\nc", |t, s| move_lines(t, s, true)), "b|\na\nc");
        assert_eq!(run("a\nb|\nc", |t, s| move_lines(t, s, false)), "a\nc\nb|");
        assert_eq!(run("a|\nb", |t, s| move_lines(t, s, true)), "none");
        assert_eq!(
            run("a\n[b\nc]\nd", |t, s| move_lines(t, s, true)),
            "[b\nc]\na\nd"
        );
        assert_eq!(
            run("a\n[b\n]c", |t, s| move_lines(t, s, false)),
            "a\nc\n[b]"
        );

        assert_eq!(run("a\nb|c", |t, s| copy_lines(t, s, false)), "a\nbc\nb|c");
        assert_eq!(run("a\nb|c", |t, s| copy_lines(t, s, true)), "a\nb|c\nbc");
        assert_eq!(
            run("|a\n|b", |t, s| copy_lines(t, s, false)),
            "a\nb\n|a\n|b"
        );

        assert_eq!(
            run("  a|b\nc", |t, s| insert_line(t, s, true)),
            "  ab\n  |\nc"
        );
        assert_eq!(
            run("  a|b\nc", |t, s| insert_line(t, s, false)),
            "  |\n  ab\nc"
        );
    }

    #[test]
    fn select_line_grows_by_lines() {
        let (text, sels) = parse("a\nb|b\nc");
        let one = select_lines(&text, &sels);
        assert_eq!(one, vec![Range { start: 2, end: 5 }]);
        assert_eq!(select_lines(&text, &one), vec![Range { start: 2, end: 6 }]);
        // The last line, to the end.
        let (text, sels) = parse("a\n|b");
        assert_eq!(select_lines(&text, &sels), vec![Range { start: 2, end: 3 }]);
    }

    #[test]
    fn next_occurrence_selects_the_word_then_each_next_one() {
        let (text, sels) = parse("let fo|o = foo + food;\nfoo");
        assert_eq!(next_occurrence(&text, &sels), Some(Next::Word(4..7)));
        // A whole word skips "food" and wraps round.
        assert_eq!(
            next_occurrence(&text, &[4..7, 10..13]),
            Some(Next::Add(22..25))
        );
        assert_eq!(
            next_occurrence(&text, &[10..13, 22..25]),
            Some(Next::Add(4..7))
        );
        // Part of a word matches inside words.
        let part = Range { start: 4, end: 6 };
        assert_eq!(next_occurrence(&text, &[part]), Some(Next::Add(10..12)));
        assert_eq!(next_occurrence(&text, &[4..7, 10..13, 22..25]), None);
    }

    #[test]
    fn brackets_match_outside_strings_and_jump_back_and_forth() {
        let text = Rope::from("f(a[1], \")\", { b })");
        let code_all = |_: usize| true;
        // In the string: the ")" at 9.
        let code = |at: usize| at != 9;
        assert_eq!(bracket_pair(&text, 2, &code), Some((1, 18)));
        assert_eq!(bracket_pair(&text, 2, &code_all), Some((1, 9)));
        assert_eq!(bracket_pair(&text, 3, &code), Some((3, 5)));
        assert_eq!(bracket_pair(&text, 19, &code), Some((18, 1)));
        assert_eq!(bracket_pair(&text, 8, &code), None);
        // After "(", to after ")" and back.
        assert_eq!(jump_to_bracket(&text, 2, &code), Some(19));
        assert_eq!(jump_to_bracket(&text, 19, &code), Some(2));
        // Inside "{ b }", to its closer.
        assert_eq!(jump_to_bracket(&text, 15, &code), Some(17));
    }
}
