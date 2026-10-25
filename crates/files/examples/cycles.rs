//! Memory of opening a big file's tab, paging down through it and closing it, again and again,
//! drawn off screen: what the code view keeps after its tab is gone shows up as growth.
//!
//! files-cycles REPO [PATH] [N] [PAGES] (huge.rs, 20 cycles of 300 PageDowns) prints the
//! footprint, malloc's bytes in use and the live views at each step, then after 10 s idle. It
//! runs with MallocLargeCache=0, as the app does.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gitviber_files::OpenOptions;
use gitviber_ui::memory;
use gitviber_ui::workspace::Workspace;
use gpui_kit::{
    AppContext as _, Context, Entity, HeadlessAppContext, IntoElement, Keystroke,
    ParentElement as _, Render, Styled as _, Window, WindowHandle, div, px, size,
};

/// Draws the workspace's active tab, as the app's center does.
struct Host(Entity<Workspace>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self
            .0
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle())
            .map(|h| h.view());
        div().size_full().children(view)
    }
}

/// The footprint, and malloc's bytes in use: pages malloc marked reusable and then reused
/// aren't counted in the footprint again, so a reopened view can look free.
fn footprint() -> String {
    format!(
        "{} (heap {})",
        memory::footprint()
            .map(memory::format_bytes)
            .unwrap_or_default(),
        memory::format_bytes(heap_in_use())
    )
}

#[repr(C)]
#[derive(Default)]
struct MallocStatistics {
    blocks_in_use: u32,
    size_in_use: usize,
    max_size_in_use: usize,
    size_allocated: usize,
}

unsafe extern "C" {
    fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut MallocStatistics);
}

fn heap_in_use() -> u64 {
    let mut stats = MallocStatistics::default();
    // SAFETY: a null zone sums every zone into `stats`, which the call fills.
    unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut stats) };
    stats.size_in_use as u64
}

fn live() -> String {
    let live: Vec<String> = memory::live_counts()
        .iter()
        .map(|(k, n)| format!("{k}={n}"))
        .collect();
    format!("{} [{}]", footprint(), live.join(" "))
}

/// Lets core's threads answer and draws frames, for `for_` of real time (one at least).
fn frames(cx: &mut HeadlessAppContext, window: WindowHandle<Host>, for_: Duration) {
    let tick = Duration::from_millis(16);
    let until = Instant::now() + for_;
    loop {
        cx.run_until_parked();
        cx.advance_clock(tick);
        _ = cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx));
        if Instant::now() >= until {
            break;
        }
        std::thread::sleep(tick);
    }
}

fn main() {
    gitviber_core::malloc::return_freed_blocks();
    let mut args = std::env::args().skip(1);
    let repo = std::path::PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let path = args.next().unwrap_or_else(|| "huge.rs".into());
    let n: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(20);
    let pages: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(300);
    gitviber_ui::store::use_config_dir(std::env::temp_dir().join("gitviber-dev"));
    let text_system = gpui_snapshot_platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(gitviber_ui::assets::Assets),
        gpui_snapshot_platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gitviber_ui::init(cx);
        gitviber_ui::start_core(cx);
        gitviber_ui::windows::install_app_actions(cx);
        gitviber_files::init(cx);
        gitviber_diff::init(cx);
    });
    let slot: Rc<RefCell<Option<Entity<Workspace>>>> = Rc::default();
    let made = slot.clone();
    let window = cx
        .open_window(size(px(1100.), px(760.)), move |window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            ws.update(cx, |ws, cx| ws.open_project(repo, window, cx));
            *made.borrow_mut() = Some(ws.clone());
            cx.new(|_| Host(ws))
        })
        .expect("a window");
    let ws = slot.borrow_mut().take().expect("the workspace");
    frames(&mut cx, window, Duration::from_secs(2));
    println!("before: {}", live());
    let weak = ws.downgrade();
    let page = Keystroke::parse("pagedown").unwrap();
    for i in 1..=n {
        _ = cx.update_window(window.into(), |_, window, cx| {
            let focus = OpenOptions {
                focus: true,
                ..Default::default()
            };
            gitviber_files::open_file(&weak, &path, focus, window, cx)
        });
        // The text is read, then parsed for colors in the background.
        frames(&mut cx, window, Duration::from_secs(2));
        let open = live();
        // Into the editor, which wasn't there yet when the tab took focus.
        _ = cx.update_window(window.into(), |_, window, cx| {
            ws.update(cx, |ws, cx| ws.focus_center(window, cx))
        });
        for _ in 0..pages {
            _ = cx.update_window(window.into(), |_, window, cx| {
                window.dispatch_keystroke(page.clone(), cx)
            });
            frames(&mut cx, window, Duration::ZERO);
        }
        frames(&mut cx, window, Duration::from_millis(300));
        let scrolled = footprint();
        cx.update(|cx| ws.update(cx, |ws, cx| ws.close_active_tab(cx)));
        frames(&mut cx, window, Duration::from_millis(500));
        println!(
            "cycle {i}: open {open}, scrolled {scrolled}, closed {}",
            live()
        );
    }
    frames(&mut cx, window, Duration::from_secs(10));
    println!("10 s after: {}", live());
}
