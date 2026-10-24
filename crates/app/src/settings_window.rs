//! Settings (⌘,) in a window of its own, one at a time. Both windows read the same Settings
//! global, so a change here shows in the workspace as it's made. Appearance works in full; the
//! other sections set what is plain to set (switches, choices, text) and the features that
//! read them come in later.

use gitviber_ui::actions::workbench;
use gitviber_ui::keymap::{self, IS_MAC, bindings_for, format_chord};
use gitviber_ui::memory::Live;
use gitviber_ui::settings::*;
use gitviber_ui::theme::{self, ThemeId, appearance_floor};
use gitviber_ui::{ActiveAppTheme, Settings, windows};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage,
    Settings as SettingsView,
};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Global, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
    px, size,
};

#[derive(Default)]
struct Open(Option<AnyWindowHandle>);
impl Global for Open {}

pub fn install(cx: &mut App) {
    cx.on_action(|_: &workbench::OpenSettings, cx| open(cx));
}

/// Shows the settings window, bringing it forward when it's open already.
pub fn open(cx: &mut App) {
    if let Some(handle) = cx.default_global::<Open>().0
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let options = windows::options(None, size(px(860.), px(640.)), size(px(620.), px(420.)), cx);
    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| SettingsWindow::new(window, cx))
    }) {
        Ok((handle, _)) => cx.global_mut::<Open>().0 = Some(handle),
        Err(err) => eprintln!("gitviber: couldn't open the settings window: {err}"),
    }
}

pub struct SettingsWindow {
    opacity: Entity<SliderState>,
    blur: Entity<SliderState>,
    _live: Live,
    _subscriptions: Vec<Subscription>,
}

impl SettingsWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = Settings::get(cx).clone();
        let opacity = cx.new(|_| {
            SliderState::new()
                .min(WINDOW_OPACITY.min)
                .max(WINDOW_OPACITY.max)
                .step(WINDOW_OPACITY.step)
                .default_value(s.window_opacity)
        });
        let blur = cx.new(|_| {
            SliderState::new()
                .min(BACKGROUND_BLUR.min)
                .max(BACKGROUND_BLUR.max)
                .step(BACKGROUND_BLUR.step)
                .default_value(s.background_blur)
        });
        let mut subs = theme::attach(window, cx);
        subs.push(cx.subscribe(&opacity, |_, _, e: &SliderEvent, cx| {
            if let SliderEvent::Change(v) = e {
                let floor = appearance_floor(Settings::get(cx));
                let v = v.start().max(floor);
                Settings::update(cx, |s| s.window_opacity = v);
            }
        }));
        subs.push(cx.subscribe(&blur, |_, _, e: &SliderEvent, cx| {
            if let SliderEvent::Change(v) = e {
                let v = v.start();
                Settings::update(cx, |s| s.background_blur = v);
            }
        }));
        subs.push(cx.observe_global::<Settings>(|_, cx| cx.notify()));
        window.set_window_title("Settings");
        Self {
            opacity,
            blur,
            _live: Live::new("SettingsWindow"),
            _subscriptions: subs,
        }
    }
}

type Get<T> = fn(&Settings) -> T;
type Set<T> = fn(&mut Settings, T);
type SetStr = for<'a> fn(&mut Settings, &'a str);

fn switch(title: &'static str, hint: &'static str, get: Get<bool>, set: Set<bool>) -> SettingItem {
    let default = get(&Settings::default());
    SettingItem::new(
        title,
        SettingField::switch(
            move |cx| get(Settings::get(cx)),
            move |v, cx| Settings::update(cx, |s| set(s, v)),
        )
        .default_value(default),
    )
    .description(hint)
}

fn choice(
    title: &'static str,
    hint: &'static str,
    options: Vec<(String, String)>,
    get: Get<String>,
    set: SetStr,
) -> SettingItem {
    let default = get(&Settings::default());
    let options = options
        .into_iter()
        .map(|(v, l)| (SharedString::from(v), SharedString::from(l)))
        .collect();
    SettingItem::new(
        title,
        SettingField::scrollable_dropdown(
            options,
            move |cx| get(Settings::get(cx)).into(),
            move |v, cx| Settings::update(cx, |s| set(s, &v)),
        )
        .default_value(SharedString::from(default)),
    )
    .description(hint)
}

