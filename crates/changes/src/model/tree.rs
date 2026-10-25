//! Flat lists of paths shown as a folder tree: folders first, then files, each by name in
//! natural order, or newest first by a rank (a modified time). A folder holding just one folder
//! shares its row ("src/lib"); a top-level folder always starts its own.

use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum TreeRow<T> {
    Folder {
        /// The deepest folder of a compacted row ("src/lib" is src/lib's).
        path: String,
        /// "src/lib" for a folder whose only entry is another folder.
        label: String,
        depth: usize,
        /// Everything under it, in the tree's order.
        items: Vec<T>,
        open: bool,
    },
    Leaf {
        item: T,
        name: String,
        depth: usize,
    },
}

type FolderOrder<'a, T> = &'a dyn Fn(&&Node<T>, &&Node<T>) -> Ordering;
type LeafOrder<'a, T> = &'a dyn Fn(&&(String, T, f64), &&(String, T, f64)) -> Ordering;

struct Node<T> {
    name: String,
    path: String,
    folders: BTreeMap<String, Node<T>>,
    leaves: Vec<(String, T, f64)>,
    /// The highest rank under it.
    rank: f64,
}

impl<T> Node<T> {
    fn new(name: String, path: String) -> Self {
        Node {
            name,
            path,
            folders: BTreeMap::new(),
            leaves: vec![],
            rank: f64::NEG_INFINITY,
        }
    }
}

/// Natural order, case-insensitive: "v9" before "v10".
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                match m.len().cmp(&n.len()).then_with(|| m.cmp(n)) {
                    Ordering::Equal => {}
                    o => return o,
                }
            }
            (Some(c), Some(d)) => {
                let (c, d) = (
                    c.to_lowercase().next().unwrap_or(c),
                    d.to_lowercase().next().unwrap_or(d),
                );
                match c.cmp(&d) {
                    Ordering::Equal => {
                        x.next();
                        y.next();
                    }
                    o => return o,
                }
            }
        }
    }
}

/// The rows of `items` as a tree; `closed` folders' contents are left out.
pub fn path_tree<T: Clone>(
    items: &[T],
    path_of: impl Fn(&T) -> &str,
    rank: Option<&dyn Fn(&T) -> f64>,
    closed: &dyn Fn(&str) -> bool,
) -> Vec<TreeRow<T>> {
    let mut root: Node<T> = Node::new(String::new(), String::new());
    for item in items {
        let path = path_of(item).trim_end_matches('/');
        let parts: Vec<&str> = path.split('/').collect();
        let r = rank.map_or(0., |f| f(item));
        let mut node = &mut root;
        node.rank = node.rank.max(r);
        for part in &parts[..parts.len() - 1] {
            let child_path = if node.path.is_empty() {
                part.to_string()
            } else {
                format!("{}/{part}", node.path)
            };
            node = node
                .folders
                .entry(part.to_string())
                .or_insert_with(|| Node::new(part.to_string(), child_path));
            node.rank = node.rank.max(r);
        }
        node.leaves
            .push((parts[parts.len() - 1].to_string(), item.clone(), r));
    }
    let ranked = rank.is_some();
    let folder_order = |a: &&Node<T>, b: &&Node<T>| {
        (if ranked {
            b.rank.partial_cmp(&a.rank).unwrap_or(Ordering::Equal)
        } else {
            Ordering::Equal
        })
        .then_with(|| natural(&a.name, &b.name))
    };
    let leaf_order = |a: &&(String, T, f64), b: &&(String, T, f64)| {
        (if ranked {
            b.2.partial_cmp(&a.2).unwrap_or(Ordering::Equal)
        } else {
            Ordering::Equal
        })
        .then_with(|| natural(&a.0, &b.0))
    };
    fn all<T: Clone>(node: &Node<T>, fo: FolderOrder<T>, lo: LeafOrder<T>) -> Vec<T> {
        let mut folders: Vec<&Node<T>> = node.folders.values().collect();
        folders.sort_by(fo);
        let mut leaves: Vec<&(String, T, f64)> = node.leaves.iter().collect();
        leaves.sort_by(lo);
        folders
            .into_iter()
            .flat_map(|f| all(f, fo, lo))
            .chain(leaves.into_iter().map(|l| l.1.clone()))
            .collect()
    }
    fn walk<T: Clone>(
        node: &Node<T>,
        depth: usize,
        rows: &mut Vec<TreeRow<T>>,
        closed: &dyn Fn(&str) -> bool,
        fo: FolderOrder<T>,
        lo: LeafOrder<T>,
    ) {
        let mut folders: Vec<&Node<T>> = node.folders.values().collect();
        folders.sort_by(fo);
        for mut folder in folders {
            let mut label = folder.name.clone();
            while folder.folders.len() == 1 && folder.leaves.is_empty() {
                folder = folder.folders.values().next().expect("one folder");
                label.push('/');
                label.push_str(&folder.name);
            }
            let open = !closed(&folder.path);
            rows.push(TreeRow::Folder {
                path: folder.path.clone(),
                label,
                depth,
                items: all(folder, fo, lo),
                open,
            });
            if open {
                walk(folder, depth + 1, rows, closed, fo, lo);
            }
        }
        let mut leaves: Vec<&(String, T, f64)> = node.leaves.iter().collect();
        leaves.sort_by(lo);
        for (name, item, _) in leaves {
            rows.push(TreeRow::Leaf {
                item: item.clone(),
                name: name.clone(),
                depth,
            });
        }
    }
    let mut rows = vec![];
    walk(&root, 0, &mut rows, closed, &folder_order, &leaf_order);
    rows
}

