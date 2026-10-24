//! A tree listed folder by folder as they open: what the explorer shows. Every open folder is
//! listed again on a refresh; a folder gone since (agents delete them) closes quietly. Pure, so
//! the rules are tested without a window.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use gpui_kit::SharedString;

/// A file or folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub name: SharedString,
    /// Repo-relative.
    pub path: SharedString,
    pub is_dir: bool,
    pub ignored: bool,
}

impl From<gitviber_core::fs::Entry> for Node {
    fn from(e: gitviber_core::fs::Entry) -> Self {
        Node {
            name: e.name.into(),
            path: e.path.into(),
            is_dir: e.is_dir,
            ignored: e.ignored,
        }
    }
}

/// Names as a person sorts them: case aside, numbers by value ("file2" before "file10").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    a.next();
                }
                let mut nb = String::new();
                while let Some(c) = b.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    b.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Folders first, then by name; exact case breaks a tie so the order is stable.
pub fn compare_nodes(a: &Node, b: &Node) -> Ordering {
    b.is_dir
        .cmp(&a.is_dir)
        .then_with(|| natural_cmp(&a.name, &b.name))
        .then_with(|| a.name.cmp(&b.name))
}

/// One row on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub node: Node,
    pub depth: usize,
}

/// What a listing did to the tree.
#[derive(Debug, PartialEq, Eq)]
pub enum Listed {
    Changed,
    /// The same entries as shown: nothing to redraw.
    Same,
    /// A newer listing of the folder was asked for.
    Stale,
    /// The repo's root couldn't be listed; it stays open so the tree comes back.
    RootFailed(String),
    /// The folder is gone: closed.
    Closed,
}

#[derive(Default)]
pub struct LazyTree {
    children: HashMap<SharedString, Vec<Node>>,
    expanded: HashSet<SharedString>,
    /// Per folder: a slow, older listing must not overwrite a newer one.
    requests: HashMap<SharedString, u64>,
}

impl LazyTree {
    pub fn new() -> Self {
        let mut tree = LazyTree::default();
        tree.expanded.insert("".into());
        tree
    }