fn text(title: &'static str, hint: &'static str, get: Get<String>, set: SetStr) -> SettingItem {
    SettingItem::new(
        title,
        SettingField::input(
            move |cx| get(Settings::get(cx)).into(),
            move |v, cx| Settings::update(cx, |s| set(s, &v)),
        ),
    )
    .description(hint)
}

fn number(
    title: &'static str,
    hint: &'static str,
    min: f64,
    max: f64,
    step: f64,
    get: Get<f64>,
    set: Set<f64>,
) -> SettingItem {
    let default = get(&Settings::default());
    SettingItem::new(
        title,
        SettingField::number_input(
            NumberFieldOptions { min, max, step },
            move |cx| get(Settings::get(cx)),
            move |v, cx| Settings::update(cx, |s| set(s, v)),
        )
        .default_value(default),
    )
    .description(hint)
}

fn list<T: ToString + Copy>(values: &[T], label: impl Fn(T) -> String) -> Vec<(String, String)> {
    values.iter().map(|v| (v.to_string(), label(*v))).collect()
}

fn themes(dark: bool) -> Vec<(String, String)> {
    ThemeId::ALL
        .iter()
        .filter(|t| t.is_dark() == dark)
        .map(|t| (t.as_str().to_string(), t.label().to_string()))
        .collect()
}

fn pairs(table: &[(&str, &str)]) -> Vec<(String, String)> {
    table
        .iter()
        .map(|(v, l)| (v.to_string(), l.to_string()))
        .collect()
}

fn names<T: Copy>(
    all: impl Iterator<Item = T>,
    name: fn(T) -> &'static str,
) -> Vec<(String, String)> {
    all.map(|v| (name(v).to_string(), name(v).to_string()))
        .collect()
}

