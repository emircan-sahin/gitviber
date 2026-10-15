//! What happened to a pane's agent tabs between its commands (a popup opened, a download
//! finished, a page's process ended), told at the end of the pane's next reply: an agent only
//! reads replies. Apart from AppKit, so it can be tested.

use super::control::Reply;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

/// A pane's notes until a reply takes them; past this the oldest go.
const MOST: usize = 20;

struct Note {
    /// The tab it's about (id, name); told by name in another tab's reply.
    tab: Option<(String, String)>,
    text: String,
}

#[derive(Default)]
pub struct Notes {
    panes: HashMap<u32, Vec<Note>>,
}

impl Notes {
    pub fn add(&mut self, pty: u32, tab: Option<(&str, &str)>, text: impl Into<String>) {
        let notes = self.panes.entry(pty).or_default();
        if notes.len() == MOST {
            notes.remove(0);
        }
        notes.push(Note {
            tab: tab.map(|(id, name)| (id.into(), name.into())),
            text: text.into(),
        });
    }

    /// The pane's notes as a reply from tab `on` (an id) tells them, oldest first.
    pub fn take(&mut self, pty: u32, on: Option<&str>) -> Vec<String> {
        let notes = self.panes.remove(&pty).unwrap_or_default();
        notes
            .into_iter()
            .map(|n| match n.tab {
                Some((id, name)) if Some(id.as_str()) != on => {
                    format!("→ tab {name}: {}", n.text)
                }
                _ => format!("→ {}", n.text),
            })
            .collect()
    }

    pub fn forget(&mut self, pty: u32) {
        self.panes.remove(&pty);
    }
}

/// Written on the main thread, taken on a command's.
static NOTES: LazyLock<Mutex<Notes>> = LazyLock::new(Mutex::default);

fn notes() -> std::sync::MutexGuard<'static, Notes> {
    NOTES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Told in pane `pty`'s next reply; `tab` (id, name) is the tab it's about.
pub fn note(pty: u32, tab: Option<(&str, &str)>, text: impl Into<String>) {
    notes().add(pty, tab, text);
}

/// The pane closed: no reply comes to tell them.
pub fn forget(pty: u32) {
    notes().forget(pty);
}

/// `reply` from tab `on` of pane `pty`, with what the pane hasn't been told yet.
pub fn tell(reply: Reply, pty: u32, on: Option<&str>) -> Reply {
    let told = notes().take(pty, on);
    with(reply, &told)
}

/// The notes after the reply's text, or its error's, and as its `notes` for --json.
fn with(mut reply: Reply, notes: &[String]) -> Reply {
    if notes.is_empty() {
        return reply;
    }
    let lines = notes.join("\n");
    let text = if reply.ok {
        &mut reply.out
    } else {
        &mut reply.error
    };
    *text = Some(match text.take().filter(|t| !t.is_empty()) {
        Some(t) => format!("{t}\n{lines}"),
        None => lines,
    });
    reply.notes = notes.to_vec();
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_panes_notes_go_once_in_its_next_reply_naming_another_tabs() {
        let mut n = Notes::default();
        n.add(
            1,
            Some(("a", "t1")),
            "popup opened as tab t1-popup1 (https://login.test/)",
        );
        n.add(
            1,
            Some(("b", "t2")),
            "downloaded report.pdf (24 KB): /c/report.pdf",
        );
        n.add(
            1,
            None,
            "tab t1-popup1 closed: its page called window.close()",
        );
        n.add(2, Some(("c", "t1")), "elsewhere");
        assert_eq!(
            n.take(1, Some("a")),
            [
                "→ popup opened as tab t1-popup1 (https://login.test/)",
                "→ tab t2: downloaded report.pdf (24 KB): /c/report.pdf",
                "→ tab t1-popup1 closed: its page called window.close()",
            ]
        );
        assert!(n.take(1, Some("a")).is_empty(), "told once");
        // A pane command (tab list) goes to no tab: each is named.
        assert_eq!(n.take(2, None), ["→ tab t1: elsewhere"]);
    }

    #[test]
    fn a_pane_keeps_its_latest_notes_and_a_closed_one_none() {
        let mut n = Notes::default();
        for i in 0..MOST + 5 {
            n.add(1, None, format!("n{i}"));
        }
        let told = n.take(1, None);
        assert_eq!(told.len(), MOST);
        assert_eq!(told[0], "→ n5");
        n.add(3, None, "x");
        n.forget(3);
        assert!(n.take(3, None).is_empty());
    }

    #[test]
    fn notes_follow_the_text_or_the_error_and_join_the_json() {
        let notes = ["→ a".to_string(), "→ b".to_string()];
        assert_eq!(
            with(Reply::out("Clicked e1"), &notes).out.unwrap(),
            "Clicked e1\n→ a\n→ b"
        );
        assert_eq!(with(Reply::out(""), &notes).out.unwrap(), "→ a\n→ b");
        let failed = with(Reply::error("No tab x"), &notes);
        assert_eq!(failed.error.unwrap(), "No tab x\n→ a\n→ b");
        assert!(!failed.ok && failed.out.is_none());
        let data = with(Reply::data("t1", json!({ "tabs": [] })), &notes);
        assert_eq!(
            data.data.unwrap(),
            json!({ "tabs": [] }),
            "beside it, not in it"
        );
        assert_eq!(data.notes, notes);
        assert_eq!(with(Reply::out("same"), &[]), Reply::out("same"));
    }

    #[test]
    fn notes_of_one_pane_never_reach_another() {
        let mut n = Notes::default();
        n.add(1, Some(("a", "t1")), "one");
        n.add(2, Some(("a", "t1")), "two");
        assert_eq!(n.take(2, Some("a")), ["→ two"]);
        n.forget(1);
        assert!(n.take(1, Some("a")).is_empty());
    }

    #[test]
    fn notes_are_a_json_array_on_every_reply() {
        let notes = ["→ a".to_string()];
        for reply in [Reply::out("Clicked e1"), Reply::error("No tab x")] {
            let json = serde_json::to_value(with(reply, &notes)).unwrap();
            assert_eq!(json["notes"], json!(["→ a"]), "{json}");
        }
    }
}
