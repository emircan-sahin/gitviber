//! The panels' keys on the real window, under the VS Code preset: each panel has one key that
//! shows or hides it and one that focuses it. Every key is pressed from every place focus can be,
//! with its panel hidden, shown with focus elsewhere, and shown with focus in it, and the panel's
//! visibility and where focus lands are checked. The views are stand-ins with the app's key
//! contexts (a WebKit page can't be made in a test: its area's context stands in for it).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gitviber_ui::keymap::{Preset, TERMINAL_CONTEXT, browser, find, markdown, terminal};
use gitviber_ui::workspace::{Panel, PanelSpec, Part, Slot, TabItem, Workspace, register_panel};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{Editor, EditorState, Input, InputState};
use gpui_kit::{
    Action, App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, Global,
    InteractiveElement as _, IntoElement, KeyContext, ParentElement as _, Render, SharedString,
    Styled as _, TestAppContext, VisualTestContext, Window, div, px, size,
};

/// Where focus can be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum At {
    Code,
    FindBar,
    Page,
    Doc,
    CommitBox,
    GitList,
    Explorer,
    Search,
    Pane,
    /// The command palette, opened from the code view.
    Palette,
}

/// The places a key is pressed from.
const STARTS: [At; 9] = [
    At::Code,
    At::FindBar,
    At::CommitBox,
    At::Pane,
    At::Page,
    At::Doc,
    At::Explorer,
    At::GitList,
    At::Palette,
];

impl At {
    /// The panel it's in, and that panel's tab; None: the code area or the palette.
    fn panel(self) -> Option<(Slot, &'static str)> {
        match self {
            At::CommitBox | At::GitList => Some((Slot::Left, "changes")),
            At::Explorer => Some((Slot::Right, "explorer")),
            At::Search => Some((Slot::Right, "search")),
            At::Pane => Some((Slot::Bottom, "terminal")),
            _ => None,
        }
    }

    /// The code area's tab it's in.
    fn tab(self) -> Option<&'static str> {
        match self {
            At::Code | At::FindBar | At::Palette => Some("code"),
            At::Page => Some("page"),
            At::Doc => Some("doc"),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// Shows or hides the panel.
    Visibility,
    /// Focuses the panel (on `tab`), or gives focus back.
    Focus,
}

struct Key {
    keys: &'static str,
    kind: Kind,
    slot: Slot,
    tab: Option<&'static str>,
}

const fn key(keys: &'static str, kind: Kind, slot: Slot, tab: Option<&'static str>) -> Key {
    Key {
        keys,
        kind,
        slot,
        tab,
    }
}

/// The VS Code preset's.
const VSCODE: [Key; 9] = [
    key("cmd-e", Kind::Visibility, Slot::Right, None),
    key("alt-cmd-b", Kind::Visibility, Slot::Right, None),
    key("shift-cmd-e", Kind::Focus, Slot::Right, Some("explorer")),
    key("shift-cmd-f", Kind::Focus, Slot::Right, Some("search")),
    key("cmd-b", Kind::Visibility, Slot::Left, None),
    key("cmd-0", Kind::Focus, Slot::Left, None),
    key("ctrl-shift-g", Kind::Focus, Slot::Left, Some("changes")),
    key("cmd-j", Kind::Visibility, Slot::Bottom, None),
    key("ctrl-`", Kind::Focus, Slot::Bottom, None),
];

/// The classic preset's: its own keys, the same rules.
const CLASSIC: [Key; 8] = [
    key("alt-cmd-b", Kind::Visibility, Slot::Right, None),
    key("shift-cmd-e", Kind::Focus, Slot::Right, Some("explorer")),
    key("shift-cmd-f", Kind::Focus, Slot::Right, Some("search")),
    key("cmd-b", Kind::Visibility, Slot::Left, None),
    key("shift-cmd-g", Kind::Focus, Slot::Left, None),
    key("ctrl-1", Kind::Focus, Slot::Left, Some("changes")),
    key("cmd-j", Kind::Visibility, Slot::Bottom, None),
    key("ctrl-`", Kind::Focus, Slot::Bottom, None),
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Hidden,
    /// Shown, focus elsewhere.
    Shown,
    /// Shown, focus in it, come there by a click from where the key was pressed.
    Focused,
}

/// What a key is to leave: whether its panel shows, where focus is, the panel's tab.
#[derive(Debug, PartialEq)]
struct Outcome {
    shown: bool,
    focus: Option<At>,
    tab: Option<&'static str>,
}

/// Where a place has its own command on the key, it wins there: ⌘0 resets the terminal's font,
/// a page's or a document's zoom (the VS Code preset's Focus Git Panel), and the classic ⇧⌘G is
/// Find Previous in the code and a document.
fn place_keeps(key: &Key, at: At) -> bool {
    match key.keys {
        "cmd-0" => matches!(at, At::Pane | At::Page | At::Doc),
        "shift-cmd-g" => matches!(at, At::Code | At::FindBar | At::Doc),
        _ => false,
    }
}

/// Where a focus key puts focus in its panel: the tab's own place.
fn panel_place(key: &Key) -> At {
    match (key.slot, key.tab) {
        (Slot::Left, _) => At::GitList,
        (Slot::Right, Some("search")) => At::Search,
        (Slot::Right, _) => At::Explorer,
        (Slot::Bottom, _) => At::Pane,
    }
}

#[derive(Default)]
struct Places(HashMap<At, FocusHandle>);
impl Global for Places {}

fn note(at: At, handle: FocusHandle, cx: &mut App) {
    cx.default_global::<Places>().0.insert(at, handle);
}

fn place(at: At, cx: &App) -> FocusHandle {
    cx.global::<Places>().0[&at].clone()
}

struct CodeTab {
    code: Entity<EditorState>,
    find: Entity<InputState>,
}

impl Render for CodeTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // As the code view, which steps its find with ⇧⌘G whether it shows or not.
        div()
            .key_context("CodeView")
            .size_full()
            .on_action(|_: &find::Prev, _, _| {})
            .child(div().h(px(120.)).child(Editor::new(&self.code)))
            .child(
                div()
                    .key_context("CodeOverlay")
                    .child(Input::new(&self.find)),
            )
    }
}

