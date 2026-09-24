//! Search in files: `git grep` over the worktree, the files the explorer lists (tracked, and
//! untracked ones git doesn't ignore). One search runs at a time: a newer one stops it.

use crate::git;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::Child;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub text: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// Comma-separated globs, as VS Code's: one without a `/` (`*.ts`, `dist`) matches at any depth.
    pub include: String,
    pub exclude: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub line: u32,
    /// The line, or the part of a long one around its first match (with … where it's cut).
    pub text: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileHits {
    pub path: String,
    pub hits: Vec<Hit>,
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub files: Vec<FileHits>,
    /// Matching lines in `files`.
    pub count: usize,
    /// Stopped at MAX_HITS: there are more.
    pub capped: bool,
    /// Stopped at TIMEOUT: what was found by then.
    pub timed_out: bool,
    /// The regex ran as POSIX extended, this git being built without PCRE: `\d`, `\w`, `\b` and
    /// lookarounds mean something else there, or nothing.
    pub posix: bool,
}

/// The error of a search a newer one stopped; nobody is waiting for it.
pub const CANCELLED: &str = "search:cancelled";
pub const MAX_HITS: usize = 2000;
const TIMEOUT: Duration = Duration::from_secs(10);
/// Bytes of a line kept around its match: a minified file's one line can be megabytes.
const LINE_WINDOW: usize = 240;
const LEAD: usize = 60;
/// Bytes of a record's path and numbers kept at most.
const HEAD_LIMIT: usize = 16 * 1024;

pub(crate) static LATEST: AtomicU64 = AtomicU64::new(0);

pub fn search(root: &Path, q: &Query) -> Result<Found, String> {
    let id = LATEST.fetch_add(1, Ordering::SeqCst) + 1;
    if q.text.is_empty() {
        return Ok(Found::default());
    }
    match run(root, q, id, true) {
        // A git built without PCRE: POSIX extended regexes instead, and the result says so.
        Err(e) if q.regex && e.contains("USE_LIBPCRE") => {
            run(root, q, id, false).map(|f| Found { posix: true, ..f })
        }
        r => r,
    }
}

/// Stops the running search, as a newer one would (the query was cleared, the view closed).
pub fn cancel() {
    LATEST.fetch_add(1, Ordering::SeqCst);
}

/// VS Code's glob shorthand as git pathspecs: a bare `*.ts` or `dist` anywhere, a folder's
/// contents too, and `{a,b}` alternatives, which git's globs lack, as one pathspec each.
fn pathspecs(globs: &str, magic: &str) -> Vec<String> {
    split_outside_braces(globs)
        .iter()
        .flat_map(|g| expand_braces(g))
        .map(|g| {
            g.trim()
                .trim_start_matches("./")
                .trim_end_matches('/')
                .to_string()
        })
        .filter(|g| !g.is_empty())
        .flat_map(|g| {
            let g = if g.contains('/') || g.starts_with("**") {
                g.to_string()
            } else {
                format!("**/{g}")
            };
            [format!(":({magic}){g}"), format!(":({magic}){g}/**")]
        })
        .collect()
}

/// `a,{b,c},d` → `a`, `{b,c}`, `d`: commas inside braces are alternatives, not separators.
fn split_outside_braces(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0usize, 0);
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// `src/*.{ts,tsx}` → `src/*.ts`, `src/*.tsx`; nested and several groups too. An unclosed `{` stays as it is.
fn expand_braces(g: &str) -> Vec<String> {
    let Some(open) = g.find('{') else {
        return vec![g.to_string()];
    };
    let mut depth = 0;
    let close = g[open..].char_indices().find_map(|(i, c)| {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        (depth == 0).then_some(open + i)
    });
    let Some(close) = close else {
        return vec![g.to_string()];
    };
    let (head, tail) = (&g[..open], &g[close + 1..]);
    split_outside_braces(&g[open + 1..close])
        .into_iter()
        .flat_map(|alt| expand_braces(&format!("{head}{alt}{tail}")))
        .collect()
}

const STOP_NONE: u8 = 0;
const STOP_CANCELLED: u8 = 1;
const STOP_TIMEOUT: u8 = 2;
const STOP_CAPPED: u8 = 3;

