//! Settings (⌘,) in a window of its own, one at a time. It reads the same Settings global as the
//! workspace, so a change here shows there as it's made. What a section shows beyond settings
//! (the open repository's remotes and identity, Obsidian's vaults, the apps Open in found, macOS's
//! notification permission) is read when the window opens and again when it comes forward.
//! ⌘W closes it, and so does Esc once nothing in it is left to cancel.

mod fields;
mod git;
mod pages;
mod shell;
mod shortcuts;

use std::collections::HashMap;
use std::path::PathBuf;

use gitviber_core::api;
use gitviber_core::notifications::Permission;
use gitviber_ui::actions::{help, tab, view, workbench};
use gitviber_ui::keymap::{self, IS_MAC, edit::chord_of};
use gitviber_ui::memory::Live;
use gitviber_ui::settings::*;
use gitviber_ui::theme::{self, appearance_floor};
use gitviber_ui::workspace::Toast;
use gitviber_ui::{ActiveAppTheme, Settings, windows};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::highlighter::SyntaxHighlighter;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, Keystroke, ParentElement as _,
    PathPromptOptions, Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px, rems, size,
};

pub use git::Which;
use shell::Page;

/// The sections, most used first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Editor,
    Diff,
    Terminal,
    Git,
    Commit,
    Review,
    Shortcuts,
    OpenIn,
    Notifications,
    Obsidian,
    Browser,
    Updates,
}

/// The sidebar's groups, each under its heading.
pub const GROUPS: [(&str, &[Section]); 3] = [
    (
        "Workspace",
        &[
            Section::Appearance,
            Section::Editor,
            Section::Diff,
            Section::Terminal,
        ],
    ),
    ("Git", &[Section::Git, Section::Commit, Section::Review]),
    (
        "App",
        &[
            Section::Shortcuts,
            Section::OpenIn,
            Section::Notifications,
            Section::Obsidian,
            Section::Browser,
            Section::Updates,
        ],
    ),
];

impl Section {
    pub const ALL: [Section; 13] = [
        Section::Appearance,
        Section::Editor,
        Section::Diff,
        Section::Terminal,
        Section::Git,
        Section::Commit,
        Section::Review,
        Section::Shortcuts,
        Section::OpenIn,
        Section::Notifications,
        Section::Obsidian,
        Section::Browser,
        Section::Updates,
    ];

    /// As the old app named them (its settings window's section ids).
    pub fn id(self) -> &'static str {
        match self {
            Section::Appearance => "appearance",
            Section::Shortcuts => "shortcuts",
            Section::OpenIn => "openIn",
            Section::Notifications => "notifications",
            Section::Updates => "updates",
            Section::Editor => "editor",
            Section::Diff => "diff",
            Section::Terminal => "terminal",
            Section::Obsidian => "obsidian",
            Section::Browser => "browser",
            Section::Git => "git",
            Section::Commit => "commit",
            Section::Review => "review",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Shortcuts => "Keyboard Shortcuts",
            Section::OpenIn => "Open In",
            Section::Notifications => "Notifications",
            Section::Updates => "Updates",
            Section::Editor => "Editor",
            Section::Diff => "Diff",
            Section::Terminal => "Terminal",
            Section::Obsidian => "Obsidian",
            Section::Browser => "Browser",
            Section::Git => "Git",
            Section::Commit => "Commit Messages",
            Section::Review => "Guided Review",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Section::Appearance => IconName::Palette,
            Section::Shortcuts => IconName::Keyboard,
            Section::OpenIn => IconName::SquareArrowOutUpRight,
            Section::Notifications => IconName::Bell,
            Section::Updates => IconName::CircleArrowUp,
            Section::Editor => IconName::Code,
            Section::Diff => IconName::GitCompareArrows,
            Section::Terminal => IconName::SquareTerminal,
            Section::Obsidian => IconName::Gem,
            Section::Browser => IconName::Globe,
            Section::Git => IconName::GitBranch,
            Section::Commit => IconName::Sparkles,
            Section::Review => IconName::ScanSearch,
        }
    }

    pub fn parse(id: &str) -> Option<Section> {
        Section::ALL
            .into_iter()
            .find(|s| s.id().eq_ignore_ascii_case(id))
    }
}

#[derive(Default)]
struct Open(Option<(AnyWindowHandle, WeakEntity<SettingsWindow>)>);
impl Global for Open {}

pub fn install(cx: &mut App) {
    cx.on_action(|_: &workbench::OpenSettings, cx| open(None, cx));
    cx.on_action(|_: &help::Shortcuts, cx| open(Some(Section::Shortcuts), cx));
}

