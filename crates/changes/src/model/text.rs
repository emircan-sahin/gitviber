//! The words and lines the panel works out: a .gitignore line for a path, conflict blocks, a
//! suggested message read out of an agent's answer, the agent's command line, a stash's name.

use std::collections::BTreeMap;

/// A .gitignore line matching exactly this repo-relative path. The leading slash anchors it (so
/// `#` and `!` are never leading); glob characters and trailing spaces are escaped.
pub fn ignore_pattern(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len() + 2);
    for c in path.chars() {
        if matches!(c, '\\' | '[' | ']' | '*' | '?') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    let body = escaped.trim_end_matches(' ');
    format!("/{body}{}", "\\ ".repeat(escaped.len() - body.len()))
}

/// One conflict block: each side's lines, the base's when diff3 wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub ours: Vec<String>,
    pub base: Option<Vec<String>>,
    pub theirs: Vec<String>,
    pub ours_label: String,
    pub theirs_label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Text(Vec<String>),
    Conflict(Block),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub segments: Vec<Segment>,
    pub trailing_newline: bool,
}

fn marker(line: &str, m: &str) -> Option<String> {
    let l = line.strip_suffix('\r').unwrap_or(line);
    if l == m || l.starts_with(&format!("{m} ")) {
        Some(l[m.len()..].trim().to_string())
    } else {
        None
    }
}

/// Splits a file with conflict markers into text runs and conflict blocks. None for markers left
/// open: don't guess.
pub fn parse_conflicts(text: &str) -> Option<Parsed> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    let trailing_newline = lines.last() == Some(&"");
    if trailing_newline {
        lines.pop();
    }
    #[derive(PartialEq)]
    enum Part {
        Ours,
        Base,
        Theirs,
    }
    let mut segments = vec![];
    let mut buf: Vec<String> = vec![];
    let mut block: Option<Block> = None;
    let mut part = Part::Ours;
    for line in lines {
        let Some(b) = block.as_mut() else {
            match marker(line, "<<<<<<<") {
                None => buf.push(line.to_string()),
                Some(label) => {
                    if !buf.is_empty() {
                        segments.push(Segment::Text(std::mem::take(&mut buf)));
                    }
                    block = Some(Block {
                        ours: vec![],
                        base: None,
                        theirs: vec![],
                        ours_label: label,
                        theirs_label: String::new(),
                    });
                    part = Part::Ours;
                }
            }
            continue;
        };
        if part == Part::Ours && marker(line, "|||||||").is_some() {
            b.base = Some(vec![]);
            part = Part::Base;
        } else if part != Part::Theirs && marker(line, "=======").as_deref() == Some("") {
            part = Part::Theirs;
        } else if part == Part::Theirs && marker(line, ">>>>>>>").is_some() {
            b.theirs_label = marker(line, ">>>>>>>").unwrap_or_default();
            segments.push(Segment::Conflict(block.take().expect("open block")));
        } else {
            match part {
                Part::Ours => b.ours.push(line.to_string()),
                Part::Base => b.base.get_or_insert_default().push(line.to_string()),
                Part::Theirs => b.theirs.push(line.to_string()),
            }
        }
    }
    if block.is_some() {
        return None;
    }
    if !buf.is_empty() {
        segments.push(Segment::Text(buf));
    }
    Some(Parsed {
        segments,
        trailing_newline,
    })
}

/// Whether a file still has conflict blocks, or a start marker left open.
pub fn has_conflict_markers(text: &str) -> bool {
    parse_conflicts(text)
        .is_none_or(|p| p.segments.iter().any(|s| matches!(s, Segment::Conflict(_))))
}

