//! A conflicted file block by block: each conflict's current and incoming sides, the merge base
//! they both changed a click away, and a choice for it (current, incoming, both). Saving writes
//! the file with the choices made, keeps the markers of blocks not decided yet, and marks the
//! file resolved once none are left. The conflicts can also go to the worktree's agent, or to the
//! merge tool git's config names.

use crate::model::conflicts::{bases_for, merging_what, resolve_prompt, tool_can_open, tool_name};
use crate::model::text::{Block, Parsed, Segment, parse_conflicts};
use gitviber_core::api;
use gitviber_diff::DiffStyle;
use gitviber_terminal::agent::{Handoff, handoff_to};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, FocusHandle, Focusable, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, Window, div, prelude::FluentBuilder as _, px,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Current,
    Incoming,
    Both,
}

/// The file the choices make; blocks with none keep their markers.
pub fn resolved_text(parsed: &Parsed, choices: &HashMap<usize, Choice>) -> String {
    let mut lines: Vec<String> = vec![];
    let mut block = 0;
    for seg in &parsed.segments {
        match seg {
            Segment::Text(t) => lines.extend(t.iter().cloned()),
            Segment::Conflict(b) => {
                match choices.get(&block) {
                    Some(Choice::Current) => lines.extend(b.ours.iter().cloned()),
                    Some(Choice::Incoming) => lines.extend(b.theirs.iter().cloned()),
                    Some(Choice::Both) => lines.extend(b.ours.iter().chain(&b.theirs).cloned()),
                    None => lines.extend(markers(b)),
                }
                block += 1;
            }
        }
    }
    let mut out = lines.join("\n");
    if parsed.trailing_newline {
        out.push('\n');
    }
    out
}

fn markers(b: &Block) -> Vec<String> {
    let label = |m: &str, l: &str| {
        if l.is_empty() {
            m.to_string()
        } else {
            format!("{m} {l}")
        }
    };
    let mut out = vec![label("<<<<<<<", &b.ours_label)];
    out.extend(b.ours.iter().cloned());
    if let Some(base) = &b.base {
        out.push("|||||||".into());
        out.extend(base.iter().cloned());
    }
    out.push("=======".into());
    out.extend(b.theirs.iter().cloned());
    out.push(label(">>>>>>>", &b.theirs_label));
    out
}

enum State {
    Loading,
    Failed(String),
    /// Not text git can merge by hand here (binary, too large), or no markers left.
    Plain(String),
    Ready(Parsed),
}

