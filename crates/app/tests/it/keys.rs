//! The app's real keymap (its commands, gpui-component's text fields, the feature crates' own
//! keys) on a window laid out with the app's key contexts: each key runs the command it should
//! where it should, and text fields keep their editing keys. A root listener notes every command
//! that reaches it; the places a command's own view would take it are noted where they are.
//! The keystrokes are macOS's; Linux's chords are checked by binding the keymap built for it.
//! Most cases pin the classic preset, whose keys they were written for; the VS Code preset's
//! own keys have their cases at the end.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gitviber_ui::keymap::{
    self, Overrides, Preset, ROOT_CONTEXT, SYSTEM_COMMANDS, TERMINAL_CONTEXT, all_commands, tab,
};
use gpui_kit::component::input::{Editor, EditorState, Input, InputState, Textarea, TextareaState};
use gpui_kit::{
    AppContext as _, Context, Entity, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext,
    Window, div, px,
};

mod panels;

type Log = Rc<RefCell<Vec<String>>>;

/// Plain `cargo test` runs these on threads of one process, which share the config folder's
/// variable: a folder per setup, and one setup at a time reading it.
static SETUP: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum At {
    Root,
    Pane,
    Changes,
    Diff,
    Tabs,
    Browser,
    Image,
    Reading,
    Explorer,
    History,
    Worktrees,
    Branches,
    Projects,
    // Text fields.
    Summary,
    Description,
    Code,
    TagMessage,
    Comment,
    Address,
}

const SPOTS: [(At, &str); 12] = [
    (At::Pane, TERMINAL_CONTEXT),
    (At::Changes, "ChangesList"),
    (At::Diff, gitviber_diff::actions::CONTEXT),
    (At::Tabs, "TabStrip"),
    (At::Browser, "Browser"),
    (At::Image, "ImageView"),
    (At::Reading, "MarkdownView MarkdownReading"),
    (At::Explorer, "Explorer"),
    (At::History, "HistoryList"),
    (At::Worktrees, "WorktreePicker"),
    (At::Branches, "BranchPicker"),
    (At::Projects, "ProjectList"),
];

struct Shell {
    log: Log,
    root: FocusHandle,
    spots: Vec<(At, FocusHandle)>,
    summary: Entity<InputState>,
    description: Entity<TextareaState>,
    code: Entity<EditorState>,
    tag: Entity<TextareaState>,
    comment: Entity<TextareaState>,
    address: Entity<InputState>,
    /// The terminal's find bar: its ⌘G and ⇧⌘G step through matches only while it's open.
    find_open: bool,
}

impl Shell {
    fn new(log: Log, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Shell {
            log,
            root: cx.focus_handle(),
            spots: SPOTS
                .iter()
                .map(|(at, _)| (*at, cx.focus_handle()))
                .collect(),
            summary: cx.new(|cx| InputState::new(window, cx)),
            description: cx.new(|cx| TextareaState::new(window, cx)),
            code: cx.new(|cx| EditorState::new(window, cx)),
            tag: cx.new(|cx| TextareaState::new(window, cx)),
            comment: cx.new(|cx| TextareaState::new(window, cx)),
            address: cx.new(|cx| InputState::new(window, cx)),
            find_open: false,
        }
    }

    fn spot(&self, at: At, context: &'static str) -> gpui_kit::Div {
        let focus = &self.spots.iter().find(|(a, _)| *a == at).unwrap().1;
        div().key_context(context).track_focus(focus).size(px(20.))
    }

    fn focus_of(&self, at: At, cx: &gpui_kit::App) -> FocusHandle {
        match at {
            At::Root => self.root.clone(),
            At::Summary => self.summary.focus_handle(cx),
            At::Description => self.description.focus_handle(cx),
            At::Code => self.code.focus_handle(cx),
            At::TagMessage => self.tag.focus_handle(cx),
            At::Comment => self.comment.focus_handle(cx),
            At::Address => self.address.focus_handle(cx),
            spot => self
                .spots
                .iter()
                .find(|(a, _)| *a == spot)
                .unwrap()
                .1
                .clone(),
        }
    }

