//! The tab as a window draws it, with no page view (a test has none to make): what the placer
//! makes of it, and the keys typed while its page area has the focus.

use super::*;
#[cfg(target_os = "macos")]
use gitviber_ui::actions::{changes, terminal, view, window};
#[cfg(target_os = "macos")]
use gitviber_ui::actions::{git, review, tab as tabs};
use gitviber_ui::keymap::ROOT_CONTEXT;
#[cfg(target_os = "macos")]
use gitviber_ui::Settings;
use gpui::{AnyView, KeyContext, TestAppContext, VisualTestContext};
use std::cell::RefCell;

fn setup(cx: &mut TestAppContext) {
    gitviber_ui::test_support::config_dir("browser-tab");
    cx.update(|cx| {
        gitviber_ui::init(cx);
        super::super::hub::install(cx, |_, _| {});
    });
}

/// The window's content: the tab or something else in its place, and maybe a menu over it.
struct Host {
    tab: Option<AnyView>,
    menu: bool,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .children(self.tab.clone())
            .when(self.menu, |d| {
                d.child(
                    div()
                        .id("menu")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .occlude(),
                )
            })
    }
}

/// A tab drawn as one whose page is made: its area reports where the page goes.
fn page_tab(ws: &Entity<Workspace>, window: &mut Window, cx: &mut App) -> Entity<BrowserTab> {
    let saved = Saved::of(&Selection::Browser {
        id: super::super::new_id(),
        url: "http://127.0.0.1:9/".into(),
        title: None,
        device: None,
        zoom: None,
        scheme: None,
    })
    .unwrap();
    let ws = ws.downgrade();
    cx.new(|cx| {
        let mut tab = BrowserTab::new(saved, "/tmp".into(), ws, window, cx);
        tab.made = Some(0);
        tab
    })
}

fn show(host: &Entity<Host>, tab: Option<&Entity<BrowserTab>>, cx: &mut VisualTestContext) {
    host.update(cx, |h, cx| {
        h.tab = tab.map(|t| t.clone().into());
        cx.notify();
    });
}

fn menu(host: &Entity<Host>, on: bool, cx: &mut VisualTestContext) {
    host.update(cx, |h, cx| {
        h.menu = on;
        cx.notify();
    });
}

/// A frame drawn, then the next one's callbacks, where the placer looks at what was drawn.
fn frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    cx.run_until_parked();
}

fn picture(cx: &mut VisualTestContext) -> Arc<RenderImage> {
    let svg = cx.update(|_, cx| cx.svg_renderer());
    let square = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4"/></svg>"#;
    Image::from_bytes(ImageFormat::Svg, square.to_vec())
        .to_image_data(svg)
        .unwrap()
}

fn shown(id: &str, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| placer::is_shown(id, cx))
}

/// A page whose tab stops being drawn hides though nothing of the browser paints (a maximized
/// terminal, a file's tab): it stayed on screen over them. A covered one's picture leaves the
/// atlas with it, and its element picker stops.
#[gpui::test]
fn a_page_out_of_sight_hides_with_nothing_of_the_browser_drawn(cx: &mut TestAppContext) {
    setup(cx);
    let ws = cx.update(|cx| cx.new(Workspace::new));
    let (host, cx) = cx.add_window_view(|_, _| Host {
        tab: None,
        menu: false,
    });
    let tab = cx.update(|window, cx| page_tab(&ws, window, cx));
    let id = tab.read_with(cx, |t, _| t.saved.id.clone());
    show(&host, Some(&tab), cx);
    frame(cx);
    assert!(shown(&id, cx));

    menu(&host, true, cx);
    frame(cx);
    assert!(!shown(&id, cx), "a menu over it");
    let image = picture(cx);
    tab.update(cx, |t, cx| {
        t.cover_came(Some(image.clone()), cx);
        t.picking = true;
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|window, _| window.has_image_atlas_entry(&image)));

    // Picked from the menu: the terminal maximized, no browser tab drawn.
    show(&host, None, cx);
    menu(&host, false, cx);
    ws.update(cx, |ws, cx| ws.set_terminal_maximized(true, cx));
    frame(cx);
    frame(cx);
    assert!(!shown(&id, cx));
    tab.read_with(cx, |t, _| {
        assert!(t.cover.is_none(), "no picture kept for a page out of sight");
        assert!(!t.picking, "the picker stopped");
    });
    assert!(!cx.update(|window, _| window.has_image_atlas_entry(&image)));

    // Its tab back: shown again.
    show(&host, Some(&tab), cx);
    frame(cx);
    assert!(shown(&id, cx));
}

