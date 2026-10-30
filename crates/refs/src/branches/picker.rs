//! Searchable branch switcher in the top bar: type to filter, ↑/↓ + Enter to switch, or create
//! what you typed. The highlighted row also offers merging it in, or rebasing onto it. A branch
//! checked out in another worktree, which git won't switch to here, opens that worktree.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use gitviber_core::{api, git};
use gitviber_ui::keymap::git as git_actions;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{
    Ends, list_step, now_unix, on_list_keys, relative_time, repaint_next_minute,
};
use gitviber_ui::workspace::PanelContext;
use gitviber_ui::{ActiveAppTheme, Project};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    Anchor, AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity,
    FocusHandle, Focusable as _, FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Pixels, Render, ScrollStrategy, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, UniformListScrollHandle, WeakEntity, Window, div, point,
    prelude::FluentBuilder as _, px, rems, size, uniform_list,
};

use super::{LOCAL, PINNED, RECENT, create_name, deletable, groups, held_in};
use crate::names::{Branch, branch_tracking, folder_name};
use crate::ops;
use crate::points::branch_point;
use crate::repo::RepoState;
use crate::requests::{self, Picker};
use crate::ui::{key_hint, key_hints};
use crate::worktrees::{Worktree, list as wt_list, main_back_offer};

/// The top bar's branch chip, kept in the bar's element state per project.
pub fn branch_chip(ctx: &PanelContext, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let project = ctx.project.clone()?;
    let id = project.entity_id();
    let chip = window.use_keyed_state(
        ("refs-branch-chip", id.as_u64() as usize),
        cx,
        |window, cx| BranchChip::new(&project, window, cx),
    );
    Some(chip.into_any_element())
}

pub struct BranchChip {
    state: Entity<RepoState>,
    window: AnyWindowHandle,
    picker: Option<Entity<BranchPicker>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl BranchChip {
    fn new(project: &Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = RepoState::get(project, cx);
        let subs = vec![cx.observe(&state, |_, _, cx| cx.notify())];
        BranchChip {
            state,
            window: window.window_handle(),
            picker: None,
            _subs: subs,
            _live: Live::new("BranchChip"),
        }
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open == self.picker.is_some() {
            return;
        }
        self.picker = open.then(|| {
            let (state, chip) = (self.state.clone(), cx.weak_entity());
            cx.new(|cx| BranchPicker::new(state, Some(chip), window, cx))
        });
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        cx.notify();
    }
}

impl Render for BranchChip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if requests::take(Picker::Branches, self.window, cx) {
            self.set_open(true, window, cx);
        }
        let t = cx.app_theme().clone();
        let s = self.state.read(cx);
        let status = s.status.clone();
        // Mid-rebase git has HEAD detached; the branch it's rebasing is what the user is on.
        let rebasing = status.as_ref().and_then(|s| {
            s.operation
                .as_ref()
                .filter(|o| o.kind == "rebase")
                .and_then(|o| o.subject.clone())
        });
        let label = match &status {
            Some(st) => st
                .branch
                .clone()
                .or(rebasing.map(|r| format!("rebasing {r}")))
                .or(st.head.as_ref().map(|h| format!("detached @ {h}"))),
            None => None,
        }
        .unwrap_or_else(|| "…".into());
        let unpublished = status.as_ref().is_some_and(|st| {
            st.branch.is_some()
                && (st.upstream.is_none() || st.upstream_gone)
                && st.follows.is_none()
        });
        let open = self.picker.is_some();
        let picker = self.picker.clone();
        let focus = picker.as_ref().map(|p| p.read(cx).query.focus_handle(cx));
        let trigger = Button::new("branch-chip").ghost().small().child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .max_w(px(288.))
                .child(Icon::new(IconName::GitBranch).small().text_color(t.primary))
                .child(
                    div()
                        .truncate()
                        .font_family(t.fonts.code.clone())
                        .text_size(rems(0.75))
                        .child(label),
                )
                .child(
                    Icon::new(IconName::ChevronsUpDown)
                        .xsmall()
                        .text_color(t.subtle),
                ),
        );
        let mut popover = Popover::new("branch-picker")
            .anchor(Anchor::TopLeft)
            .open(open)
            .on_open_change(
                cx.listener(|this, open: &bool, window, cx| this.set_open(*open, window, cx)),
            )
            .trigger(trigger)
            .content(move |_, _, _| {
                picker
                    .clone()
                    .map(|p| p.into_any_element())
                    .unwrap_or_else(|| div().into_any_element())
            });
        if let Some(f) = &focus {
            popover = popover.track_focus(f);
        }
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .child(div().text_color(t.subtle).child("/"))
            .child(popover)
            .when(unpublished, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_1p5()
                        .text_size(rems(0.6875))
                        .text_color(t.subtle)
                        .child(Icon::new(IconName::CloudOff).xsmall())
                        .child("Not published"),
                )
            })
    }
}