    fn text(&self, at: At, cx: &gpui_kit::App) -> String {
        match at {
            At::Summary => self.summary.read(cx).value(),
            At::Description => self.description.read(cx).value(),
            At::Code => self.code.read(cx).value(),
            At::TagMessage => self.tag.read(cx).value(),
            At::Address => self.address.read(cx).value(),
            _ => panic!("{at:?} is no text field"),
        }
        .to_string()
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .key_context(ROOT_CONTEXT)
            .track_focus(&self.root)
            .size_full()
            .flex()
            .flex_col();
        for c in all_commands().chain(SYSTEM_COMMANDS) {
            let (log, id) = (self.log.clone(), c.id);
            root = root.on_boxed_action(&*c.action(), move |_, _, _| {
                log.borrow_mut().push(id.to_string());
            });
        }
        let note = |log: &Log, what: &str| log.borrow_mut().push(what.to_string());
        let (log, find_log) = (self.log.clone(), self.log.clone());
        let terminal_panel = div()
            .key_context(gitviber_terminal::PANEL_CONTEXT)
            .on_action(
                cx.listener(move |this, _: &gitviber_terminal::FindPrev, _, cx| {
                    if this.find_open {
                        note(&find_log, "terminal find: previous");
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .on_action(move |_: &tab::Goto1, _, _| note(&log, "terminal: tab 1"))
            .child(self.spot(At::Pane, TERMINAL_CONTEXT));
        let changes = self.spot(At::Changes, "ChangesList").size_auto().child(
            div()
                .key_context("CommitMessage")
                .child(Input::new(&self.summary))
                .child(Textarea::new(&self.description)),
        );
        let code_area = div()
            .key_context("CodeArea")
            .child(self.spot(At::Tabs, "TabStrip"))
            .child(self.spot(At::Diff, gitviber_diff::actions::CONTEXT))
            .child(
                div()
                    .key_context("CodeView")
                    .h(px(120.))
                    .child(Editor::new(&self.code)),
            )
            .child(
                self.spot(At::Browser, "Browser")
                    .size_auto()
                    .child(Input::new(&self.address)),
            )
            .child(self.spot(At::Image, "ImageView"))
            .child(self.spot(At::Reading, "MarkdownView MarkdownReading"))
            .child(self.spot(At::Explorer, "Explorer"));
        root.child(changes)
            .child(code_area)
            .child(terminal_panel)
            .child(self.spot(At::History, "HistoryList"))
            .child(self.spot(At::Worktrees, "WorktreePicker"))
            .child(self.spot(At::Branches, "BranchPicker"))
            .child(self.spot(At::Projects, "ProjectList"))
            .child(
                div()
                    .key_context("TagMessage")
                    .child(Textarea::new(&self.tag)),
            )
            .child(
                div()
                    .key_context("IssueComment")
                    .child(Textarea::new(&self.comment)),
            )
    }
}

/// The app's keymap as `gitviber::init` builds it, from the classic preset: gitviber-ui's
/// commands and gpui-component's fields, then the keys the feature crates add.
fn setup(cx: &mut TestAppContext) -> (Entity<Shell>, Log, &mut VisualTestContext) {
    setup_with(Preset::Classic, cx)
}

fn setup_with(
    preset: Preset,
    cx: &mut TestAppContext,
) -> (Entity<Shell>, Log, &mut VisualTestContext) {
    let one = SETUP.lock().unwrap_or_else(|e| e.into_inner());
    let dir = gitviber_ui::test_support::config_dir(&format!("app-keys-{}", preset.as_str()));
    // In the file, as a launch reads it: the keymap is built from it before the feature crates'.
    let settings = format!(r#"{{"keymapPreset": "{}"}}"#, preset.as_str());
    std::fs::write(dir.join("settings.json"), settings).unwrap();
    cx.update(|cx| {
        gitviber_ui::init(cx);
        gitviber_ui::install_core(cx);
        gitviber_changes::init(cx);
        gitviber_terminal::init(cx);
        gitviber_files::init(cx);
    });
    drop(one);
    let log = Log::default();
    let mut shell = None;
    let window = cx.add_window(|window, cx| {
        let view = cx.new(|cx| Shell::new(log.clone(), window, cx));
        shell = Some(view.clone());
        gpui_kit::base::Root::new(view, window, cx)
    });
    let cx = VisualTestContext::from_window(*window, cx).into_mut();
    cx.run_until_parked();
    (shell.unwrap(), log, cx)
}

/// What `keys` run with focus `at`: the commands noted, in order.
fn press(
    cx: &mut VisualTestContext,
    shell: &Entity<Shell>,
    log: &Log,
    at: At,
    keys: &str,
) -> Vec<String> {
    cx.update(|window, cx| {
        let focus = shell.read(cx).focus_of(at, cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    log.borrow_mut().clear();
    cx.simulate_keystrokes(keys);
    std::mem::take(&mut *log.borrow_mut())
}

fn text(cx: &mut VisualTestContext, shell: &Entity<Shell>, at: At) -> String {
    cx.update(|_, cx| shell.read(cx).text(at, cx))
}

fn expect(
    cx: &mut VisualTestContext,
    shell: &Entity<Shell>,
    log: &Log,
    at: At,
    cases: &[(&str, &[&str])],
) {
    for (keys, want) in cases {
        let got = press(cx, shell, log, at, keys);
        assert_eq!(got, *want, "{keys} at {at:?}");
    }
}

#[gpui::test]
fn global_keys_run_from_the_workspace(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup(cx);
    expect(
        cx,
        &shell,
        &log,
        At::Root,
        &[
            ("shift-cmd-p", &["workbench.showCommands"]),
            ("cmd-p", &["workbench.quickOpen"]),
            ("j", &["review.nextFile"]),
            ("cmd-w", &["tab.close"]),
            ("ctrl-1", &["view.changes"]),
            // ⇧⌘] comes from macOS as ⌘}.
            ("cmd-}", &["tab.next"]),
            ("cmd-right", &["tab.next"]),
            ("ctrl-tab", &["tab.next"]),
            ("cmd-z", &["git.undo"]),
            ("f6", &["view.focusNextPanel"]),
            ("ctrl-cmd-c", &["changes.focusMessage"]),
            ("shift-cmd-g", &["view.focusGitPanel"]),
            ("cmd-=", &["view.zoomIn"]),
            ("cmd-q", &["app.quit"]),
        ],
    );
}

#[gpui::test]
fn the_terminal_keeps_the_shells_keys_and_takes_its_own(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup(cx);
    expect(
        cx,
        &shell,
        &log,
        At::Pane,
        &[
            // Its own over the workspace's.
            ("cmd-w", &["terminal.close"]),
            ("cmd-=", &["terminal.fontZoomIn"]),
            ("cmd-enter", &["terminal.toggleMaximize"]),
            ("cmd-d", &["terminal.split"]),
            ("shift-cmd-d", &["terminal.splitDown"]),
            ("cmd-k", &["terminal.clear"]),
            ("cmd-r", &["terminal.renamePane"]),
            // The pane keys follow cmux's.
            ("shift-cmd-enter", &["terminal.zoomPane"]),
            ("alt-cmd-left", &["terminal.focusLeft"]),
            ("alt-cmd-down", &["terminal.focusDown"]),
            ("shift-cmd-left", &["terminal.prevPane"]),
            ("cmd-up", &["terminal.prevCommand"]),
            ("ctrl-shift-space", &["terminal.hints"]),
            // ⌘1 picks the terminal's tab, the panel's answer.
            ("cmd-1", &["terminal: tab 1"]),
            // ⌃ with a non-letter is the app's; letters, ⌥, F-keys and ⌘-arrows stay the shell's.
            ("ctrl-tab", &["tab.next"]),
            ("ctrl-1", &["view.changes"]),
            ("ctrl-`", &["terminal.focus"]),
            ("cmd-j", &["terminal.toggle"]),
            ("j", &[]),
            ("ctrl-r", &[]),
            ("alt-s", &[]),
            ("f6", &[]),
            ("cmd-right", &[]),
        ],
    );
}

/// ⇧⌘G is Focus Git Panel, and the terminal's Find Previous while its find bar is open: the
/// panel has to let the key go on with the bar closed.
#[gpui::test]
fn shift_cmd_g_in_the_terminal_steps_its_find_only_while_open(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup(cx);
    assert_eq!(
        press(cx, &shell, &log, At::Pane, "shift-cmd-g"),
        ["view.focusGitPanel"]
    );
    shell.update(cx, |s, _| s.find_open = true);
    assert_eq!(
        press(cx, &shell, &log, At::Pane, "shift-cmd-g"),
        ["terminal find: previous"]
    );
}

#[gpui::test]
fn text_fields_keep_their_editing_keys(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup(cx);
    // Letters, ⌥ and ⌃ chords type or edit; the review and diff keys stay out.
    for keys in [
        "j",
        "k",
        "s",
        "v",
        "c",
        "alt-s",
        "alt-left",
        "cmd-left",
        "cmd-right",
        "ctrl-a",
    ] {
        assert_eq!(
            press(cx, &shell, &log, At::Summary, keys),
            Vec::<String>::new(),
            "{keys}"
        );
    }
    assert_eq!(text(cx, &shell, At::Summary), "jksvc");
    // ⌘A selects the text (typing replaces it), not every change in the list around it.
    assert!(press(cx, &shell, &log, At::Summary, "cmd-a x").is_empty());
    assert_eq!(text(cx, &shell, At::Summary), "x");
    // ⌘Z and ⌘⌫ are the field's: not Undo Git Action, not Discard.
    for keys in [
        "cmd-z",
        "shift-cmd-z",
        "cmd-backspace",
        "alt-backspace",
        "shift-cmd-backspace",
    ] {
        assert!(
            press(cx, &shell, &log, At::Summary, keys).is_empty(),
            "{keys}"
        );
    }
    // ⌘ chords that don't edit text still run.
    assert_eq!(
        press(cx, &shell, &log, At::Summary, "shift-cmd-p"),
        ["workbench.showCommands"]
    );
    assert_eq!(
        press(cx, &shell, &log, At::Summary, "f7"),
        ["diff.nextChange"]
    );
}

#[gpui::test]
fn cmd_enter_sends_the_message_instead_of_a_new_line(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup(cx);
    for (at, command) in [
        (At::Summary, "git.commit"),
        (At::Description, "git.commit"),
        (At::TagMessage, "git.createTag"),
        (At::Comment, "github.postComment"),
    ] {
        assert!(press(cx, &shell, &log, at, "a b").is_empty());
        assert_eq!(
            press(cx, &shell, &log, at, "cmd-enter"),
            [command],
            "{at:?}"
        );
        assert_eq!(
            press(cx, &shell, &log, at, "ctrl-enter"),
            [command],
            "{at:?}"
        );
    }
    assert_eq!(text(cx, &shell, At::Description), "ab");
    assert_eq!(text(cx, &shell, At::TagMessage), "ab");
    // ↵ alone is still a new line in a message.
    assert!(press(cx, &shell, &log, At::Description, "enter").is_empty());
    assert_eq!(text(cx, &shell, At::Description), "ab\n");
}

#[gpui::test]
fn the_code_view_types_and_keeps_find_in_files_and_definitions(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup(cx);
    assert!(press(cx, &shell, &log, At::Code, "j k s").is_empty());
    assert_eq!(text(cx, &shell, At::Code), "jks");
    // The editor's own ⇧⌘F would open its Replace.
    assert_eq!(
        press(cx, &shell, &log, At::Code, "shift-cmd-f"),
        ["search.findInFiles"]
    );
    assert_eq!(
        press(cx, &shell, &log, At::Code, "cmd-enter"),
        ["editor.goToDefinition"]
    );
    assert_eq!(text(cx, &shell, At::Code), "jks");
    assert_eq!(
        press(cx, &shell, &log, At::Code, "f12"),
        ["editor.goToDefinition"]
    );
    for keys in ["cmd-a", "cmd-z", "cmd-backspace", "alt-left"] {
        assert!(press(cx, &shell, &log, At::Code, keys).is_empty(), "{keys}");
    }
}

#[gpui::test]
fn each_place_runs_its_own_keys(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup(cx);
    expect(
        cx,
        &shell,
        &log,
        At::Changes,
        &[
            ("cmd-a", &["git.selectAllChanges"]),
            ("s", &["git.toggleStage"]),
            ("cmd-backspace", &["git.discard"]),
            ("j", &["review.nextFile"]),
        ],
    );
    expect(
        cx,
        &shell,
        &log,
        At::Diff,
        &[
            ("f7", &["diff.nextChange"]),
            ("alt-down", &["diff.nextChange"]),
            ("shift-f7", &["diff.prevChange"]),
            ("alt-s", &["diff.toggleSplit"]),
            ("alt-cmd-s", &["diff.stageChange"]),
            ("shift-cmd-backspace", &["diff.discardChange"]),
            ("c", &["review.addNote"]),
            ("v", &["review.toggleViewed"]),
            ("cmd-enter", &["editor.goToDefinition"]),
            ("shift-f12", &["editor.goToReferences"]),
        ],
    );
    expect(
        cx,
        &shell,
        &log,
        At::Tabs,
        &[
            ("alt-left", &["tab.moveLeft"]),
            ("alt-right", &["tab.moveRight"]),
        ],
    );
    // A browser tab's keys go before the workspace's same ones.
    expect(
        cx,
        &shell,
        &log,
        At::Browser,
        &[
            ("cmd-l", &["browser.focusAddress"]),
            ("cmd-r", &["browser.reload"]),
            ("cmd-[", &["browser.back"]),
            ("cmd-f", &["browser.find"]),
            ("cmd-g", &["browser.findNext"]),
            ("shift-cmd-g", &["browser.findPrev"]),
            ("cmd-=", &["browser.zoomIn"]),
            ("cmd-0", &["browser.zoomReset"]),
            ("shift-cmd-c", &["browser.pick"]),
            ("shift-cmd-r", &["browser.toggleReader"]),
        ],
    );
    // Its address bar types, and the page's keys reach past it.
    expect(
        cx,
        &shell,
        &log,
        At::Address,
        &[
            ("j", &[]),
            ("cmd-f", &["browser.find"]),
            ("cmd-[", &["browser.back"]),
        ],
    );
    assert_eq!(text(cx, &shell, At::Address), "j");
    expect(
        cx,
        &shell,
        &log,
        At::Image,
        &[
            ("=", &["media.zoomIn"]),
            ("+", &["media.zoomIn"]),
            ("-", &["media.zoomOut"]),
            ("0", &["media.fit"]),
        ],
    );
    // A rendered document takes the window's zoom keys for its text; the code font's still work.
    expect(
        cx,
        &shell,
        &log,
        At::Reading,
        &[
            ("cmd-=", &["markdown.zoomIn"]),
            ("cmd--", &["markdown.zoomOut"]),
            ("cmd-0", &["markdown.zoomReset"]),
            ("alt-cmd-=", &["editor.fontZoomIn"]),
        ],
    );
    expect(
        cx,
        &shell,
        &log,
        At::Explorer,
        &[
            ("f2", &["explorer.rename"]),
            ("cmd-backspace", &["explorer.delete"]),
        ],
    );
    expect(
        cx,
        &shell,
        &log,
        At::History,
        &[
            ("/", &["history.find"]),
            ("?", &["history.find"]),
            ("j", &["review.nextFile"]),
        ],
    );
    expect(
        cx,
        &shell,
        &log,
        At::Worktrees,
        &[
            ("t", &["worktree.openTerminal"]),
            ("m", &["worktree.merge"]),
            ("f2", &["worktree.rename"]),
            ("backspace", &["worktree.remove"]),
        ],
    );
    expect(
        cx,
        &shell,
        &log,
        At::Branches,
        &[("f2", &["git.renameBranch"])],
    );
    expect(
        cx,
        &shell,
        &log,
        At::Projects,
        &[
            ("backspace", &["project.forget"]),
            ("delete", &["project.forget"]),
        ],
    );
}

#[gpui::test]
fn a_rebinding_replaces_the_default_key(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup(cx);
    // Set, not saved: under plain `cargo test` the save would land in another case's folder.
    cx.update(|_, cx| {
        let mut s = gitviber_ui::Settings::get(cx).clone();
        s.keybindings
            .insert("review.nextFile".into(), vec!["n".into()]);
        cx.set_global(s);
    });
    cx.run_until_parked();
    assert_eq!(press(cx, &shell, &log, At::Root, "n"), ["review.nextFile"]);
    assert!(press(cx, &shell, &log, At::Root, "j").is_empty());
}

/// Off macOS "cmd" is Ctrl and "ctrl" the Super key: Ctrl+Alt stays Ctrl+Alt, and the macOS
/// system keys leave Ctrl+H and Ctrl+M to the text.
#[gpui::test]
fn linux_keys_are_ctrl_chords(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup(cx);
    cx.update(|_, cx| {
        cx.clear_key_bindings();
        cx.bind_keys(keymap::key_bindings(
            Preset::Classic,
            &Overrides::new(),
            false,
        ));
    });
    let cases: &[(At, &str, &[&str])] = &[
        (At::Pane, "ctrl-alt-left", &["terminal.prevPane"]),
        (At::Pane, "ctrl-alt-right", &["terminal.nextPane"]),
        (At::Pane, "super-alt-left", &[]),
        (At::Pane, "ctrl-shift-w", &["terminal.close"]),
        (At::Pane, "ctrl-shift-d", &["terminal.split"]),
        (At::Pane, "ctrl-alt-shift-d", &["terminal.splitDown"]),
        (At::Pane, "ctrl-shift-enter", &["terminal.zoomPane"]),
        (At::Pane, "ctrl-shift-t", &["terminal.new"]),
        (At::Pane, "ctrl-w", &[]),
        (At::Pane, "ctrl-r", &[]),
        (At::Root, "alt-1", &["view.changes"]),
        (At::Root, "ctrl-tab", &["tab.next"]),
        (At::Root, "ctrl-shift-]", &["tab.next"]),
        (At::Root, "ctrl-w", &["tab.close"]),
        (At::Root, "ctrl-q", &["app.quit"]),
        (At::Root, "ctrl-h", &[]),
        (At::Root, "ctrl-m", &[]),
        (At::Root, "ctrl-super-f", &[]),
    ];
    for (at, keys, want) in cases {
        assert_eq!(
            press(cx, &shell, &log, *at, keys),
            *want,
            "{keys} at {at:?}"
        );
    }
}

/// Two global commands on one chord: only the first listed would ever run.
#[test]
fn no_two_global_commands_share_a_default_chord() {
    for preset in Preset::ALL {
        for mac in [true, false] {
            let mut by_chord: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for c in all_commands().filter(|c| c.local.is_none()) {
                for k in preset.keys(c, mac) {
                    by_chord.entry(k).or_default().push(c.id);
                }
            }
            let shared: Vec<_> = by_chord.iter().filter(|(_, ids)| ids.len() > 1).collect();
            assert!(shared.is_empty(), "{preset:?} mac {mac}: {shared:?}");
        }
    }
}

#[gpui::test]
fn the_vscode_presets_keys_run_where_they_should(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup_with(Preset::VsCode, cx);
    expect(
        cx,
        &shell,
        &log,
        At::Root,
        &[
            ("cmd-b", &["view.toggleGitPanel"]),
            ("cmd-e", &["view.toggleExplorer"]),
            ("alt-cmd-b", &["view.toggleExplorer"]),
            ("shift-cmd-e", &["view.showExplorer"]),
            ("shift-cmd-f", &["search.findInFiles"]),
            ("ctrl-shift-g", &["view.changes"]),
            ("ctrl-`", &["terminal.focus"]),
            ("cmd-j", &["terminal.toggle"]),
            ("ctrl-~", &["terminal.new"]),
            ("f1", &["workbench.showCommands"]),
            ("ctrl-1", &["tab.goto1"]),
            ("ctrl-0", &["tab.last"]),
            ("ctrl-9", &["tab.last"]),
            ("cmd-1", &["view.focusCode"]),
            ("cmd-0", &["view.focusGitPanel"]),
            ("alt-cmd-right", &["tab.next"]),
            ("alt-cmd-left", &["tab.prev"]),
            ("ctrl-r", &["file.switchProject"]),
            ("alt-cmd-r", &["file.reveal"]),
            // Run Build Task's key opens nothing, and ⇧⌘G is only a find box's.
            ("shift-cmd-b", &[]),
            ("shift-cmd-g", &[]),
        ],
    );
    // The terminal keeps its pane keys over the tabs' ⌥⌘arrows, and splits on ⌘\ too.
    expect(
        cx,
        &shell,
        &log,
        At::Pane,
        &[
            ("alt-cmd-right", &["terminal.focusRight"]),
            ("alt-cmd-left", &["terminal.focusLeft"]),
            ("shift-cmd-enter", &["terminal.zoomPane"]),
            ("cmd-\\", &["terminal.split"]),
            ("cmd-d", &["terminal.split"]),
            ("cmd-k", &["terminal.clear"]),
            ("ctrl-1", &["terminal: tab 1"]),
            ("ctrl-`", &["terminal.focus"]),
            ("ctrl-r", &[]),
        ],
    );
    // ⌘0 resets a document's text size; elsewhere it's the side bar's.
    expect(
        cx,
        &shell,
        &log,
        At::Reading,
        &[
            ("cmd-=", &["markdown.zoomIn"]),
            ("cmd--", &["markdown.zoomOut"]),
            ("cmd-0", &["markdown.zoomReset"]),
        ],
    );
    expect(
        cx,
        &shell,
        &log,
        At::Diff,
        &[
            ("alt-f5", &["diff.nextChange"]),
            ("f7", &["diff.nextChange"]),
        ],
    );
    // Typing in the code view or the commit box: the keys a field doesn't edit with still run.
    for at in [At::Code, At::Summary, At::Description] {
        expect(
            cx,
            &shell,
            &log,
            at,
            &[
                ("ctrl-shift-g", &["view.changes"]),
                ("ctrl-r", &["file.switchProject"]),
                ("alt-cmd-right", &["tab.next"]),
                ("alt-cmd-left", &["tab.prev"]),
                ("ctrl-1", &["tab.goto1"]),
                ("ctrl-0", &["tab.last"]),
                ("shift-cmd-p", &["workbench.showCommands"]),
                ("ctrl-k", &[]),
            ],
        );
    }
    expect(
        cx,
        &shell,
        &log,
        At::Browser,
        &[
            ("ctrl-shift-g", &["view.changes"]),
            ("ctrl-r", &["file.switchProject"]),
            ("alt-cmd-right", &["tab.next"]),
            ("ctrl-1", &["tab.goto1"]),
            ("cmd-0", &["browser.zoomReset"]),
            ("cmd-1", &["view.focusCode"]),
            ("shift-cmd-r", &["browser.toggleReader"]),
        ],
    );
}

/// Off macOS Alt+digit types nothing in a field, so the VS Code tab keys run there; the
/// terminal keeps it, readline's count (Alt+3 Alt+B: back three words).
#[gpui::test]
fn linux_vscode_tab_keys_run_from_fields_not_the_terminal(cx: &mut TestAppContext) {
    let (shell, log, cx) = setup_with(Preset::VsCode, cx);
    cx.update(|_, cx| {
        cx.clear_key_bindings();
        cx.bind_keys(keymap::key_bindings(
            Preset::VsCode,
            &Overrides::new(),
            false,
        ));
    });
    let cases: &[(At, &str, &[&str])] = &[
        (At::Root, "alt-1", &["tab.goto1"]),
        (At::Summary, "alt-1", &["tab.goto1"]),
        (At::Description, "alt-0", &["tab.last"]),
        (At::Browser, "alt-9", &["tab.last"]),
        (At::Pane, "alt-1", &[]),
        (At::Pane, "ctrl-shift-g", &["view.changes"]),
    ];
    for (at, keys, want) in cases {
        assert_eq!(
            press(cx, &shell, &log, *at, keys),
            *want,
            "{keys} at {at:?}"
        );
    }
}
