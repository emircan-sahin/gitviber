//! The sections that set what the app looks like and how its views and terminals behave:
//! Appearance, Editor, Diff, Terminal, Open In, Notifications, Obsidian, Browser and Updates.

use gitviber_core::notifications::Permission;
use gitviber_ui::actions::browser as browser_cmds;
use gitviber_ui::keymap::IS_MAC;
use gitviber_ui::settings::*;
use gitviber_ui::theme::{ThemeId, appearance_floor};
use gitviber_ui::{ActiveAppTheme, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::base::input::Rope;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::highlighter::SyntaxHighlighter;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderState};
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Entity, IntoElement, ParentElement as _, SharedString,
    Styled as _, StyledText, Subscription, Window, div, prelude::FluentBuilder as _, px, rems,
};

use super::SettingsWindow;
use super::fields::{
    action, choice, font_choice, note, options, pre, segmented, stepper, switch, text, toggle,
    with_keys,
};
use super::shell::{Group, Page, Row};

/// What the System theme follows.
const DESKTOP: &str = if IS_MAC { "macOS" } else { "your desktop" };

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

/// A custom font field's hint: the font it falls back to shows when the name isn't installed.
fn font_hint(custom: &str, resolved: &str) -> String {
    let name = custom.trim();
    if !name.is_empty() && name != resolved {
        "Not installed: the default font shows instead.".into()
    } else {
        "Any installed font's name.".into()
    }
}

fn slider_row(slider: &Entity<SliderState>, label: String, disabled: bool) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_3()
        .w(px(240.))
        .child(Slider::new(slider).disabled(disabled).flex_1())
        .child(div().w(px(48.)).text_right().child(label))
        .into_any_element()
}

/// What Background blur stores when switched on: the old app's middle radius.
const BLUR_ON: f32 = 20.;

pub fn appearance(w: &SettingsWindow, cx: &App) -> Page {
    let s = Settings::get(cx);
    let mut theme = vec![segmented(
        "appearance",
        "Theme",
        format!("System follows {DESKTOP} between your light and dark themes."),
        vec![
            ("system".into(), "System".into()),
            ("light".into(), "Light".into()),
            ("dark".into(), "Dark".into()),
        ],
        |s| s.appearance.as_str().into(),
        |s, v| s.appearance = Appearance::parse(v).unwrap_or(s.appearance),
    )];
    // Picking a theme picks its own syntax theme too; either can be changed after.
    if s.appearance != Appearance::Light {
        theme.push(choice(
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
        ));
    }
    if s.appearance != Appearance::Dark {
        theme.push(choice(
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
        ));
    }
    // macOS only: elsewhere the window can't be made clear this way.
    let mut glass = Vec::new();
    if IS_MAC {
        let floor = appearance_floor(s);
        let opacity = w.opacity.clone();
        let solid = s.window_opacity >= 100.;
        glass.push(
            Row::new("Window opacity", move |_, _, cx| {
                let floor = appearance_floor(Settings::get(cx));
                let v = Settings::get(cx).window_opacity.max(floor);
                slider_row(&opacity, format!("{v:.0}%"), false)
            })
            .on_reset(
                |cx| Settings::get(cx).window_opacity != 100.,
                |cx| Settings::update(cx, |s| s.window_opacity = 100.),
            )
            .description(format!(
                "Below 100%, the desktop shows through the whole window, down to {floor:.0}%, the \
                 lowest the chosen light and dark themes both stay readable at. Text, dialogs and \
                 menus stay solid, as does the window in full screen."
            )),
        );
        // macOS blurs at one strength (an NSVisualEffectView): on or off. The setting keeps the
        // old app's radius, any of which is on.
        glass.push(
            switch(
                "Background blur",
                "Blurs the desktop behind a see-through window. Off, it shows sharp.",
                |s| s.background_blur > 0.,
                |s, v| s.background_blur = if v { BLUR_ON } else { 0. },
            )
            .disabled_because(solid, SOLID),
        );
        glass.push(
            switch(
                "Terminal too",
                "Lets the desktop show through the terminal at the same opacity and blur. Colored \
                 cells (a diff, Claude Code's prompt box) stay solid, and so does a maximized terminal.",
                |s| s.terminal_glass,
                |s, v| s.terminal_glass = v,
            )
            .disabled_because(solid, SOLID),
        );
    }
    let t = cx.app_theme();
    let mut interface = vec![font_choice(
        "Interface font",
        "The code font is under Editor, the terminal's under Terminal.",
        names(UiFont::choices(), UiFont::as_str),
        |v, _| match UiFont::parse(v)? {
            UiFont::System => Some(".SystemUIFont".into()),
            UiFont::Custom => None,
            f => Some(f.as_str().into()),
        },
        |s| s.ui_font.as_str().into(),
        |s, v| s.ui_font = UiFont::parse(v).unwrap_or(s.ui_font),
    )];
    if s.ui_font == UiFont::Custom {
        interface.push(text(
            "custom-ui-font",
            "Custom interface font",
            font_hint(&s.custom_ui_font, &t.fonts.ui),
            "Installed font name",
            true,
            |s| s.custom_ui_font.clone(),
            |s, v| s.custom_ui_font = clean_font_name(v),
        ));
    }
    interface.extend([
        segmented(
            "ui-weight",
            "Interface font weight",
            "Labels set in medium or semibold stay a step bolder.",
            UI_FONT_WEIGHTS
                .iter()
                .map(|(w, l)| (w.to_string(), l.to_string()))
                .collect(),
            |s| s.ui_font_weight.to_string(),
            |s, v| s.ui_font_weight = v.parse().unwrap_or(s.ui_font_weight),
        ),
        segmented(
            "ui-scale",
            "Interface scale",
            with_keys(
                "Zooms the whole window. The code and terminal font sizes stay their own settings.",
                &["view.zoomIn", "view.zoomOut", "view.zoomReset"],
            ),
            options(UI_SCALES, |z| format!("{:.0}%", z * 100.)),
            |s| s.ui_scale.to_string(),
            |s, v| s.ui_scale = v.parse().unwrap_or(s.ui_scale),
        ),
        switch(
            "Projects in the sidebar",
            "List your projects at the top of the left panel, to switch with one click. The \
             project menu in the top bar stays.",
            |s| s.sidebar_projects,
            |s, v| s.sidebar_projects = v,
        ),
    ]);
    let mut page = Page::new().groups([
        Group::new().title("Theme").items(theme),
        Group::new().title("Interface").items(interface),
    ]);
    // Last: set once, if ever.
    if !glass.is_empty() {
        page = page.group(Group::new().title("Transparency").items(glass));
    }
    page
}