    pub fn is_open(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    pub fn expanded(&self) -> impl Iterator<Item = &SharedString> {
        self.expanded.iter()
    }

    pub fn children(&self, path: &str) -> Option<&[Node]> {
        self.children.get(path).map(Vec::as_slice)
    }

    /// A listing of `path` starts; its id goes back with the answer.
    pub fn begin(&mut self, path: &SharedString) -> u64 {
        let id = self.requests.entry(path.clone()).or_default();
        *id += 1;
        *id
    }

    pub fn listed(
        &mut self,
        path: &SharedString,
        id: u64,
        result: Result<Vec<Node>, String>,
    ) -> Listed {
        if self.requests.get(path) != Some(&id) {
            return Listed::Stale;
        }
        match result {
            Ok(mut nodes) => {
                if self
                    .children
                    .get(path)
                    .is_some_and(|shown| same(shown, &nodes))
                {
                    return Listed::Same;
                }
                nodes.sort_by(compare_nodes);
                self.children.insert(path.clone(), nodes);
                Listed::Changed
            }
            Err(e) if path.is_empty() => Listed::RootFailed(e),
            Err(_) => {
                self.expanded.remove(path);
                self.children.remove(path);
                Listed::Closed
            }
        }
    }

    /// Opens or closes a folder; true when it should be listed (always on open: the cached
    /// listing may be from before something changed it).
    pub fn set_open(&mut self, path: &str, open: bool) -> bool {
        if open {
            self.expanded.insert(path.to_string().into());
        } else {
            self.expanded.remove(path);
        }
        open
    }

    /// Opens the folders down to `path` (and `path` itself when `enter`); the ones to list.
    pub fn open_to(&mut self, path: &str, enter: bool) -> Vec<SharedString> {
        let parts: Vec<&str> = path.split('/').collect();
        let upto = if enter {
            parts.len()
        } else {
            parts.len().saturating_sub(1)
        };
        let dirs: Vec<SharedString> = (1..=upto)
            .map(|i| SharedString::from(parts[..i].join("/")))
            .collect();
        self.expanded.extend(dirs.iter().cloned());
        dirs
    }

    /// Closes everything but the root.
    pub fn collapse_all(&mut self) {
        self.expanded.retain(|p| p.is_empty());
    }

    /// A folder renamed: what was open inside it stays open under its new name; the ones to list.
    pub fn renamed(&mut self, from: &str, to: &str) -> Vec<SharedString> {
        let inside = |p: &str| p == from || p.starts_with(&format!("{from}/"));
        let moved: Vec<SharedString> = self
            .expanded
            .iter()
            .filter(|p| inside(p))
            .cloned()
            .collect();
        let mut out = Vec::new();
        for p in moved {
            self.expanded.remove(&p);
            let next: SharedString = format!("{to}{}", &p[from.len()..]).into();
            self.expanded.insert(next.clone());
            out.push(next);
        }
        out
    }

    /// What's on screen, top to bottom.
    pub fn rows(&self) -> Vec<Row> {
        let mut out = Vec::new();
        self.walk("", 0, &mut out);
        out
    }

    fn walk(&self, dir: &str, depth: usize, out: &mut Vec<Row>) {
        let Some(nodes) = self.children.get(dir) else {
            return;
        };
        for node in nodes {
            out.push(Row {
                node: node.clone(),
                depth,
            });
            if node.is_dir && self.expanded.contains(&node.path) {
                self.walk(&node.path, depth + 1, out);
            }
        }
    }
}

/// A fresh, unsorted listing that says what `shown` already does.
fn same(shown: &[Node], listed: &[Node]) -> bool {
    shown.len() == listed.len() && {
        let by_name: HashMap<&str, &Node> = shown.iter().map(|n| (n.name.as_ref(), n)).collect();
        listed.iter().all(|n| {
            by_name
                .get(n.name.as_ref())
                .is_some_and(|was| was.is_dir == n.is_dir && was.ignored == n.ignored)
        })
    }
}

/// The parent folder of a repo-relative path ("" at the root).
pub fn dirname(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// What a key does in the tree, for the tree to carry out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeKey {
    Move(isize),
    Open(SharedString),
    Close(SharedString),
    Select(SharedString),
    /// A file: open it; `pin` keeps the tab, `focus_code` moves to it.
    Activate {
        node: Node,
        pin: bool,
        focus_code: bool,
    },
}

/// The tree keys: ↓ ↑ move, → opens a folder (then steps into it) or a file, ← closes a folder
/// or goes to its parent, ↵ opens for good. `cur`: the keyboard's row, if any.
pub fn tree_key(
    key: &str,
    rows: &[Row],
    cur: Option<usize>,
    is_open: impl Fn(&str) -> bool,
) -> Option<TreeKey> {
    match key {
        "down" => return Some(TreeKey::Move(cur.map_or(0, |c| c as isize + 1))),
        "up" => {
            return Some(TreeKey::Move(
                cur.map_or(rows.len() as isize - 1, |c| c as isize - 1),
            ));
        }
        "home" => return Some(TreeKey::Move(0)),
        "end" => return Some(TreeKey::Move(rows.len() as isize - 1)),
        _ => {}
    }
    let at = cur?;
    let node = &rows.get(at)?.node;
    match key {
        "right" if !node.is_dir => Some(TreeKey::Activate {
            node: node.clone(),
            pin: false,
            focus_code: true,
        }),
        "right" if !is_open(&node.path) => Some(TreeKey::Open(node.path.clone())),
        "right" => Some(match rows.get(at + 1) {
            Some(next) if dirname(&next.node.path) == node.path.as_ref() => {
                TreeKey::Move(at as isize + 1)
            }
            _ => TreeKey::Select(node.path.clone()),
        }),
        "left" if node.is_dir && is_open(&node.path) => Some(TreeKey::Close(node.path.clone())),
        "left" => {
            let parent = dirname(&node.path);
            Some(TreeKey::Select(if parent.is_empty() {
                node.path.clone()
            } else {
                parent.to_string().into()
            }))
        }
        "enter" => Some(TreeKey::Activate {
            node: node.clone(),
            pin: true,
            focus_code: false,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(path: &str, is_dir: bool) -> Node {
        Node {
            name: path.rsplit('/').next().unwrap().to_string().into(),
            path: path.to_string().into(),
            is_dir,
            ignored: false,
        }
    }

    #[test]
    fn folders_first_then_names_as_people_sort_them() {
        let mut v = [
            node("b10", false),
            node("B2", false),
            node("src", true),
            node("a", false),
        ];
        v.sort_by(compare_nodes);
        let names: Vec<&str> = v.iter().map(|n| n.name.as_ref()).collect();
        assert_eq!(names, ["src", "a", "B2", "b10"]);
    }

    #[test]
    fn listings_open_close_and_ignore_stale_answers() {
        let mut t = LazyTree::new();
        let root: SharedString = "".into();
        let id = t.begin(&root);
        assert_eq!(
            t.listed(
                &root,
                id,
                Ok(vec![node("src", true), node("README.md", false)])
            ),
            Listed::Changed
        );
        assert_eq!(t.rows().len(), 2);
        let src: SharedString = "src".into();
        assert!(t.set_open("src", true));
        let old = t.begin(&src);
        let new = t.begin(&src);
        assert_eq!(
            t.listed(&src, new, Ok(vec![node("src/lib.rs", false)])),
            Listed::Changed
        );
        assert_eq!(t.listed(&src, old, Ok(vec![])), Listed::Stale);
        let rows = t.rows();
        assert_eq!(
            (rows[1].node.path.as_ref(), rows[1].depth),
            ("src/lib.rs", 1)
        );
        // The same entries again: nothing changes.
        let id = t.begin(&src);
        assert_eq!(
            t.listed(&src, id, Ok(vec![node("src/lib.rs", false)])),
            Listed::Same
        );
        // Gone: it closes; the root's failure keeps it open.
        let id = t.begin(&src);
        assert_eq!(t.listed(&src, id, Err("gone".into())), Listed::Closed);
        assert!(!t.is_open("src"));
        let id = t.begin(&root);
        assert!(matches!(
            t.listed(&root, id, Err("x".into())),
            Listed::RootFailed(_)
        ));
        assert!(t.is_open(""));
    }

    #[test]
    fn renames_keep_inner_folders_open() {
        let mut t = LazyTree::new();
        t.open_to("a/b/c.rs", false);
        assert!(t.is_open("a") && t.is_open("a/b"));
        let again = t.renamed("a", "z");
        assert!(t.is_open("z/b") && !t.is_open("a/b"));
        assert_eq!(again.len(), 2);
    }

    #[test]
    fn keys_walk_the_tree() {
        let rows = vec![
            Row {
                node: node("src", true),
                depth: 0,
            },
            Row {
                node: node("src/a.rs", false),
                depth: 1,
            },
            Row {
                node: node("b.md", false),
                depth: 0,
            },
        ];
        let open = |p: &str| p == "src";
        assert_eq!(tree_key("down", &rows, None, open), Some(TreeKey::Move(0)));
        assert_eq!(tree_key("up", &rows, None, open), Some(TreeKey::Move(2)));
        assert_eq!(
            tree_key("right", &rows, Some(0), open),
            Some(TreeKey::Move(1))
        );
        assert_eq!(
            tree_key("right", &rows, Some(0), |_| false),
            Some(TreeKey::Open("src".into()))
        );
        assert_eq!(
            tree_key("left", &rows, Some(1), open),
            Some(TreeKey::Select("src".into()))
        );
        assert_eq!(
            tree_key("left", &rows, Some(0), open),
            Some(TreeKey::Close("src".into()))
        );
        assert!(matches!(
            tree_key("enter", &rows, Some(2), open),
            Some(TreeKey::Activate { pin: true, .. })
        ));
    }
}
