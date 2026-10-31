//! A trackpad swipe through a full scrollback, drawn by the real text system and renderer off
//! screen: what a frame of it costs (`scroll_bench`, ignored: run it with `--run-ignored all
//! --no-capture`), and that a long scroll keeps memory flat.

use std::cell::Cell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gitviber_core::Core;
use gpui::{
    AnyView, AnyWindowHandle, AppContext as _, Context, Entity, HeadlessAppContext, IntoElement,
    Modifiers, MouseMoveEvent, ParentElement as _, PlatformInput, Render, ScrollDelta,
    ScrollWheelEvent, Styled as _, TouchPhase, Window, div, point, px, size,
};

use crate::{TerminalConfig, TerminalPane, TerminalTheme};

struct Host(Entity<TerminalPane>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

/// Bytes allocated and not freed by the thread counting: what a long scroll leaves behind. Other
/// threads go uncounted, as plain `cargo test` runs the crate's other tests in this process.
struct Counting;

thread_local! {
    // Const and without a destructor: read inside the allocator, it never allocates itself.
    static LIVE: Cell<Option<isize>> = const { Cell::new(None) };
}

fn count(bytes: isize) {
    let _ = LIVE.try_with(|l| l.set(l.get().map(|n| n + bytes)));
}

// SAFETY: forwards to the system allocator, only counting.
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        count(layout.size() as isize);
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        count(-(layout.size() as isize));
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        count(new_size as isize - layout.size() as isize);
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// What `f` left allocated, counted on this thread only.
fn left_allocated(f: impl FnOnce()) -> isize {
    LIVE.with(|l| l.set(Some(0)));
    f();
    LIVE.with(|l| l.replace(None)).unwrap_or(0)
}

/// A line as a busy agent's output has them: a colored prompt, 256-color words, a background,
/// bold, wide characters; `i` in it so no two lines are the same row.
fn line(i: usize) -> String {
    let mut s = format!("\x1b[1;32muser@host\x1b[0m:\x1b[34m~/src/{i:05}\x1b[0m$ ");
    for w in 0..18 {
        let color = (i * 7 + w * 13) % 216 + 16;
        match w % 6 {
            0 => s.push_str(&format!("\x1b[38;5;{color}mword{w}\x1b[0m ")),
            1 => s.push_str(&format!("\x1b[48;5;{color}m bg{w} \x1b[0m ")),
            2 => s.push_str("漢字テスト "),
            3 => s.push_str(&format!("\x1b[1mbold{i}\x1b[0m ")),
            4 => s.push_str("├──┤ "),
            _ => s.push_str(&format!("plain-{w}-{i} ")),
        }
    }
    s.push_str("\r\n");
    s
}

/// An agent's prose: words in the default color, a bold one and a colored path now and then.
fn prose(i: usize) -> String {
    let mut s = format!("  \x1b[1m{i:05}\x1b[0m ");
    for w in 0..24 {
        match w % 8 {
            3 => s.push_str(&format!("\x1b[36msrc/pane-{w}.rs\x1b[0m ")),
            _ => s.push_str(["the ", "scrollback ", "keeps ", "its ", "place ", "while "][w % 6]),
        }
    }
    s.push_str("\r\n");
    s
}

/// The pane first: it goes before the app it's in.
struct Bench {
    pane: Entity<TerminalPane>,
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
}

impl Bench {
    /// A pane of about 200×60 cells holding `lines` of history, drawn once.
    fn new(lines: usize, line: fn(usize) -> String) -> Self {
        gitviber_ui::test_support::config_dir("terminal-scroll-test");
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(gpui_apple::AppleTextSystem::new()),
            Arc::new(gitviber_ui::assets::Assets),
            gpui_snapshot_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            gitviber_ui::init(cx);
            crate::init(cx);
        });
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        let config = TerminalConfig {
            cursor_blink: false,
            shell_integration: false,
            ..Default::default()
        };
        let slot: std::rc::Rc<std::cell::RefCell<Option<Entity<TerminalPane>>>> =
            Default::default();
        let made = slot.clone();
        let window: AnyWindowHandle = cx
            .open_window(size(px(1640.), px(940.)), move |window, cx| {
                let pane = crate::pane::new_pane(
                    1,
                    core,
                    std::env::temp_dir(),
                    None,
                    None,
                    config,
                    TerminalTheme::dark(),
                    window,
                    cx,
                );
                pane.update(cx, |p, _| p.without_shell());
                *made.borrow_mut() = Some(pane.clone());
                let host = cx.new(|_| Host(pane));
                cx.new(|cx| gpui_kit::base::Root::new(AnyView::from(host), window, cx))
            })
            .unwrap()
            .into();
        let pane = slot.borrow_mut().take().unwrap();
        let mut bench = Bench { pane, cx, window };
        bench.frame();
        let text: String = (0..lines).map(line).collect();
        bench.cx.update(|cx| {
            bench.pane.read(cx).session.emu.lock().feed(text.as_bytes());
        });
        bench.frame();
        let center = bench.metrics().bounds.center();
        bench.event(PlatformInput::MouseMove(MouseMoveEvent {
            position: center,
            pressed_button: None,
            modifiers: Modifiers::none(),
        }));
        bench
    }

    fn metrics(&mut self) -> crate::pane::Metrics {
        let pane = self.pane.clone();
        self.cx
            .update(|cx| pane.read(cx).metrics)
            .expect("laid out")
    }

    fn event(&mut self, e: PlatformInput) {
        let _ = self
            .cx
            .update_window(self.window, |_, w, cx| w.dispatch_event(e, cx));
    }

    fn frame(&mut self) -> Duration {
        self.cx.run_until_parked();
        let t = Instant::now();
        let _ = self
            .cx
            .update_window(self.window, |_, w, cx| w.draw(cx).clear(cx));
        t.elapsed()
    }

    /// A swipe's event for `dy` pixels and the frame it draws (the test platform draws a window
    /// the event dirtied before the event returns): that frame's time, None where the view didn't
    /// move, and an app draws nothing.
    fn swipe(&mut self, dy: f32) -> Option<Duration> {
        let position = self.metrics().bounds.center();
        let was = self.view();
        let t = Instant::now();
        self.event(PlatformInput::ScrollWheel(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
        }));
        let took = t.elapsed();
        self.cx.run_until_parked();
        (self.view() != was).then_some(took)
    }

    /// Where the view is in the history: rows up from the bottom, and the part of a row short.
    fn view(&mut self) -> (usize, u32) {
        let pane = self.pane.clone();
        self.cx.update(|cx| {
            let p = pane.read(cx);
            let offset = p.session.emu.lock().term.grid().display_offset();
            (offset, p.scroll_frac.to_bits())
        })
    }
}