/// A covered page's picture decoded once it showed again, or once its tab went out of sight,
/// isn't kept: it stood in under the page, or for a page no longer covered.
#[gpui::test]
fn a_cover_decoded_too_late_is_dropped(cx: &mut TestAppContext) {
    setup(cx);
    let ws = cx.update(|cx| cx.new(Workspace::new));
    let (host, cx) = cx.add_window_view(|_, _| Host {
        tab: None,
        menu: false,
    });
    let tab = cx.update(|window, cx| page_tab(&ws, window, cx));
    show(&host, Some(&tab), cx);
    frame(cx);
    menu(&host, true, cx);
    frame(cx);
    menu(&host, false, cx);
    frame(cx);
    let image = picture(cx);
    tab.update(cx, |t, cx| t.cover_came(Some(image.clone()), cx));
    assert!(tab.read_with(cx, |t, _| t.cover.is_none()), "shown again");

    menu(&host, true, cx);
    frame(cx);
    show(&host, None, cx);
    ws.update(cx, |ws, cx| ws.set_terminal_maximized(true, cx));
    frame(cx);
    frame(cx);
    tab.update(cx, |t, cx| t.cover_came(Some(image.clone()), cx));
    assert!(tab.read_with(cx, |t, _| t.cover.is_none()), "out of sight");
}

/// A tab opened on a page (a popup, one restored) gives the page the keys as it shows; a new
/// blank one its address bar. A popup's typed J ran Next Changed File.
#[gpui::test]
fn a_tab_opened_on_a_page_gives_it_the_keys(cx: &mut TestAppContext) {
    setup(cx);
    let ws = cx.update(|cx| cx.new(Workspace::new));
    let (_, cx) = cx.add_window_view(|_, _| Host {
        tab: None,
        menu: false,
    });
    let tab = cx.update(|window, cx| page_tab(&ws, window, cx));
    tab.read_with(cx, |t, cx| {
        assert_eq!(t.tab_focus(cx).as_ref(), Some(&t.page_focus));
    });
    let blank = cx.update(|window, cx| {
        let saved = Saved::of(&Selection::Browser {
            id: super::super::new_id(),
            url: BLANK.into(),
            title: None,
            device: None,
            zoom: None,
            scheme: None,
        })
        .unwrap();
        let ws = ws.downgrade();
        cx.new(|cx| BrowserTab::new(saved, "/tmp".into(), ws, window, cx))
    });
    blank.read_with(cx, |t, cx| {
        assert_eq!(t.tab_focus(cx).as_ref(), Some(&t.focus));
    });
}

/// A pick's mark stays inside the device's screen in device mode, not the whole page area.
#[gpui::test]
fn a_picks_mark_stays_on_the_devices_screen(cx: &mut TestAppContext) {
    setup(cx);
    let ws = cx.update(|cx| cx.new(Workspace::new));
    let (_, cx) = cx.add_window_view(|_, _| Host {
        tab: None,
        menu: false,
    });
    let tab = cx.update(|window, cx| page_tab(&ws, window, cx));
    tab.update(cx, |t, _| {
        t.area.set((1200., 900.));
        assert_eq!(t.mark_room(), (1200., 900.));
        t.saved.device = Some(devices::DeviceChoice::default_device());
        let (_, _, f) = t.fitted().unwrap();
        assert_eq!(t.mark_room(), (f.page.w as f32, f.page.h as f32));
        assert!(t.mark_room().0 < 600.);
    });
}