/// A model's answer as summary and description. Models wrap it anyway at times: a code fence,
/// quotes, a "Subject:" label or emphasis on the first line. None when it's empty.
pub fn parse_suggestion(output: &str) -> Option<(String, String)> {
    let mut text = output
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string();
    // A fenced block anywhere ("Here's a message:\n```\n…\n```") is the message.
    if let Some(inner) = fenced(&text) {
        text = inner.trim().to_string();
    }
    if let Some(q) = text
        .chars()
        .next()
        .filter(|c| matches!(c, '"' | '\'' | '`'))
        && text.len() >= 2
        && text.ends_with(q)
    {
        let inner = &text[1..text.len() - 1];
        if !inner.contains(q) {
            text = inner.trim().to_string();
        }
    }
    let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
    let hashes = first.len() - first.trim_start_matches('#').len();
    let mut summary = if hashes > 0 && first[hashes..].starts_with(char::is_whitespace) {
        first[hashes..].trim_start()
    } else {
        first
    }
    .to_string();
    for w in ["**", "__"] {
        if summary.len() > 4 && summary.starts_with(w) && summary.ends_with(w) {
            summary = summary[2..summary.len() - 2].to_string();
        }
    }
    let lower = summary.to_lowercase();
    for label in ["summary:", "subject:", "title:", "commit message:"] {
        if lower.starts_with(label) {
            summary = summary[label.len()..].to_string();
            break;
        }
    }
    let summary = summary.trim().to_string();
    let mut body = rest.trim().to_string();
    let lower = body.to_lowercase();
    for label in ["body:", "description:"] {
        if lower.starts_with(label) {
            body = body[label.len()..].trim_start().to_string();
            break;
        }
    }
    (!summary.is_empty()).then_some((summary, body))
}

/// The text between a fence line and its closing fence, when the answer has one.
fn fenced(text: &str) -> Option<&str> {
    let lines: Vec<(usize, &str)> = text
        .split('\n')
        .scan(0, |at, l| {
            let start = *at;
            *at += l.len() + 1;
            Some((start, l))
        })
        .collect();
    let open = lines
        .iter()
        .position(|(_, l)| l.starts_with("```") || l.starts_with("~~~"))?;
    let (start, line) = lines[open];
    let fence: String = line
        .chars()
        .take_while(|c| *c == '`' || *c == '~')
        .collect();
    let close = lines
        .iter()
        .skip(open + 1)
        .position(|(_, l)| l.trim_end() == fence)?
        + open
        + 1;
    let inner_start = start + line.len() + 1;
    let inner_end = lines[close].0.saturating_sub(1).max(inner_start);
    Some(&text[inner_start.min(text.len())..inner_end.min(text.len())])
}

/// The commit box's command as run: a preset's with its lean, model and effort flags (the
/// user's picks in `models` / `efforts`, else its defaults); a custom one carries its own.
pub fn command_line(
    command: &str,
    models: &BTreeMap<String, String>,
    efforts: &BTreeMap<String, String>,
) -> String {
    gitviber_ui::settings::command_line(command, models, efforts, false, true)
}

/// The program a template runs, by name ("claude" from "~/.local/bin/claude -p").
pub fn program_of(command: &str) -> String {
    gitviber_ui::settings::program_of(command).to_string()
}

/// Sent ahead of the diff, word for word; Settings shows it.
pub const SUGGEST_PROMPT: &str = "Write a git commit message for this diff: a summary line under 72 characters, a blank line, then a short body saying what changed and why. Output only the commit message, with no quotes or code fences.";

/// "On main: message" → message and branch; "WIP on main: abc123 subject" keeps its subject.
pub fn describe_stash(message: &str) -> (String, Option<String>) {
    let (wip, rest) = if let Some(r) = message.strip_prefix("WIP on ") {
        (true, r)
    } else if let Some(r) = message.strip_prefix("On ") {
        (false, r)
    } else {
        return (message.to_string(), None);
    };
    let Some((branch, text)) = rest.split_once(": ") else {
        return (message.to_string(), None);
    };
    let text = if wip {
        let subject = text
            .split_once(' ')
            .filter(|(sha, _)| sha.chars().all(|c| c.is_ascii_hexdigit()))
            .map_or(text, |(_, s)| s);
        format!("WIP: {subject}")
    } else {
        text.to_string()
    };
    (text, Some(branch.to_string()))
}

/// GitHub reads a co-author only as "Name <email>".
pub fn is_co_author(s: &str) -> bool {
    let Some((name, email)) = s.strip_suffix('>').and_then(|r| r.split_once(" <")) else {
        return false;
    };
    let at = email
        .char_indices()
        .any(|(i, c)| c == '@' && i > 0 && i + 1 < email.len());
    !name.is_empty()
        && !name.contains(['<', '>', '\n'])
        && at
        && !email.contains(|c: char| c == '<' || c == '>' || c.is_whitespace())
}

/// "Ada <ada@x.dev>" → "Ada"; the whole text when there's no name before the email.
pub fn author_name(author: &str) -> &str {
    let name = author
        .strip_suffix('>')
        .and_then(|r| r.rfind('<').filter(|i| !r[i + 1..].contains('>')))
        .map_or(author, |i| author[..i].trim_end());
    if name.is_empty() { author } else { name }
}

