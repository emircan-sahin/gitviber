//! Where terminal links resolve: the repo's file list (read on the first ⌘-hover, kept for one
//! revision), the disk for what the list lacks (ignored files), the commits git knows, and files
//! elsewhere on disk. The calls block, so `resolve` runs off the UI thread; what each answered is
//! kept until the working tree changes, so a line hovered again asks nothing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gitviber_core::Core;
use gitviber_core::api::{files, history};
use gitviber_core::fs::Kind;

use crate::links::{
    FileIndex, Link, LinkKind, Target, disk_candidates, disk_target, github_item,
    outside_candidate, resolve_terminal_link, slashes, split_position,
};

/// What the panel tells its panes links resolve against.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinkSite {
    /// The open worktree's top folder.
    pub root: String,
    /// The repo's worktrees: a shell in one inside `root` isn't in `root`'s checkout.
    pub worktrees: Arc<Vec<String>>,
    /// The working tree's state: answers are kept for one.
    pub revision: u64,
    /// The GitHub repos whose #123 and pull request URLs open here: origin, then its parent.
    pub repos: Vec<String>,
}

/// Answers kept between hovers, for one root and revision; a few thousand at most.
#[derive(Default)]
pub struct LinkCache {
    stamp: String,
    index: Option<Arc<FileIndex>>,
    disk: HashMap<String, Option<(String, bool)>>,
    commits: HashMap<String, Option<String>>,
    /// Absolute paths elsewhere: only answers are kept, a file that isn't there yet is asked again.
    outside: HashMap<String, (String, bool)>,
}

const KEPT: usize = 2000;

pub type SharedCache = Arc<Mutex<LinkCache>>;

impl LinkCache {
    /// The file list, read once a revision: a terminal hovered while an agent writes files asks
    /// again only after it's changed.
    fn index(&mut self, core: &Core) -> Arc<FileIndex> {
        self.index
            .get_or_insert_with(|| {
                Arc::new(FileIndex::new(files::list_files(core).unwrap_or_default()))
            })
            .clone()
    }

    fn for_site(&mut self, site: &LinkSite) {
        let stamp = format!("{}@{}", site.root, site.revision);
        if self.stamp != stamp || self.disk.len() > KEPT || self.commits.len() > KEPT {
            *self = LinkCache {
                stamp,
                outside: std::mem::take(&mut self.outside),
                ..Default::default()
            };
        }
        if self.outside.len() > 500 {
            self.outside.clear();
        }
    }
}

/// Where a shell in `cwd` is in the repo: its folder ("" the root; None outside it), and whether
/// paths may fall back to the root (not from a worktree inside this one, another checkout).
fn place(site: &LinkSite, cwd: &str) -> (Option<String>, bool) {
    let (root, from) = (slashes(&site.root), slashes(cwd));
    let dir = if from == root {
        Some(String::new())
    } else {
        from.strip_prefix(&format!("{root}/")).map(str::to_string)
    };
    let nested = dir.is_some()
        && site.worktrees.iter().any(|w| {
            let at = slashes(w);
            at.starts_with(&format!("{root}/"))
                && (from == at || from.starts_with(&format!("{at}/")))
        });
    (dir.clone(), dir.is_some() && !nested)
}

/// What `found` go to (None: nowhere), each with its link: paths by the shell's folder `cwd`, from
/// the file list or else the disk; SHAs the repo knows; #123 when origin is on GitHub. A path with
/// spaces: the longest of its readings that's there wins, and the words it spans lose their own
/// links. A bare name under a line naming its folder (`dirs_above[i]`, `context_dirs`) is looked
/// for there too.
pub fn resolve(
    core: &Core,
    cache: &SharedCache,
    site: &LinkSite,
    cwd: &str,
    found: &[Link],
    dirs_above: &[Vec<String>],
) -> Vec<(Link, Option<Target>)> {
    let extra: Vec<(usize, Link)> = found
        .iter()
        .enumerate()
        .flat_map(|(i, l)| l.longer.iter().map(move |x| (i, x.clone())))
        .collect();
    let mut all: Vec<Link> = found.to_vec();
    all.extend(extra.iter().map(|(_, l)| l.clone()));
    let resolved = resolve_all(core, cache, site, cwd, &all, dirs_above, true);
    let mut out: Vec<(Link, Option<Target>)> = found
        .iter()
        .cloned()
        .zip(resolved.iter().cloned())
        .collect();
    let mut taken = vec![];
    for (k, (i, link)) in extra.iter().enumerate() {
        if let Some(target) = resolved[found.len() + k].clone()
            && !taken.contains(i)
        {
            taken.push(*i);
            out[*i] = (link.clone(), Some(target));
        }
    }
    let spans: Vec<(usize, usize, usize)> = taken
        .iter()
        .map(|&i| (i, out[i].0.start, out[i].0.end))
        .collect();
    for (j, (_, t)) in out.iter_mut().enumerate() {
        let start = found[j].start;
        if !taken.contains(&j)
            && spans
                .iter()
                .any(|&(i, s, e)| i != j && start >= s && start < e)
        {
            *t = None;
        }
    }
    out
}

