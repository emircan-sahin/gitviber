//! A guide's diagram: the data it changes as cards of their fields, then its flows as boxes and
//! arrows, laid out natively (model::layout). A part with a section links to it.

use std::rc::Rc;

use gitviber_ui::AppTheme;
use gpui_kit::{
    AnyElement, App, Bounds, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    PathBuilder, Pixels, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    canvas, div, point, prelude::FluentBuilder as _, px, rems,
};

use crate::model::guide::{Flow, Model, PartStatus, section_badge};
use crate::model::layout::{Sizes, layout};

/// Scrolls the guide to section `n` (from 1).
pub type OnSection = Rc<dyn Fn(usize, &mut Window, &mut App)>;

const SIZES: Sizes = Sizes {
    box_w: 188.,
    box_h: 46.,
    gap_x: 28.,
    gap_y: 30.,
};

fn mark(status: PartStatus, t: &AppTheme) -> AnyElement {
    let (sign, color) = match status {
        PartStatus::New => ("+", t.added),
        PartStatus::Changed => ("~", t.modified),
        PartStatus::Same => ("", t.subtle),
    };
    div()
        .w(px(10.))
        .flex_none()
        .text_center()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(color)
        .child(sign)
        .into_any_element()
}

fn badge(id: SharedString, n: Option<usize>, t: &AppTheme, go: &OnSection) -> Option<AnyElement> {
    let n = n?;
    let go = go.clone();
    Some(
        div()
            .id(id)
            .ml_auto()
            .flex_none()
            .px(px(4.))
            .rounded(px(3.))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.elevated)
            .font_family(t.fonts.code.clone())
            .text_size(rems(0.62))
            .text_color(t.muted_foreground)
            .cursor_pointer()
            .hover(|s| s.text_color(t.foreground))
            .child(section_badge(n))
            .on_click(move |_, window, cx| go(n, window, cx))
            .into_any_element(),
    )
}

fn model_card(i: usize, m: &Model, t: &AppTheme, go: &OnSection) -> AnyElement {
    let file = m.file.rsplit('/').next().unwrap_or(&m.file).to_string();
    let mut card = div()
        .min_w(px(200.))
        .flex_1()
        .rounded(px(6.))
        .border_1()
        .border_color(if m.status == PartStatus::New {
            t.added.opacity(0.5)
        } else {
            t.border
        })
        .bg(t.panel)
        .text_size(rems(0.75))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .py(px(6.))
                .border_b_1()
                .border_color(t.border)
                .child(mark(m.status, t))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(t.fonts.code.clone())
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child(m.name.clone()),
                )
                .when(!file.is_empty(), |d| {
                    d.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.68))
                            .text_color(t.subtle)
                            .child(file),
                    )
                })
                .children(badge(format!("model-{i}").into(), m.section, t, go)),
        );
    if !m.note.is_empty() {
        card = card.child(
            div()
                .px(px(8.))
                .py(px(4.))
                .border_b_1()
                .border_color(t.border)
                .text_size(rems(0.72))
                .text_color(t.muted_foreground)
                .child(m.note.clone()),
        );
    }
    if !m.fields.is_empty() {
        card = card.child(
            div()
                .py(px(4.))
                .children(m.fields.iter().enumerate().map(|(j, f)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(8.))
                        .py(px(2.))
                        .when(f.status == PartStatus::New, |d| d.bg(t.add_bg))
                        .child(mark(f.status, t))
                        .child(
                            div()
                                .flex_none()
                                .font_family(t.fonts.code.clone())
                                .child(f.name.clone()),
                        )
                        .when(!f.note.is_empty(), |d| {
                            d.child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(rems(0.72))
                                    .text_color(t.subtle)
                                    .child(f.note.clone()),
                            )
                        })
                        .children(badge(format!("field-{i}-{j}").into(), f.section, t, go))
                })),
        );
    }
    card.into_any_element()
}

fn status_color(s: PartStatus, t: &AppTheme) -> Hsla {
    match s {
        PartStatus::New => t.added,
        PartStatus::Changed => t.modified,
        PartStatus::Same => t.border_strong,
    }
}

