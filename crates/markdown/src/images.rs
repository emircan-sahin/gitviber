//! An image cache a view owns, so its images go with it: textures are dropped from the windows
//! when the cache is released. Still images on disk are decoded off the main thread and scaled
//! down to `max_side`, so a 6000px screenshot shown at 800px doesn't keep 140 MB of pixels.
//! SVG, animated GIF/WebP and remote images take gpui's own loader, unless the bytes come from
//! the view's own reader (a document at a revision, [`crate::MarkdownView::read_images`]).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use gpui_kit::{
    App, AppContext as _, Entity, EntityId, Image, ImageCache, ImageCacheError, ImageCacheItem,
    ImageFormat, RenderImage, Resource, SvgRenderer, Task, WeakEntity, Window,
};

/// Past this many images the oldest go: a long page scrolled through holds a bounded set.
const MAX_IMAGES: usize = 64;

type Loaded = Result<Arc<RenderImage>, ImageCacheError>;

/// Reads an image's file in place of the disk; called off the main thread.
pub(crate) type ReadBytes = Arc<dyn Fn(&Path) -> Result<Vec<u8>, String> + Send + Sync>;

enum Entry {
    Gpui(ImageCacheItem),
    /// Decoded by us: a still image scaled down, or any file the reader gives.
    Ours {
        result: Option<Loaded>,
        waiting: Vec<EntityId>,
        _task: Task<()>,
    },
}

impl Entry {
    fn loaded(&self) -> Option<Loaded> {
        match self {
            Entry::Gpui(item) => item.get(),
            Entry::Ours { result, .. } => result.clone(),
        }
    }
}

pub struct BoundedImageCache {
    me: WeakEntity<Self>,
    max_side: u32,
    entries: HashMap<u64, Entry>,
    /// Load order, oldest first, for evicting.
    order: Vec<u64>,
    read: Option<ReadBytes>,
    _live: gitviber_ui::memory::Live,
}

impl BoundedImageCache {
    /// A cache whose still images are at most `max_side` pixels on their longer side.
    pub fn new(max_side: u32, cx: &mut App) -> Entity<Self> {
        let cache = cx.new(|cx| BoundedImageCache {
            me: cx.weak_entity(),
            max_side: max_side.max(64),
            entries: HashMap::new(),
            order: Vec::new(),
            read: None,
            _live: gitviber_ui::memory::Live::new("BoundedImageCache"),
        });
        cx.observe_release(&cache, |cache, cx| {
            for (_, entry) in cache.entries.drain() {
                if let Some(Ok(image)) = entry.loaded() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        cache
    }

    /// The longer side a view of `window`'s size wants: its device pixels, times two for zoom.
    pub fn display_side(window: &Window) -> u32 {
        let size = window.viewport_size();
        let longest = f32::from(size.width.max(size.height)) * window.scale_factor() * 2.0;
        (longest as u32).clamp(1024, 8192)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn loaded(&self) -> usize {
        self.entries
            .values()
            .filter(|e| matches!(e.loaded(), Some(Ok(_))))
            .count()
    }

    /// Drops every image, as when the file shown changed.
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        self.order.clear();
        for (_, entry) in self.entries.drain() {
            if let Some(Ok(image)) = entry.loaded() {
                cx.drop_image(image, Some(window));
            }
        }
    }

    /// Image files come from `read` instead of the disk (None: the disk again); what was
    /// loaded before is dropped.
    pub(crate) fn set_read(&mut self, read: Option<ReadBytes>, window: &mut Window, cx: &mut App) {
        self.read = read;
        self.clear(window, cx);
    }

    fn evict(&mut self, window: &mut Window, cx: &mut App) {
        while self.order.len() > MAX_IMAGES {
            let key = self.order.remove(0);
            if let Some(entry) = self.entries.remove(&key)
                && let Some(Ok(image)) = entry.loaded()
            {
                cx.drop_image(image, Some(window));
            }
        }
    }
}

/// The kinds gpui decodes itself (vector or animated); still images are scaled down by us.
fn gpui_format(path: &Path) -> Option<ImageFormat> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "svg" => Some(ImageFormat::Svg),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

fn key(resource: &Resource) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    resource.hash(&mut h);
    h.finish()
}

/// Decodes the file at `path`: a still image scaled to fit `max_side`, the rest as gpui does.
fn decode(path: &Path, bytes: Vec<u8>, max_side: u32, svg: SvgRenderer) -> Loaded {
    match gpui_format(path) {
        None => decode_scaled(&bytes, max_side),
        Some(ImageFormat::Svg) => Ok(svg.render_single_frame(&bytes, 1.0)?),
        Some(format) => Ok(Image::from_bytes(format, bytes).to_image_data(svg)?),
    }
}

/// Decodes `bytes` and scales it to fit `max_side`, as BGRA, which gpui draws.
pub fn decode_scaled(bytes: &[u8], max_side: u32) -> Loaded {
    let image = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .decode()?;
    let (w, h) = (image.width(), image.height());
    let image = if w.max(h) > max_side {
        let scale = max_side as f64 / w.max(h) as f64;
        let (nw, nh) = (
            ((w as f64 * scale).round() as u32).max(1),
            ((h as f64 * scale).round() as u32).max(1),
        );
        image.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut rgba = image.into_rgba8();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])))
}

impl ImageCache for BoundedImageCache {
    fn load(&mut self, resource: &Resource, window: &mut Window, cx: &mut App) -> Option<Loaded> {
        let key = key(resource);
        if let Some(entry) = self.entries.get_mut(&key) {
            return match entry {
                Entry::Gpui(item) => item.use_image(window),
                Entry::Ours {
                    result: Some(result),
                    ..
                } => Some(result.clone()),
                Entry::Ours {
                    result: None,
                    waiting,
                    ..
                } => {
                    let view = window.current_view();
                    if !waiting.contains(&view) {
                        waiting.push(view);
                    }
                    None
                }
            };
        }
        let entry = match resource {
            Resource::Path(path) if self.read.is_some() || gpui_format(path).is_none() => {
                let path = path.clone();
                let max_side = self.max_side;
                let read = self.read.clone();
                let svg = cx.svg_renderer();
                let waiting = vec![window.current_view()];
                let me = self.me.clone();
                let task = cx.spawn(async move |cx| {
                    let decoded = cx
                        .background_spawn(async move {
                            let bytes = match read {
                                Some(read) => read(&path).map_err(std::io::Error::other)?,
                                None => std::fs::read(&path)?,
                            };
                            decode(&path, bytes, max_side, svg)
                        })
                        .await;
                    cx.update(|cx| {
                        let views = me.update(cx, |cache, _| match cache.entries.get_mut(&key) {
                            Some(Entry::Ours {
                                result, waiting, ..
                            }) => {
                                *result = Some(decoded);
                                std::mem::take(waiting)
                            }
                            _ => vec![],
                        });
                        for view in views.unwrap_or_default() {
                            cx.notify(view);
                        }
                    });
                });
                Entry::Ours {
                    result: None,
                    waiting,
                    _task: task,
                }
            }
            _ => Entry::Gpui(ImageCacheItem::new(resource, cx)),
        };
        let loaded = match &entry {
            Entry::Gpui(item) => item.use_image(window),
            Entry::Ours { .. } => None,
        };
        self.entries.insert(key, entry);
        self.order.push(key);
        self.evict(window, cx);
        loaded
    }
}
