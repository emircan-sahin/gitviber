//! Raster images (PNG, JPEG, GIF, WebP, BMP, ICO, TIFF, …) decoded no bigger than they're shown.
//! A view holds the whole image at the size fitting shows it; zoomed past what that holds, only
//! the part on screen is decoded again at the display's density, its pixels kept square above
//! 100% so pixel art stays crisp (gpui's sampler is linear). An animation plays from a thread
//! that decodes one frame ahead, so only two frames are ever held. Photos turn as their EXIF
//! orientation says.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::time::Duration;

use gpui_kit::RenderImage;
use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{
    AnimationDecoder as _, DynamicImage, Frames, ImageDecoder as _, ImageFormat, ImageReader,
    RgbaImage, imageops, metadata::Orientation,
};

/// Past this many pixels an image isn't decoded: 256 MB while it is.
pub const MAX_PIXELS: u64 = 64_000_000;
/// The atlas' largest texture side.
const MAX_SIDE: u32 = 16384;
/// A frame shorter than this plays at 100 ms, as browsers do.
const MIN_DELAY: Duration = Duration::from_millis(20);

type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// A file's size as its bar shows it: "812 B", "34.5 KB", "2.1 MB".
pub fn file_size(n: u64) -> String {
    match n {
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.1} KB", n as f64 / 1024.),
        n => format!("{n} B"),
    }
}

/// What an image's header says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// As shown: turned by its orientation.
    pub size: (u32, u32),
    /// More than one frame (GIF, WebP, APNG).
    pub animated: bool,
}

fn reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(err)?;
    if reader.format().is_none() {
        return Err("Not an image format GitViber reads".into());
    }
    Ok(reader)
}

fn turns(o: Orientation) -> bool {
    use Orientation::*;
    matches!(o, Rotate90 | Rotate270 | Rotate90FlipH | Rotate270FlipH)
}

fn too_large(w: u32, h: u32) -> Option<String> {
    (w as u64 * h as u64 > MAX_PIXELS).then(|| format!("{w} × {h} is too large to preview"))
}

/// The frames of an animation; None for a still image.
fn animation(bytes: &[u8]) -> Option<Frames<'_>> {
    let at = || Cursor::new(bytes);
    match image::guess_format(bytes).ok()? {
        ImageFormat::Gif => Some(GifDecoder::new(at()).ok()?.into_frames()),
        ImageFormat::WebP => {
            let d = WebPDecoder::new(at()).ok()?;
            d.has_animation().then(|| d.into_frames())
        }
        ImageFormat::Png => {
            let d = PngDecoder::new(at()).ok()?;
            if !d.is_apng().ok()? {
                return None;
            }
            Some(d.apng().ok()?.into_frames())
        }
        _ => None,
    }
}

/// Reads the header, and for a GIF its first two frames.
pub fn probe(bytes: &[u8]) -> Result<Probe> {
    let mut decoder = reader(bytes)?.into_decoder().map_err(err)?;
    let (w, h) = decoder.dimensions();
    if let Some(e) = too_large(w, h) {
        return Err(e);
    }
    let size = match decoder.orientation() {
        Ok(o) if turns(o) => (h, w),
        _ => (w, h),
    };
    let animated = animation(bytes).is_some_and(|f| f.take(2).filter(|f| f.is_ok()).count() == 2);
    Ok(Probe { size, animated })
}

/// The first frame, turned as its orientation says, as RGBA.
fn decode(bytes: &[u8]) -> Result<RgbaImage> {
    let mut decoder = reader(bytes)?.into_decoder().map_err(err)?;
    let (w, h) = decoder.dimensions();
    if let Some(e) = too_large(w, h) {
        return Err(e);
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(err)?;
    image.apply_orientation(orientation);
    Ok(image.into_rgba8())
}

/// `image` shrunk to fit `target`, never grown.
fn shrunk(image: RgbaImage, target: (u32, u32)) -> RgbaImage {
    let (w, h) = image.dimensions();
    let k = (target.0 as f32 / w as f32)
        .min(target.1 as f32 / h as f32)
        .min(MAX_SIDE as f32 / w.max(h) as f32);
    if k >= 1. {
        return image;
    }
    let side = |n: u32| ((n as f32 * k).round() as u32).max(1);
    imageops::thumbnail(&image, side(w), side(h))
}

/// RGBA as gpui draws it: BGRA, one frame.
fn render_image(mut rgba: RgbaImage) -> Arc<RenderImage> {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(rgba)]))
}

/// The whole image, at most `target` big (its own size if that's smaller).
pub fn whole(bytes: &[u8], target: (u32, u32)) -> Result<Arc<RenderImage>> {
    Ok(render_image(shrunk(decode(bytes)?, target)))
}

/// A part of an image to draw sharp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    /// x, y, width, height in the image's pixels.
    pub part: [f32; 4],
    /// The texture's size.
    pub device: (u32, u32),
    /// Each of the image's pixels a square block: shown above 100%.
    pub crisp: bool,
}

