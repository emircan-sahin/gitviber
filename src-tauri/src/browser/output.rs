//! What `gitviber browser` says back, apart from AppKit so it can be tested: the line an action
//! ends with, the nudge out of a loop, and the address a wait waits for.

use super::cut;
use super::network::{Entry, Kind};
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use tauri::Url;

/// What an action set off, from the page (agent.js's `changes`) and the tab's logs.
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    pub added: u64,
    pub removed: u64,
    pub texts: u64,
    pub attributes: u64,
    /// Dialogs on show after, less before: <dialog open>, role=dialog, aria-modal.
    pub dialogs: i64,
    pub url_before: String,
    pub url: String,
    /// The page is another document now.
    pub new_page: bool,
    pub errors: usize,
    /// The page's alert, confirm and prompt answered by themselves meanwhile, as console.rs
    /// notes them.
    pub answered: Vec<String>,
    /// Requests the action started, oldest first.
    pub requests: Vec<Request>,
}

#[derive(Debug, PartialEq)]
pub struct Request {
    pub method: String,
    pub url: String,
    /// 200, failed, … while it runs.
    pub state: String,
    pub failed: bool,
}

impl Request {
    /// A request as the summary tells it: what the page asked for, not each image it drew.
    pub fn of(e: &Entry) -> Option<Self> {
        let page = matches!(e.kind, Kind::Fetch | Kind::Xhr | Kind::Ws);
        (page || e.failed()).then(|| Self {
            method: match e.kind {
                Kind::Ws => "WS".into(),
                Kind::Resource => "GET".into(),
                _ => e.method.clone(),
            },
            url: e.url.clone(),
            state: e.state(),
            failed: e.failed(),
        })
    }
}

/// Requests a summary names; past them, a count.
const REQUESTS: usize = 3;

/// The one line an action ends with: what changed on the page and around it, or that nothing
/// did. `→ +3 elements, dialog opened, url /lobby, 1 console error, POST /api/join 500`.
pub fn changed(c: &Changes) -> String {
    let mut said = vec![];
    let count = |n: u64, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    match (c.added, c.removed) {
        (0, 0) => {}
        (a, 0) => said.push(format!("+{a} element{}", plural(a))),
        (0, r) => said.push(format!("−{r} element{}", plural(r))),
        (a, r) => said.push(format!("+{a} −{r} elements")),
    }
    said.extend(count(c.texts, "text changed", "texts changed"));
    said.extend(count(
        c.attributes,
        "attribute changed",
        "attributes changed",
    ));
    match c.dialogs {
        0 => {}
        1 => said.push("dialog opened".into()),
        n if n > 1 => said.push(format!("{n} dialogs opened")),
        -1 => said.push("dialog closed".into()),
        n => said.push(format!("{} dialogs closed", -n)),
    }
    if c.new_page {
        said.push(format!("new page {}", short_url(&c.url, &c.url_before)));
    } else if c.url != c.url_before {
        said.push(format!("url {}", short_url(&c.url, &c.url_before)));
    }
    said.extend(c.answered.iter().cloned());
    said.extend(count(c.errors as u64, "console error", "console errors"));
    // The failed first: they're what an agent needs to see.
    let mut requests: Vec<&Request> = c.requests.iter().filter(|r| r.failed).collect();
    requests.extend(c.requests.iter().filter(|r| !r.failed));
    for r in requests.iter().take(REQUESTS) {
        said.push(format!(
            "{} {} {}",
            r.method,
            short_url(&r.url, &c.url_before),
            r.state
        ));
    }
    if requests.len() > REQUESTS {
        said.push(format!("+{} more requests", requests.len() - REQUESTS));
    }
    if said.is_empty() {
        "→ no visible change".into()
    } else {
        format!("→ {}", said.join(", "))
    }
}

fn plural(n: u64) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// An address as a summary names it: its path on the page's own origin, else its host and path.
fn short_url(url: &str, page: &str) -> String {
    let (Ok(u), Ok(p)) = (Url::parse(url), Url::parse(page)) else {
        return cut(url, 80);
    };
    let path = match u.query() {
        Some(q) => format!("{}?{q}", u.path()),
        None => u.path().into(),
    };
    let host = u.host_str().unwrap_or_default();
    let short = match (u.origin() == p.origin(), u.port()) {
        (true, _) => path,
        (false, Some(port)) => format!("{host}:{port}{path}"),
        (false, None) => format!("{host}{path}"),
    };
    cut(&short, 80)
}

/// One `dialog answered` line from console.rs's note: `confirm("…") answered Cancel by itself`
/// as `confirm() answered Cancel`, the page's text left out.
pub fn answered(note: &str) -> Option<String> {
    let what = note.split('(').next()?;
    let (_, answer) = note.rsplit_once(" answered ")?;
    let answer = answer.strip_suffix(" by itself")?;
    matches!(what, "alert" | "confirm" | "prompt").then(|| format!("{what}() answered {answer}"))
}

/// The tab's last few commands with the page as each left it, for the nudge out of a loop.
#[derive(Default)]
pub struct History(VecDeque<(u64, u64)>);

/// Commands kept, and how many alike in a row are a loop.
const HISTORY: usize = 20;
const LOOP: usize = 3;
pub const NUDGE: &str = "(same command 3× with no change: try something else)";

impl History {
    /// `command` (its name and arguments) run, the page then in `state`: whether it's the
    /// LOOPth alike in a row, the page the same after each.
    pub fn again(&mut self, command: &str, state: &str) -> bool {
        if self.0.len() == HISTORY {
            self.0.pop_front();
        }
        self.0.push_back((hash(command), hash(state)));
        let last = self.0.back().copied();
        self.0.len() >= LOOP && self.0.iter().rev().take(LOOP).all(|c| Some(*c) == last)
    }
}

