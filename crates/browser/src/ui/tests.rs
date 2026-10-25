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

/// With Show agents' tabs off, an agent's page keeps out of the strip until the user takes it or
/// its pane closes; the user's own pages always show.
#[gpui::test]
fn an_agents_tab_hides_unless_shown_taken_or_let_go(cx: &mut TestAppContext) {
    use gitviber_ui::workspace::TabItem as _;
    use gitviber_ui::Settings;
    setup(cx);
    let (_host, cx) = cx.add_window_view(|_, _| Host(None));
    let mut tab = |agent: Option<u32>, taken: bool, released: bool| {
        cx.update(|window, cx| {
            let saved = saved(BLANK);
            let id = saved.id.clone();
            let tab = cx.new(|cx| {
                BrowserTab::new(
                    saved,
                    "/tmp".into(),
                    WeakEntity::<Workspace>::new_invalid(),
                    window,
                    cx,
                )
            });
            Hub::global(cx).unwrap().update(cx, |h, _| {
                let page = h.page_mut(&id);
                page.agent = agent;
                page.taken = taken;
                page.released = released;
            });
            tab
        })
    };
    let tabs = [
        tab(Some(1), false, false),
        tab(Some(1), true, false),
        tab(Some(2), false, true),
        tab(None, false, false),
    ];
    let hidden = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| {
            tabs.iter()
                .map(|t| t.read(cx).hidden(cx))
                .collect::<Vec<_>>()
        })
    };
    cx.update(|_, cx| Settings::update(cx, |s| s.browser_show_agent_tabs = false));
    assert_eq!(hidden(cx), [true, false, false, false]);
    cx.update(|_, cx| Settings::update(cx, |s| s.browser_show_agent_tabs = true));
    assert_eq!(hidden(cx), [false; 4]);
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
fn a_saved_device_or_scheme_is_taken_only_when_this_build_knows_it() {
    use super::devices::{device_of, RESPONSIVE_MAX, RESPONSIVE_MIN};
    use super::tab::Scheme;
    use serde_json::json;
    let of = |device: serde_json::Value, scheme: &str| {
        Saved::of(&Selection::Browser {
            id: "a".into(),
            url: BLANK.into(),
            title: None,
            device: Some(device),
            zoom: None,
            scheme: Some(scheme.into()),
        })
        .unwrap()
    };
    for no in [
        json!(null),
        json!("iPhone 16"),
        json!({}),
        json!({ "name": 1 }),
        json!({ "name": "iPhone 16", "rotated": "yes" }),
        json!({ "name": "iPhone 16", "w": "9" }),
        json!({ "name": "Gone Phone" }),
    ] {
        assert_eq!(of(no.clone(), "dark").device, None, "{no}");
    }
    for ok in [
        json!({ "name": "iPhone 16" }),
        json!({ "name": "iPhone 16", "rotated": true }),
        json!({ "name": "Responsive", "w": 320, "h": 640 }),
    ] {
        assert!(of(ok.clone(), "dark").device.is_some(), "{ok}");
    }
    // Past what a field or a drag gives is still a choice: its sides are bounded where it's used.
    let far = of(json!({ "name": "Responsive", "w": 1e6, "h": -5 }), "dark");
    let d = device_of(&far.device.unwrap()).unwrap();
    assert_eq!((d.w, d.h), (RESPONSIVE_MAX, RESPONSIVE_MIN));
    assert_eq!(of(json!({}), "light").scheme, Some(Scheme::Light));
    assert_eq!(of(json!({}), "auto").scheme, None);
}

#[test]
fn ids_look_like_uuids_and_differ() {
    let (a, b) = (super::new_id(), super::new_id());
    assert_ne!(a, b);
    assert_eq!(a.len(), 36);
    assert_eq!(a.matches('-').count(), 4);
}