impl TabItem for CodeTab {
    fn tab_title(&self, _: &App) -> SharedString {
        "code".into()
    }
    fn tab_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(self.code.focus_handle(cx))
    }
}

/// A view that is one focusable area in `context`: a document, the explorer, a terminal pane.
/// `own` are the place's own commands it handles there (its zoom reset), as its view does.
struct Area {
    context: KeyContext,
    focus: FocusHandle,
    own: Vec<Box<dyn Action>>,
}

impl Area {
    fn new(context: &str, own: Vec<Box<dyn Action>>, cx: &mut Context<Self>) -> Self {
        Area {
            context: KeyContext::parse(context).unwrap(),
            focus: cx.focus_handle(),
            own,
        }
    }
}

impl Render for Area {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let area = div()
            .key_context(self.context.clone())
            .track_focus(&self.focus)
            .size_full();
        self.own
            .iter()
            .fold(area, |area, own| area.on_boxed_action(&**own, |_, _, _| {}))
    }
}

impl TabItem for Area {
    fn tab_title(&self, _: &App) -> SharedString {
        "area".into()
    }
    fn tab_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }
}

impl Panel for Area {
    fn panel_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }
}

/// A browser tab: its page's area, with the page's id, in the tab's own context.
struct PageTab {
    page: FocusHandle,
}

impl Render for PageTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut page = KeyContext::default();
        page.add("BrowserPage");
        page.set("page", "p1");
        div()
            .key_context("Browser")
            .size_full()
            .on_action(|_: &browser::ZoomReset, _, _| {})
            .child(div().key_context(page).track_focus(&self.page).size_full())
    }
}

impl TabItem for PageTab {
    fn tab_title(&self, _: &App) -> SharedString {
        "page".into()
    }
    fn tab_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.page.clone())
    }
}

/// The git panel's Changes: its file list with the commit box under it.
struct GitPanel {
    list: FocusHandle,
    summary: Entity<InputState>,
}

impl Render for GitPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("ChangesList")
            .track_focus(&self.list)
            .size_full()
            .child(
                div()
                    .key_context("CommitMessage")
                    .child(Input::new(&self.summary)),
            )
    }
}

impl Panel for GitPanel {
    fn panel_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.list.clone())
    }
}

/// The explorer's Search tab: its query box.
struct SearchPanel {
    query: Entity<InputState>,
}

impl Render for SearchPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("SearchPanel")
            .size_full()
            .child(Input::new(&self.query))
    }
}

impl Panel for SearchPanel {
    fn panel_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(self.query.focus_handle(cx))
    }
}

/// The terminal panel with one pane.
struct TerminalPanel {
    pane: Entity<Area>,
}

impl Render for TerminalPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(gitviber_terminal::PANEL_CONTEXT)
            .size_full()
            .child(self.pane.clone())
    }
}