const SOLID: &str = "Needs Window opacity below 100%.";

const CODE_SAMPLE: &str = "const total = items.filter((x) => x.price >= 10)\n  .reduce((sum, x) => sum + x.price, 0); // != === =>";

/// The Editor preview's code parsed for its colors; blocking, so run it in the background.
pub fn parse_sample() -> SyntaxHighlighter {
    let mut h = SyntaxHighlighter::new("javascript");
    h.update(None, &Rope::from(CODE_SAMPLE), None);
    h
}

/// The code or terminal font as set, on a sample; the code in the syntax theme in use.
fn sample(text: &'static str, terminal: bool, w: Option<&SettingsWindow>, cx: &App) -> Row {
    let colors = w.and_then(|w| w.sample.as_ref()).map(|h| {
        let theme = gitviber_ui::theme::syntax::current(cx);
        let styles = h.styles(&(0..text.len()), &*theme);
        (theme.style.editor_foreground, styles)
    });
    Row::custom(move |_, cx| sample_text(text, terminal, colors.clone(), cx))
}

type Colors = (
    Option<gpui_kit::Hsla>,
    Vec<(std::ops::Range<usize>, gpui_kit::HighlightStyle)>,
);

fn sample_text(text: &'static str, terminal: bool, colors: Option<Colors>, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    let s = Settings::get(cx);
    let (family, size, height) = if terminal {
        (
            t.fonts.terminal.clone(),
            t.fonts.terminal_size,
            s.terminal_line_height,
        )
    } else {
        (t.fonts.code.clone(), t.fonts.code_size, s.line_height)
    };
    div()
        .w_full()
        .px_3()
        .py_2()
        .rounded(px(6.))
        .border_1()
        .border_color(t.border)
        .bg(t.code(t.background))
        .font_family(family)
        .font_weight(t.fonts.code_weight)
        .text_size(size)
        .line_height(size * height)
        .map(|d| match colors {
            Some((fg, styles)) => d
                .text_color(fg.unwrap_or(t.foreground))
                .child(StyledText::new(text).with_highlights(styles)),
            None => d.text_color(t.muted_foreground).child(text),
        })
        .into_any_element()
}