fn report(what: &str, events: usize, mut times: Vec<Duration>) {
    times.sort();
    let ms = |d: Option<&Duration>| d.map_or(0., |d| d.as_secs_f64() * 1000.);
    let at = |q: usize| ms(times.get((times.len() * q / 100).min(times.len().max(1) - 1)));
    let mean = times.iter().map(|d| ms(Some(d))).sum::<f64>() / times.len().max(1) as f64;
    eprintln!(
        "{what}: {} of {events} events moved the view; frame mean {mean:.2} ms, p50 {:.2}, \
         p95 {:.2}, max {:.2}",
        times.len(),
        at(50),
        at(95),
        ms(times.last()),
    );
}

/// A still frame's time, then a swipe's frames: slow (3 px an event, under a row), fast (60 px:
/// rows new to the view each frame) and back, on a pane of about 200×60 cells.
#[test]
#[ignore = "a benchmark"]
fn scroll_bench() {
    for (content, line) in [("mixed", line as fn(usize) -> String), ("prose", prose)] {
        let mut b = Bench::new(5000, line);
        let m = b.metrics();
        eprintln!("{content}: grid {}×{}, cell {:?}", m.cols, m.rows, m.cell);
        // Warm the glyph atlas and the text system's caches.
        for _ in 0..200 {
            b.swipe(20.);
        }
        report("still", 200, (0..200).map(|_| b.frame()).collect());
        for (what, dy) in [
            ("slow swipe, 3 px", 3.),
            ("fast swipe, 60 px", 60.),
            ("fast back, -60 px", -60.),
        ] {
            let times: Vec<_> = (0..400).filter_map(|_| b.swipe(dy)).collect();
            report(what, 400, times);
        }
        assert!(b.view().0 != 0);
    }
}

/// Swipes up and down through 2000 lines of history, back and forth, leave no more allocated than
/// the first ones did: the shaped rows and their cache stay a screenful.
#[test]
fn a_long_scroll_keeps_memory_flat() {
    let mut b = Bench::new(2000, line);
    let pass = |b: &mut Bench| {
        for dy in [300., -300.] {
            for _ in 0..30 {
                b.swipe(dy);
            }
        }
    };
    pass(&mut b);
    pass(&mut b);
    let grew = left_allocated(|| {
        for _ in 0..3 {
            pass(&mut b);
        }
    });
    eprintln!("3 passes through the history: {grew} bytes still allocated");
    assert!(grew < 1_000_000, "scrolling left {grew} bytes behind");
}