impl Panel for TerminalPanel {
    fn panel_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(self.pane.read(cx).focus.clone())
    }
}

fn register_panels(cx: &mut App) {
    register_panel(
        cx,
        PanelSpec::new("changes", Slot::Left, "Changes", |_, window, cx| {
            let panel = cx.new(|cx| GitPanel {
                list: cx.focus_handle(),
                summary: cx.new(|cx| InputState::new(window, cx)),
            });
            let (list, summary) = {
                let p = panel.read(cx);
                (p.list.clone(), p.summary.focus_handle(cx))
            };
            note(At::GitList, list, cx);
            note(At::CommitBox, summary, cx);
            panel
        }),
    );
    register_panel(
        cx,
        PanelSpec::new("explorer", Slot::Right, "Explorer", |_, _, cx| {
            let panel = cx.new(|cx| Area::new("Explorer", Vec::new(), cx));
            let focus = panel.read(cx).focus.clone();
            note(At::Explorer, focus, cx);
            panel
        }),
    );
    register_panel(
        cx,
        PanelSpec::new("search", Slot::Right, "Search", |_, window, cx| {
            let panel = cx.new(|cx| SearchPanel {
                query: cx.new(|cx| InputState::new(window, cx)),
            });
            let focus = panel.read(cx).query.focus_handle(cx);
            note(At::Search, focus, cx);
            panel
        })
        .order(1),
    );
    register_panel(
        cx,
        PanelSpec::new("terminal", Slot::Bottom, "Terminal", |_, _, cx| {
            let reset: Box<dyn Action> = Box::new(terminal::FontZoomReset);
            let pane = cx.new(|cx| Area::new(TERMINAL_CONTEXT, vec![reset], cx));
            note(At::Pane, pane.read(cx).focus.clone(), cx);
            cx.new(|_| TerminalPanel { pane })
        })
        .per_window(),
    );
}

/// The app's keymap under `preset`, the feature crates' keys and the stand-in panels; the
/// project to open, a repository (a plain folder's git panel offers `git init` instead).
fn init(preset: Preset, cx: &mut TestAppContext) -> PathBuf {
    let one = super::SETUP.lock().unwrap_or_else(|e| e.into_inner());
    let dir = gitviber_ui::test_support::config_dir(&format!("app-panels-{}", preset.as_str()));
    let settings = format!(r#"{{"keymapPreset": "{}"}}"#, preset.as_str());
    std::fs::write(dir.join("settings.json"), settings).unwrap();
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let git = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&project)
        .status();
    assert!(git.is_ok_and(|s| s.success()), "git init");
    cx.update(|cx| {
        gitviber_ui::init(cx);
        gitviber_ui::install_core(cx);
        gitviber_changes::init(cx);
        gitviber_terminal::init(cx);
        gitviber_files::init(cx);
        register_panels(cx);
    });
    drop(one);
    project
}

/// A new main window on `project`, every panel shown, with three tabs: code (with its find
/// bar), a page, a document. A window of its own per case: nothing one case did is left for the
/// next to find.
fn open(project: &Path, cx: &mut TestAppContext) -> VisualTestContext {
    let before = cx.update(|cx| cx.windows());
    let project = project.to_path_buf();
    cx.update(|cx| gitviber_app::open_main_window(Some(project), cx));
    let window = cx
        .update(|cx| cx.windows())
        .into_iter()
        .find(|w| !before.contains(w))
        .unwrap();
    let mut cx = VisualTestContext::from_window(window, cx);
    cx.simulate_resize(size(px(1400.), px(900.)));
    cx.update(|window, cx| {
        window.activate_window();
        let ws = Workspace::for_window(window, cx).unwrap();
        ws.update(cx, |ws, cx| {
            ws.open_tab(
                "code",
                false,
                |window, cx| {
                    let tab = cx.new(|cx| CodeTab {
                        code: cx.new(|cx| EditorState::new(window, cx)),
                        find: cx.new(|cx| InputState::new(window, cx)),
                    });
                    let t = tab.read(cx);
                    let (code, find) = (t.code.focus_handle(cx), t.find.focus_handle(cx));
                    note(At::Code, code, cx);
                    note(At::FindBar, find, cx);
                    tab
                },
                window,
                cx,
            );
            ws.open_tab(
                "page",
                false,
                |_, cx| {
                    let tab = cx.new(|cx| PageTab {
                        page: cx.focus_handle(),
                    });
                    note(At::Page, tab.read(cx).page.clone(), cx);
                    tab
                },
                window,
                cx,
            );
            ws.open_tab(
                "doc",
                false,
                |_, cx| {
                    let own: Vec<Box<dyn Action>> =
                        vec![Box::new(markdown::ZoomReset), Box::new(find::Prev)];
                    let tab = cx.new(|cx| Area::new("MarkdownView MarkdownReading", own, cx));
                    note(At::Doc, tab.read(cx).focus.clone(), cx);
                    tab
                },
                window,
                cx,
            );
            for slot in [Slot::Left, Slot::Right, Slot::Bottom] {
                ws.set_open(slot, true, cx);
            }
            // Each right tab drawn once, so both are built.
            ws.show_panel("search", cx);
        })
    });
    draw(&mut cx);
    let ws = workspace(&mut cx);
    cx.update(|_, cx| ws.update(cx, |ws, cx| ws.show_panel("explorer", cx)));
    draw(&mut cx);
    cx
}