pub fn editor(w: &SettingsWindow, cx: &App) -> Page {
    let s = Settings::get(cx);
    let t = cx.app_theme();
    let mut font = vec![font_choice(
        "Font",
        "",
        names(CodeFont::choices(), CodeFont::as_str),
        |v, _| (v != CodeFont::Custom.as_str()).then(|| v.to_string().into()),
        |s| s.code_font.as_str().into(),
        |s, v| s.code_font = CodeFont::parse(v).unwrap_or(s.code_font),
    )];
    if s.code_font == CodeFont::Custom {
        font.push(text(
            "custom-code-font",
            "Custom font",
            font_hint(&s.custom_code_font, &t.fonts.code),
            "Installed font name",
            true,
            |s| s.custom_code_font.clone(),
            |s, v| s.custom_code_font = clean_font_name(v),
        ));
    }
    font.extend([
        stepper(
            "code-size",
            "Font size",
            with_keys(
                "",
                &[
                    "editor.fontZoomIn",
                    "editor.fontZoomOut",
                    "editor.fontZoomReset",
                ],
            ),
            |s| s.code_font_size,
            |s, v| s.code_font_size = v,
            0.5,
            CODE_FONT_MIN,
            CODE_FONT_MAX,
        ),
        segmented(
            "code-weight",
            "Font weight",
            "The code view, diffs and the terminal. Bold text stays bolder.",
            CODE_FONT_WEIGHTS
                .iter()
                .map(|(w, l)| (w.to_string(), l.to_string()))
                .collect(),
            |s| s.code_font_weight.to_string(),
            |s, v| s.code_font_weight = v.parse().unwrap_or(s.code_font_weight),
        ),
        segmented(
            "line-height",
            "Line height",
            "",
            LINE_HEIGHTS
                .iter()
                .map(|(h, l)| (h.to_string(), l.to_string()))
                .collect(),
            |s| s.line_height.to_string(),
            |s, v| s.line_height = v.parse().unwrap_or(s.line_height),
        ),
        switch(
            "Font ligatures",
            "Joins pairs like => and != in fonts that have them (JetBrains Mono).",
            |s| s.ligatures,
            |s, v| s.ligatures = v,
        ),
    ]);
    let dark = cx.app_theme().dark;
    let in_use = |on: bool| if on { " The preview shows it." } else { "" };
    Page::new().groups([
        Group::new().plain().item(sample(CODE_SAMPLE, false, Some(w), cx)),
        // Each appearance keeps its own syntax theme, so switching back restores it.
        Group::new().title("Code colors").items([
            choice(
                "Dark syntax theme",
                format!("Code colors while the app is dark.{}", in_use(dark)),
                pairs(SYNTAX_THEMES),
                |s| s.syntax_theme.clone(),
                |s, v| s.syntax_theme = v.into(),
            ),
            choice(
                "Light syntax theme",
                format!("Code colors while the app is light.{}", in_use(!dark)),
                pairs(LIGHT_SYNTAX_THEMES),
                |s| s.light_syntax_theme.clone(),
                |s, v| s.light_syntax_theme = v.into(),
            ),
        ]),
        Group::new().title("Font").items(font),
        Group::new().title("Behavior").items([
            switch(
                "Word wrap",
                with_keys(
                    "Wrap long lines instead of scrolling sideways.",
                    &["editor.toggleWrap"],
                ),
                |s| s.word_wrap,
                |s, v| s.word_wrap = v,
            ),
            switch(
                "Open Markdown as preview",
                "Opening a .md file shows it rendered. Diffs of Markdown files always start on the diff.",
                |s| s.markdown_preview,
                |s, v| s.markdown_preview = v,
            ),
        ]),
    ])
}

pub fn diff() -> Page {
    Page::new().group(Group::new().items([
        segmented(
            "diff-layout",
            "Layout",
            with_keys(
                "Unified shows one column, split shows before and after side by side.",
                &["diff.toggleSplit"],
            ),
            vec![
                ("unified".into(), "Unified".into()),
                ("split".into(), "Split".into()),
            ],
            |s| if s.side_by_side { "split" } else { "unified" }.into(),
            |s, v| s.side_by_side = v == "split",
        ),
        switch(
            "Collapse unchanged lines",
            with_keys(
                "Fold long runs of unchanged code between changes.",
                &["diff.toggleCollapse"],
            ),
            |s| s.hide_unchanged,
            |s, v| s.hide_unchanged = v,
        ),
        segmented(
            "whitespace",
            "Ignore whitespace",
            with_keys(
                "Changes hides re-indented lines and trailing spaces, like git diff -b. All \
                     ignores every space, like git diff -w.",
                &["diff.toggleWhitespace"],
            ),
            vec![
                ("off".into(), "Off".into()),
                ("amount".into(), "Changes".into()),
                ("all".into(), "All".into()),
            ],
            |s| match s.diff_whitespace() {
                None => "off".into(),
                Some(w) => w.as_str().into(),
            },
            |s, v| match Whitespace::parse(v) {
                Some(w) => {
                    s.ignore_whitespace = true;
                    s.whitespace_mode = w;
                }
                None => s.ignore_whitespace = false,
            },
        ),
    ]))
}

const TERMINAL_SAMPLE: &str = "~/project main ❯ git status --short\n M src/app.ts\n?? notes.md";

