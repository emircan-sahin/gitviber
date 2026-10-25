//! GitViber → About GitViber, and the version in the status bar: version, commit, OS, git, gh,
//! a line to copy for a bug report, the links, and the third-party licenses.

use gitviber_core::api::app::About;
use gitviber_ui::ActiveAppTheme;
use gitviber_ui::actions::app;
use gitviber_ui::assets::LOGO;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::Toast;
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, img, px, rems,
};

use crate::help::{REPO, bug_report_url, diagnostics, open_url, toast, with_about};

pub struct AboutView {
    about: Option<About>,
    _live: Live,
}

pub fn install(cx: &mut App) {
    cx.on_action(|_: &app::About, cx| {
        cx.defer(|cx| {
            if let Some(w) = cx
                .active_window()
                .or_else(|| cx.windows().into_iter().next())
            {
                let _ = w.update(cx, |_, window, cx| open(window, cx));
            }
        })
    });
}

/// Opens it in `window`; git may have been upgraded since, so it's asked again each time.
pub fn open(window: &mut Window, cx: &mut App) {
    let view = cx.new(|_| AboutView {
        about: None,
        _live: Live::new("AboutDialog"),
    });
    if gitviber_ui::has_core(cx) {
        let weak = view.downgrade();
        with_about(cx, move |about, cx| {
            let _ = weak.update(cx, |v, cx| {
                v.about = about;
                cx.notify();
            });
        });
    }
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.w(px(384.)).margin_top(px(110.)).child(view.clone())
    });
}

fn row(label: &'static str, value: AnyElement, cx: &App) -> [AnyElement; 2] {
    let t = cx.app_theme();
    [
        div()
            .text_right()
            .text_color(t.muted_foreground)
            .child(label)
            .into_any_element(),
        div()
            .font_family(t.fonts.code.clone())
            .child(value)
            .into_any_element(),
    ]
}

impl Render for AboutView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let text = |s: String| div().child(s).into_any_element();
        let dim = |s: &'static str, color| div().text_color(color).child(s).into_any_element();
        let a = self.about.as_ref();
        let pending = || dim("…", t.subtle);
        let commit = match a {
            // Text, not a link Button: a Button centres in its cell and has its own font size.
            Some(a) if !a.commit.is_empty() => {
                let url = format!("{REPO}/commit/{}", a.commit);
                div()
                    .id("about-commit")
                    .cursor_pointer()
                    .text_color(t.primary)
                    .underline()
                    .child(a.commit.clone())
                    .on_click(move |_, _, cx| open_url(url.clone(), cx))
                    .into_any_element()
            }
            Some(_) => dim("unknown", t.subtle),
            None => pending(),
        };
        // No row where there is no web view to name (Linux), or none was found.
        let webview = match a {
            Some(a) => a.webview.clone().map(text),
            None => cfg!(target_os = "macos").then(pending),
        };
        let mut cells: Vec<AnyElement> = Vec::new();
        let rows: [(&'static str, AnyElement); 5] = [
            ("Version", text(env!("CARGO_PKG_VERSION").to_string())),
            ("Commit", commit),
            (
                "System",
                a.map_or_else(pending, |a| text(format!("{} {}", a.os, a.arch))),
            ),
            (
                "git",
                match a {
                    Some(a) => match &a.git {
                        Some(g) => text(g.clone()),
                        None => dim("not found", t.destructive),
                    },
                    None => pending(),
                },
            ),
            (
                "gh",
                match a {
                    Some(a) => match &a.gh {
                        Some(g) => text(g.clone()),
                        None => dim("not installed", t.subtle),
                    },
                    None => pending(),
                },
            ),
        ];
        for (label, value) in rows.into_iter().chain(webview.map(|w| ("Web view", w))) {
            cells.extend(row(label, value, cx));
        }
        let line: Option<SharedString> = a.map(|a| diagnostics(a).into());
        let report = a.map(|a| bug_report_url(Some(a)));
        div()
            .flex()
            .flex_col()
            .items_center()
            .pt_4()
            .pb_2()
            .text_center()
            .child(img(LOGO).size(px(72.)))
            .child(
                div()
                    .mt_4()
                    .text_size(rems(1.25))
                    .font_weight(t.semibold())
                    .child("GitViber"),
            )
            .child(
                div()
                    .mt_1p5()
                    .max_w(px(256.))
                    .text_size(rems(0.78125))
                    .text_color(t.muted_foreground)
                    .child("A desktop git client for reviewing what your coding agents wrote."),
            )
            .child(
                div()
                    .mt_6()
                    .grid()
                    .grid_cols(2)
                    .gap_x_3()
                    .gap_y_1p5()
                    .text_left()
                    .text_size(rems(0.78125))
                    .children(cells),
            )
            .child(
                Button::new("about-copy")
                    .ghost()
                    .xsmall()
                    .mt_2()
                    .icon(IconName::Copy)
                    .label("Copy for a bug report")
                    .on_click(move |_, _, cx| match &line {
                        Some(line) => {
                            cx.write_to_clipboard(ClipboardItem::new_string(line.to_string()));
                            toast(Toast::success(line.clone()).title("Diagnostics copied"), cx);
                        }
                        None => toast(
                            Toast::error("The version info hasn't loaded.").title("Couldn't copy"),
                            cx,
                        ),
                    }),
            )
            .child(
                div()
                    .mt_6()
                    .flex()
                    .justify_center()
                    .gap_2()
                    .child(
                        Button::new("about-github")
                            .small()
                            .icon(IconName::ExternalLink)
                            .label("GitHub")
                            .on_click(|_, _, cx| open_url(REPO, cx)),
                    )
                    .child(
                        Button::new("about-bug")
                            .small()
                            .icon(IconName::Bug)
                            .label("Report a Bug")
                            .on_click(move |_, _, cx| {
                                open_url(report.clone().unwrap_or_else(|| bug_report_url(None)), cx)
                            }),
                    )
                    .child(
                        Button::new("about-license")
                            .small()
                            .icon(IconName::Scale)
                            .label("License")
                            .on_click(|_, _, cx| open_url(format!("{REPO}/blob/main/LICENSE"), cx)),
                    ),
            )
            .child(
                div()
                    .mt_5()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(rems(0.6875))
                    .text_color(t.subtle)
                    .child("© 2026 Emircan Sahin · GPL-3.0 License ·")
                    .child(
                        Button::new("about-licenses")
                            .link()
                            .xsmall()
                            .label("Third-Party Licenses")
                            .on_click(|_, window, cx| super::licenses::open(window, cx)),
                    ),
            )
    }
}