impl Crop {
    /// Rounded, so a frame that asks for the same part again is told apart from a new one.
    pub fn key(&self) -> [i32; 6] {
        let [x, y, w, h] = self.part.map(|v| (v * 16.).round() as i32);
        [x, y, w, h, self.device.0 as i32, self.device.1 as i32]
    }
}

/// The crop an image `natural` big needs, shown `scale` view pixels to one of its pixels with
/// `visible` of it (its pixels) on screen, `density` device pixels to a view pixel. None when
/// the whole image as held (`held` pixels wide) is sharp enough.
pub fn crop_for(
    natural: (u32, u32),
    held: u32,
    scale: f32,
    density: f32,
    visible: [f32; 4],
) -> Option<Crop> {
    let [x, y, w, h] = visible;
    if w <= 0. || h <= 0. || scale <= 0. {
        return None;
    }
    let side = |v: f32| (v.round() as u32).clamp(1, MAX_SIDE);
    if scale > 1.001 {
        let k = scale * density;
        return Some(Crop {
            part: visible,
            device: (side(w * k), side(h * k)),
            crisp: true,
        });
    }
    // Up to 100% the GPU may stretch an image's own pixels: none past them is wanted.
    let k = (scale * density).min(1.);
    if natural.0 as f32 * k <= held as f32 * 1.1 {
        return None;
    }
    // Whole pixels of the image, shrunk to the screen's.
    let (x0, y0) = (x.floor().max(0.), y.floor().max(0.));
    let (x1, y1) = (
        (x + w).ceil().min(natural.0 as f32),
        (y + h).ceil().min(natural.1 as f32),
    );
    let (pw, ph) = (x1 - x0, y1 - y0);
    (pw > 0. && ph > 0.).then(|| Crop {
        part: [x0, y0, pw, ph],
        device: (side(pw * k), side(ph * k)),
        crisp: false,
    })
}

/// Draws `crop` of the image in `bytes`.
pub fn draw(bytes: &[u8], crop: &Crop) -> Result<Arc<RenderImage>> {
    let image = decode(bytes)?;
    let out = if crop.crisp {
        nearest(&image, crop.part, crop.device)
    } else {
        let [x, y, w, h] = crop.part.map(|v| v as u32);
        let part = imageops::crop_imm(&image, x, y, w, h).to_image();
        drop(image);
        shrunk(part, crop.device)
    };
    Ok(render_image(out))
}

/// `part` of `image` (its pixels) at `device` size, each device pixel taking the image's pixel
/// under its middle.
fn nearest(image: &RgbaImage, part: [f32; 4], device: (u32, u32)) -> RgbaImage {
    let (iw, ih) = image.dimensions();
    let at = |d: u32, from: f32, len: f32, out: u32, max: u32| {
        ((from + (d as f32 + 0.5) * len / out as f32).max(0.) as u32).min(max - 1) as usize
    };
    let xs: Vec<usize> = (0..device.0)
        .map(|d| at(d, part[0], part[2], device.0, iw) * 4)
        .collect();
    let src = image.as_raw();
    let stride = iw as usize * 4;
    let mut out = RgbaImage::new(device.0, device.1);
    for (dy, row) in out.chunks_exact_mut(device.0 as usize * 4).enumerate() {
        let from = &src[at(dy as u32, part[1], part[3], device.1, ih) * stride..];
        for (px, &sx) in row.as_chunks_mut::<4>().0.iter_mut().zip(&xs) {
            px.copy_from_slice(&from[sx..sx + 4]);
        }
    }
    out
}

/// An animation playing: a thread decodes the next frame, `size` big at most, while one shows,
/// and stops once this is dropped.
pub struct Player {
    frames: Receiver<(Arc<RenderImage>, Duration)>,
}

/// What a player has for the view now.
pub enum Next {
    Frame(Arc<RenderImage>, Duration),
    /// Still decoding.
    Wait,
    /// No frame could be read.
    Done,
}

impl Player {
    pub fn start(bytes: Arc<[u8]>, size: (u32, u32)) -> Option<Player> {
        let (tx, frames) = sync_channel(1);
        std::thread::Builder::new()
            .name("gitviber-frames".into())
            .spawn(move || {
                loop {
                    let Some(all) = animation(&bytes) else { return };
                    let mut shown = false;
                    for frame in all {
                        let Ok(frame) = frame else { break };
                        let delay = Duration::from(frame.delay());
                        let delay = if delay < MIN_DELAY {
                            Duration::from_millis(100)
                        } else {
                            delay
                        };
                        let image = render_image(shrunk(frame.into_buffer(), size));
                        // The view is gone.
                        if tx.send((image, delay)).is_err() {
                            return;
                        }
                        shown = true;
                    }
                    if !shown {
                        return;
                    }
                }
            })
            .ok()?;
        Some(Player { frames })
    }

