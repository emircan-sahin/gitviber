//! The shared widgets on one page, in the app's theme: `cargo run -p gitviber-ui --example widgets`.

use gitviber_ui::ActiveAppTheme;
use gitviber_ui::widgets::{
    ChangeKind, CountBadge, FileIcon, Keycaps, LineCounts, Look, PathLabel, RelativeTime,
    StatusDot, StatusLetter, StatusPill, now_unix,
};
use gpui_kit::{
    AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px,
};

struct Widgets;

impl Render for Widgets {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let row = || div().flex().items_center().gap_3();
        let files = [
            "src/search/fuzzy.ts",
            "src/search/fuzzy.test.ts",
            "src/palette/QuickOpen.tsx",
            "Cargo.toml",
            "README.md",
            "package.json",
            "main.rs",
            "index.html",
        ];
        div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .bg(t.background)
            .child(row().children(files.iter().map(|f| {
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(FileIcon::file(f))
                    .child(PathLabel((*f).into()))
            })))
            .child(
                row()
                    .child(FileIcon::folder("src", false))
                    .child(FileIcon::folder("src", true))
                    .child(FileIcon::folder("node_modules", false)),
            )
            .child(
                row()
                    .child(CountBadge::new("3").active(true))
                    .child(CountBadge::new("12")),
            )
            .child(
                row().children(
                    [
                        ChangeKind::Added,
                        ChangeKind::Modified,
                        ChangeKind::Deleted,
                        ChangeKind::Renamed,
                        ChangeKind::Conflict,
                    ]
                    .map(|k| row().child(StatusLetter(k)).child(StatusPill(k))),
                ),
            )
            .child(
                row()
                    .child(LineCounts::new(Some(46), Some(3)))
                    .child(LineCounts::new(Some(16), None)),
            )
            .child(
                row().children(
                    [Look::Needs, Look::Unread, Look::Working, Look::Done].map(StatusDot),
                ),
            )
            .child(
                row()
                    .child(Keycaps::command("workbench.showCommands", cx))
                    .child(Keycaps::chord("alt+cmd+b")),
            )
            .child(
                row()
                    .child(RelativeTime::new(now_unix() - 3 * 3600))
                    .child(div().w(px(1.))),
            )
    }
}

fn main() {
    gitviber_ui::dev::run(|_, cx| cx.new(|_| Widgets).into());
}