fn appearance_page(opacity: &Entity<SliderState>, blur: &Entity<SliderState>) -> SettingPage {
    let desktop = if IS_MAC { "macOS" } else { "your desktop" };
    let mut theme = SettingGroup::new().title("Theme").items([
        choice(
            "Theme",
            if IS_MAC {
                "System follows macOS between the light and dark themes below."
            } else {
                "System follows your desktop between the light and dark themes below."
            },
            vec![
                ("system".into(), "System".into()),
                ("light".into(), "Light".into()),
                ("dark".into(), "Dark".into()),
            ],
            |s| s.appearance.as_str().into(),
            |s, v| s.appearance = Appearance::parse(v).unwrap_or(s.appearance),
        ),
        // Picking a theme picks its own syntax theme too; either can be changed after.
        choice(
            "Dark theme",
            "The whole app's colors while it's dark. Dimmed is a softer, lighter dark.",
            themes(true),
            |s| s.dark_theme.as_str().into(),
            |s, v| {
                if let Some(t) = ThemeId::parse(v) {
                    s.dark_theme = t;
                    s.syntax_theme = t.syntax().into();
                }
            },
        ),
        choice(
            "Light theme",
            "The whole app's colors while it's light.",
            themes(false),
            |s| s.light_theme.as_str().into(),
            |s, v| {
                if let Some(t) = ThemeId::parse(v) {
                    s.light_theme = t;
                    s.light_syntax_theme = t.syntax().into();
                }
            },
        ),
    ]);
    let _ = desktop;
    // macOS only: elsewhere the window can't be made clear this way.
    if IS_MAC {
        let opacity = opacity.clone();
        let blur = blur.clone();
        theme = theme.items([
            SettingItem::new(
                "Window opacity",
                SettingField::render(move |_, _, cx| {
                    let floor = appearance_floor(Settings::get(cx));
                    let v = Settings::get(cx).window_opacity.max(floor);
                    div().flex().items_center().gap_3().w(px(240.)).child(Slider::new(&opacity).flex_1()).child(div().w(px(44.)).text_right().child(format!("{v:.0}%")))
                }),
            )
            .description("Below 100%, the desktop shows through the whole window, down to the lowest the chosen light and dark themes both stay readable at. Text, dialogs and menus stay solid."),
            SettingItem::new(
                "Background blur",
                SettingField::render(move |_, _, cx| {
                    let s = Settings::get(cx);
                    let (v, off) = (s.background_blur, s.window_opacity >= 100.);
                    div().flex().items_center().gap_3().w(px(240.)).child(Slider::new(&blur).disabled(off).flex_1()).child(div().w(px(44.)).text_right().child(format!("{v:.0} px")))
                }),
            )
            .description("Blurs the desktop behind a see-through window. GPUI blurs at one strength: any value above 0 turns it on."),
            switch("Terminal too", "Lets the desktop show through the terminal at the same opacity and blur.", |s| s.terminal_glass, |s, v| s.terminal_glass = v),
        ]);
    }
    SettingPage::new("Appearance").icon(IconName::Palette).groups([
        theme,
        // Each appearance keeps its own syntax theme, so switching back restores it.
        SettingGroup::new().title("Code colors").items([
            choice("Dark syntax theme", "Code colors while the app is dark.", pairs(SYNTAX_THEMES), |s| s.syntax_theme.clone(), |s, v| s.syntax_theme = v.into()),
            choice("Light syntax theme", "Code colors while the app is light.", pairs(LIGHT_SYNTAX_THEMES), |s| s.light_syntax_theme.clone(), |s, v| s.light_syntax_theme = v.into()),
        ]),
        SettingGroup::new().title("Interface").items([
            choice("Interface font", "The code font is under Editor, the terminal's under Terminal.", names(UiFont::choices(), UiFont::as_str), |s| s.ui_font.as_str().into(), |s, v| s.ui_font = UiFont::parse(v).unwrap_or(s.ui_font)),
            text("Custom interface font", "An installed font's name, used when Interface font is Custom.", |s| s.custom_ui_font.clone(), |s, v| s.custom_ui_font = v.into()),
            choice("Interface font weight", "Labels set in medium or semibold stay a step bolder.", UI_FONT_WEIGHTS.iter().map(|(w, l)| (w.to_string(), l.to_string())).collect(), |s| s.ui_font_weight.to_string(), |s, v| s.ui_font_weight = v.parse().unwrap_or(s.ui_font_weight)),
            choice("Interface scale", "Zooms the whole window (⌘= ⌘− ⌘0). The code and terminal font sizes stay their own settings.", list(UI_SCALES, |z| format!("{:.0}%", z * 100.)), |s| s.ui_scale.to_string(), |s, v| s.ui_scale = v.parse().unwrap_or(s.ui_scale)),
            switch("Projects in the sidebar", "List your projects at the top of the left panel, to switch with one click.", |s| s.sidebar_projects, |s, v| s.sidebar_projects = v),
        ]),
    ])
}

fn editor_page() -> SettingPage {
    SettingPage::new("Editor").icon(IconName::Code).groups([
        SettingGroup::new().title("Font").items([
            choice(
                "Font",
                "The code view's font.",
                names(CodeFont::choices(), CodeFont::as_str),
                |s| s.code_font.as_str().into(),
                |s, v| s.code_font = CodeFont::parse(v).unwrap_or(s.code_font),
            ),
            text(
                "Custom font",
                "An installed font's name, used when Font is Custom.",
                |s| s.custom_code_font.clone(),
                |s, v| s.custom_code_font = v.into(),
            ),
            number(
                "Font size",
                "⌥⌘= ⌥⌘− ⌥⌘0 change it too.",
                CODE_FONT_MIN as f64,
                CODE_FONT_MAX as f64,
                0.5,
                |s| s.code_font_size as f64,
                |s, v| s.code_font_size = v as f32,
            ),
            choice(
                "Font weight",
                "Bold text goes two steps up from it.",
                CODE_FONT_WEIGHTS
                    .iter()
                    .map(|(w, l)| (w.to_string(), l.to_string()))
                    .collect(),
                |s| s.code_font_weight.to_string(),
                |s, v| s.code_font_weight = v.parse().unwrap_or(s.code_font_weight),
            ),
            number(
                "Line height",
                "A multiple of the font size.",
                1.0,
                3.0,
                0.1,
                |s| s.line_height as f64,
                |s, v| s.line_height = v as f32,
            ),
            switch(
                "Font ligatures",
                "Draws => and != as single glyphs where the font has them.",
                |s| s.ligatures,
                |s, v| s.ligatures = v,
            ),
        ]),
        SettingGroup::new().title("Behavior").items([
            switch(
                "Word wrap",
                "Long lines wrap instead of scrolling sideways (⌥Z).",
                |s| s.word_wrap,
                |s, v| s.word_wrap = v,
            ),
            switch(
                "Open Markdown as preview",
                "Markdown files open rendered; diffs always start on the diff.",
                |s| s.markdown_preview,
                |s, v| s.markdown_preview = v,
            ),
        ]),
    ])
}

