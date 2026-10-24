//! Where a diff's code goes: Copy GitHub Link / Open on GitHub for the picked lines (else the
//! file), Go to Definition (⌘-click, F12), Peek (⌥F12) and References (⇧F12) on either side,
//! Copy path in the header.

use super::*;
use crate::model::indent::Shown;
use gitviber_core::definitions::{self, Location};
use gitviber_core::{api, git, github};
use gitviber_ui::workspace::{Selection, Toast, Workspace};
use std::path::Path;

type OpenFile = Rc<dyn Fn(&str, Option<(u32, u32)>, &mut Window, &mut App)>;

struct FileOpener(OpenFile);

impl gpui_kit::Global for FileOpener {}

/// How a diff opens a repo file at a line and 0-based column (Go to Definition into another
/// file, a preview's link): the files crate sets it. Without it the file opens at its top.
pub fn set_file_opener(
    cx: &mut App,
    open: impl Fn(&str, Option<(u32, u32)>, &mut Window, &mut App) + 'static,
) {
    cx.set_global(FileOpener(Rc::new(open)));
}

pub(super) fn open_file(path: &str, at: Option<(u32, u32)>, window: &mut Window, cx: &mut App) {
    if let Some(open) = cx.try_global::<FileOpener>().map(|o| o.0.clone()) {
        return open(path, at, window, cx);
    }
    if let Some(ws) = Workspace::for_window(window, cx) {
        let sel = Selection::File {
            path: path.to_string(),
        };
        ws.update(cx, |ws, cx| ws.open(sel, false, window, cx));
    }
}

/// Where one side of a diff is on GitHub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GitHubSide {
    /// The file on disk: linked to the newest pushed commit whose lines match, found on use.
    OnDisk { path: String },
    /// `path` in commit `rev`, once GitHub has that commit.
    Commit { path: String, rev: String },
}

/// The sides GitHub can show: a commit's (its parent's for the old side), and the file on disk
/// for diffs against it. The index and a range's head aren't linked.
pub(crate) fn github_side(source: &DiffSource, side: Side) -> Option<GitHubSide> {
    let status = source.file.status.as_str();
    match (&source.compare, side) {
        (Compare::Commit { sha }, Side::New) if status != "D" => Some(GitHubSide::Commit {
            path: source.path.clone(),
            rev: sha.clone(),
        }),
        (Compare::Commit { sha }, Side::Old) if status != "A" => Some(GitHubSide::Commit {
            path: source.original_path().to_string(),
            rev: format!("{sha}^"),
        }),
        (Compare::Unstaged | Compare::Base { .. } | Compare::Worktree, Side::New)
            if status != "D" =>
        {
            Some(GitHubSide::OnDisk {
                path: source.path.clone(),
            })
        }
        _ => None,
    }
}

/// GitHub's page of `path` at `sha`, to `lines` (`#L10-L24`) if any.
pub(crate) fn permalink_url(
    web: &str,
    sha: &str,
    path: &str,
    tree: bool,
    lines: Option<(u32, u32)>,
) -> String {
    let at: String = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|seg| {
            let mut out = String::from("/");
            for b in seg.bytes() {
                if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
                    out.push(b as char);
                } else {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
            out
        })
        .collect();
    let anchor = match lines {
        Some((a, b)) if a == b => format!("#L{a}"),
        Some((a, b)) => format!("#L{a}-L{b}"),
        None => String::new(),
    };
    let kind = if tree { "tree" } else { "blob" };
    format!("{web}/{kind}/{sha}{at}{anchor}")
}

/// origin's page on GitHub; None off github.com.
fn web_url(repo: &Path) -> Option<String> {
    let url = git::remote_url(repo, "origin")?;
    let g = github::parse_remote(&url)?;
    Some(format!("https://github.com/{}/{}", g.owner, g.name))
}

fn text_of(out: Result<Vec<u8>, String>) -> String {
    out.map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .unwrap_or_default()
}