/// Shows the settings window at `section` (or where it was), bringing it forward when it's open
/// already. After the update that asked: ⌘, in the settings window itself is that window's.
pub fn open(section: Option<Section>, cx: &mut App) {
    cx.defer(move |cx| {
        if let Some((handle, view)) = cx.default_global::<Open>().0.clone()
            && let Some(view) = view.upgrade()
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            if let Some(section) = section {
                view.update(cx, |w, cx| w.show(section, cx));
            }
            return;
        }
        let options =
            windows::options(None, size(px(880.), px(660.)), size(px(620.), px(420.)), cx);
        let section = section.unwrap_or(Section::Appearance);
        match gpui_kit::open_window(options, cx, move |window, cx| view(section, window, cx)) {
            Ok((handle, view)) => cx.global_mut::<Open>().0 = Some((handle, view.downgrade())),
            Err(err) => eprintln!("gitviber: couldn't open the settings window: {err}"),
        }
    });
}

/// The window's view at `section`: what `open` puts in its window, and a snapshot draws.
pub fn view(section: Section, window: &mut Window, cx: &mut App) -> Entity<SettingsWindow> {
    cx.new(|cx| SettingsWindow::new(section, window, cx))
}

/// The key being recorded: command `id`'s key `index` (its count: a new one), and why the last
/// key pressed couldn't be taken.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub id: &'static str,
    pub index: usize,
    pub refused: Option<String>,
}

#[derive(Clone, Default)]
pub struct Identity {
    pub own_name: Option<String>,
    pub own_email: Option<String>,
    pub global_name: Option<String>,
    pub global_email: Option<String>,
}

#[derive(Clone)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

/// The open repository's own settings; None while none is open.
#[derive(Default)]
pub struct GitData {
    /// The main window's project, by its main worktree.
    pub main: Option<String>,
    pub identity: Option<Identity>,
    /// "This repository" is picked for Commit as.
    pub own: bool,
    pub remotes: Option<Vec<Remote>>,
    pub accounts: Option<Vec<gitviber_core::github::GhAccount>>,
    pub picked_account: Option<String>,
    /// The remote being edited, by name; "" for a new one.
    pub editing: Option<String>,
}

pub struct SkillInfo {
    /// Where each agent's skill stands, a line each.
    pub lines: Vec<String>,
    /// What the skill teaches, for the agents without skills.
    pub note: String,
}

#[derive(Default)]
pub struct Data {
    pub permission: Option<Permission>,
    /// macOS's notification prompt is up.
    pub asking: bool,
    pub vaults: Option<Vec<gitviber_core::obsidian::Vault>>,
    pub git: GitData,
    pub skill: Option<SkillInfo>,
}

pub struct SettingsWindow {
    weak: WeakEntity<SettingsWindow>,
    /// Focused when the window opens: with nothing focused, its keys (⌘W, Esc) reached nothing.
    focus: FocusHandle,
    section: Section,
    search: Entity<InputState>,
    /// The open section's rows; back to the top on another section.
    scroll: ScrollHandle,
    opacity: Entity<SliderState>,
    pub data: Data,
    identity_inputs: Option<(Entity<InputState>, Entity<InputState>)>,
    remote_inputs: Option<(Entity<InputState>, Entity<InputState>)>,
    /// Redraws as the inputs above are typed in (Save turns on); replaced with them.
    input_subs: [Vec<Subscription>; 2],
    /// Custom is picked for Commit Messages' and Guided Review's agent, whatever the command.
    custom_agent: [bool; 2],
    custom_language: bool,
    /// Guided Review's own command from before "Same as" was picked, until the app quits.
    last_review_command: Option<String>,
    recording: Option<Recording>,
    /// Keyboard Shortcuts' own filter: a name or a key, and a category.
    shortcut_filter: Entity<InputState>,
    shortcut_category: Option<keymap::Category>,
    /// Commit Messages' and Guided Review's "Show what the command gets" is open.
    show_prompt: [bool; 2],
    /// The Editor preview's code, parsed once it's first shown.
    sample: Option<SyntaxHighlighter>,
    _intercept: Option<Subscription>,
    loads: HashMap<&'static str, Task<()>>,
    _live: Live,
    _subscriptions: Vec<Subscription>,
}