fn diff_page() -> SettingPage {
    SettingPage::new("Diff")
        .icon(IconName::GitCompareArrows)
        .group(SettingGroup::new().items([
            choice(
                "Layout",
                "Unified or side by side (⌥S).",
                vec![
                    ("false".into(), "Unified".into()),
                    ("true".into(), "Split".into()),
                ],
                |s| s.side_by_side.to_string(),
                |s, v| s.side_by_side = v == "true",
            ),
            switch(
                "Collapse unchanged lines",
                "Long runs of unchanged lines fold away (⌥C).",
                |s| s.hide_unchanged,
                |s, v| s.hide_unchanged = v,
            ),
            switch(
                "Ignore whitespace",
                "Hides lines whose only change is whitespace (⌥W).",
                |s| s.ignore_whitespace,
                |s, v| s.ignore_whitespace = v,
            ),
            choice(
                "Whitespace ignored",
                "Changes in its amount, or any whitespace at all.",
                vec![
                    ("amount".into(), "Amount".into()),
                    ("all".into(), "All".into()),
                ],
                |s| s.whitespace_mode.as_str().into(),
                |s, v| s.whitespace_mode = Whitespace::parse(v).unwrap_or(s.whitespace_mode),
            ),
        ]))
}

fn terminal_page() -> SettingPage {
    let fonts = std::iter::once(TerminalFont::Editor).chain(
        TerminalFont::ALL
            .iter()
            .copied()
            .filter(|f| *f != TerminalFont::Editor && (IS_MAC || !f.mac_only())),
    );
    let mut shell = vec![
        switch(
            "Shell integration",
            "New terminals mark each command, for ⌘↑ / ⌘↓ between prompts (zsh, bash 4.4+).",
            |s| s.shell_integration,
            |s, v| s.shell_integration = v,
        ),
        choice(
            "Resume agents on restore",
            "A restored terminal an agent ran in: its resume command typed at the prompt, run, or neither.",
            vec![
                ("type".into(), "Type it".into()),
                ("run".into(), "Run it".into()),
                ("off".into(), "Off".into()),
            ],
            |s| s.resume_agents.as_str().into(),
            |s, v| s.resume_agents = ResumeMode::parse(v).unwrap_or(s.resume_agents),
        ),
        switch(
            "Ask before quitting while agents are working",
            "Also while a command runs or a push or pull is on its way.",
            |s| s.ask_before_quit,
            |s, v| s.ask_before_quit = v,
        ),
    ];
    if IS_MAC {
        shell.push(choice(
            "Option as Meta",
            "What ⌥ does in the terminal: type characters, or send Meta.",
            vec![
                ("off".into(), "Off".into()),
                ("left".into(), "Left ⌥".into()),
                ("both".into(), "Both ⌥".into()),
            ],
            |s| s.option_as_meta.as_str().into(),
            |s, v| s.option_as_meta = OptionKey::parse(v).unwrap_or(s.option_as_meta),
        ));
    }
    SettingPage::new("Terminal")
        .icon(IconName::SquareTerminal)
        .groups([
            SettingGroup::new().title("Font").items([
                choice(
                    "Font",
                    "Editor follows the code font.",
                    names(fonts, TerminalFont::as_str),
                    |s| s.terminal_font.as_str().into(),
                    |s, v| s.terminal_font = TerminalFont::parse(v).unwrap_or(s.terminal_font),
                ),
                text(
                    "Custom font",
                    "An installed font's name, used when Font is Custom.",
                    |s| s.custom_terminal_font.clone(),
                    |s, v| s.custom_terminal_font = v.into(),
                ),
                number(
                    "Font size",
                    "⌘= ⌘− ⌘0 in the terminal change it too.",
                    TERMINAL_FONT_MIN as f64,
                    TERMINAL_FONT_MAX as f64,
                    1.0,
                    |s| s.terminal_font_size as f64,
                    |s, v| s.terminal_font_size = v as f32,
                ),
                choice(
                    "Line height",
                    "A multiple of the font size.",
                    list(TERMINAL_LINE_HEIGHTS, |h| format!("{h}")),
                    |s| s.terminal_line_height.to_string(),
                    |s, v| s.terminal_line_height = v.parse().unwrap_or(s.terminal_line_height),
                ),
            ]),
            SettingGroup::new().title("Cursor").items([
                choice(
                    "Style",
                    "",
                    vec![
                        ("block".into(), "Block".into()),
                        ("bar".into(), "Bar".into()),
                        ("underline".into(), "Underline".into()),
                    ],
                    |s| s.terminal_cursor.as_str().into(),
                    |s, v| {
                        s.terminal_cursor = TerminalCursor::parse(v).unwrap_or(s.terminal_cursor)
                    },
                ),
                switch(
                    "Blink",
                    "",
                    |s| s.terminal_cursor_blink,
                    |s, v| s.terminal_cursor_blink = v,
                ),
            ]),
            SettingGroup::new().title("Panes").items([
                choice(
                    "Dim unfocused panes",
                    "How far a split tab's other panes fade.",
                    list(DIM_LEVELS, |d| {
                        if d == 0 {
                            "Off".into()
                        } else {
                            format!("{d}%")
                        }
                    }),
                    |s| s.terminal_inactive_dim.to_string(),
                    |s, v| s.terminal_inactive_dim = v.parse().unwrap_or(s.terminal_inactive_dim),
                ),
                choice(
                    "Scrollback",
                    "Lines of history each terminal keeps.",
                    list(SCROLLBACK_LINES, |n| format!("{n} lines")),
                    |s| s.terminal_scrollback.to_string(),
                    |s, v| s.terminal_scrollback = v.parse().unwrap_or(s.terminal_scrollback),
                ),
            ]),
            SettingGroup::new().title("Shell").items(shell),
            SettingGroup::new().title("Accessibility").items([switch(
                "Screen reader mode",
                "Keeps the rows readable to a screen reader and announces new output.",
                |s| s.terminal_screen_reader,
                |s, v| s.terminal_screen_reader = v,
            )]),
        ])
}

