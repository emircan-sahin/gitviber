//! The section navigator, the sections shown to jump to: a rail beside the guide on a wide tab,
//! each section's files under it (folded by its chevron), a row of chips over the guide on a
//! narrow one.

use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;

use super::parts::{importance_icon, importance_tone};
use super::render::title_text;
use super::*;
use crate::model::nav::{NavRow, file_tree};

/// A tab this wide has room for the rail beside the guide and its diffs.
pub(super) const RAIL_FROM: Pixels = px(1280.);
const RAIL_W: Pixels = px(220.);
const INDENT: f32 = 12.;

/// A section the navigator lists, from 1, and its files: none once reviewed, as its diffs fold.
pub(super) struct NavItem {
    pub n: usize,
    pub done: bool,
    pub files: Vec<String>,
}

impl GuideView {
    /// The sections shown (the filter's).
    pub(super) fn nav_items(&self) -> Vec<NavItem> {
        let Some(g) = &self.guide else { return vec![] };
        g.sections
            .iter()
            .enumerate()
            .filter(|(_, s)| self.fits(s))
            .map(|(i, _)| {
                let done = self.is_done(i);
                let files = match (&self.placed, done) {
                    (Some(p), false) => p.shown.get(i).cloned().unwrap_or_default(),
                    _ => vec![],
                };
                NavItem {
                    n: i + 1,
                    done,
                    files,
                }
            })
            .collect()
    }

    /// A section's category, number, title and importance, checked once reviewed.
    fn nav_button(
        &self,
        n: usize,
        s: &Section,
        done: bool,
        chip: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.app_theme().clone();
        let title = title_text(s);
        let tip: SharedString = format!(
            "{}: {title}. {} importance{}",
            s.category.label(),
            s.importance.label(),
            if done { " (reviewed)" } else { "" }
        )
        .into();
        div()
            .id(SharedString::from(format!("nav-{chip}-{n}")))
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .text_size(rems(0.75))
            .cursor_pointer()
            .hover(|d| d.bg(t.hover))
            .when(self.current == Some(n), |d| d.bg(t.active))
            .when(done, |d| d.text_color(t.muted_foreground))
            .map(|d| {
                if chip {
                    d.flex_none()
                        .h(px(24.))
                        .px(px(8.))
                        .rounded_full()
                        .border_1()
                        .border_color(t.border)
                } else {
                    d.flex_1().px(px(6.)).py(px(4.)).rounded(px(4.))
                }
            })
            .child(icon(
                category_icon(s.category),
                category_tone(s.category, &t),
            ))
            .child(
                div()
                    .flex_none()
                    .font_family(t.fonts.code.clone())
                    .text_size(rems(0.68))
                    .text_color(t.subtle)
                    .child(section_badge(n)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .when(chip, |d| d.max_w(px(160.)))
                    .child(title),
            )
            .child(icon(
                importance_icon(s.importance),
                importance_tone(s.importance, &t),
            ))
            .when(done, |d| d.child(icon(IconName::Check, t.added)))
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.go_to(n, cx)))
            .into_any_element()
    }

    /// Section `n`'s files as folders and files; a file brings its diff to the top.
    fn nav_files(&self, n: usize, paths: &[String], cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = cx.app_theme().clone();
        file_tree(paths)
            .into_iter()
            .enumerate()
            .map(|(j, row)| {
                let depth = match &row {
                    NavRow::Folder { depth, .. } | NavRow::File { depth, .. } => depth + 1,
                };
                let guides = (0..depth).map(|i| {
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(14. + i as f32 * INDENT))
                        .w(px(1.))
                        .bg(t.border)
                });
                let line = div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(24.))
                    .pl(px(8. + depth as f32 * INDENT))
                    .pr(px(8.))
                    .text_size(rems(0.75))
                    .children(guides);
                match row {
                    NavRow::Folder { path, label, .. } => {
                        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                        line.text_color(t.muted_foreground)
                            .child(gitviber_ui::widgets::FileIcon::folder(&name, true))
                            .child(div().min_w_0().truncate().child(label))
                            .into_any_element()
                    }
                    NavRow::File { path, name, .. } => {
                        let tip: SharedString = path.clone().into();
                        line.id(SharedString::from(format!("nav-file-{n}-{j}")))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|d| d.bg(t.hover))
                            .child(gitviber_ui::widgets::FileIcon::file(&path))
                            .child(div().min_w_0().truncate().child(name))
                            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.go_to_file(&path, window, cx)
                            }))
                            .into_any_element()
                    }
                }
            })
            .collect()
    }

    pub(super) fn nav_rail(
        &self,
        items: &[NavItem],
        guide: &Guide,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.app_theme().clone();
        let mut col = div()
            .id("guide-nav")
            .w(RAIL_W)
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(t.border)
            .p(px(6.))
            .pb(px(24.));
        for item in items {
            let Some(s) = guide.sections.get(item.n - 1) else {
                continue;
            };
            let open = !self.nav_folded.contains(&s.title);
            let has = !item.files.is_empty();
            let (title, n) = (s.title.clone(), item.n);
            let chevron = div()
                .id(SharedString::from(format!("nav-fold-{}", item.n)))
                .size(px(20.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .when(has, |d| {
                    d.cursor_pointer()
                        .child(
                            Icon::new(if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(px(12.))
                            .text_color(t.subtle),
                        )
                        .tooltip(move |window, cx| {
                            Tooltip::new(format!("Files of section {}", section_badge(n)))
                                .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.nav_folded.remove(&title) {
                                this.nav_folded.insert(title.clone());
                            }
                            cx.notify();
                        }))
                });
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .child(chevron)
                    .child(self.nav_button(item.n, s, item.done, false, cx)),
            );
            if open && has {
                col = col.children(self.nav_files(item.n, &item.files, cx));
            }
        }
        col.into_any_element()
    }

    /// Pinned over the guide; sideways when there are more than fit.
    pub(super) fn nav_chips(
        &self,
        items: &[NavItem],
        guide: &Guide,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.app_theme().clone();
        let chips: Vec<AnyElement> = items
            .iter()
            .filter_map(|i| {
                let s = guide.sections.get(i.n - 1)?;
                Some(self.nav_button(i.n, s, i.done, true, cx))
            })
            .collect();
        div()
            .id("guide-nav-chips")
            .flex_none()
            .h(px(36.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(6.))
            .overflow_x_scroll()
            .border_b_1()
            .border_color(t.border)
            .children(chips)
            .into_any_element()
    }
}
