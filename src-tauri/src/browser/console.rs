//! What a page logs as errors and warnings (scripts/console.js), kept per view for the browser
//! tab's console badge and panel: the latest few hundred, with a mark where each load began.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Entries a view keeps; older ones go.
const KEEP: usize = 500;
/// What console.js cuts a message and stack to, and the backend holds it to whatever arrives.
const MAX_TEXT: usize = 2048;

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Entry {
    /// "error", "warn", or "load" for the mark a new page left.
    pub level: String,
    pub msg: String,
    #[serde(default)]
    pub stack: String,
    pub url: String,
    /// Milliseconds since the epoch.
    pub ts: f64,
}

/// What the badge shows: since the panel was last cleared.
#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Counts {
    pub errors: u32,
    pub warnings: u32,
}

#[derive(Default)]
pub struct Log {
    entries: VecDeque<Entry>,
    counts: Counts,
}

fn cut(mut s: String) -> String {
    if s.len() > MAX_TEXT {
        let mut at = MAX_TEXT;
        while !s.is_char_boundary(at) {
            at -= 1;
        }
        s.truncate(at);
    }
    s
}

impl Log {
    fn push(&mut self, entry: Entry) {
        if self.entries.len() == KEEP {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// One line of console.js's JSON; anything else (a page posting its own) is no entry.
    pub fn add(&mut self, json: &str) -> bool {
        let Ok(mut entry) = serde_json::from_str::<Entry>(json) else {
            return false;
        };
        match entry.level.as_str() {
            "error" => self.counts.errors += 1,
            "warn" => self.counts.warnings += 1,
            _ => return false,
        }
        entry.msg = cut(entry.msg);
        entry.stack = cut(entry.stack);
        entry.url = cut(entry.url);
        self.push(entry);
        true
    }

    /// A new page loaded: what came before belongs to the last one.
    pub fn loaded(&mut self, url: &str, ts: f64) {
        self.push(Entry {
            level: "load".into(),
            msg: String::new(),
            stack: String::new(),
            url: cut(url.into()),
            ts,
        });
    }

    pub fn entries(&self) -> Vec<Entry> {
        self.entries.iter().cloned().collect()
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(level: &str, msg: &str) -> String {
        serde_json::json!({ "level": level, "msg": msg, "stack": "at f (app.js:1:2)", "url": "http://localhost:5173/", "ts": 1.0 }).to_string()
    }

    #[test]
    fn errors_and_warnings_count_and_the_oldest_go_past_the_limit() {
        let mut log = Log::default();
        assert!(log.add(&line("error", "boom")));
        assert!(log.add(&line("warn", "careful")));
        log.loaded("http://localhost:5173/next", 2.0);
        assert_eq!(
            log.counts(),
            Counts {
                errors: 1,
                warnings: 1
            }
        );
        assert_eq!(
            log.entries()
                .iter()
                .map(|e| e.level.as_str())
                .collect::<Vec<_>>(),
            ["error", "warn", "load"]
        );
        for i in 0..KEEP {
            log.add(&line("error", &i.to_string()));
        }
        let entries = log.entries();
        assert_eq!(entries.len(), KEEP);
        assert_eq!(entries[0].msg, "0", "the first three went");
        assert_eq!(log.counts().errors, 1 + KEEP as u32);
        log.clear();
        assert!(log.entries().is_empty());
        assert_eq!(log.counts(), Counts::default());
    }

    #[test]
    fn what_isnt_console_js_is_no_entry_and_long_text_is_cut() {
        let mut log = Log::default();
        for no in [
            "",
            "nonsense",
            "{}",
            r#"{"level":"info","msg":"x","url":"u","ts":1}"#,
            r#"{"level":"load","msg":"","url":"u","ts":1}"#,
        ] {
            assert!(!log.add(no), "{no}");
        }
        // A missing stack is an empty one.
        assert!(log.add(r#"{"level":"error","msg":"x","url":"u","ts":1}"#));
        let long = "é".repeat(MAX_TEXT);
        assert!(log.add(&line("warn", &long)));
        let msg = &log.entries()[1].msg;
        assert!(msg.len() <= MAX_TEXT && msg.chars().all(|c| c == 'é'));
        assert_eq!(
            log.counts(),
            Counts {
                errors: 1,
                warnings: 1
            }
        );
    }
}
