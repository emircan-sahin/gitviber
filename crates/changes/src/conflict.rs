//! A conflicted file, block by block: each conflict's two sides with the merge base they both
//! changed a click away, and a choice for it (current, incoming, both, or written by hand). The
//! text between folds to a few lines; the result can be previewed whole. Once every block is
//! chosen, Mark resolved writes the file and stages it. A conflict with no blocks to choose in
//! (a deleted side, a binary, markers already gone) keeps one side of the whole file instead.
//! The conflicts can also go to the worktree's agent, or to the merge tool git's config names.

use crate::model::conflicts::{
    Choice, Pick, bases_for, ends_with_newline, for_terminal, is_crlf, merging_what,
    resolve_prompt, result_lines, side_names, tool_can_open, tool_name, whole_file,
};
use crate::model::text::{Block, Parsed, Segment, parse_conflicts};
use crate::toasts::git_error;
use gitviber_core::api;
use gitviber_core::git::Side as GitSide;
use gitviber_diff::DiffStyle;
use gitviber_diff::model::indent::{Shown, indent_unit};
use gitviber_terminal::agent::{Handoff, handoff_to};
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{FileIcon, PathLabel, Toggle as _};
use gitviber_ui::workspace::{Toast, Workspace};
use gitviber_ui::{ActiveAppTheme as _, AppTheme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable,
    FontFeatures, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, div,
    prelude::FluentBuilder as _, px,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Unchanged lines a fold between conflicts leaves on each side.
const CONTEXT: usize = 3;

struct Loaded {
    /// The file as read: a save checks nothing changed it since.
    text: String,
    lossy: bool,
    /// None when it can't be split into blocks: not UTF-8, or a marker left open.
    parsed: Option<Parsed>,
    /// Whether each side's file ends with a newline, for a conflict at the very end.
    sides: Option<(bool, bool)>,
    crlf: bool,
    /// Spaces an indentation level, widened when drawn (model::indent).
    unit: usize,
}

enum State {
    Loading,
    Failed(String),
    Loaded(Loaded),
}

enum Saved {
    Done,
    /// The file changed on disk since it was read.
    Changed,
}

pub struct ConflictView {
    path: String,
    /// git's two-letter conflict status: UU, AA, UD, DU, AU, UA or DD.
    code: String,
    state: State,
    choices: HashMap<usize, Choice>,
    /// Blocks being written by hand, by block.
    editing: HashMap<usize, Entity<TextareaState>>,
    /// Each block's merge base; None where it can't be told.
    bases: Vec<Option<Vec<String>>>,
    /// Blocks whose base is shown; the ones the file wrote a base for (diff3 style) start so.
    open_bases: HashSet<usize>,
    /// Folded runs of text between conflicts opened, by segment.
    open_runs: HashSet<usize>,
    preview: bool,
    /// In a rebase the sides swap meaning: HEAD is the base, incoming the user's own commit.
    rebase: bool,
    /// The merge tool git's config names.
    merge_tool: Option<String>,
    /// The tool is open on the file: a second click waits for it.
    in_tool: bool,
    focus: FocusHandle,
    busy: bool,
    _load: Task<()>,
    _save: Task<()>,
    _tool: Task<()>,
    _ask: Task<()>,
    _live: Live,
}

fn conflict_blocks(p: &Parsed) -> Vec<&Block> {
    p.segments
        .iter()
        .filter_map(|s| match s {
            Segment::Conflict(b) => Some(b),
            Segment::Text(_) => None,
        })
        .collect()
}

impl ConflictView {
    pub fn new(path: String, code: String, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = ConflictView {
            path,
            code,
            state: State::Loading,
            choices: HashMap::new(),
            editing: HashMap::new(),
            bases: vec![],
            open_bases: HashSet::new(),
            open_runs: HashSet::new(),
            preview: false,
            rebase: false,
            merge_tool: None,
            in_tool: false,
            focus: cx.focus_handle(),
            busy: false,
            _load: Task::ready(()),
            _save: Task::ready(()),
            _tool: Task::ready(()),
            _ask: Task::ready(()),
            _live: Live::new("ConflictView"),
        };
        view.refresh(cx);
        view
    }

    /// The repo changed on disk: read again, unless the user started resolving.
    pub fn repo_changed(&mut self, cx: &mut Context<Self>) {
        if self.choices.is_empty() && self.editing.is_empty() {
            self.refresh(cx);
        }
    }

    /// Reads the file again, with its base, the merge tool and what operation is going on.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let (core, path) = (crate::core(cx), self.path.clone());
        self._load = cx.spawn(async move |this, cx| {
            let (file, sides, tools, rebase, unit) = cx
                .background_spawn(async move {
                    let file = api::files::read_file(&core, path.clone());
                    let unit = file.as_ref().map_or(0, |f| indent_unit([f.text.as_str()]));
                    // The file merged again in diff3 style, for the bases git's default style
                    // doesn't write, and how each side ends.
                    let sides = api::changes::conflict_base(&core, path).ok().flatten();
                    let tools = api::changes::external_tools(&core).ok();
                    let rebase = api::changes::status(&core)
                        .ok()
                        .and_then(|s| s.operation)
                        .is_some_and(|o| o.kind == "rebase");
                    (file, sides, tools, rebase, unit)
                })
                .await;
            this.update(cx, |this, cx| {
                this.merge_tool = tools.and_then(|t| t.merge);
                this.rebase = rebase;
                this.state = match file {
                    Err(e) => State::Failed(e),
                    Ok(f) => {
                        let text = if f.exists && !f.binary && !f.too_large {
                            f.text
                        } else {
                            String::new()
                        };
                        let parsed = if f.lossy {
                            None
                        } else {
                            parse_conflicts(&text)
                        };
                        let rebuilt = sides
                            .as_ref()
                            .and_then(|s| s.merged.as_deref())
                            .and_then(parse_conflicts);
                        let blocks = parsed.as_ref().map(conflict_blocks).unwrap_or_default();
                        let rebuilt = rebuilt.as_ref().map(conflict_blocks).unwrap_or_default();
                        this.bases = bases_for(&blocks, &rebuilt);
                        this.open_bases = (0..blocks.len())
                            .filter(|&i| blocks[i].base.is_some())
                            .collect();
                        let n = blocks.len();
                        this.choices.retain(|i, _| *i < n);
                        this.editing.retain(|i, _| *i < n);
                        State::Loaded(Loaded {
                            unit,
                            crlf: is_crlf(&text),
                            lossy: f.lossy,
                            sides: sides.map(|s| (s.ours_newline, s.theirs_newline)),
                            parsed,
                            text,
                        })
                    }
                };
                cx.notify();
            })
            .ok();
        });
    }

    fn blocks(&self) -> Vec<&Block> {
        match &self.state {
            State::Loaded(Loaded {
                parsed: Some(p), ..
            }) => conflict_blocks(p),
            _ => vec![],
        }
    }

    fn choose(&mut self, block: usize, kind: Pick, cx: &mut Context<Self>) {
        if let Some(b) = self.blocks().get(block) {
            let choice = Choice::of(b, kind);
            self.choices.insert(block, choice);
            cx.notify();
        }
    }

    fn choose_all(&mut self, kind: Pick, cx: &mut Context<Self>) {
        let all: Vec<Choice> = self.blocks().iter().map(|b| Choice::of(b, kind)).collect();
        self.editing.clear();
        self.choices = all.into_iter().enumerate().collect();
        cx.notify();
    }

    fn undo(&mut self, block: usize, cx: &mut Context<Self>) {
        self.choices.remove(&block);
        cx.notify();
    }

    /// Both sides in a text box, to write the block as it should be.
    fn edit(&mut self, block: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.blocks().get(block).copied() else {
            return;
        };
        let lines: Vec<String> = b
            .ours
            .iter()
            .chain(&b.theirs)
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect();
        let rows = (lines.len() + 1).min(24);
        let text = lines.join("\n");
        let state = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(rows)
                .default_value(text)
        });
        state.update(cx, |s, cx| s.focus(window, cx));
        self.editing.insert(block, state);
        cx.notify();
    }

    fn use_edit(&mut self, block: usize, cx: &mut Context<Self>) {
        if let Some(state) = self.editing.remove(&block) {
            let text = state.read(cx).value();
            self.choices.insert(block, Choice::custom(&text));
            cx.notify();
        }
    }

    fn cancel_edit(&mut self, block: usize, cx: &mut Context<Self>) {
        self.editing.remove(&block);
        cx.notify();
    }

    fn toggle_base(&mut self, block: usize, cx: &mut Context<Self>) {
        if !self.open_bases.remove(&block) {
            self.open_bases.insert(block);
        }
        cx.notify();
    }

    fn toast(&self, toast: Toast, window: &Window, cx: &mut App) {
        if let Some(ws) = Workspace::for_window(window, cx) {
            ws.update(cx, |ws, cx| ws.toast(toast, cx));
        }
    }

    /// Writes the file the choices make and stages it, unless something changed the file since
    /// it was read: then the choices start over, so nothing is overwritten.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Loaded(l) = &self.state else {
            return;
        };
        let Some(parsed) = &l.parsed else { return };
        let n = conflict_blocks(parsed).len();
        if n == 0 || self.choices.len() < n {
            return;
        }
        let last = match parsed.segments.last() {
            Some(Segment::Conflict(_)) => self.choices.get(&(n - 1)).map(|c| c.kind),
            _ => None,
        };
        let mut out = result_lines(parsed, &self.choices, l.crlf).join("\n");
        if ends_with_newline(parsed, last, l.sides) {
            out.push('\n');
        }
        let (core, path, read) = (crate::core(cx), self.path.clone(), l.text.clone());
        self.busy = true;
        cx.notify();
        self._save = cx.spawn_in(window, async move |this, cx| {
            let p = path.clone();
            let result = cx
                .background_spawn(async move {
                    let now = api::files::read_file(&core, p.clone())?;
                    if now.text != read {
                        return Ok(Saved::Changed);
                    }
                    api::files::write_file(&core, p.clone(), out)?;
                    api::changes::stage(&core, vec![p], false)?;
                    Ok::<_, String>(Saved::Done)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                let toast = match result {
                    Ok(Saved::Done) => Toast::success(path.clone()).title("Conflict resolved"),
                    Ok(Saved::Changed) => Toast::error(
                        "Your choices were reset so nothing is overwritten. Resolve again.",
                    )
                    .title("File changed on disk"),
                    Err(e) => git_error("Could not save resolution", &e, &vec![]),
                };
                this.toast(toast, window, cx);
                this.choices.clear();
                this.editing.clear();
                this.refresh(cx);
            })
            .ok();
        });
    }

    /// For a conflict with no blocks: one side of the whole file, or the file as it is.
    fn resolve_whole(
        &mut self,
        side: Option<GitSide>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (core, path) = (crate::core(cx), self.path.clone());
        let as_is = side.is_none();
        self.busy = true;
        cx.notify();
        self._save = cx.spawn_in(window, async move |this, cx| {
            let p = path.clone();
            let result = cx
                .background_spawn(async move {
                    match side {
                        Some(side) => api::changes::resolve_side(&core, p, side),
                        None => api::changes::stage(&core, vec![p], false),
                    }
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                let toast = match (result, as_is) {
                    (Ok(()), true) => Toast::success(path.clone()).title("Marked resolved"),
                    (Ok(()), false) => Toast::success(path.clone()).title("Conflict resolved"),
                    (Err(e), true) => git_error("Could not mark resolved", &e, &vec![]),
                    (Err(e), false) => git_error("Could not resolve", &e, &vec![]),
                };
                this.toast(toast, window, cx);
                this.refresh(cx);
            })
            .ok();
        });
    }

    /// Hands every conflicted file to the agent running in this worktree's terminal; without
    /// one, or while it's busy, the toast offers the prompt to copy.
    fn ask_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (core, path) = (crate::core(cx), self.path.clone());
        let theirs = self.blocks().first().map(|b| b.theirs_label.clone());
        self._ask = cx.spawn_in(window, async move |this, cx| {
            let status = cx
                .background_spawn(async move { api::changes::status(&core) })
                .await;
            this.update_in(cx, |this, window, cx| {
                let Ok(status) = status else {
                    return;
                };
                let mut files: Vec<String> =
                    status.conflicted.iter().map(|f| f.path.clone()).collect();
                if files.is_empty() {
                    files.push(path.clone());
                }
                let op = status.operation.as_ref();
                let merging = merging_what(
                    op.map(|o| o.kind.as_str()),
                    op.and_then(|o| o.subject.as_deref()),
                    status.branch.as_deref(),
                    theirs.as_deref(),
                );
                let prompt = resolve_prompt(&files, merging.as_deref());
                let root = Path::new(&status.root);
                let agent = gitviber_terminal::worktree_agent(root, cx);
                let offer = |title: String, why: &str| {
                    let prompt = prompt.clone();
                    Toast::info(format!("{why}, or copy the prompt for it."))
                        .title(title)
                        .action("Copy prompt", move |_, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(prompt.clone()))
                        })
                };
                let toast = match handoff_to(agent.as_ref()) {
                    Handoff::Paste
                        if gitviber_terminal::paste_to_agent(
                            root,
                            &for_terminal(&prompt),
                            window,
                            cx,
                        ) =>
                    {
                        return;
                    }
                    Handoff::Wait { name, working } => offer(
                        format!(
                            "{name} is {}",
                            if working {
                                "working"
                            } else {
                                "asking something"
                            }
                        ),
                        "Ask again once it's free",
                    ),
                    _ => offer(
                        "No agent running in this worktree".into(),
                        "Start one in a terminal here",
                    ),
                };
                this.toast(toast, window, cx);
            })
            .ok();
        });
    }

    /// Opens the merge tool on the file; it's read again once the tool is closed, git having
    /// staged it if it was merged.
    fn open_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tool) = self.merge_tool.clone().filter(|_| !self.in_tool) else {
            return;
        };
        let (core, path) = (crate::core(cx), self.path.clone());
        self.in_tool = true;
        cx.notify();
        self._tool = cx.spawn_in(window, async move |this, cx| {
            let p = path.clone();
            let result = cx
                .background_spawn(async move { api::changes::open_merge_tool(&core, p) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.in_tool = false;
                let name = tool_name(&tool);
                let toast = match result {
                    Ok(()) => {
                        Toast::success(format!("{path}, in {name}")).title("Conflict resolved")
                    }
                    Err(e) => Toast::info(e).title(format!("{path} is still in conflict")),
                };
                this.toast(toast, window, cx);
                this.choices.clear();
                this.editing.clear();
                this.refresh(cx);
            })
            .ok();
        });
    }
}