pub fn terminal(cx: &App) -> Page {
    let s = Settings::get(cx);
    let t = cx.app_theme();
    let fonts: Vec<(String, String)> = TerminalFont::ALL
        .iter()
        .copied()
        .filter(|f| IS_MAC || !f.mac_only())
        .map(|f| {
            let label = if f == TerminalFont::Editor {
                "Same as editor"
            } else {
                f.as_str()
            };
            (f.as_str().to_string(), label.to_string())
        })
        .collect();
    let mut font = vec![font_choice(
        "Font",
        "Same as editor follows the code font. The font weight is the editor's.",
        fonts,
        |v, cx| match TerminalFont::parse(v)? {
            TerminalFont::Editor => Some(cx.app_theme().fonts.code.clone()),
            TerminalFont::Custom => None,
            f => Some(f.as_str().into()),
        },
        |s| s.terminal_font.as_str().into(),
        |s, v| s.terminal_font = TerminalFont::parse(v).unwrap_or(s.terminal_font),
    )];
    if s.terminal_font == TerminalFont::Custom {
        font.push(text(
            "custom-terminal-font",
            "Custom font",
            font_hint(&s.custom_terminal_font, &t.fonts.terminal),
            "Installed font name",
            true,
            |s| s.custom_terminal_font.clone(),
            |s, v| s.custom_terminal_font = clean_font_name(v),
        ));
    }
    font.extend([
        stepper(
            "terminal-size",
            "Font size",
            with_keys(
                "Separate from the code font size. With the terminal focused, the zoom keys or \
                 Ctrl+scroll change it too.",
                &[
                    "terminal.fontZoomIn",
                    "terminal.fontZoomOut",
                    "terminal.fontZoomReset",
                ],
            ),
            |s| s.terminal_font_size,
            |s, v| s.terminal_font_size = v,
            1.0,
            TERMINAL_FONT_MIN,
            TERMINAL_FONT_MAX,
        ),
        segmented(
            "terminal-line-height",
            "Line height",
            "Taller lines fit fewer rows in a pane.",
            options(TERMINAL_LINE_HEIGHTS, |h| format!("{h:.1}")),
            |s| s.terminal_line_height.to_string(),
            |s, v| s.terminal_line_height = v.parse().unwrap_or(s.terminal_line_height),
        ),
    ]);
    let mut shell = vec![
        switch(
            "Shell integration",
            with_keys(
                "Lets zsh and bash 4.4+ mark where each command starts and ends, without changing \
                 your dotfiles: a dot beside each command, red when it failed, keys to jump \
                 between them, and Copy Last Command Output. fish marks its own. Terminals opened \
                 after a change pick it up.",
                &["terminal.prevCommand", "terminal.nextCommand"],
            ),
            |s| s.shell_integration,
            |s, v| s.shell_integration = v,
        ),
        segmented(
            "resume",
            "Resume agents on restore",
            "A terminal restored from last session that was running Claude Code, Gemini CLI or \
             opencode gets the command that resumes its conversation: typed at the prompt for you \
             to run with Enter, or run at once. An agent you exited isn't resumed.",
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
            "Quitting GitViber or closing its window asks first while an agent is working or a \
             command runs in a terminal, or a push, pull or commit is on its way. An agent done \
             with its turn is resumed on the next run, so it isn't asked about. Quitting again \
             while it asks quits at once.",
            |s| s.ask_before_quit,
            |s, v| s.ask_before_quit = v,
        ),
    ];
    if IS_MAC {
        shell.push(segmented(
            "option-meta",
            "Option as Meta",
            "Off, ⌥ types characters like @ or œ. As Meta, ⌥ keys go to the shell and agents \
             instead (Claude Code's ⌥P, readline's ⌥B and ⌥F). Left ⌥ keeps the right one for \
             the characters your layout needs.",
            vec![
                ("off".into(), "Off".into()),
                ("left".into(), "Left ⌥".into()),
                ("both".into(), "Both ⌥".into()),
            ],
            |s| s.option_as_meta.as_str().into(),
            |s, v| s.option_as_meta = OptionKey::parse(v).unwrap_or(s.option_as_meta),
        ));
    }
    Page::new().groups([
        Group::new()
            .plain()
            .item(sample(TERMINAL_SAMPLE, true, None, cx)),
        Group::new().title("Font").items(font),
        Group::new().title("Cursor").items([
            segmented(
                "cursor",
                "Style",
                "",
                vec![
                    ("block".into(), "Block".into()),
                    ("bar".into(), "Bar".into()),
                    ("underline".into(), "Underline".into()),
                ],
                |s| s.terminal_cursor.as_str().into(),
                |s, v| s.terminal_cursor = TerminalCursor::parse(v).unwrap_or(s.terminal_cursor),
            ),
            switch(
                "Blink",
                "",
                |s| s.terminal_cursor_blink,
                |s, v| s.terminal_cursor_blink = v,
            ),
        ]),
        Group::new().title("Panes").items([
            segmented(
                "dim",
                "Dim unfocused panes",
                with_keys(
                    "In a split terminal, the panes other than the one you type into fade, \
                         so it's clear where the keys go.",
                    &["terminal.split", "terminal.splitDown"],
                ),
                options(DIM_LEVELS, |d| {
                    if *d == 0 {
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
                "Lines of history each terminal keeps. After a restart, a terminal comes back \
                     with its last 1,000.",
                options(SCROLLBACK_LINES, |n| format!("{} lines", thousands(*n))),
                |s| s.terminal_scrollback.to_string(),
                |s, v| s.terminal_scrollback = v.parse().unwrap_or(s.terminal_scrollback),
            ),
        ]),
        Group::new().title("Shell").items(shell),
        Group::new().title("Accessibility").items([switch(
            "Screen reader mode",
            "Lets VoiceOver read the terminal's output and the line you type. Open terminals \
                 switch at once.",
            |s| s.terminal_screen_reader,
            |s, v| s.terminal_screen_reader = v,
        )]),
    ])
}

/// 100000 → 100,000.
pub fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "None: show the list", then the apps found here (unless hidden) and the user's own.
fn open_in_choices(apps: &[gitviber_ui::open_in::OpenApp], s: &Settings) -> Vec<(String, String)> {
    let mut out = vec![(String::new(), "None: show the list".to_string())];
    out.extend(apps.iter().map(|a| (a.id.clone(), a.name.clone())));
    // The default stays listed while what it names isn't found, so it can be seen and changed.
    if !s.open_in_app.is_empty() && !out.iter().any(|(id, _)| *id == s.open_in_app) {
        out.push((s.open_in_app.clone(), s.open_in_app.clone()));
    }
    out
}

struct AppRowState {
    name: Entity<InputState>,
    command: Entity<InputState>,
    _subs: [Subscription; 2],
}

/// One of the user's own apps: name and command, saved on Enter or leaving a field.
fn custom_app_row(i: usize, app: CustomApp, window: &mut Window, cx: &mut App) -> AnyElement {
    let key = SharedString::from(format!("open-in-{}", app.id));
    let id = app.id.clone();
    let state = window.use_keyed_state(key, cx, |window, cx| {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Name")
                .default_value(app.name.clone())
        });
        let command = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("nvim-qt {file}")
                .default_value(app.command.clone())
        });
        let commit = |field: fn(&mut CustomApp, String), id: String| {
            move |_: &mut AppRowState,
                  input: Entity<InputState>,
                  e: &InputEvent,
                  cx: &mut gpui_kit::Context<AppRowState>| {
                if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().to_string();
                    let id = id.clone();
                    Settings::update(cx, move |s| {
                        if let Some(a) = s.open_in_custom.iter_mut().find(|a| a.id == id) {
                            field(a, v);
                        }
                    });
                }
            }
        };
        let subs = [
            cx.subscribe(&name, commit(|a, v| a.name = v, app.id.clone())),
            cx.subscribe(&command, commit(|a, v| a.command = v, app.id.clone())),
        ];
        AppRowState {
            name,
            command,
            _subs: subs,
        }
    });
    let (name, command) = {
        let s = state.read(cx);
        (s.name.clone(), s.command.clone())
    };
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .w_full()
        .child(div().w(px(140.)).child(Input::new(&name).small()))
        .child(div().flex_1().child(Input::new(&command).small()))
        .child(
            Button::new(("open-in-remove", i))
                .ghost()
                .small()
                .icon(IconName::X)
                .tooltip("Remove")
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    Settings::update(cx, move |s| s.open_in_custom.retain(|a| a.id != id))
                }),
        )
        .into_any_element()
}