fn close(mut cx: VisualTestContext) {
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn workspace(cx: &mut VisualTestContext) -> Entity<Workspace> {
    cx.update(|window, cx| Workspace::for_window(window, cx).unwrap())
}

/// Focus to `at` as a click puts it there.
fn click(at: At, cx: &mut VisualTestContext) {
    if at == At::Palette {
        click(At::Code, cx);
        cx.simulate_keystrokes("shift-cmd-p");
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "palette"
        );
        return;
    }
    let ws = workspace(cx);
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            if let Some((slot, tab)) = at.panel() {
                ws.set_open(slot, true, cx);
                ws.show_panel(tab, cx);
            }
            if let Some(tab) = at.tab() {
                ws.activate_tab(tab, window, cx);
            }
        });
    });
    draw(cx);
    cx.update(|window, cx| window.focus(&place(at, cx), cx));
    draw(cx);
}

fn focused(cx: &mut VisualTestContext) -> Option<At> {
    cx.update(|window, cx| {
        let places = &cx.global::<Places>().0;
        places
            .iter()
            .find(|(_, h)| h.is_focused(window))
            .map(|(at, _)| *at)
    })
}

/// What `key` pressed from `start` with its panel in `state` should leave; None where the state
/// can't be (the panel hidden with focus in it, the palette with focus in a panel).
fn want(key: &Key, start: At, state: State) -> Option<Outcome> {
    let inside = start.panel().is_some_and(|(slot, _)| slot == key.slot);
    let own_tab = key.tab.is_none() || key.tab == start.panel().map(|p| p.1);
    let into = Outcome {
        shown: true,
        focus: Some(panel_place(key)),
        tab: key.tab,
    };
    let stays = |shown: bool, focus: At| Outcome {
        shown,
        focus: Some(focus),
        tab: None,
    };
    let home = if start == At::Palette {
        At::Code
    } else {
        start
    };
    match state {
        State::Hidden if inside => None,
        State::Hidden => Some(match key.kind {
            _ if place_keeps(key, start) => stays(false, home),
            Kind::Focus => into,
            Kind::Visibility => stays(true, home),
        }),
        // From another tab of the panel, a focus key switches to its own.
        State::Shown if inside && own_tab => None,
        State::Shown => Some(match key.kind {
            _ if place_keeps(key, start) => stays(true, home),
            Kind::Focus => into,
            Kind::Visibility => stays(false, home),
        }),
        State::Focused if start == At::Palette || (inside && !own_tab) => None,
        State::Focused => Some(stays(key.kind == Kind::Focus, came_from(key, start))),
    }
}

/// Where focus was before it went into the key's panel: from inside it, the code view.
fn came_from(key: &Key, start: At) -> At {
    match start.panel() {
        Some((slot, _)) if slot == key.slot => At::Code,
        _ => start,
    }
}

/// Lays out `state` for `key` pressed from `start`, by clicks as the user would.
fn arrange(key: &Key, start: At, state: State, cx: &mut VisualTestContext) {
    match state {
        State::Hidden => {
            click(start, cx);
            let ws = workspace(cx);
            cx.update(|_, cx| ws.update(cx, |ws, cx| ws.set_open(key.slot, false, cx)));
            draw(cx);
        }
        State::Shown => click(start, cx),
        State::Focused => {
            let from = came_from(key, start);
            click(from, cx);
            click(
                if from == start {
                    panel_place(key)
                } else {
                    start
                },
                cx,
            );
        }
    }
}