/// A page area with GPUI's focus, in its tab under the app's root, as a browser tab draws it.
struct Keys {
    page: FocusHandle,
}

impl Render for Keys {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut page = KeyContext::default();
        page.add(PAGE_CONTEXT);
        page.set("page", "p1");
        div().key_context(ROOT_CONTEXT).size_full().child(
            div()
                .key_context(CONTEXT)
                .size_full()
                .child(div().key_context(page).track_focus(&self.page).size_full()),
        )
    }
}

fn record<A: gpui::Action>(name: &'static str, ran: &Rc<RefCell<Vec<&'static str>>>, cx: &mut App) {
    let ran = ran.clone();
    cx.on_action(move |_: &A, _| ran.borrow_mut().push(name));
}

/// A window whose page area has the focus, the keys typed in it going through `page_key`.
fn typing_in_a_page(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(|cx| cx.intercept_keystrokes(page_key).detach());
    let (view, cx) = cx.add_window_view(|_, cx| Keys {
        page: cx.focus_handle(),
    });
    cx.update(|window, cx| {
        let page = view.read(cx).page.clone();
        page.focus(window, cx);
        window.activate_window();
    });
    cx.run_until_parked();
    cx
}

/// Keys typed for a page run only the chords of the app's commands, as the keymap has them now:
/// not a single key (J, S), not an editing chord bound elsewhere (⌘Z is Undo Git Action, ⌘⌫
/// Discard), not a key GPUI reads as no command's (a German ⌘Ü).
// Pages are WebKit views on macOS only; the chords here are ⌘ ones.
#[cfg(target_os = "macos")]
#[gpui::test]
fn keys_in_a_page_are_the_apps_only_for_its_commands(cx: &mut TestAppContext) {
    setup(cx);
    let ran = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        record::<review::NextFile>("next file", &ran, cx);
        record::<git::ToggleStage>("stage", &ran, cx);
        record::<review::AddNote>("note", &ran, cx);
        record::<git::Undo>("undo", &ran, cx);
        record::<git::Redo>("redo", &ran, cx);
        record::<git::Discard>("discard", &ran, cx);
        record::<tabs::Close>("close", &ran, cx);
    });
    let cx = typing_in_a_page(cx);
    for k in [
        "j",
        "s",
        "c",
        "cmd-z",
        "shift-cmd-z",
        "cmd-backspace",
        "cmd-c",
        "cmd-a",
        "cmd-ü",
    ] {
        cx.simulate_keystrokes(k);
    }
    assert!(ran.borrow().is_empty(), "{:?}", ran.borrow());
    cx.simulate_keystrokes("cmd-w");
    assert_eq!(*ran.borrow(), ["close"]);

    // Rebound, a command's new chord reaches it from a page, its old one doesn't.
    cx.update(|_, cx| {
        Settings::update(cx, |s| {
            s.keybindings
                .insert("tab.close".into(), vec!["alt+cmd+u".into()]);
        })
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-w alt-cmd-u");
    assert_eq!(*ran.borrow(), ["close", "close"]);
}

// These three read chords as macOS has them: elsewhere ⌃ is the Super key, Minimize unbound.

/// A ⌃ chord one of the app's commands has leaves a page (⌃1, ⌃Tab, ⌃`, ⌃⌘C); ⌃ with a letter
/// edits the page's text field (⌃A, ⌃K), and no command has the rest.
#[cfg(target_os = "macos")]
#[gpui::test]
fn a_bound_ctrl_chord_reaches_the_app_from_a_page(cx: &mut TestAppContext) {
    setup(cx);
    let ran = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        record::<view::Changes>("changes", &ran, cx);
        record::<tabs::Next>("next tab", &ran, cx);
        record::<terminal::Toggle>("terminal", &ran, cx);
        record::<changes::FocusMessage>("message", &ran, cx);
    });
    let cx = typing_in_a_page(cx);
    for k in [
        "ctrl-a",
        "ctrl-e",
        "ctrl-k",
        "ctrl-z",
        "ctrl-shift-a",
        "ctrl-shift-e",
        "ctrl-[",
        "ctrl-left",
        "ctrl-enter",
        "ctrl-space",
        "ctrl-cmd-a",
        "ctrl-cmd-v",
        "ctrl-cmd-z",
    ] {
        cx.simulate_keystrokes(k);
    }
    assert!(ran.borrow().is_empty(), "{:?}", ran.borrow());
    cx.simulate_keystrokes("ctrl-1 ctrl-tab ctrl-` ctrl-cmd-c");
    assert_eq!(
        *ran.borrow(),
        ["changes", "next tab", "terminal", "message"]
    );
}