/// The link to `side` (its `lines`), or why GitHub can't show it yet.
fn resolve(
    repo: &Path,
    web: &str,
    side: &GitHubSide,
    lines: Option<(u32, u32)>,
) -> Result<String, String> {
    match side {
        GitHubSide::OnDisk { path } => {
            let p = git::permalink(repo, path, lines)?;
            Ok(permalink_url(web, &p.sha, path, p.tree, p.lines))
        }
        GitHubSide::Commit { path, rev } => {
            let spec = format!("{rev}^{{commit}}");
            let sha = text_of(git::run(repo, &["rev-parse", "--verify", "--quiet", &spec]));
            if sha.is_empty() {
                return Err(format!("{path} has no earlier version to link to."));
            }
            // GitHub only has commits that reached one of origin's branches.
            let on = text_of(git::run(
                repo,
                &[
                    "for-each-ref",
                    "--count=1",
                    "--format=%(refname)",
                    "--contains",
                    &sha,
                    "refs/remotes/origin",
                ],
            ));
            if on.is_empty() {
                return Err("This commit isn't on GitHub yet. Push it first.".into());
            }
            Ok(permalink_url(web, &sha, path, false, lines))
        }
    }
}

/// Where a side's names are looked up: its path, and the tree it's in (None: the worktree).
fn definition_side(source: &DiffSource, side: Side) -> (String, Option<String>) {
    let path = match side {
        Side::Old => source.original_path().to_string(),
        Side::New => source.path.clone(),
    };
    let rev = match (&source.compare, side) {
        (Compare::Commit { sha }, Side::Old) => Some(format!("{sha}^")),
        (Compare::Commit { sha }, Side::New) => Some(sha.clone()),
        (Compare::Range { base, .. } | Compare::Base { base }, Side::Old) => Some(base.clone()),
        (Compare::Range { head, .. }, Side::New) => Some(head.clone()),
        _ => None,
    };
    (path, rev)
}

/// The byte of `line` under shown column `col` (its tabs and indentation widened as drawn).
pub(crate) fn byte_at_column(line: &str, unit: usize, col: usize) -> usize {
    let shown = Shown::new(line, unit);
    let target = shown
        .text
        .char_indices()
        .nth(col)
        .map_or(shown.text.len(), |(i, _)| i);
    line.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(line.len()))
        .take_while(|&i| shown.offset(i) <= target)
        .last()
        .unwrap_or(0)
}

/// What a lookup of the name at the caret is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LookUp {
    Definition,
    /// The definition shown over the lines, without leaving them.
    Peek,
    References,
}

/// Lines a peek shows on each side of the place.
const PEEK_AROUND: u32 = 6;

/// Places a name is defined or used: a list over the lines to pick from. A peek shows the lines
/// around the one picked.
pub(super) struct Places {
    title: SharedString,
    side: Side,
    places: Vec<Location>,
    peek: Option<Peek>,
}

/// The lines around a peeked place (`at` of the list), the first one's number.
struct Peek {
    at: usize,
    first: u32,
    lines: Vec<SharedString>,
}

impl DiffView {
    /// Fetches origin's page once, for the menu's GitHub items.
    pub(super) fn fetch_web(&mut self, cx: &mut Context<Self>) {
        if self.web.is_some()
            || (github_side(&self.source, Side::New).is_none()
                && github_side(&self.source, Side::Old).is_none())
        {
            return;
        }
        let core = self.core.clone();
        self._web = cx.spawn(async move |this, cx| {
            let web = cx
                .background_spawn(async move { core.repo().ok().and_then(|r| web_url(&r)) })
                .await;
            this.update(cx, |this, cx| {
                this.web = Some(web);
                cx.notify();
            })
            .ok();
        });
    }

    /// The side a link goes to and its lines: the picked lines' side, else the new side (the
    /// old one for a deleted file). An image or a preview has no lines: the whole file.
    fn link_target(&self) -> Option<(GitHubSide, Option<(u32, u32)>)> {
        let picked = (self.image.is_none() && !self.previewing())
            .then(|| self.selection())
            .flatten();
        let side = |s| github_side(&self.source, s);
        match picked {
            Some(r) => side(r.side)
                .map(|s| (s, Some((r.start, r.end))))
                .or_else(|| {
                    side(Side::New)
                        .or_else(|| side(Side::Old))
                        .map(|s| (s, None))
                }),
            None => side(Side::New)
                .or_else(|| side(Side::Old))
                .map(|s| (s, None)),
        }
    }

