//! The open views and the memory policy over them, apart from AppKit so it can be tested. Each
//! live view is a WebContent process (about 20 MB for a plain page, 150 MB and up for a dev app):
//! a few hidden ones stay alive to come back at once, and the rest park. A parked view is closed,
//! its address and a picture of it kept, and loads again when its tab shows.

/// How many hidden views stay alive until settings say (browserLiveHidden).
const LIVE_HIDDEN: usize = 2;
/// How many agents' tabs draw out of sight at once until settings say (browserAgentDrawn), each
/// in a panel at full rate: past it, the least recently used sleeps.
const AWAKE_DRAWN: usize = 12;

pub struct Registry<V> {
    views: Vec<Entry<V>>,
    live_hidden: usize,
    awake_drawn: usize,
    /// Every hide gets the next number: a park timer set by one tells a later one apart.
    hides: u64,
    /// Every use of an awake view gets the next number, to tell which went longest unused.
    uses: u64,
}

struct Entry<V> {
    id: String,
    root: String,
    state: State<V>,
    /// The page as it last showed, taken as it went out of sight: what it parks with.
    picture: Option<String>,
    /// In use by the agents of the pane that opened it: out of its tab's sight, it's Offscreen.
    awake: bool,
    /// Shown, with something of the app page's over it for now (a menu, the maximized terminal).
    covered: bool,
    /// The use (`touch`) it last had.
    used: u64,
}

enum State<V> {
    /// Placed in its tab, or about to be.
    Shown(V),
    /// Out of sight since the hide numbered so.
    Hidden(V, u64),
    /// Out of its tab's sight but drawn, in a panel of its own off every screen, while it's
    /// awake: it neither parks nor counts toward the cap.
    Offscreen(V),
    Parked(Parked),
}

/// What the view's host does as the registry moves it.
#[derive(Debug, PartialEq)]
pub enum Step {
    Stay,
    /// Drawn in its panel, off every screen.
    Offscreen,
    /// Hidden under what covers it, still its tab's.
    Cover,
    /// Out of sight since the hide numbered so: its park timer starts, and the hidden past the
    /// cap park.
    Hide(u64, Vec<(String, u64)>),
}

/// A parked view: where it was, and how it looked.
#[derive(Debug, PartialEq)]
pub struct Parked {
    pub url: String,
    /// A JPEG data URL, when WebKit gave one.
    pub snapshot: Option<String>,
}

impl<V> Entry<V> {
    fn view(&self) -> Option<&V> {
        match &self.state {
            State::Shown(v) | State::Hidden(v, _) | State::Offscreen(v) => Some(v),
            State::Parked(_) => None,
        }
    }

    /// The state, as `f` makes it from the one it had.
    fn restate(&mut self, f: impl FnOnce(State<V>) -> State<V>) {
        let was = std::mem::replace(&mut self.state, State::Parked(placeholder()));
        self.state = f(was);
    }

    fn into_view(self) -> Option<V> {
        match self.state {
            State::Shown(v) | State::Hidden(v, _) | State::Offscreen(v) => Some(v),
            State::Parked(_) => None,
        }
    }
}

impl<V> Registry<V> {
    pub const fn new() -> Self {
        Self {
            views: Vec::new(),
            live_hidden: LIVE_HIDDEN,
            awake_drawn: AWAKE_DRAWN,
            hides: 0,
            uses: 0,
        }
    }

    fn entry(&mut self, id: &str) -> Option<&mut Entry<V>> {
        self.views.iter_mut().find(|e| e.id == id)
    }

