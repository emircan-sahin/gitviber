//! What an agent's page sent and loaded (scripts/network.js), kept per view for `gitviber
//! browser network`: its requests with their headers and bodies, its WebSockets with their
//! latest frames, and the rest it loaded. Only ever read by the agent, never written anywhere.

use super::cut;
use super::output;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;

/// Requests (fetch, XMLHttpRequest, WebSocket) a view keeps; older ones go.
const KEEP: usize = 300;
/// Everything else it loaded, kept apart: a dev server's page loads its modules by the hundred,
/// and they would push out the requests an agent asks about.
const KEEP_RESOURCES: usize = 500;
/// Frames a WebSocket keeps, its latest.
const FRAMES: usize = 50;
// Characters kept of each part of an entry. network.js cuts its own to these; whatever else a
// page posts is held to them here.
const BODY: usize = 8192;
const FRAME: usize = 1024;
const URL: usize = 4096;
/// A header's name, a status text, a body's type, a close reason.
const NAME: usize = 256;
const VALUE: usize = 2048;
/// A method, an error, an initiator.
const WORD: usize = 32;
const HEADERS: usize = 100;
/// Characters of one list of headers in all: real ones are a few hundred, a page's own could
/// be the whole of HEADERS × VALUE for each of three hundred requests.
const HEADERS_TEXT: usize = 16 * 1024;
/// A post is a few updates, or a hundred resources: past this it's none of network.js's.
const MAX_LINE: usize = 1 << 20;
/// What an entry shows of its address in a list; `network <id>` has all of it.
const LIST_URL: usize = 300;

#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Fetch,
    Xhr,
    Ws,
    Resource,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::Fetch, Kind::Xhr, Kind::Ws, Kind::Resource];

    fn name(self) -> &'static str {
        match self {
            Kind::Fetch => "fetch",
            Kind::Xhr => "xhr",
            Kind::Ws => "ws",
            Kind::Resource => "resource",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Self::ALL
            .into_iter()
            .find(|k| k.name().eq_ignore_ascii_case(s))
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    /// None: not text, only its type and size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// Bytes, decoded; None when not known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    /// The size is only what was read: the body is at least that.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub at_least: bool,
    /// The text is only its start.
    #[serde(default)]
    pub cut: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    /// Its number on the socket, from 1: a gap is frames past network.js's limit.
    pub n: u32,
    /// Sent by the page, else received.
    pub out: bool,
    /// Milliseconds after the socket was made.
    pub at: f64,
    /// None: binary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub size: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Close {
    pub code: u16,
    pub reason: String,
    pub clean: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The view's own, counting up across its pages and clears.
    pub id: u32,
    #[serde(skip)]
    key: String,
    pub kind: Kind,
    pub method: String,
    pub url: String,
    /// Where a redirect ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// For a resource, its initiator type (script, img, css, navigation, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initiator: Option<String>,
    /// The origin of the frame that made it, when not the page itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_frame: Option<String>,
    /// Milliseconds since the epoch.
    pub start: f64,
    /// None while it runs, or a body that streams on.
    pub duration: Option<f64>,
    pub status: Option<u16>,
    pub status_text: String,
    /// "failed", "aborted" or "timeout".
    pub error: Option<String>,
    /// Bytes of the response's body, decoded: as read to its end, else as resource timing has
    /// them. None: not known (the response body may still say at least how many).
    pub size: Option<f64>,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, String)>,
    pub request_body: Option<Body>,
    pub response_body: Option<Body>,
    #[serde(skip_serializing_if = "VecDeque::is_empty")]
    pub frames: VecDeque<Frame>,
    /// Frames the socket had in all, kept or not.
    #[serde(skip_serializing_if = "is_zero")]
    pub frame_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close: Option<Close>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// One part of an entry as network.js posts it: the first names it, the rest fill it in.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Update {
    key: String,
    kind: Option<Kind>,
    method: Option<String>,
    url: Option<String>,
    to: Option<String>,
    initiator: Option<String>,
    in_frame: Option<String>,
    start: Option<f64>,
    duration: Option<f64>,
    status: Option<u16>,
    status_text: Option<String>,
    error: Option<String>,
    size: Option<f64>,
    request_headers: Option<Vec<(String, String)>>,
    response_headers: Option<Vec<(String, String)>>,
    request_body: Option<Body>,
    response_body: Option<Body>,
    frame: Option<Frame>,
    close: Option<Close>,
    /// Requests the page made past network.js's limit, never posted.
    skipped: Option<u32>,
}

#[derive(Default)]
pub struct Log {
    requests: VecDeque<Entry>,
    resources: VecDeque<Entry>,
    next: u32,
    /// Entries gone from either ring, or never posted, since the last clear.
    dropped: u64,
}

/// A page's text as the terminal may show it: cut, and no control character (an escape
/// sequence, a carriage return over its own line) or bidi override (text shown in another
/// order than it reads) but a body's or a frame's lines and tabs. Every string a page posts
/// goes through here, once, as it's kept.
fn clean(s: &str, max: usize, lines: bool) -> String {
    cut(s, max)
        .chars()
        .map(|c| match c {
            '\n' | '\t' if lines => c,
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => '\u{FFFD}',
            c if c.is_control() => '\u{FFFD}',
            c => c,
        })
        .collect()
}

fn line(s: &str, max: usize) -> String {
    clean(s, max, false)
}

fn text(s: &str, max: usize) -> String {
    clean(&s.replace("\r\n", "\n"), max, true)
}

fn headers(list: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut left = HEADERS_TEXT;
    list.into_iter()
        .take(HEADERS)
        .map(|(k, v)| (line(&k, NAME), line(&v, VALUE)))
        .take_while(|(k, v)| {
            left = left.saturating_sub(k.chars().count() + v.chars().count());
            left > 0
        })
        .collect()
}