impl SettingsWindow {
    fn new(section: Section, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = Settings::get(cx).clone();
        let opacity = cx.new(|_| {
            SliderState::new()
                .min(WINDOW_OPACITY.min)
                .max(WINDOW_OPACITY.max)
                .step(WINDOW_OPACITY.step)
                .default_value(s.window_opacity)
        });
        let mut subs = theme::attach(window, cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let shortcut_filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(format!(
                "Filter by name or key (e.g. {})",
                keymap::format_chord("cmd+b", IS_MAC)
            ))
        });
        for input in [&search, &shortcut_filter] {
            subs.push(cx.subscribe(input, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.scroll.set_offset(gpui_kit::Point::default());
                    cx.notify();
                }
            }));
        }
        subs.push(cx.subscribe(&opacity, |_, _, e: &SliderEvent, cx| {
            if let SliderEvent::Change(v) = e {
                let floor = appearance_floor(Settings::get(cx));
                let v = v.start().max(floor);
                Settings::update(cx, |s| s.window_opacity = v);
            }
        }));
        let mut last_agent = s.browser_agent_control;
        subs.push(cx.observe_global::<Settings>(move |this, cx| {
            let on = Settings::get(cx).browser_agent_control;
            if on != last_agent {
                last_agent = on;
                this.load_skill(cx);
            }
            cx.notify();
        }));
        subs.push(cx.observe_global::<gitviber_ui::open_in::Found>(|_, cx| cx.notify()));
        // Back from System Settings or Obsidian: what they changed shows.
        subs.push(cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.load_permission(cx);
                this.load_vaults(cx);
                this.load_git(cx);
            } else {
                this.stop_recording(cx);
            }
        }));
        if let Some(ws) = crate::workspace_view::main_workspace(cx) {
            subs.push(cx.observe(&ws, |this, ws, cx| {
                let main = main_of(&ws, cx);
                if main != this.data.git.main {
                    this.load_git(cx);
                }
            }));
        }
        window.set_window_title("Settings");
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut this = Self {
            weak: cx.weak_entity(),
            focus,
            section,
            search,
            scroll: ScrollHandle::new(),
            opacity,
            data: Data::default(),
            identity_inputs: None,
            remote_inputs: None,
            input_subs: [Vec::new(), Vec::new()],
            custom_agent: [false; 2],
            custom_language: false,
            last_review_command: None,
            recording: None,
            shortcut_filter,
            shortcut_category: None,
            show_prompt: [false; 2],
            sample: None,
            _intercept: None,
            loads: HashMap::new(),
            _live: Live::new("SettingsWindow"),
            _subscriptions: subs,
        };
        if gitviber_ui::has_core(cx) {
            gitviber_ui::open_in::refresh(cx);
            this.load_permission(cx);
            this.load_vaults(cx);
            this.load_git(cx);
            this.load_skill(cx);
        }
        this.load_sample(cx);
        this
    }

    /// Shows `section`, as a menu item or a button elsewhere asks.
    pub fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        if section != self.section {
            self.scroll.set_offset(gpui_kit::Point::default());
        }
        self.section = section;
        self.load_sample(cx);
        cx.notify();
    }

    /// Parses the Editor preview's code off the UI thread, the first time Editor shows.
    fn load_sample(&mut self, cx: &mut Context<Self>) {
        if self.section != Section::Editor || self.loads.contains_key("sample") {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            let parsed = cx
                .background_spawn(async move { pages::parse_sample() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.sample = Some(parsed);
                cx.notify();
            });
        });
        self.loads.insert("sample", task);
    }

    /// Runs `work` with core off the UI thread, then `done` with its answer; one load per name.
    fn load<T: Send + 'static>(
        &mut self,
        name: &'static str,
        work: impl FnOnce(&gitviber_core::Core) -> T + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if !gitviber_ui::has_core(cx) {
            return;
        }
        let core = gitviber_ui::core(cx);
        let task = cx.spawn(async move |this, cx| {
            let answer = cx.background_spawn(async move { work(&core) }).await;
            let _ = this.update(cx, |this, cx| {
                done(this, answer, cx);
                cx.notify();
            });
        });
        self.loads.insert(name, task);
    }

    fn load_permission(&mut self, cx: &mut Context<Self>) {
        if !IS_MAC {
            return;
        }
        self.load(
            "permission",
            |core| api::app::notification_permission(core).ok(),
            |this, p, _| this.data.permission = p,
            cx,
        );
    }

    fn load_vaults(&mut self, cx: &mut Context<Self>) {
        self.load(
            "vaults",
            |core| api::obsidian::vaults(core).unwrap_or_default(),
            |this, v, _| this.data.vaults = Some(v),
            cx,
        );
    }

    fn load_skill(&mut self, cx: &mut Context<Self>) {
        if !IS_MAC || !Settings::get(cx).browser_agent_control {
            self.data.skill = None;
            return;
        }
        let on = Settings::get(cx).browser_agent_skill;
        self.load(
            "skill",
            move |_| {
                let skill = gitviber_browser::skill::sync(None);
                let mut lines: Vec<String> = skill
                    .targets
                    .iter()
                    .filter(|t| on || matches!(t.status, gitviber_browser::skill::Status::Failed))
                    .map(|t| {
                        let state = match t.status {
                            gitviber_browser::skill::Status::Written => "written",
                            gitviber_browser::skill::Status::Absent => "not written",
                            gitviber_browser::skill::Status::Newer => "a newer GitViber's, kept",
                            gitviber_browser::skill::Status::NoAgent => "agent not installed",
                            gitviber_browser::skill::Status::Theirs => "yours, left alone",
                            gitviber_browser::skill::Status::Failed => "couldn't be written",
                        };
                        format!("{}: {} ({state})", t.agent, t.path)
                    })
                    .collect();
                lines.extend(skill.why);
                SkillInfo {
                    lines,
                    note: gitviber_browser::skill::note(),
                }
            },
            |this, skill, _| this.data.skill = Some(skill),
            cx,
        );
    }

    fn load_git(&mut self, cx: &mut Context<Self>) {
        let main = crate::workspace_view::main_workspace(cx).and_then(|ws| main_of(&ws, cx));
        self.data.git.picked_account = main
            .as_ref()
            .and_then(|m| gitviber_github::store::Store::get(cx).account(m));
        if main != self.data.git.main {
            self.data.git = GitData {
                main: main.clone(),
                picked_account: self.data.git.picked_account.clone(),
                ..GitData::default()
            };
            self.identity_inputs = None;
            self.remote_inputs = None;
        }
        if main.is_none() {
            return;
        }
        self.load(
            "identity",
            |core| api::repo::repo_identity(core).ok(),
            |this, id, _| {
                this.data.git.identity = id.map(|i| Identity {
                    own_name: i.own.name,
                    own_email: i.own.email,
                    global_name: i.global.name,
                    global_email: i.global.email,
                });
                if this.identity_inputs.is_none() {
                    this.data.git.own = this
                        .data
                        .git
                        .identity
                        .as_ref()
                        .is_some_and(|i| i.own_name.is_some() || i.own_email.is_some());
                }
            },
            cx,
        );
        self.load(
            "remotes",
            |core| api::repo::remote_list(core).ok(),
            |this, list, _| {
                this.data.git.remotes = list.map(|l| {
                    l.into_iter()
                        .map(|r| Remote {
                            name: r.name,
                            url: r.url,
                        })
                        .collect()
                })
            },
            cx,
        );
        let accounts = gitviber_github::account::accounts(cx);
        let task = cx.spawn(async move |this, cx| {
            let list = accounts.await;
            let _ = this.update(cx, |this, cx| {
                this.data.git.accounts = Some(list);
                cx.notify();
            });
        });
        self.loads.insert("accounts", task);
    }

    pub fn ask_notifications(&mut self, enabling: bool, cx: &mut Context<Self>) {
        if !IS_MAC {
            return;
        }
        let asked = self.data.permission;
        self.data.asking = true;
        cx.notify();
        self.load(
            "permission",
            move |core| {
                let now = api::app::notification_permission(core).ok().or(asked);
                if enabling && now != Some(Permission::Prompt) {
                    return Ok(now);
                }
                api::app::notification_request(core).map(Some)
            },
            |this, answer, cx| {
                this.data.asking = false;
                match answer {
                    Ok(p) => this.data.permission = p,
                    Err(err) => crate::help::toast(
                        Toast::error(err).title("Couldn't ask macOS about notifications"),
                        cx,
                    ),
                }
            },
            cx,
        );
    }

    pub fn pick_worktree_root(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        let task = cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(dir) = paths.into_iter().next()
            {
                let dir = dir.to_string_lossy().into_owned();
                cx.update(|cx| Settings::update(cx, |s| s.worktree_root = Some(dir)));
            }
        });
        self.loads.insert("worktree-root", task);
    }

    pub fn set_identity_scope(&mut self, own: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.data.git.own = own;
        let id = self.data.git.identity.clone().unwrap_or_default();
        if own && self.identity_inputs.is_none() {
            let input = |text: Option<String>,
                         placeholder: &'static str,
                         window: &mut Window,
                         cx: &mut Context<Self>| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(placeholder)
                        .default_value(text.unwrap_or_default())
                })
            };
            let name = input(
                id.own_name.clone().or(id.global_name.clone()),
                "Name",
                window,
                cx,
            );
            let email = input(
                id.own_email.clone().or(id.global_email.clone()),
                "Email",
                window,
                cx,
            );
            self.input_subs[0] = [&name, &email]
                .map(|i| cx.observe(i, |_, _, cx| cx.notify()))
                .into();
            self.identity_inputs = Some((name, email));
        }
        if !own && (id.own_name.is_some() || id.own_email.is_some()) {
            self.set_repo_identity(None, cx);
        }
        cx.notify();
    }

    pub fn save_identity(&mut self, cx: &mut Context<Self>) {
        let Some((name, email)) = &self.identity_inputs else {
            return;
        };
        let name = name.read(cx).value().trim().to_string();
        let email = email.read(cx).value().trim().to_string();
        if name.is_empty() || email.is_empty() {
            return;
        }
        self.set_repo_identity(Some((name, email)), cx);
    }

    fn set_repo_identity(&mut self, identity: Option<(String, String)>, cx: &mut Context<Self>) {
        self.load(
            "set-identity",
            move |core| {
                let (name, email) = identity.unzip();
                api::repo::set_repo_identity(core, name, email)
            },
            |this, done, cx| match done {
                Ok(()) => this.load_git(cx),
                Err(err) => {
                    crate::help::toast(Toast::error(err).title("Couldn't set the identity"), cx)
                }
            },
            cx,
        );
    }

    pub fn pick_account(&mut self, login: Option<String>, cx: &mut Context<Self>) {
        let Some(main) = self.data.git.main.clone() else {
            return;
        };
        match gitviber_github::account::pick_account(&main, login.clone(), cx) {
            Ok(()) => self.data.git.picked_account = login,
            Err(err) => {
                crate::help::toast(Toast::error(err).title("Couldn't switch the account"), cx)
            }
        }
        cx.notify();
    }

    /// Starts editing remote `name` ("" a new one), or stops with None.
    pub fn edit_remote(
        &mut self,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.data.git.editing = name.clone();
        self.remote_inputs = name.map(|name| {
            let remotes = self.data.git.remotes.clone().unwrap_or_default();
            let url = remotes
                .iter()
                .find(|r| r.name == name)
                .map(|r| r.url.clone())
                .unwrap_or_default();
            // The first remote is origin, as clone names it.
            let shown = if name.is_empty() && remotes.is_empty() {
                "origin".to_string()
            } else {
                name
            };
            let name = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Name (origin, upstream…)")
                    .default_value(shown)
            });
            let url = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("URL (https://… or git@…)")
                    .default_value(url)
            });
            (name, url)
        });
        self.input_subs[1] = self
            .remote_inputs
            .clone()
            .map(|(a, b)| {
                [&a, &b]
                    .map(|i| cx.observe(i, |_, _, cx| cx.notify()))
                    .into()
            })
            .unwrap_or_default();
        cx.notify();
    }

    pub fn save_remote(&mut self, cx: &mut Context<Self>) {
        let (Some(editing), Some((name, url))) =
            (self.data.git.editing.clone(), self.remote_inputs.clone())
        else {
            return;
        };
        let name = name.read(cx).value().trim().to_string();
        let url = url.read(cx).value().trim().to_string();
        if name.is_empty() || url.is_empty() {
            return;
        }
        let was = self
            .data
            .git
            .remotes
            .iter()
            .flatten()
            .find(|r| r.name == editing)
            .map(|r| r.url.clone());
        self.load(
            "remote-edit",
            move |core| {
                let edit = |action: &str, name: &str, value: Option<String>| {
                    api::repo::remote_edit(core, action.into(), name.into(), value)
                };
                if editing.is_empty() {
                    return edit("add", &name, Some(url));
                }
                if was.as_deref() != Some(url.as_str()) {
                    edit("set-url", &editing, Some(url))?;
                }
                if name != editing {
                    edit("rename", &editing, Some(name))?;
                }
                Ok(())
            },
            Self::remote_edited,
            cx,
        );
    }

    fn remote_edited(&mut self, done: gitviber_core::Res<()>, cx: &mut Context<Self>) {
        match done {
            Ok(()) => {
                self.data.git.editing = None;
                self.remote_inputs = None;
            }
            Err(err) => {
                crate::help::toast(Toast::error(err).title("Couldn't change the remote"), cx)
            }
        }
        self.load_git(cx);
        if let Some(ws) = crate::workspace_view::main_workspace(cx)
            && let Some(project) = ws.read(cx).project().cloned()
        {
            project.update(cx, |p, cx| p.repo_changed(cx));
        }
    }

    pub fn remove_remote(
        &mut self,
        name: String,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked = gitviber_refs::ask::ask(
            gitviber_refs::ask::Ask::new(
                "Remove remote",
                format!(
                    "Remove {name}? Its remote branches go from this repository, and branches \
                     that track them stop tracking. The repository at {url} is untouched."
                ),
                "Remove",
            )
            .danger(),
            window,
            cx,
        );
        let task = cx.spawn(async move |this, cx| {
            if asked.await != Ok(true) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.load(
                    "remote-edit",
                    move |core| api::repo::remote_edit(core, "remove".into(), name, None),
                    Self::remote_edited,
                    cx,
                )
            });
        });
        self.loads.insert("remote-remove", task);
    }

    /// A preset's command typed, or Custom kept picked while the text is any.
    pub fn pick_agent(&mut self, which: Which, key: &str, cx: &mut Context<Self>) {
        let custom = key == "custom";
        self.custom_agent[which as usize] = custom;
        if let Some(p) = AGENT_PRESETS.iter().find(|p| p.key == key) {
            Settings::update(cx, |s| which.set(s, p.command));
        }
        cx.notify();
    }

    /// Its own command starts from the one it had, else Commit Messages', to change from there.
    pub fn set_review_own(&mut self, own: bool, cx: &mut Context<Self>) {
        let current = Settings::get(cx).review_command.clone();
        if current.is_some() {
            self.last_review_command = current;
        }
        let last = self.last_review_command.clone();
        Settings::update(cx, |s| {
            s.review_command = own.then(|| last.unwrap_or_else(|| s.suggest_command.clone()))
        });
        cx.notify();
    }

    pub fn pick_language(&mut self, name: &str, cx: &mut Context<Self>) {
        self.custom_language = name == "custom";
        if !self.custom_language {
            let name = name.to_string();
            Settings::update(cx, |s| s.review_language = name);
        }
        cx.notify();
    }

    pub fn start_recording(
        &mut self,
        id: &'static str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recording = Some(Recording {
            id,
            index,
            refused: None,
        });
        let here = window.window_handle();
        let weak = cx.weak_entity();
        // Before the keymap: the key being recorded must not run what it's bound to now.
        self._intercept = Some(cx.intercept_keystrokes(move |e, window, cx| {
            if window.window_handle() != here {
                return;
            }
            let taken = weak
                .update(cx, |this, cx| this.recorded_key(&e.keystroke, cx))
                .unwrap_or(false);
            if taken {
                cx.stop_propagation();
            }
        }));
        cx.notify();
    }

    pub fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording.take().is_some() {
            self._intercept = None;
            cx.notify();
        }
    }

    /// A key while recording; true when it was taken (not passed on).
    fn recorded_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(rec) = self.recording.clone() else {
            return false;
        };
        let m = &ks.modifiers;
        let plain = !(m.control || m.alt || m.platform);
        if ks.key == "escape" && plain && !m.shift {
            self.stop_recording(cx);
            return true;
        }
        // Tab still moves focus, so recording can't lock anyone in.
        if ks.key == "tab" && plain {
            self.stop_recording(cx);
            return false;
        }
        let Some(chord) = chord_of(ks, IS_MAC) else {
            return false;
        };
        if let Some(why) = shortcuts::refusal(&chord) {
            self.recording = Some(Recording {
                refused: Some(why),
                ..rec
            });
            cx.notify();
            return true;
        }
        shortcuts::record(rec.id, rec.index, &chord, cx);
        self.stop_recording(cx);
        true
    }

    /// Esc, once what's open in the window (a dialog, a menu, a key being recorded) didn't take
    /// it: a remote being edited is cancelled, a filled filter being typed in emptied, else the
    /// window closes.
    fn escape(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        if k.key != "escape" || k.modifiers.modified() {
            return;
        }
        cx.stop_propagation();
        if self.data.git.editing.is_some() {
            self.edit_remote(None, window, cx);
            return;
        }
        for input in [&self.search, &self.shortcut_filter] {
            if input.focus_handle(cx).is_focused(window) && !input.read(cx).value().is_empty() {
                input.update(cx, |i, cx| i.set_value("", window, cx));
                return;
            }
        }
        window.remove_window();
    }

    fn reset_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asked = gitviber_refs::ask::ask(
            gitviber_refs::ask::Ask::new(
                "Reset settings",
                "Reset every setting, including keyboard shortcuts, to its default? Your own Open \
                 in apps and the repositories you sign off in stay.",
                "Reset",
            )
            .danger(),
            window,
            cx,
        );
        let task = cx.spawn(async move |this, cx| {
            if asked.await == Ok(true) {
                cx.update(Settings::reset);
                let _ = this.update(cx, |this, cx| {
                    this.custom_agent = [false; 2];
                    this.custom_language = false;
                    cx.notify();
                });
            }
        });
        self.loads.insert("reset", task);
    }
}