    /// The worktree a view, live or parked, was made for.
    pub fn root_of(&self, id: &str) -> Option<&str> {
        self.views
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.root.as_str())
    }

    /// A live view; a parked one has none.
    pub fn get(&self, id: &str) -> Option<&V> {
        self.views.iter().find(|e| e.id == id)?.view()
    }

    /// A new view, shown, or Offscreen when it's an agent's awake; one parked under the same id
    /// makes way.
    pub fn insert(&mut self, id: &str, root: &str, view: V, awake: bool) {
        self.views.retain(|e| e.id != id);
        self.views.push(Entry {
            id: id.into(),
            root: root.into(),
            state: if awake {
                State::Offscreen(view)
            } else {
                State::Shown(view)
            },
            picture: None,
            awake,
            covered: false,
            used: 0,
        });
        self.touch(id);
    }

    /// Takes a parked view out, for its tab to load again.
    pub fn unpark(&mut self, id: &str) -> Option<Parked> {
        let at = self.views.iter().position(|e| e.id == id)?;
        if !matches!(self.views[at].state, State::Parked(_)) {
            return None;
        }
        match self.views.remove(at).state {
            State::Parked(p) => Some(p),
            _ => None,
        }
    }

    /// Closed, parked or not; the view when it was live.
    pub fn remove(&mut self, id: &str) -> Option<V> {
        let at = self.views.iter().position(|e| e.id == id)?;
        self.views.remove(at).into_view()
    }

    /// Every view of a worktree, out of the registry.
    pub fn remove_root(&mut self, root: &str) -> Vec<V> {
        let (gone, kept): (Vec<_>, _) = std::mem::take(&mut self.views)
            .into_iter()
            .partition(|e| e.root == root);
        self.views = kept;
        gone.into_iter().filter_map(Entry::into_view).collect()
    }

    pub fn take_all(&mut self) -> Vec<V> {
        std::mem::take(&mut self.views)
            .into_iter()
            .filter_map(Entry::into_view)
            .collect()
    }

    pub fn live(&self) -> Vec<&V> {
        self.views.iter().filter_map(Entry::view).collect()
    }

    /// The live views of the worktree that holds `dir`.
    pub fn within(&self, dir: &str) -> Vec<&V> {
        let inside = |root: &str| dir == root || dir.starts_with(&format!("{root}/"));
        self.views
            .iter()
            .filter(|e| inside(&e.root))
            .filter_map(Entry::view)
            .collect()
    }

    pub fn show(&mut self, id: &str) {
        if let Some(e) = self.entry(id) {
            e.covered = false;
            e.restate(|s| match s {
                State::Hidden(v, _) | State::Offscreen(v) => State::Shown(v),
                s => s,
            });
        }
    }

    /// Something of the app page's over a shown view: an awake one is drawn off-screen meanwhile,
    /// as hidden WebKit stops its frames; any other hides.
    pub fn cover(&mut self, id: &str) -> Step {
        match self.entry(id) {
            Some(e) if matches!(e.state, State::Shown(_)) => {
                e.covered = true;
                if e.awake {
                    Step::Offscreen
                } else {
                    Step::Cover
                }
            }
            _ => Step::Stay,
        }
    }

    /// In use by its pane's agents: out of sight or covered, it's drawn off-screen from now on.
    /// None when it was awake already, or isn't live.
    pub fn wake(&mut self, id: &str) -> Option<Step> {
        let e = self.entry(id).filter(|e| e.view().is_some() && !e.awake)?;
        e.awake = true;
        let mut step = if e.covered {
            Step::Offscreen
        } else {
            Step::Stay
        };
        e.restate(|s| match s {
            State::Hidden(v, _) | State::Offscreen(v) => {
                step = Step::Offscreen;
                State::Offscreen(v)
            }
            s => s,
        });
        Some(step)
    }

    /// No longer in use by its agents: out of sight it hides as any other, covered it hides
    /// under the cover. None when it wasn't awake.
    pub fn sleep(&mut self, id: &str) -> Option<Step> {
        let e = self.entry(id).filter(|e| e.awake)?;
        e.awake = false;
        if matches!(e.state, State::Offscreen(_)) {
            return self.hide(id).map(|(since, over)| Step::Hide(since, over));
        }
        Some(if e.covered { Step::Cover } else { Step::Stay })
    }

    /// Out of sight: the hide's number, for its park timer, and the hidden views past the cap
    /// (the longest hidden first) with theirs. None for a view that isn't live, or one awake,
    /// which goes Offscreen instead.
    pub fn hide(&mut self, id: &str) -> Option<(u64, Vec<(String, u64)>)> {
        self.hides += 1;
        let since = self.hides;
        let e = self.entry(id)?;
        e.view()?;
        e.covered = false;
        let awake = e.awake;
        e.restate(|s| match s {
            State::Shown(v) | State::Hidden(v, _) | State::Offscreen(v) => {
                if awake {
                    State::Offscreen(v)
                } else {
                    State::Hidden(v, since)
                }
            }
            s => s,
        });
        (!awake).then(|| (since, self.over_cap()))
    }

    /// Drawn in its panel for its agents: out of sight, or under a cover.
    pub fn offscreen(&self, id: &str) -> bool {
        self.views.iter().any(|e| {
            e.id == id
                && (matches!(e.state, State::Offscreen(_))
                    || e.awake && e.covered && matches!(e.state, State::Shown(_)))
        })
    }

    /// An agent's command on it now.
    pub fn touch(&mut self, id: &str) {
        self.uses += 1;
        let used = self.uses;
        if let Some(e) = self.entry(id) {
            e.used = used;
        }
    }

    /// Every view drawn off-screen, the longest unused first.
    pub fn drawn(&self) -> Vec<String> {
        let mut drawn: Vec<&Entry<V>> = self
            .views
            .iter()
            .filter(|e| self.offscreen(&e.id))
            .collect();
        drawn.sort_by_key(|e| e.used);
        drawn.into_iter().map(|e| e.id.clone()).collect()
    }

    /// The views to sleep to bring those drawn off-screen down to the cap, the longest unused
    /// first, passing over `keep` and any `pinned`: a pinned one stays awake, over the cap if
    /// need be.
    pub fn past_cap(&self, keep: &str, pinned: impl Fn(&V) -> bool) -> Vec<String> {
        let drawn = self.drawn();
        let over = drawn.len().saturating_sub(self.awake_drawn);
        drawn
            .into_iter()
            .filter(|id| id != keep && self.get(id).is_some_and(|v| !pinned(v)))
            .take(over)
            .collect()
    }

    /// The views drawn off-screen past `cap`, the longest unused first: they sleep.
    #[cfg(test)]
    fn over_awake(&self, cap: usize) -> Vec<String> {
        let mut drawn = self.drawn();
        let over = drawn.len().saturating_sub(cap);
        drawn.truncate(over);
        drawn
    }

    /// How many may draw off-screen at once (`over_awake`).
    #[cfg(test)]
    pub fn awake_drawn(&self) -> usize {
        self.awake_drawn
    }

    pub fn set_awake_drawn(&mut self, n: usize) {
        self.awake_drawn = n;
    }

    pub fn awake(&self, id: &str) -> bool {
        self.views.iter().any(|e| e.id == id && e.awake)
    }

    #[cfg(test)]
    fn covered(&self, id: &str) -> bool {
        self.views.iter().any(|e| e.id == id && e.covered)
    }

    /// Fewer to keep alive: those past it park now.
    pub fn set_live_hidden(&mut self, n: usize) -> Vec<(String, u64)> {
        self.live_hidden = n;
        self.over_cap()
    }

    fn over_cap(&self) -> Vec<(String, u64)> {
        let mut hidden: Vec<(String, u64)> = self
            .views
            .iter()
            .filter_map(|e| match e.state {
                State::Hidden(_, since) => Some((e.id.clone(), since)),
                _ => None,
            })
            .collect();
        // The most recently hidden stay.
        hidden.sort_by_key(|h| std::cmp::Reverse(h.1));
        hidden.split_off(self.live_hidden.min(hidden.len()))
    }

    /// A parked view, left where it is.
    pub fn parked(&self, id: &str) -> Option<&Parked> {
        self.views.iter().find_map(|e| match &e.state {
            State::Parked(p) if e.id == id => Some(p),
            _ => None,
        })
    }

    /// Whether the view is on show in its tab (something may still be drawn over it).
    pub fn shown(&self, id: &str) -> bool {
        self.views
            .iter()
            .any(|e| e.id == id && matches!(e.state, State::Shown(_)))
    }

    /// The hidden views, with their hides' numbers.
    pub fn hidden(&self) -> Vec<(String, u64)> {
        self.views
            .iter()
            .filter_map(|e| match e.state {
                State::Hidden(_, since) => Some((e.id.clone(), since)),
                _ => None,
            })
            .collect()
    }

    /// The page's picture, which may come after it parked: then it's the parked one's.
    pub fn set_picture(&mut self, id: &str, picture: Option<String>) {
        if let Some(e) = self.entry(id) {
            match &mut e.state {
                State::Parked(p) => p.snapshot = picture,
                _ => e.picture = picture,
            }
        }
    }

    /// Whether the hide numbered `since` still stands: the view hasn't shown or closed since.
    pub fn still_hidden(&self, id: &str, since: u64) -> bool {
        self.views
            .iter()
            .any(|e| e.id == id && matches!(e.state, State::Hidden(_, s) if s == since))
    }

    /// Parks a view hidden since `since`, giving its view to close; None when it showed again
    /// (or closed) since. With no picture of its own, it keeps the one taken as it hid.
    pub fn park(&mut self, id: &str, since: u64, mut parked: Parked) -> Option<V> {
        if !self.still_hidden(id, since) {
            return None;
        }
        let e = self.entry(id)?;
        if parked.snapshot.is_none() {
            parked.snapshot = e.picture.take();
        }
        let mut view = None;
        e.restate(|s| match s {
            State::Hidden(v, _) => {
                view = Some(v);
                State::Parked(parked)
            }
            s => s,
        });
        view
    }
}