type StateFn = std::rc::Rc<dyn Fn(&Entity<RepoState>, &mut Window, &mut App)>;

#[derive(Clone, Debug)]
enum Row {
    Branch(Branch),
    Create(String),
}

/// Each line of the list, a group's header too, is this tall: the list draws only the lines in
/// view, which a big org's thousands of remote branches need.
const LINE: f32 = 28.;
/// The list's height at most.
const LIST_MAX: f32 = 360.;

/// A line of the list: a group's header, or row `n`.
#[derive(Clone)]
enum Line {
    Group {
        name: String,
        count: String,
        open: bool,
    },
    Row(usize),
}

/// The list as laid out from what it was laid out from: built once each time one of those
/// changes, not for each frame or key.
struct Layout {
    query: String,
    branches: Rc<Vec<Branch>>,
    pins: Vec<String>,
    visited: Vec<String>,
    collapsed: HashSet<String>,
    lines: Vec<Line>,
    rows: Vec<Row>,
    /// Each row's line, past the group headers between them.
    line_of: Vec<usize>,
    /// No group has a branch.
    empty: bool,
}

pub struct BranchPicker {
    state: Entity<RepoState>,
    chip: Option<WeakEntity<BranchChip>>,
    query: Entity<InputState>,
    main: String,
    pins: Vec<String>,
    visited: Vec<String>,
    /// Ahead, behind or gone, counted as it opens.
    tracks: HashMap<String, (u32, u32, bool)>,
    /// Squash- or rebase-merged upstream; None until known, so Clean up never counts from an older open.
    landed: Option<HashSet<String>>,
    collapsed: HashSet<String>,
    /// GitHub-protected branches, as origin's remote branches ("origin/main"): GitHub refuses
    /// their delete, so none is offered. Asked as it opens.
    guarded: HashSet<String>,
    /// Each remote's GitHub repository (owner/name), for whether the account may push there.
    remote_repos: HashMap<String, Option<String>>,
    index: Option<usize>,
    /// The list's: ↑/↓ keep the highlighted row in view, ⇧F10 finds it for its menu.
    scroll: UniformListScrollHandle,
    layout: RefCell<Option<Rc<Layout>>>,
    _reads: Vec<Task<()>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl BranchPicker {
    /// The picker's list; `chip`: the top bar's, which closes it.
    pub fn new(
        state: Entity<RepoState>,
        chip: Option<WeakEntity<BranchChip>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Find or create a branch…"));
        let subs = vec![cx.subscribe_in(
            &query,
            window,
            |this: &mut Self, _, e: &InputEvent, window, cx| match e {
                InputEvent::Change => {
                    this.index = this.first_index(cx);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.choose(window, cx),
                _ => {}
            },
        )];
        let main = ops::main_of(&state, cx);
        // A pin of a branch deleted or renamed outside the app goes, rather than pin a new branch
        // that takes its name. Not while the list is still empty.
        let saved = crate::store::pinned(&main, cx);
        let branches = state.read(cx).branches.clone();
        let pins: Vec<String> = if branches.is_empty() {
            saved.clone()
        } else {
            saved
                .iter()
                .filter(|n| branches.iter().any(|b| &b.name == *n))
                .cloned()
                .collect()
        };
        if pins.len() != saved.len() {
            crate::store::set_pinned(&main, pins.clone(), cx);
        }
        let core = gitviber_ui::core(cx);
        let c = core.clone();
        // Where GitHub would refuse a delete anyway: a protected branch, a remote you can't push.
        let rights = cx.spawn(async move |this, cx| {
            let (protected, remotes) = cx
                .background_spawn(async move {
                    (
                        api::github::gh_protected_branches(&c).unwrap_or_default(),
                        api::github::gh_remotes(&c).unwrap_or_default(),
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.guarded = protected
                    .into_iter()
                    .map(|n| format!("origin/{n}"))
                    .collect();
                this.remote_repos = remotes.into_iter().map(|r| (r.name, r.repo)).collect();
                cx.notify();
            });
        });
        let reads = vec![
            rights,
            cx.spawn(async move |this, cx| {
                let c = core.clone();
                let (visited, tracks) = cx
                    .background_spawn(async move {
                        (
                            api::branches::recent_branches(&c).unwrap_or_default(),
                            api::branches::branch_tracking(&c).unwrap_or_default(),
                        )
                    })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        let hot = this.hot_key(cx);
                        this.visited = visited;
                        this.tracks = tracks
                            .into_iter()
                            .map(|t| (t.name, (t.ahead, t.behind, t.gone)))
                            .collect();
                        this.index = this.keep_index(hot, cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                // Reads their diffs: asked on each open, never on a refresh.
                let landed = cx
                    .background_spawn(async move {
                        api::branches::merged_upstream(&core).unwrap_or_default()
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.landed = Some(landed.into_iter().collect());
                    cx.notify();
                });
            }),
        ];
        let mut picker = BranchPicker {
            state,
            chip,
            query,
            main,
            pins,
            visited: vec![],
            tracks: HashMap::new(),
            landed: None,
            collapsed: HashSet::new(),
            guarded: HashSet::new(),
            remote_repos: HashMap::new(),
            index: None,
            scroll: UniformListScrollHandle::new(),
            layout: RefCell::new(None),
            _reads: reads,
            _subs: subs,
            _live: Live::new("BranchPicker"),
        };
        picker.index = picker.first_index(cx);
        picker
    }

    fn typed(&self, cx: &App) -> String {
        self.query.read(cx).value().to_string()
    }

    /// What opening reads, now, on this thread: for a demo's first frame. The UI never calls it.
    #[doc(hidden)]
    pub fn load_now(&mut self, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        self.visited = api::branches::recent_branches(&core).unwrap_or_default();
        self.tracks = api::branches::branch_tracking(&core)
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.name, (t.ahead, t.behind, t.gone)))
            .collect();
        self.landed = Some(
            api::branches::merged_upstream(&core)
                .unwrap_or_default()
                .into_iter()
                .collect(),
        );
        self.index = self.first_index(cx);
        cx.notify();
    }

    /// The list for what's typed and the branches now, laid out again only when one of them,
    /// the pins, the recent ones or the collapsed groups changed.
    fn layout(&self, cx: &App) -> Rc<Layout> {
        let query = self.typed(cx);
        let branches = self.state.read(cx).branches.clone();
        if let Some(l) = self.layout.borrow().as_ref()
            && l.query == query
            && Rc::ptr_eq(&l.branches, &branches)
            && l.pins == self.pins
            && l.visited == self.visited
            && l.collapsed == self.collapsed
        {
            return l.clone();
        }
        let l = Rc::new(self.lay_out(query, branches));
        *self.layout.borrow_mut() = Some(l.clone());
        l
    }

    fn lay_out(&self, query: String, branches: Rc<Vec<Branch>>) -> Layout {
        let found = groups(&branches, &query, &self.pins, &self.visited);
        // Searching shows every match, whatever is collapsed.
        let searching = !query.trim().is_empty();
        // Local branches pinned or recent are listed above, not again under Local: its count says so.
        let above = found
            .iter()
            .filter(|(g, _)| g == PINNED || g == RECENT)
            .flat_map(|(_, l)| l)
            .filter(|b| !b.remote)
            .count();
        let (mut lines, mut rows, mut line_of) = (vec![], vec![], vec![]);
        let empty = found.is_empty();
        for (name, list) in found {
            let open = searching || !self.collapsed.contains(&name);
            let count = if name == LOCAL && above > 0 {
                format!("{} · {above} above", list.len() + above)
            } else {
                list.len().to_string()
            };
            lines.push(Line::Group { name, count, open });
            if open {
                for b in list {
                    line_of.push(lines.len());
                    lines.push(Line::Row(rows.len()));
                    rows.push(Row::Branch(b));
                }
            }
        }
        if let Some(name) = create_name(&branches, &query) {
            line_of.push(lines.len());
            lines.push(Line::Row(rows.len()));
            rows.push(Row::Create(name));
        }
        Layout {
            query,
            branches,
            pins: self.pins.clone(),
            visited: self.visited.clone(),
            collapsed: self.collapsed.clone(),
            lines,
            rows,
            line_of,
            empty,
        }
    }

    fn hot_row(&self, cx: &App) -> Option<Row> {
        self.index
            .and_then(|i| self.layout(cx).rows.get(i).cloned())
    }

    /// Opening starts on the current branch, where Enter does nothing; a search, on its first match.
    fn first_index(&self, cx: &App) -> Option<usize> {
        let layout = self.layout(cx);
        if layout.rows.is_empty() {
            return None;
        }
        if !self.typed(cx).trim().is_empty() {
            return Some(0);
        }
        Some(
            layout
                .rows
                .iter()
                .position(|r| matches!(r, Row::Branch(b) if b.current))
                .unwrap_or(0),
        )
    }

    /// Rows that move under the highlight (Recent arriving after the picker opened) keep it on
    /// the same branch, so a quick ↓ and Enter switches to the one that was highlighted.
    fn keep_index(&self, hot: Option<String>, cx: &App) -> Option<usize> {
        let layout = self.layout(cx);
        hot.and_then(|k| {
            layout
                .rows
                .iter()
                .position(|r| matches!(r, Row::Branch(b) if b.name == k))
        })
        .or(self.index)
    }

    fn hot_key(&self, cx: &App) -> Option<String> {
        match self.hot_row(cx) {
            Some(Row::Branch(b)) => Some(b.name),
            _ => None,
        }
    }

    /// Where line `ix` was last drawn, when all of it is in view.
    fn shown_line(&self, ix: usize) -> Option<Bounds<Pixels>> {
        let s = self.scroll.0.borrow();
        let list = s.base_handle.bounds();
        let top = list.origin.y + s.base_handle.offset().y + px(LINE) * ix as f32;
        let line = Bounds::new(point(list.origin.x, top), size(list.size.width, px(LINE)));
        (list.size.height > px(0.) && line.top() >= list.top() && line.bottom() <= list.bottom())
            .then_some(line)
    }

    /// ⇧F10: the highlighted row's menu. One scrolled out of view comes in first; the second
    /// frame has it drawn there.
    fn open_menu(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self
            .index
            .and_then(|i| self.layout(cx).line_of.get(i).copied())
        else {
            return;
        };
        if let Some(line) = self.shown_line(ix) {
            return crate::ui::open_row_menu(line, window, cx);
        }
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        let this = cx.weak_entity();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                let line = this.read_with(cx, |p, _| p.shown_line(ix)).ok().flatten();
                if let Some(line) = line {
                    crate::ui::open_row_menu(line, window, cx);
                }
            })
        });
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(chip) = &self.chip {
            let _ = chip.update(cx, |c, cx| c.close(cx));
        }
    }

    fn upstream_merged(&self, b: &Branch) -> bool {
        !b.remote && !b.merged && self.landed.as_ref().is_some_and(|l| l.contains(&b.name))
    }

    fn worktrees(&self, cx: &App) -> Vec<Worktree> {
        wt_list(&self.state.read(cx).worktrees)
    }

    fn holder_of(&self, b: &Branch, cx: &App) -> Option<Worktree> {
        let held = held_in(b, &self.state.read(cx).branches)?;
        self.worktrees(cx).into_iter().find(|w| w.path == held)
    }

    fn open_holder(&mut self, b: &Branch, window: &mut Window, cx: &mut Context<Self>) {
        let Some(held) = held_in(b, &self.state.read(cx).branches) else {
            return;
        };
        let name = b.short_name().to_string();
        let state = self.state.clone();
        match self.holder_of(b, cx).filter(|w| w.prunable) {
            Some(w) => ops::prune_holder(&state, w, name, window, cx),
            None => ops::open_holder(&state, held, name, window, cx),
        }
    }

    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.hot_row(cx) else {
            return;
        };
        let state = self.state.clone();
        match row {
            Row::Create(name) => ops::create_branch_here(&state, name, window, cx),
            Row::Branch(b) if held_in(&b, &state.read(cx).branches).is_some() => {
                self.open_holder(&b, window, cx)
            }
            Row::Branch(b) if b.remote => ops::switch_remote(&state, b, window, cx),
            Row::Branch(b) if !b.current => ops::switch_branch(&state, b.name, window, cx),
            Row::Branch(_) => return,
        }
        self.close(cx);
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        if crate::ui::is_menu_key(k) {
            self.open_menu(window, cx);
            cx.stop_propagation();
            return;
        }
        if k.modifiers.platform || k.modifiers.control || k.modifiers.alt {
            return;
        }
        let layout = self.layout(cx);
        let n = layout.rows.len();
        let typed = !self.typed(cx).is_empty();
        if let Some(to) = list_step(k, self.index, n, Ends::Wrap, typed) {
            self.index = Some(to);
            if let Some(&line) = layout.line_of.get(to) {
                self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
            }
        } else {
            match k.key.as_str() {
                "escape" => self.close(cx),
                "enter" => self.choose(window, cx),
                _ => return,
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn toggle_pin(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(i) = self.pins.iter().position(|p| p == name) {
            self.pins.remove(i);
        } else {
            self.pins.push(name.to_string());
        }
        crate::store::set_pinned(&self.main, self.pins.clone(), cx);
        cx.notify();
    }

    /// Runs a row's action and closes the picker.
    fn then(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&Entity<RepoState>, &mut Window, &mut App),
    ) {
        let state = self.state.clone();
        self.close(cx);
        f(&state, window, cx);
    }

    /// Line `ix` of `layout`: a group's header, which folds it, or a row.
    fn line(&self, layout: &Layout, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let (name, count, open) = match &layout.lines[ix] {
            Line::Row(n) => return self.row(*n, &layout.rows[*n], cx),
            Line::Group { name, count, open } => (name.clone(), count.clone(), *open),
        };
        let t = cx.app_theme();
        let searching = !layout.query.trim().is_empty();
        div()
            .id(gpui_kit::SharedString::from(format!("group-{name}")))
            .w_full()
            .flex()
            .items_end()
            .gap_1()
            .h(px(LINE))
            .px_1()
            .pb_1()
            .text_size(rems(0.65625))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(t.subtle)
            .when(!searching, |d| {
                d.cursor_pointer().hover(|s| s.text_color(t.foreground))
            })
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .xsmall(),
            )
            .child(name.to_uppercase())
            .child(div().font_weight(FontWeight::NORMAL).child(count))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.collapsed.remove(&name) {
                    this.collapsed.insert(name.clone());
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn row(&self, i: usize, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let hot = self.index == Some(i);
        let dim = if hot {
            t.primary_foreground.opacity(0.8)
        } else {
            t.subtle
        };
        let base = div()
            .id(("branch-row", i))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .h(px(LINE))
            .px_2()
            .rounded(px(3.))
            .text_size(rems(0.75))
            .when(hot, |d| d.bg(t.primary).text_color(t.primary_foreground))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.index != Some(i) {
                    this.index = Some(i);
                    cx.notify();
                }
            }))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.index = Some(i);
                this.choose(window, cx);
            }));
        let b = match row {
            Row::Create(name) => {
                return base
                    .cursor_pointer()
                    .child(Icon::new(IconName::Plus).small())
                    .child(div().truncate().child("Create branch "))
                    .child(
                        div()
                            .font_family(t.fonts.code.clone())
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name.clone()),
                    )
                    .into_any_element();
            }
            Row::Branch(b) => b.clone(),
        };
        let branches = self.state.read(cx).branches.clone();
        let current = self.state.read(cx).branch().map(str::to_string);
        let held = held_in(&b, &branches);
        let holder_gone = self.holder_of(&b, cx).is_some_and(|w| w.prunable);
        let upstream = self.upstream_merged(&b);
        let icon = if b.current {
            IconName::Check
        } else if held.is_some() {
            IconName::FolderGit2
        } else if b.remote {
            IconName::Cloud
        } else {
            IconName::GitBranch
        };
        // Text, not colour: ↑2 ↓1, "local only", "upstream gone". Merged upstream says so in its hint.
        // With no remote at all every branch is local only: the top bar's "Not published" says it.
        let remote = b.upstream.is_some() || branches.iter().any(|b| b.remote);
        let tracking = (!upstream && remote)
            .then(|| {
                branch_tracking(
                    b.remote,
                    b.upstream.as_deref(),
                    self.tracks.get(&b.name).copied(),
                )
            })
            .flatten();
        let hint = if let Some(h) = &held {
            Some(if holder_gone {
                format!("{} is gone", folder_name(h))
            } else {
                format!("opens {}", folder_name(h))
            })
        } else if hot {
            None
        } else if b.current {
            Some("current".into())
        } else {
            let when = relative_time(b.timestamp, now_unix());
            Some(if b.merged {
                format!("merged · {when}")
            } else if upstream {
                format!("merged upstream · {when}")
            } else {
                when
            })
        };
        let actions =
            hot.then(|| self.row_actions(&b, current.clone(), held.is_some(), upstream, cx));
        let menu_b = b.clone();
        let weak = cx.weak_entity();
        let pinned = self.pins.contains(&b.name);
        let main_back = main_back_offer(&self.worktrees(cx), &branches)
            .filter(|o| o.branch == b.short_name())
            .map(|o| (o.branch, folder_name(&o.main.path).to_string()));
        base.when(!b.current, |d| d.cursor_pointer())
            .child(
                Icon::new(icon)
                    .small()
                    .when(!b.current, |i| i.text_color(dim)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(t.fonts.code.clone())
                    .text_size(rems(0.71875))
                    .child(b.name.clone()),
            )
            .children(actions)
            .children(tracking.map(|tr| {
                div()
                    .flex_none()
                    .text_size(rems(0.65625))
                    .text_color(dim)
                    .child(tr.text)
            }))
            .children(hint.map(|h| {
                div()
                    .flex_none()
                    .max_w(px(160.))
                    .truncate()
                    .text_size(rems(0.65625))
                    .text_color(dim)
                    .child(h)
            }))
            .context_menu(move |menu, _, _| {
                let b = menu_b.clone();
                let act = |label: String, icon: IconName, f: StateFn| {
                    let weak = weak.clone();
                    PopupMenuItem::new(label).icon(Icon::new(icon)).on_click(
                        move |_, window, cx| {
                            let f = f.clone();
                            let _ = weak.update(cx, |this, cx| {
                                this.then(window, cx, |s, w, cx| f(s, w, cx))
                            });
                        },
                    )
                };
                let mut menu = menu;
                if let Some(h) = held.clone() {
                    let b2 = b.clone();
                    let w2 = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(if holder_gone {
                            format!("Prune {}, its folder is gone…", folder_name(&h))
                        } else {
                            format!("Open worktree {}", folder_name(&h))
                        })
                        .icon(Icon::new(IconName::FolderGit2))
                        .on_click(move |_, window, cx| {
                            let _ = w2.update(cx, |this, cx| {
                                this.close(cx);
                                this.open_holder(&b2, window, cx);
                            });
                        }),
                    );
                    if !holder_gone {
                        let b2 = b.clone();
                        menu = menu.item(act(
                            "Check out here, detached".into(),
                            IconName::GitCommitHorizontal,
                            std::rc::Rc::new(move |s, w, cx| {
                                ops::detach_here(s, b2.clone(), w, cx)
                            }),
                        ));
                    }
                    if let Some((branch, into)) = main_back.clone() {
                        menu = menu.item(act(
                            format!("Move {branch} back to {into}…"),
                            IconName::FolderGit2,
                            std::rc::Rc::new(move |s, w, cx| {
                                ops::move_main_back(s, branch.clone(), w, cx)
                            }),
                        ));
                    }
                    menu = menu.separator();
                }
                if let Some(cur) = current.clone().filter(|_| !b.current) {
                    for (label, how) in [
                        (format!("Merge into {cur}"), git::MergeKind::Ff),
                        (
                            format!("Merge into {cur} (No Fast-forward)"),
                            git::MergeKind::NoFf,
                        ),
                        (
                            format!("Squash and Merge into {cur}"),
                            git::MergeKind::Squash,
                        ),
                    ] {
                        let name = b.name.clone();
                        menu = menu.item(act(
                            label,
                            IconName::GitMerge,
                            std::rc::Rc::new(move |s, w, cx| {
                                ops::merge(s, name.clone(), how, false, w, cx)
                            }),
                        ));
                    }
                    menu = menu.separator();
                }
                // Current is the base: what the branch has that it lacks, which merging it would bring.
                let compare_label = match current.clone().filter(|_| !b.current) {
                    Some(c) => format!("Compare with {c}…"),
                    None => "Compare Branches…".into(),
                };
                let (cur, b2) = (current.clone(), b.clone());
                menu = menu.item(act(
                    compare_label,
                    IconName::GitCompareArrows,
                    std::rc::Rc::new(move |_, w, cx| match cur.clone().filter(|_| !b2.current) {
                        Some(c) => crate::hooks::open_compare(
                            Some(branch_point(&c, false)),
                            Some(branch_point(&b2.name, b2.remote)),
                            w,
                            cx,
                        ),
                        None => crate::hooks::open_compare(None, None, w, cx),
                    }),
                ));
                if !b.remote {
                    let b2 = b.clone();
                    menu = menu.item(act(
                        "Rename…".into(),
                        IconName::Pencil,
                        std::rc::Rc::new(move |_, w, cx| {
                            super::open_rename_branch(b2.clone(), w, cx)
                        }),
                    ));
                }
                let (w2, name) = (weak.clone(), b.name.clone());
                menu = menu.item(
                    PopupMenuItem::new(if pinned { "Unpin" } else { "Pin to top" })
                        .icon(Icon::new(if pinned {
                            IconName::PinOff
                        } else {
                            IconName::Pin
                        }))
                        .on_click(move |_, _, cx| {
                            let _ = w2.update(cx, |this, cx| this.toggle_pin(&name, cx));
                        }),
                );
                let base = b.full_ref();
                menu = menu.item(act(
                    format!("New branch from {}…", b.name),
                    IconName::GitBranchPlus,
                    std::rc::Rc::new(move |_, w, cx| super::open_new_branch(base.clone(), w, cx)),
                ));
                if !b.remote {
                    let b2 = b.clone();
                    menu = menu.separator().item(act(
                        if b.upstream.is_some() {
                            "Change upstream…".into()
                        } else {
                            "Set upstream…".into()
                        },
                        IconName::Link,
                        std::rc::Rc::new(move |_, w, cx| {
                            super::open_set_upstream(b2.clone(), w, cx)
                        }),
                    ));
                    if let Some(up) = b.upstream.clone() {
                        let name = b.name.clone();
                        menu = menu.item(act(
                            format!("Unset upstream ({up})"),
                            IconName::Unlink,
                            std::rc::Rc::new(move |s, w, cx| {
                                let (name, up) = (name.clone(), up.clone());
                                let action = crate::repo::GitAction::new("Unset upstream")
                                    .untracked()
                                    .done(format!("{name} no longer tracks {up}"));
                                drop(s.update(cx, |s, cx| {
                                    s.run(
                                        action,
                                        move |core, _| {
                                            crate::repo::plain(api::branches::set_upstream(
                                                core, name, None,
                                            ))
                                        },
                                        w,
                                        cx,
                                    )
                                }));
                            }),
                        ));
                    }
                }
                menu
            })
            .into_any_element()
    }

    fn row_actions(
        &self,
        b: &Branch,
        current: Option<String>,
        held: bool,
        upstream: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.app_theme().clone();
        let mut row: Vec<AnyElement> = vec![];
        let mut button =
            |id: &'static str, icon: IconName, tip: String, f: StateFn, cx: &mut Context<Self>| {
                row.push(
                    Button::new(id)
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(icon).text_color(t.primary_foreground))
                        .tooltip(tip)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            let f = f.clone();
                            this.then(window, cx, |s, w, cx| f(s, w, cx));
                        }))
                        .into_any_element(),
                );
            };
        if !held {
            let name = b.short_name().to_string();
            let tip = if b.current {
                "Open a terminal here"
            } else {
                "Open a terminal in a new worktree"
            };
            button(
                "br-terminal",
                IconName::SquareTerminal,
                tip.into(),
                std::rc::Rc::new(move |s, w, cx| ops::branch_terminal(s, name.clone(), w, cx)),
                cx,
            );
        }
        if let Some(cur) = current.filter(|_| !b.current) {
            let name = b.name.clone();
            button(
                "br-merge",
                IconName::GitMerge,
                format!("Merge into {cur}"),
                std::rc::Rc::new(move |s, w, cx| {
                    ops::merge(s, name.clone(), git::MergeKind::Ff, false, w, cx)
                }),
                cx,
            );
            let name = b.name.clone();
            button(
                "br-rebase",
                IconName::GitPullRequestArrow,
                format!("Rebase {cur} onto it"),
                std::rc::Rc::new(move |s, w, cx| ops::rebase(s, name.clone(), false, None, w, cx)),
                cx,
            );
        }
        let push = |remote: &str| {
            let repo = self.remote_repos.get(remote).cloned().flatten()?;
            crate::hooks::push_access(&repo, cx)
        };
        if deletable(b, held, &self.guarded, push) {
            let tip = if b.remote {
                "Delete from the remote…"
            } else if b.merged || upstream {
                "Delete branch (merged)"
            } else {
                "Delete branch…"
            };
            let b2 = b.clone();
            button(
                "br-delete",
                IconName::Trash,
                tip.into(),
                std::rc::Rc::new(move |s, w, cx| {
                    ops::delete_branch(s, b2.clone(), upstream, w, cx)
                }),
                cx,
            );
        }
        div()
            .flex()
            .flex_none()
            .gap_0p5()
            .children(row)
            .into_any_element()
    }
}