/// The links that go somewhere: each one's index in `found`, its link as it won, and its target.
pub fn resolve_links(
    core: &Core,
    cache: &SharedCache,
    site: &LinkSite,
    cwd: &str,
    found: &[Link],
    dirs_above: &[Vec<String>],
) -> Vec<(usize, Link, Target)> {
    resolve(core, cache, site, cwd, found, dirs_above)
        .into_iter()
        .enumerate()
        .filter_map(|(i, (l, t))| Some((i, l, t?)))
        .collect()
}

fn resolve_all(
    core: &Core,
    cache: &SharedCache,
    site: &LinkSite,
    cwd: &str,
    found: &[Link],
    dirs_above: &[Vec<String>],
    retry: bool,
) -> Vec<Option<Target>> {
    let (dir, root_too) = place(site, cwd);
    let root = slashes(&site.root);
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    c.for_site(site);
    let index = if found.iter().any(|l| l.kind == LinkKind::File) && !root.is_empty() {
        c.index(core)
    } else {
        Arc::new(FileIndex::default())
    };
    // Commits and issues only from a shell in this checkout, whose repo they'd be.
    let shas: Vec<String> = if dir.is_some() {
        found
            .iter()
            .filter(|l| l.kind == LinkKind::Commit && !c.commits.contains_key(&l.spec))
            .map(|l| l.spec.clone())
            .collect()
    } else {
        vec![]
    };
    if !shas.is_empty() {
        let ids =
            history::known_commits(core, shas.clone()).unwrap_or_else(|_| vec![None; shas.len()]);
        for (s, id) in shas.into_iter().zip(ids) {
            c.commits.insert(s, id);
        }
    }
    let mut targets: Vec<Option<Target>> = found
        .iter()
        .map(|l| match l.kind {
            LinkKind::Commit => c
                .commits
                .get(&l.spec)
                .cloned()
                .flatten()
                .map(Target::Commit),
            LinkKind::Issue if dir.is_none() => None,
            _ => github_item(l, &site.repos)
                .or_else(|| resolve_terminal_link(l, dir.as_deref(), &index, &root, root_too)),
        })
        .collect();
    // What the list lacks may be on disk, ignored: one call for the line's paths.
    let asks: Vec<Vec<String>> = found
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if targets[i].is_some() {
                vec![]
            } else {
                disk_candidates(l, dir.as_deref(), &root, root_too)
            }
        })
        .collect();
    let new: Vec<String> = asks
        .iter()
        .flatten()
        .filter(|p| !c.disk.contains_key(*p))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if !new.is_empty() {
        let kinds = files::path_kinds(core, new.clone())
            .unwrap_or_else(|_| new.iter().map(|_| None).collect());
        let there = new.iter().zip(&kinds).filter(|(_, k)| k.is_some());
        let cased = index.spellings(there.map(|(p, _)| p.as_str()));
        for (p, k) in new.into_iter().zip(kinds) {
            let answer = k.map(|k| {
                let path = cased
                    .get(&p.to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| p.clone());
                (path, matches!(k, Kind::Dir))
            });
            c.disk.insert(p, answer);
        }
    }
    for (i, paths) in asks.iter().enumerate() {
        if let Some((path, is_dir)) = paths.iter().find_map(|p| c.disk.get(p).cloned().flatten()) {
            targets[i] = disk_target(&found[i].spec, &path, is_dir);
        }
    }
    // Absolute and ~ paths no repo file or folder answered to: a file or folder elsewhere.
    let outside: Vec<(usize, String)> = found
        .iter()
        .enumerate()
        .filter(|(i, _)| targets[*i].is_none())
        .filter_map(|(i, l)| outside_candidate(l).map(|p| (i, p)))
        .collect();
    let ask: Vec<String> = outside
        .iter()
        .map(|(_, p)| p.clone())
        .filter(|p| !c.outside.contains_key(p))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(50)
        .collect();
    if !ask.is_empty() {
        let hits = files::external_find(core, ask.clone()).unwrap_or_default();
        for (p, hit) in ask.into_iter().zip(hits) {
            if let Some(hit) = hit {
                c.outside
                    .insert(p, (hit.path, matches!(hit.kind, Kind::Dir)));
            }
        }
    }
    for (i, path) in outside {
        if let Some((real, is_dir)) = c.outside.get(&path).cloned() {
            let (_, line, column) = split_position(&found[i].spec);
            if !is_dir {
                targets[i] = Some(Target::External {
                    path: real,
                    line,
                    column,
                    dir: false,
                });
            } else if line.is_none() {
                targets[i] = Some(Target::External {
                    path: real,
                    line: None,
                    column: None,
                    dir: true,
                });
            }
        }
    }
    drop(c);
    // A bare name (`- p4.png`) under a line that names its folder is in that folder.
    if retry {
        let again: Vec<(usize, Link)> = found
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                targets[*i].is_none()
                    && l.kind == LinkKind::File
                    && !split_position(&l.spec).0.contains(['/', '\\'])
            })
            .flat_map(|(i, l)| {
                dirs_above.get(i).into_iter().flatten().map(move |d| {
                    (
                        i,
                        Link {
                            spec: format!("{d}/{}", l.spec),
                            longer: vec![],
                            ..l.clone()
                        },
                    )
                })
            })
            .collect();
        if !again.is_empty() {
            let links: Vec<Link> = again.iter().map(|(_, l)| l.clone()).collect();
            let hits = resolve_all(core, cache, site, cwd, &links, &[], false);
            for ((i, _), t) in again.into_iter().zip(hits) {
                if targets[i].is_none() {
                    targets[i] = t;
                }
            }
        }
    }
    targets
}