fn body(mut b: Body) -> Body {
    if let Some(t) = &b.text {
        b.cut |= t.chars().nth(BODY).is_some();
        b.text = Some(text(t, BODY));
    }
    b.r#type = b.r#type.map(|t| line(&t, NAME));
    b
}

impl Log {
    /// One post of network.js's: its updates in order. Anything else (a page posting its own
    /// junk) changes nothing.
    pub fn add(&mut self, json: &str) -> bool {
        if json.len() > MAX_LINE {
            return false;
        }
        // network.js posts a lone surrogate as U+FFFD: one in a post is the page's own.
        let Ok(updates) = serde_json::from_str::<Vec<Update>>(json) else {
            return false;
        };
        let mut any = false;
        for u in updates {
            any |= self.apply(u);
        }
        any
    }

    fn apply(&mut self, u: Update) -> bool {
        if let Some(n) = u.skipped {
            self.dropped = self.dropped.saturating_add(u64::from(n));
            return true;
        }
        if u.key.is_empty() || u.key.len() > 64 {
            return false;
        }
        let found = self
            .requests
            .iter_mut()
            .rev()
            .chain(self.resources.iter_mut().rev())
            .find(|e| e.key == u.key);
        if let Some(e) = found {
            merge(e, u);
            return true;
        }
        // A new one names its kind and address; an update of one gone (cleared, or pushed out)
        // is nothing.
        let (Some(kind), Some(url)) = (u.kind, &u.url) else {
            return false;
        };
        self.next += 1;
        let mut e = Entry {
            id: self.next,
            key: u.key.clone(),
            kind,
            url: line(url, URL),
            ..Entry::default()
        };
        merge(&mut e, u);
        let (ring, keep) = match kind {
            Kind::Resource => (&mut self.resources, KEEP_RESOURCES),
            _ => (&mut self.requests, KEEP),
        };
        if ring.len() == keep {
            ring.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        ring.push_back(e);
        true
    }

    /// Oldest first, requests and resources in the order they began.
    pub fn entries(&self) -> Vec<&Entry> {
        let mut all: Vec<&Entry> = self.requests.iter().chain(&self.resources).collect();
        all.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.id.cmp(&b.id)));
        all
    }

    pub fn get(&self, id: u32) -> Option<&Entry> {
        self.requests
            .iter()
            .chain(&self.resources)
            .find(|e| e.id == id)
    }

    /// The highest id given so far: an entry past it is newer.
    pub fn newest(&self) -> u32 {
        self.next
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Ids go on counting, so one never names two requests.
    pub fn clear(&mut self) {
        *self = Self {
            next: self.next,
            ..Self::default()
        };
    }
}

