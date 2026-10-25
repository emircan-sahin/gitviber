//! Worktrees: what the picker and its dialogs show of each (its name, where it sits, its color,
//! whether Clean up may take it), and the events the terminal follows (a new worktree's command,
//! a terminal asked for in one). Pure but for the events and the colors' store.

mod dialogs;
mod picker;

pub use dialogs::{
    IssueAsk, PullAsk, Remember, open_clean_up, open_lock, open_new_worktree, open_rename,
    open_worktree_for_issue, open_worktree_for_pull, with_issue,
};
pub use picker::{WorktreePicker, pull_chip as picker_pull_chip, worktree_chip};

use std::collections::{BTreeMap, HashMap};

use gitviber_core::{api, git};
use gpui_kit::{App, AppContext as _, Entity, EventEmitter, Global, Hsla, Rgba, Window};

use crate::names::{Branch, folder_name};

/// What the terminal crate follows: subscribe to `WorktreeEvents::global(cx)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreeEvent {
    /// A worktree was made; `command` runs in a new terminal there (None: no terminal asked for).
    Created {
        path: String,
        command: Option<String>,
    },
    /// A terminal in this worktree (the picker's T, a branch's terminal button).
    OpenTerminal { path: String },
    /// A past agent conversation in it, to resume (the row menu's Resume).
    ResumeConversation { path: String },
}

pub struct WorktreeEvents;
impl EventEmitter<WorktreeEvent> for WorktreeEvents {}

struct Events(Entity<WorktreeEvents>);
impl Global for Events {}

impl WorktreeEvents {
    pub fn global(cx: &mut App) -> Entity<WorktreeEvents> {
        if let Some(e) = cx.try_global::<Events>() {
            return e.0.clone();
        }
        let e = cx.new(|_| WorktreeEvents);
        cx.set_global(Events(e.clone()));
        e
    }

    pub fn emit(event: WorktreeEvent, cx: &mut App) {
        WorktreeEvents::global(cx).update(cx, |_, cx| cx.emit(event));
    }
}

/// The New worktree dialog, starting from `base` (a commit id or a full ref; None: the branch
/// checked out, else the default one). History's "New worktree from here…".
pub fn open_new_worktree_dialog(base: Option<String>, window: &mut Window, cx: &mut App) {
    open_new_worktree(base, window, cx);
}

/// After a pull request merged in the app: the worktree that has its branch out at its head is
/// offered for removal (the head too: a fork's PR may share a branch name with another). Only
/// where Clean up would offer it: nothing uncommitted, no terminal, no worktree inside it.
pub fn offer_worktree_removal(
    branch: String,
    number: u64,
    head: String,
    window: &mut Window,
    cx: &mut App,
) {
    let core = gitviber_ui::core(cx);
    let b = branch.clone();
    let h = head.to_lowercase();
    let read = cx.background_spawn(async move {
        let list: Vec<Worktree> = api::worktrees::worktrees(&core)
            .ok()?
            .iter()
            .map(Worktree::from)
            .collect();
        let w = list
            .iter()
            .find(|w| {
                w.branch.as_deref() == Some(b.as_str())
                    && w.head
                        .as_ref()
                        .is_some_and(|x| h.starts_with(&x.to_lowercase()))
            })?
            .clone();
        let state = State::from(api::worktrees::worktree_state(&core, w.path.clone(), false).ok()?);
        Some((list, w, state))
    });
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let Some((list, w, state)) = read.await else {
            return;
        };
        let _ = handle.update(cx, |_, window, cx| {
            let states = HashMap::from([(w.path.clone(), state)]);
            let pull = |b: Option<&str>| {
                (b == Some(branch.as_str())).then(|| MergedPull {
                    number,
                    merged: true,
                    head_sha: head.clone(),
                })
            };
            let Some(mut offer) =
                cleanable(&list, &states, pull, |p| crate::hooks::terminals_in(p, cx))
                    .into_iter()
                    .find(|c| c.worktree.path == w.path)
            else {
                return;
            };
            offer.why = format!("#{number} merged");
            let remove = crate::toast::Action::new("Remove…", move |window, cx| {
                open_clean_up(vec![offer.clone()], window, cx)
            });
            crate::toast::Note::info(format!("Remove the worktree of #{number}?"))
                .detail(format!(
                    "{branch} is checked out in {}.",
                    folder_name(&w.path)
                ))
                .action(Some(remove))
                .show(window, cx);
        });
    })
    .detach();
}