pub fn open_in(cx: &App) -> Page {
    let choices = open_in_choices(&gitviber_ui::open_in::apps(cx), Settings::get(cx));
    Page::new().groups([
        Group::new().title("Default").items([
            choice(
                "Default app",
                with_keys(
                    "What a click on Open in (in the status bar) runs. It opens the worktree \
                         as a project, and editors that can also show the open file in it. \
                         Picking an app from its list makes that one the default.",
                    &["file.openIn"],
                ),
                choices,
                |s| s.open_in_app.clone(),
                |s, v| s.open_in_app = v.into(),
            ),
            switch(
                "Show detected apps",
                "Editors, terminals and git clients found on this computer. Turn off to list \
                     only your own apps.",
                |s| !s.open_in_hide_builtins,
                |s, v| s.open_in_hide_builtins = !v,
            ),
        ]),
        Group::new().title("Your apps").items([
            Row::custom(|_, cx| {
                note(
                    "A command run directly, not through a shell. {path} is the worktree, \
                         {file} the open file (the worktree when none is), {line} the line in \
                         view. With none of them, the worktree goes last, or from a file's \
                         right-click menu, that file. Example: code -g {file}:{line}",
                    cx,
                )
            })
            .keywords(["custom", "command", "path", "file", "line"]),
            Row::custom(|window, cx| {
                let list = Settings::get(cx).open_in_custom.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .w_full()
                    .children(
                        list.into_iter()
                            .enumerate()
                            .map(|(i, app)| custom_app_row(i, app, window, cx)),
                    )
                    .child(
                        div().flex().child(
                            Button::new("open-in-add")
                                .small()
                                .outline()
                                .icon(IconName::Plus)
                                .label("Add app")
                                .on_click(|_, _, cx| {
                                    let ms = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map_or(0, |d| d.as_millis() as u64);
                                    let id = format!("custom:{}", radix36(ms));
                                    Settings::update(cx, move |s| {
                                        s.open_in_custom.push(CustomApp {
                                            id,
                                            name: String::new(),
                                            command: String::new(),
                                        })
                                    })
                                }),
                        ),
                    )
                    .into_any_element()
            })
            .keywords(["add app"]),
        ]),
    ])
}

/// `n` in base 36, as the old app made an app's id from the time.
fn radix36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The line under the switch while notifications are on and something is in the way, and how
/// to fix it.
pub fn permission_notice(
    on: bool,
    state: Option<Permission>,
) -> Option<(&'static str, Option<&'static str>)> {
    if !on {
        return None;
    }
    Some(match state? {
        Permission::Denied => (
            "macOS isn't allowing GitViber to show notifications. Turn them on in System \
             Settings → Notifications → GitViber.",
            Some("settings"),
        ),
        Permission::Quiet => (
            "Banners are off: GitViber's alert style is None in System Settings → \
             Notifications, so they only go to Notification Center.",
            Some("settings"),
        ),
        Permission::Prompt => (
            "macOS hasn't been asked yet. Ask it, then allow GitViber in its prompt.",
            Some("ask"),
        ),
        Permission::Unbundled => (
            "This development build isn't an app bundle, so macOS can't be asked: notifications \
             can't be posted from it.",
            None,
        ),
        Permission::Granted => return None,
    })
}