/// `items` in the order the tree shows them, closed folders included.
pub fn tree_order<T: Clone>(
    items: &[T],
    path_of: impl Fn(&T) -> &str,
    rank: Option<&dyn Fn(&T) -> f64>,
) -> Vec<T> {
    path_tree(items, path_of, rank, &|_| false)
        .into_iter()
        .filter_map(|r| match r {
            TreeRow::Leaf { item, .. } => Some(item),
            TreeRow::Folder { .. } => None,
        })
        .collect()
}

/// The folders `path` sits in, outermost first: "a/b/c.ts" → "a", "a/b".
pub fn folders_of(path: &str) -> Vec<String> {
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    (1..parts.len()).map(|i| parts[..i].join("/")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &String) -> &str {
        s
    }
    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn shape(rows: &[TreeRow<String>]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                TreeRow::Folder {
                    label,
                    depth,
                    items,
                    ..
                } => format!("{}{label}/ ({})", "  ".repeat(*depth), items.len()),
                TreeRow::Leaf { name, depth, .. } => format!("{}{name}", "  ".repeat(*depth)),
            })
            .collect()
    }
    fn never(_: &str) -> bool {
        false
    }

    // As pathTree.test.ts.
    #[test]
    fn folders_first_natural_order_and_compacted_rows() {
        let items = strings(&[
            "src/lib/ui/a.ts",
            "src/lib/ui/b.ts",
            "README.md",
            "src/main.ts",
            "docs/v10.md",
            "docs/v9.md",
            "app/x/y/z.ts",
        ]);
        let rows = path_tree(&items, id, None, &never);
        assert_eq!(
            shape(&rows),
            [
                "app/x/y/ (1)",
                "  z.ts",
                "docs/ (2)",
                "  v9.md",
                "  v10.md",
                "src/ (3)",
                "  lib/ui/ (2)",
                "    a.ts",
                "    b.ts",
                "  main.ts",
                "README.md"
            ]
        );
        let lib = rows
            .iter()
            .find(|r| matches!(r, TreeRow::Folder { label, .. } if label == "lib/ui"));
        assert!(matches!(lib, Some(TreeRow::Folder { path, .. }) if path == "src/lib/ui"));
    }

    #[test]
    fn a_closed_folder_hides_but_covers() {
        let rows = path_tree(&strings(&["a/b/c.ts", "a/d.ts", "e.ts"]), id, None, &|p| {
            p == "a"
        });
        assert_eq!(shape(&rows), ["a/ (2)", "e.ts"]);
        assert!(
            matches!(&rows[0], TreeRow::Folder { items, open: false, .. } if *items == strings(&["a/b/c.ts", "a/d.ts"]))
        );
    }

    #[test]
    fn ranked_newest_first() {
        let time = |p: &String| match p.as_str() {
            "old/a.ts" => 1.,
            "new/b.ts" => 5.,
            "new/c.ts" => 3.,
            "top.ts" => 9.,
            "mid.ts" => 4.,
            _ => 0.,
        };
        let items = strings(&["old/a.ts", "new/b.ts", "new/c.ts", "top.ts", "mid.ts"]);
        assert_eq!(
            shape(&path_tree(&items, id, Some(&time), &never)),
            [
                "new/ (2)", "  b.ts", "  c.ts", "old/ (1)", "  a.ts", "top.ts", "mid.ts"
            ]
        );
        assert_eq!(
            tree_order(&items, id, Some(&time)),
            strings(&["new/b.ts", "new/c.ts", "old/a.ts", "top.ts", "mid.ts"])
        );
    }

    #[test]
    fn nested_repos_and_chains() {
        assert_eq!(
            shape(&path_tree(
                &strings(&["vendor/lib/", "vendor/x.ts"]),
                id,
                None,
                &never
            )),
            ["vendor/ (2)", "  lib", "  x.ts"]
        );
        assert_eq!(folders_of("a/b/c.ts"), strings(&["a", "a/b"]));
        assert!(folders_of("c.ts").is_empty());
        assert_eq!(folders_of("vendor/lib/"), strings(&["vendor"]));
        assert_eq!(
            shape(&path_tree(
                &strings(&["a/b/c/d.ts", "a/x.ts", "a/b/y.ts", "a/b/c/e.ts"]),
                id,
                None,
                &never
            )),
            [
                "a/ (4)",
                "  b/ (3)",
                "    c/ (2)",
                "      d.ts",
                "      e.ts",
                "    y.ts",
                "  x.ts"
            ]
        );
        assert_eq!(
            shape(&path_tree(&strings(&["b.ts", "a.ts"]), id, None, &|_| true)),
            ["a.ts", "b.ts"]
        );
        assert_eq!(
            shape(&path_tree(
                &strings(&["a/b/c/d.ts", "z.ts"]),
                id,
                None,
                &|p| p == "a/b/c"
            )),
            ["a/b/c/ (1)", "z.ts"]
        );
    }

    #[test]
    fn unranked_files_fall_back_to_names() {
        let rank = |p: &String| {
            if p == "new/name.ts" {
                7.
            } else {
                f64::NEG_INFINITY
            }
        };
        let items = strings(&["b/gone.ts", "a/gone.ts", "new/name.ts"]);
        assert_eq!(
            shape(&path_tree(&items, id, Some(&rank), &never)),
            [
                "new/ (1)",
                "  name.ts",
                "a/ (1)",
                "  gone.ts",
                "b/ (1)",
                "  gone.ts"
            ]
        );
    }

    #[test]
    fn hundreds_of_folders_keep_their_order() {
        let items: Vec<String> = (0..800).map(|i| format!("d{i}/sub/f{i}.ts")).collect();
        let rows = path_tree(&items, id, None, &never);
        assert_eq!(rows.len(), 1600);
        assert!(matches!(&rows[0], TreeRow::Folder { label, .. } if label == "d0/sub"));
        assert!(matches!(&rows[2], TreeRow::Folder { label, .. } if label == "d1/sub"));
    }
}