/// A worktree as the UI keeps it: core's `git::Worktree`, cloneable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Worktree {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub lock_reason: Option<String>,
    pub in_use: bool,
    pub prunable: bool,
    pub current: bool,
    pub main: bool,
}

impl From<&git::Worktree> for Worktree {
    fn from(w: &git::Worktree) -> Self {
        Worktree {
            path: w.path.clone(),
            head: w.head.clone(),
            branch: w.branch.clone(),
            detached: w.detached,
            bare: w.bare,
            locked: w.locked,
            lock_reason: w.lock_reason.clone(),
            in_use: w.in_use,
            prunable: w.prunable,
            current: w.current,
            main: w.main,
        }
    }
}

pub fn list(worktrees: &[git::Worktree]) -> Vec<Worktree> {
    worktrees.iter().map(Worktree::from).collect()
}

/// Where a worktree stands, as the picker counts it: core's `git::WorktreeState`, copyable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub uncommitted: u32,
    pub commits: u32,
    pub merged: bool,
    pub updated: Option<u64>,
}

impl From<git::WorktreeState> for State {
    fn from(s: git::WorktreeState) -> Self {
        State {
            uncommitted: s.uncommitted,
            commits: s.commits,
            merged: s.merged,
            updated: s.updated,
        }
    }
}

/// `path` is somewhere inside the folder `dir`.
pub fn is_inside(path: &str, dir: &str) -> bool {
    path.len() > dir.len() && path.starts_with(dir) && path.as_bytes()[dir.len()] == b'/'
}

/// Where a worktree sits, as short as it can be said: inside the main one, or beside it.
pub fn short_path(path: &str, main: &str) -> String {
    if path == main {
        return ".".into();
    }
    if let Some(rest) = path.strip_prefix(main).and_then(|r| r.strip_prefix('/')) {
        return rest.into();
    }
    let parent = main.rfind('/').map_or("", |i| &main[..i]);
    if !parent.is_empty()
        && let Some(rest) = path.strip_prefix(parent).and_then(|r| r.strip_prefix('/'))
    {
        return format!("../{rest}");
    }
    path.into()
}

/// The folder `path` is in, with its trailing slash.
pub fn parent_folder(path: &str) -> &str {
    let name = folder_name(path);
    &path[..path.trim_end_matches('/').len() - name.len()]
}

/// Worktree folders are named after their branch, "/" being a folder separator.
pub fn folder_for_branch(branch: &str) -> String {
    branch.replace('/', "-")
}

/// A worktree as the top bar names it: its branch first, its folder second when that says
/// something else (a "vault-tiers" on main).
pub fn worktree_name(w: &Worktree) -> (String, Option<String>) {
    let branch = match &w.branch {
        Some(b) => b.clone(),
        None if w.bare => "bare".into(),
        None => format!("detached @ {}", w.head.as_deref().unwrap_or("?")),
    };
    let folder = folder_name(&w.path).to_string();
    let folder = (!folder.is_empty() && folder != folder_for_branch(&branch)).then_some(folder);
    (branch, folder)
}

