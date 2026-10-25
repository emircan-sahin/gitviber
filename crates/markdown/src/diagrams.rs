//! Mermaid diagrams and TeX math, drawn natively: mermaid-rs-renderer makes the diagram's SVG,
//! which gpui's own resvg rasterizes; RaTeX lays math out and paints it with the KaTeX fonts.
//! Both run off the main thread at the display's scale, in the theme's colors, and land in a
//! cache the document owns: a bounded number of pictures, their textures dropped with it.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use gpui_kit::{App, AppContext as _, Context, Entity, Hsla, RenderImage, Rgba, Task, WeakEntity};

use crate::blocks::DiagramKind;

/// Pictures a document keeps; a long note's formulas past this are drawn again when scrolled back.
const KEEP: usize = 64;

/// What a picture is drawn for: the theme's look and the display's density.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub dark: bool,
    pub color: Hsla,
    /// The text size math is set at, in logical pixels.
    pub font_size: f32,
    pub scale: f32,
}

#[derive(Clone)]
pub enum Drawn {
    /// Device pixels; shown at `image size / scale`.
    Ready {
        image: Arc<RenderImage>,
        scale: f32,
    },
    Failed(String),
}

enum Entry {
    /// Dropping it stops the drawing: an evicted or released entry doesn't finish.
    Pending(#[allow(dead_code)] Task<()>),
    Done(Drawn),
}

type OnReady = Box<dyn Fn(&mut App)>;

pub struct Diagrams {
    entries: HashMap<u64, Entry>,
    order: VecDeque<u64>,
    /// Told when a picture lands, to lay the document out again.
    on_ready: Option<OnReady>,
    me: WeakEntity<Self>,
}

fn key(kind: DiagramKind, source: &str, inline: bool, look: &Look) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        kind as u8,
        source,
        inline,
        look.dark,
        look.font_size.to_bits(),
        look.scale.to_bits(),
    )
        .hash(&mut h);
    let c = look.color;
    (c.h.to_bits(), c.s.to_bits(), c.l.to_bits(), c.a.to_bits()).hash(&mut h);
    h.finish()
}

impl Diagrams {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let diagrams = cx.new(|cx| Diagrams {
            entries: HashMap::new(),
            order: VecDeque::new(),
            on_ready: None,
            me: cx.weak_entity(),
        });
        cx.observe_release(&diagrams, |d, cx| {
            for (_, entry) in d.entries.drain() {
                if let Entry::Done(Drawn::Ready { image, .. }) = entry {
                    // Dropped once this update is over: the window it runs in is out of the app's
                    // list until then, and would keep the picture.
                    cx.defer(move |cx| cx.drop_image(image, None));
                }
            }
        })
        .detach();
        diagrams
    }

    pub fn on_ready(&mut self, f: impl Fn(&mut App) + 'static) {
        self.on_ready = Some(Box::new(f));
    }

    /// The picture of `source`, once drawn; None while it's being drawn (the first ask starts it).
    pub fn get(
        &mut self,
        kind: DiagramKind,
        source: &str,
        inline: bool,
        look: Look,
        cx: &mut Context<Self>,
    ) -> Option<Drawn> {
        let k = key(kind, source, inline, &look);
        if let Some(entry) = self.entries.get(&k) {
            return match entry {
                Entry::Done(drawn) => Some(drawn.clone()),
                Entry::Pending(_) => None,
            };
        }
        let svg = cx.svg_renderer();
        let source = source.to_string();
        let me = self.me.clone();
        let task = cx.spawn(async move |_, cx| {
            let drawn = cx
                .background_spawn(async move {
                    match kind {
                        DiagramKind::Mermaid => draw_mermaid(&source, &look, &svg),
                        DiagramKind::Math => draw_math(&source, inline, &look),
                    }
                })
                .await;
            let drawn = match drawn {
                Ok(image) => Drawn::Ready {
                    image,
                    scale: look.scale,
                },
                Err(e) => Drawn::Failed(e),
            };
            _ = me.update(cx, |d, cx| {
                d.entries.insert(k, Entry::Done(drawn));
                if let Some(f) = &d.on_ready {
                    f(cx);
                }
            });
        });
        self.entries.insert(k, Entry::Pending(task));
        self.order.push_back(k);
        while self.order.len() > KEEP {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if let Some(Entry::Done(Drawn::Ready { image, .. })) = self.entries.remove(&old) {
                cx.defer(move |cx| cx.drop_image(image, None));
            }
        }
        None
    }
}

