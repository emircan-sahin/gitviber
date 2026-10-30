//! The status bar's Open in: a click opens the worktree as a project, with the open file in it,
//! in the app picked last; the arrow lists the rest. File → Open in… (⇧⌘O) runs the same, and
//! shows the list until an app has been picked.

use gitviber_ui::memory::Live;
use gitviber_ui::open_in::{self, OpenApp, OpenTarget, grouped};
use gitviber_ui::widgets::Pressable as _;
use gitviber_ui::widgets::{Ends, list_step};
use gitviber_ui::workspace::Selection;
use gitviber_ui::{ActiveAppTheme, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    Anchor, App, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, img, prelude::FluentBuilder as _, px, rems,
};

pub struct OpenInButton {
    workspace: Entity<Workspace>,
    open: bool,
    /// The list's: it takes the keys as it opens.
    focus: FocusHandle,
    /// The row the keys are on: the apps as listed, then Add Your Own App….
    index: Option<usize>,
    _live: Live,
}

/// The apps in the list's order.
fn listed(cx: &App) -> Vec<OpenApp> {
    grouped(&open_in::apps(cx))
        .into_iter()
        .flat_map(|(_, list)| list)
        .collect()
}

/// What a click opens: the worktree as a project, with the open tab's file in it while it's on
/// disk (a deleted change's isn't).
pub fn target_of(selection: Option<&Selection>) -> OpenTarget {
    let path = match selection {
        Some(Selection::File { path }) => Some(path.clone()),
        Some(
            Selection::Unstaged { file }
            | Selection::Staged { file }
            | Selection::Conflict { file }
            | Selection::Branch { file, .. },
        ) if file.status != "D" => Some(file.path.clone()),
        _ => None,
    };
    OpenTarget::project(path, None)
}

impl OpenInButton {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        Self {
            workspace,
            open: false,
            focus: cx.focus_handle(),
            index: None,
            _live: Live::new("OpenInButton"),
        }
    }

    fn target(&self, cx: &App) -> OpenTarget {
        let ws = self.workspace.read(cx);
        target_of(ws.tabs().active().and_then(|t| t.item.selection()))
    }

    /// The command: the app picked last, else the list.
    pub fn run(&mut self, cx: &mut Context<Self>) {
        match open_in::last(cx) {
            Some(app) => {
                let target = self.target(cx);
                open_in::open(&app, target, cx);
            }
            None => self.show(true, cx),
        }
    }

    /// Apps come and go: looked for again each time the list opens.
    /// The keys start on the app picked last.
    fn show(&mut self, open: bool, cx: &mut Context<Self>) {
        if open && !self.open {
            open_in::refresh(cx);
            let last = open_in::last(cx).map(|a| a.id);
            self.index = Some(
                listed(cx)
                    .iter()
                    .position(|a| Some(&a.id) == last.as_ref())
                    .unwrap_or(0),
            );
        }
        self.open = open;
        cx.notify();
    }

    /// ↑/↓, Home/End, PgUp/PgDn move through the list, ↵ picks; Esc is the popover's.
    fn on_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let apps = listed(cx);
        let k = &e.keystroke;
        if let Some(to) = list_step(k, self.index, apps.len() + 1, Ends::Wrap, false) {
            self.index = Some(to);
        } else if k.key == "enter" && !k.modifiers.modified() {
            match self.index.map(|i| apps.get(i)) {
                Some(Some(app)) => self.pick(app, cx),
                Some(None) => self.add_own(cx),
                None => return,
            }
        } else {
            return;
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn pick(&mut self, app: &OpenApp, cx: &mut Context<Self>) {
        let target = self.target(cx);
        self.show(false, cx);
        open_in::open(app, target, cx);
    }

    fn add_own(&mut self, cx: &mut Context<Self>) {
        self.show(false, cx);
        crate::settings_window::open(Some(crate::settings_window::Section::OpenIn), cx);
    }

    /// The pointer moves the keys' row too, so one row shows lit.
    fn hover_row(&mut self, i: usize, cx: &mut Context<Self>) {
        if self.index != Some(i) {
            self.index = Some(i);
            cx.notify();
        }
    }
}