    pub fn next(&self) -> Next {
        match self.frames.try_recv() {
            Ok((image, delay)) => Next::Frame(image, delay),
            Err(TryRecvError::Empty) => Next::Wait,
            Err(TryRecvError::Disconnected) => Next::Done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(image: &RgbaImage) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// The BGRA at (x, y).
    fn at(image: &RenderImage, x: usize, y: usize) -> [u8; 4] {
        let w = image.size(0).width.0 as usize;
        let b = image.as_bytes(0).unwrap();
        let i = (y * w + x) * 4;
        [b[i], b[i + 1], b[i + 2], b[i + 3]]
    }

    #[test]
    fn the_whole_image_is_held_no_bigger_than_shown() {
        let bytes = png(&RgbaImage::from_pixel(
            400,
            200,
            image::Rgba([255, 0, 0, 255]),
        ));
        let p = probe(&bytes).unwrap();
        assert_eq!(
            p,
            Probe {
                size: (400, 200),
                animated: false
            }
        );
        let fit = whole(&bytes, (100, 100)).unwrap();
        assert_eq!((fit.size(0).width.0, fit.size(0).height.0), (100, 50));
        // Never grown past its own pixels.
        let big = whole(&bytes, (4000, 4000)).unwrap();
        assert_eq!(big.size(0).width.0, 400);
        assert_eq!(at(&fit, 50, 25), [0, 0, 255, 255]);
        assert!(probe(b"not an image").is_err());
    }

    #[test]
    fn a_crop_is_wanted_once_the_held_pixels_are_too_few() {
        let natural = (4000, 3000);
        // At fit (a tenth) with that tenth held: none.
        assert_eq!(
            crop_for(natural, 800, 0.1, 2., [0., 0., 4000., 3000.]),
            None
        );
        // At half size the held 800 are too few: whole pixels of the part, at the screen's.
        let c = crop_for(natural, 800, 0.5, 1., [10.5, 20.2, 800., 600.]).unwrap();
        assert_eq!(c.part, [10., 20., 801., 601.]);
        assert_eq!(c.device, (401, 301));
        assert!(!c.crisp);
        // Up to 100%: never more than the image's own pixels.
        let c = crop_for(natural, 800, 0.9, 2., [0., 0., 100., 100.]).unwrap();
        assert_eq!(c.device, (100, 100));
        // Held whole at its own size: nothing to add until past 100%.
        assert_eq!(crop_for((64, 64), 64, 1., 2., [0., 0., 64., 64.]), None);
        // Past it, the part on screen at the display's density, crisp.
        let c = crop_for((64, 64), 64, 4., 2., [8., 8., 16., 10.]).unwrap();
        assert_eq!((c.device, c.crisp), ((128, 80), true));
    }

    #[test]
    fn crisp_pixels_are_square_blocks_from_the_right_part() {
        // A 4 × 4 checker of red and blue pixels.
        let checker = RgbaImage::from_fn(4, 4, |x, y| {
            if (x + y) % 2 == 0 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let bytes = png(&checker);
        let crop = crop_for((4, 4), 4, 8., 1., [1., 0., 2., 2.]).unwrap();
        let drawn = draw(&bytes, &crop).unwrap();
        assert_eq!(drawn.size(0).width.0, 16);
        // (1, 0) is blue, (2, 0) red: an 8-pixel block of each, edges sharp.
        assert_eq!(at(&drawn, 0, 0), [255, 0, 0, 255]);
        assert_eq!(at(&drawn, 7, 7), [255, 0, 0, 255]);
        assert_eq!(at(&drawn, 8, 0), [0, 0, 255, 255]);
        assert_eq!(at(&drawn, 8, 8), [255, 0, 0, 255]);
    }

    #[test]
    fn an_animation_plays_frame_after_frame() {
        use image::codecs::gif::GifEncoder;
        let mut bytes = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut bytes);
            for c in [[255, 0, 0, 255], [0, 255, 0, 255]] {
                let frame = image::Frame::from_parts(
                    RgbaImage::from_pixel(8, 8, image::Rgba(c)),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(50, 1),
                );
                enc.encode_frame(frame).unwrap();
            }
        }
        assert!(probe(&bytes).unwrap().animated);
        let player = Player::start(bytes.into(), (4, 4)).unwrap();
        let mut seen = Vec::new();
        while seen.len() < 3 {
            match player.next() {
                Next::Frame(image, delay) => {
                    assert_eq!(image.size(0).width.0, 4);
                    assert_eq!(delay, Duration::from_millis(50));
                    seen.push(at(&image, 1, 1));
                }
                Next::Wait => std::thread::sleep(Duration::from_millis(2)),
                Next::Done => panic!("ended"),
            }
        }
        // Red, green, then red again from the start.
        assert_eq!(seen[0], seen[2]);
        assert_ne!(seen[0], seen[1]);
    }
}
