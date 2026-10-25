//! A browser tab closed gives its memory back: no task, subscription or callback keeps its
//! view, its fields or its page's entry alive. Off the main thread no page view is made, so
//! this is the GPUI side's own bookkeeping.

use super::hub::Hub;
use super::tab::{BrowserTab, Saved};
use super::url::BLANK;
use gitviber_ui::workspace::{Selection, Workspace};
use gpui::{
    div, AnyView, AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, WeakEntity, Window,
};

struct Host(Option<AnyView>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.0.clone())
    }
}

fn setup(cx: &mut TestAppContext) {
    gitviber_ui::test_support::config_dir("browser-test");
    cx.update(|cx| {
        gitviber_ui::init(cx);
        super::hub::install(cx, |_, _| {});
    });
}

fn saved(url: &str) -> Saved {
    Saved::of(&Selection::Browser {
        id: super::new_id(),
        url: url.into(),
        title: None,
        device: None,
        zoom: None,
        scheme: None,
    })
    .unwrap()
}

#[gpui::test]
fn closing_browser_tabs_gives_their_entities_back(cx: &mut TestAppContext) {
    setup(cx);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    for round in 0..4 {
        cx.update(|window, cx| {
            let url = if round % 2 == 0 {
                BLANK
            } else {
                "http://127.0.0.1:9/"
            };
            let tab = cx.new(|cx| {
                BrowserTab::new(
                    saved(url),
                    "/tmp".into(),
                    WeakEntity::<Workspace>::new_invalid(),
                    window,
                    cx,
                )
            });
            host.update(cx, |h, cx| {
                h.0 = Some(tab.clone().into());
                cx.notify();
            });
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            host.update(cx, |h, cx| {
                h.0 = None;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    cx.update(|_, cx| {
        cx.assert_no_new_leaks(&snapshot);
        let hub = Hub::global(cx).unwrap();
        assert_eq!(
            hub.read(cx).page_count(),
            0,
            "a closed tab's page is forgotten"
        );
    });
}

/// ⇧⌘B's command comes while its window is busy with the key: it runs once that's over.
#[gpui::test]
fn a_command_from_a_key_runs_in_the_window_in_front(cx: &mut TestAppContext) {
    use gitviber_ui::actions::browser as act;
    use std::cell::Cell;
    use std::rc::Rc;
    let ran = Rc::new(Cell::new(0));
    let seen = ran.clone();
    cx.update(|cx| {
        cx.bind_keys([gpui::KeyBinding::new("cmd-shift-b", act::Open, None)]);
        cx.on_action(move |_: &act::Open, cx| {
            let seen = seen.clone();
            super::in_front(cx, move |_, _| seen.set(seen.get() + 1));
        });
    });
    let (_, cx) = cx.add_window_view(|_, _| Host(None));
    cx.update(|window, _| window.activate_window());
    cx.simulate_keystrokes("cmd-shift-b");
    cx.run_until_parked();
    assert_eq!(ran.get(), 1);
}

struct Picture(std::sync::Arc<gpui::RenderImage>);

impl Render for Picture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::img(self.0.clone()).size(gpui::px(4.))
    }
}

/// A page's picture dropped while its window is being updated (the page came back as it was
/// drawn) leaves that window's atlas: dropped then, it stayed (90 MB over ten closed tabs).
#[gpui::test]
fn a_picture_dropped_in_its_windows_update_leaves_the_atlas(cx: &mut TestAppContext) {
    let svg = cx.update(|cx| cx.svg_renderer());
    let square = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4"/></svg>"#;
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Svg, square.to_vec())
        .to_image_data(svg)
        .unwrap();
    let (_, cx) = cx.add_window_view(|_, _| Picture(image.clone()));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|window, _| window.has_image_atlas_entry(&image)));
    cx.update(|_, cx| super::tab::drop_picture(image.clone(), cx));
    cx.run_until_parked();
    assert!(!cx.update(|window, _| window.has_image_atlas_entry(&image)));
}

#[test]
fn a_tab_saves_only_what_it_can_load_again() {
    let s = Saved::of(&Selection::Browser {
        id: "a".into(),
        url: "javascript:alert(1)".into(),
        title: Some("x".into()),
        device: Some(serde_json::json!({ "name": "Nokia 3310" })),
        zoom: Some(1.3),
        scheme: Some("dark".into()),
    })
    .unwrap();
    assert_eq!(s.url, BLANK);
    assert_eq!(s.device, None);
    assert_eq!(s.zoom, 1.0);
    let back = s.selection();
    let Selection::Browser { scheme, zoom, .. } = &back else {
        unreachable!()
    };
    assert_eq!(scheme.as_deref(), Some("dark"));
    assert_eq!(*zoom, None);
    let phone = Saved::of(&Selection::Browser {
        id: "b".into(),
        url: "http://localhost:5173/".into(),
        title: None,
        device: Some(serde_json::json!({ "name": "iPhone 16 Pro", "rotated": true })),
        zoom: Some(1.25),
        scheme: None,
    })
    .unwrap();
    assert!(phone.device.as_ref().is_some_and(|d| d.rotated));
    assert_eq!(Saved::of(&phone.selection()).unwrap(), phone);
}

#[test]
fn ids_look_like_uuids_and_differ() {
    let (a, b) = (super::new_id(), super::new_id());
    assert_ne!(a, b);
    assert_eq!(a.len(), 36);
    assert_eq!(a.matches('-').count(), 4);
}