/// The project a workspace shows, by its main worktree.
fn main_of(ws: &Entity<gitviber_ui::Workspace>, cx: &App) -> Option<String> {
    let project = ws.read(cx).project()?.read(cx);
    let main: PathBuf = project.main_worktree()?.to_path_buf();
    Some(main.to_string_lossy().into_owned())
}

pub fn open_notification_settings(cx: &mut App) {
    let core = gitviber_ui::core(cx);
    let opened = cx.background_spawn(async move { api::app::notification_settings(&core) });
    cx.spawn(async move |cx| {
        if let Err(err) = opened.await {
            cx.update(|cx| {
                crate::help::toast(Toast::error(err).title("Couldn't open System Settings"), cx)
            });
        }
    })
    .detach();
}

/// Send Test: sent where macOS can show it, and otherwise says why not. Never asks by itself.
pub fn send_test_notification(cx: &mut App) {
    let core = gitviber_ui::core(cx);
    let sent = cx.background_spawn(async move {
        let permission = if IS_MAC {
            Some(api::app::notification_permission(&core)?)
        } else {
            None
        };
        let refused = match permission {
            Some(Permission::Denied) => Some("macOS isn't allowing GitViber to show notifications"),
            Some(Permission::Prompt) => Some("macOS hasn't been asked yet"),
            _ => None,
        };
        if let Some(why) = refused {
            return Ok::<_, String>(Err(why));
        }
        api::app::notification_send(
            &core,
            "GitViber".into(),
            "Notifications are on. Click one to come back here.".into(),
            None,
        )?;
        Ok(Ok(permission == Some(Permission::Quiet)))
    });
    cx.spawn(async move |cx| {
        let shown = match sent.await {
            Ok(Ok(quiet)) => {
                let toast = Toast::success(if quiet {
                    "Banners are off, so it's in Notification Center only."
                } else {
                    "Look in the corner of the screen."
                });
                toast.title("Test notification sent")
            }
            Ok(Err(why)) => Toast::info("Nothing was sent.").title(why),
            Err(err) => Toast::error(err).title("Couldn't send a notification"),
        };
        cx.update(|cx| crate::help::toast(shown, cx));
    })
    .detach();
}