/// ⇧ with a symbol comes as the symbol it types (⇧⌘] is ⌘}): Next Tab, not ⌘]'s Go Forward;
/// ⇧⌘= zooms the page.
#[cfg(target_os = "macos")]
#[gpui::test]
fn shifted_punctuation_reaches_the_command_bound_to_it(cx: &mut TestAppContext) {
    setup(cx);
    let ran = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        record::<tabs::Next>("next tab", &ran, cx);
        record::<tabs::Prev>("prev tab", &ran, cx);
        record::<act::Forward>("forward", &ran, cx);
        record::<act::ZoomIn>("zoom in", &ran, cx);
    });
    let cx = typing_in_a_page(cx);
    cx.simulate_keystrokes("cmd-} cmd-{ cmd-] cmd-+");
    assert_eq!(
        *ran.borrow(),
        ["next tab", "prev tab", "forward", "zoom in"]
    );
}

/// ⇧⌘M is Toggle Device Mode from a page; ⌘M stays macOS's Minimize, and ⌥⇧⌘M, no one's, the
/// page's.
#[cfg(target_os = "macos")]
#[gpui::test]
fn device_mode_leaves_a_page_while_minimize_stays_the_systems(cx: &mut TestAppContext) {
    setup(cx);
    let ran = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        record::<act::ToggleDevice>("device", &ran, cx);
        record::<window::Minimize>("minimize", &ran, cx);
    });
    let cx = typing_in_a_page(cx);
    cx.simulate_keystrokes("alt-shift-cmd-m");
    assert!(ran.borrow().is_empty(), "{:?}", ran.borrow());
    cx.simulate_keystrokes("shift-cmd-m cmd-m");
    assert_eq!(*ran.borrow(), ["device", "minimize"]);
}

/// A page named by its title has its host beside it; by its address till it has one.
#[gpui::test]
fn a_titled_pages_tab_shows_its_host_beside_the_title(cx: &mut TestAppContext) {
    setup(cx);
    let ws = cx.update(|cx| cx.new(Workspace::new));
    let (_, cx) = cx.add_window_view(|_, _| Host {
        tab: None,
        menu: false,
    });
    let tab = |title: Option<&str>, cx: &mut VisualTestContext| {
        let saved = Saved::of(&Selection::Browser {
            id: super::super::new_id(),
            url: "http://localhost:5173/docs".into(),
            title: title.map(Into::into),
            device: None,
            zoom: None,
            scheme: None,
        })
        .unwrap();
        let ws = ws.downgrade();
        let tab = cx.update(|window, cx| {
            cx.new(|cx| BrowserTab::new(saved, "/tmp".into(), ws, window, cx))
        });
        tab.read_with(cx, |t, cx| (t.tab_title(cx), t.tab_detail(cx)))
    };
    assert_eq!(
        tab(Some("Docs"), cx),
        ("Docs".into(), Some("localhost:5173".into()))
    );
    assert_eq!(tab(None, cx), ("localhost:5173/docs".into(), None));
}
