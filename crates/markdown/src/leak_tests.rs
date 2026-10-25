//! A rendered document closed gives its parse, its text views and its images back.

use gpui_kit::{
    AnyView, AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, div,
};

use crate::{MarkdownBase, MarkdownView};

struct Host(Option<AnyView>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.0.clone())
    }
}

const DOC: &str = "---\ntitle: Notes\ntags: a\n---\n# Title\n\n> [!NOTE]\n> An **alert** with [a link](other.md).\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n```mermaid\ngraph TD; A-->B\n```\n\n$$\nx^2\n$$\n\n```rust\nfn main() {}\n```\n\n![logo](assets/logo.png)\n";

#[gpui_kit::test]
fn closing_a_document_gives_its_entities_back(cx: &mut TestAppContext) {
    gitviber_ui::test_support::config_dir("markdown-test");
    cx.update(gitviber_ui::init);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    for round in 0..4 {
        cx.update(|window, cx| {
            let base = MarkdownBase::file(std::env::temp_dir(), "docs/notes.md");
            let view = cx.new(|cx| {
                if round % 2 == 0 {
                    MarkdownView::new(DOC, base, window, cx)
                } else {
                    MarkdownView::inline(DOC, base, window, cx)
                }
            });
            host.update(cx, |h, cx| {
                h.0 = Some(view.into());
                cx.notify();
            });
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        // A find's marks and its text snapshot go with the view.
        let view = host.read_with(cx, |h, _| {
            h.0.clone().and_then(|v| v.downcast::<MarkdownView>().ok())
        });
        if let Some(view) = view {
            view.update(cx, |v, cx| {
                let query = crate::FindQuery {
                    text: "a".into(),
                    ..Default::default()
                };
                assert!(v.find(&query, cx).is_ok_and(|n| n > 0));
                v.step_match(1, cx);
            });
        }
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| {
            host.update(cx, |h, cx| {
                h.0 = None;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
}

/// A host's reader gives a document's images (a file at a revision): nothing is read from disk,
/// paths come relative to the repo, and the view still goes with its tab.
#[gpui_kit::test]
fn a_reader_gives_the_images_instead_of_the_disk(cx: &mut TestAppContext) {
    gitviber_ui::test_support::config_dir("markdown-test");
    cx.update(gitviber_ui::init);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let read: crate::ReadImage = {
        let asked = asked.clone();
        std::sync::Arc::new(move |path: &str| {
            asked.lock().unwrap().push(path.to_string());
            if path.ends_with(".svg") {
                return Ok(
                    br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"/>"#.to_vec(),
                );
            }
            let mut png = Vec::new();
            image::DynamicImage::new_rgba8(4, 4)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            Ok(png)
        })
    };
    cx.update(|window, cx| {
        // A root with nothing on disk: only the reader can give these.
        let base = MarkdownBase::file("/nonexistent-gitviber-root", "docs/notes.md");
        let doc = "![a](img/a.png)\n\n![b](../b.svg)\n";
        let view = cx.new(|cx| {
            let mut view = MarkdownView::new(doc, base, window, cx);
            view.read_images(Some(read), window, cx);
            view
        });
        host.update(cx, |h, cx| {
            h.0 = Some(view.into());
            cx.notify();
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let view = host
        .read_with(cx, |h, _| h.0.clone())
        .and_then(|v| v.downcast::<MarkdownView>().ok())
        .unwrap();
    let loaded = view.read_with(cx, |v, cx| v.images().read(cx).loaded());
    let mut paths = asked.lock().unwrap().clone();
    paths.sort();
    assert_eq!(paths, ["b.svg", "docs/img/a.png"]);
    assert_eq!(loaded, 2);
    drop(view);
    cx.update(|window, cx| {
        host.update(cx, |h, cx| {
            h.0 = None;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
}

/// Bytes allocated and not freed, for the test below: the shaped text a closed document leaves in
/// gpui-base's frame-to-frame table is invisible to the leak detector (no entity holds it).
struct Counting;

static LIVE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

// SAFETY: forwards to the system allocator, only counting.
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, std::sync::atomic::Ordering::Relaxed);
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        LIVE.fetch_sub(layout.size() as isize, std::sync::atomic::Ordering::Relaxed);
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(
            new_size as isize - layout.size() as isize,
            std::sync::atomic::Ordering::Relaxed,
        );
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// A paragraph per line, `i` in each so no two documents are the same text.
fn prose(i: usize) -> String {
    (0..12)
        .map(|p| format!("Paragraph {p} of document {i}, with **bold** and `code {i}` in it.\n\n"))
        .collect()
}

/// gpui-base keeps each painted paragraph's shaped text for the next frame. Unpatched it swept a
/// closed document's only past 4096 entries: these 200 documents' 2400 paragraphs all stayed
/// (vendor/gpui-base/PATCH.md): 12 MB of them stayed. Shaped by the real text system and drawn
/// by the real renderer, off screen: the test platform's keeps nothing worth measuring.
#[cfg(target_os = "macos")]
#[test]
fn closed_documents_leave_no_shaped_text_behind() {
    use gpui_kit::{HeadlessAppContext, px, size};
    gitviber_ui::test_support::config_dir("markdown-test");
    let text_system = std::sync::Arc::new(gpui_apple::AppleTextSystem::new());
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        std::sync::Arc::new(gitviber_ui::assets::Assets),
        gpui_snapshot_platform::current_headless_renderer,
    );
    cx.update(gitviber_ui::init);
    let slot: std::rc::Rc<std::cell::RefCell<Option<gpui_kit::Entity<Host>>>> = Default::default();
    let made = slot.clone();
    let window = cx
        .open_window(size(px(480.), px(360.)), move |window, cx| {
            let host = cx.new(|_| Host(None));
            *made.borrow_mut() = Some(host.clone());
            cx.new(|cx| gpui_kit::base::Root::new(AnyView::from(host), window, cx))
        })
        .unwrap();
    let host = slot.borrow_mut().take().unwrap();
    let frame = |cx: &mut HeadlessAppContext| {
        cx.run_until_parked();
        cx.advance_clock(std::time::Duration::from_millis(16));
        _ = cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx));
    };
    let cycle = |i: usize, cx: &mut HeadlessAppContext| {
        _ = cx.update_window(window.into(), |_, window, cx| {
            let view =
                cx.new(|cx| MarkdownView::new(&prose(i), MarkdownBase::default(), window, cx));
            host.update(cx, |h, cx| {
                h.0 = Some(view.into());
                cx.notify();
            });
        });
        frame(cx);
        frame(cx);
        // Drawn to pixels as an app's frame is.
        _ = cx.capture_screenshot(window.into());
        cx.update(|cx| {
            host.update(cx, |h, cx| {
                h.0 = None;
                cx.notify();
            })
        });
        frame(cx);
    };
    for i in 0..20 {
        cycle(i, &mut cx);
    }
    let before = LIVE.load(std::sync::atomic::Ordering::Relaxed);
    for i in 20..140 {
        cycle(i, &mut cx);
    }
    let grew = LIVE.load(std::sync::atomic::Ordering::Relaxed) - before;
    eprintln!("120 closed documents: {grew} bytes still allocated");
    // Unpatched: 12.9 MB; patched: 0.7 MB.
    assert!(
        grew < 3_000_000,
        "120 closed documents left {grew} bytes behind"
    );
}