pub struct ConflictView {
    path: String,
    state: State,
    choices: HashMap<usize, Choice>,
    /// Each block's merge base, by block; None where it can't be told.
    bases: Vec<Option<Vec<String>>>,
    /// Blocks whose base is shown; the ones the file wrote a base for (diff3 style) start so.
    open_bases: HashSet<usize>,
    /// The merge tool git's config names; None for none.
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

impl ConflictView {
    pub fn new(path: String, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = ConflictView {
            path,
            state: State::Loading,
            choices: HashMap::new(),
            bases: vec![],
            open_bases: HashSet::new(),
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

    /// Reads the file again; choices made stay while the blocks are the same count.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let (core, path) = (crate::core(cx), self.path.clone());
        self._load = cx.spawn(async move |this, cx| {
            let (text, sides, tools) = cx
                .background_spawn(async move {
                    let text = api::files::read_file(&core, path.clone());
                    // The file merged again in diff3 style, for the bases git's default style
                    // doesn't write.
                    let sides = api::changes::conflict_base(&core, path).ok().flatten();
                    (text, sides, api::changes::external_tools(&core).ok())
                })
                .await;
            this.update(cx, |this, cx| {
                this.merge_tool = tools.and_then(|t| t.merge);
                this.state = match text {
                    Err(e) => State::Failed(e),
                    Ok(f) if !f.exists => State::Plain("This file was deleted on one side. Take a side from the Changes list.".into()),
                    Ok(f) if f.binary || f.too_large || f.lossy => State::Plain("This file can't be merged by hand here. Take a side from the Changes list.".into()),
                    Ok(f) => match parse_conflicts(&f.text) {
                        None => State::Plain("A conflict marker is left open: fix the file in an editor.".into()),
                        Some(p) if !p.segments.iter().any(|s| matches!(s, Segment::Conflict(_))) => State::Plain("No conflicts are left. Mark it resolved from the Changes list.".into()),
                        Some(p) => {
                            let blocks = conflict_blocks(&p);
                            this.choices.retain(|i, _| *i < blocks.len());
                            let rebuilt = sides
                                .and_then(|s| s.merged)
                                .and_then(|m| parse_conflicts(&m))
                                .unwrap_or(Parsed { segments: vec![], trailing_newline: false });
                            this.bases = bases_for(&blocks, &conflict_blocks(&rebuilt));
                            this.open_bases = (0..blocks.len()).filter(|&i| blocks[i].base.is_some()).collect();
                            State::Ready(p)
                        }
                    },
                };
                cx.notify();
            })
            .ok();
        });
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

    /// Hands every conflicted file to the agent running in this worktree's terminal. Without
    /// one, or while it's busy, the prompt is copied for it instead.
    fn ask_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (core, path) = (crate::core(cx), self.path.clone());
        let theirs = match &self.state {
            State::Ready(p) => conflict_blocks(p).first().map(|b| b.theirs_label.clone()),
            _ => None,
        };
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
                let copied =
                    |this: &Self, title: String, why: &str, window: &Window, cx: &mut App| {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(prompt.clone()));
                        this.toast(
                            Toast::info(format!("{why} The prompt is copied.")).title(title),
                            window,
                            cx,
                        );
                    };
                match handoff_to(agent.as_ref()) {
                    Handoff::Paste
                        if gitviber_terminal::paste_to_agent(root, &prompt, window, cx) => {}
                    Handoff::Wait { name, working } => copied(
                        this,
                        format!(
                            "{name} is {}",
                            if working {
                                "working"
                            } else {
                                "asking something"
                            }
                        ),
                        "Paste it once it's free.",
                        window,
                        cx,
                    ),
                    _ => copied(
                        this,
                        "No agent running in this worktree".into(),
                        "Start one in a terminal here and paste it.",
                        window,
                        cx,
                    ),
                }
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
                this.refresh(cx);
            })
            .ok();
        });
    }

    fn choose(&mut self, block: usize, choice: Choice, cx: &mut Context<Self>) {
        if self.choices.get(&block) == Some(&choice) {
            self.choices.remove(&block);
        } else {
            self.choices.insert(block, choice);
        }
        cx.notify();
    }

    /// Writes the choices; with every block decided, the file is staged (marked resolved).
    fn save(&mut self, cx: &mut Context<Self>) {
        let State::Ready(parsed) = &self.state else {
            return;
        };
        let blocks = parsed
            .segments
            .iter()
            .filter(|s| matches!(s, Segment::Conflict(_)))
            .count();
        let done = self.choices.len() == blocks;
        let text = resolved_text(parsed, &self.choices);
        let (core, path) = (crate::core(cx), self.path.clone());
        self.busy = true;
        self._save = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    api::files::write_file(&core, path.clone(), text)?;
                    if done {
                        api::changes::stage(&core, vec![path], false)
                    } else {
                        Ok(())
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                if let Err(e) = result {
                    this.state = State::Failed(e);
                }
                this.choices.clear();
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

impl Focusable for ConflictView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn lines(lines: &[String], bg: Option<Hsla>, style: &DiffStyle) -> gpui_kit::Div {
    v_flex()
        .w_full()
        .when_some(bg, |el, bg| el.bg(bg))
        .font_family(style.font_family.clone())
        .text_size(style.font_size)
        .line_height(style.line_px())
        .children(lines.iter().map(|l| {
            div()
                .px_3()
                .whitespace_nowrap()
                .child(SharedString::from(l.replace('\t', "    ")))
        }))
}

impl Render for ConflictView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = DiffStyle::get(cx).clone();
        let c = &style.colors;
        let body = match &self.state {
            State::Loading => div().into_any_element(),
            State::Failed(e) => div()
                .p_6()
                .text_color(c.muted)
                .child(format!("Could not read the file: {e}"))
                .into_any_element(),
            State::Plain(text) => div()
                .p_6()
                .text_color(c.muted)
                .child(text.clone())
                .into_any_element(),
            State::Ready(parsed) => {
                let mut block = 0;
                let mut parts = vec![];
                for seg in &parsed.segments {
                    match seg {
                        Segment::Text(t) => parts.push(lines(t, None, &style).into_any_element()),
                        Segment::Conflict(b) => {
                            let i = block;
                            block += 1;
                            let chosen = self.choices.get(&i).copied();
                            let pick =
                                |label: &'static str, choice: Choice, cx: &mut Context<Self>| {
                                    Button::new(SharedString::from(format!("{label}-{i}")))
                                        .xsmall()
                                        .label(label)
                                        .toggled(chosen == Some(choice))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.choose(i, choice, cx)
                                        }))
                                };
                            let dim = |on: bool| if chosen.is_none() || on { 1. } else { 0.4 };
                            parts.push(
                                v_flex()
                                    .w_full()
                                    .my_1()
                                    .border_1()
                                    .border_color(c.border)
                                    .child(
                                        h_flex()
                                            .gap_1()
                                            .px_2()
                                            .py_1()
                                            .bg(c.panel)
                                            .text_size(style.ui_font_size)
                                            .child(
                                                div()
                                                    .text_color(c.subtle)
                                                    .child(format!("Conflict {}", i + 1)),
                                            )
                                            .child(div().flex_1())
                                            .child(pick("Current", Choice::Current, cx))
                                            .child(pick("Incoming", Choice::Incoming, cx))
                                            .child(pick("Both", Choice::Both, cx)),
                                    )
                                    .child(
                                        div()
                                            .px_3()
                                            .text_size(style.ui_font_size - px(1.))
                                            .text_color(c.subtle)
                                            .child(format!("Current ({})", b.ours_label)),
                                    )
                                    .child(
                                        div()
                                            .opacity(dim(matches!(
                                                chosen,
                                                Some(Choice::Current | Choice::Both)
                                            )))
                                            .child(lines(&b.ours, Some(c.add_bg), &style)),
                                    )
                                    .child(
                                        div()
                                            .px_3()
                                            .text_size(style.ui_font_size - px(1.))
                                            .text_color(c.subtle)
                                            .child(format!("Incoming ({})", b.theirs_label)),
                                    )
                                    .child(
                                        div()
                                            .opacity(dim(matches!(
                                                chosen,
                                                Some(Choice::Incoming | Choice::Both)
                                            )))
                                            .child(lines(&b.theirs, Some(c.del_bg), &style)),
                                    )
                                    .when_some(self.bases.get(i).cloned().flatten(), |el, base| {
                                        let open = self.open_bases.contains(&i);
                                        let what = match base.len() {
                                            0 => "empty: both sides added these lines".to_string(),
                                            1 => "1 line before either side changed it".to_string(),
                                            n => {
                                                format!("{n} lines before either side changed them")
                                            }
                                        };
                                        el.child(
                                            h_flex()
                                                .id(("base", i))
                                                .gap_1()
                                                .px_3()
                                                .py_0p5()
                                                .bg(c.hover)
                                                .cursor_pointer()
                                                .text_size(style.ui_font_size - px(1.))
                                                .text_color(c.subtle)
                                                .child(
                                                    Icon::new(if open {
                                                        Lucide::ChevronDown
                                                    } else {
                                                        Lucide::ChevronRight
                                                    })
                                                    .xsmall(),
                                                )
                                                .child(div().text_color(c.muted).child("Base"))
                                                .child(what)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.toggle_base(i, cx)
                                                })),
                                        )
                                        .when(open && !base.is_empty(), |el| {
                                            el.child(lines(&base, Some(c.hover), &style))
                                        })
                                    })
                                    .into_any_element(),
                            );
                        }
                    }
                }
                div()
                    .id("conflict-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .py_2()
                    .children(parts)
                    .into_any_element()
            }
        };
        let blocks = match &self.state {
            State::Ready(p) => p
                .segments
                .iter()
                .filter(|s| matches!(s, Segment::Conflict(_)))
                .count(),
            _ => 0,
        };
        let decided = self.choices.len();
        v_flex()
            .id("conflict-view")
            .track_focus(&self.focus)
            .size_full()
            .bg(c.background)
            .text_color(c.foreground)
            .child(
                h_flex()
                    .h(px(34.))
                    .flex_none()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(c.border)
                    .text_size(style.ui_font_size)
                    .child(div().child(self.path.clone()))
                    .when(blocks > 0, |el| {
                        el.child(
                            div()
                                .text_color(c.subtle)
                                .child(format!("{decided}/{blocks} decided")),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("ask-agent")
                            .xsmall()
                            .icon(Lucide::Bot)
                            .label("Ask agent to resolve")
                            .tooltip("Hands the conflicts to the agent in this worktree's terminal")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.ask_agent(window, cx)),
                            ),
                    )
                    .when_some(
                        self.merge_tool
                            .clone()
                            .filter(|_| tool_can_open(&self.path)),
                        |el, tool| {
                            let name = tool_name(&tool);
                            el.child(
                                Button::new("merge-tool")
                                    .xsmall()
                                    .icon(Lucide::GitMerge)
                                    .label(if self.in_tool {
                                        format!("Waiting for {name}…")
                                    } else {
                                        format!("Open in {name}")
                                    })
                                    .disabled(self.in_tool || self.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_tool(window, cx)
                                    })),
                            )
                        },
                    )
                    .when(blocks > 0, |el| {
                        el.child(
                            Button::new("save-resolution")
                                .xsmall()
                                .primary()
                                .label(if decided == blocks {
                                    "Save and mark resolved"
                                } else {
                                    "Save choices"
                                })
                                .loading(self.busy)
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        )
                    }),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_make_the_file_and_undecided_blocks_keep_markers() {
        let p = parse_conflicts("a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> main\nd\n<<<<<<< HEAD\ne\n=======\nf\n>>>>>>> main\n").unwrap();
        let mut choices = HashMap::from([(0, Choice::Both)]);
        assert_eq!(
            resolved_text(&p, &choices),
            "a\nb\nc\nd\n<<<<<<< HEAD\ne\n=======\nf\n>>>>>>> main\n"
        );
        choices.insert(1, Choice::Incoming);
        assert_eq!(resolved_text(&p, &choices), "a\nb\nc\nd\nf\n");
    }
}