pub fn notifications(w: &SettingsWindow, cx: &App) -> Page {
    let s = Settings::get(cx);
    let weak = w.weak.clone();
    let mut desktop = String::from(
        "While GitViber isn't the app in front. Clicking one brings it back, at the terminal it's about.",
    );
    if w.data.asking {
        desktop.push_str(" Waiting for your answer to macOS's prompt…");
    }
    let notice = if IS_MAC {
        permission_notice(s.notify, w.data.permission)
    } else {
        None
    };
    let mut top = vec![{
        let weak = weak.clone();
        Row::new("Desktop notifications", move |off, _, cx| {
            let weak = weak.clone();
            toggle(
                "Desktop notifications".into(),
                Settings::get(cx).notify,
                off,
                move |v, cx| {
                    Settings::update(cx, |s| s.notify = v);
                    if v {
                        let _ = weak.update(cx, |w, cx| w.ask_notifications(true, cx));
                    }
                },
                cx,
            )
        })
        .description(desktop)
        .on_reset(
            |cx| Settings::get(cx).notify,
            |cx| Settings::update(cx, |s| s.notify = false),
        )
    }];
    if let Some((text, fix)) = notice {
        let weak = weak.clone();
        top.push(
            Row::custom(move |_, cx| {
                let t = cx.app_theme();
                let weak = weak.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .text_size(rems(0.71875))
                            .text_color(t.modified)
                            .child(text),
                    )
                    .when_some(fix, |d, fix| {
                        d.child(
                            Button::new("notify-fix")
                                .small()
                                .outline()
                                .label(if fix == "ask" {
                                    "Ask macOS"
                                } else {
                                    "Open System Settings"
                                })
                                .on_click(move |_, _, cx| {
                                    if fix == "ask" {
                                        let _ =
                                            weak.update(cx, |w, cx| w.ask_notifications(false, cx));
                                    } else {
                                        super::open_notification_settings(cx);
                                    }
                                }),
                        )
                    })
                    .into_any_element()
            })
            .keywords(["permission", "macos"]),
        );
    }
    top.push(action(
        "notify-test",
        "Test notification",
        if IS_MAC {
            "Shows one now. If macOS won't show it, this says why."
        } else {
            "Shows one now, to see how they look."
        },
        "Send Test",
        move |_, cx| super::send_test_notification(cx),
    ));
    // These only post notifications; a long command also marks its tab, so it stays on.
    let off = !s.notify;
    const OFF: &str = "Desktop notifications are off.";
    let mut when = vec![
        switch(
            "An agent finishes",
            "Claude Code, or another agent that reports its state, stops working in a terminal.",
            |s| s.notify_agent_done,
            |s, v| s.notify_agent_done = v,
        )
        .disabled_because(off, OFF),
        switch(
            "An agent asks for you",
            "It waits for an answer or a permission before it goes on.",
            |s| s.notify_agent_waiting,
            |s, v| s.notify_agent_waiting = v,
        )
        .disabled_because(off, OFF),
        switch(
            "A terminal rings or notifies",
            "Any other program ringing the bell or sending a notification (OSC 9, 777 or 99). An \
             agent that reports its state goes by the two switches above.",
            |s| s.notify_terminal,
            |s, v| s.notify_terminal = v,
        )
        .disabled_because(off, OFF),
        switch(
            "A long command finishes",
            format!(
                "A command in a terminal you aren't looking at ends after running longer than {}: \
                 its tab gets a dot, and a notification while they're on. Needs shell integration \
                 (zsh, bash 4.4+). An agent goes by the switches above.",
                duration(s.long_command_seconds)
            ),
            |s| s.notify_long_command,
            |s, v| s.notify_long_command = v,
        ),
    ];
    if s.notify_long_command {
        when.push(segmented(
            "long-command",
            "Long means longer than",
            "",
            options(LONG_COMMAND_SECONDS, |n| duration(*n)),
            |s| s.long_command_seconds.to_string(),
            |s, v| s.long_command_seconds = v.parse().unwrap_or(s.long_command_seconds),
        ));
    }
    when.push(
        switch(
            "A git command ends",
            "A push, pull, fetch, clone or commit finishes or fails.",
            |s| s.notify_git,
            |s, v| s.notify_git = v,
        )
        .disabled_because(off, OFF),
    );
    Page::new().groups([
        Group::new().items(top),
        Group::new().title("Notify when").items(when),
    ])
}

/// 5 → "5 s", 300 → "5 min".
fn duration(seconds: u32) -> String {
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{} min", seconds / 60)
    }
}