pub(crate) fn run(root: &Path, q: &Query, id: u64, pcre: bool) -> Result<Found, String> {
    // Typing queues searches: one already overtaken needn't start.
    if LATEST.load(Ordering::SeqCst) != id {
        return Err(CANCELLED.into());
    }
    let mut args: Vec<String> = [
        "grep",
        "--untracked",
        "-I",
        "-z",
        "-n",
        "--column",
        "--no-color",
    ]
    .map(String::from)
    .to_vec();
    if !q.match_case {
        args.push("-i".into());
    }
    if q.whole_word {
        args.push("-w".into());
    }
    args.push(
        if !q.regex {
            "-F"
        } else if pcre {
            "-P"
        } else {
            "-E"
        }
        .into(),
    );
    args.extend(["-e".into(), q.text.clone(), "--".into()]);
    args.extend(pathspecs(&q.include, "glob"));
    args.extend(pathspecs(&q.exclude, "exclude,glob"));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut cmd = git::command(root, &args);
    // Globs here are globs.
    cmd.env_remove("GIT_LITERAL_PATHSPECS");
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not run git grep: {e}"))?;
    let stdout = child.stdout.take().ok_or("git grep has no output")?;
    let mut stderr = child.stderr.take();
    let err = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });

    // Stopped by a newer search, the clock, or enough hits: git is killed, its pipe ends.
    let child = Arc::new(Mutex::new(child));
    let stop = Arc::new(AtomicU8::new(STOP_NONE));
    let done = Arc::new(AtomicU8::new(0));
    let watch = {
        let (child, stop, done) = (child.clone(), stop.clone(), done.clone());
        let start = Instant::now();
        std::thread::spawn(move || {
            while done.load(Ordering::SeqCst) == 0 {
                let why = if LATEST.load(Ordering::SeqCst) != id {
                    STOP_CANCELLED
                } else if start.elapsed() > TIMEOUT {
                    STOP_TIMEOUT
                } else {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                };
                stop.store(why, Ordering::SeqCst);
                kill(&child);
                break;
            }
        })
    };

    let mut found = Found::default();
    let mut reader = BufReader::with_capacity(64 * 1024, stdout);
    while let Some(read) = read_hit(&mut reader) {
        let Some((path, hit)) = read else {
            continue;
        };
        match found.files.last_mut() {
            Some(f) if f.path == path => f.hits.push(hit),
            _ => found.files.push(FileHits {
                path,
                hits: vec![hit],
            }),
        }
        found.count += 1;
        if found.count >= MAX_HITS {
            let _ =
                stop.compare_exchange(STOP_NONE, STOP_CAPPED, Ordering::SeqCst, Ordering::SeqCst);
            kill(&child);
            break;
        }
    }
    done.store(1, Ordering::SeqCst);
    let _ = watch.join();
    let status = child.lock().unwrap().wait().map_err(|e| e.to_string())?;
    let stderr = err.join().unwrap_or_default();
    match stop.load(Ordering::SeqCst) {
        STOP_CANCELLED => Err(CANCELLED.into()),
        STOP_TIMEOUT => {
            found.timed_out = true;
            Ok(found)
        }
        STOP_CAPPED => {
            found.capped = true;
            Ok(found)
        }
        // 1: nothing matched.
        _ if status.success() || status.code() == Some(1) => Ok(found),
        _ => Err(
            Some(stderr.trim().trim_start_matches("fatal: ").to_string())
                .filter(|e| !e.is_empty())
                .unwrap_or_else(|| format!("git grep failed ({})", status.code().unwrap_or(-1))),
        ),
    }
}

fn kill(child: &Mutex<Child>) {
    let _ = child.lock().unwrap().kill();
}

/// One matching line as `git grep -z -n --column` prints it, `path\0line\0column\0text\n`, read
/// keeping of its text only what's shown: the start, and the part around the match (a minified
/// file's one line can be megabytes). Some(None) for a record that doesn't parse; None at the end.
fn read_hit(r: &mut impl BufRead) -> Option<Option<(String, Hit)>> {
    let mut head = Vec::new();
    // NULs seen; the third ends the header.
    let mut fields = 0;
    let (mut first, mut around) = (Vec::new(), Vec::new());
    // Where `around` starts in the text, and the text's bytes so far.
    let (mut lo, mut len) = (0, 0);
    let mut any = false;
    loop {
        let buf = match r.fill_buf() {
            Ok(b) if !b.is_empty() => b,
            _ => break,
        };
        any = true;
        let (mut used, mut ended) = (0, false);
        for &b in buf {
            used += 1;
            if b == b'\n' {
                ended = true;
                break;
            }
            if fields < 3 {
                if b == 0 {
                    fields += 1;
                    if fields == 3 {
                        lo = column(&head).saturating_sub(1 + LEAD);
                    }
                }
                if head.len() < HEAD_LIMIT {
                    head.push(b);
                }
                continue;
            }
            if len < LINE_WINDOW + 4 {
                first.push(b);
            }
            if len >= lo && len < lo + LINE_WINDOW + 4 {
                around.push(b);
            }
            len += 1;
        }
        r.consume(used);
        if ended {
            break;
        }
    }
    any.then(|| {
        let mut parts = head.split(|&b| b == 0);
        let path = String::from_utf8_lossy(parts.next()?).into_owned();
        let line = number(parts.next()?)? as u32;
        let text = cut(&first, &around, lo, len);
        Some((path, Hit { line, text }))
    })
}

