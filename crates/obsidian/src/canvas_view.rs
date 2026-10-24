//! A JSON Canvas (Obsidian's .canvas) as its board, read-only: groups, arrows and the dotted
//! backdrop painted for the area on screen, cards drawn only where they're visible. Text cards
//! render as notes do, file cards show the file as an embed would, link cards open in the
//! browser. Drag or scroll to move around, pinch or ⌘-scroll to zoom, double-click the
//! background to fit. Zoomed far out, cards are painted as plain boxes.

use std::cell::Cell;
use std::collections::HashMap;
use std::panic::Location;
use std::rc::Rc;
use std::sync::Arc;

use gitviber_markdown::{BoundedImageCache, MarkdownEvent, MarkdownView};
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::media;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, BorderStyle, Bounds, ClipboardItem, ContentMask, Context,
    Corners, Edges, Element, ElementId, Entity, FocusHandle, GlobalElementId, Hsla,
    InspectorElementId, InteractiveElement as _, IntoElement, KeyDownEvent, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement as _, PathBuilder, PinchEvent, Pixels, Render,
    ScrollWheelEvent, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    WeakEntity, Window, canvas, div, image_cache, point, prelude::FluentBuilder as _, px, quad,
    size,
};

use crate::canvas::{self as model, Canvas, NodeKind, Rect, canvas_color, edge_path, fit_box};
use crate::links::LinkIndex;
use crate::markup;
use crate::note::{EmbedView, NoteCx, follow};
use crate::vaults::{Vaults, VaultsEvent};

const PAD: f64 = 40.;
const MIN_SCALE: f64 = 0.05;
const MAX_SCALE: f64 = 4.;
/// Fitting never goes below this: two cards far apart would be dots; the rest is a pan away.
const FIT_MIN: f64 = 0.1;
/// Below this zoom cards are painted boxes: their text would be unreadable anyway.
const DETAIL_SCALE: f64 = 0.35;
/// Cards' views kept built out of sight; older ones are built again when they show.
const MAX_CARDS: usize = 64;
/// New cards' views built per frame; the rest come on the next ones.
const BUILD_PER_FRAME: usize = 24;

/// Where the canvas is looked at: its point (0, 0) on screen at (x, y), drawn `scale` times its size.
#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    x: f64,
    y: f64,
    scale: f64,
}

fn fitted(b: Rect, w: f64, h: f64) -> View {
    if w <= 0. || h <= 0. || b.width <= 0. {
        return View {
            x: w / 2.,
            y: h / 2.,
            scale: 1.,
        };
    }
    let scale = ((w - 2. * PAD) / b.width)
        .min((h - 2. * PAD) / b.height)
        .clamp(FIT_MIN, 1.);
    View {
        scale,
        x: (w - b.width * scale) / 2. - b.x * scale,
        y: (h - b.height * scale) / 2. - b.y * scale,
    }
}

enum Card {
    Text {
        view: Entity<MarkdownView>,
        markdown: String,
        _sub: Subscription,
    },
    File(Entity<EmbedView>),
}

