//! A section's files in the guide's navigator, as a folder tree: folders first, then files, each
//! by name in natural order. A folder holding just one folder shares its row ("src/lib"); a
//! top-level folder always starts its own.

use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum NavRow {
    Folder {
        /// The deepest folder of a shared row.
        path: String,
        label: String,
        depth: usize,
    },
    File {
        path: String,
        name: String,
        depth: usize,
    },
}

#[derive(Default)]
struct Node {
    folders: BTreeMap<String, Node>,
    /// Name and path.
    files: Vec<(String, String)>,
}

/// "file2" before "file10", case aside.
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let (n, m) = (digits(&mut x), digits(&mut y));
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                match n.len().cmp(&m.len()).then_with(|| n.cmp(m)) {
                    Ordering::Equal => {}
                    o => return o,
                }
            }
            (Some(c), Some(d)) => match c.to_lowercase().cmp(d.to_lowercase()) {
                Ordering::Equal => {
                    x.next();
                    y.next();
                }
                o => return o,
            },
        }
    }
}

fn digits(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(c) = it.next_if(char::is_ascii_digit) {
        s.push(c);
    }
    s
}

fn join(at: &str, name: &str) -> String {
    if at.is_empty() {
        name.to_string()
    } else {
        format!("{at}/{name}")
    }
}

fn walk(node: &Node, at: &str, depth: usize, rows: &mut Vec<NavRow>) {
    let mut folders: Vec<(&String, &Node)> = node.folders.iter().collect();
    folders.sort_by(|a, b| natural(a.0, b.0));
    for (name, mut folder) in folders {
        let (mut label, mut path) = (name.clone(), join(at, name));
        while folder.files.is_empty() && folder.folders.len() == 1 {
            let Some((next, inner)) = folder.folders.iter().next() else {
                break;
            };
            label = format!("{label}/{next}");
            path = join(&path, next);
            folder = inner;
        }
        rows.push(NavRow::Folder {
            path: path.clone(),
            label,
            depth,
        });
        walk(folder, &path, depth + 1, rows);
    }
    let mut files: Vec<&(String, String)> = node.files.iter().collect();
    files.sort_by(|a, b| natural(&a.0, &b.0));
    rows.extend(files.into_iter().map(|(name, path)| NavRow::File {
        path: path.clone(),
        name: name.clone(),
        depth,
    }));
}

pub fn file_tree(paths: &[String]) -> Vec<NavRow> {
    let mut root = Node::default();
    for p in paths {
        let mut parts: Vec<&str> = p.trim_end_matches('/').split('/').collect();
        let name = parts.pop().unwrap_or_default();
        let mut node = &mut root;
        for d in parts {
            node = node.folders.entry(d.to_string()).or_default();
        }
        node.files.push((name.to_string(), p.clone()));
    }
    let mut rows = vec![];
    walk(&root, "", 0, &mut rows);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_first_in_natural_order_one_folder_chains_share_a_row() {
        let paths: Vec<String> = [
            "src/lib/b10.rs",
            "README.md",
            "src/lib/b2.rs",
            "docs/guide/intro.md",
            "src/main.rs",
            "Cargo.toml",
        ]
        .map(String::from)
        .to_vec();
        let rows: Vec<(String, usize)> = file_tree(&paths)
            .into_iter()
            .map(|r| match r {
                NavRow::Folder { label, depth, .. } => (format!("{label}/"), depth),
                NavRow::File { name, depth, .. } => (name, depth),
            })
            .collect();
        let want = [
            ("docs/guide/", 0),
            ("intro.md", 1),
            ("src/", 0),
            ("lib/", 1),
            ("b2.rs", 2),
            ("b10.rs", 2),
            ("main.rs", 1),
            ("Cargo.toml", 0),
            ("README.md", 0),
        ];
        assert_eq!(
            rows,
            want.map(|(s, d)| (s.to_string(), d)).to_vec(),
            "{rows:?}"
        );
        let NavRow::Folder { path, .. } = &file_tree(&paths)[0] else {
            unreachable!()
        };
        assert_eq!(path, "docs/guide");
    }

    #[test]
    fn natural_order_reads_numbers_as_numbers_and_ignores_case() {
        let mut v = ["b10", "B2", "a", "b02x", "b2"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["a", "B2", "b2", "b02x", "b10"]);
    }
}
