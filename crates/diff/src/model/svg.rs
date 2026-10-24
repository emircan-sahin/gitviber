//! An SVG's own size, from its root element, as the renderer works it out: width and height, a
//! missing one from the viewBox's proportions, else the viewBox, else 100 × 100. The preview
//! draws it at this size at 1:1 and rasterizes for it, so a wrong guess costs sharpness, never
//! memory.

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
                    (None, '>') => return Some(&text[start..start + i]),
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
    let mut rest = tag;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + name.len()..].trim_start();
        rest = &rest[at + name.len()..];
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(value) = after.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let q = value.chars().next()?;
        if q != '"' && q != '\'' {
            continue;
        }
        let end = value[1..].find(q)?;
        return Some(&value[1..1 + end]);
    }
    None
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
}