fn shortcuts_page() -> SettingPage {
    let item = |c: &'static keymap::Command| {
        SettingItem::render(move |_, _, cx| {
            let keys = bindings_for(c, &Settings::get(cx).keybindings, IS_MAC);
            let t = cx.app_theme();
            div()
                .flex()
                .items_center()
                .justify_between()
                .w_full()
                .py_1()
                .child(div().child(c.title))
                .child(
                    div()
                        .font_family(t.fonts.code.clone())
                        .text_color(t.muted_foreground)
                        .child(
                            keys.iter()
                                .map(|k| format_chord(k, IS_MAC))
                                .collect::<Vec<_>>()
                                .join("  "),
                        ),
                )
        })
        .keywords([c.title, c.id])
    };
    let mut groups: Vec<SettingGroup> = Vec::new();
    let mut categories: Vec<keymap::Category> =
        keymap::all_commands().map(|c| c.category).collect();
    categories.dedup();
    categories.sort();
    categories.dedup();
    for category in categories {
        groups.push(
            SettingGroup::new().title(category.label()).items(
                keymap::all_commands()
                    .filter(|c| c.category == category)
                    .map(item),
            ),
        );
    }
    SettingPage::new("Keyboard Shortcuts")
        .icon(IconName::Keyboard)
        .description("Rebinding comes with the shortcut editor; the keybindings in settings.json apply when GitViber starts.")
        .resettable(false)
        .groups(groups)
}

