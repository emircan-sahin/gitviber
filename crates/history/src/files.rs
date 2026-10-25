//! A comparison's changed files, each opening its diff: only the rows on screen are laid out.

use std::rc::Rc;

use gitviber_core::git;
use gitviber_ui::ActiveAppTheme;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{ChangeKind, FileIcon, LineCounts, PathLabel, StatusLetter};
use gpui_kit::{
    App, ClickEvent, Context, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _,
    Styled as _, UniformListScrollHandle, Window, div, prelude::FluentBuilder as _, px, rems,
    uniform_list,
};

/// What a click on a file does: `pin` for a double click.
pub type OnOpen = Rc<dyn Fn(&git::FileChange, bool, &mut Window, &mut App)>;

pub struct FileList {
    files: Option<Rc<Vec<git::FileChange>>>,
    error: Option<SharedString>,
    empty: SharedString,
    selected: Option<usize>,
    on_open: OnOpen,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    _live: Live,
}

impl FileList {
    pub fn new(empty: impl Into<SharedString>, on_open: OnOpen, cx: &mut Context<Self>) -> Self {
        FileList {
            files: None,
            error: None,
            empty: empty.into(),
            selected: None,
            on_open,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            _live: Live::new("FileList"),
        }
    }

    /// What it says with no files: the comparison knows why.
    pub fn set_empty(&mut self, empty: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.empty = empty.into();
        cx.notify();
    }

    pub fn set(&mut self, files: Result<Vec<git::FileChange>, String>, cx: &mut Context<Self>) {
        match files {
            Ok(f) => {
                self.files = Some(Rc::new(f));
                self.error = None;
            }
            Err(e) => self.error = Some(e.into()),
        }
        self.selected = None;
        cx.notify();
    }

    pub fn files(&self) -> Option<&Rc<Vec<git::FileChange>>> {
        self.files.as_ref()
    }

    /// Lines added and removed, over all files.
    pub fn counts(&self) -> (u32, u32) {
        self.files
            .iter()
            .flat_map(|f| f.iter())
            .fold((0, 0), |(a, d), f| {
                (a + f.additions.unwrap_or(0), d + f.deletions.unwrap_or(0))
            })
    }

    fn open(&mut self, i: usize, pin: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.files.as_ref().and_then(|f| f.get(i)).cloned() else {
            return;
        };
        self.selected = Some(i);
        self.scroll.scroll_to_item(i, ScrollStrategy::Nearest);
        (self.on_open)(&f, pin, window, cx);
        cx.notify();
    }

    /// ↑/↓ preview the next file, ↵ keeps its tab.
    pub(crate) fn key_down(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let n = self.files.as_ref().map_or(0, |f| f.len());
        let m = &e.keystroke.modifiers;
        if n == 0 || m.platform || m.control || m.alt || m.shift {
            return;
        }
        let to = match (e.keystroke.key.as_str(), self.selected) {
            ("down", None) => 0,
            ("down", Some(i)) => (i + 1).min(n - 1),
            ("up", None) => n - 1,
            ("up", Some(i)) => i.saturating_sub(1),
            ("enter", Some(i)) if i < n => {
                self.open(i, true, window, cx);
                cx.stop_propagation();
                return;
            }
            _ => return,
        };
        cx.stop_propagation();
        if Some(to) != self.selected {
            self.open(to, false, window, cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }
}

impl Render for FileList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let Some(files) = self.files.clone() else {
            let text: SharedString = self.error.clone().unwrap_or_else(|| "Loading…".into());
            return div()
                .py_1()
                .pl_4()
                .pr_2()
                .text_size(rems(0.71875))
                .text_color(t.subtle)
                .child(text)
                .into_any_element();
        };
        if files.is_empty() {
            return div()
                .py_1()
                .pl_4()
                .text_size(rems(0.71875))
                .text_color(t.subtle)
                .child(self.empty.clone())
                .into_any_element();
        }
        let focus = self.focus.clone();
        let selected = self.selected;
        uniform_list(
            "files",
            files.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                let t = cx.app_theme().clone();
                range
                    .map(|i| {
                        let f = &files[i];
                        let active = selected == Some(i);
                        div()
                            .id(("file", i))
                            .relative()
                            .w_full()
                            .overflow_hidden()
                            .flex()
                            .h(px(26.))
                            .items_center()
                            .gap_2()
                            .pl_4()
                            .pr_2()
                            .text_size(rems(0.75))
                            .cursor_pointer()
                            .when(active, |d| {
                                d.bg(t.primary.opacity(0.15)).child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .left_0()
                                        .w(px(2.))
                                        .bg(t.primary),
                                )
                            })
                            .when(!active, |d| d.hover(|s| s.bg(t.hover)))
                            .child(FileIcon::file(&f.path))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(PathLabel(f.path.clone().into())),
                            )
                            .child(LineCounts::new(f.additions, f.deletions))
                            .child(StatusLetter(ChangeKind::from_letter(
                                f.status.chars().next().unwrap_or('M'),
                            )))
                            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                                this.focus.focus(window, cx);
                                this.open(i, e.click_count() > 1, window, cx)
                            }))
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
        .track_focus(&focus)
        .on_key_down(cx.listener(Self::key_down))
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui_kit::{AppContext as _, Keystroke, TestAppContext};

    use super::*;

    fn file(path: &str) -> git::FileChange {
        git::FileChange {
            path: path.into(),
            old_path: None,
            status: "M".into(),
            additions: Some(1),
            deletions: Some(0),
            oid: None,
            index_oid: None,
            conflict: None,
            mode: None,
            submodule: None,
            nested: None,
        }
    }

    #[gpui_kit::test]
    fn arrows_preview_the_next_file_and_enter_keeps_it(cx: &mut TestAppContext) {
        let opened = Rc::new(RefCell::new(Vec::<(String, bool)>::new()));
        let log = opened.clone();
        let cx = cx.add_empty_window();
        let list = cx.update(|_, cx| {
            cx.new(|cx| {
                FileList::new(
                    "No differences.",
                    Rc::new(move |f, pin, _, _| log.borrow_mut().push((f.path.clone(), pin))),
                    cx,
                )
            })
        });
        list.update(cx, |l, cx| l.set(Ok(vec![file("a"), file("b")]), cx));
        let key = |k: &str| KeyDownEvent {
            keystroke: Keystroke::parse(k).unwrap(),
            is_held: false,
            prefer_character_input: false,
        };
        list.update_in(cx, |l, window, cx| {
            for k in ["down", "down", "down", "enter", "up", "shift-down"] {
                l.key_down(&key(k), window, cx);
            }
        });
        // The last row holds; ⇧ isn't a move here.
        assert_eq!(
            *opened.borrow(),
            [
                ("a".into(), false),
                ("b".into(), false),
                ("b".into(), true),
                ("a".into(), false)
            ]
        );
        assert_eq!(list.read_with(cx, |l, _| l.selected()), Some(0));
    }
}
