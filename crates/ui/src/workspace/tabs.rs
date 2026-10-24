//! The center's tabs as a plain list: dedupe by key, one preview tab that a single click reuses
//! and a double click (or an edit) keeps, closed tabs to reopen where they were, and browser-like
//! stepping. Pure, so it's tested without a window.

use gpui_kit::SharedString;

/// Which tabs Close Others / to the Left / to the Right / All close, around one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabGroup {
    Others,
    Left,
    Right,
    All,
}

#[derive(Clone, Debug)]
pub struct TabEntry<T> {
    pub key: SharedString,
    /// Shown in italics; the next preview open replaces it.
    pub preview: bool,
    pub item: T,
}

/// A closed tab: its key and where it was. Nothing of its view stays: reopening builds it
/// again from the key.
#[derive(Clone, Debug, PartialEq)]
pub struct ClosedTab {
    pub key: SharedString,
    pub index: usize,
}

/// Closed tabs kept for reopening, as browsers keep them.
const CLOSED_CAP: usize = 20;

#[derive(Clone, Debug)]
pub struct Tabs<T> {
    tabs: Vec<TabEntry<T>>,
    active: Option<SharedString>,
    closed: Vec<ClosedTab>,
}

impl<T> Default for Tabs<T> {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            closed: Vec::new(),
        }
    }
}

/// What `open` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    /// The tab was open already and is active now.
    Existing,
    /// It took the preview tab's place, which went away.
    ReplacedPreview,
    Added,
}

impl<T> Tabs<T> {
    pub fn tabs(&self) -> &[TabEntry<T>] {
        &self.tabs
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_key(&self) -> Option<&SharedString> {
        self.active.as_ref()
    }

    pub fn active(&self) -> Option<&TabEntry<T>> {
        let key = self.active.as_ref()?;
        self.tabs.iter().find(|t| &t.key == key)
    }

    pub fn active_index(&self) -> Option<usize> {
        let key = self.active.as_ref()?;
        self.position(key)
    }

    pub fn position(&self, key: &str) -> Option<usize> {
        self.tabs.iter().position(|t| t.key.as_ref() == key)
    }

    pub fn get(&self, key: &str) -> Option<&TabEntry<T>> {
        self.tabs.iter().find(|t| t.key.as_ref() == key)
    }

    pub fn can_reopen(&self) -> bool {
        !self.closed.is_empty()
    }

    /// Opens `key` and makes it active. Open already: it stays where it is, kept when `preview` is
    /// false. Else a preview open takes the preview tab's place; `make` runs only for a new tab.
    pub fn open(&mut self, key: SharedString, preview: bool, make: impl FnOnce() -> T) -> Opened {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.key == key) {
            tab.preview = tab.preview && preview;
            self.active = Some(key);
            return Opened::Existing;
        }
        let entry = TabEntry {
            key: key.clone(),
            preview,
            item: make(),
        };
        self.active = Some(key);
        match self.tabs.iter().position(|t| t.preview).filter(|_| preview) {
            Some(at) => {
                self.tabs[at] = entry;
                Opened::ReplacedPreview
            }
            None => {
                self.tabs.push(entry);
                Opened::Added
            }
        }
    }

    /// A tab at the end, the active one left as it is: one opened in the background.
    pub fn add(&mut self, key: SharedString, make: impl FnOnce() -> T) {
        if self.position(&key).is_none() {
            self.tabs.push(TabEntry {
                key,
                preview: false,
                item: make(),
            });
        }
    }

    pub fn activate(&mut self, key: &str) -> bool {
        let found = self.position(key).is_some();
        if found {
            self.active = Some(SharedString::from(key.to_string()));
        }
        found
    }