#[cfg(test)]
impl ConflictView {
    pub(crate) fn bases_for_test(&self) -> Vec<Option<Vec<String>>> {
        self.bases.clone()
    }

    pub(crate) fn ask_agent_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_agent(window, cx);
    }

    pub(crate) fn act_for_test(
        &mut self,
        act: TestAct,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match act {
            TestAct::Choose(b, kind) => self.choose(b, kind, cx),
            TestAct::All(kind) => self.choose_all(kind, cx),
            TestAct::Edit(b, text) => {
                self.edit(b, window, cx);
                if let Some(s) = self.editing.get(&b) {
                    s.update(cx, |s, cx| s.set_value(text, window, cx));
                }
                self.use_edit(b, cx);
            }
            TestAct::Save => self.save(window, cx),
            TestAct::Whole(side) => self.resolve_whole(side, window, cx),
        }
    }

    pub(crate) fn indent_unit_for_test(&self) -> Option<usize> {
        match &self.state {
            State::Loaded(l) => Some(l.unit),
            _ => None,
        }
    }

    pub(crate) fn whole_file_for_test(&self) -> Option<crate::model::conflicts::WholeFile> {
        match &self.state {
            State::Loaded(l) if self.blocks().is_empty() => Some(whole_file(
                &self.code,
                l.lossy,
                !l.lossy && l.parsed.is_none(),
                self.rebase,
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
pub(crate) enum TestAct {
    Choose(usize, Pick),
    All(Pick),
    Edit(usize, String),
    Save,
    Whole(Option<GitSide>),
}

impl Focusable for ConflictView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// How code is drawn here, as in the diffs: ligatures as Settings has them, and indentation
/// widened to 4 columns a level (`unit`: the file's own step), tabs to the next stop.
struct Look<'a> {
    style: &'a DiffStyle,
    features: FontFeatures,
    unit: usize,
}

fn code_lines(lines: &[String], color: Option<Hsla>, look: &Look) -> gpui_kit::Div {
    let style = look.style;
    v_flex()
        .w_full()
        .py_0p5()
        .font_family(style.font_family.clone())
        .font_features(look.features.clone())
        .text_size(style.font_size)
        .line_height(style.line_px())
        .when_some(color, |el, c| el.text_color(c))
        .children(lines.iter().map(|l| {
            let l = l.strip_suffix('\r').unwrap_or(l);
            div()
                .px_4()
                .min_h(style.line_px())
                .whitespace_nowrap()
                .child(SharedString::from(Shown::new(l, look.unit).text))
        }))
}

impl ConflictView {
    /// Unchanged text between conflicts, folded to a few lines each side.
    fn text_run(
        &self,
        seg: usize,
        lines: &[String],
        t: &AppTheme,
        look: &Look,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dim = Some(t.foreground.opacity(0.7));
        if self.open_runs.contains(&seg) || lines.len() <= CONTEXT * 2 + 1 {
            return code_lines(lines, dim, look).into_any_element();
        }
        v_flex()
            .child(code_lines(&lines[..CONTEXT], dim, look))
            .child(
                h_flex()
                    .id(("fold", seg))
                    .gap_2()
                    .px_4()
                    .py_1()
                    .border_y_1()
                    .border_color(t.border)
                    .bg(t.panel)
                    .cursor_pointer()
                    .hover(|s| s.bg(t.elevated))
                    .text_size(px(11.5))
                    .text_color(t.subtle)
                    .child(Icon::new(Lucide::ChevronsUpDown).xsmall())
                    .child(format!("{} unchanged lines", lines.len() - CONTEXT * 2))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_runs.insert(seg);
                        cx.notify();
                    })),
            )
            .child(code_lines(&lines[lines.len() - CONTEXT..], dim, look))
            .into_any_element()
    }

    fn side(
        title: &str,
        label: &str,
        tone: Hsla,
        lines: &[String],
        t: &AppTheme,
        look: &Look,
    ) -> gpui_kit::Div {
        v_flex()
            .border_l_2()
            .border_color(tone)
            .bg(tone.opacity(0.1))
            .child(
                h_flex()
                    .gap_2()
                    .px_4()
                    .pt_1p5()
                    .text_size(px(10.5))
                    .text_color(t.muted_foreground)
                    .child(
                        div()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(title.to_uppercase()),
                    )
                    .when(!label.is_empty(), |el| {
                        el.child(
                            div()
                                .font_family(look.style.font_family.clone())
                                .text_color(t.subtle)
                                .child(label.to_string()),
                        )
                    }),
            )
            .child(if lines.is_empty() {
                div()
                    .px_4()
                    .pb_1p5()
                    .text_size(px(11.5))
                    .italic()
                    .text_color(t.subtle)
                    .child("(empty)")
            } else {
                code_lines(lines, None, look).pb_1()
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn card(
        &self,
        i: usize,
        n: usize,
        b: &Block,
        names: (&str, &str),
        t: &AppTheme,
        look: &Look,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let choice = self.choices.get(&i);
        let editing = self.editing.get(&i).cloned();
        let label = choice.map(|c| match c.kind {
            Pick::Ours => format!("Accepted {}", names.0.to_lowercase()),
            Pick::Theirs => format!("Accepted {}", names.1.to_lowercase()),
            Pick::Both => "Accepted both".into(),
            Pick::Custom => "Edited".into(),
        });
        let button = |id: &str, text: &'static str| {
            Button::new(SharedString::from(format!("{id}-{i}")))
                .xsmall()
                .label(text)
        };
        let buttons: Vec<AnyElement> = if choice.is_some() {
            vec![
                button("undo", "Undo")
                    .ghost()
                    .icon(Lucide::Undo2)
                    .on_click(cx.listener(move |this, _, _, cx| this.undo(i, cx)))
                    .into_any_element(),
            ]
        } else if editing.is_some() {
            vec![
                button("cancel", "Cancel")
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| this.cancel_edit(i, cx)))
                    .into_any_element(),
                button("use", "Use this")
                    .primary()
                    .on_click(cx.listener(move |this, _, _, cx| this.use_edit(i, cx)))
                    .into_any_element(),
            ]
        } else {
            vec![
                button("ours", "Accept current")
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(i, Pick::Ours, cx)))
                    .into_any_element(),
                button("theirs", "Accept incoming")
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(i, Pick::Theirs, cx)))
                    .into_any_element(),
                button("both", "Accept both")
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(i, Pick::Both, cx)))
                    .into_any_element(),
                button("edit", "Edit")
                    .ghost()
                    .icon(Lucide::Pencil)
                    .on_click(cx.listener(move |this, _, window, cx| this.edit(i, window, cx)))
                    .into_any_element(),
            ]
        };
        let tone = if choice.is_some() {
            t.added
        } else {
            t.conflict
        };
        let body = if let Some(c) = choice {
            code_lines(&c.lines, None, look)
                .bg(t.add_bg)
                .into_any_element()
        } else if let Some(state) = editing {
            div()
                .p_1()
                .font_family(look.style.font_family.clone())
                .font_features(look.features.clone())
                .text_size(look.style.font_size)
                .child(Textarea::new(&state))
                .into_any_element()
        } else {
            v_flex()
                .child(Self::side(
                    names.0,
                    &b.ours_label,
                    t.primary,
                    &b.ours,
                    t,
                    look,
                ))
                .when_some(self.bases.get(i).cloned().flatten(), |el, base| {
                    el.child(self.base_strip(i, &base, t, look, cx))
                })
                .child(Self::side(
                    names.1,
                    &b.theirs_label,
                    t.renamed,
                    &b.theirs,
                    t,
                    look,
                ))
                .into_any_element()
        };
        v_flex()
            .mx_3()
            .my_2()
            .rounded_md()
            .overflow_hidden()
            .border_1()
            .border_color(tone.opacity(0.55))
            .child(
                h_flex()
                    .h(px(32.))
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(t.border)
                    .bg(t.panel)
                    .text_size(px(11.5))
                    .child(
                        div()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(tone)
                            .child(format!("Conflict {} of {n}", i + 1)),
                    )
                    .when_some(label, |el, l| {
                        el.child(div().text_color(t.muted_foreground).child(format!("· {l}")))
                    })
                    .child(div().flex_1())
                    .children(buttons),
            )
            .child(body)
            .into_any_element()
    }

    /// The merge base between the two sides, folded to a strip: open to see what each changed.
    fn base_strip(
        &self,
        i: usize,
        base: &[String],
        t: &AppTheme,
        look: &Look,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.open_bases.contains(&i);
        let what = match base.len() {
            0 => "empty: both sides added these lines".to_string(),
            1 => "1 line before either side changed it".to_string(),
            n => format!("{n} lines before either side changed them"),
        };
        v_flex()
            .border_l_2()
            .border_color(t.subtle)
            .bg(t.hover)
            .child(
                h_flex()
                    .id(("base", i))
                    .gap_1()
                    .px_4()
                    .py_1()
                    .cursor_pointer()
                    .text_size(px(10.5))
                    .text_color(t.muted_foreground)
                    .child(
                        Icon::new(if open {
                            Lucide::ChevronDown
                        } else {
                            Lucide::ChevronRight
                        })
                        .xsmall(),
                    )
                    .child(
                        div()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("BASE"),
                    )
                    .child(div().text_color(t.subtle).child(what))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_base(i, cx))),
            )
            .when(open && !base.is_empty(), |el| {
                el.child(code_lines(base, None, look).pb_1())
            })
            .into_any_element()
    }

    /// Conflicts without text blocks (deletions, binaries): a side of the whole file.
    fn whole(&self, l: &Loaded, t: &AppTheme, cx: &mut Context<Self>) -> AnyElement {
        let w = whole_file(
            &self.code,
            l.lossy,
            !l.lossy && l.parsed.is_none(),
            self.rebase,
        );
        let busy = self.busy;
        let side = |ix: usize, git: GitSide, cx: &mut Context<Self>| {
            let (label, note) = w.sides[ix];
            v_flex()
                .id(("whole", ix))
                .flex_1()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(t.border_strong)
                .bg(t.panel)
                .when(!busy, |el| {
                    el.cursor_pointer()
                        .hover(|s| s.border_color(t.primary))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.resolve_whole(Some(git), window, cx)
                        }))
                })
                .when(busy, |el| el.opacity(0.5))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child(label),
                )
                .child(
                    div()
                        .mt_0p5()
                        .text_size(px(11.5))
                        .text_color(t.muted_foreground)
                        .child(note),
                )
        };
        v_flex()
            .items_center()
            .w_full()
            .pt_16()
            .px_6()
            .child(
                v_flex()
                    .max_w(px(512.))
                    .w_full()
                    .items_center()
                    .gap_1()
                    .child(
                        Icon::new(Lucide::GitMerge)
                            .size(px(28.))
                            .text_color(t.conflict),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_size(px(13.))
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(w.title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.muted_foreground)
                            .child(w.note.clone()),
                    )
                    .when(w.as_is, |el| {
                        el.child(
                            div().mt_3().child(
                                Button::new("as-is")
                                    .small()
                                    .primary()
                                    .icon(Lucide::Check)
                                    .label("Mark resolved as it is")
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.resolve_whole(None, window, cx)
                                    })),
                            ),
                        )
                    })
                    .child(
                        h_flex()
                            .mt_4()
                            .w_full()
                            .gap_2()
                            .child(side(0, GitSide::Ours, cx))
                            .child(side(1, GitSide::Theirs, cx)),
                    ),
            )
            .into_any_element()
    }
}