    pub(super) fn has_github_link(&self) -> bool {
        matches!(self.web, Some(Some(_))) && self.link_target().is_some()
    }

    /// Copy GitHub Link (`open`: Open on GitHub).
    pub(super) fn github_link(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(Some(web)), Some((side, lines))) = (self.web.clone(), self.link_target()) else {
            return cx.propagate();
        };
        // The saved file's lines are what's matched against GitHub's.
        if lines.is_some()
            && matches!(side, GitHubSide::OnDisk { .. })
            && self.has_unsaved_edits(cx)
        {
            Toast::info("The link goes by the saved file's line numbers.")
                .title("Save the file first")
                .show(window, cx);
            return;
        }
        let core = self.core.clone();
        self._link = cx.spawn_in(window, async move |_, cx| {
            let link = cx
                .background_spawn(async move { core.in_repo(|r| resolve(r, &web, &side, lines)) })
                .await;
            cx.update(|window, cx| match link {
                Ok(url) if open => cx.open_url(&url),
                Ok(url) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                    Toast::success("Link copied").show(window, cx);
                }
                Err(e) => Toast::info(e).title("Not on GitHub yet").show(window, cx),
            })
            .ok();
        });
    }

    pub(super) fn copy_path(&self, window: &mut Window, cx: &mut App) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.source.path.clone()));
        Toast::success("Path copied").show(window, cx);
    }

    /// The line and byte a click at `x` (window coordinates) lands on in item `ix`'s `column`.
    pub(super) fn caret_at(
        &self,
        ix: usize,
        column: Column,
        x: Pixels,
        window: &Window,
        cx: &App,
    ) -> Option<(LinePos, usize)> {
        let data = self.data.as_ref()?;
        let (side, r) = *self.item_rows(ix, column).first()?;
        let row = &data.rows[r as usize];
        let pos = LinePos {
            side,
            line: if side == Side::Old { row.o } else { row.n },
        };
        let style = DiffStyle::get(cx);
        let m = Metrics::new(
            window,
            style,
            data.old.lines.max(data.new.lines),
            self.line,
            self.scroll.left(),
        );
        let body = self.body_bounds.get();
        let start = match column {
            Column::Both => m.number_w * 2.,
            Column::Left => m.number_w,
            Column::Right => body.size.width / 2. + px(1.) + m.number_w,
        } + m.sign_w
            + px(4.)
            - m.left;
        let into = x - body.origin.x - start;
        if into < px(0.) {
            return None;
        }
        let col = (into / m.char_w).floor() as usize;
        let text = data.side(side).line(pos.line);
        Some((pos, byte_at_column(&text, data.unit, col)))
    }

    /// Looks up the name at the caret (the last click on code). Go to Definition: one in this
    /// file shows here, one elsewhere opens there, several list, and on the definition itself
    /// its uses list. Peek shows the definition over the lines; References lists every use.
    pub(super) fn look_up(&mut self, kind: LookUp, window: &mut Window, cx: &mut Context<Self>) {
        let (Some((pos, byte)), Some(data)) = (self.caret, &self.data) else {
            return cx.propagate();
        };
        let side_text = data.side(pos.side);
        if !side_text.exists || !side_text.textual() {
            return;
        }
        let line = side_text.line(pos.line);
        let column = line[..byte.min(line.len())].encode_utf16().count() as u32;
        let (path, rev) = definition_side(&self.source, pos.side);
        let (text, core) = (side_text.text(), self.core.clone());
        let asked = path.clone();
        self._lookup = cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let ask = |references: bool| {
                        let request = definitions::Request {
                            path: asked.clone(),
                            text: text.clone(),
                            line: pos.line,
                            column,
                            rev: rev.clone(),
                        };
                        if references {
                            api::files::references(&core, request)
                        } else {
                            api::files::definitions(&core, request)
                        }
                    };
                    match ask(kind == LookUp::References)? {
                        p if kind == LookUp::Definition
                            && p.len() == 1
                            && p[0].path == asked
                            && p[0].line == pos.line
                            && (p[0].column..=p[0].end_column).contains(&column) =>
                        {
                            ask(true).map(|p| (LookUp::References, p))
                        }
                        p => Ok((kind, p)),
                    }
                })
                .await;
            this.update_in(cx, |this, window, cx| match found {
                Err(e) if e == definitions::CANCELLED => {}
                Err(e) => Toast::error(e).show(window, cx),
                Ok((kind, places)) => this.show_places(kind, pos.side, &path, places, window, cx),
            })
            .ok();
        });
    }

    /// A lookup by key, on lines; an image, a preview or the editor leaves the key to its host.
    pub(super) fn look_up_if_code(
        &mut self,
        kind: LookUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.can_find() {
            self.look_up(kind, window, cx)
        } else {
            cx.propagate()
        }
    }

    fn show_places(
        &mut self,
        kind: LookUp,
        side: Side,
        path: &str,
        mut places: Vec<Location>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let n = places.len();
        if n == 0 {
            let none = if kind == LookUp::References {
                "No references found."
            } else {
                "No definition found."
            };
            return Toast::info(none).show(window, cx);
        }
        if kind == LookUp::Definition && n == 1 {
            let place = places.remove(0);
            return self.go_to(side, path, &place, window, cx);
        }
        let title = match (kind, n) {
            (LookUp::References, 1) => "1 reference".to_string(),
            (LookUp::References, n) => format!("{n} references"),
            (_, 1) => "Definition".to_string(),
            (_, n) => format!("{n} definitions"),
        };
        self.places = Some(Places {
            title: title.into(),
            side,
            places,
            peek: None,
        });
        if kind == LookUp::Peek {
            self.peek_at(0, cx);
        }
        cx.notify();
    }

    /// Shows the lines around place `i` of the list: this side's own from the diff, another
    /// file's read from the side's tree.
    fn peek_at(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(p) = &mut self.places else { return };
        let Some(place) = p.places.get(i).map(copy) else {
            return;
        };
        let (path, rev) = definition_side(&self.source, p.side);
        let first = place.line.saturating_sub(PEEK_AROUND).max(1);
        let count = (place.line + PEEK_AROUND + 1 - first) as usize;
        if place.path == path
            && let Some(data) = &self.data
        {
            let text = data.side(p.side);
            let lines = (first..=text.lines)
                .take(count)
                .map(|l| peek_line(&text.line(l)))
                .collect();
            p.peek = Some(Peek {
                at: i,
                first,
                lines,
            });
            return cx.notify();
        }
        let core = self.core.clone();
        self._peek = cx.spawn(async move |this, cx| {
            let lines = cx
                .background_spawn(async move {
                    let file = match rev {
                        Some(rev) => api::files::text_at(&core, rev, place.path),
                        None => api::files::read_file(&core, place.path),
                    };
                    match file {
                        Ok(f) if f.exists && !f.binary && !f.too_large => f
                            .text
                            .lines()
                            .skip(first as usize - 1)
                            .take(count)
                            .map(peek_line)
                            .collect(),
                        _ => vec![],
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                if let Some(p) = &mut this.places {
                    p.peek = Some(Peek {
                        at: i,
                        first,
                        lines,
                    });
                    cx.notify();
                }
            })
            .ok();
        });
    }

    fn go_to(
        &mut self,
        side: Side,
        path: &str,
        place: &Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.places = None;
        if place.path == path {
            let range = LineRange {
                side,
                start: place.line,
                end: place.line,
            };
            self.scroll_to_line(
                LinePos {
                    side,
                    line: place.line,
                },
                cx,
            );
            self.select_lines(range, cx);
        } else {
            open_file(&place.path, Some((place.line, place.column)), window, cx);
        }
    }

    /// Where a lookup looks from, as a click on code sets it (tests).
    #[doc(hidden)]
    pub fn set_caret(&mut self, pos: LinePos, byte: usize) {
        self.caret = Some((pos, byte));
    }

    /// The listed places' title and the peeked lines (tests).
    #[doc(hidden)]
    pub fn places_state(&self) -> Option<(SharedString, Option<Vec<SharedString>>)> {
        let p = self.places.as_ref()?;
        Some((p.title.clone(), p.peek.as_ref().map(|k| k.lines.clone())))
    }

    pub(super) fn places_list(
        &self,
        style: &DiffStyle,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let p = self.places.as_ref()?;
        let c = &style.colors;
        let (path, _) = definition_side(&self.source, p.side);
        let side = p.side;
        let peeking = p.peek.as_ref().map(|k| k.at);
        // A single peeked place needs no list.
        let list = !(peeking.is_some() && p.places.len() == 1);
        Some(
            v_flex()
                .id("diff-places")
                .occlude()
                .absolute()
                .top_2()
                .left_4()
                .w(px(if peeking.is_some() { 640. } else { 420. }))
                .max_h(px(if peeking.is_some() { 420. } else { 280. }))
                .overflow_y_scroll()
                .p_1()
                .rounded(px(6.))
                .bg(c.elevated)
                .border_1()
                .border_color(c.border)
                .text_size(style.ui_font_size)
                .child(
                    h_flex()
                        .px_2()
                        .h(px(24.))
                        .gap_2()
                        .text_color(c.subtle)
                        .child(div().flex_1().truncate().child(p.title.clone()))
                        .when(peeking.is_some(), |el| {
                            el.child("Click the code to open it")
                        })
                        .child(
                            icon_button("places-close", Lucide::Close, "Close (esc)", false)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.places = None;
                                    cx.notify();
                                })),
                        ),
                )
                .when(list, |el| {
                    el.children(p.places.iter().enumerate().map(|(i, place)| {
                        let label = format!("{}:{}", place.path, place.line);
                        let path = path.clone();
                        div()
                            .id(("diff-place", i))
                            .flex_none()
                            .px_2()
                            .h(px(22.))
                            .rounded_sm()
                            .truncate()
                            .cursor_pointer()
                            .hover(|s| s.bg(c.hover))
                            .when(peeking == Some(i), |el| el.bg(c.selection))
                            .child(label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if peeking.is_some() {
                                    return this.peek_at(i, cx);
                                }
                                let Some(place) =
                                    this.places.as_mut().map(|p| p.places.swap_remove(i))
                                else {
                                    return;
                                };
                                this.go_to(side, &path, &place, window, cx);
                            }))
                    }))
                })
                .children(p.peek.as_ref().map(|k| {
                    let place = &p.places[k.at];
                    let numbers = format!("{}", k.first as usize + k.lines.len()).len();
                    let path = path.clone();
                    v_flex()
                        .id("diff-peek")
                        .flex_none()
                        .mt_1()
                        .py_1()
                        .rounded_sm()
                        .bg(c.background)
                        .cursor_pointer()
                        .font_family(style.font_family.clone())
                        .text_size(style.font_size)
                        .line_height(self.line)
                        .when(k.lines.is_empty(), |el| {
                            el.child(div().px_2().text_color(c.subtle).child(format!(
                                "{}:{} can't be shown here.",
                                place.path, place.line
                            )))
                        })
                        .children(k.lines.iter().enumerate().map(|(j, text)| {
                            let n = k.first + j as u32;
                            h_flex()
                                .px_2()
                                .gap_3()
                                .when(n == place.line, |el| el.bg(c.selection))
                                .child(
                                    div()
                                        .flex_none()
                                        .w(style.font_size * (numbers as f32 * 0.6 + 0.4))
                                        .text_right()
                                        .text_color(c.subtle)
                                        .child(n.to_string()),
                                )
                                .child(div().whitespace_nowrap().child(text.clone()))
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let Some(place) = this
                                .places
                                .as_ref()
                                .and_then(|p| p.places.get(p.peek.as_ref()?.at).map(copy))
                            else {
                                return;
                            };
                            this.go_to(side, &path, &place, window, cx);
                        }))
                }))
                .into_any_element(),
        )
    }
}