    /// Changes the item of the tab under `key`, when there is one.
    pub fn set(&mut self, key: &str, f: impl FnOnce(&mut T)) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.key.as_ref() == key) {
            f(&mut tab.item);
        }
    }

    /// Keeps a preview tab: a double click on it, or an edit in it.
    pub fn pin(&mut self, key: &str) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.key.as_ref() == key) {
            tab.preview = false;
        }
    }

    /// Closes `keys`, remembering where each was for Reopen Closed Tab (unless `remember` is
    /// false: a tab that can't come back as it was). The active one hands over to its right
    /// neighbour, else its left. Returns the closed tabs' items, for the caller to drop.
    pub fn close(&mut self, keys: &[SharedString], remember: bool) -> Vec<T> {
        let was_active = self.active_index();
        let mut closed_now = Vec::new();
        let mut removed = Vec::new();
        let mut slots: Vec<Option<TabEntry<T>>> = Vec::with_capacity(self.tabs.len());
        for (index, tab) in std::mem::take(&mut self.tabs).into_iter().enumerate() {
            if keys.contains(&tab.key) {
                if remember {
                    closed_now.push(ClosedTab {
                        key: tab.key.clone(),
                        index,
                    });
                }
                removed.push(tab.item);
                slots.push(None);
            } else {
                slots.push(Some(tab));
            }
        }
        // The leftmost comes back first, so each returns to its own place.
        closed_now.reverse();
        self.closed.retain(|c| !keys.contains(&c.key));
        self.closed.extend(closed_now);
        let overflow = self.closed.len().saturating_sub(CLOSED_CAP);
        self.closed.drain(..overflow);
        if let Some(at) = was_active.filter(|i| slots[*i].is_none()) {
            let right = slots[at..].iter().flatten().next();
            let left = slots[..at].iter().rev().flatten().next();
            self.active = right.or(left).map(|t| t.key.clone());
        }
        self.tabs = slots.into_iter().flatten().collect();
        removed
    }

    /// The tab under `key` goes under `new_key` in its place, its item from `make`, active if it
    /// was and as kept as it was; nothing is remembered for reopening. A tab open under `new_key`
    /// already takes it in instead (a preview only if both were). Returns the old item.
    pub fn replace(
        &mut self,
        key: &str,
        new_key: SharedString,
        make: impl FnOnce() -> T,
    ) -> Option<T> {
        let at = self.position(key)?;
        let was_active = self.active_index() == Some(at);
        let old = match self.position(&new_key) {
            Some(twin) => {
                let preview = self.tabs[at].preview;
                self.tabs[twin].preview &= preview;
                self.tabs.remove(at).item
            }
            None => {
                let preview = self.tabs[at].preview;
                let entry = TabEntry {
                    key: new_key.clone(),
                    preview,
                    item: make(),
                };
                std::mem::replace(&mut self.tabs[at], entry).item
            }
        };
        if was_active {
            self.active = Some(new_key);
        }
        Some(old)
    }

    /// The last closed tab, taken off the list; None when there's none.
    pub fn take_reopen(&mut self) -> Option<ClosedTab> {
        self.closed.pop()
    }

    /// Puts a reopened tab back at `index` (as far as the list reaches), kept and active.
    pub fn insert(&mut self, index: usize, key: SharedString, item: T) {
        if self.position(&key).is_none() {
            let at = index.min(self.tabs.len());
            self.tabs.insert(
                at,
                TabEntry {
                    key: key.clone(),
                    preview: false,
                    item,
                },
            );
        }
        self.active = Some(key);
    }

    /// The keys `which` names around the tab at `index`.
    pub fn group(&self, index: usize, which: TabGroup) -> Vec<SharedString> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(j, _)| match which {
                TabGroup::Others => *j != index,
                TabGroup::Left => *j < index,
                TabGroup::Right => *j > index,
                TabGroup::All => true,
            })
            .map(|(_, t)| t.key.clone())
            .collect()
    }

    /// Next or previous tab, wrapping, as in browsers.
    pub fn step(&mut self, dir: isize) {
        let n = self.tabs.len() as isize;
        if n < 2 {
            return;
        }
        let at = self.active_index().unwrap_or(0) as isize;
        let next = (at + dir).rem_euclid(n) as usize;
        self.active = Some(self.tabs[next].key.clone());
    }

    /// ⌘1–⌘8 go to that tab; out of range does nothing.
    pub fn go_to(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get(index) {
            self.active = Some(tab.key.clone());
        }
    }

    pub fn go_to_last(&mut self) {
        if let Some(tab) = self.tabs.last() {
            self.active = Some(tab.key.clone());
        }
    }

    /// Moves the tab at `from` to `to`, the others shifting.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from < self.tabs.len() && to < self.tabs.len() && from != to {
            let tab = self.tabs.remove(from);
            self.tabs.insert(to, tab);
        }
    }

    /// The active tab one place left (-1) or right (1).
    pub fn move_active(&mut self, dir: isize) {
        if let Some(at) = self.active_index() {
            let to = at as isize + dir;
            if to >= 0 && (to as usize) < self.tabs.len() {
                self.move_tab(at, to as usize);
            }
        }
    }

    pub fn items(&self) -> impl Iterator<Item = &T> {
        self.tabs.iter().map(|t| &t.item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys<T>(tabs: &Tabs<T>) -> Vec<String> {
        tabs.tabs()
            .iter()
            .map(|t| format!("{}{}", t.key, if t.preview { "*" } else { "" }))
            .collect()
    }

    fn k(s: &str) -> SharedString {
        SharedString::from(s.to_string())
    }

    #[test]
    fn open_dedupes_and_reuses_the_preview() {
        let mut tabs: Tabs<u32> = Tabs::default();
        assert_eq!(tabs.open(k("a"), true, || 1), Opened::Added);
        assert_eq!(tabs.open(k("b"), true, || 2), Opened::ReplacedPreview);
        assert_eq!(keys(&tabs), ["b*"]);
        assert_eq!(
            tabs.open(k("b"), false, || unreachable!()),
            Opened::Existing
        );
        assert_eq!(keys(&tabs), ["b"]);
        tabs.open(k("c"), true, || 3);
        tabs.open(k("d"), false, || 4);
        assert_eq!(keys(&tabs), ["b", "c*", "d"]);
        assert_eq!(tabs.active_key().unwrap(), "d");
        // A preview open of a kept tab doesn't make it a preview again.
        tabs.open(k("b"), true, || unreachable!());
        assert_eq!(keys(&tabs), ["b", "c*", "d"]);
        tabs.pin("c");
        tabs.open(k("e"), true, || 5);
        assert_eq!(keys(&tabs), ["b", "c", "d", "e*"]);
    }

    #[test]
    fn closing_hands_over_right_then_left() {
        let mut tabs: Tabs<u32> = Tabs::default();
        for (i, key) in ["a", "b", "c"].iter().enumerate() {
            tabs.open(k(key), false, || i as u32);
        }
        tabs.activate("b");
        let gone = tabs.close(&[k("b")], true);
        assert_eq!(gone, [1]);
        assert_eq!(tabs.active_key().unwrap(), "c");
        tabs.close(&[k("c")], true);
        assert_eq!(tabs.active_key().unwrap(), "a");
        tabs.close(&[k("a")], false);
        assert!(tabs.is_empty());
        assert!(tabs.active_key().is_none());
        assert_eq!(tabs.take_reopen().unwrap().key, "c");
    }

    #[test]
    fn reopen_puts_tabs_back_in_place_leftmost_first() {
        let mut tabs: Tabs<u32> = Tabs::default();
        for (i, key) in ["a", "b", "c", "d"].iter().enumerate() {
            tabs.open(k(key), false, || i as u32);
        }
        tabs.close(&[k("b"), k("d")], true);
        assert_eq!(keys(&tabs), ["a", "c"]);
        let first = tabs.take_reopen().unwrap();
        assert_eq!((first.key.as_ref(), first.index), ("b", 1));
        tabs.insert(first.index, first.key, 1);
        let second = tabs.take_reopen().unwrap();
        tabs.insert(second.index, second.key, 3);
        assert_eq!(keys(&tabs), ["a", "b", "c", "d"]);
        assert_eq!(tabs.active_key().unwrap(), "d");
        assert!(!tabs.can_reopen());
    }

    #[test]
    fn replace_keeps_the_place_and_a_twin_takes_it_in() {
        let mut tabs: Tabs<u32> = Tabs::default();
        tabs.open(k("a"), false, || 1);
        tabs.open(k("b"), true, || 2);
        tabs.open(k("c"), false, || 3);
        tabs.activate("b");
        assert_eq!(tabs.replace("b", k("b2"), || 20), Some(2));
        assert_eq!(keys(&tabs), ["a", "b2*", "c"]);
        assert_eq!(tabs.active_key().unwrap(), "b2");
        // Onto an open tab: one tab, kept, active.
        assert_eq!(tabs.replace("b2", k("c"), || unreachable!()), Some(20));
        assert_eq!(keys(&tabs), ["a", "c"]);
        assert_eq!(tabs.active_key().unwrap(), "c");
        assert_eq!(tabs.replace("zz", k("y"), || 0), None);
        assert!(!tabs.can_reopen());
    }

    #[test]
    fn groups_steps_and_moves() {
        let mut tabs: Tabs<()> = Tabs::default();
        for key in ["a", "b", "c", "d"] {
            tabs.open(k(key), false, || ());
        }
        assert_eq!(tabs.group(1, TabGroup::Others), [k("a"), k("c"), k("d")]);
        assert_eq!(tabs.group(1, TabGroup::Left), [k("a")]);
        assert_eq!(tabs.group(1, TabGroup::Right), [k("c"), k("d")]);
        tabs.step(1);
        assert_eq!(tabs.active_key().unwrap(), "a");
        tabs.step(-1);
        assert_eq!(tabs.active_key().unwrap(), "d");
        tabs.go_to(1);
        tabs.move_active(1);
        assert_eq!(keys(&tabs), ["a", "c", "b", "d"]);
        tabs.go_to(9);
        assert_eq!(tabs.active_key().unwrap(), "b");
        tabs.go_to_last();
        assert_eq!(tabs.active_key().unwrap(), "d");
    }
}