/// What the window shows now, as `Outcome` names it.
fn outcome(key: &Key, want: &Outcome, cx: &mut VisualTestContext) -> Outcome {
    let ws = workspace(cx);
    let (shown, tab) = cx.update(|_, cx| {
        let ws = ws.read(cx);
        (ws.is_shown(key.slot), ws.active_panel(key.slot, cx))
    });
    Outcome {
        shown,
        focus: focused(cx),
        tab: want.tab.and(tab),
    }
}

/// Each of `keys` from each place, with its panel hidden, shown, and focused: how many cases.
fn run(preset: Preset, keys: &[Key], cx: &mut TestAppContext) -> usize {
    let project = init(preset, cx);
    let mut failed = Vec::new();
    let mut cases = 0;
    for key in keys {
        for start in STARTS {
            for state in [State::Hidden, State::Shown, State::Focused] {
                let Some(want) = want(key, start, state) else {
                    continue;
                };
                cases += 1;
                let mut window = open(&project, cx);
                let cx = &mut window;
                arrange(key, start, state, cx);
                cx.simulate_keystrokes(key.keys);
                draw(cx);
                let got = outcome(key, &want, cx);
                let dialog = cx.update(|window, cx| window.has_active_dialog(cx));
                if got != want || dialog {
                    failed.push(format!(
                        "{} from {start:?}, {state:?}: got {got:?}{}, want {want:?}",
                        key.keys,
                        if dialog { " with the palette open" } else { "" },
                    ));
                }
                close(window);
            }
        }
    }
    assert!(
        failed.is_empty(),
        "{preset:?}, {} of {cases}:\n{}",
        failed.len(),
        failed.join("\n")
    );
    cases
}

/// Each panel key from each place, with its panel hidden, shown, and focused.
#[gpui::test]
fn every_panel_key_works_from_everywhere(cx: &mut TestAppContext) {
    assert_eq!(run(Preset::VsCode, &VSCODE, cx), 210, "the matrix's size");
}

#[gpui::test]
fn the_classic_panel_keys_work_the_same(cx: &mut TestAppContext) {
    assert_eq!(run(Preset::Classic, &CLASSIC, cx), 186, "the matrix's size");
}

/// Repeated presses alternate cleanly: a focus key in and back, a visibility key shown and hidden,
/// focus staying in the code view.
#[gpui::test]
fn repeated_presses_alternate(cx: &mut TestAppContext) {
    let project = init(Preset::VsCode, cx);
    let cx = &mut open(&project, cx);
    let ws = workspace(cx);
    click(At::Code, cx);
    for _ in 0..3 {
        cx.simulate_keystrokes("shift-cmd-e");
        draw(cx);
        assert_eq!(focused(cx), Some(At::Explorer));
        cx.simulate_keystrokes("shift-cmd-e");
        draw(cx);
        assert_eq!(focused(cx), Some(At::Code));
    }
    for _ in 0..3 {
        cx.simulate_keystrokes("cmd-e");
        draw(cx);
        assert!(!cx.update(|_, cx| ws.read(cx).is_shown(Slot::Right)));
        cx.simulate_keystrokes("cmd-e");
        draw(cx);
        assert!(cx.update(|_, cx| ws.read(cx).is_shown(Slot::Right)));
        assert_eq!(focused(cx), Some(At::Code));
    }
}

/// Where focus came from is gone: back goes to the code view, and with no tab, the terminal.
#[gpui::test]
fn focus_from_a_closed_place_goes_to_the_code_view_then_the_terminal(cx: &mut TestAppContext) {
    let project = init(Preset::VsCode, cx);
    let cx = &mut open(&project, cx);
    let ws = workspace(cx);
    click(At::Page, cx);
    cx.simulate_keystrokes("shift-cmd-e");
    draw(cx);
    let close = |key: &str, cx: &mut VisualTestContext| {
        cx.update(|_, cx| ws.update(cx, |ws, cx| ws.close_tab(key, cx)));
        draw(cx);
    };
    close("page", cx);
    cx.simulate_keystrokes("shift-cmd-e");
    draw(cx);
    let part =
        |cx: &mut VisualTestContext| cx.update(|window, cx| ws.read(cx).focused_part(window, cx));
    assert_eq!(
        part(cx),
        Some(Part::Center),
        "the page closed: the code view"
    );
    cx.simulate_keystrokes("shift-cmd-e");
    draw(cx);
    close("code", cx);
    close("doc", cx);
    cx.simulate_keystrokes("shift-cmd-e");
    draw(cx);
    assert_eq!(focused(cx), Some(At::Pane), "no tab: the terminal");
    assert_eq!(part(cx), Some(Part::Slot(Slot::Bottom)));
}