/// core's Location isn't Clone.
fn copy(l: &Location) -> Location {
    Location {
        path: l.path.clone(),
        line: l.line,
        column: l.column,
        end_column: l.end_column,
    }
}

/// A peeked line as drawn: tabs at four columns, no line end.
fn peek_line(line: &str) -> SharedString {
    line.trim_end_matches(['\r', '\n'])
        .replace('\t', "    ")
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEB: &str = "https://github.com/o/r";

    #[test]
    fn links_to_a_file_its_lines_or_a_folder() {
        assert_eq!(
            permalink_url(WEB, "abc", "src/a b.rs", false, Some((3, 9))),
            "https://github.com/o/r/blob/abc/src/a%20b.rs#L3-L9"
        );
        assert_eq!(
            permalink_url(WEB, "abc", "x.rs", false, Some((4, 4))),
            "https://github.com/o/r/blob/abc/x.rs#L4"
        );
        assert_eq!(
            permalink_url(WEB, "abc", "d", true, None),
            "https://github.com/o/r/tree/abc/d"
        );
    }

    #[test]
    fn which_sides_github_has() {
        let file = |status: &str| crate::FileFacts {
            status: status.into(),
            ..Default::default()
        };
        let commit = DiffSource::new(Compare::Commit { sha: "c1".into() }, "new.rs")
            .old_path(Some("old.rs".into()))
            .file(file("R"));
        assert_eq!(
            github_side(&commit, Side::Old),
            Some(GitHubSide::Commit {
                path: "old.rs".into(),
                rev: "c1^".into()
            })
        );
        let added = commit.clone().file(file("A"));
        assert_eq!(github_side(&added, Side::Old), None);
        let unstaged = DiffSource::new(Compare::Unstaged, "a.rs").file(file("M"));
        assert!(matches!(
            github_side(&unstaged, Side::New),
            Some(GitHubSide::OnDisk { .. })
        ));
        assert_eq!(github_side(&unstaged, Side::Old), None);
        let deleted = unstaged.clone().file(file("D"));
        assert_eq!(github_side(&deleted, Side::New), None);
        assert_eq!(
            github_side(&DiffSource::new(Compare::Staged, "a.rs"), Side::New),
            None
        );
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn a_commit_links_once_origin_has_it() {
        let dir = std::env::temp_dir().join(format!("gitviber-diff-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.name", "Test"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "one"]);
        let first = git(&dir, &["rev-parse", "HEAD"]);
        git(
            &dir,
            &["remote", "add", "origin", "https://github.com/o/r.git"],
        );
        git(&dir, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        git(&dir, &["commit", "-q", "-am", "two"]);
        let second = git(&dir, &["rev-parse", "HEAD"]);
        let web = web_url(&dir).expect("origin is on GitHub");
        assert_eq!(web, WEB);
        let at = |rev: &str| GitHubSide::Commit {
            path: "a.txt".into(),
            rev: rev.into(),
        };
        assert_eq!(
            resolve(&dir, &web, &at(&first), Some((1, 1))),
            Ok(format!("{WEB}/blob/{first}/a.txt#L1"))
        );
        assert!(
            resolve(&dir, &web, &at(&second), None).is_err(),
            "not pushed"
        );
        // The unpushed commit's parent is on GitHub.
        assert_eq!(
            resolve(&dir, &web, &at(&format!("{second}^")), None),
            Ok(format!("{WEB}/blob/{first}/a.txt"))
        );
        let disk = GitHubSide::OnDisk {
            path: "a.txt".into(),
        };
        assert_eq!(
            resolve(&dir, &web, &disk, None),
            Ok(format!("{WEB}/blob/{first}/a.txt"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_click_column_lands_on_the_lines_own_byte() {
        // Tabs are drawn 4 wide: column 4 is "b".
        assert_eq!(byte_at_column("a\tb", 0, 4), 2);
        assert_eq!(byte_at_column("a\tb", 0, 2), 1);
        // A 2-space indent drawn as 4.
        assert_eq!(byte_at_column("  foo", 2, 4), 2);
        assert_eq!(byte_at_column("ab", 0, 40), 2);
        assert_eq!(byte_at_column("ünï", 0, 1), 2);
    }
}