/// The project's default branch: what origin/HEAD points at (origin's, when several remotes have
/// one), else a local main or master.
pub fn default_branch(branches: &[Branch]) -> Option<String> {
    let heads: Vec<&Branch> = branches
        .iter()
        .filter(|b| b.remote && b.remote_default)
        .collect();
    let head = heads
        .iter()
        .find(|b| b.name.starts_with("origin/"))
        .or(heads.first());
    if let Some(h) = head {
        return Some(h.short_name().to_string());
    }
    ["main", "master"]
        .into_iter()
        .find(|n| branches.iter().any(|b| !b.remote && b.name == *n))
        .map(str::to_string)
}

/// The default branch held by a linked worktree while the main folder is on another branch: git
/// won't switch the main folder to it, which the folder's own name hides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MainBackOffer {
    pub main: Worktree,
    pub holder: Worktree,
    pub branch: String,
}

pub fn main_back_offer(list: &[Worktree], branches: &[Branch]) -> Option<MainBackOffer> {
    let branch = default_branch(branches)?;
    let main = list.iter().find(|w| {
        w.main && !w.bare && !w.prunable && w.branch.as_ref().is_some_and(|b| *b != branch)
    })?;
    let holder = list
        .iter()
        .find(|w| !w.main && !w.prunable && w.branch.as_deref() == Some(branch.as_str()))?;
    Some(MainBackOffer {
        main: main.clone(),
        holder: holder.clone(),
        branch,
    })
}

/// The worktree `path` is in: the deepest, as agents' worktrees sit inside the main one.
pub fn worktree_of<'a>(path: &str, worktrees: &'a [Worktree]) -> Option<&'a Worktree> {
    worktrees
        .iter()
        .filter(|w| path == w.path || is_inside(path, &w.path))
        .max_by_key(|w| w.path.len())
}

/// A branch's pull request, as far as Clean up needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergedPull {
    pub number: u64,
    pub merged: bool,
    pub head_sha: String,
}

/// A worktree Clean up offers, and what says it's merged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cleanable {
    pub worktree: Worktree,
    /// "merged", or "#12 merged" when only its pull request says so (a squash git can't see yet).
    pub why: String,
    /// That pull request's head: its branch goes too while it's still there.
    pub merged_head: Option<String>,
}

/// The worktrees Clean up offers: merged (by git, or by its pull request), nothing uncommitted,
/// no terminal in it, not locked, not the main or the open one, and no other worktree inside it.
/// Those whose state isn't read yet aren't offered.
pub fn cleanable(
    list: &[Worktree],
    states: &HashMap<String, State>,
    pull_of: impl Fn(Option<&str>) -> Option<MergedPull>,
    terminals_in: impl Fn(&str) -> usize,
) -> Vec<Cleanable> {
    list.iter()
        .filter_map(|w| {
            let s = states.get(&w.path)?;
            let pull = pull_of(w.branch.as_deref()).filter(|p| p.merged);
            if w.main
                || w.current
                || w.locked
                || w.prunable
                || w.bare
                || s.uncommitted > 0
                || !(s.merged || pull.is_some())
            {
                return None;
            }
            if terminals_in(&w.path) > 0 || list.iter().any(|o| is_inside(&o.path, &w.path)) {
                return None;
            }
            Some(Cleanable {
                worktree: w.clone(),
                why: if s.merged {
                    "merged".into()
                } else {
                    format!("#{} merged", pull.as_ref()?.number)
                },
                merged_head: pull.map(|p| p.head_sha),
            })
        })
        .collect()
}

/// The worktree colors: OKLCH hues spread around the wheel, drawn at the theme's tint.
pub const HUES: [(&str, f32); 8] = [
    ("red", 25.),
    ("orange", 60.),
    ("yellow", 100.),
    ("green", 150.),
    ("teal", 190.),
    ("blue", 245.),
    ("violet", 295.),
    ("pink", 345.),
];

pub type Hue = usize;

pub fn hue_name(h: Hue) -> &'static str {
    HUES[h].0
}

/// A stored color: one of the hues, or "none" (Some(None)); anything else is dropped.
pub fn parse_choice(v: &str) -> Option<Option<Hue>> {
    if v == "none" {
        return Some(None);
    }
    HUES.iter().position(|(n, _)| *n == v).map(Some)
}

