//! An SVG's own size, from its root element, as the renderer works it out: width and height, a
//! missing one from the viewBox's proportions, else the viewBox, else 100 × 100. The preview
//! draws it at this size at 1:1 and rasterizes for it, so a wrong guess costs sharpness, never
//! memory. And the same drawing cropped to a part of it, for a zoomed preview to rasterize only
//! what's on screen.

use std::ops::Range;

const DEFAULT: (f32, f32) = (100., 100.);

/// None when there's no `<svg` element in the text.
pub fn intrinsic_size(text: &str) -> Option<(f32, f32)> {
    let tag = root_tag(text)?;
    let width = attribute(tag, "width").and_then(length);
    let height = attribute(tag, "height").and_then(length);
    let view_box = attribute(tag, "viewBox").and_then(|v| {
        let n: Vec<f32> = v
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        (n.len() == 4 && n[2] > 0. && n[3] > 0.).then(|| (n[2], n[3]))
    });
    let size = match (width, height, view_box) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some((vw, vh))) => (w, w * vh / vw),
        (None, Some(h), Some((vw, vh))) => (h * vw / vh, h),
        (None, None, Some(vb)) => vb,
        (w, h, _) => (w.unwrap_or(DEFAULT.0), h.unwrap_or(DEFAULT.1)),
    };
    let ok = |v: f32| v.is_finite() && v > 0.;
    Some(if ok(size.0) && ok(size.1) {
        size
    } else {
        DEFAULT
    })
}

/// The root element's start tag, attributes and all.
fn root_tag(text: &str) -> Option<&str> {
    root_range(text).map(|r| &text[r])
}

/// Where the root's start tag is: from its `<svg` to before its `>`.
fn root_range(text: &str) -> Option<Range<usize>> {
    let mut from = 0;
    while let Some(at) = text[from..].find("<svg") {
        let start = from + at;
        let after = text[start + 4..].chars().next();
        if after.is_some_and(|c| c.is_whitespace() || c == '>' || c == '/') {
            let mut quote = None;
            for (i, c) in text[start..].char_indices() {
                match (quote, c) {
                    (None, '"' | '\'') => quote = Some(c),
                    (Some(q), c) if c == q => quote = None,
                    (None, '>') => return Some(start..start + i),
                    _ => {}
                }
            }
            return None;
        }
        from = start + 4;
    }
    None
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    attribute_span(tag, name).map(|(_, value)| value)
}

/// Attribute `name` of `tag`: its span, from the space before it to its closing quote, and its
/// value.
fn attribute_span<'a>(tag: &'a str, name: &str) -> Option<(Range<usize>, &'a str)> {
    let mut from = 0;
    while let Some(at) = tag[from..].find(name).map(|a| a + from) {
        from = at + name.len();
        let Some(before) = tag[..at].chars().next_back().filter(|c| c.is_whitespace()) else {
            continue;
        };
        let Some(value) = tag[from..].trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let q = value.chars().next()?;
        if q != '"' && q != '\'' {
            continue;
        }
        let open = tag.len() - value.len();
        let close = open + 1 + tag[open + 1..].find(q)?;
        return Some((at - before.len_utf8()..close + 1, &tag[open + 1..close]));
    }
    None
}

/// `text` showing only `part` of its drawing (x, y, width, height in its own pixels) on
/// `device` pixels. The root keeps its own viewBox and is sized to its natural size inside an
/// outer one that crops, so percentages in the drawing still mean what they did.
pub fn crop(text: &str, part: [f32; 4], device: (u32, u32)) -> Option<String> {
    let range = root_range(text)?;
    let (w, h) = intrinsic_size(text)?;
    let mut root = text[range.clone()].to_string();
    // A nested root would be moved by its x and y, which an outermost one ignores, and clipped
    // to its size, which costs a mask the size of the crop: the outer one clips anyway.
    for name in ["width", "height", "x", "y", "overflow"] {
        while let Some((span, _)) = attribute_span(&root, name) {
            root.replace_range(span, "");
        }
    }
    root.insert_str(
        4,
        &format!(" width=\"{w}\" height=\"{h}\" overflow=\"visible\""),
    );
    let [x, y, pw, ph] = part;
    Some(format!(
        "{}<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" \
         viewBox=\"{x} {y} {pw} {ph}\" preserveAspectRatio=\"none\">{root}{}</svg>",
        &text[..range.start],
        device.0.max(1),
        device.1.max(1),
        &text[range.end..],
    ))
}

/// A length in CSS pixels; None for a percentage, which leaves the size to the viewBox.
fn length(v: &str) -> Option<f32> {
    let v = v.trim();
    let split = v
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
        .unwrap_or(v.len());
    let (n, unit) = v.split_at(split);
    let n: f32 = n.parse().ok()?;
    let per = match unit.trim() {
        "" | "px" => 1.,
        "pt" => 4. / 3.,
        "pc" => 16.,
        "in" => 96.,
        "cm" => 96. / 2.54,
        "mm" => 96. / 25.4,
        "em" => 16.,
        "ex" => 8.,
        _ => return None,
    };
    Some(n * per)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_as_the_renderer_works_them_out() {
        let s = |t: &str| intrinsic_size(t);
        assert_eq!(
            s(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="16">"#),
            Some((24., 16.))
        );
        assert_eq!(
            s(r#"<?xml?><svg viewBox="0 0 48 32"></svg>"#),
            Some((48., 32.))
        );
        assert_eq!(
            s(r#"<svg width="96px" viewBox="0,0,48,32">"#),
            Some((96., 64.))
        );
        assert_eq!(s(r#"<svg width="1in" height="50%">"#), Some((96., 100.)));
        assert_eq!(s("<svg>"), Some((100., 100.)));
        assert_eq!(s(r#"<svg width="2em" height="0">"#), Some((100., 100.)));
        // An attribute's name inside another, and a '>' inside a quoted value.
        assert_eq!(
            s(r#"<svg data-x="a>b" stroke-width="3" width='10' height="5">"#),
            Some((10., 5.))
        );
        assert_eq!(s("<svgx width='1'>"), None);
        assert_eq!(s("plain text"), None);
    }

    #[test]
    fn a_crop_wraps_the_root_at_its_own_size() {
        let text = r#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" x="3" width="50%" stroke-width="2" viewBox="0 0 12 8"><rect width="100%"/></svg>"#;
        let out = crop(text, [6., 0., 6., 8.], (60, 80)).unwrap();
        assert_eq!(
            out,
            r#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" width="60" height="80" viewBox="6 0 6 8" preserveAspectRatio="none"><svg width="12" height="8" overflow="visible" xmlns="http://www.w3.org/2000/svg" stroke-width="2" viewBox="0 0 12 8"><rect width="100%"/></svg></svg>"#
        );
        assert_eq!(crop("no drawing", [0., 0., 1., 1.], (1, 1)), None);
    }
}