fn notifications_page() -> SettingPage {
    SettingPage::new("Notifications")
        .icon(IconName::Bell)
        .group(SettingGroup::new().items([
            switch(
                "Desktop notifications",
                "While GitViber is in the background.",
                |s| s.notify,
                |s, v| s.notify = v,
            ),
            switch(
                "An agent finished",
                "",
                |s| s.notify_agent_done,
                |s, v| s.notify_agent_done = v,
            ),
            switch(
                "An agent waits for an answer",
                "",
                |s| s.notify_agent_waiting,
                |s, v| s.notify_agent_waiting = v,
            ),
            switch(
                "A terminal rang its bell",
                "Or sent a notification.",
                |s| s.notify_terminal,
                |s, v| s.notify_terminal = v,
            ),
            switch(
                "A long command ended",
                "Out of sight; its tab is marked too.",
                |s| s.notify_long_command,
                |s, v| s.notify_long_command = v,
            ),
            choice(
                "Long means longer than",
                "",
                list(LONG_COMMAND_SECONDS, |n| {
                    if n < 60 {
                        format!("{n} s")
                    } else {
                        format!("{} min", n / 60)
                    }
                }),
                |s| s.long_command_seconds.to_string(),
                |s, v| s.long_command_seconds = v.parse().unwrap_or(s.long_command_seconds),
            ),
            switch(
                "A push, pull, fetch, clone or commit ended",
                "",
                |s| s.notify_git,
                |s, v| s.notify_git = v,
            ),
        ]))
}

fn open_in_page() -> SettingPage {
    SettingPage::new("Open In")
        .icon(IconName::SquareArrowOutUpRight)
        .group(SettingGroup::new().items([
            text(
                "Default app",
                "A built-in app's id or one of yours; set from the Open in menu.",
                |s| s.open_in_app.clone(),
                |s, v| s.open_in_app = v.into(),
            ),
            switch(
                "Show detected apps",
                "Off lists only your own apps.",
                |s| !s.open_in_hide_builtins,
                |s, v| s.open_in_hide_builtins = !v,
            ),
        ]))
}

fn updates_page() -> SettingPage {
    SettingPage::new("Updates")
        .icon(IconName::CircleArrowUp)
        .group(SettingGroup::new().items([switch(
            "Check for updates automatically",
            "At launch and every few hours.",
            |s| s.auto_update,
            |s, v| s.auto_update = v,
        )]))
}

fn obsidian_page() -> SettingPage {
    SettingPage::new("Obsidian").icon(IconName::Gem).group(
        SettingGroup::new().title("Vaults").items([switch(
            "Show vaults in the explorer",
            "When Obsidian lists any.",
            |s| s.obsidian,
            |s, v| s.obsidian = v,
        )]),
    )
}

fn browser_page() -> SettingPage {
    SettingPage::new("Browser").icon(IconName::Globe).groups([
        SettingGroup::new().title("Memory").items([
            choice(
                "Pages kept alive out of sight",
                "",
                list(BROWSER_LIVE_HIDDEN, |n| n.to_string()),
                |s| s.browser_live_hidden.to_string(),
                |s, v| s.browser_live_hidden = v.parse().unwrap_or(s.browser_live_hidden),
            ),
            choice(
                "Close a page out of sight after",
                "",
                list(BROWSER_PARK_AFTER, |n| {
                    if n == 0 {
                        "Never".into()
                    } else {
                        format!("{n} min")
                    }
                }),
                |s| s.browser_park_after_min.to_string(),
                |s, v| s.browser_park_after_min = v.parse().unwrap_or(s.browser_park_after_min),
            ),
        ]),
        SettingGroup::new().title("Agents").items([
            switch(
                "Let agents use the browser",
                "With `gitviber browser` in their terminal.",
                |s| s.browser_agent_control,
                |s, v| s.browser_agent_control = v,
            ),
            switch(
                "Teach Claude Code and Codex (skill)",
                "",
                |s| s.browser_agent_skill,
                |s, v| s.browser_agent_skill = v,
            ),
            switch(
                "Show agents' tabs",
                "",
                |s| s.browser_show_agent_tabs,
                |s, v| s.browser_show_agent_tabs = v,
            ),
            switch(
                "Mute agents' tabs",
                "",
                |s| s.browser_mute_agent_tabs,
                |s, v| s.browser_mute_agent_tabs = v,
            ),
            choice(
                "Agent tabs drawing at once",
                "",
                list(BROWSER_AGENT_DRAWN, |n| n.to_string()),
                |s| s.browser_agent_drawn.to_string(),
                |s, v| s.browser_agent_drawn = v.parse().unwrap_or(s.browser_agent_drawn),
            ),
            switch(
                "Reload when an agent finishes",
                "Unless the dev server reloads the page itself.",
                |s| s.browser_reload_on_agent_done,
                |s, v| s.browser_reload_on_agent_done = v,
            ),
        ]),
        SettingGroup::new().title("Console").items([switch(
            "Keep the page's console",
            "Errors and warnings, for the console badge.",
            |s| s.browser_console,
            |s, v| s.browser_console = v,
        )]),
    ])
}