fn merge(e: &mut Entry, u: Update) {
    if let Some(m) = u.method {
        e.method = line(&m, WORD);
    }
    if let Some(to) = u.to {
        e.to = Some(line(&to, URL));
    }
    if let Some(i) = u.initiator {
        e.initiator = Some(line(&i, WORD));
    }
    if let Some(f) = u.in_frame {
        e.in_frame = Some(line(&f, URL));
    }
    if let Some(s) = u.start {
        e.start = s;
    }
    if u.duration.is_some() {
        e.duration = u.duration;
    }
    if u.status.is_some() {
        e.status = u.status;
    }
    if let Some(s) = u.status_text {
        e.status_text = line(&s, NAME);
    }
    if let Some(err) = u.error {
        e.error = Some(line(&err, WORD));
    }
    if u.size.is_some() {
        e.size = u.size;
    }
    if let Some(h) = u.request_headers {
        e.request_headers = headers(h);
    }
    if let Some(h) = u.response_headers {
        e.response_headers = headers(h);
    }
    if let Some(b) = u.request_body {
        e.request_body = Some(body(b));
    }
    if let Some(b) = u.response_body {
        if e.size.is_none() && !b.at_least {
            e.size = b.size;
        }
        e.response_body = Some(body(b));
    }
    if let Some(mut f) = u.frame {
        f.text = f.text.map(|t| text(&t, FRAME));
        e.frame_count = e.frame_count.max(f.n);
        if e.frames.len() == FRAMES {
            e.frames.pop_front();
        }
        e.frames.push_back(f);
    }
    if let Some(mut c) = u.close {
        c.reason = line(&c.reason, NAME);
        e.close = Some(c);
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Filter {
    pub failed: bool,
    /// Part of the address, any case.
    pub text: Option<String>,
    pub kind: Option<Kind>,
    /// GET, POST, ...: uppercase.
    pub method: Option<String>,
    /// A status (404), or its hundred (4 for 4xx).
    pub status: Option<Status>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Is(u16),
    Class(u16),
}

impl Status {
    fn parse(s: &str) -> Option<Status> {
        let lower = s.to_ascii_lowercase();
        match lower.strip_suffix("xx") {
            Some(d) if d.len() == 1 => d
                .parse()
                .ok()
                .filter(|d| (1..=5).contains(d))
                .map(Status::Class),
            Some(_) => None,
            None => s
                .parse()
                .ok()
                .filter(|n| (100..600).contains(n))
                .map(Status::Is),
        }
    }

    fn holds(self, status: Option<u16>) -> bool {
        match (self, status) {
            (Status::Is(want), Some(s)) => s == want,
            (Status::Class(c), Some(s)) => s / 100 == c,
            (_, None) => false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Ask {
    List { filter: Filter, clear: bool },
    One(u32),
}

/// `network`'s arguments: an id, else what to list.
pub fn ask(args: &[String]) -> Result<Ask, String> {
    if let [one] = args {
        if let Ok(id) = one.trim_start_matches('#').parse() {
            return Ok(Ask::One(id));
        }
    }
    let (mut filter, mut clear) = (Filter::default(), false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--failed" => filter.failed = true,
            "--clear" => clear = true,
            "--filter" => filter.text = Some(value()?.to_lowercase()),
            "--method" => filter.method = Some(value()?.to_ascii_uppercase()),
            "--status" => {
                let s = value()?;
                filter.status = Some(Status::parse(s).ok_or_else(|| {
                    format!("--status is a status (404) or a class (4xx), not {s:?}")
                })?);
            }
            "--type" => {
                let kind = value()?;
                filter.kind = Some(Kind::parse(kind).ok_or_else(|| {
                    format!("--type is fetch, xhr, ws or resource, not {kind:?}")
                })?);
            }
            _ => return Err(format!("network doesn't take {arg:?}")),
        }
    }
    Ok(Ask::List { filter, clear })
}

impl Entry {
    /// A status past 399, a request that got no answer, or a socket that closed badly; not one
    /// the page aborted itself.
    pub fn failed(&self) -> bool {
        self.status.is_some_and(|s| s >= 400)
            || self.error.as_deref().is_some_and(|e| e != "aborted")
            || self.close.as_ref().is_some_and(|c| !c.clean)
    }

    fn shown(&self, f: &Filter) -> bool {
        (!f.failed || self.failed())
            && f.kind.is_none_or(|k| k == self.kind)
            && f.method.as_ref().is_none_or(|m| *m == self.method)
            && f.status.is_none_or(|s| s.holds(self.status))
            && f.text
                .as_ref()
                .is_none_or(|t| self.url.to_lowercase().contains(t.as_str()))
    }

    /// The method column: a resource's initiator, as nothing else names it.
    fn what(&self) -> &str {
        match self.kind {
            Kind::Ws => "WS",
            Kind::Resource => match self.initiator.as_deref() {
                Some("navigation") | None => "page",
                Some(i) => i,
            },
            _ => &self.method,
        }
    }

    /// Its status as the list shows it: 200, failed, open, … while it runs.
    pub fn state(&self) -> String {
        if let Some(c) = &self.close {
            return c.code.to_string();
        }
        if let Some(e) = &self.error {
            return e.clone();
        }
        match (self.status, self.kind) {
            (Some(101), Kind::Ws) => "open".into(),
            (Some(0), _) => "opaque".into(),
            (Some(s), _) => s.to_string(),
            (None, Kind::Resource) => "-".into(),
            (None, _) => "…".into(),
        }
    }

    fn took(&self) -> String {
        match self.duration {
            Some(ms) => ms_text(ms),
            None if self.kind == Kind::Ws && self.status.is_some() => "open".into(),
            None => "…".into(),
        }
    }

    fn amount(&self) -> String {
        if self.kind == Kind::Ws {
            return format!("{} fr", self.frame_count);
        }
        match (self.size, &self.response_body) {
            (Some(size), _) => bytes_text(size),
            (
                None,
                Some(Body {
                    size: Some(size),
                    at_least: true,
                    ..
                }),
            ) => format!("≥{}", bytes_text(*size)),
            _ => "-".into(),
        }
    }
}

fn ms_text(ms: f64) -> String {
    let ms = ms.round();
    if ms < 1000.0 {
        format!("{ms} ms")
    } else {
        format!("{:.1} s", ms / 1000.0)
    }
}

fn bytes_text(n: f64) -> String {
    match n {
        n if n < 1024.0 => format!("{n} B"),
        n if n < 1024.0 * 1024.0 => format!("{:.1} KB", n / 1024.0),
        n => format!("{:.1} MB", n / 1024.0 / 1024.0),
    }
}

fn list_line(e: &Entry) -> String {
    let frame = e.in_frame.as_ref().map(|f| format!("  (frame {f})"));
    format!(
        "{:>4}  {:<7} {:>7} {:>8} {:>9}  {}{}",
        e.id,
        e.what(),
        e.state(),
        e.took(),
        e.amount(),
        cut(&e.url, LIST_URL),
        frame.unwrap_or_default()
    )
}

/// The list as an agent reads it: one line a request, oldest first, and how many weren't kept.
pub fn list_text(log: &Log, filter: &Filter) -> String {
    let shown: Vec<String> = log
        .entries()
        .into_iter()
        .filter(|e| e.shown(filter))
        .map(list_line)
        .collect();
    let mut out = vec![];
    if log.dropped() > 0 {
        out.push(format!(
            "({} more not kept: the newest {KEEP} requests and {KEEP_RESOURCES} other loads are)",
            log.dropped()
        ));
    }
    if shown.is_empty() {
        let none = if *filter == Filter::default() {
            "No requests yet."
        } else {
            "No requests match."
        };
        out.push(none.into());
    } else {
        out.extend(shown);
    }
    out.join("\n")
}

fn body_text(name: &str, b: &Body) -> String {
    let mut about = vec![];
    if let Some(t) = &b.r#type {
        about.push(t.clone());
    }
    about.push(match b.size {
        Some(size) if b.at_least => format!("at least {}", bytes_text(size)),
        Some(size) => bytes_text(size),
        None => "size not known".into(),
    });
    let head = format!("{name} ({}):", about.join(", "));
    let Some(text) = &b.text else {
        return head;
    };
    let body = text
        .lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    if b.cut {
        format!(
            "{head}\n{body}\n  [cut: the first {} characters]",
            text.chars().count()
        )
    } else {
        format!("{head}\n{body}")
    }
}

/// One entry in full, `now` (ms since the epoch) for when it started.
pub fn entry_text(e: &Entry, now: f64) -> String {
    let mut out = vec![format!(
        "#{} {} {} ({})",
        e.id,
        e.what(),
        e.url,
        e.kind.name()
    )];
    if let Some(to) = &e.to {
        out.push(format!("Redirected to {to}"));
    }
    if let Some(frame) = &e.in_frame {
        out.push(format!("From a frame of {frame}"));
    }
    out.push(match (&e.close, &e.error, e.status) {
        (Some(c), _, _) => format!(
            "Closed: {}{}{}",
            c.code,
            if c.reason.is_empty() {
                String::new()
            } else {
                format!(" {:?}", c.reason)
            },
            if c.clean { "" } else { ", not cleanly" }
        ),
        (None, Some(err), _) => format!("Failed: {err}"),
        (None, None, Some(101)) if e.kind == Kind::Ws => "Open".into(),
        (None, None, Some(s)) => format!("Status: {s} {}", e.status_text).trim_end().into(),
        (None, None, None) if e.kind == Kind::Resource => "Loaded".into(),
        (None, None, None) => "Waiting for an answer".into(),
    });
    let mut when = vec![format!("Started {} ago", ms_text((now - e.start).max(0.0)))];
    match e.duration {
        Some(ms) => when.push(format!("took {}", ms_text(ms))),
        None if e.response_body.is_some() => when.push("still streaming".into()),
        None => {}
    }
    if let Some(size) = e.size {
        when.push(bytes_text(size));
    }
    out.push(when.join("; "));
    for (side, list, b) in [
        ("Request", &e.request_headers, &e.request_body),
        ("Response", &e.response_headers, &e.response_body),
    ] {
        if !list.is_empty() {
            out.push(format!("{side} headers:"));
            out.extend(list.iter().map(|(k, v)| format!("  {k}: {v}")));
        }
        if let Some(b) = b {
            out.push(body_text(&format!("{side} body"), b));
        }
    }
    if e.frame_count > 0 {
        out.push(if e.frames.len() as u32 == e.frame_count {
            format!("Frames ({}):", e.frame_count)
        } else {
            format!(
                "Frames ({}, the last {} kept):",
                e.frame_count,
                e.frames.len()
            )
        });
        for f in &e.frames {
            let what = match &f.text {
                Some(t) => t.replace(['\n', '\t'], " "),
                None => "[binary]".into(),
            };
            out.push(format!(
                "  +{} {} {} {what}",
                ms_text(f.at.max(0.0)),
                if f.out { "↑" } else { "↓" },
                bytes_text(f.size)
            ));
        }
    }
    out.join("\n")
}

/// The list as `--json` has it: each entry without its headers, bodies and frames.
fn list_json(log: &Log, filter: &Filter) -> Value {
    let entries: Vec<_> = log
        .entries()
        .into_iter()
        .filter(|e| e.shown(filter))
        .map(|e| {
            json!({
                "id": e.id, "kind": e.kind, "method": e.method, "initiator": e.initiator,
                "url": e.url, "inFrame": e.in_frame, "status": e.status, "error": e.error, "start": e.start,
                "duration": e.duration, "size": e.size, "frameCount": e.frame_count,
            })
        })
        .collect();
    json!({ "dropped": log.dropped(), "entries": entries })
}

/// `network`'s answer as text, and as JSON for `--json`; a list may clear the log after.
pub fn answer(log: &mut Log, ask: &Ask, now: f64, reveal: bool) -> Result<(String, Value), String> {
    match ask {
        Ask::One(id) => {
            let e = log.get(*id).ok_or_else(|| {
                format!("No request #{id}: `gitviber browser network` lists them")
            })?;
            Ok(hidden((entry_text(e, now), json!(e)), reveal))
        }
        Ask::List { filter, clear } => {
            let answer = (list_text(log, filter), list_json(log, filter));
            if *clear {
                log.clear();
            }
            Ok(hidden(answer, reveal))
        }
    }
}

/// Credentials in headers, bodies and addresses hidden unless --reveal: by the same list as
/// every other reply.
fn hidden((text, mut data): (String, Value), reveal: bool) -> (String, Value) {
    if reveal {
        return (text, data);
    }
    output::redact_json(&mut data);
    (output::redact(&text), data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn post(log: &mut Log, updates: Value) -> bool {
        log.add(&updates.to_string())
    }

    fn fetch(log: &mut Log, key: &str, url: &str, start: f64) {
        assert!(post(
            log,
            json!([{ "key": key, "kind": "fetch", "method": "GET", "url": url, "start": start }])
        ));
    }

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn arguments_list_with_filters_or_name_one_entry() {
        assert_eq!(ask(&words("12")), Ok(Ask::One(12)));
        assert_eq!(ask(&words("#7")), Ok(Ask::One(7)));
        assert_eq!(
            ask(&[]),
            Ok(Ask::List {
                filter: Filter::default(),
                clear: false
            })
        );
        assert_eq!(
            ask(&words("--failed --filter /API/ --type xhr --clear")),
            Ok(Ask::List {
                filter: Filter {
                    failed: true,
                    text: Some("/api/".into()),
                    kind: Some(Kind::Xhr),
                    ..Filter::default()
                },
                clear: true
            })
        );
        assert_eq!(
            ask(&words("--method post --status 5XX")),
            Ok(Ask::List {
                filter: Filter {
                    method: Some("POST".into()),
                    status: Some(Status::Class(5)),
                    ..Filter::default()
                },
                clear: false
            })
        );
        for bad in [
            "--type",
            "--type http",
            "--method",
            "--status",
            "--status 6xx",
            "--status 4x",
            "--status 99",
            "--status ok",
            "--filter",
            "--all",
            "12 --clear",
            "twelve",
            "-1",
        ] {
            assert!(ask(&words(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_frames_request_says_which_frame() {
        let mut log = Log::default();
        assert!(post(
            &mut log,
            json!([{ "key": "f", "kind": "fetch", "method": "GET", "url": "https://ads.example.test/bid",
                "start": 0.0, "inFrame": "https://ads.example.test\u{1b}[2J" }])
        ));
        let e = log.get(1).unwrap();
        let listed = list_line(e);
        assert!(listed.contains("https://ads.example.test/bid  (frame https://ads.example.test"));
        assert!(
            !listed.contains('\u{1b}'),
            "a page's tag is cleaned as its other text"
        );
        assert!(entry_text(e, 0.0).contains("From a frame of https://ads.example.test"));
        // The page's own say nothing of a frame.
        fetch(&mut log, "p", "http://localhost/api", 0.0);
        assert!(!list_line(log.get(2).unwrap()).contains("frame"));
    }

    #[test]
    fn the_list_narrows_by_method_and_status() {
        let mut log = Log::default();
        for (key, method, status) in [
            ("a", "GET", 200),
            ("b", "POST", 500),
            ("c", "POST", 201),
            ("d", "GET", 404),
        ] {
            assert!(post(
                &mut log,
                json!([{ "key": key, "kind": "fetch", "method": method, "url": format!("http://localhost/{key}"),
                    "start": 0.0, "status": status }])
            ));
        }
        let shown = |f: Filter| {
            let text = list_text(&log, &f);
            text.lines()
                .map(|l| l.rsplit('/').next().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        let post_only = Filter {
            method: Some("POST".into()),
            ..Filter::default()
        };
        assert_eq!(shown(post_only), ["b", "c"]);
        let class = |c| Filter {
            status: Some(Status::Class(c)),
            ..Filter::default()
        };
        assert_eq!(shown(class(4)), ["d"]);
        assert_eq!(shown(class(2)), ["a", "c"]);
        let get_404 = Filter {
            method: Some("GET".into()),
            status: Some(Status::Is(404)),
            ..Filter::default()
        };
        assert_eq!(shown(get_404), ["d"]);
    }

    #[test]
    fn an_entry_starts_once_and_its_later_parts_fill_it_in() {
        let mut log = Log::default();
        assert!(post(
            &mut log,
            json!([{
                "key": "d1:1", "kind": "fetch", "method": "POST", "url": "http://localhost:5173/api/items",
                "start": 1000.0, "requestHeaders": [["content-type", "application/json"]],
                "requestBody": { "text": "{\"name\":\"x\"}", "size": 12.0 }
            }])
        ));
        assert!(post(
            &mut log,
            json!([{ "key": "d1:1", "status": 201, "statusText": "Created",
                     "responseHeaders": [["content-type", "application/json"]] }])
        ));
        assert!(post(
            &mut log,
            json!([{ "key": "d1:1", "duration": 85.4,
                     "responseBody": { "text": "{\"id\":5}", "type": "application/json", "size": 8.0 } }])
        ));
        let e = log.get(1).unwrap();
        assert_eq!(
            (e.status, e.duration, e.size),
            (Some(201), Some(85.4), Some(8.0))
        );
        assert_eq!(e.request_headers[0].1, "application/json");
        assert_eq!(
            list_line(e),
            "   1  POST        201    85 ms       8 B  http://localhost:5173/api/items"
        );
        let full = entry_text(e, 5000.0);
        for part in [
            "#1 POST http://localhost:5173/api/items (fetch)",
            "Status: 201 Created",
            "Started 4.0 s ago; took 85 ms; 8 B",
            "Request headers:\n  content-type: application/json\nRequest body (12 B):\n  {\"name\":\"x\"}",
            "Response body (application/json, 8 B):\n  {\"id\":5}",
        ] {
            assert!(full.contains(part), "{part}\n{full}");
        }
        // An update for a key never started, or one without a kind, is nothing.
        assert!(!post(&mut log, json!([{ "key": "d1:9", "status": 200 }])));
        assert!(!post(
            &mut log,
            json!([{ "key": "d1:9", "kind": "ftp", "url": "x" }])
        ));
        assert_eq!(log.entries().len(), 1);
    }

    #[test]
    fn the_ring_keeps_the_newest_and_says_how_many_went() {
        let mut log = Log::default();
        for i in 0..KEEP + 5 {
            fetch(
                &mut log,
                &format!("d:{i}"),
                &format!("http://localhost/{i}"),
                i as f64,
            );
        }
        // Resources have their own ring: a page's modules don't push requests out.
        let resources: Vec<Value> = (0..KEEP_RESOURCES + 2)
            .map(|i| json!({ "key": format!("d:r{i}"), "kind": "resource", "initiator": "script",
                             "url": format!("http://localhost/src/{i}.ts"), "start": 1e6 + i as f64, "duration": 3.0 }))
            .collect();
        for batch in resources.chunks(100) {
            assert!(post(&mut log, Value::Array(batch.to_vec())));
        }
        let entries = log.entries();
        assert_eq!(entries.len(), KEEP + KEEP_RESOURCES);
        assert_eq!(entries[0].url, "http://localhost/5", "the first five went");
        assert_eq!(log.dropped(), 7);
        // The oldest's later parts go nowhere.
        assert!(!post(&mut log, json!([{ "key": "d:0", "status": 200 }])));
        // What the page never posted counts too.
        assert!(post(&mut log, json!([{ "skipped": 3 }])));
        let text = list_text(&log, &Filter::default());
        assert!(text.starts_with("(10 more not kept"), "{text}");
        // Cleared, ids go on.
        log.clear();
        assert_eq!(list_text(&log, &Filter::default()), "No requests yet.");
        fetch(&mut log, "d:new", "http://localhost/new", 0.0);
        assert_eq!(
            log.entries()[0].id as usize,
            KEEP + 5 + KEEP_RESOURCES + 2 + 1
        );
        assert_eq!(log.dropped(), 0);
    }

    #[test]
    fn lists_filter_by_failure_kind_and_address() {
        let mut log = Log::default();
        post(
            &mut log,
            json!([
                { "key": "a", "kind": "fetch", "method": "GET", "url": "http://localhost/api/ok", "start": 1.0, "status": 200 },
                { "key": "b", "kind": "xhr", "method": "PUT", "url": "http://localhost/api/Broken", "start": 2.0, "status": 500 },
                { "key": "c", "kind": "fetch", "method": "GET", "url": "http://localhost/api/gone", "start": 3.0, "error": "failed" },
                { "key": "d", "kind": "fetch", "method": "GET", "url": "http://localhost/api/cancel", "start": 4.0, "error": "aborted" },
                { "key": "e", "kind": "ws", "url": "ws://localhost/live", "start": 5.0, "status": 101 },
                { "key": "f", "kind": "resource", "initiator": "img", "url": "http://localhost/a.png", "start": 0.5, "duration": 2.0, "size": 2048.0 },
            ]),
        );
        let ids = |f: Filter| -> Vec<u32> {
            list_json(&log, &f)["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["id"].as_u64().unwrap() as u32)
                .collect()
        };
        assert_eq!(
            ids(Filter::default()),
            [6, 1, 2, 3, 4, 5],
            "by when each began"
        );
        assert_eq!(
            ids(Filter {
                failed: true,
                ..Filter::default()
            }),
            [2, 3]
        );
        assert_eq!(
            ids(Filter {
                kind: Some(Kind::Ws),
                ..Filter::default()
            }),
            [5]
        );
        assert_eq!(
            ids(Filter {
                text: Some("broken".into()),
                ..Filter::default()
            }),
            [2]
        );
        assert_eq!(
            list_text(
                &log,
                &Filter {
                    text: Some("nothing".into()),
                    ..Filter::default()
                }
            ),
            "No requests match."
        );
        let text = list_text(&log, &Filter::default());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "   6  img           -     2 ms    2.0 KB  http://localhost/a.png"
        );
        assert!(lines[3].contains(" failed "), "{}", lines[3]);
        assert!(
            lines[5].contains("WS         open     open      0 fr"),
            "{}",
            lines[5]
        );
    }

    #[test]
    fn columns_line_up_whatever_the_state_and_a_cut_body_says_at_least() {
        let mut log = Log::default();
        post(
            &mut log,
            json!([
                { "key": "a", "kind": "fetch", "method": "GET", "url": "http://localhost/a", "start": 1.0, "error": "aborted", "duration": 999.6 },
                { "key": "b", "kind": "xhr", "method": "GET", "url": "http://localhost/b", "start": 2.0, "error": "timeout", "duration": 12.0 },
                { "key": "c", "kind": "fetch", "method": "GET", "url": "http://localhost/c", "start": 3.0, "status": 200,
                  "responseBody": { "text": "x", "size": 8200.0, "atLeast": true, "cut": true } },
            ]),
        );
        let text = list_text(&log, &Filter::default());
        let at: Vec<usize> = text
            .lines()
            .map(|l| l[..l.find("http").unwrap()].chars().count())
            .collect();
        assert!(at.windows(2).all(|w| w[0] == w[1]), "{text}");
        assert!(text.contains("aborted    1.0 s"), "{text}");
        assert!(text.contains("≥8.0 KB"), "{text}");
        assert!(entry_text(log.get(3).unwrap(), 3.0).contains("(at least 8.0 KB):"));
        assert_eq!(Kind::parse("WS"), Some(Kind::Ws));
        assert_eq!(
            ask(&["--type".into(), "Fetch".into()]),
            Ok(Ask::List {
                filter: Filter {
                    kind: Some(Kind::Fetch),
                    ..Filter::default()
                },
                clear: false
            })
        );
    }

    #[test]
    fn a_socket_keeps_its_latest_frames_and_how_it_closed() {
        let mut log = Log::default();
        post(
            &mut log,
            json!([{ "key": "w", "kind": "ws", "method": "GET", "url": "ws://localhost:5173/", "start": 0.0 }]),
        );
        post(&mut log, json!([{ "key": "w", "status": 101 }]));
        for n in 1..=(FRAMES as u32 + 10) {
            let frame = if n % 2 == 0 {
                json!({ "n": n, "out": true, "at": f64::from(n), "text": "ping", "size": 4.0 })
            } else {
                json!({ "n": n, "out": false, "at": f64::from(n), "size": 1024.0 })
            };
            assert!(post(&mut log, json!([{ "key": "w", "frame": frame }])));
        }
        // A gap: frames past network.js's limit.
        post(
            &mut log,
            json!([{ "key": "w", "frame": { "n": 100, "out": false, "at": 100.0, "text": "x".repeat(5000), "size": 5000.0 } }]),
        );
        post(
            &mut log,
            json!([{ "key": "w", "close": { "code": 1006, "reason": "", "clean": false }, "duration": 2500.0 }]),
        );
        let e = log.get(1).unwrap();
        assert_eq!((e.frames.len(), e.frame_count), (FRAMES, 100));
        assert_eq!(
            e.frames
                .back()
                .unwrap()
                .text
                .as_ref()
                .unwrap()
                .chars()
                .count(),
            FRAME
        );
        assert!(e.failed());
        assert!(
            list_line(e).contains("WS         1006    2.5 s    100 fr"),
            "{}",
            list_line(e)
        );
        let full = entry_text(e, 3000.0);
        assert!(full.contains("Closed: 1006, not cleanly"), "{full}");
        assert!(full.contains("Frames (100, the last 50 kept):"), "{full}");
        assert!(full.contains("+12 ms ↑ 4 B ping"), "{full}");
        assert!(full.contains("+13 ms ↓ 1.0 KB [binary]"), "{full}");
    }

    #[test]
    fn whatever_a_page_posts_is_bounded_and_never_reaches_the_terminal_raw() {
        let mut log = Log::default();
        for no in [
            "",
            "{}",
            "null",
            r#"{"key":"a","kind":"fetch","url":"u"}"#,
            r#"[{"key":"a","kind":"fetch","url":1}]"#,
            r#"[{"key":"a","kind":"fetch","url":"u","status":70000}]"#,
            r#"[{"key":"","kind":"fetch","url":"u"}]"#,
            r#"[{"kind":"fetch","url":"u"}]"#,
        ] {
            assert!(!log.add(no), "{no}");
        }
        assert!(!log.add(&format!(
            r#"[{{"key":"a","kind":"fetch","url":"{}"}}]"#,
            "x".repeat(MAX_LINE)
        )));
        let emoji = "😀".repeat(3 * BODY);
        let many: Vec<(String, String)> = (0..300)
            .map(|i| (format!("x-{i}"), "v".repeat(VALUE + 10)))
            .collect();
        assert!(post(
            &mut log,
            json!([{ "key": "a", "kind": "fetch", "method": "GET", "url": format!("http://localhost/{emoji}"),
                     "start": 0.0, "responseHeaders": many,
                     "responseBody": { "text": format!("ok\u{1b}[2J{emoji}"), "size": 1.0 } }])
        ));
        let e = log.get(1).unwrap();
        assert_eq!(e.url.chars().count(), URL);
        assert_eq!(e.response_headers[0].1.chars().count(), VALUE);
        let held: usize = e
            .response_headers
            .iter()
            .map(|(k, v)| k.chars().count() + v.chars().count())
            .sum();
        assert!(
            held <= HEADERS_TEXT && e.response_headers.len() == 7,
            "{held}"
        );
        let body = e.response_body.as_ref().unwrap();
        assert!(body.cut && body.text.as_ref().unwrap().chars().count() == BODY);
        let full = entry_text(e, 0.0);
        assert!(!full.contains('\u{1b}') && full.contains("ok\u{FFFD}[2J"));
        assert!(full.contains(&format!("[cut: the first {BODY} characters]")));
        // Random bytes never panic.
        let mut seed: u64 = 7;
        for _ in 0..5000 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let n = (seed >> 58) as usize;
            let junk: String = (0..n)
                .map(|k| char::from(b"[]{}\":,keyindu1 \\"[((seed >> (k % 50)) as usize + k) % 17]))
                .collect();
            let _ = log.add(&junk);
        }
    }

    #[test]
    fn one_entry_or_the_list_answers_with_json_beside_the_text_and_clear_empties_it() {
        let mut log = Log::default();
        fetch(&mut log, "a", "http://localhost/api", 0.0);
        let (text, data) = answer(&mut log, &Ask::One(1), 10.0, true).unwrap();
        assert_eq!(
            (data["id"].clone(), data["kind"].clone()),
            (json!(1), json!("fetch"))
        );
        assert!(data.get("key").is_none(), "the page's own key stays inside");
        assert!(text.starts_with("#1 GET http://localhost/api (fetch)\nWaiting for an answer"));
        assert!(answer(&mut log, &Ask::One(2), 0.0, true)
            .unwrap_err()
            .contains("No request #2"));
        let (text, data) = answer(
            &mut log,
            &Ask::List {
                filter: Filter::default(),
                clear: true,
            },
            0.0,
            false,
        )
        .unwrap();
        assert_eq!(data["entries"].as_array().unwrap().len(), 1);
        assert_eq!(data["entries"][0]["frameCount"], 0);
        assert!(text.contains("http://localhost/api"));
        assert!(log.entries().is_empty());
    }

    #[test]
    fn credential_headers_are_hidden_unless_revealed() {
        let mut log = Log::default();
        assert!(post(
            &mut log,
            json!([{ "key": "a", "kind": "fetch", "method": "GET", "url": "http://localhost/api", "start": 0.0,
                "requestHeaders": [["Authorization", "Bearer t0ken"], ["X-Api-Key", "k3y"], ["Accept", "*/*"]],
                "responseHeaders": [["Set-Cookie", "sid=s3ss"], ["Content-Type", "text/plain"]] }])
        ));
        let (text, data) = answer(&mut log, &Ask::One(1), 0.0, false).unwrap();
        for hidden in ["t0ken", "k3y", "s3ss"] {
            assert!(
                !text.contains(hidden) && !data.to_string().contains(hidden),
                "{hidden}"
            );
        }
        assert!(text.contains("  Authorization: [redacted]") && text.contains("  Accept: */*"));
        assert!(text.contains("  Content-Type: text/plain"));
        let (shown, _) = answer(&mut log, &Ask::One(1), 0.0, true).unwrap();
        assert!(shown.contains("Bearer t0ken") && shown.contains("sid=s3ss"));
    }

    #[test]
    fn a_dev_page_busy_for_long_stays_within_its_rings() {
        let mut log = Log::default();
        let resources = |from: usize, n: usize| -> Vec<Value> {
            (from..from + n)
                .map(|i| {
                    json!({ "key": format!("d:r{i}"), "kind": "resource", "initiator": "script",
                                 "url": format!("http://localhost:5173/src/Thing{i}.tsx?import"),
                                 "start": i as f64, "duration": 4.0, "size": 1200.0 })
                })
                .collect()
        };
        for batch in resources(0, 641).chunks(100) {
            assert!(post(&mut log, Value::Array(batch.to_vec())));
        }
        assert!(post(
            &mut log,
            json!([{ "key": "d:hmr", "kind": "ws", "method": "GET", "url": "ws://localhost:5173/?token=t-1", "start": 1.0 }])
        ));
        let headers: Vec<(String, String)> = (0..120)
            .map(|i| (format!("x-h{i}"), "v".repeat(140)))
            .collect();
        let (mut requests, mut loads, mut frames) = (1usize, 641usize, 0u32);
        for s in 0..900usize {
            let key = format!("d:p{s}");
            let start = 1000.0 * s as f64;
            assert!(post(
                &mut log,
                json!([{ "key": key, "kind": "fetch", "method": "GET", "url": format!("http://localhost:5173/api/poll?n={s}"),
                         "start": start, "requestHeaders": headers }])
            ));
            assert!(post(
                &mut log,
                json!([{ "key": key, "status": 200, "statusText": "OK", "responseHeaders": headers }])
            ));
            assert!(post(
                &mut log,
                json!([{ "key": key, "duration": 5.0,
                         "responseBody": { "type": "application/json", "text": "é😀".repeat(9000), "size": 50000.0, "cut": true } }])
            ));
            requests += 1;
            if s % 5 == 0 {
                assert!(post(
                    &mut log,
                    json!([{ "key": format!("d:x{s}"), "kind": "xhr", "method": "POST", "url": "http://localhost:5173/api/metrics",
                             "start": start + 1.0, "status": 204, "duration": 2.0 }])
                ));
                requests += 1;
            }
            if s % 30 == 0 {
                for _ in 0..21 {
                    frames += 1;
                    // Once pushed out, the socket's frames go nowhere.
                    let _ = post(
                        &mut log,
                        json!([{ "key": "d:hmr", "frame": { "n": frames, "out": false, "at": start, "text": "u".repeat(5000), "size": 5000.0 } }]),
                    );
                }
                for batch in resources(10_000 + s * 40, 40).chunks(100) {
                    assert!(post(&mut log, Value::Array(batch.to_vec())));
                }
                loads += 40;
            }
        }
        assert_eq!(log.requests.len(), KEEP);
        assert_eq!(log.resources.len(), KEEP_RESOURCES);
        assert_eq!(
            log.dropped(),
            (requests - KEEP + loads - KEEP_RESOURCES) as u64
        );
        for e in log.entries() {
            for list in [&e.request_headers, &e.response_headers] {
                let held: usize = list
                    .iter()
                    .map(|(k, v)| k.chars().count() + v.chars().count())
                    .sum();
                assert!(list.len() <= HEADERS && held <= HEADERS_TEXT);
            }
            for b in [&e.request_body, &e.response_body].into_iter().flatten() {
                assert!(b.text.as_ref().is_none_or(|t| t.chars().count() <= BODY));
            }
            assert!(e.frames.len() <= FRAMES);
        }
        let list = list_text(&log, &Filter::default());
        assert_eq!(list.lines().count(), 1 + KEEP + KEEP_RESOURCES);
        let last = log.get(log.next).unwrap();
        assert!(entry_text(last, 1e9).len() < 200_000);
    }

    #[test]
    fn no_field_a_page_posts_reaches_the_terminal_raw() {
        let mut log = Log::default();
        // fetch("/", { method: "\x1b[2J" }) posts its start before WebKit refuses the method; a
        // server's Content-Type may carry byte 0x9b, which Headers hands over as U+009B.
        assert!(post(
            &mut log,
            json!([{ "key": "e", "kind": "fetch", "method": "\u{1b}[2J", "url": "http://localhost/", "start": 0.0,
                     "error": "\u{9b}31m", "responseBody": { "type": "text/\u{1b}]8;;http://x.test\u{7}", "size": 1.0 } }])
        ));
        assert!(post(
            &mut log,
            json!([{ "key": "r", "kind": "resource", "initiator": "\u{1b}[1m", "url": "http://localhost/a.js", "start": 1.0 }])
        ));
        let mut shown = vec![list_text(&log, &Filter::default())];
        shown.extend(log.entries().into_iter().map(|e| entry_text(e, 0.0)));
        for s in shown {
            assert!(
                !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t'),
                "{s:?}"
            );
        }
    }

    #[test]
    fn a_post_with_a_lone_surrogate_is_the_pages_own_and_refused() {
        // network.js posts one as U+FFFD (networkScriptEdges.test.ts); a pair is no lone one.
        let mut log = Log::default();
        assert!(!log.add(
            r#"[{"key":"s","kind":"fetch","method":"POST","url":"http://localhost/save","requestBody":{"text":"{\"note\":\"hi \ud83d\"}","size":17,"cut":false}}]"#
        ));
        assert!(log.add(
            r#"[{"key":"t","kind":"fetch","url":"u","requestBody":{"text":"\ud83d\ude00 \\ud83d"}}]"#
        ));
        let body = log.get(1).unwrap().request_body.clone().unwrap();
        assert_eq!(body.text.as_deref(), Some("😀 \\ud83d"));
        // Text shown in another order than it reads is no text of the page's either.
        assert!(log.add(
            r#"[{"key":"b","kind":"fetch","method":"GET","url":"http://localhost/\u202egnp.exe","start":0}]"#
        ));
        assert_eq!(log.get(2).unwrap().url, "http://localhost/\u{FFFD}gnp.exe");
    }

    #[test]
    fn credentials_in_bodies_are_hidden_unless_revealed() {
        let mut log = Log::default();
        assert!(post(
            &mut log,
            json!([{ "key": "a", "kind": "fetch", "method": "POST", "url": "http://localhost/oauth/token", "start": 0.0,
                "status": 200,
                "requestBody": { "text": "grant_type=authorization_code&code=c0de&client_secret=s3cr3t", "size": 60.0 },
                "responseBody": { "text": "{\"access_token\":\"t0k3n\",\"refresh_token\":\"r3fr3sh\"}", "type": "application/json", "size": 50.0 } }])
        ));
        let (text, data) = answer(&mut log, &Ask::One(1), 10.0, false).unwrap();
        for hidden in ["c0de", "s3cr3t", "t0k3n", "r3fr3sh"] {
            assert!(
                !text.contains(hidden) && !data.to_string().contains(hidden),
                "{hidden}: {text}"
            );
        }
    }
}