fn number(b: &[u8]) -> Option<usize> {
    std::str::from_utf8(b).ok()?.parse().ok()
}

/// The header's third field: the match's 1-based byte column.
fn column(head: &[u8]) -> usize {
    head.split(|&b| b == 0).nth(2).and_then(number).unwrap_or(1)
}

/// The text to show of a line `len` bytes long: all of a short one, else LINE_WINDOW bytes of
/// `around` (which starts at byte `lo`) on character boundaries, with … where text was left out.
fn cut(first: &[u8], around: &[u8], lo: usize, len: usize) -> String {
    let text = |b: &[u8]| String::from_utf8_lossy(b.strip_suffix(b"\r").unwrap_or(b)).into_owned();
    if len <= LINE_WINDOW {
        return text(first);
    }
    let inside = |b: &u8| b & 0xC0 == 0x80;
    let start = if lo == 0 {
        0
    } else {
        around.iter().take_while(|b| inside(b)).count()
    };
    let mut end = (start + LINE_WINDOW).min(around.len());
    while end < around.len() && inside(&around[end]) {
        end += 1;
    }
    let mut out = String::new();
    if lo + start > 0 {
        out.push('…');
    }
    out.push_str(&text(&around[start..end]));
    if lo + end < len {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_anywhere_unless_anchored() {
        assert_eq!(
            pathspecs(" *.ts, src/lib/ ,", "glob"),
            [
                ":(glob)**/*.ts",
                ":(glob)**/*.ts/**",
                ":(glob)src/lib",
                ":(glob)src/lib/**"
            ]
        );
        assert!(pathspecs("", "exclude,glob").is_empty());
    }

    #[test]
    fn braces_are_alternatives() {
        let globs = |s: &str| {
            pathspecs(s, "glob")
                .into_iter()
                .filter(|p| !p.ends_with("/**"))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            globs("*.{ts,tsx}, docs"),
            [":(glob)**/*.ts", ":(glob)**/*.tsx", ":(glob)**/docs"]
        );
        assert_eq!(
            globs("{src,lib}/*.{js,c{,pp}}"),
            [
                ":(glob)src/*.js",
                ":(glob)src/*.c",
                ":(glob)src/*.cpp",
                ":(glob)lib/*.js",
                ":(glob)lib/*.c",
                ":(glob)lib/*.cpp"
            ]
        );
        // Unclosed: kept, for git to match literally.
        assert_eq!(globs("a{b"), [":(glob)**/a{b"]);
    }

    fn hits(out: &[u8]) -> Vec<(String, u32, String)> {
        let mut r = BufReader::with_capacity(1024, out);
        std::iter::from_fn(|| read_hit(&mut r))
            .flatten()
            .map(|(p, h)| (p, h.line, h.text))
            .collect()
    }

    #[test]
    fn records_parse_one_line_each() {
        let out = b"src/a b.rs\x0012\x003\x00let x = 1;\r\nb.md\x001\x001\x00last";
        assert_eq!(
            hits(out),
            [
                ("src/a b.rs".into(), 12, "let x = 1;".into()),
                ("b.md".into(), 1, "last".into())
            ]
        );
    }

    #[test]
    fn long_lines_keep_the_part_around_the_match() {
        // Far past any buffer: the match is still what's shown.
        let at = 300_000;
        let line = format!("{}needle{}", "a".repeat(at), "b".repeat(at));
        let out = format!("f\x001\x00{}\x00{line}\nnext\x002\x001\x00x\n", at + 1);
        let found = hits(out.as_bytes());
        let text = &found[0].2;
        assert!(text.starts_with("…") && text.ends_with("…"), "{text}");
        assert!(text.contains("needle") && text.len() < LINE_WINDOW + 10);
        assert_eq!(found[1], ("next".into(), 2, "x".into()));
        // Never inside a character.
        let wide = "é".repeat(400);
        let out = format!("f\x001\x00301\x00{wide}\n");
        assert!(hits(out.as_bytes())[0]
            .2
            .chars()
            .all(|c| c == 'é' || c == '…'));
    }
}