pub struct CanvasView {
    note: NoteCx,
    source: String,
    canvas: Result<Rc<Canvas>, String>,
    fit: Rect,
    /// None: fitted, following the pane's size until the user moves.
    view: Option<View>,
    /// Where the board was laid out last, for fitting and zooming about its middle.
    viewport: Rc<Cell<Bounds<Pixels>>>,
    drag: Option<gpui_kit::Point<Pixels>>,
    index: Option<Arc<LinkIndex>>,
    cards: HashMap<usize, (Card, u64)>,
    frame: u64,
    images: Entity<BoundedImageCache>,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl CanvasView {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        vault: String,
        path: String,
        source: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let model = Vaults::global(cx);
        let index = model.update(cx, |m, cx| m.index(&vault, cx));
        let v = vault.clone();
        let sub = cx.subscribe(
            &model,
            move |this: &mut Self, model, e: &VaultsEvent, cx| {
                if *e == VaultsEvent::IndexChanged(v.clone()) {
                    this.index = model.update(cx, |m, cx| m.index(&v, cx));
                    this.reindex(cx);
                    cx.notify();
                }
            },
        );
        let note = NoteCx {
            trail: vec![format!("{path}#")],
            vault,
            path,
            // A canvas's cards are already one level in.
            depth: 1,
            workspace,
            top: None,
        };
        let mut this = CanvasView {
            note,
            source: String::new(),
            canvas: Ok(Rc::default()),
            fit: Rect::default(),
            view: None,
            viewport: Rc::new(Cell::new(Bounds::default())),
            drag: None,
            index,
            cards: HashMap::new(),
            frame: 0,
            images: BoundedImageCache::new(BoundedImageCache::display_side(window), cx),
            focus: cx.focus_handle(),
            _subs: vec![sub],
            _live: Live::new("CanvasView"),
        };
        this.set_source(source, cx);
        this
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The canvas changed on disk: its cards are built again; where it's looked at stays.
    pub fn set_source(&mut self, source: String, cx: &mut Context<Self>) {
        if source == self.source && !self.source.is_empty() {
            return;
        }
        self.canvas = model::parse_canvas(&source).map(Rc::new);
        let rects: Vec<Rect> = match &self.canvas {
            Ok(c) => c.nodes.iter().map(|n| n.rect).collect(),
            Err(_) => vec![],
        };
        self.fit = fit_box(&rects);
        self.source = source;
        self.cards.clear();
        cx.notify();
    }

    fn reindex(&mut self, cx: &mut App) {
        let (Some(index), Ok(canvas)) = (self.index.clone(), &self.canvas) else {
            return;
        };
        for (ix, (card, _)) in self.cards.iter_mut() {
            if let Card::Text { view, markdown, .. } = card {
                let text = canvas.nodes[*ix].text.as_deref().unwrap_or("");
                let md = card_markdown(text, &index, &self.note.path);
                if md != *markdown {
                    view.update(cx, |v, cx| v.set_source(&md, cx));
                    *markdown = md;
                }
            }
        }
    }

    fn size(&self, window: &Window) -> (f64, f64) {
        let b = self.viewport.get();
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        if w > 0. && h > 0. {
            return (w as f64, h as f64);
        }
        let s = window.viewport_size();
        (f32::from(s.width) as f64, f32::from(s.height) as f64)
    }

    fn current(&self, window: &Window) -> View {
        let (w, h) = self.size(window);
        self.view.unwrap_or_else(|| fitted(self.fit, w, h))
    }

    /// Zooms by `factor`, the canvas point under (`px`, `py`) (on the board) staying there.
    fn zoom_at(&mut self, factor: f64, px: f64, py: f64, window: &Window, cx: &mut Context<Self>) {
        let v = self.current(window);
        let scale = (v.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        self.view = Some(View {
            scale,
            x: px - (px - v.x) * scale / v.scale,
            y: py - (py - v.y) * scale / v.scale,
        });
        cx.notify();
    }

    /// Zooms by `factor` about the board's middle.
    pub fn zoom_by(&mut self, factor: f64, window: &Window, cx: &mut Context<Self>) {
        self.zoom_middle(factor, window, cx);
    }

    fn zoom_middle(&mut self, factor: f64, window: &Window, cx: &mut Context<Self>) {
        let (w, h) = self.size(window);
        self.zoom_at(factor, w / 2., h / 2., window, cx);
    }

    fn pan_by(&mut self, dx: f64, dy: f64, window: &Window, cx: &mut Context<Self>) {
        let v = self.current(window);
        self.view = Some(View {
            x: v.x + dx,
            y: v.y + dy,
            ..v
        });
        cx.notify();
    }

    fn fit_now(&mut self, cx: &mut Context<Self>) {
        self.view = None;
        cx.notify();
    }

    fn local(&self, p: gpui_kit::Point<Pixels>) -> (f64, f64) {
        let o = self.viewport.get().origin;
        (f32::from(p.x - o.x) as f64, f32::from(p.y - o.y) as f64)
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = &e.keystroke.modifiers;
        if m.platform || m.control || m.alt {
            return;
        }
        let step = 64.;
        let (dx, dy) = match e.keystroke.key.as_str() {
            "left" => (step, 0.),
            "right" => (-step, 0.),
            "up" => (0., step),
            "down" => (0., -step),
            _ => return,
        };
        self.pan_by(dx, dy, window, cx);
        cx.stop_propagation();
    }

    /// The card's view at `ix`, built if it isn't (at most BUILD_PER_FRAME new ones a frame).
    fn card(
        &mut self,
        ix: usize,
        built_now: &mut usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some((card, stamp)) = self.cards.get_mut(&ix) {
            *stamp = self.frame;
            return Some(match card {
                Card::Text { view, .. } => view.clone().into_any_element(),
                Card::File(view) => view.clone().into_any_element(),
            });
        }
        if *built_now >= BUILD_PER_FRAME {
            // The rest on the next frame.
            cx.notify();
            return None;
        }
        let index = self.index.clone()?;
        let node = self.canvas.as_ref().ok()?.nodes.get(ix)?.clone();
        let card = match node.kind {
            NodeKind::Text => {
                let markdown =
                    card_markdown(node.text.as_deref().unwrap_or(""), &index, &self.note.path);
                let base = self.note_base();
                let view = cx.new(|cx| MarkdownView::inline(&markdown, base, window, cx));
                let note = self.note.clone();
                let sub =
                    cx.subscribe_in(&view, window, move |_, _, e: &MarkdownEvent, window, cx| {
                        let MarkdownEvent::OpenFile { path, .. } = e;
                        follow(&note, path, window, cx);
                    });
                Card::Text {
                    view,
                    markdown,
                    _sub: sub,
                }
            }
            NodeKind::File => {
                let target = format!(
                    "{}{}",
                    node.file.unwrap_or_default(),
                    node.subpath.unwrap_or_default()
                );
                let note = self.note.clone();
                Card::File(cx.new(|cx| EmbedView::new(note, target, String::new(), true, cx)))
            }
            _ => return None,
        };
        *built_now += 1;
        let element = match &card {
            Card::Text { view, .. } => view.clone().into_any_element(),
            Card::File(view) => view.clone().into_any_element(),
        };
        self.cards.insert(ix, (card, self.frame));
        while self.cards.len() > MAX_CARDS {
            let oldest = self
                .cards
                .iter()
                .filter(|(_, (_, s))| *s < self.frame)
                .min_by_key(|(_, (_, s))| *s)
                .map(|(i, _)| *i);
            match oldest {
                Some(i) => {
                    self.cards.remove(&i);
                }
                None => break,
            }
        }
        Some(element)
    }

    fn note_base(&self) -> gitviber_markdown::MarkdownBase {
        gitviber_markdown::MarkdownBase {
            root: Some(std::path::PathBuf::from(&self.note.vault)),
            file: Some(self.note.path.clone()),
            callouts: gitviber_markdown::Callouts::Obsidian,
        }
    }

    #[cfg(test)]
    pub(crate) fn built_cards(&self) -> usize {
        self.cards.len()
    }
}

fn card_markdown(text: &str, index: &LinkIndex, canvas: &str) -> String {
    let text = markup::strip_comments(text);
    let cx = markup::Context {
        index,
        note: canvas,
    };
    markup::to_markdown("", &text, &cx)
}

/// What the paint layer draws, worked out in render: groups, arrows, far-out cards, the dots.
struct Paint {
    dots: Option<(f32, f32, f32)>,
    dot: Hsla,
    boxes: Vec<(Bounds<Pixels>, f32, Hsla, Hsla)>,
    edges: Vec<(model::EdgePath, Hsla, bool, bool)>,
    view: View,
    viewport: Rc<Cell<Bounds<Pixels>>>,
}

fn paint_layer(p: Paint, bounds: Bounds<Pixels>, window: &mut Window) {
    let o = bounds.origin;
    let at = |x: f64, y: f64| {
        point(
            o.x + px((p.view.x + x * p.view.scale) as f32),
            o.y + px((p.view.y + y * p.view.scale) as f32),
        )
    };
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        if let Some((sx, sy, step)) = p.dots {
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let mut y = sy;
            while y < h {
                let mut x = sx;
                while x < w {
                    let b = Bounds::new(
                        point(o.x + px(x - 1.), o.y + px(y - 1.)),
                        size(px(2.), px(2.)),
                    );
                    window.paint_quad(quad(
                        b,
                        Corners::all(px(1.)),
                        p.dot,
                        Edges::all(px(0.)),
                        p.dot,
                        BorderStyle::Solid,
                    ));
                    x += step;
                }
                y += step;
            }
        }
        for (b, radius, fill, border) in &p.boxes {
            let b = Bounds::new(o + b.origin, b.size);
            window.paint_quad(quad(
                b,
                Corners::all(px(*radius)),
                *fill,
                Edges::all(px(2.)),
                *border,
                BorderStyle::Solid,
            ));
        }
        let width = px((2.5 * p.view.scale).max(1.) as f32);
        for (e, color, from_arrow, to_arrow) in &p.edges {
            let mut path = PathBuilder::stroke(width);
            path.move_to(at(e.start.0, e.start.1));
            path.cubic_bezier_to(at(e.end.0, e.end.1), at(e.c1.0, e.c1.1), at(e.c2.0, e.c2.1));
            if let Ok(path) = path.build() {
                window.paint_path(path, *color);
            }
            let arrows = [
                (*to_arrow, e.end, e.end_angle),
                (*from_arrow, e.start, e.start_angle),
            ];
            for (on, tip, angle) in arrows {
                if !on {
                    continue;
                }
                // In canvas units: the head grows with the zoom, as the line does.
                let (bx, by) = (tip.0 - angle.cos() * 14., tip.1 - angle.sin() * 14.);
                let (nx, ny) = (-angle.sin() * 7., angle.cos() * 7.);
                let mut head = PathBuilder::fill();
                head.move_to(at(tip.0, tip.1));
                head.line_to(at(bx + nx, by + ny));
                head.line_to(at(bx - nx, by - ny));
                head.close();
                if let Ok(head) = head.build() {
                    window.paint_path(head, *color);
                }
            }
        }
    });
}