fn git_page() -> SettingPage {
    SettingPage::new("Git").icon(IconName::GitBranch).groups([
        SettingGroup::new().title("Background").items([choice(
            "Fetch in the background",
            "A fetch can prompt for an SSH key every time.",
            list(FETCH_INTERVALS, |n| {
                if n == 0 {
                    "Off".into()
                } else {
                    format!("Every {n} min")
                }
            }),
            |s| s.background_fetch.to_string(),
            |s, v| s.background_fetch = v.parse().unwrap_or(s.background_fetch),
        )]),
        SettingGroup::new().title("Worktrees").items([text(
            "One folder for all worktrees",
            "New worktrees go in <folder>/<project>/ rather than beside the project. Empty is off.",
            |s| s.worktree_root.clone().unwrap_or_default(),
            |s, v| s.worktree_root = (!v.is_empty()).then(|| v.to_string()),
        )]),
    ])
}

fn commit_page() -> SettingPage {
    SettingPage::new("Commit Messages")
        .icon(IconName::Sparkles)
        .group(SettingGroup::new().title("Suggestions").items([
            switch(
                "Suggest commit messages",
                "A ✦ button in the commit box runs the command below. Off: nothing is ever run.",
                |s| s.suggest_enabled,
                |s, v| s.suggest_enabled = v,
            ),
            text(
                "Command",
                "Your agent CLI, split like a shell command line.",
                |s| s.suggest_command.clone(),
                |s, v| s.suggest_command = v.into(),
            ),
        ]))
}

fn review_page() -> SettingPage {
    SettingPage::new("Guided Review")
        .icon(IconName::ScanSearch)
        .group(SettingGroup::new().title("Writing").items([text(
            "Language",
            "Guided reviews are written in it; commit messages and pull requests stay English.",
            |s| s.review_language.clone(),
            |s, v| s.review_language = v.into(),
        )]))
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let sidebar = gpui_kit::StyleRefinement::default().bg(t.chrome(t.sidebar));
        div()
            .key_context(keymap::ROOT_CONTEXT)
            .size_full()
            .flex()
            .flex_col()
            .bg(t.chrome(t.elevated))
            .text_color(t.foreground)
            .font_family(t.fonts.ui.clone())
            .font_weight(t.fonts.ui_weight)
            .text_size(t.ui_text())
            .child(
                div()
                    .h(windows::TOP_BAR_HEIGHT)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_b_1()
                    .border_color(t.border)
                    .bg(t.chrome(t.sidebar))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Settings"),
            )
            .child(
                div().flex_1().min_h_0().child(
                    SettingsView::new("settings")
                        .small()
                        .sidebar_width(px(220.))
                        .sidebar_style(&sidebar)
                        .pages([
                            appearance_page(&self.opacity, &self.blur),
                            shortcuts_page(),
                            open_in_page(),
                            notifications_page(),
                            updates_page(),
                            editor_page(),
                            diff_page(),
                            terminal_page(),
                            obsidian_page(),
                            browser_page(),
                            git_page(),
                            commit_page(),
                            review_page(),
                        ]),
                ),
            )
    }
}