impl SettingsWindow {
    fn page(&self, section: Section, cx: &App) -> Page {
        match section {
            Section::Appearance => pages::appearance(self, cx),
            Section::Editor => pages::editor(self, cx),
            Section::Diff => pages::diff(),
            Section::Terminal => pages::terminal(cx),
            Section::Git => git::git(self, cx),
            Section::Commit => git::commit(self, cx),
            Section::Review => git::review(self, cx),
            Section::Shortcuts => shortcuts::page(self, cx),
            Section::OpenIn => pages::open_in(cx),
            Section::Notifications => pages::notifications(self, cx),
            Section::Obsidian => pages::obsidian(self, cx),
            Section::Browser => pages::browser(self, cx),
            Section::Updates => pages::updates(cx),
        }
    }

    fn sidebar(&self, shown: &[Section], cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let groups = GROUPS.iter().filter_map(|(name, sections)| {
            let items: Vec<_> = sections
                .iter()
                .copied()
                .filter(|s| shown.contains(s))
                .map(|section| {
                    let active = section == self.section;
                    div()
                        .id(section.id())
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_size(rems(shell::TITLE_TEXT))
                        .when(active, |d| d.bg(t.active).text_color(t.foreground))
                        .when(!active, |d| {
                            d.text_color(t.muted_foreground)
                                .hover(|s| s.bg(t.hover).text_color(t.foreground))
                        })
                        .child(Icon::new(section.icon()).small())
                        .child(section.title())
                        .on_click(cx.listener(move |this, _, _, cx| this.show(section, cx)))
                })
                .collect();
            (!items.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(shell::heading((*name).into(), &t).px_2().pt_3().pb_1())
                    .children(items)
            })
        });
        div()
            .w(px(208.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(t.border)
            .bg(t.chrome(t.sidebar))
            .child(
                div().flex_none().p_2().child(
                    Input::new(&self.search)
                        .small()
                        .prefix(Icon::new(IconName::Search).xsmall())
                        .cleanable(true),
                ),
            )
            .child(
                div()
                    .id("settings-nav")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_2()
                    .children(groups),
            )
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let query = self.search.read(cx).value().trim().to_lowercase();
        // Only the open section is built, unless a search needs every row to look in.
        let (shown, page) = if query.is_empty() {
            (
                Section::ALL.to_vec(),
                Some((self.section, self.page(self.section, cx))),
            )
        } else {
            let mut found: Vec<(Section, Page)> = Section::ALL
                .into_iter()
                .filter_map(|s| {
                    let mut page = self.page(s, cx);
                    page.filter(&query, cx).then_some((s, page))
                })
                .collect();
            let shown: Vec<Section> = found.iter().map(|(s, _)| *s).collect();
            let at = found
                .iter()
                .position(|(s, _)| *s == self.section)
                .unwrap_or(0);
            let page = (!found.is_empty()).then(|| found.swap_remove(at));
            (shown, page)
        };
        let content = match page {
            Some((section, page)) => page.view(section.title(), &self.scroll).into_any_element(),
            None => div()
                .flex_1()
                .flex()
                .justify_center()
                .pt(px(48.))
                .text_size(rems(shell::TITLE_TEXT))
                .text_color(t.subtle)
                .child(format!("No settings match “{query}”"))
                .into_any_element(),
        };
        div()
            .id("settings-window")
            .key_context(keymap::ROOT_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::escape))
            .size_full()
            .flex()
            .flex_col()
            .bg(t.chrome(t.elevated))
            .text_color(t.foreground)
            .font_family(t.fonts.ui.clone())
            .font_weight(t.fonts.ui_weight)
            .text_size(t.ui_text())
            .on_action(|_: &tab::Close, window, _| window.remove_window())
            .on_action(|_: &workbench::OpenSettings, _, _| {})
            .on_action(
                cx.listener(|this, _: &help::Shortcuts, _, cx| this.show(Section::Shortcuts, cx)),
            )
            .on_action(|_: &view::ZoomIn, _, cx| Settings::step_ui_scale(cx, 1))
            .on_action(|_: &view::ZoomOut, _, cx| Settings::step_ui_scale(cx, -1))
            .on_action(|_: &view::ZoomReset, _, cx| Settings::step_ui_scale(cx, 0))
            .child(
                div()
                    .h(windows::TOP_BAR_HEIGHT)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl(px(84.))
                    .pr_2()
                    .border_b_1()
                    .border_color(t.border)
                    .bg(t.chrome(t.sidebar))
                    .child(
                        div()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("Settings"),
                    )
                    .child(
                        Button::new("reset-all-settings")
                            .ghost()
                            .small()
                            .icon(IconName::RotateCcw)
                            .label("Reset All Settings…")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.reset_all(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.sidebar(&shown, cx))
                    .child(content),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_round_trip_their_ids() {
        for s in Section::ALL {
            assert_eq!(Section::parse(s.id()), Some(s));
        }
        assert_eq!(Section::parse("SHORTCUTS"), Some(Section::Shortcuts));
        assert_eq!(Section::parse("nope"), None);
    }

    #[test]
    fn the_sidebar_groups_list_every_section_once_in_order() {
        let listed: Vec<Section> = GROUPS.iter().flat_map(|(_, s)| s.iter().copied()).collect();
        assert_eq!(listed, Section::ALL);
    }

    /// Opens the window as ⌘, does, drawn: its handle and its view.
    fn opened(cx: &mut gpui_kit::TestAppContext) -> (AnyWindowHandle, WeakEntity<SettingsWindow>) {
        cx.update(|cx| open(None, cx));
        cx.run_until_parked();
        let (handle, view) = cx.update(|cx| cx.global::<Open>().0.clone()).unwrap();
        let _ = cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        (handle, view)
    }

    struct Main;
    impl Render for Main {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// ⌘W and Esc close the window as soon as it opens: out of the window list, its views
    /// dropped. Esc in a filled filter empties it first.
    #[gpui_kit::test]
    fn cmd_w_and_escape_close_the_window_and_drop_it(cx: &mut gpui_kit::TestAppContext) {
        let dir = std::env::temp_dir().join(format!("gitviber-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(gitviber_ui::init);
        let main: AnyWindowHandle = cx
            .add_window(|window, cx| gpui_kit::base::Root::new(cx.new(|_| Main), window, cx))
            .into();
        // As on macOS, the main window comes forward once Settings is gone. (The test platform
        // also keeps the last active window, and a text field's input handler with it, till then.)
        let closed = |handle: AnyWindowHandle,
                      view: WeakEntity<SettingsWindow>,
                      cx: &mut gpui_kit::TestAppContext| {
            cx.run_until_parked();
            assert!(!cx.update(|cx| cx.windows().contains(&handle)));
            assert!(view.upgrade().is_none());
            let _ = main.update(cx, |_, window, _| window.activate_window());
            cx.run_until_parked();
        };
        let close = |key: &str, cx: &mut gpui_kit::TestAppContext| {
            let (handle, view) = opened(cx);
            gpui_kit::VisualTestContext::from_window(handle, cx).simulate_keystrokes(key);
            closed(handle, view, cx);
        };
        close("cmd-w", cx);
        let snapshot = cx.update(|cx| cx.leak_detector_snapshot());
        close("escape", cx);
        close("cmd-w", cx);

        let (handle, view) = opened(cx);
        let search = view
            .upgrade()
            .unwrap()
            .read_with(cx, |w, _| w.search.clone());
        let _ = handle.update(cx, |_, window, cx| {
            search.update(cx, |s, cx| {
                s.set_value("font", window, cx);
                s.focus(window, cx);
            })
        });
        let mut vcx = gpui_kit::VisualTestContext::from_window(handle, cx);
        vcx.simulate_keystrokes("escape");
        assert!(search.read_with(&vcx, |s, _| s.value().is_empty()));
        assert!(vcx.update(|_, cx| cx.windows().contains(&handle)));
        vcx.simulate_keystrokes("escape");
        drop(search);
        closed(handle, view, cx);
        cx.update(|cx| cx.assert_no_new_leaks(&snapshot));
    }
}
