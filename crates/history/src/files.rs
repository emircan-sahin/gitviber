//! A comparison's changed files, each opening its diff: only the rows on screen are laid out.

use std::rc::Rc;

use gitviber_core::git;
use gitviber_ui::ActiveAppTheme;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{ChangeKind, FileIcon, LineCounts, PathLabel, StatusLetter};
use gpui_kit::{
    App, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle, Window,
    div, prelude::FluentBuilder as _, px, rems, uniform_list,
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
    _live: Live,
}

impl FileList {
    pub fn new(empty: impl Into<SharedString>, on_open: OnOpen) -> Self {
        FileList {
            files: None,
            error: None,
            empty: empty.into(),
            selected: None,
            on_open,
            scroll: UniformListScrollHandle::new(),
            _live: Live::new("FileList"),
        }
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

    fn open(&mut self, i: usize, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.files.as_ref().and_then(|f| f.get(i)).cloned() else {
            return;
        };
        self.selected = Some(i);
        (self.on_open)(&f, e.click_count() > 1, window, cx);
        cx.notify();
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
                                this.open(i, e, window, cx)
                            }))
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
        .into_any_element()
    }
}