impl gpui_kit::Focusable for BranchPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.focus_handle(cx)
    }
}

impl Render for BranchPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The rows' "3h ago" stay current while it's open.
        repaint_next_minute(window, cx);
        let t = cx.app_theme().clone();
        let layout = self.layout(cx);
        let searching = !layout.query.trim().is_empty();
        let lines = layout.lines.len();
        let branches = self.state.read(cx).branches.clone();
        let current = self.state.read(cx).branch().map(str::to_string);
        // Merged and held by no worktree, this one included: deleting them loses nothing.
        let stale: Vec<String> = branches
            .iter()
            .filter(|b| b.merged && b.worktree.is_none())
            .map(|b| b.name.clone())
            .collect();
        let squashed: Vec<String> = branches
            .iter()
            .filter(|b| self.upstream_merged(b) && !b.current && b.worktree.is_none())
            .map(|b| b.name.clone())
            .collect();
        let cleanable = stale.len() + squashed.len();
        let hot = self.hot_row(cx);
        let enter = match &hot {
            Some(Row::Branch(b)) => match held_in(b, &branches) {
                Some(h) if self.holder_of(b, cx).is_some_and(|w| w.prunable) => {
                    format!("prune {}, then switch", folder_name(&h))
                }
                Some(h) => format!("opens {}", folder_name(&h)),
                None => "switch".into(),
            },
            _ => "switch".into(),
        };
        let link = |id: &'static str| {
            div()
                .id(id)
                .flex_none()
                .h(px(20.))
                .flex()
                .items_center()
                .px_1p5()
                .rounded(px(3.))
                .cursor_pointer()
                .hover(|s| s.bg(t.hover).text_color(t.foreground))
        };
        let base = current
            .as_ref()
            .map_or("HEAD".to_string(), |c| format!("refs/heads/{c}"));
        on_list_keys(div(), cx.listener(Self::on_key))
            .key_context("BranchPicker ListFilter")
            .capture_key_down(cx.listener(Self::on_key))
            .on_action(
                cx.listener(|this, _: &git_actions::RenameBranch, window, cx| {
                    if let Some(Row::Branch(b)) = this.hot_row(cx)
                        && !b.remote
                    {
                        this.close(cx);
                        super::open_rename_branch(b, window, cx);
                    }
                }),
            )
            .w(px(384.))
            .flex()
            .flex_col()
            .text_size(rems(0.75))
            .child(
                div().p_1().border_b_1().border_color(t.border).child(
                    Input::new(&self.query)
                        .appearance(false)
                        .prefix(Icon::new(IconName::Search).small().text_color(t.subtle)),
                ),
            )
            .child(
                div()
                    .p_1()
                    .when(lines > 0, |d| {
                        d.child(
                            uniform_list(
                                "branch-list",
                                lines,
                                cx.processor(|this, range: Range<usize>, _, cx| {
                                    let layout = this.layout(cx);
                                    range.map(|ix| this.line(&layout, ix, cx)).collect()
                                }),
                            )
                            .h(px((lines as f32 * LINE).min(LIST_MAX)))
                            .track_scroll(&self.scroll),
                        )
                    })
                    .when(layout.empty && !searching, |d| {
                        d.child(
                            div()
                                .px_2()
                                .py_3()
                                .text_center()
                                .text_color(t.subtle)
                                .child("No branches"),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .border_t_1()
                    .border_color(t.border)
                    .px_2p5()
                    .py_1p5()
                    .text_size(rems(0.65625))
                    .text_color(t.subtle)
                    // The hints get a line of their own: beside the actions they were cut short.
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_1()
                            .child(link("br-new").child("New branch…").on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.close(cx);
                                    super::open_new_branch(base.clone(), window, cx);
                                },
                            )))
                            .when(self.landed.is_some() && cleanable > 0, |d| {
                                d.child(
                                    link("br-cleanup")
                                        .child(format!("Clean up {cleanable} merged"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            let (stale, squashed) =
                                                (stale.clone(), squashed.clone());
                                            this.then(window, cx, |s, w, cx| {
                                                ops::clean_up_branches(s, stale, squashed, w, cx)
                                            });
                                        })),
                                )
                            }),
                    )
                    .child(key_hints([
                        key_hint("enter", enter),
                        key_hint("f2", "rename"),
                        key_hint("", "right-click for more"),
                    ])),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::time::Instant;

    use gitviber_ui::Project;
    use gpui_kit::{TestAppContext, VisualTestContext, point, px, size};

    use super::*;

    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui_kit::test]
    fn five_thousand_remote_branches_filter_as_you_type(cx: &mut TestAppContext) {
        let dir = gitviber_ui::test_support::config_dir("refs-picker");
        cx.update(|cx| {
            gitviber_ui::init(cx);
            gitviber_ui::install_core(cx);
        });
        let (_, cx) = cx.add_window_view(|_, _| Host);
        let state = cx.update(|_, cx| {
            let project = cx.new(|cx| Project::new(dir.clone(), cx));
            RepoState::get(&project, cx)
        });
        cx.run_until_parked();
        let mut list: Vec<Branch> = (0..5000)
            .map(|i| Branch {
                name: format!("origin/feature/team-{}/ticket-{i}", i % 40),
                remote: true,
                sha: format!("{i:040x}"),
                timestamp: 1_700_000_000 - i as i64,
                ..Default::default()
            })
            .collect();
        list.push(Branch {
            name: "main".into(),
            current: true,
            upstream: Some("origin/main".into()),
            ..Default::default()
        });
        state.update(cx, |s, _| s.branches = Rc::new(list));
        let picker =
            cx.update(|window, cx| cx.new(|cx| BranchPicker::new(state, None, window, cx)));
        let draw = |cx: &mut VisualTestContext| {
            let picker = picker.clone();
            let start = Instant::now();
            cx.draw(
                point(px(0.), px(0.)),
                size(px(400.), px(600.)),
                move |_, _| div().child(picker),
            );
            start.elapsed()
        };
        let first = draw(cx);
        let mut typing = std::time::Duration::ZERO;
        for q in ["t", "ti", "tic", "tick", "ticket-4", "ticket-49"] {
            picker.update_in(cx, |p, window, cx| {
                p.query.update(cx, |i, cx| i.set_value(q, window, cx))
            });
            typing = typing.max(draw(cx));
        }
        eprintln!(
            "branch picker, 5001 branches: first draw {first:?}, slowest keystroke {typing:?}"
        );
        // ticket-49, -490…499 and -4900…4999 under origin's header, then Create ticket-49.
        let shown = picker.read_with(cx, |p, cx| {
            let l = p.layout(cx);
            (l.rows.len(), l.lines.len())
        });
        assert_eq!(shown, (112, 113));
        // Debug build, M-series: 960 ms a key while every row was built, 7 ms drawing the ones
        // in view.
        assert!(typing.as_millis() < 250, "{typing:?}");
    }
}
