//! The open views and the memory policy over them, apart from AppKit so it can be tested. Each
//! live view is a WebContent process (about 20 MB for a plain page, 150 MB and up for a dev app):
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
    /// The page as it last showed, taken as it went out of sight: what it parks with.
    picture: Option<String>,
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
            picture: None,
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
    fn a_park_keeps_the_picture_taken_as_the_view_hid_even_one_that_comes_later() {
        let mut r = Registry::new();
        r.insert("a", "/w", 1);
        r.insert("b", "/w", 2);
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
                        r.insert(&id, &root, n);
                        model.push((id, root, n, Is::Shown));
                    }
                    Some(at) => {
                        assert_eq!(r.unpark(&id), None, "a live view isn't parked");
                        r.show(&id);
                        model[at].3 = Is::Shown;
                    }
                    None => {
                        r.insert(&id, &root, n);
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

    #[test]
    fn hiding_again_restarts_the_clock_and_keeps_the_most_recent() {
        let mut r = Registry::new();
        for id in ["a", "b", "c"] {
            r.insert(id, "/w", id);
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
        r.insert("x", "/w", 1);
        assert!(r.set_live_hidden(0).is_empty(), "shown views never park");
        let (since, over) = r.hide("x").unwrap();
        assert_eq!(over, [("x".to_string(), since)]);
    }

    #[test]
    fn a_reopened_id_replaces_its_parked_entry_and_unknown_ids_do_nothing() {
        let mut r = Registry::new();
        r.insert("a", "/w", 1);
        let (since, _) = r.hide("a").unwrap();
        r.park("a", since, parked("http://localhost:1/"));
        r.insert("a", "/w", 2);
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
        r.insert("a", "/w/app", 1);
        r.insert("b", "/w/app-2", 2);
        r.insert("c", "/w/app/nested", 3);
        assert_eq!(r.within("/w/app/"), [&1]);
        assert_eq!(r.within("/w/app/nested/src"), [&1, &3]);
        assert_eq!(r.within("/w/app-2/x"), [&2]);
        assert!(r.within("/w/ap").is_empty());
        assert!(r.within("").is_empty());
    }
}
