//! JSON Canvas (jsoncanvas.org, Obsidian's .canvas files): parsed defensively, and the geometry
//! to draw it. Coordinates stay f64 as written: a card dragged to x = 1e9 is still a number.

use std::collections::{HashMap, HashSet};

use gpui_kit::{Hsla, Rgba};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    fn parse(v: Option<&Value>) -> Option<Side> {
        Some(match v?.as_str()? {
            "top" => Side::Top,
            "right" => Side::Right,
            "bottom" => Side::Bottom,
            "left" => Side::Left,
            _ => return None,
        })
    }

    /// The direction straight out of this side.
    fn normal(self) -> (f64, f64) {
        match self {
            Side::Top => (0., -1.),
            Side::Right => (1., 0.),
            Side::Bottom => (0., 1.),
            Side::Left => (-1., 0.),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Text,
    File,
    Link,
    Group,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanvasNode {
    pub id: String,
    pub kind: NodeKind,
    pub rect: Rect,
    pub color: Option<String>,
    pub text: Option<String>,
    pub file: Option<String>,
    /// A file node's #heading or #^block.
    pub subpath: Option<String>,
    pub url: Option<String>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanvasEdge {
    pub id: String,
    /// Indexes into `Canvas::nodes`.
    pub from: usize,
    pub to: usize,
    pub from_side: Option<Side>,
    pub to_side: Option<Side>,
    pub from_arrow: bool,
    pub to_arrow: bool,
    pub color: Option<String>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Canvas {
    /// Groups first: later nodes draw over earlier ones, and a group sits behind what's in it.
    pub nodes: Vec<CanvasNode>,
    pub edges: Vec<CanvasEdge>,
}

fn string(o: &Map<String, Value>, key: &str) -> Option<String> {
    o.get(key)?.as_str().map(str::to_string)
}

fn number(o: &Map<String, Value>, key: &str) -> Option<f64> {
    o.get(key)?.as_f64().filter(|n| n.is_finite())
}

fn objects<'a>(v: Option<&'a Value>) -> impl Iterator<Item = &'a Map<String, Value>> + 'a {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
}

/// The canvas in `text`, leaving out what the spec doesn't allow; an error when it isn't JSON.
pub fn parse_canvas(text: &str) -> Result<Canvas, String> {
    if text.trim().is_empty() {
        return Ok(Canvas::default());
    }
    let data: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let data = data.as_object();
    let mut seen = HashSet::new();
    let mut nodes: Vec<CanvasNode> = objects(data.and_then(|d| d.get("nodes")))
        .filter_map(|n| {
            let id = string(n, "id")?;
            let kind = match n.get("type")?.as_str()? {
                "text" => NodeKind::Text,
                "file" => NodeKind::File,
                "link" => NodeKind::Link,
                "group" => NodeKind::Group,
                _ => return None,
            };
            let (x, y) = (number(n, "x")?, number(n, "y")?);
            let (width, height) = (number(n, "width")?, number(n, "height")?);
            // Ids are unique (a second one with the same id is left out), sizes positive.
            if width <= 0. || height <= 0. || !seen.insert(id.clone()) {
                return None;
            }
            Some(CanvasNode {
                id,
                kind,
                rect: Rect {
                    x,
                    y,
                    width,
                    height,
                },
                color: string(n, "color"),
                text: string(n, "text"),
                file: string(n, "file"),
                subpath: string(n, "subpath"),
                url: string(n, "url"),
                label: string(n, "label"),
            })
        })
        .collect();
    nodes.sort_by_key(|n| n.kind != NodeKind::Group);
    let at: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let edges = objects(data.and_then(|d| d.get("edges")))
        .filter_map(|e| {
            let from = *at.get(e.get("fromNode")?.as_str()?)?;
            let to = *at.get(e.get("toNode")?.as_str()?)?;
            Some(CanvasEdge {
                id: string(e, "id")?,
                from,
                to,
                from_side: Side::parse(e.get("fromSide")),
                to_side: Side::parse(e.get("toSide")),
                from_arrow: e.get("fromEnd").and_then(Value::as_str) == Some("arrow"),
                to_arrow: e.get("toEnd").and_then(Value::as_str) != Some("none"),
                color: string(e, "color"),
                label: string(e, "label"),
            })
        })
        .collect();
    Ok(Canvas { nodes, edges })
}

/// The box around every rect; a 0×0 box for none.
pub fn bounds<'a>(rects: impl IntoIterator<Item = &'a Rect>) -> Rect {
    let (mut x, mut y, mut right, mut bottom) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for r in rects {
        x = x.min(r.x);
        y = y.min(r.y);
        right = right.max(r.right());
        bottom = bottom.max(r.bottom());
    }
    if !x.is_finite() {
        return Rect::default();
    }
    Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs[(xs.len() - 1) / 2]
}

/// The box a canvas is fitted to: around every card, unless a few lie far off from the rest (a
/// card dragged to x = 10,000,000), which would shrink the rest to dots. Those are left out: the
/// cards more than four times as far from the middle as three in four of them are, past a
/// card's size. They stay a pan away.
pub fn fit_box(rects: &[Rect]) -> Rect {
    let all = bounds(rects);
    if rects.len() < 3 {
        return all;
    }
    let centers: Vec<(f64, f64)> = rects
        .iter()
        .map(|r| (r.x + r.width / 2., r.y + r.height / 2.))
        .collect();
    let mx = median(centers.iter().map(|c| c.0).collect());
    let my = median(centers.iter().map(|c| c.1).collect());
    let dist: Vec<f64> = centers
        .iter()
        .map(|(x, y)| (x - mx).hypot(y - my))
        .collect();
    let mut sorted = dist.clone();
    sorted.sort_by(f64::total_cmp);
    let card = rects
        .iter()
        .take(1000)
        .map(|r| r.width.hypot(r.height))
        .fold(0., f64::max);
    let reach = 4. * sorted[((sorted.len() - 1) as f64 * 0.75) as usize] + card;
    let near: Vec<&Rect> = rects
        .iter()
        .zip(&dist)
        .filter(|(_, d)| **d <= reach)
        .map(|(r, _)| r)
        .collect();
    if near.is_empty() { all } else { bounds(near) }
}

/// The middle of a rect's side, where an edge meets it.
pub fn anchor(r: &Rect, side: Side) -> (f64, f64) {
    match side {
        Side::Top => (r.x + r.width / 2., r.y),
        Side::Bottom => (r.x + r.width / 2., r.bottom()),
        Side::Left => (r.x, r.y + r.height / 2.),
        Side::Right => (r.right(), r.y + r.height / 2.),
    }
}

/// The side of `from` that faces `to` best, for an edge that names none.
pub fn facing(from: &Rect, to: &Rect) -> Side {
    let dx = to.x + to.width / 2. - (from.x + from.width / 2.);
    let dy = to.y + to.height / 2. - (from.y + from.height / 2.);
    if dx.abs() > dy.abs() {
        if dx > 0. { Side::Right } else { Side::Left }
    } else if dy > 0. {
        Side::Bottom
    } else {
        Side::Top
    }
}

/// An edge as a cubic curve leaving each node straight out of its side, as Obsidian draws them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgePath {
    pub start: (f64, f64),
    pub c1: (f64, f64),
    pub c2: (f64, f64),
    pub end: (f64, f64),
    /// The curve's point at t = ½, for the label.
    pub mid: (f64, f64),
    /// The direction it arrives in at each end, for the arrows.
    pub start_angle: f64,
    pub end_angle: f64,
}

impl EdgePath {
    /// The box around the curve (its control points hold it).
    pub fn bounds(&self) -> Rect {
        let xs = [self.start.0, self.c1.0, self.c2.0, self.end.0];
        let ys = [self.start.1, self.c1.1, self.c2.1, self.end.1];
        let (x, y) = (
            xs.iter().copied().fold(f64::INFINITY, f64::min),
            ys.iter().copied().fold(f64::INFINITY, f64::min),
        );
        Rect {
            x,
            y,
            width: xs.iter().copied().fold(f64::NEG_INFINITY, f64::max) - x,
            height: ys.iter().copied().fold(f64::NEG_INFINITY, f64::max) - y,
        }
    }
}

pub fn edge_path(
    from: &Rect,
    to: &Rect,
    from_side: Option<Side>,
    to_side: Option<Side>,
) -> EdgePath {
    let fs = from_side.unwrap_or_else(|| facing(from, to));
    let ts = to_side.unwrap_or_else(|| facing(to, from));
    let (x1, y1) = anchor(from, fs);
    let (x2, y2) = anchor(to, ts);
    let reach = ((x2 - x1).hypot(y2 - y1) / 2.).clamp(40., 240.);
    let (n1, n2) = (fs.normal(), ts.normal());
    let c1 = (x1 + n1.0 * reach, y1 + n1.1 * reach);
    let c2 = (x2 + n2.0 * reach, y2 + n2.1 * reach);
    EdgePath {
        start: (x1, y1),
        c1,
        c2,
        end: (x2, y2),
        mid: (
            (x1 + 3. * c1.0 + 3. * c2.0 + x2) / 8.,
            (y1 + 3. * c1.1 + 3. * c2.1 + y2) / 8.,
        ),
        start_angle: (-n1.1).atan2(-n1.0),
        end_angle: (-n2.1).atan2(-n2.0),
    }
}

/// Obsidian's default theme colors as (dark, light): the canvas's six presets use them.
const PRESETS: [(u32, u32); 6] = [
    (0xfb464c, 0xe93147), // red
    (0xe9973f, 0xec7500), // orange
    (0xe0de71, 0xe0ac00), // yellow
    (0x44cf6e, 0x08b94e), // green
    (0x53dfdd, 0x00bfbc), // cyan
    (0xa882ff, 0x7852ee), // purple
];

/// A node's or edge's color: Obsidian's presets "1" to "6" by theme, a hex color as written,
/// None for anything else (the theme's).
pub fn canvas_color(color: Option<&str>, dark: bool) -> Option<Hsla> {
    let color = color?.trim();
    if let Some(i) = color
        .parse::<usize>()
        .ok()
        .filter(|i| (1..=6).contains(i) && color.len() == 1)
    {
        let (d, l) = PRESETS[i - 1];
        return Some(gpui_kit::rgb(if dark { d } else { l }).into());
    }
    let hex = color.strip_prefix('#')?;
    if !matches!(hex.len(), 3 | 4 | 6 | 8) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Rgba::try_from(color).ok().map(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Instant;

    fn node(id: &str, x: f64, y: f64) -> Value {
        json!({ "id": id, "type": "text", "x": x, "y": y, "width": 250, "height": 60, "text": id })
    }

    fn with(mut v: Value, extra: Value) -> Value {
        for (k, val) in extra.as_object().unwrap() {
            v[k] = val.clone();
        }
        v
    }

    fn ids(c: &Canvas) -> Vec<&str> {
        c.nodes.iter().map(|n| n.id.as_str()).collect()
    }

    #[test]
    fn a_canvas_keeps_what_the_spec_allows_groups_first() {
        let c = parse_canvas(
            &json!({
                "nodes": [
                    { "id": "t", "type": "text", "x": 0, "y": 0, "width": 200, "height": 100, "text": "# Hi", "color": "4" },
                    { "id": "g", "type": "group", "x": -20, "y": -40, "width": 500, "height": 300, "label": "Plan" },
                    { "id": "f", "type": "file", "x": 300, "y": 0, "width": 200, "height": 100, "file": "Notes/Idea.md", "subpath": "#Part" },
                    { "id": "bad", "type": "text", "x": 0, "y": 0 },
                    { "id": "w", "type": "widget", "x": 0, "y": 0, "width": 1, "height": 1 },
                    { "id": "t", "type": "text", "x": 9, "y": 9, "width": 50, "height": 50, "text": "same id" },
                    { "id": "n", "type": "text", "x": 0, "y": 0, "width": -10, "height": 50 },
                ],
                "edges": [
                    { "id": "e1", "fromNode": "t", "toNode": "f", "fromSide": "right", "toSide": "left" },
                    { "id": "e2", "fromNode": "t", "toNode": "missing" },
                    { "id": "e3", "fromNode": "f", "toNode": "t", "fromEnd": "arrow", "toEnd": "none", "fromSide": "middle" },
                ],
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(ids(&c), ["g", "t", "f"]);
        assert_eq!(c.nodes[2].subpath.as_deref(), Some("#Part"));
        let edges: Vec<_> = c
            .edges
            .iter()
            .map(|e| (e.id.as_str(), e.from_arrow, e.to_arrow, e.from_side))
            .collect();
        assert_eq!(
            edges,
            [
                ("e1", false, true, Some(Side::Right)),
                ("e3", true, false, None)
            ]
        );
        // Edges point at the nodes as reordered.
        assert_eq!(
            (
                c.nodes[c.edges[0].from].id.as_str(),
                c.nodes[c.edges[0].to].id.as_str()
            ),
            ("t", "f")
        );
        assert_eq!(parse_canvas("").unwrap(), Canvas::default());
        assert!(parse_canvas("{nope").is_err());
    }

    #[test]
    fn geometry_bounds_facing_sides_and_edge_curves() {
        let a = Rect {
            x: 0.,
            y: 0.,
            width: 100.,
            height: 50.,
        };
        let b = Rect {
            x: 300.,
            y: 0.,
            width: 100.,
            height: 50.,
        };
        assert_eq!(
            bounds([&a, &b]),
            Rect {
                x: 0.,
                y: 0.,
                width: 400.,
                height: 50.
            }
        );
        // One card dragged far away doesn't shrink the rest to dots; spread-out cards all stay in.
        let grid: Vec<Rect> = (0..9)
            .map(|i| Rect {
                x: (i % 3) as f64 * 300.,
                y: (i / 3) as f64 * 200.,
                width: 250.,
                height: 150.,
            })
            .collect();
        let mut far = grid.clone();
        far.push(Rect {
            x: 1e7,
            y: 1e7,
            width: 250.,
            height: 150.,
        });
        assert_eq!(fit_box(&far), bounds(&grid));
        assert_eq!(fit_box(&grid), bounds(&grid));
        assert_eq!(fit_box(&[a, b]), bounds([&a, &b]));
        let many: Vec<Rect> = (0..200_000)
            .map(|i| Rect {
                x: i as f64,
                y: -(i as f64),
                width: 1.,
                height: 1.,
            })
            .collect();
        assert_eq!(
            bounds(&many),
            Rect {
                x: 0.,
                y: -199_999.,
                width: 200_000.,
                height: 200_000.
            }
        );
        assert_eq!(facing(&a, &b), Side::Right);
        assert_eq!(facing(&b, &a), Side::Left);
        assert_eq!(
            facing(
                &a,
                &Rect {
                    x: 0.,
                    y: 400.,
                    width: 10.,
                    height: 10.
                }
            ),
            Side::Bottom
        );
        let e = edge_path(&a, &b, None, None);
        assert_eq!(e.start, (100., 25.));
        assert_eq!(e.end, (300., 25.));
        assert_eq!(e.mid, (200., 25.));
    }

    #[test]
    fn colors_obsidians_presets_by_theme_hex_as_written_nothing_else() {
        let rgb = |c: u32| -> Option<Hsla> { Some(gpui_kit::rgb(c).into()) };
        assert_eq!(canvas_color(Some("1"), true), rgb(0xfb464c));
        assert_eq!(canvas_color(Some("4"), false), rgb(0x08b94e));
        assert_eq!(canvas_color(Some("#ff8800"), true), rgb(0xff8800));
        assert!(canvas_color(Some("#f80"), true).is_some());
        assert_eq!(canvas_color(Some("red; background: url(x)"), true), None);
        assert_eq!(canvas_color(None, true), None);
        assert_eq!(canvas_color(Some("constructor"), true), None);
        assert_eq!(canvas_color(Some("7"), true), None);
        assert_eq!(canvas_color(Some("#12345"), true), None);
    }

    #[test]
    fn malformed_canvases_not_json_is_an_error_wrong_shapes_are_skipped() {
        for bad in ["{", r#"{"nodes": ["#, "nul"] {
            assert!(parse_canvas(bad).is_err(), "{bad}");
        }
        for odd in [
            "null",
            "[]",
            "42",
            r#""text""#,
            r#"{"nodes": {}, "edges": "x"}"#,
            r#"{"nodes": [null, 1, "a", []]}"#,
        ] {
            assert_eq!(parse_canvas(odd).unwrap(), Canvas::default(), "{odd}");
        }
        let c = parse_canvas(
            &json!({ "nodes": [
                with(node("s", 0., 0.), json!({ "x": "10" })),
                with(node("z", 0., 0.), json!({ "width": 0, "height": 0 })),
                with(node("n", 0., 0.), json!({ "x": null })),
                node("ok", 0., 0.),
            ]})
            .to_string(),
        )
        .unwrap();
        assert_eq!(ids(&c), ["ok"]);
    }

    #[test]
    fn cards_point_at_missing_files_urls_of_any_kind_and_markup_kept_as_data() {
        let c = parse_canvas(
            &json!({ "nodes": [
                with(node("f", 0., 0.), json!({ "type": "file", "file": "../../etc/passwd" })),
                with(node("u", 0., 100.), json!({ "type": "link", "url": "javascript:alert(1)" })),
                with(node("t", 0., 200.), json!({ "text": "<img src=x onerror=alert(1)> [[Note]]" })),
                with(node("c", 0., 300.), json!({ "color": "red; background: url(https://example.com/x)" })),
            ]})
            .to_string(),
        )
        .unwrap();
        assert_eq!(c.nodes.len(), 4);
        assert_eq!(
            c.nodes[3].color.as_deref(),
            Some("red; background: url(https://example.com/x)")
        );
    }

    #[test]
    fn far_apart_cards_still_have_a_finite_box_to_fit() {
        let c = parse_canvas(
            &json!({ "nodes": [node("a", -1e9, -1e9), node("b", 1e9, 1e9)] }).to_string(),
        )
        .unwrap();
        let rects: Vec<Rect> = c.nodes.iter().map(|n| n.rect).collect();
        let b = bounds(&rects);
        assert!(b.width.is_finite() && b.height.is_finite());
        assert_eq!(b.width, 2e9 + 250.);
    }

    #[test]
    fn a_big_generated_canvas_parses_and_measures_quickly() {
        let nodes: Vec<Value> = (0..10000)
            .map(|i| {
                node(
                    &format!("n{i}"),
                    (i % 100) as f64 * 300.,
                    (i / 100) as f64 * 100.,
                )
            })
            .collect();
        let edges: Vec<Value> = (0..10000)
            .map(|i| json!({ "id": format!("e{i}"), "fromNode": format!("n{i}"), "toNode": format!("n{}", (i + 1) % 10000) }))
            .collect();
        let text = json!({ "nodes": nodes, "edges": edges }).to_string();
        let start = Instant::now();
        let c = parse_canvas(&text).unwrap();
        let rects: Vec<Rect> = c.nodes.iter().map(|n| n.rect).collect();
        let b = bounds(&rects);
        for e in &c.edges {
            edge_path(
                &c.nodes[e.from].rect,
                &c.nodes[e.to].rect,
                e.from_side,
                e.to_side,
            );
        }
        assert_eq!(c.edges.len(), 10000);
        assert!(b.width > 0.);
        // A debug build; the old app's limit was 1s in node.
        assert!(start.elapsed().as_millis() < 3000, "{:?}", start.elapsed());
    }

    #[test]
    fn an_edge_from_a_card_to_itself_draws_a_loop_not_nan() {
        let c = parse_canvas(
            &json!({ "nodes": [node("a", 0., 0.)], "edges": [{ "id": "e", "fromNode": "a", "toNode": "a" }] }).to_string(),
        )
        .unwrap();
        let r = c.nodes[0].rect;
        let e = edge_path(&r, &r, None, None);
        for (x, y) in [e.start, e.c1, e.c2, e.end, e.mid] {
            assert!(x.is_finite() && y.is_finite());
        }
        assert_eq!(c.edges.len(), 1);
    }
}