pub fn updates(cx: &App) -> Page {
    let status = crate::updates::status_line(cx);
    let available = crate::updates::available(cx);
    let enabled = crate::updates::enabled(cx);
    Page::new().group(
        Group::new().items([
            switch(
                "Check for updates automatically",
                "At launch, every few hours and when this page opens, GitViber asks GitHub \
                 Releases whether a newer version is out. Nothing downloads until you choose to \
                 update.",
                |s| s.auto_update,
                |s, v| s.auto_update = v,
            ),
            Row::new(
                format!("GitViber {}", env!("CARGO_PKG_VERSION")),
                move |_, _, _| {
                    let button = if available {
                        Button::new("whats-new")
                            .small()
                            .outline()
                            .label("What's New…")
                            .on_click(|_, _, cx| crate::updates::show(cx))
                    } else {
                        Button::new("check-now")
                            .small()
                            .outline()
                            .label("Check Now")
                            .disabled(!enabled)
                            .on_click(|_, _, cx| crate::updates::check(true, cx))
                    };
                    button.into_any_element()
                },
            )
            .description(status),
        ]),
    )
}

/// Where Obsidian keeps its vault list, as shown when it has none.
const VAULT_LIST: &str = if IS_MAC {
    "~/Library/Application Support/obsidian/obsidian.json"
} else if cfg!(windows) {
    "%APPDATA%\\obsidian\\obsidian.json"
} else {
    "~/.config/obsidian/obsidian.json"
};

pub fn obsidian(w: &SettingsWindow, cx: &App) -> Page {
    let s = Settings::get(cx);
    let mut groups = vec![Group::new().item(switch(
        "Show vaults in the explorer",
        "A section beside the files lists your Obsidian vault. Its notes open in a tab, rendered \
         as Obsidian shows them.",
        |s| s.obsidian,
        |s, v| s.obsidian = v,
    ))];
    if s.obsidian {
        let items: Vec<Row> = match &w.data.vaults {
            Some(vaults) if !vaults.is_empty() => vaults
                .iter()
                .map(|v| {
                    let path = v.path.clone();
                    let shown = path.clone();
                    let name = SharedString::from(v.name.clone());
                    Row::new(name.clone(), move |off, _, cx| {
                        let path = path.clone();
                        toggle(
                            name.clone(),
                            !Settings::get(cx).hidden_vaults.contains(&shown),
                            off,
                            move |on, cx| {
                                let path = path.clone();
                                Settings::update(cx, move |s| {
                                    s.hidden_vaults.retain(|p| *p != path);
                                    if !on {
                                        s.hidden_vaults.push(path);
                                    }
                                })
                            },
                            cx,
                        )
                    })
                    .description(v.path.clone())
                })
                .collect(),
            found => {
                let text: SharedString = if found.is_some() {
                    format!(
                        "No vaults found. GitViber reads the ones Obsidian lists in {VAULT_LIST}."
                    )
                    .into()
                } else {
                    "Looking for vaults…".into()
                };
                vec![Row::custom(move |_, cx| note(text.clone(), cx))]
            }
        };
        groups.push(Group::new().title("Vaults").items(items));
    }
    Page::new().groups(groups)
}

fn minutes(n: u32) -> String {
    match n {
        0 => "Never".into(),
        n if n < 60 => format!("{n} min"),
        n => format!("{} h", n / 60),
    }
}