impl Render for OpenInButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let last = open_in::last(cx);
        let label: SharedString = last
            .as_ref()
            .map_or("Open in…".into(), |a| a.name.clone().into());
        let icon = last.as_ref().and_then(|a| open_in::icon(a, cx));
        let keys = gitviber_ui::widgets::command_kbd("file.openIn", cx);
        let tip: SharedString = match &last {
            Some(a) => format!("Open in {}", a.name).into(),
            None => "Open in…".into(),
        };
        let apps = open_in::apps(cx);
        let picked = last.map(|a| a.id);
        let this = cx.entity().downgrade();
        let index = self.index;
        let rows = move |cx: &App| {
            let t = cx.app_theme().clone();
            let mut out: Vec<gpui_kit::AnyElement> = Vec::new();
            let mut row = 0;
            let (lit_of, hover) = (this.clone(), t.hover);
            let lit = move |i: usize, d: gpui_kit::Stateful<gpui_kit::Div>| {
                let this = lit_of.clone();
                d.when(index == Some(i), |d| d.bg(hover))
                    .on_mouse_move(move |_, _, cx| {
                        let _ = this.update(cx, |b, cx| b.hover_row(i, cx));
                    })
            };
            for (i, (group, list)) in grouped(&apps).into_iter().enumerate() {
                if i > 0 {
                    out.push(div().my_1().h_px().bg(t.border).into_any_element());
                }
                out.push(
                    div()
                        .px_2()
                        .pt_1()
                        .text_size(rems(0.65625))
                        .font_weight(t.semibold())
                        .text_color(t.subtle)
                        .child(group.label())
                        .into_any_element(),
                );
                for app in list {
                    let (this, chosen) = (this.clone(), picked.as_deref() == Some(app.id.as_str()));
                    let name = app.name.clone();
                    let i = row;
                    row += 1;
                    out.push(
                        lit(
                            i,
                            div()
                                .id(SharedString::from(format!("open-in-{}", app.id)))
                                .flex()
                                .items_center()
                                .gap_2()
                                .h(px(26.))
                                .px_2()
                                .rounded(px(3.))
                                .cursor_pointer()
                                .hover(|s| s.bg(t.hover))
                                .child(div().flex_1().child(name))
                                .when(chosen, |d| d.child(Icon::new(IconName::Check).xsmall()))
                                .on_click(move |_, _, cx| {
                                    let _ = this.update(cx, |b, cx| b.pick(&app, cx));
                                }),
                        )
                        .into_any_element(),
                    );
                }
            }
            if apps.is_empty() {
                out.push(
                    div()
                        .px_2()
                        .py_1()
                        .text_color(t.subtle)
                        .child("No known editors or terminals found")
                        .into_any_element(),
                );
            }
            out.push(div().my_1().h_px().bg(t.border).into_any_element());
            let close = this.clone();
            out.push(
                lit(
                    row,
                    div()
                        .id("open-in-add")
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(px(26.))
                        .px_2()
                        .rounded(px(3.))
                        .cursor_pointer()
                        .hover(|s| s.bg(t.hover))
                        .child(Icon::new(IconName::Settings2).xsmall())
                        .child("Add Your Own App…")
                        .on_click(move |_, _, cx| {
                            let _ = close.update(cx, |b, cx| b.add_own(cx));
                        }),
                )
                .into_any_element(),
            );
            out
        };
        let focus = self.focus.clone();
        let on_key = std::rc::Rc::new(cx.listener(Self::on_key));
        let click = cx.entity().downgrade();
        let arrow = cx.entity().downgrade();
        div()
            .flex()
            .items_center()
            .h_full()
            .child(
                div()
                    .id("status-open-in")
                    .pressable(cx)
                    .flex()
                    .items_center()
                    .gap_1()
                    .h_full()
                    .px(px(6.))
                    .rounded(px(3.))
                    .hover(|s| s.bg(t.hover))
                    .child(match icon {
                        Some(image) => img(image).size(px(13.)).into_any_element(),
                        None => Icon::new(IconName::SquareArrowOutUpRight)
                            .xsmall()
                            .into_any_element(),
                    })
                    .child(label)
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                            .key_binding(keys.clone())
                            .build(window, cx)
                    })
                    .on_click(move |_, _, cx| {
                        let _ = click.update(cx, |b, cx| b.run(cx));
                    }),
            )
            .child(
                Popover::new("open-in-list")
                    .anchor(Anchor::BottomRight)
                    .open(self.open)
                    .track_focus(&focus)
                    .on_open_change(move |open: &bool, _, cx| {
                        let _ = arrow.update(cx, |b, cx| b.show(*open, cx));
                    })
                    .trigger(
                        Button::new("status-open-in-arrow")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::ChevronDown).xsmall()),
                    )
                    .content(move |_, _, cx| {
                        let on_key = on_key.clone();
                        div()
                            .track_focus(&focus)
                            .on_key_down(move |e, window, cx| on_key(e, window, cx))
                            .w(px(240.))
                            .p_1()
                            .flex()
                            .flex_col()
                            .text_size(rems(0.75))
                            .children(rows(cx))
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn change(status: &str) -> Arc<gitviber_core::git::FileChange> {
        Arc::new(gitviber_core::git::FileChange {
            path: "src/a.rs".into(),
            old_path: None,
            status: status.into(),
            additions: None,
            deletions: None,
            oid: None,
            index_oid: None,
            conflict: None,
            mode: None,
            submodule: None,
            nested: None,
        })
    }

    #[test]
    fn the_worktree_opens_with_the_open_file_in_it_while_its_on_disk() {
        let file = Selection::File {
            path: "README.md".into(),
        };
        assert_eq!(target_of(Some(&file)).path, "README.md");
        assert!(target_of(Some(&file)).project);
        let edited = Selection::Unstaged { file: change("M") };
        assert_eq!(target_of(Some(&edited)).path, "src/a.rs");
        let deleted = Selection::Staged { file: change("D") };
        assert_eq!(target_of(Some(&deleted)).path, "");
        assert_eq!(target_of(None), OpenTarget::project(None, None));
    }
}
