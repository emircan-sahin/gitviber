//! An image cache a view owns, so its images go with it: textures are dropped from the windows
//! when the cache is released. Still images on disk are decoded off the main thread and scaled
//! down to `max_side`, so a 6000px screenshot shown at 800px doesn't keep 140 MB of pixels.
//! SVG, animated GIF/WebP and remote images take gpui's own loader.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use gpui_kit::{
    App, AppContext as _, Entity, EntityId, ImageCache, ImageCacheError, ImageCacheItem,
    RenderImage, Resource, Task, WeakEntity, Window,
};

/// Past this many images the oldest go: a long page scrolled through holds a bounded set.
const MAX_IMAGES: usize = 64;

type Loaded = Result<Arc<RenderImage>, ImageCacheError>;

enum Entry {
    Gpui(ImageCacheItem),
    Scaled {
        result: Option<Loaded>,
        waiting: Vec<EntityId>,
        _task: Task<()>,
    },
}

impl Entry {
    fn loaded(&self) -> Option<Loaded> {
        match self {
            Entry::Gpui(item) => item.get(),
            Entry::Scaled { result, .. } => result.clone(),
        }
    }
}

pub struct BoundedImageCache {
    me: WeakEntity<Self>,
    max_side: u32,
    entries: HashMap<u64, Entry>,
    /// Load order, oldest first, for evicting.
    order: Vec<u64>,
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

    /// Drops every image, as when the file shown changed.
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        self.order.clear();
        for (_, entry) in self.entries.drain() {
            if let Some(Ok(image)) = entry.loaded() {
                cx.drop_image(image, Some(window));
            }
        }
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

    fn scaled(path: &Path) -> bool {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        !matches!(ext.as_str(), "svg" | "gif" | "webp")
    }
}

fn key(resource: &Resource) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    resource.hash(&mut h);
    h.finish()
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
                Entry::Scaled {
                    result: Some(result),
                    ..
                } => Some(result.clone()),
                Entry::Scaled {
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
            Resource::Path(path) if Self::scaled(path) => {
                let path = path.clone();
                let max_side = self.max_side;
                let waiting = vec![window.current_view()];
                let me = self.me.clone();
                let task = cx.spawn(async move |cx| {
                    let decoded = cx
                        .background_spawn(async move {
                            let bytes = std::fs::read(&path)?;
                            decode_scaled(&bytes, max_side)
                        })
                        .await;
                    cx.update(|cx| {
                        let views = me.update(cx, |cache, _| match cache.entries.get_mut(&key) {
                            Some(Entry::Scaled {
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
                Entry::Scaled {
                    result: None,
                    waiting,
                    _task: task,
                }
            }
            _ => Entry::Gpui(ImageCacheItem::new(resource, cx)),
        };
        let loaded = match &entry {
            Entry::Gpui(item) => item.use_image(window),
            Entry::Scaled { .. } => None,
        };
        self.entries.insert(key, entry);
        self.order.push(key);
        self.evict(window, cx);
        loaded
    }
}