/// A multi-line paste into Summary split as git reads a message: its first line in at the caret
/// (`from`..`to`), the rest starting the description, which it replaces while that's empty or
/// still `start_body`; then where the caret goes. None for one line.
pub fn paste_message(
    summary: &str,
    body: &str,
    start_body: &str,
    text: &str,
    from: usize,
    to: usize,
) -> Option<(String, String, usize)> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if !normalized.contains('\n') {
        return None;
    }
    // Leading blank lines go, as git drops them.
    let past_blank = |s: &str| -> usize {
        let ws = s.len() - s.trim_start().len();
        s[..ws].rfind('\n').map_or(0, |i| i + 1)
    };
    let lead = &normalized[past_blank(&normalized)..];
    let (first, rest) = lead.split_once('\n').unwrap_or((lead, ""));
    let more = rest[past_blank(rest)..].trim_end();
    let blank = body.trim().is_empty() || body == start_body;
    let new_body = if more.is_empty() {
        body.to_string()
    } else if blank {
        more.to_string()
    } else {
        format!("{more}\n\n{body}")
    };
    let (from, to) = (from.min(summary.len()), to.min(summary.len()));
    let (from, to) = (from.min(to), from.max(to));
    Some((
        format!("{}{first}{}", &summary[..from], &summary[to..]),
        new_body,
        from + first.len(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // As gitignore.test.ts.
    #[test]
    fn ignore_lines() {
        assert_eq!(ignore_pattern("dist/out.js"), "/dist/out.js");
        assert_eq!(ignore_pattern("#notes.md"), "/#notes.md");
        assert_eq!(ignore_pattern("!keep"), "/!keep");
        assert_eq!(ignore_pattern("app/[id].tsx"), "/app/\\[id\\].tsx");
        assert_eq!(ignore_pattern("a*b?.txt"), "/a\\*b\\?.txt");
        assert_eq!(ignore_pattern("back\\slash"), "/back\\\\slash");
        assert_eq!(ignore_pattern("name  "), "/name\\ \\ ");
        assert_eq!(ignore_pattern("a b"), "/a b");
    }

    // As conflicts.test.ts.
    #[test]
    fn conflict_markers() {
        assert!(has_conflict_markers(
            "a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> main\n"
        ));
        assert!(has_conflict_markers("a\n<<<<<<< HEAD\nb\n"));
        assert!(!has_conflict_markers("a\n=======\nb\n"));
        assert!(!has_conflict_markers("a\nb\n"));
        let p = parse_conflicts("x\n<<<<<<< HEAD\nm\n||||||| base\no\n=======\nt\n>>>>>>> f\ny")
            .unwrap();
        assert!(!p.trailing_newline);
        assert_eq!(p.segments.len(), 3);
        let Segment::Conflict(b) = &p.segments[1] else {
            panic!("a conflict")
        };
        assert_eq!(
            (
                b.ours.clone(),
                b.base.clone(),
                b.theirs.clone(),
                b.theirs_label.as_str()
            ),
            (
                vec!["m".into()],
                Some(vec!["o".into()]),
                vec!["t".into()],
                "f"
            )
        );
    }

    // As suggest.test.ts.
    #[test]
    fn suggestions() {
        let s = |summary: &str, body: &str| Some((summary.to_string(), body.to_string()));
        assert_eq!(
            parse_suggestion("Fix the thing\n\nIt was broken.\nNow it isn't.\n"),
            s("Fix the thing", "It was broken.\nNow it isn't.")
        );
        assert_eq!(
            parse_suggestion("  Only a summary  \n"),
            s("Only a summary", "")
        );
        assert_eq!(
            parse_suggestion("Summary\r\n\r\n\r\nBody\r\n"),
            s("Summary", "Body")
        );
        assert_eq!(
            parse_suggestion("Summary\n- one\n- two"),
            s("Summary", "- one\n- two")
        );
        let plain = s("feat: add x", "Because y.");
        assert_eq!(
            parse_suggestion("```\nfeat: add x\n\nBecause y.\n```"),
            plain
        );
        assert_eq!(
            parse_suggestion("Here's a message:\n\n```text\nfeat: add x\n\nBecause y.\n```\n"),
            plain
        );
        assert_eq!(parse_suggestion("\"feat: add x\n\nBecause y.\""), plain);
        assert_eq!(parse_suggestion("**feat: add x**\n\nBecause y."), plain);
        assert_eq!(parse_suggestion("# feat: add x\n\nBecause y."), plain);
        assert_eq!(
            parse_suggestion("Subject: feat: add x\n\nBody: Because y."),
            plain
        );
        assert_eq!(
            parse_suggestion("Rename \"a\" to \"b\""),
            s("Rename \"a\" to \"b\"", "")
        );
        assert_eq!(
            parse_suggestion("Use `x` over `y`"),
            s("Use `x` over `y`", "")
        );
        assert_eq!(parse_suggestion(""), None);
        assert_eq!(parse_suggestion("  \n\n "), None);
        assert_eq!(parse_suggestion("```\n\n```"), None);
        assert_eq!(program_of("~/.local/bin/claude -p"), "claude");
    }

    #[test]
    fn preset_command_lines() {
        let lean =
            "--strict-mcp-config --no-session-persistence --disable-slash-commands --tools \"\"";
        let m = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>()
        };
        let none = m(&[]);
        assert_eq!(
            command_line(" claude -p ", &none, &none),
            format!("claude -p {lean} --model sonnet --effort medium")
        );
        assert_eq!(
            command_line("claude -p", &none, &m(&[("claude", "high")])),
            format!("claude -p {lean} --model sonnet --effort high")
        );
        assert_eq!(
            command_line(
                "codex exec",
                &m(&[("codex", "gpt-x")]),
                &m(&[("codex", "high")])
            ),
            "codex exec -m gpt-x -c model_reasoning_effort=high"
        );
        assert_eq!(
            command_line("codex exec", &none, &none),
            "codex exec -m gpt-6-luna -c model_reasoning_effort=medium"
        );
        assert_eq!(
            command_line("claude -p", &m(&[("claude", " ")]), &m(&[("claude", "")])),
            format!("claude -p {lean}")
        );
        assert_eq!(
            command_line(
                "pi -p --model a/b",
                &m(&[("claude", "x")]),
                &m(&[("pi", "high")])
            ),
            "pi -p --model a/b"
        );
        assert_eq!(
            command_line(
                "opencode run --agent plan",
                &m(&[("opencode", "zai/glm-5.3")]),
                &none
            ),
            "opencode run --agent plan -m zai/glm-5.3"
        );
        assert_eq!(
            command_line("llm", &none, &m(&[("llm", "high")])),
            "llm -m gpt-6-luna"
        );
    }

    #[test]
    fn stash_names() {
        assert_eq!(
            describe_stash("On main: tidy up"),
            ("tidy up".into(), Some("main".into()))
        );
        assert_eq!(
            describe_stash("WIP on feat/x: 1a2b3c4 Add login"),
            ("WIP: Add login".into(), Some("feat/x".into()))
        );
        assert_eq!(describe_stash("odd"), ("odd".into(), None));
    }

    // As pasteMessage.test.ts.
    #[test]
    fn pasted_messages() {
        assert_eq!(
            paste_message("", "", "", "feat: x\n\nwhy", 0, 0),
            Some(("feat: x".into(), "why".into(), 7))
        );
        assert_eq!(
            paste_message("ab", "mine", "", "X\nmore", 1, 1),
            Some(("aXb".into(), "more\n\nmine".into(), 2))
        );
        assert_eq!(
            paste_message("ab", "tmpl", "tmpl", "X\r\nmore", 2, 2),
            Some(("abX".into(), "more".into(), 3))
        );
        // Over a selection, which may have been made right to left.
        assert_eq!(
            paste_message("abcd", "", "", "\n\nX\nmore", 3, 1),
            Some(("aXd".into(), "more".into(), 2))
        );
        assert_eq!(paste_message("ab", "", "", "one line", 0, 0), None);
    }

    // As GitHub reads them.
    #[test]
    fn co_authors() {
        for ok in ["Ada Lovelace <ada@x.dev>", "a <b@c>", "Ada  <a@b@c>"] {
            assert!(is_co_author(ok), "{ok}");
        }
        for bad in [
            "Ada",
            "<ada@x.dev>",
            "Ada <ada>",
            "Ada <@x>",
            "Ada <a@>",
            "Ada <a b@x>",
            "A <b <c@d>",
        ] {
            assert!(!is_co_author(bad), "{bad}");
        }
        assert_eq!(author_name("Ada Lovelace <ada@x.dev>"), "Ada Lovelace");
        assert_eq!(author_name("<ada@x.dev>"), "<ada@x.dev>");
        assert_eq!(author_name("Ada"), "Ada");
    }
}