fn picked_of(path: &str, picked: &BTreeMap<String, String>) -> Option<Option<Hue>> {
    picked.get(path).and_then(|v| parse_choice(v))
}

/// A worktree's color: the one picked for it, else one its folder's name always gives. The main
/// worktree has none unless picked: it's the baseline the others stand out from.
pub fn worktree_hue(path: &str, main: bool, picked: &BTreeMap<String, String>) -> Option<Hue> {
    if let Some(p) = picked_of(path, picked) {
        return p;
    }
    if main {
        return None;
    }
    // FNV-1a over the name's UTF-16 units, as the old app hashed it: the same name, the same color.
    let mut h: u32 = 0x811c_9dc5;
    for ch in folder_name(path).chars() {
        let mut buf = [0u16; 2];
        let unit = ch.encode_utf16(&mut buf)[0];
        h = (h ^ unit as u32).wrapping_mul(0x0100_0193);
    }
    Some(h as usize % HUES.len())
}

/// The colors of one repo's worktrees, by path. A picked one stands; each other linked one takes
/// its name's hue unless one before it already has that, then the least used from there on round
/// the wheel: names that hash alike still get apart, until there are more than eight.
pub fn worktree_hues(
    list: &[(String, bool)],
    picked: &BTreeMap<String, String>,
) -> HashMap<String, Option<Hue>> {
    let mut used = [0usize; HUES.len()];
    for (path, _) in list {
        if let Some(Some(h)) = picked_of(path, picked) {
            used[h] += 1;
        }
    }
    let mut out = HashMap::new();
    for (path, main) in list {
        let named = worktree_hue(path, *main, picked);
        let Some(named) = named.filter(|_| picked_of(path, picked).is_none()) else {
            out.insert(path.clone(), named);
            continue;
        };
        let mut best = named;
        for k in 1..HUES.len() {
            let h = (named + k) % HUES.len();
            if used[h] < used[best] {
                best = h;
            }
        }
        used[best] += 1;
        out.insert(path.clone(), Some(best));
    }
    out
}