impl Render for CanvasView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        self.frame += 1;
        let board = div()
            .id("canvas-board")
            .key_context("ImageView")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(t.background);
        let data = match &self.canvas {
            Err(e) => {
                return board
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(t.muted_foreground)
                            .child("This canvas can't be read"),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(t.subtle)
                            .child(format!("{e}. Switch to Code to see it.")),
                    )
                    .into_any_element();
            }
            Ok(c) => Rc::clone(c),
        };
        let v = self.current(window);
        let (w, h) = self.size(window);
        let s = v.scale;
        let margin = 40. / s;
        let visible = Rect {
            x: -v.x / s - margin,
            y: -v.y / s - margin,
            width: w / s + 2. * margin,
            height: h / s + 2. * margin,
        };
        let screen = |r: &Rect| {
            Bounds::new(
                point(px((v.x + r.x * s) as f32), px((v.y + r.y * s) as f32)),
                size(px((r.width * s) as f32), px((r.height * s) as f32)),
            )
        };
        let detail = s >= DETAIL_SCALE;
        let mut boxes = Vec::new();
        let mut labels: Vec<AnyElement> = Vec::new();
        let mut cards: Vec<AnyElement> = Vec::new();
        let mut built_now = 0;
        for (ix, node) in data.nodes.iter().enumerate() {
            if !node.rect.intersects(&visible) {
                continue;
            }
            let color = canvas_color(node.color.as_deref(), t.dark);
            let border = color.unwrap_or(t.border_strong);
            let b = screen(&node.rect);
            let radius = (12. * s) as f32;
            if node.kind == NodeKind::Group {
                let fill = color.map_or(t.foreground.opacity(0.03), |c| c.opacity(0.06));
                boxes.push((b, radius, fill, border));
                if detail && let Some(label) = node.label.clone() {
                    labels.push(
                        div()
                            .absolute()
                            .left(b.origin.x)
                            .top(b.origin.y - px((30. * s) as f32))
                            .max_w(b.size.width)
                            .h(px((26. * s) as f32))
                            .flex()
                            .items_center()
                            .truncate()
                            .text_size(px((15. * s) as f32))
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(color.unwrap_or(t.muted_foreground))
                            .child(label)
                            .into_any_element(),
                    );
                }
                continue;
            }
            if !detail {
                let fill = color.map_or(t.panel, |c| c.opacity(0.25));
                boxes.push((b, radius, fill, border));
                continue;
            }
            let content = match node.kind {
                NodeKind::Link => {
                    let url = node.url.clone().unwrap_or_default();
                    Some(link_card(url, s, &t, &self.note.workspace))
                }
                _ => self.card(ix, &mut built_now, window, cx),
            };
            let pad = (12. * s) as f32;
            cards.push(
                div()
                    .absolute()
                    .left(b.origin.x)
                    .top(b.origin.y)
                    .w(b.size.width)
                    .h(b.size.height)
                    .rounded(px(radius))
                    .border(px(2.))
                    .border_color(border)
                    .bg(t.panel)
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .size_full()
                            .overflow_hidden()
                            .when_some(color, |d, c| d.bg(c.opacity(0.08)))
                            .px(px(pad * 4. / 3.))
                            .py(px(pad))
                            .text_size(px((13. * s) as f32))
                            .children(
                                content.map(|c| Zoomed::new(c, window.rem_size() * s as f32)),
                            ),
                    )
                    .into_any_element(),
            );
        }
        let mut edges = Vec::new();
        let mut edge_labels: Vec<AnyElement> = Vec::new();
        for e in &data.edges {
            let (a, b) = (&data.nodes[e.from].rect, &data.nodes[e.to].rect);
            let path = edge_path(a, b, e.from_side, e.to_side);
            if !path.bounds().intersects(&visible) {
                continue;
            }
            let color = canvas_color(e.color.as_deref(), t.dark).unwrap_or(t.subtle);
            edges.push((path, color, e.from_arrow, e.to_arrow));
            if detail && let Some(label) = e.label.clone() {
                let (lw, lh) = (240. * s, 40. * s);
                edge_labels.push(
                    div()
                        .absolute()
                        .left(px((v.x + path.mid.0 * s - lw / 2.) as f32))
                        .top(px((v.y + path.mid.1 * s - lh / 2.) as f32))
                        .w(px(lw as f32))
                        .h(px(lh as f32))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .max_w_full()
                                .px(px((8. * s) as f32))
                                .py(px((2. * s) as f32))
                                .rounded(px((6. * s) as f32))
                                .border_1()
                                .border_color(t.border)
                                .bg(t.background)
                                .text_size(px((13. * s) as f32))
                                .text_color(t.muted_foreground)
                                .truncate()
                                .child(label),
                        )
                        .into_any_element(),
                );
            }
        }
        let step = {
            let mut step = 24. * s;
            while step < 12. {
                step *= 2.;
            }
            step as f32
        };
        let dots = Some((
            (v.x as f32).rem_euclid(step),
            (v.y as f32).rem_euclid(step),
            step,
        ));
        let paint = Paint {
            dots,
            dot: t.border_strong,
            boxes,
            edges,
            view: v,
            viewport: self.viewport.clone(),
        };
        let empty = data.nodes.is_empty();
        let zoom = format!("{}%", (s * 100.).round());
        board
            .on_key_down(cx.listener(Self::on_key))
            .on_action(
                cx.listener(|this, _: &media::ZoomIn, window, cx| {
                    this.zoom_middle(1.25, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &media::ZoomOut, window, cx| {
                    this.zoom_middle(0.8, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &media::Fit, _, cx| this.fit_now(cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    if e.click_count >= 2 {
                        this.drag = None;
                        return this.fit_now(cx);
                    }
                    this.drag = Some(e.position);
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, window, cx| {
                let Some(from) = this.drag else { return };
                if e.pressed_button != Some(MouseButton::Left) {
                    this.drag = None;
                    return;
                }
                this.drag = Some(e.position);
                let d = e.position - from;
                this.pan_by(f32::from(d.x) as f64, f32::from(d.y) as f64, window, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, window, cx| {
                let d = e.delta.pixel_delta(px(16.));
                let (dx, dy) = (f32::from(d.x) as f64, f32::from(d.y) as f64);
                if e.modifiers.platform || e.modifiers.control {
                    let (x, y) = this.local(e.position);
                    this.zoom_at((dy * 0.004).exp(), x, y, window, cx);
                } else {
                    this.pan_by(dx, dy, window, cx);
                }
            }))
            .on_pinch(cx.listener(|this, e: &PinchEvent, window, cx| {
                let (x, y) = this.local(e.position);
                this.zoom_at((1. + e.delta as f64).max(0.1), x, y, window, cx);
            }))
            .child(
                canvas(
                    move |bounds, window, _| {
                        let was = paint.viewport.replace(bounds);
                        // The first frame guessed the board's size; draw again with the real one.
                        if was != bounds {
                            window.request_animation_frame();
                        }
                        paint
                    },
                    |bounds, paint, window, _| paint_layer(paint, bounds, window),
                )
                .absolute()
                .inset_0(),
            )
            .child(
                image_cache(self.images.clone())
                    .absolute()
                    .inset_0()
                    .children(labels)
                    .children(edge_labels)
                    .children(cards),
            )
            .when(empty, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.5))
                        .text_color(t.muted_foreground)
                        .child("This canvas is empty"),
                )
            })
            .child(
                div()
                    .absolute()
                    .right_3()
                    .bottom_3()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .p_0p5()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.elevated)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        Button::new("zoom-out")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Minus)
                            .tooltip("Zoom out")
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.zoom_middle(0.8, window, cx)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(40.))
                            .text_center()
                            .font_family(t.fonts.code.clone())
                            .text_size(px(11.))
                            .text_color(t.subtle)
                            .child(SharedString::from(zoom)),
                    )
                    .child(
                        Button::new("zoom-in")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("Zoom in")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.zoom_middle(1.25, window, cx)
                            })),
                    )
                    .child(
                        Button::new("zoom-fit")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Maximize)
                            .tooltip("Fit")
                            .on_click(cx.listener(|this, _, _, cx| this.fit_now(cx))),
                    ),
            )
            .into_any_element()
    }
}