fn hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Whether the page's address is the one `wait --url` waits for: one with no wildcard anywhere
/// in it; with them, all of it, or of its path from a pattern that starts with `/`. `**` is any
/// text, `*` any but a `/`, `?` one character but a `/`.
pub fn url_matches(pattern: &str, url: &str) -> bool {
    if !pattern.contains(['*', '?']) {
        return url.contains(pattern);
    }
    let text = match Url::parse(url) {
        Ok(u) if pattern.starts_with('/') => {
            let (query, fragment) = (u.query(), u.fragment());
            let mut path = u.path().to_string();
            if let Some(q) = query {
                path = format!("{path}?{q}");
            }
            if let Some(f) = fragment {
                path = format!("{path}#{f}");
            }
            path
        }
        _ => url.to_string(),
    };
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    glob(&p, &t)
}

fn glob(p: &[char], t: &[char]) -> bool {
    match p {
        [] => t.is_empty(),
        ['*', '*', rest @ ..] => (0..=t.len()).any(|i| glob(rest, &t[i..])),
        ['*', rest @ ..] => {
            let most = t.iter().position(|&c| c == '/').unwrap_or(t.len());
            (0..=most).any(|i| glob(rest, &t[i..]))
        }
        ['?', rest @ ..] => matches!(t, [c, ..] if *c != '/') && glob(rest, &t[1..]),
        [c, rest @ ..] => t.first() == Some(c) && glob(rest, &t[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, url: &str, state: &str, failed: bool) -> Request {
        Request {
            method: method.into(),
            url: url.into(),
            state: state.into(),
            failed,
        }
    }

    #[test]
    fn an_action_ends_with_one_line_of_what_changed() {
        let page = "http://localhost:3000/join";
        let c = Changes {
            added: 3,
            dialogs: 1,
            url_before: page.into(),
            url: "http://localhost:3000/lobby".into(),
            errors: 1,
            requests: vec![
                request("GET", "http://localhost:3000/api/me", "200", false),
                request("POST", "http://localhost:3000/api/join?x=1", "500", true),
            ],
            ..Changes::default()
        };
        assert_eq!(
            changed(&c),
            "→ +3 elements, dialog opened, url /lobby, 1 console error, POST /api/join?x=1 500, \
             GET /api/me 200"
        );
        let quiet = Changes {
            url_before: page.into(),
            url: page.into(),
            ..Changes::default()
        };
        assert_eq!(changed(&quiet), "→ no visible change");
    }

    #[test]
    fn a_summary_stays_one_line_however_much_happened() {
        let page = "http://localhost:3000/";
        let c = Changes {
            added: 1,
            removed: 40,
            texts: 2,
            attributes: 1,
            dialogs: -1,
            url_before: page.into(),
            url: "https://auth.example.test/login".into(),
            new_page: true,
            answered: vec!["confirm() answered Cancel".into()],
            requests: (0..6)
                .map(|i| request("GET", &format!("{page}api/{i}"), "…", false))
                .collect(),
            ..Changes::default()
        };
        let line = changed(&c);
        assert!(!line.contains('\n'));
        assert_eq!(
            line,
            "→ +1 −40 elements, 2 texts changed, 1 attribute changed, dialog closed, new page \
             auth.example.test/login, confirm() answered Cancel, GET /api/0 …, GET /api/1 …, \
             GET /api/2 …, +3 more requests"
        );
    }

    #[test]
    fn a_dialogs_note_reads_without_the_pages_text() {
        assert_eq!(
            answered(r#"confirm("Delete (all) answered OK?") answered Cancel by itself"#)
                .as_deref(),
            Some("confirm() answered Cancel")
        );
        assert_eq!(
            answered(r#"prompt("Name") answered with its default text by itself"#).as_deref(),
            Some("prompt() answered with its default text")
        );
        assert_eq!(answered("[network] GET /x → 500"), None);
    }

    #[test]
    fn the_third_command_alike_on_an_unchanged_page_is_a_loop() {
        let mut h = History::default();
        assert!(!h.again("click e5", "page A"));
        assert!(!h.again("click e5", "page A"));
        assert!(h.again("click e5", "page A"));
        assert!(h.again("click e5", "page A"), "and each after it");
        // The page changing, or another command, starts over.
        assert!(!h.again("click e5", "page B"));
        assert!(!h.again("click e6", "page B"));
        assert!(!h.again("click e5", "page B"));
        // A toggle back and forth is no loop.
        let mut t = History::default();
        for state in ["open", "closed", "open", "closed"] {
            assert!(!t.again("click e1", state));
        }
        assert!(t.0.len() <= HISTORY);
    }

    #[test]
    fn a_wait_for_an_address_takes_part_of_it_or_a_glob() {
        let url = "http://localhost:3000/rooms/42/lobby?tab=chat";
        assert!(url_matches("/lobby", url));
        assert!(url_matches("localhost:3000/rooms", url));
        assert!(!url_matches("/login", url));
        assert!(url_matches("**/lobby*", url));
        assert!(url_matches("/rooms/*/lobby?tab=*", url));
        assert!(!url_matches("/rooms/*", url), "* stops at a slash");
        assert!(url_matches("/rooms/**", url));
        assert!(url_matches("http://localhost:3000/rooms/4?/lobby**", url));
        assert!(!url_matches("http://localhost:3000/rooms/?/lobby**", url));
        assert!(url_matches("**", url));
        assert!(!url_matches("*", url));
    }
}