impl Render for ConflictView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = DiffStyle::get(cx).clone();
        let look = Look {
            style: &style,
            features: style.code_font(cx).features,
            unit: match &self.state {
                State::Loaded(l) => l.unit,
                _ => 0,
            },
        };
        let t = cx.app_theme().clone();
        let names = side_names(self.rebase);
        let n = self.blocks().len();
        let resolved = self.choices.len();
        let body = match &self.state {
            State::Loading => div().into_any_element(),
            State::Failed(e) => div()
                .p_6()
                .text_color(t.muted_foreground)
                .child(format!("Could not read the file: {e}"))
                .into_any_element(),
            State::Loaded(l) if n == 0 => self.whole(l, &t, cx),
            State::Loaded(l) => {
                let parsed = l.parsed.as_ref().expect("blocks come from a parse");
                let parts: Vec<AnyElement> = if self.preview {
                    vec![
                        code_lines(&result_lines(parsed, &self.choices, false), None, &look)
                            .into_any_element(),
                    ]
                } else {
                    let mut block = 0;
                    let mut parts = vec![];
                    for (seg, s) in parsed.segments.iter().enumerate() {
                        match s {
                            Segment::Text(lines) => {
                                parts.push(self.text_run(seg, lines, &t, &look, cx))
                            }
                            Segment::Conflict(b) => {
                                parts.push(self.card(block, n, b, names, &t, &look, cx));
                                block += 1;
                            }
                        }
                    }
                    parts
                };
                div()
                    .id("conflict-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .py_2()
                    .children(parts)
                    .into_any_element()
            }
        };
        let tool = self
            .merge_tool
            .clone()
            .filter(|_| (self.code == "UU" || self.code == "AA") && tool_can_open(&self.path));
        let small = |id: &'static str, label: String| Button::new(id).xsmall().label(label);
        v_flex()
            .id("conflict-view")
            .track_focus(&self.focus)
            .size_full()
            .bg(style.colors.background)
            .text_color(t.foreground)
            .child(
                h_flex()
                    .h(px(36.))
                    .flex_none()
                    .gap_2()
                    .pl_3()
                    .pr_2()
                    .border_b_1()
                    .border_color(t.border)
                    .text_size(px(12.))
                    .child(FileIcon::file(&self.path))
                    .child(div().min_w_0().child(PathLabel(self.path.clone().into())))
                    .child(
                        div()
                            .px_1p5()
                            .rounded_sm()
                            .bg(t.conflict_fill)
                            .text_size(px(10.5))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(t.on_status)
                            .child("Conflict"),
                    )
                    .when(n > 0, |el| {
                        el.child(
                            h_flex()
                                .text_size(px(11.5))
                                .text_color(t.muted_foreground)
                                .child(
                                    div()
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .text_color(if resolved == n {
                                            t.added
                                        } else {
                                            t.foreground
                                        })
                                        .child(resolved.to_string()),
                                )
                                .child(format!("/{n} resolved")),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        small("ask-agent", "Ask agent to resolve".into())
                            .icon(Lucide::Bot)
                            .tooltip("Pastes a prompt into the terminal an agent runs in, here")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.ask_agent(window, cx)),
                            ),
                    )
                    .when_some(tool, |el, tool| {
                        let name = tool_name(&tool);
                        el.child(
                            small(
                                "merge-tool",
                                if self.in_tool {
                                    format!("Waiting for {name}…")
                                } else {
                                    format!("Open in {name}")
                                },
                            )
                            .icon(Lucide::GitMerge)
                            .disabled(self.in_tool || self.busy)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_tool(window, cx)),
                            ),
                        )
                    })
                    .when(n > 0, |el| {
                        el.child(
                            small("preview", "Preview result".into())
                                .icon(Lucide::Eye)
                                .toggle(self.preview, cx)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.preview = !this.preview;
                                    cx.notify();
                                })),
                        )
                        .child(small("all-current", "All current".into()).on_click(
                            cx.listener(|this, _, _, cx| this.choose_all(Pick::Ours, cx)),
                        ))
                        .child(small("all-incoming", "All incoming".into()).on_click(
                            cx.listener(|this, _, _, cx| this.choose_all(Pick::Theirs, cx)),
                        ))
                        .child(
                            small("mark-resolved", "Mark resolved".into())
                                .primary()
                                .icon(Lucide::Check)
                                .loading(self.busy)
                                .disabled(self.busy || resolved < n)
                                .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                        )
                    }),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}
