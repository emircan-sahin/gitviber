//! The Changes lists as the panel shows them: conflicts, staged and unstaged files in display
//! order (J/K walk it), flat or as a folder tree, by name or newest first.

use super::tree::{TreeRow, path_tree, tree_order};
use gitviber_core::git::{FileChange, RepoStatus};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Which list a file is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Conflict,
    Staged,
    Unstaged,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Conflict => "conflict",
            Kind::Staged => "staged",
            Kind::Unstaged => "unstaged",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Kind::Conflict => "Conflicts",
            Kind::Staged => "Staged",
            Kind::Unstaged => "Changes",
        }
    }
}

/// A file of one of the lists.
#[derive(Clone)]
pub struct Change {
    pub kind: Kind,
    pub file: Arc<FileChange>,
}

impl Change {
    /// As selection_key names its tab: kind, no scope, path.
    pub fn key(&self) -> String {
        format!("{}::{}", self.kind.name(), self.file.path)
    }
}

/// How Changes lists files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChangesView {
    pub tree: bool,
    /// Newest first, by modified time.
    pub recent: bool,
}

/// Modified times (ms) of the listed files on disk, by path; a deleted file has none.
pub type Mtimes = HashMap<String, f64>;

/// One list's files in the order Changes shows them.
pub fn order_files(
    files: &[Arc<FileChange>],
    view: ChangesView,
    mtimes: Option<&Mtimes>,
) -> Vec<Arc<FileChange>> {
    let rank = |f: &Arc<FileChange>| {
        mtimes
            .and_then(|m| m.get(&f.path).copied())
            .unwrap_or(f64::NEG_INFINITY)
    };
    let ranked = view.recent && mtimes.is_some();
    if view.tree {
        return tree_order(
            files,
            |f| f.path.as_str(),
            ranked.then_some(&rank as &dyn Fn(&Arc<FileChange>) -> f64),
        );
    }
    if !ranked {
        return files.to_vec();
    }
    // Stable: ties keep git's order.
    let mut out = files.to_vec();
    out.sort_by(|a, b| {
        rank(b)
            .partial_cmp(&rank(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// A row of a list: a file, or in the tree view a folder.
#[derive(Clone)]
pub enum Item {
    File {
        change: Change,
        depth: usize,
        name: Option<String>,
    },
    Folder {
        kind: Kind,
        path: String,
        label: String,
        depth: usize,
        open: bool,
        files: Vec<Arc<FileChange>>,
    },
}

impl Item {
    pub fn key(&self) -> String {
        match self {
            Item::File { change, .. } => change.key(),
            Item::Folder { kind, path, .. } => format!("folder:{}", folder_key(*kind, path)),
        }
    }
}

pub fn folder_key(kind: Kind, path: &str) -> String {
    format!("{}:{path}", kind.name())
}

/// The rows one list shows: its files, or in the tree view its folders and the files of open ones.
pub fn items_of(
    kind: Kind,
    list: &[Arc<FileChange>],
    view: ChangesView,
    mtimes: Option<&Mtimes>,
    closed: &HashSet<String>,
) -> Vec<Item> {
    if !view.tree {
        return list
            .iter()
            .map(|f| Item::File {
                change: Change {
                    kind,
                    file: f.clone(),
                },
                depth: 0,
                name: None,
            })
            .collect();
    }
    let rank = |f: &Arc<FileChange>| {
        mtimes
            .and_then(|m| m.get(&f.path).copied())
            .unwrap_or(f64::NEG_INFINITY)
    };
    let ranked =
        (view.recent && mtimes.is_some()).then_some(&rank as &dyn Fn(&Arc<FileChange>) -> f64);
    path_tree(list, |f| f.path.as_str(), ranked, &|p| {
        closed.contains(&folder_key(kind, p))
    })
    .into_iter()
    .map(|r| match r {
        TreeRow::Leaf { item, name, depth } => Item::File {
            change: Change { kind, file: item },
            depth,
            name: Some(name),
        },
        TreeRow::Folder {
            path,
            label,
            depth,
            items,
            open,
        } => Item::Folder {
            kind,
            path,
            label,
            depth,
            open,
            files: items,
        },
    })
    .collect()
}

/// The three lists of a status, each as `Arc`s, in display order.
#[derive(Clone, Default)]
pub struct Lists {
    pub conflicted: Vec<Arc<FileChange>>,
    pub staged: Vec<Arc<FileChange>>,
    pub unstaged: Vec<Arc<FileChange>>,
}

impl Lists {
    pub fn of(status: &RepoStatus) -> Self {
        let arcs = |v: &[FileChange]| v.iter().cloned().map(Arc::new).collect();
        Lists {
            conflicted: arcs(&status.conflicted),
            staged: arcs(&status.staged),
            unstaged: arcs(&status.unstaged),
        }
    }

    pub fn list(&self, kind: Kind) -> &[Arc<FileChange>] {
        match kind {
            Kind::Conflict => &self.conflicted,
            Kind::Staged => &self.staged,
            Kind::Unstaged => &self.unstaged,
        }
    }

    pub fn ordered(&self, view: ChangesView, mtimes: Option<&Mtimes>) -> Self {
        Lists {
            conflicted: order_files(&self.conflicted, view, mtimes),
            staged: order_files(&self.staged, view, mtimes),
            unstaged: order_files(&self.unstaged, view, mtimes),
        }
    }

    /// Only the files `keep` accepts (the panel's filter).
    pub fn filtered(&self, keep: impl Fn(&FileChange) -> bool) -> Self {
        let f = |v: &[Arc<FileChange>]| v.iter().filter(|x| keep(x)).cloned().collect();
        Lists {
            conflicted: f(&self.conflicted),
            staged: f(&self.staged),
            unstaged: f(&self.unstaged),
        }
    }

    /// Every reviewable change in display order; J/K walk this. Nested repos have no diff.
    pub fn changes(&self) -> Vec<Change> {
        let of = |kind: Kind, v: &[Arc<FileChange>]| {
            v.iter()
                .filter(|f| f.nested.is_none())
                .map(move |f| Change {
                    kind,
                    file: f.clone(),
                })
                .collect::<Vec<_>>()
        };
        [
            of(Kind::Conflict, &self.conflicted),
            of(Kind::Staged, &self.staged),
            of(Kind::Unstaged, &self.unstaged),
        ]
        .concat()
    }

    /// Where a file is now: its list first, else wherever it moved (resolved → staged…).
    pub fn relocate(&self, kind: Kind, path: &str) -> Option<Change> {
        [kind, Kind::Conflict, Kind::Staged, Kind::Unstaged]
            .into_iter()
            .find_map(|k| {
                self.list(k)
                    .iter()
                    .find(|f| f.path == path)
                    .map(|f| Change {
                        kind: k,
                        file: f.clone(),
                    })
            })
    }

    pub fn total(&self) -> usize {
        self.conflicted.len() + self.staged.len() + self.unstaged.len()
    }
}

pub fn sum_lines<'a>(files: impl IntoIterator<Item = &'a FileChange>) -> (u32, u32) {
    files.into_iter().fold((0, 0), |(a, d), f| {
        (a + f.additions.unwrap_or(0), d + f.deletions.unwrap_or(0))
    })
}

/// The paths `git add` takes: nested repositories are left out (it would record a pointer).
pub fn stageable(files: &[Arc<FileChange>]) -> (Vec<String>, usize) {
    let paths: Vec<String> = files
        .iter()
        .filter(|f| f.nested.is_none())
        .map(|f| f.path.clone())
        .collect();
    let skipped = files.len() - paths.len();
    (paths, skipped)
}

pub const NESTED_EXPLAINED: &str = "It's a separate git repository. git add would record only a pointer to its current commit, not its files. Commit inside it instead.";

/// git restore leaves a submodule's checkout as it is; only a deleted one comes back.
pub fn kept_by_restore(f: &FileChange) -> bool {
    f.submodule.is_some() && f.status != "D"
}

pub fn files(n: usize) -> String {
    format!("{n} {}", if n == 1 { "file" } else { "files" })
}

pub fn left_out(n: usize) -> String {
    format!(
        "Left out {n} nested {}",
        if n == 1 { "repository" } else { "repositories" }
    )
}

/// What a viewed mark remembers of a file: a new edit changes it, and the mark lapses.
pub fn file_sig(f: &FileChange) -> String {
    match &f.oid {
        Some(oid) => format!("{}:{oid}", f.status),
        None => format!(
            "{}:{}:{}",
            f.status,
            f.additions.map_or("null".into(), |n| n.to_string()),
            f.deletions.map_or("null".into(), |n| n.to_string())
        ),
    }
}

/// The key a file's viewed mark is kept under.
pub fn viewed_key(kind: &str, path: &str) -> String {
    format!("{kind}:{path}")
}

/// The row now in the place of `from`, which left a list whose keys were `before`: the next
/// one still there, else the one before it. None when the list has nothing left.
pub fn row_in_place(before: &[String], after: impl Fn(&str) -> bool, from: &str) -> Option<String> {
    let at = before.iter().position(|k| k == from)?;
    before[at + 1..]
        .iter()
        .find(|k| after(k))
        .or_else(|| before[..at].iter().rev().find(|k| after(k)))
        .cloned()
}

/// `picked` with `item` added, or taken out when it's in.
pub fn toggled(picked: &[String], item: &str) -> Vec<String> {
    if picked.iter().any(|p| p == item) {
        picked.iter().filter(|p| *p != item).cloned().collect()
    } else {
        let mut out = picked.to_vec();
        out.push(item.to_string());
        out
    }
}

/// The keys of `list` from `from` to `to`, either way round; only `to` when either isn't listed.
pub fn range_of(list: &[String], from: &str, to: &str) -> Vec<String> {
    match (
        list.iter().position(|k| k == from),
        list.iter().position(|k| k == to),
    ) {
        (Some(i), Some(j)) => list[i.min(j)..=i.max(j)].to_vec(),
        _ => vec![to.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> Arc<FileChange> {
        Arc::new(FileChange {
            path: path.into(),
            old_path: None,
            status: "M".into(),
            additions: None,
            deletions: None,
            oid: None,
            index_oid: None,
            conflict: None,
            mode: None,
            submodule: None,
            nested: None,
        })
    }
    fn paths(v: &[Arc<FileChange>]) -> Vec<&str> {
        v.iter().map(|f| f.path.as_str()).collect()
    }

    // As changesView.test.ts.
    fn files4() -> Vec<Arc<FileChange>> {
        vec![
            file("b.ts"),
            file("src/a.ts"),
            file("c.ts"),
            file("gone.ts"),
        ]
    }
    fn times() -> Mtimes {
        [("b.ts", 10.), ("src/a.ts", 30.), ("c.ts", 20.)]
            .into_iter()
            .map(|(p, t)| (p.to_string(), t))
            .collect()
    }

    #[test]
    fn by_name_is_gits_order() {
        assert_eq!(
            paths(&order_files(&files4(), ChangesView::default(), None)),
            ["b.ts", "src/a.ts", "c.ts", "gone.ts"]
        );
    }

    #[test]
    fn recent_puts_the_newest_first_and_deleted_last() {
        let recent = ChangesView {
            tree: false,
            recent: true,
        };
        assert_eq!(
            paths(&order_files(&files4(), recent, Some(&times()))),
            ["src/a.ts", "c.ts", "b.ts", "gone.ts"]
        );
        assert_eq!(
            paths(&order_files(&files4(), recent, None)),
            ["b.ts", "src/a.ts", "c.ts", "gone.ts"]
        );
    }

    #[test]
    fn the_tree_walks_folders_first() {
        assert_eq!(
            paths(&order_files(
                &files4(),
                ChangesView {
                    tree: true,
                    recent: false
                },
                None
            )),
            ["src/a.ts", "b.ts", "c.ts", "gone.ts"]
        );
        assert_eq!(
            paths(&order_files(
                &files4(),
                ChangesView {
                    tree: true,
                    recent: true
                },
                Some(&times())
            )),
            ["src/a.ts", "c.ts", "b.ts", "gone.ts"]
        );
    }

    #[test]
    fn tree_rows_have_keys_per_list_and_closed_folders_hide_files() {
        let list = vec![file("src/lib/a.ts"), file("src/b.ts"), file("c.ts")];
        let tree = ChangesView {
            tree: true,
            recent: false,
        };
        let closed: HashSet<String> = ["unstaged:src/lib".to_string()].into();
        let rows = items_of(Kind::Unstaged, &list, tree, None, &closed);
        let keys: Vec<String> = rows
            .iter()
            .map(|r| match r {
                Item::Folder { .. } => r.key(),
                Item::File { change, .. } => change.file.path.clone(),
            })
            .collect();
        assert_eq!(
            keys,
            [
                "folder:unstaged:src",
                "folder:unstaged:src/lib",
                "src/b.ts",
                "c.ts"
            ]
        );
        let closed: HashSet<String> = ["unstaged:src".to_string()].into();
        assert_eq!(items_of(Kind::Staged, &list, tree, None, &closed).len(), 5);
    }

    // As leaving.test.ts.
    #[test]
    fn the_row_in_place_of_one_that_left() {
        let before: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let gone = |out: &'static [&'static str]| move |k: &str| !out.contains(&k);
        assert_eq!(
            row_in_place(&before, gone(&["b"]), "b").as_deref(),
            Some("c")
        );
        assert_eq!(
            row_in_place(&before, gone(&["b", "c", "d"]), "b").as_deref(),
            Some("a")
        );
        assert_eq!(
            row_in_place(&before, gone(&["a", "b", "c", "d"]), "b"),
            None
        );
        assert_eq!(row_in_place(&before, gone(&[]), "x"), None);
    }

    // As pick.test.ts.
    #[test]
    fn picking_rows() {
        let list: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        assert_eq!(toggled(&["a".into()], "b"), ["a", "b"]);
        assert_eq!(toggled(&["a".into(), "b".into()], "a"), ["b"]);
        assert_eq!(range_of(&list, "c", "a"), ["a", "b", "c"]);
        assert_eq!(range_of(&list, "x", "b"), ["b"]);
    }

    #[test]
    fn signatures_and_lists() {
        let mut f = (*file("a")).clone();
        assert_eq!(file_sig(&f), "M:null:null");
        f.oid = Some("123".into());
        assert_eq!(file_sig(&f), "M:123");
        let lists = Lists {
            conflicted: vec![],
            staged: vec![file("s")],
            unstaged: vec![file("u"), file("s")],
        };
        assert_eq!(
            lists.changes().iter().map(Change::key).collect::<Vec<_>>(),
            ["staged::s", "unstaged::u", "unstaged::s"]
        );
        assert_eq!(
            lists.relocate(Kind::Conflict, "u").map(|c| c.kind),
            Some(Kind::Unstaged)
        );
        assert_eq!(
            stageable(&lists.unstaged),
            (vec!["u".to_string(), "s".to_string()], 0)
        );
    }
}