const fn placeholder() -> Parked {
    Parked {
        url: String::new(),
        snapshot: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parked(url: &str) -> Parked {
        Parked {
            url: url.into(),
            snapshot: Some("data:image/jpeg;base64,AA==".into()),
        }
    }

    #[test]
    fn the_longest_hidden_park_first_past_the_cap() {
        let mut r = Registry::new();
        for id in ["a", "b", "c", "d"] {
            r.insert(id, "/w/one", id, false);
        }
        let (a, over) = r.hide("a").unwrap();
        assert!(over.is_empty());
        r.hide("b").unwrap();
        // A third hidden one: the first hidden goes.
        let (_, over) = r.hide("c").unwrap();
        assert_eq!(over, [("a".to_string(), a)]);
        assert_eq!(r.park("a", a, parked("http://localhost:5173/")), Some("a"));
        assert_eq!(r.get("a"), None);
        // Shown again, b counts as hidden no more: hiding d keeps c and d.
        r.show("b");
        let (_, over) = r.hide("d").unwrap();
        assert!(over.is_empty());
    }

    #[test]
    fn a_lower_cap_parks_the_oldest_at_once() {
        let mut r = Registry::new();
        let mut since = Vec::new();
        for id in ["a", "b", "c"] {
            r.insert(id, "/w", id, false);
        }
        for id in ["a", "b"] {
            since.push(r.hide(id).unwrap().0);
        }
        assert_eq!(
            r.set_live_hidden(1),
            [("a".to_string(), since[0])],
            "the older of two"
        );
        assert_eq!(
            r.set_live_hidden(0),
            [("b".to_string(), since[1]), ("a".to_string(), since[0])]
        );
        assert!(r.set_live_hidden(4).is_empty());
    }

    #[test]
    fn an_awake_view_is_drawn_offscreen_and_neither_parks_nor_counts_until_it_sleeps() {
        let mut r = Registry::new();
        r.insert("new", "/w", 0, true);
        assert!(r.offscreen("new") && r.awake("new") && !r.shown("new"));
        for (id, n) in [("a", 1), ("b", 2), ("c", 3)] {
            r.insert(id, "/w", n, false);
        }
        let (a, _) = r.hide("a").unwrap();
        assert_eq!(r.wake("a"), Some(Step::Offscreen));
        assert_eq!(
            r.wake("b"),
            Some(Step::Stay),
            "an awake view on show stays so"
        );
        assert_eq!(r.wake("a"), None, "already awake");
        assert!(r.offscreen("a") && r.get("a") == Some(&1) && r.shown("b"));
        assert!(r.hidden().is_empty() && !r.still_hidden("a", a));
        assert_eq!(r.park("a", a, parked("http://x.test/")), None);
        // Out of sight while awake: off-screen, not Hidden.
        assert_eq!(r.hide("b"), None);
        assert!(r.offscreen("b"));
        assert!(r.set_live_hidden(0).is_empty(), "none hidden to park");
        // Asleep off-screen: hidden at once, and past the cap of none, parking.
        let Some(Step::Hide(b, over)) = r.sleep("b") else {
            panic!("asleep off-screen, it hides")
        };
        assert_eq!(over, [("b".to_string(), b)]);
        assert!(!r.offscreen("b") && !r.awake("b") && r.sleep("b").is_none());
        let (since, _) = r.hide("c").unwrap();
        r.park("c", since, parked("http://x.test/"));
        assert_eq!(r.wake("c"), None, "a parked view isn't live");
        assert!(r.wake("nope").is_none() && r.sleep("nope").is_none());
    }

    #[test]
    fn past_the_cap_the_longest_unused_off_screen_view_sleeps_first() {
        let mut r = Registry::new();
        for (id, n) in [("a", 1), ("b", 2), ("c", 3), ("d", 4)] {
            r.insert(id, "/w", n, true);
        }
        assert!(r.over_awake(4).is_empty());
        assert_eq!(r.over_awake(3), ["a"]);
        // Used again, a goes last; b is now the longest unused.
        r.touch("a");
        assert_eq!(r.over_awake(2), ["b", "c"]);
        // One on show, or covered and shown, draws in the window: it doesn't count; covered
        // awake, it's in its panel and does.
        r.show("b");
        assert_eq!(r.over_awake(2), ["c"]);
        r.cover("b");
        assert_eq!(r.over_awake(2), ["b", "c"]);
        // Asleep, out of the count.
        r.sleep("c");
        r.sleep("b");
        assert!(r.over_awake(2).is_empty());
        assert_eq!(r.over_awake(0), ["d", "a"]);
        assert_eq!(r.drawn(), r.over_awake(0));
        r.set_awake_drawn(1);
        assert_eq!(r.over_awake(r.awake_drawn()), ["d"]);
    }

    #[test]
    fn past_the_cap_a_view_that_is_kept_or_pinned_stays_awake_and_the_next_sleeps() {
        let mut r = Registry::new();
        for (id, n) in [("a", 1), ("b", 2), ("c", 3), ("d", 4)] {
            r.insert(id, "/w", n, true);
        }
        r.set_awake_drawn(2);
        assert_eq!(r.past_cap("", |_| false), ["a", "b"]);
        // The one a command is about to use is passed over for the next.
        assert_eq!(r.past_cap("a", |_| false), ["b", "c"]);
        // One holding a key (view 2) too: the next longest unused sleeps in its place.
        assert_eq!(r.past_cap("a", |n| *n == 2), ["c", "d"]);
        // All pinned: none sleeps, the cap is exceeded.
        assert!(r.past_cap("", |_| true).is_empty());
        // Within the cap, none.
        r.set_awake_drawn(4);
        assert!(r.past_cap("", |_| false).is_empty());
    }

    #[test]
    fn a_covered_view_hides_under_its_cover_asleep_and_is_drawn_off_screen_awake() {
        let mut r = Registry::new();
        r.insert("a", "/w", 1, false);
        assert_eq!(r.cover("a"), Step::Cover);
        assert!(r.covered("a") && r.shown("a"), "still its tab's");
        // Woken under the cover: drawn, not hidden, for its agents' commands.
        assert_eq!(r.wake("a"), Some(Step::Offscreen));
        assert!(r.shown("a") && r.offscreen("a"));
        assert_eq!(r.sleep("a"), Some(Step::Cover));
        assert!(!r.offscreen("a"));
        assert_eq!(r.wake("a"), Some(Step::Offscreen));
        assert_eq!(r.cover("a"), Step::Offscreen);
        // Shown again: uncovered, on show whatever it is.
        r.show("a");
        assert!(!r.covered("a"));
        assert_eq!(r.sleep("a"), Some(Step::Stay));
        // Out of sight, nothing covers it; one not shown isn't covered.
        r.cover("a");
        r.hide("a").unwrap();
        assert!(!r.covered("a"));
        assert_eq!(r.cover("a"), Step::Stay);
        assert!(!r.covered("a"));
    }

    #[test]
    fn a_timer_from_before_the_view_showed_again_parks_nothing() {
        let mut r = Registry::new();
        r.insert("a", "/w", 1, false);
        let (first, _) = r.hide("a").unwrap();
        r.show("a");
        assert!(!r.still_hidden("a", first));
        assert_eq!(r.park("a", first, parked("http://x.test/")), None);
        // Hidden again: only the new hide's timer counts.
        let (second, _) = r.hide("a").unwrap();
        assert_eq!(r.park("a", first, parked("http://x.test/")), None);
        assert_eq!(r.park("a", second, parked("http://x.test/")), Some(1));
        // Already parked: nothing to park, show or hide.
        assert_eq!(r.park("a", second, parked("http://x.test/")), None);
        assert_eq!(r.hide("a"), None);
        r.show("a");
        assert_eq!(r.get("a"), None);
    }

    #[test]
    fn a_parked_view_comes_back_once_where_it_was() {
        let mut r = Registry::new();
        r.insert("a", "/w", 1, false);
        let (since, _) = r.hide("a").unwrap();
        r.park("a", since, parked("http://localhost:3000/docs"));
        assert_eq!(r.unpark("a"), Some(parked("http://localhost:3000/docs")));
        assert_eq!(r.unpark("a"), None);
        r.insert("a", "/w", 2, false);
        assert_eq!(r.unpark("a"), None, "a live view isn't parked");
        assert_eq!(r.get("a"), Some(&2));
    }

    #[test]
    fn a_park_keeps_the_picture_taken_as_the_view_hid_even_one_that_comes_later() {
        let mut r = Registry::new();
        r.insert("a", "/w", 1, false);
        r.insert("b", "/w", 2, false);
        let (a, _) = r.hide("a").unwrap();
        r.set_picture("a", Some("data:a".into()));
        assert!(!r.shown("a") && r.shown("b"));
        assert_eq!(r.hidden(), [("a".to_string(), a)]);
        let none = Parked {
            url: "http://x.test/".into(),
            snapshot: None,
        };
        assert_eq!(r.park("a", a, none), Some(1));
        assert_eq!(
            r.parked("a").and_then(|p| p.snapshot.as_deref()),
            Some("data:a")
        );
        // Parked before its picture came: the picture still finds it.
        let (b, _) = r.hide("b").unwrap();
        r.park(
            "b",
            b,
            Parked {
                url: "http://y.test/".into(),
                snapshot: None,
            },
        );
        r.set_picture("b", Some("data:b".into()));
        assert_eq!(
            r.unpark("b").and_then(|p| p.snapshot),
            Some("data:b".to_string())
        );
        assert!(r.hidden().is_empty());
    }

    #[test]
    fn a_view_knows_its_worktree_parked_or_not() {
        let mut r = Registry::new();
        r.insert("a", "/w/one", 1, false);
        let (since, _) = r.hide("a").unwrap();
        r.park("a", since, parked("http://x.test/"));
        assert_eq!(r.root_of("a"), Some("/w/one"));
        assert_eq!(r.root_of("b"), None);
    }

    #[test]
    fn closing_takes_parked_views_too_and_gives_only_live_ones() {
        let mut r = Registry::new();
        r.insert("a", "/w/one", 1, false);
        r.insert("b", "/w/one", 2, false);
        r.insert("c", "/w/two", 3, false);
        let (since, _) = r.hide("a").unwrap();
        r.park("a", since, parked("http://x.test/"));
        assert_eq!(r.remove_root("/w/one"), [2]);
        assert_eq!(r.unpark("a"), None);
        assert_eq!(r.remove("c"), Some(3));
        r.insert("d", "/w/one", 4, false);
        let (since, _) = r.hide("d").unwrap();
        r.park("d", since, parked("http://x.test/"));
        assert_eq!(r.remove("d"), None);
        assert_eq!(r.unpark("d"), None);
        assert!(r.take_all().is_empty());
    }

    #[test]
    fn a_worktrees_live_views_are_those_its_folders_hold() {
        let mut r = Registry::new();
        r.insert("a", "/w/one", 1, false);
        r.insert("b", "/w/one-two", 2, false);
        r.insert("c", "/w/one", 3, false);
        let (since, _) = r.hide("c").unwrap();
        r.park("c", since, parked("http://x.test/"));
        assert_eq!(r.within("/w/one"), [&1]);
        assert_eq!(r.within("/w/one/src/app"), [&1]);
        assert_eq!(r.within("/w/one-two"), [&2]);
        assert!(r.within("/w").is_empty());
    }

    /// A cheap generator, so the fuzz below is the same on every run.
    fn lcg(seed: &mut u64) -> usize {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 33) as usize
    }

    /// What a view is in the model the fuzz checks the registry against.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Is {
        Shown,
        Hidden(u64),
        Parked,
    }

    #[test]
    fn random_shows_hides_parks_and_closes_keep_to_the_cap_and_the_latest_hide() {
        let mut r = Registry::new();
        let mut model: Vec<(String, String, u32, Is)> = Vec::new();
        // Park timers set and not yet run: (id, the hide that set it).
        let mut timers: Vec<(String, u64)> = Vec::new();
        let mut cap = LIVE_HIDDEN;
        let mut seed = 11;
        let park_now = |r: &mut Registry<u32>,
                        model: &mut Vec<(String, String, u32, Is)>,
                        id: &str,
                        since: u64| {
            let expected = model
                .iter()
                .position(|m| m.0 == id && m.3 == Is::Hidden(since));
            let got = r.park(id, since, parked("http://localhost:5173/"));
            match expected {
                Some(at) => {
                    assert_eq!(got, Some(model[at].2), "{id} hidden since {since}");
                    model[at].3 = Is::Parked;
                }
                None => assert_eq!(got, None, "{id}: a stale hide ({since}) parks nothing"),
            }
        };
        for n in 0..20_000u32 {
            let id = format!("t{}", lcg(&mut seed) % 12);
            let root = format!("/w/{}", lcg(&mut seed) % 3);
            match lcg(&mut seed) % 9 {
                // A tab shows: its view made, or a parked one made again, or the live one shown.
                0 | 1 => match model.iter().position(|m| m.0 == id) {
                    Some(at) if model[at].3 == Is::Parked => {
                        assert!(r.unpark(&id).is_some());
                        model.remove(at);
                        r.insert(&id, &root, n, false);
                        model.push((id, root, n, Is::Shown));
                    }
                    Some(at) => {
                        assert_eq!(r.unpark(&id), None, "a live view isn't parked");
                        r.show(&id);
                        model[at].3 = Is::Shown;
                    }
                    None => {
                        r.insert(&id, &root, n, false);
                        model.push((id, root, n, Is::Shown));
                    }
                },
                // Out of sight: the cap parks the longest hidden at once.
                2 | 3 => {
                    let got = r.hide(&id);
                    let at = model.iter().position(|m| m.0 == id);
                    match at {
                        Some(at) if model[at].3 != Is::Parked => {
                            let (since, over) = got.expect("a live view hides");
                            model[at].3 = Is::Hidden(since);
                            timers.push((id.clone(), since));
                            for (gone, since) in over {
                                park_now(&mut r, &mut model, &gone, since);
                            }
                        }
                        _ => assert_eq!(got, None, "{id} isn't live"),
                    }
                }
                // A park timer runs, maybe long after a later show or hide.
                4 | 5 if !timers.is_empty() => {
                    let (id, since) = timers.swap_remove(lcg(&mut seed) % timers.len());
                    let stands = model.iter().any(|m| m.0 == id && m.3 == Is::Hidden(since));
                    assert_eq!(r.still_hidden(&id, since), stands);
                    park_now(&mut r, &mut model, &id, since);
                }
                6 => {
                    let at = model.iter().position(|m| m.0 == id);
                    let live = at.and_then(|at| (model[at].3 != Is::Parked).then(|| model[at].2));
                    if let Some(at) = at {
                        model.remove(at);
                    }
                    assert_eq!(r.remove(&id), live);
                }
                7 => {
                    cap = lcg(&mut seed) % 5;
                    for (gone, since) in r.set_live_hidden(cap) {
                        park_now(&mut r, &mut model, &gone, since);
                    }
                }
                _ => {
                    let live: Vec<u32> = model
                        .iter()
                        .filter(|m| m.1 == root && m.3 != Is::Parked)
                        .map(|m| m.2)
                        .collect();
                    model.retain(|m| m.1 != root);
                    assert_eq!(r.remove_root(&root), live);
                }
            }
            // Never more hidden and alive than the cap, and every live one where the model says.
            let hidden = model
                .iter()
                .filter(|m| matches!(m.3, Is::Hidden(_)))
                .count();
            assert!(hidden <= cap, "{hidden} hidden past a cap of {cap}");
            for m in &model {
                let live = (m.3 != Is::Parked).then_some(&m.2);
                assert_eq!(r.get(&m.0), live, "{m:?}");
            }
        }
        let live: Vec<u32> = model
            .iter()
            .filter(|m| m.3 != Is::Parked)
            .map(|m| m.2)
            .collect();
        assert_eq!(r.take_all(), live);
    }

    /// Agents' tabs among the user's, waking, sleeping and covered in any order: an awake view is
    /// never Hidden, parked, nor one the cap names, a new awake one starts off-screen, only an
    /// awake one is off-screen, and only a shown one is covered.
    #[test]
    fn random_runs_never_hide_park_or_cap_an_awake_view() {
        use std::collections::HashSet;
        let mut r = Registry::new();
        let mut hides: Vec<(String, u64)> = Vec::new();
        let mut awake: HashSet<String> = HashSet::new();
        let mut cap = LIVE_HIDDEN;
        let mut seed = 23;
        let not_awake = |over: &[(String, u64)], awake: &HashSet<String>| {
            for (id, _) in over {
                assert!(!awake.contains(id), "the cap named {id}, which is awake");
            }
        };
        for n in 0..20_000u32 {
            let id = format!("t{}", lcg(&mut seed) % 8);
            let park_all = |r: &mut Registry<u32>, over: Vec<(String, u64)>| {
                for (gone, since) in over {
                    assert!(r.park(&gone, since, parked("http://x.test/")).is_some());
                }
            };
            match lcg(&mut seed) % 10 {
                0 => {
                    if r.get(&id).is_none() {
                        r.unpark(&id);
                        let agents = lcg(&mut seed).is_multiple_of(2);
                        r.insert(&id, "/w", n, agents);
                        if agents {
                            assert!(r.offscreen(&id), "a new awake view starts off-screen");
                            awake.insert(id.clone());
                        } else {
                            awake.remove(&id);
                        }
                    }
                }
                1 => r.show(&id),
                2 => match r.hide(&id) {
                    Some((since, over)) => {
                        assert!(!awake.contains(&id), "{id} awake and Hidden");
                        not_awake(&over, &awake);
                        hides.push((id.clone(), since));
                        park_all(&mut r, over);
                    }
                    None if awake.contains(&id) => assert!(r.offscreen(&id)),
                    None => assert!(r.get(&id).is_none()),
                },
                3 => {
                    let woke = r.wake(&id);
                    assert_eq!(woke.is_some(), r.get(&id).is_some() && !awake.contains(&id));
                    if r.get(&id).is_some() {
                        awake.insert(id.clone());
                        assert!(r.shown(&id) || r.offscreen(&id));
                    }
                }
                4 => match r.sleep(&id) {
                    Some(Step::Hide(since, over)) => {
                        assert!(awake.remove(&id));
                        not_awake(&over, &awake);
                        hides.push((id.clone(), since));
                        park_all(&mut r, over);
                    }
                    Some(step) => {
                        assert!(awake.remove(&id));
                        assert!(r.shown(&id));
                        assert_eq!(step == Step::Cover, r.covered(&id));
                    }
                    None => assert!(!awake.contains(&id)),
                },
                9 => {
                    let shown = r.shown(&id);
                    let step = r.cover(&id);
                    assert_eq!(r.covered(&id), shown);
                    match step {
                        Step::Offscreen => assert!(shown && awake.contains(&id)),
                        Step::Cover => assert!(shown && !awake.contains(&id)),
                        _ => assert!(!shown),
                    }
                }
                5 if !hides.is_empty() => {
                    let (id, since) = hides.swap_remove(lcg(&mut seed) % hides.len());
                    let got = r.park(&id, since, parked("http://x.test/"));
                    if awake.contains(&id) {
                        assert_eq!(got, None, "{id} parked while awake");
                    }
                }
                6 => {
                    cap = lcg(&mut seed) % 4;
                    let over = r.set_live_hidden(cap);
                    not_awake(&over, &awake);
                    for (gone, since) in over {
                        r.park(&gone, since, parked("http://x.test/"));
                    }
                }
                _ => {
                    r.remove(&id);
                    awake.remove(&id);
                }
            }
            let hidden = r.hidden();
            assert!(hidden.len() <= cap, "{} hidden past {cap}", hidden.len());
            for id in (0..8).map(|i| format!("t{i}")) {
                assert_eq!(r.awake(&id), awake.contains(&id), "{id}");
                assert!(
                    !r.covered(&id) || r.shown(&id),
                    "{id} covered and not shown"
                );
                if awake.contains(&id) {
                    assert!(r.get(&id).is_some() && r.parked(&id).is_none(), "{id}");
                    assert!(!hidden.iter().any(|h| h.0 == id), "{id} awake and Hidden");
                } else {
                    assert!(!r.offscreen(&id), "{id} off-screen asleep");
                }
            }
        }
    }

    #[test]
    fn hiding_again_restarts_the_clock_and_keeps_the_most_recent() {
        let mut r = Registry::new();
        for id in ["a", "b", "c"] {
            r.insert(id, "/w", id, false);
        }
        let (a1, _) = r.hide("a").unwrap();
        r.hide("b").unwrap();
        // a hidden once more (a second hide while out of sight): now the most recent.
        let (a2, over) = r.hide("a").unwrap();
        assert!(a2 > a1 && over.is_empty());
        assert!(!r.still_hidden("a", a1));
        let (_, over) = r.hide("c").unwrap();
        assert_eq!(over.iter().map(|o| o.0.as_str()).collect::<Vec<_>>(), ["b"]);
        // A cap of none parks even the one just hidden.
        let mut r = Registry::new();
        r.insert("x", "/w", 1, false);
        assert!(r.set_live_hidden(0).is_empty(), "shown views never park");
        let (since, over) = r.hide("x").unwrap();
        assert_eq!(over, [("x".to_string(), since)]);
    }

    #[test]
    fn a_reopened_id_replaces_its_parked_entry_and_unknown_ids_do_nothing() {
        let mut r = Registry::new();
        r.insert("a", "/w", 1, false);
        let (since, _) = r.hide("a").unwrap();
        r.park("a", since, parked("http://localhost:1/"));
        r.insert("a", "/w", 2, false);
        assert_eq!(r.get("a"), Some(&2));
        assert_eq!(r.unpark("a"), None);
        assert_eq!(r.remove("a"), Some(2));
        assert_eq!(r.remove("a"), None);
        assert_eq!(r.hide("nope"), None);
        assert!(!r.still_hidden("nope", 1));
        assert_eq!(r.park("nope", 1, parked("http://x.test/")), None);
        r.show("nope");
        assert!(r.within("/w").is_empty());
    }

    #[test]
    fn a_worktree_holds_its_subfolders_but_not_its_namesakes() {
        let mut r = Registry::new();
        r.insert("a", "/w/app", 1, false);
        r.insert("b", "/w/app-2", 2, false);
        r.insert("c", "/w/app/nested", 3, false);
        assert_eq!(r.within("/w/app/"), [&1]);
        assert_eq!(r.within("/w/app/nested/src"), [&1, &3]);
        assert_eq!(r.within("/w/app-2/x"), [&2]);
        assert!(r.within("/w/ap").is_empty());
        assert!(r.within("").is_empty());
    }

    /// Agents' commands (agent.rs `live`: made awake or woken, touched, then the cap applied as
    /// `sleep_past_cap` does, skipping a tab a command runs on) among the user's shows, hides,
    /// covers, idle sleeps and closes: the cap names exactly the longest unused of the views
    /// drawn off-screen, never the one just used nor one on show, and after a command no more
    /// than CAP are drawn but for those a command still runs on.
    #[test]
    fn random_agent_commands_keep_the_awake_cap_and_sleep_the_longest_unused() {
        use std::collections::{HashMap, HashSet};
        const TABS: usize = 14;
        const CAP: usize = 6;
        let ids: Vec<String> = (0..TABS).map(|i| format!("t{i}")).collect();
        let mut r = Registry::new();
        let mut running: HashSet<String> = HashSet::new();
        // The model's own order of uses.
        let mut used: HashMap<String, u64> = HashMap::new();
        let mut clock = 0u64;
        let mut seed = 31;
        let drawn = |r: &Registry<u32>| -> Vec<String> {
            ids.iter().filter(|id| r.offscreen(id)).cloned().collect()
        };
        let park_all = |r: &mut Registry<u32>, over: Vec<(String, u64)>| {
            for (gone, since) in over {
                assert!(r.park(&gone, since, parked("http://x.test/")).is_some());
            }
        };
        let mut most = 0;
        for n in 0..30_000u32 {
            let id = ids[lcg(&mut seed) % TABS].clone();
            match lcg(&mut seed) % 12 {
                // A command on the tab.
                0..=4 => {
                    if r.get(&id).is_none() {
                        r.unpark(&id);
                        r.insert(&id, "/w", n, true);
                    } else {
                        r.wake(&id);
                    }
                    r.touch(&id);
                    clock += 1;
                    used.insert(id.clone(), clock);
                    let before = drawn(&r);
                    let mut by_use = before.clone();
                    by_use.sort_by_key(|d| used[d]);
                    let over = r.over_awake(CAP);
                    let expected = &by_use[..before.len().saturating_sub(CAP)];
                    assert_eq!(over, expected, "the longest unused, first");
                    assert!(!over.contains(&id), "{id}, just used, sleeps");
                    for gone in &over {
                        if running.contains(gone) {
                            continue;
                        }
                        // Out of sight it hides; covered on show, under its cover.
                        match r.sleep(gone) {
                            Some(Step::Hide(_, past)) => park_all(&mut r, past),
                            Some(Step::Cover) => assert!(r.shown(gone) && r.covered(gone)),
                            step => panic!("{gone} drawn off-screen slept as {step:?}"),
                        }
                        assert!(!r.offscreen(gone) && !r.awake(gone));
                    }
                    let after = drawn(&r);
                    let busy = after.iter().filter(|d| running.contains(*d)).count();
                    assert!(
                        after.len() <= CAP + busy,
                        "{} drawn, {busy} busy",
                        after.len()
                    );
                    if running.is_empty() {
                        assert!(after.len() <= CAP);
                    }
                    most = most.max(after.len());
                    assert!(r.awake(&id) && (r.shown(&id) || r.offscreen(&id)), "{id}");
                }
                // The user opens the tab, or shows it.
                5 => {
                    if r.get(&id).is_none() {
                        r.unpark(&id);
                        r.insert(&id, "/w", n, false);
                        clock += 1;
                        used.insert(id.clone(), clock);
                    } else {
                        r.show(&id);
                    }
                }
                6 => {
                    if let Some((_, over)) = r.hide(&id) {
                        park_all(&mut r, over);
                    }
                }
                7 => {
                    r.cover(&id);
                }
                // Its idle timer, unless a command runs on it.
                8 if !running.contains(&id) => {
                    if let Some(Step::Hide(_, over)) = r.sleep(&id) {
                        park_all(&mut r, over);
                    }
                }
                9 if r.get(&id).is_some() => {
                    if !running.remove(&id) {
                        running.insert(id.clone());
                    }
                }
                10 => {
                    let at = lcg(&mut seed) % 4;
                    for (gone, since) in r.set_live_hidden(at) {
                        r.park(&gone, since, parked("http://x.test/"));
                    }
                }
                _ => {
                    r.remove(&id);
                    running.remove(&id);
                }
            }
            running.retain(|id| r.get(id).is_some());
            // What's drawn off-screen is exactly what the cap counts, all of it awake; a view on
            // show and uncovered is never among it.
            let all = r.drawn();
            let mut drawn_now = drawn(&r);
            drawn_now.sort_by_key(|d| used.get(d).copied().unwrap_or_default());
            assert_eq!(all, drawn_now);
            for id in &ids {
                if r.offscreen(id) {
                    assert!(r.awake(id) && r.get(id).is_some(), "{id}");
                }
                if r.shown(id) && !r.covered(id) {
                    assert!(!all.contains(id), "{id} on show counted");
                }
            }
        }
        assert!(most >= CAP, "the fuzz never reached the cap ({most})");
    }
}
