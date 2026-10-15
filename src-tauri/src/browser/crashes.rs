//! A page's WebContent process ending (macos/crashes.rs): loaded again, but not again and
//! again for a page that brings it down as it loads, or every little while. Apart from AppKit,
//! so it can be tested.

use std::time::{Duration, Instant};

/// Ending again this soon after its reload is a loop: the page is left stopped.
const LOOP: Duration = Duration::from_secs(30);
/// And so is ending this often in WINDOW, however far apart: each reload holds its parking.
const TIMES: usize = 3;
const WINDOW: Duration = Duration::from_secs(5 * 60);
/// A reload counts as running this long even before WebKit says it's loading.
const STARTING: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct Crashes {
    /// When it ended lately, within WINDOW, and when it was last reloaded for it.
    ended: Vec<Instant>,
    reloaded: Option<Instant>,
    /// Why it keeps ending, once it's left stopped.
    stopped: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Ended {
    Reload,
    /// Within LOOP of its reload, or the TIMES-th in WINDOW: left stopped.
    Stop,
    /// Stopped already.
    Nothing,
}

impl Crashes {
    pub fn ended(&mut self, why: &str, now: Instant) -> Ended {
        if self.stopped.is_some() {
            return Ended::Nothing;
        }
        let gone = |at: Instant| now.saturating_duration_since(at);
        self.ended.retain(|&at| gone(at) < WINDOW);
        self.ended.push(now);
        let soon = self.reloaded.is_some_and(|at| gone(at) < LOOP);
        if soon || self.ended.len() >= TIMES {
            self.reloaded = None;
            self.stopped = Some(why.into());
            return Ended::Stop;
        }
        self.reloaded = Some(now);
        Ended::Reload
    }

    /// Sent somewhere by its user or agent: ending after this reloads it again.
    pub fn sent(&mut self) {
        *self = Self::default();
    }

    /// Whether its reload after an ending is still running, as WebKit's `loading` says: no
    /// park or close may take the view meanwhile.
    pub fn recovering(&self, now: Instant, loading: bool) -> bool {
        self.reloaded.is_some_and(|at| {
            let gone = now.saturating_duration_since(at);
            gone < STARTING || (loading && gone < LOOP)
        })
    }

    /// Why it was left stopped.
    pub fn stopped(&self) -> Option<&str> {
        self.stopped.as_deref()
    }
}

/// WebKit's _WKProcessTerminationReason, as said; None where it gave none.
pub fn reason(code: Option<isize>) -> &'static str {
    match code {
        Some(0) => "out of memory",
        Some(1) => "too much CPU",
        Some(2) => "ended by the app",
        Some(3) => "crashed",
        Some(4) => "its process crashed too often, and WebKit won't start it again",
        Some(_) => "for a reason WebKit didn't name",
        None => "crashed, or memory ran short",
    }
}

/// Memory is short on this Mac (kern.memorystatus_vm_pressure_level: 1 normal, 2 warning, 4
/// critical): a page that just ended isn't kept from parking.
pub fn pressed(level: i32) -> bool {
    level >= 2
}

pub fn reloaded(why: &str) -> String {
    format!("the page's process ended ({why}) and was reloaded; console and network logs restarted")
}

pub fn stopped(why: &str) -> String {
    format!("the page's process keeps ending ({why}): it's left stopped, not reloaded")
}

/// The page's error, as its tab shows it.
pub fn keeps_crashing(why: &str) -> String {
    format!(
        "The page keeps crashing ({why}): its process ended within {} s of a reload, or {TIMES} \
         times in {} minutes.",
        LOOP.as_secs(),
        WINDOW.as_secs() / 60
    )
}

/// What an agent's command on the stopped page says.
pub fn try_again(why: &str) -> String {
    format!(
        "{} `reload` or `open <url>` tries again.",
        keeps_crashing(why)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ending_reloads_and_another_soon_after_leaves_it_stopped_till_sent() {
        let mut c = Crashes::default();
        let t0 = Instant::now();
        assert_eq!(c.ended("crashed", t0), Ended::Reload);
        assert_eq!(c.ended("out of memory", t0 + LOOP / 2), Ended::Stop);
        assert_eq!(c.stopped(), Some("out of memory"));
        assert_eq!(
            c.ended("crashed", t0 + LOOP * 3),
            Ended::Nothing,
            "stays stopped"
        );
        assert!(!c.recovering(t0 + LOOP / 2, true));
        c.sent();
        assert_eq!(c.stopped(), None);
        assert_eq!(c.ended("crashed", t0 + LOOP * 4), Ended::Reload);
    }

    #[test]
    fn a_page_ending_every_little_while_stops_on_its_third_in_the_window() {
        let mut c = Crashes::default();
        let t0 = Instant::now();
        let every = LOOP + Duration::from_secs(1);
        assert_eq!(c.ended("crashed", t0), Ended::Reload);
        assert_eq!(c.ended("crashed", t0 + every), Ended::Reload);
        assert_eq!(c.ended("crashed", t0 + every * 2), Ended::Stop);
        // Far apart, each reloads.
        let mut c = Crashes::default();
        for i in 0..5 {
            assert_eq!(c.ended("crashed", t0 + WINDOW * i), Ended::Reload, "{i}");
        }
    }

    #[test]
    fn a_reload_is_recovering_while_it_starts_or_loads_never_past_the_loop() {
        let mut c = Crashes::default();
        let t0 = Instant::now();
        assert!(!c.recovering(t0, true), "nothing ended");
        c.ended("crashed", t0);
        assert!(c.recovering(t0, false), "WebKit may not say loading yet");
        assert!(c.recovering(t0 + STARTING / 2, false));
        assert!(!c.recovering(t0 + STARTING * 2, false), "loaded");
        assert!(c.recovering(t0 + STARTING * 2, true));
        assert!(!c.recovering(t0 + LOOP, true), "a load that never ends");
    }

    #[test]
    fn the_reason_and_what_is_told() {
        assert_eq!(reason(Some(0)), "out of memory");
        assert_eq!(reason(Some(3)), "crashed");
        assert_eq!(reason(Some(9)), "for a reason WebKit didn't name");
        assert_eq!(
            reloaded(reason(Some(0))),
            "the page's process ended (out of memory) and was reloaded; console and network \
             logs restarted"
        );
        assert_eq!(
            reason(Some(4)),
            "its process crashed too often, and WebKit won't start it again"
        );
        assert_eq!(
            stopped("crashed"),
            "the page's process keeps ending (crashed): it's left stopped, not reloaded"
        );
        assert_eq!(
            try_again("crashed"),
            "The page keeps crashing (crashed): its process ended within 30 s of a reload, or 3 \
             times in 5 minutes. `reload` or `open <url>` tries again."
        );
        assert!(!pressed(1) && pressed(2) && pressed(4));
    }
}
