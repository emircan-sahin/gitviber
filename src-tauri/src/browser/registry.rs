//! The open views and the memory policy over them, apart from AppKit so it can be tested. Each
//! live view is a WebContent process (17 MB for a plain page, 150 MB and up for a heavy dev app):
//! a few hidden ones stay alive to come back at once, and the rest park. A parked view is closed,
//! its address and a picture of it kept, and loads again when its tab shows.

/// How many hidden views stay alive until settings say (browserLiveHidden).
const LIVE_HIDDEN: usize = 2;

pub struct Registry<V> {
    views: Vec<Entry<V>>,
    live_hidden: usize,
    /// Every hide gets the next number: a park timer set by one tells a later one apart.
    hides: u64,
}

struct Entry<V> {
    id: String,
    root: String,
    state: State<V>,
}

enum State<V> {
    /// Placed in its tab, or about to be.
    Shown(V),
    /// Out of sight since the hide numbered so.
    Hidden(V, u64),
    Parked(Parked),
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
            State::Shown(v) | State::Hidden(v, _) => Some(v),
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
            State::Shown(v) | State::Hidden(v, _) => Some(v),
            State::Parked(_) => None,
        }
    }
}

impl<V> Registry<V> {
    pub const fn new() -> Self {
        Self {
            views: Vec::new(),
            live_hidden: LIVE_HIDDEN,
            hides: 0,
        }
    }

    fn entry(&mut self, id: &str) -> Option<&mut Entry<V>> {
        self.views.iter_mut().find(|e| e.id == id)
    }

    /// A live view; a parked one has none.
    pub fn get(&self, id: &str) -> Option<&V> {
        self.views.iter().find(|e| e.id == id)?.view()
    }

    /// A new view, shown; one parked under the same id makes way.
    pub fn insert(&mut self, id: &str, root: &str, view: V) {
        self.views.retain(|e| e.id != id);
        self.views.push(Entry {
            id: id.into(),
            root: root.into(),
            state: State::Shown(view),
        });
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
            e.restate(|s| match s {
                State::Hidden(v, _) => State::Shown(v),
                s => s,
            });
        }
    }

    /// Out of sight: the hide's number, for its park timer, and the hidden views past the cap
    /// (the longest hidden first) with theirs. None for a view that isn't live.
    pub fn hide(&mut self, id: &str) -> Option<(u64, Vec<(String, u64)>)> {
        self.hides += 1;
        let since = self.hides;
        let mut live = false;
        self.entry(id)?.restate(|s| match s {
            State::Shown(v) | State::Hidden(v, _) => {
                live = true;
                State::Hidden(v, since)
            }
            s => s,
        });
        live.then(|| (since, self.over_cap()))
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

    /// Whether the hide numbered `since` still stands: the view hasn't shown or closed since.
    pub fn still_hidden(&self, id: &str, since: u64) -> bool {
        self.views
            .iter()
            .any(|e| e.id == id && matches!(e.state, State::Hidden(_, s) if s == since))
    }

    /// Parks a view hidden since `since`, giving its view to close; None when it showed again
    /// (or closed) while its picture was taken.
    pub fn park(&mut self, id: &str, since: u64, parked: Parked) -> Option<V> {
        if !self.still_hidden(id, since) {
            return None;
        }
        let mut view = None;
        self.entry(id)?.restate(|s| match s {
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
            r.insert(id, "/w/one", id);
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
            r.insert(id, "/w", id);
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
    fn a_timer_from_before_the_view_showed_again_parks_nothing() {
        let mut r = Registry::new();
        r.insert("a", "/w", 1);
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
        r.insert("a", "/w", 1);
        let (since, _) = r.hide("a").unwrap();
        r.park("a", since, parked("http://localhost:3000/docs"));
        assert_eq!(r.unpark("a"), Some(parked("http://localhost:3000/docs")));
        assert_eq!(r.unpark("a"), None);
        r.insert("a", "/w", 2);
        assert_eq!(r.unpark("a"), None, "a live view isn't parked");
        assert_eq!(r.get("a"), Some(&2));
    }

    #[test]
    fn closing_takes_parked_views_too_and_gives_only_live_ones() {
        let mut r = Registry::new();
        r.insert("a", "/w/one", 1);
        r.insert("b", "/w/one", 2);
        r.insert("c", "/w/two", 3);
        let (since, _) = r.hide("a").unwrap();
        r.park("a", since, parked("http://x.test/"));
        assert_eq!(r.remove_root("/w/one"), [2]);
        assert_eq!(r.unpark("a"), None);
        assert_eq!(r.remove("c"), Some(3));
        r.insert("d", "/w/one", 4);
        let (since, _) = r.hide("d").unwrap();
        r.park("d", since, parked("http://x.test/"));
        assert_eq!(r.remove("d"), None);
        assert_eq!(r.unpark("d"), None);
        assert!(r.take_all().is_empty());
    }

    #[test]
    fn a_worktrees_live_views_are_those_its_folders_hold() {
        let mut r = Registry::new();
        r.insert("a", "/w/one", 1);
        r.insert("b", "/w/one-two", 2);
        r.insert("c", "/w/one", 3);
        let (since, _) = r.hide("c").unwrap();
        r.park("c", since, parked("http://x.test/"));
        assert_eq!(r.within("/w/one"), [&1]);
        assert_eq!(r.within("/w/one/src/app"), [&1]);
        assert_eq!(r.within("/w/one-two"), [&2]);
        assert!(r.within("/w").is_empty());
    }
}