/// A link card: the page's address, opened in the browser (only the web; anything else is copied).
fn link_card(
    url: String,
    s: f64,
    t: &gitviber_ui::AppTheme,
    workspace: &WeakEntity<Workspace>,
) -> AnyElement {
    let ws = workspace.clone();
    let web = url.starts_with("https://") || url.starts_with("http://");
    let u = url.clone();
    div()
        .id(ElementId::Name(format!("canvas-link-{url}").into()))
        .flex()
        .items_center()
        .gap(px((8. * s) as f32))
        .cursor_pointer()
        .text_color(t.primary)
        .child(Icon::new(IconName::Globe).size(px((16. * s) as f32)))
        .child(div().min_w_0().truncate().child(url))
        .on_click(move |_, _, cx| {
            if web {
                cx.open_url(&u);
            } else {
                cx.write_to_clipboard(ClipboardItem::new_string(u.clone()));
                _ = ws.update(cx, |ws, cx| {
                    ws.toast(
                        Toast::info("Link copied: it can't be opened from here."),
                        cx,
                    )
                });
            }
        })
        .into_any_element()
}

/// Its child laid out and drawn with another rem size: a card's text at the canvas's zoom.
struct Zoomed {
    child: AnyElement,
    rem: Pixels,
}

impl Zoomed {
    fn new(child: AnyElement, rem: Pixels) -> Self {
        Zoomed { child, rem }
    }
}

impl IntoElement for Zoomed {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Zoomed {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let child = &mut self.child;
        (
            window.with_rem_size(Some(self.rem), |window| child.request_layout(window, cx)),
            (),
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| {
            child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| child.paint(window, cx));
    }
}