pub fn browser(w: &SettingsWindow, cx: &App) -> Page {
    if cfg!(target_os = "linux") {
        return Page::new().group(Group::new().item(Row::custom(|_, cx| {
            note(
                "The browser tab needs macOS for now. The settings here are for macOS, \
                     where each tab has a browser of its own.",
                cx,
            )
        })));
    }
    let s = Settings::get(cx);
    let mut agents = vec![switch(
        "Let agents use the browser",
        "An agent in a terminal can open pages, read them, click, type and take screenshots \
             with the gitviber browser command. A terminal pane's agents get tabs of their own, \
             opened in the background without taking your active tab. Off, the command says so \
             and does nothing, except that an agent can still open a page for you with --show.",
        |s| s.browser_agent_control,
        |s, v| s.browser_agent_control = v,
    )];
    if s.browser_agent_control {
        let mut hint = String::from(
            "A skill on when and how to use the browser, rewritten as GitViber updates and deleted \
             when this or agent control is turned off. A file there of your own is never touched.",
        );
        if let Some(skill) = &w.data.skill {
            for line in &skill.lines {
                hint.push('\n');
                hint.push_str(line);
            }
        }
        agents.push(switch(
            "Teach Claude Code and Codex (skill)",
            hint,
            |s| s.browser_agent_skill,
            |s, v| s.browser_agent_skill = v,
        ));
        let agent_note = w.data.skill.as_ref().map(|s| s.note.clone());
        if let Some(text) = agent_note {
            agents.push(
                Row::custom(move |_, cx| {
                    let text = text.clone();
                    let copy = text.clone();
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .w_full()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().flex_1().min_w_0().child(note(
                                    "Other agents learn of it from their instructions: add this \
                                     to the project's CLAUDE.md or AGENTS.md.",
                                    cx,
                                )))
                                .child(
                                    Button::new("copy-agent-note")
                                        .small()
                                        .outline()
                                        .label("Copy")
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(
                                                gpui_kit::ClipboardItem::new_string(copy.clone()),
                                            );
                                            crate::help::toast(
                                                gitviber_ui::workspace::Toast::success(
                                                    "Paste it into CLAUDE.md or AGENTS.md.",
                                                )
                                                .title("Copied"),
                                                cx,
                                            );
                                        }),
                                ),
                        )
                        .child(pre(text, cx))
                        .into_any_element()
                })
                .keywords(["claude.md", "agents.md"]),
            );
        }
    }
    agents.extend([
        segmented(
            "agent-drawn",
            "Agent tabs drawing at once",
            "Agents' tabs out of sight keep drawing while they're used, each like an open page: \
             more uses more memory and energy. Past this many, the one an agent used longest ago \
             sleeps until its next command.",
            options(BROWSER_AGENT_DRAWN, |n| n.to_string()),
            |s| s.browser_agent_drawn.to_string(),
            |s, v| s.browser_agent_drawn = v.parse().unwrap_or(s.browser_agent_drawn),
        ),
        switch(
            "Show agents' tabs",
            "The tabs agents open appear in the tab strip. Off, they stay out of it and keep \
             running as they were; one you took over stays, and turning this on brings the rest back.",
            |s| s.browser_show_agent_tabs,
            |s, v| s.browser_show_agent_tabs = v,
        ),
        switch(
            "Mute agents' tabs",
            "Agent tabs make no sound for you; the page still plays. Tabs you open or take over \
             are never muted.",
            |s| s.browser_mute_agent_tabs,
            |s, v| s.browser_mute_agent_tabs = v,
        ),
        switch(
            "Reload when an agent finishes",
            "When an agent in a terminal finishes its turn, the pages of its worktree load again. \
             A page its dev server already reloads by itself (Vite, webpack, Next) is left as it is.",
            |s| s.browser_reload_on_agent_done,
            |s, v| s.browser_reload_on_agent_done = v,
        ),
    ]);
    Page::new().groups([
        Group::new().title("Agents").items(agents),
        Group::new().title("Memory").items([
            segmented(
                "live-hidden",
                "Pages kept alive out of sight",
                "Each open page runs as a process of its own: about 20 MB for a plain one, 150 MB \
                 or more for a large app. Past this many, the one out of sight longest closes, and \
                 loads again from its address when its tab shows.",
                options(BROWSER_LIVE_HIDDEN, |n| n.to_string()),
                |s| s.browser_live_hidden.to_string(),
                |s, v| s.browser_live_hidden = v.parse().unwrap_or(s.browser_live_hidden),
            ),
            segmented(
                "park-after",
                "Close a page out of sight after",
                "However many are kept alive. Never leaves it to the count above.",
                options(BROWSER_PARK_AFTER, |n| minutes(*n)),
                |s| s.browser_park_after_min.to_string(),
                |s, v| s.browser_park_after_min = v.parse().unwrap_or(s.browser_park_after_min),
            ),
        ]),
        Group::new().title("Console").item(switch(
            "Keep the page's console",
            "A browser tab keeps what its page logs as errors and warnings, shows how many, and \
             sends the errors to the worktree's agent on request. Off, pages load without it.",
            |s| s.browser_console,
            |s, v| s.browser_console = v,
        )),
        Group::new().title("Data").item(action(
            "clear-browsing",
            "Browsing data",
            "The browser tabs keep their own cookies, site storage and cache, apart from \
             GitViber's settings. Chrome's sign-ins and cookies don't carry over: the tabs are \
             WebKit, as Safari is.",
            "Clear Browsing Data…",
            |window, cx| window.dispatch_action(Box::new(browser_cmds::ClearData), cx),
        )),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_as_the_old_app_wrote_them() {
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(100_000), "100,000");
        assert_eq!(thousands(10), "10");
        assert_eq!(duration(30), "30 s");
        assert_eq!(duration(300), "5 min");
        assert_eq!(minutes(0), "Never");
        assert_eq!(minutes(60), "1 h");
        assert_eq!(radix36(35), "z");
        assert_eq!(radix36(36), "10");
    }

    #[test]
    fn the_default_app_list_keeps_a_missing_default() {
        let s = Settings {
            open_in_app: "custom:gone".into(),
            ..Settings::default()
        };
        let apps = vec![gitviber_ui::open_in::OpenApp {
            id: "zed".into(),
            name: "Zed".into(),
            group: gitviber_ui::open_in::Group::Editor,
            command: None,
        }];
        let list = open_in_choices(&apps, &s);
        let ids: Vec<&str> = list.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["", "zed", "custom:gone"]);
    }

    #[test]
    fn a_permission_notice_shows_only_while_on_and_in_the_way() {
        assert_eq!(permission_notice(false, Some(Permission::Denied)), None);
        assert_eq!(permission_notice(true, Some(Permission::Granted)), None);
        assert_eq!(permission_notice(true, None), None);
        assert_eq!(
            permission_notice(true, Some(Permission::Prompt)).unwrap().1,
            Some("ask")
        );
    }
}