/// An OSC 8 file:// link's file or folder in the repo, from the list or else the disk; the repo's
/// own folder has no row in the explorer, so it shows in the file manager.
pub fn hyperlink_file(
    core: &Core,
    cache: &SharedCache,
    site: &LinkSite,
    target: Target,
) -> Option<Target> {
    let Target::Path {
        path, line, column, ..
    } = target
    else {
        return Some(target);
    };
    if path.is_empty() {
        return Some(Target::External {
            path: site.root.clone(),
            line: None,
            column: None,
            dir: true,
        });
    }
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    c.for_site(site);
    let index = c.index(core);
    if index.files.contains(&path) {
        return Some(Target::Path {
            path,
            line,
            column,
            dir: false,
        });
    }
    if index.folders().contains(&path) {
        return Some(Target::Path {
            path,
            line: None,
            column: None,
            dir: true,
        });
    }
    // As a terminal link's: asked once, spelled as the index has it.
    let known = match c.disk.get(&path) {
        Some(known) => known.clone(),
        None => {
            let kind = files::path_kinds(core, vec![path.clone()])
                .ok()?
                .pop()
                .flatten();
            let known = kind.map(|k| {
                let cased = index
                    .spellings([path.as_str()])
                    .remove(&path.to_lowercase());
                (
                    cased.unwrap_or_else(|| path.clone()),
                    matches!(k, Kind::Dir),
                )
            });
            c.disk.insert(path, known.clone());
            known
        }
    };
    let (path, dir) = known?;
    Some(Target::Path {
        path,
        line: if dir { None } else { line },
        column: if dir { None } else { column },
        dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_in_a_worktree_inside_the_repo_has_its_own_files() {
        let site = LinkSite {
            root: "/repo".into(),
            worktrees: Arc::new(vec!["/repo/.claude/worktrees/x".into()]),
            ..Default::default()
        };
        assert_eq!(place(&site, "/repo"), (Some(String::new()), true));
        assert_eq!(place(&site, "/repo/src"), (Some("src".into()), true));
        assert_eq!(
            place(&site, "/repo/.claude/worktrees/x/src"),
            (Some(".claude/worktrees/x/src".into()), false)
        );
        assert_eq!(place(&site, "/elsewhere"), (None, false));
    }

    /// The disk finds a path in any case (APFS): a folder, or a file not named in ASCII (a
    /// hyperlink's), that the output spells otherwise opens as the repo spells it, not as a second
    /// tab of it.
    #[test]
    fn a_path_spelled_in_another_case_opens_as_the_repo_spells_it() {
        let base = std::env::temp_dir().join(format!("gv-link-case-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src/Lib")).unwrap();
        std::fs::write(base.join("src/Lib/Özet.md"), "x").unwrap();
        // A case-sensitive disk finds neither: nothing to fix there.
        if base.join("SRC").exists() {
            let git = std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&base)
                .status()
                .unwrap();
            assert!(git.success());
            let core = Core::new(Arc::new(|_| {}));
            let root = gitviber_core::api::repo::open_repo(&core, base.to_string_lossy().into())
                .unwrap()
                .root;
            let site = LinkSite {
                root: root.clone(),
                ..Default::default()
            };
            let line: Vec<char> = "see src/LIB/ and SRC/lib/".chars().collect();
            let found = crate::links::find_terminal_links(&line, None);
            let cache = SharedCache::default();
            let paths: Vec<(String, bool)> =
                resolve_links(&core, &cache, &site, &root, &found, &[])
                    .into_iter()
                    .filter_map(|(_, _, t)| match t {
                        Target::Path { path, dir, .. } => Some((path, dir)),
                        _ => None,
                    })
                    .collect();
            assert_eq!(paths, [("src/Lib".into(), true), ("src/Lib".into(), true)]);
            let link = Target::Path {
                path: "src/lib/ÖZET.md".into(),
                line: Some(3),
                column: None,
                dir: false,
            };
            assert_eq!(
                hyperlink_file(&core, &cache, &site, link),
                Some(Target::Path {
                    path: "src/Lib/Özet.md".into(),
                    line: Some(3),
                    column: None,
                    dir: false,
                })
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