/// `hue` at the theme's tint lightness and chroma, `alpha` of it over what's behind.
pub fn hue_color(hue: Hue, l: f32, c: f32, alpha: f32) -> Hsla {
    let h = HUES[hue].1.to_radians();
    let (a, b) = (c * h.cos(), c * h.sin());
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let bl = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    let gamma = |x: f32| {
        let x = x.clamp(0., 1.);
        if x <= 0.003_130_8 {
            12.92 * x
        } else {
            1.055 * x.powf(1. / 2.4) - 0.055
        }
    };
    Rgba {
        r: gamma(r),
        g: gamma(g),
        b: gamma(bl),
        a: alpha,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issues_number_fills_in_its_command() {
        assert_eq!(
            with_issue("claude \"Fix #{issue}\"", 12),
            "claude \"Fix #12\""
        );
        assert_eq!(with_issue("claude", 12), "claude");
        assert_eq!(with_issue("{issue} {issue}", 7), "7 7");
    }

    fn wt(path: &str, branch: Option<&str>) -> Worktree {
        Worktree {
            path: path.into(),
            head: Some("abc1234".into()),
            branch: branch.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn short_worktree_paths() {
        let main = "/Users/me/code/app";
        assert_eq!(short_path(main, main), ".");
        assert_eq!(
            short_path(&format!("{main}/.claude/worktrees/agent-1"), main),
            ".claude/worktrees/agent-1"
        );
        assert_eq!(
            short_path("/Users/me/code/app-hotfix", main),
            "../app-hotfix"
        );
        assert_eq!(short_path("/Volumes/x/app", main), "/Volumes/x/app");
        assert_eq!(short_path("/Users/me/code/app2", main), "../app2");
    }

    #[test]
    fn clean_up_offers_merged_clean_idle_worktrees_only() {
        let mut list = vec![
            Worktree {
                main: true,
                current: true,
                ..wt("/p/app", Some("main"))
            },
            wt("/p/app.worktrees/done", Some("done")),
            wt("/p/app.worktrees/squashed", Some("squashed")),
            wt("/p/app.worktrees/dirty", Some("dirty")),
            wt("/p/app.worktrees/busy", Some("busy")),
            Worktree {
                locked: true,
                ..wt("/p/app.worktrees/locked", Some("locked"))
            },
            wt("/p/app.worktrees/open", Some("open")),
            wt("/p/app.worktrees/holder", Some("holder")),
            Worktree {
                locked: true,
                ..wt(
                    "/p/app.worktrees/holder/.claude/worktrees/inner",
                    Some("inner"),
                )
            },
        ];
        list.push(wt("/p/app.worktrees/unread", Some("unread")));
        let st = |merged: bool, uncommitted: u32, commits: u32| State {
            uncommitted,
            commits,
            merged,
            updated: None,
        };
        let states: HashMap<String, State> = [
            ("/p/app", st(false, 0, 0)),
            ("/p/app.worktrees/done", st(true, 0, 0)),
            ("/p/app.worktrees/squashed", st(false, 0, 2)),
            ("/p/app.worktrees/dirty", st(true, 3, 0)),
            ("/p/app.worktrees/busy", st(true, 0, 0)),
            ("/p/app.worktrees/locked", st(true, 0, 0)),
            ("/p/app.worktrees/open", st(false, 0, 1)),
            ("/p/app.worktrees/holder", st(true, 0, 0)),
            (
                "/p/app.worktrees/holder/.claude/worktrees/inner",
                st(true, 0, 0),
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let head = "f".repeat(40);
        let found = cleanable(
            &list,
            &states,
            |b| {
                (b == Some("squashed")).then(|| MergedPull {
                    number: 12,
                    merged: true,
                    head_sha: head.clone(),
                })
            },
            |p| usize::from(p.ends_with("/busy")),
        );
        let got: Vec<(Option<String>, String, Option<String>)> = found
            .into_iter()
            .map(|c| (c.worktree.branch, c.why, c.merged_head))
            .collect();
        assert_eq!(
            got,
            vec![
                (Some("done".into()), "merged".into(), None),
                (Some("squashed".into()), "#12 merged".into(), Some(head))
            ]
        );
    }

    #[test]
    fn clean_up_tells_a_worktree_inside_another_from_one_whose_name_only_starts_the_same() {
        let list = vec![
            Worktree {
                main: true,
                current: true,
                ..wt("/p/app", Some("app"))
            },
            wt("/p/app.worktrees/fix", Some("fix")),
            wt("/p/app.worktrees/fix-2", Some("fix-2")),
            wt("/p/app.worktrees/fix 3", Some("fix 3")),
            wt("/p/app.worktrees/fix.old", Some("fix.old")),
        ];
        let states: HashMap<String, State> = list
            .iter()
            .map(|w| {
                (
                    w.path.clone(),
                    State {
                        merged: true,
                        ..Default::default()
                    },
                )
            })
            .collect();
        let found: Vec<String> = cleanable(&list, &states, |_| None, |_| 0)
            .into_iter()
            .filter_map(|c| c.worktree.branch)
            .collect();
        assert_eq!(found, vec!["fix", "fix-2", "fix 3", "fix.old"]);
    }

    #[test]
    fn a_worktrees_color_comes_from_its_folders_name_until_one_is_picked() {
        let none = BTreeMap::new();
        let a = worktree_hue("/p/app.worktrees/fix-login", false, &none).unwrap();
        assert_eq!(
            worktree_hue("/q/other.worktrees/fix-login", false, &none),
            Some(a)
        );
        let names: std::collections::HashSet<_> = "abcdefghijkl"
            .chars()
            .map(|n| worktree_hue(&format!("/p/x/{n}"), false, &none))
            .collect();
        assert!(names.len() >= 4);
        assert_eq!(worktree_hue("/p/app", true, &none), None);
        let teal = BTreeMap::from([("/p/app".to_string(), "teal".to_string())]);
        assert_eq!(
            worktree_hue("/p/app", true, &teal).map(hue_name),
            Some("teal")
        );
        let off = BTreeMap::from([("/p/app.worktrees/fix-login".to_string(), "none".to_string())]);
        assert_eq!(
            worktree_hue("/p/app.worktrees/fix-login", false, &off),
            None
        );
    }

    #[test]
    fn same_named_worktrees_in_two_repos_share_a_hue_until_one_is_picked_for_either() {
        let (a, b) = (
            "/one/app.worktrees/fix-login",
            "/two/web.worktrees/fix-login",
        );
        let none = BTreeMap::new();
        assert_eq!(worktree_hue(a, false, &none), worktree_hue(b, false, &none));
        assert_ne!(worktree_hue(a, false, &none).map(hue_name), Some("violet"));
        let picked = BTreeMap::from([(a.to_string(), "violet".to_string())]);
        assert_eq!(
            worktree_hue(a, false, &picked).map(hue_name),
            Some("violet")
        );
        assert_eq!(
            worktree_hue(b, false, &picked),
            worktree_hue(b, false, &none)
        );
    }

    #[test]
    fn a_stored_color_that_isnt_one_of_the_hues_is_dropped() {
        for v in ["toString", "constructor", "__proto__", "Red", ""] {
            assert_eq!(parse_choice(v), None, "{v}");
        }
        for (n, _) in HUES {
            assert!(parse_choice(n).is_some());
        }
        assert_eq!(parse_choice("none"), Some(None));
    }

    #[test]
    fn one_repos_worktrees_get_apart_colors_a_pick_still_standing() {
        let mut list = vec![("/p/app".to_string(), true)];
        list.extend(
            "abcdefghij"
                .chars()
                .map(|n| (format!("/p/app.worktrees/{n}"), false)),
        );
        let none = BTreeMap::new();
        let hues = worktree_hues(&list, &none);
        assert_eq!(hues["/p/app"], None);
        let linked: Vec<Hue> = list[1..].iter().map(|(p, _)| hues[p].unwrap()).collect();
        assert_eq!(
            linked[..8]
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            8
        );
        assert_eq!(linked[0], worktree_hue(&list[1].0, false, &none).unwrap());
        assert_eq!(worktree_hues(&list, &none), hues);
        let picked = BTreeMap::from([
            (list[2].0.clone(), hue_name(linked[0]).to_string()),
            (list[3].0.clone(), "none".to_string()),
        ]);
        let again = worktree_hues(&list, &picked);
        assert_eq!(again[&list[2].0], Some(linked[0]));
        assert_eq!(again[&list[3].0], None);
        assert_ne!(again[&list[1].0], Some(linked[0]));
    }

    #[test]
    fn a_worktree_is_named_by_its_branch_then_its_folder() {
        assert_eq!(
            worktree_name(&wt("/p/app.worktrees/vault-tiers", Some("main"))),
            ("main".into(), Some("vault-tiers".into()))
        );
        assert_eq!(
            worktree_name(&wt("/p/app.worktrees/fix-login", Some("fix-login"))),
            ("fix-login".into(), None)
        );
        assert_eq!(
            worktree_name(&wt("/p/app.worktrees/feat-x", Some("feat/x"))),
            ("feat/x".into(), None)
        );
        assert_eq!(folder_for_branch("feat/x/y"), "feat-x-y");
        assert_eq!(
            worktree_name(&wt("/p/det", None)),
            ("detached @ abc1234".into(), Some("det".into()))
        );
        assert_eq!(
            worktree_name(&Worktree {
                head: None,
                ..wt("/p/det", None)
            })
            .0,
            "detached @ ?"
        );
        assert_eq!(
            worktree_name(&Worktree {
                bare: true,
                ..wt("/p/x.git", None)
            })
            .0,
            "bare"
        );
        assert_eq!(
            worktree_name(&wt("/p/fix-login", Some("Fix-Login"))),
            ("Fix-Login".into(), Some("fix-login".into()))
        );
        assert_eq!(worktree_name(&wt("/p/app.worktrees/x/", Some("x"))).1, None);
    }

    fn br(name: &str, remote_default: bool) -> Branch {
        Branch {
            name: name.into(),
            remote: name.contains('/'),
            remote_default,
            ..Default::default()
        }
    }

    #[test]
    fn the_default_branch_is_origin_heads_else_a_local_main_or_master() {
        assert_eq!(
            default_branch(&[
                br("trunk", false),
                br("origin/trunk", true),
                br("main", false)
            ])
            .as_deref(),
            Some("trunk")
        );
        assert_eq!(
            default_branch(&[br("up/dev", true), br("origin/rel/1", true)]).as_deref(),
            Some("rel/1")
        );
        assert_eq!(
            default_branch(&[br("feat", false), br("master", false)]).as_deref(),
            Some("master")
        );
        assert_eq!(
            default_branch(&[br("main", false), br("master", false)]).as_deref(),
            Some("main")
        );
        assert_eq!(
            default_branch(&[br("feat", false), br("origin/main", false)]),
            None
        );
    }

    #[test]
    fn move_main_back_is_offered_only_when_a_linked_worktree_holds_the_default_branch() {
        let branches = vec![
            br("main", false),
            br("feat/x", false),
            br("origin/main", true),
        ];
        let main = Worktree {
            main: true,
            ..wt("/p/app", Some("feat/x"))
        };
        let holder = wt("/p/app-vault-tiers", Some("main"));
        assert_eq!(
            main_back_offer(&[main.clone(), holder.clone()], &branches),
            Some(MainBackOffer {
                main: main.clone(),
                holder: holder.clone(),
                branch: "main".into()
            })
        );
        assert_eq!(
            main_back_offer(
                &[
                    Worktree {
                        branch: Some("main".into()),
                        ..main.clone()
                    },
                    wt("/p/other", Some("x"))
                ],
                &branches
            ),
            None
        );
        assert_eq!(
            main_back_offer(
                &[
                    Worktree {
                        branch: None,
                        ..main.clone()
                    },
                    holder.clone()
                ],
                &branches
            ),
            None
        );
        assert_eq!(
            main_back_offer(
                &[
                    Worktree {
                        bare: true,
                        ..main.clone()
                    },
                    holder.clone()
                ],
                &branches
            ),
            None
        );
        assert_eq!(
            main_back_offer(
                &[
                    main.clone(),
                    Worktree {
                        prunable: true,
                        ..holder.clone()
                    }
                ],
                &branches
            ),
            None
        );
        assert_eq!(
            main_back_offer(&[main, holder], &[br("feat/x", false)]),
            None
        );
    }

    #[test]
    fn the_deepest_worktree_holds_a_path() {
        let list = vec![wt("/p/app", None), wt("/p/app/.claude/worktrees/a", None)];
        assert_eq!(
            worktree_of("/p/app/.claude/worktrees/a/src", &list).map(|w| w.path.as_str()),
            Some("/p/app/.claude/worktrees/a")
        );
        assert_eq!(worktree_of("/p/app2", &list), None);
        assert_eq!(parent_folder("/p/app.worktrees/x"), "/p/app.worktrees/");
    }

    #[test]
    fn hue_colors_are_in_range() {
        for h in 0..HUES.len() {
            let c = hue_color(h, 0.74, 0.13, 1.);
            assert!((0. ..=1.).contains(&c.l) && c.a == 1.);
        }
    }
}