fn flow_view(fi: usize, flow: &Flow, t: &AppTheme, go: &OnSection) -> AnyElement {
    let laid = layout(flow, SIZES);
    let line = t.muted_foreground.opacity(0.7);
    let curves = laid.curves.clone();
    let lines = canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let at = |p: (f32, f32)| point(bounds.origin.x + px(p.0), bounds.origin.y + px(p.1));
            for c in &curves {
                let mut path = PathBuilder::stroke(px(1.25));
                path.move_to(at(c.start));
                path.cubic_bezier_to(at(c.end), at(c.c1), at(c.c2));
                if let Ok(p) = path.build() {
                    window.paint_path(p, line);
                }
                // The arrowhead, along the curve's last stretch.
                let (dx, dy) = (c.end.0 - c.c2.0, c.end.1 - c.c2.1);
                let len = (dx * dx + dy * dy).sqrt().max(0.001);
                let (ux, uy) = (dx / len, dy / len);
                let back = (c.end.0 - ux * 7., c.end.1 - uy * 7.);
                let mut head = PathBuilder::fill();
                head.move_to(at(c.end));
                head.line_to(at((back.0 - uy * 4., back.1 + ux * 4.)));
                head.line_to(at((back.0 + uy * 4., back.1 - ux * 4.)));
                head.close();
                if let Ok(p) = head.build() {
                    window.paint_path(p, line);
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let boxes = laid.boxes.iter().map(|b| {
        let step = &flow.steps[b.step];
        let file = step
            .file
            .rsplit('/')
            .next()
            .unwrap_or(&step.file)
            .to_string();
        let what = if step.decision {
            "Decision".to_string()
        } else {
            step.status.word().to_string()
        };
        let meta = [
            step.section.map(section_badge).unwrap_or_default(),
            what,
            file,
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        let section = step.section;
        let go = go.clone();
        div()
            .id(SharedString::from(format!("step-{fi}-{}", b.step)))
            .absolute()
            .left(px(b.x))
            .top(px(b.y))
            .w(px(SIZES.box_w))
            .h(px(SIZES.box_h))
            .px(px(8.))
            .flex()
            .flex_col()
            .justify_center()
            .rounded(if step.decision {
                px(SIZES.box_h / 2.)
            } else {
                px(6.)
            })
            .border_1()
            .border_color(status_color(step.status, t))
            .bg(t.panel)
            .when(section.is_some(), |d| {
                d.cursor_pointer()
                    .hover(|s| s.bg(t.hover))
                    .on_click(move |_, window, cx| {
                        if let Some(n) = section {
                            go(n, window, cx)
                        }
                    })
            })
            .child(
                div()
                    .truncate()
                    .text_size(rems(0.62))
                    .text_color(t.subtle)
                    .child(meta),
            )
            .child(
                div()
                    .truncate()
                    .text_size(rems(0.72))
                    .when(!step.decision, |d| d.font_family(t.fonts.code.clone()))
                    .when(step.decision, |d| {
                        d.font_weight(gpui_kit::FontWeight::MEDIUM)
                    })
                    .child(step.label.clone()),
            )
    });
    let labels = laid.curves.iter().filter_map(|c| {
        let label = &flow.edges[c.edge].label;
        (!label.is_empty()).then(|| {
            div()
                .absolute()
                .left(px(c.label_at.0 - 18.))
                .top(px(c.label_at.1 - 8.))
                .w(px(36.))
                .flex()
                .justify_center()
                .child(
                    div()
                        .px(px(4.))
                        .rounded(px(3.))
                        .bg(t.background)
                        .text_size(rems(0.62))
                        .text_color(t.muted_foreground)
                        .child(label.clone()),
                )
        })
    });
    let title = if flow.title.is_empty() {
        "Flow".to_string()
    } else {
        flow.title.clone()
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .text_size(rems(0.75))
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .text_color(t.muted_foreground)
                .child(title),
        )
        .child(
            div()
                .id(SharedString::from(format!("flow-{fi}")))
                .overflow_x_scroll()
                .rounded(px(6.))
                .border_1()
                .border_color(t.border)
                .p(px(12.))
                .child(
                    div()
                        .relative()
                        .w(px(laid.width))
                        .h(px(laid.height))
                        .child(lines)
                        .children(boxes)
                        .children(labels),
                ),
        )
        .into_any_element()
}

/// The data cards, then the flows; a guide from before structured diagrams has its Mermaid
/// drawn (`legacy`, a Markdown view).
pub fn diagram(
    models: &[Model],
    flows: &[Flow],
    legacy: Option<AnyElement>,
    t: &AppTheme,
    go: &OnSection,
) -> AnyElement {
    let mut col = div().flex().flex_col().gap(px(14.));
    if !models.is_empty() {
        col = col.child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(rems(0.75))
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .text_color(t.muted_foreground)
                        .child("Data"),
                )
                .child(
                    div().flex().flex_wrap().gap(px(8.)).children(
                        models
                            .iter()
                            .enumerate()
                            .map(|(i, m)| model_card(i, m, t, go)),
                    ),
                ),
        );
    }
    for (i, f) in flows.iter().enumerate() {
        col = col.child(flow_view(i, f, t, go));
    }
    col.children(legacy).into_any_element()
}