fn hex(c: Hsla) -> String {
    let c: Rgba = c.into();
    let b = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

/// A diagram's SVG, in the theme's text and line color, on no background.
pub fn mermaid_svg(source: &str, look: &Look) -> Result<String, String> {
    let mut theme = if look.dark {
        mermaid_rs_renderer::Theme::dark()
    } else {
        mermaid_rs_renderer::Theme::modern()
    };
    theme.background = "transparent".into();
    let ink = hex(look.color);
    theme.text_color = ink.clone();
    theme.primary_text_color = ink;
    let options = mermaid_rs_renderer::RenderOptions {
        theme,
        layout: Default::default(),
    };
    // The renderer is new; a diagram it can't take must not take the app down with it.
    std::panic::catch_unwind(|| mermaid_rs_renderer::render_with_options(source, options))
        .map_err(|_| "The diagram couldn't be drawn.".to_string())?
        .map_err(|e| e.to_string())
}

fn draw_mermaid(
    source: &str,
    look: &Look,
    svg: &gpui_kit::SvgRenderer,
) -> Result<Arc<RenderImage>, String> {
    let text = mermaid_svg(source, look)?;
    // At the display's density: gpui's own factor would draw it at twice that, four times the pixels.
    svg.render_single_frame(
        text.as_bytes(),
        look.scale / gpui_kit::SMOOTH_SVG_SCALE_FACTOR,
    )
    .map_err(|e| e.to_string())
}

/// A formula as PNG bytes: display style for a block, text style inline.
pub fn math_png(source: &str, inline: bool, look: &Look) -> Result<Vec<u8>, String> {
    use ratex_types::color::Color;
    use ratex_types::math_style::MathStyle;
    let c: Rgba = look.color.into();
    let color = Color {
        r: c.r,
        g: c.g,
        b: c.b,
        a: 1.0,
    };
    let style = if inline {
        MathStyle::Text
    } else {
        MathStyle::Display
    };
    std::panic::catch_unwind(|| {
        let nodes = ratex_parser::parse(source).map_err(|e| e.to_string())?;
        let options = ratex_layout::LayoutOptions::default()
            .with_style(style)
            .with_color(color);
        let list = ratex_layout::to_display_list(&ratex_layout::layout(&nodes, &options));
        let render = ratex_render::RenderOptions {
            font_size: look.font_size * if inline { 1.0 } else { 1.15 },
            padding: if inline { 1.0 } else { 4.0 },
            background_color: Color {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 0.,
            },
            font_dir: String::new(),
            device_pixel_ratio: look.scale,
        };
        ratex_render::render_to_png(&list, &render)
    })
    .map_err(|_| "The formula couldn't be drawn.".to_string())?
}

fn draw_math(source: &str, inline: bool, look: &Look) -> Result<Arc<RenderImage>, String> {
    let png = math_png(source, inline, look)?;
    crate::images::decode_scaled(&png, 8192).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> Look {
        Look {
            dark: true,
            color: gpui_kit::rgb(0xdddddd).into(),
            font_size: 16.,
            scale: 2.,
        }
    }

    #[test]
    fn diagrams_come_out_as_svg_in_the_themes_ink() {
        let svg = mermaid_svg("flowchart LR; A[Start]-->B[End]", &look()).unwrap();
        assert!(svg.contains("<svg") && svg.contains("Start"));
        assert!(svg.to_lowercase().contains("#dddddd"));
        assert!(mermaid_svg("this is not a diagram ((", &look()).is_err());
    }

    #[test]
    fn formulas_come_out_as_pixels_at_the_displays_scale() {
        let png = math_png(r"\frac{a}{b} + \sqrt{x^2}", false, &look()).unwrap();
        let image = image::load_from_memory(&png).unwrap();
        // Two device pixels a point: taller than a line of 16px text.
        assert!(image.height() > 40, "{}", image.height());
        let small = image::load_from_memory(&math_png("x", true, &look()).unwrap()).unwrap();
        assert!(small.height() < image.height());
        assert!(math_png(r"\frac{a}{", false, &look()).is_err());
    }
}
