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
    let dir = std::env::temp_dir().join(format!("gitviber-browser-test-{}", std::process::id()));
    // SAFETY: the tests that read the config dir all point it at a temporary folder.
    unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
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
