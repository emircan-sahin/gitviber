//! The fields the pages are built of. Each reads and writes one Settings value, so a change made
//! elsewhere (a key, the status bar, a reset) shows here as it's made.

use std::rc::Rc;

use gitviber_ui::keymap::shortcut_label;
use gitviber_ui::{ActiveAppTheme, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::setting::{SettingField, SettingItem};
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Entity, Focusable as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, Subscription, Window, div, px, rems,
};

pub type Get<T> = fn(&Settings) -> T;
pub type Set<T> = fn(&mut Settings, T);
pub type SetStr = for<'a> fn(&mut Settings, &'a str);

/// `hint`, then the keys of `commands` as they're bound now.
pub fn with_keys(hint: &str, commands: &[&str], cx: &App) -> String {
    let keys: Vec<String> = commands
        .iter()
        .filter_map(|id| shortcut_label(id, cx))
        .collect();
    match (hint.is_empty(), keys.is_empty()) {
        (_, true) => hint.to_string(),
        (true, false) => keys.join("  "),
        (false, false) => format!("{hint}  {}", keys.join("  ")),
    }
}

pub fn switch(title: &str, hint: impl Into<String>, get: Get<bool>, set: Set<bool>) -> SettingItem {
    let default = get(&Settings::default());
    SettingItem::new(
        title.to_string(),
        SettingField::switch(
            move |cx| get(Settings::get(cx)),
            move |v, cx| Settings::update(cx, |s| set(s, v)),
        )
        .default_value(default),
    )
    .description(hint.into())
}

pub fn options<T>(values: &[T], label: impl Fn(&T) -> String) -> Vec<(String, String)>
where
    T: ToString,
{
    values.iter().map(|v| (v.to_string(), label(v))).collect()
}

/// A dropdown of `options` (value, label).
pub fn choice(
    title: &str,
    hint: impl Into<String>,
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
        title.to_string(),
        SettingField::scrollable_dropdown(
            options,
            move |cx| get(Settings::get(cx)).into(),
            move |v, cx| Settings::update(cx, |s| set(s, &v)),
        )
        .default_value(SharedString::from(default)),
    )
    .description(hint.into())
}

/// A row of buttons, one lit: the old app's segmented control, for a few short choices.
pub fn segmented(
    id: &'static str,
    title: &str,
    hint: impl Into<String>,
    options: Vec<(String, String)>,
    get: Get<String>,
    set: SetStr,
) -> SettingItem {
    let default = get(&Settings::default());
    SettingItem::new(
        title.to_string(),
        SettingField::render(move |opts, _, cx| {
            segments(
                id,
                &options,
                &get(Settings::get(cx)),
                opts.is_disabled(),
                move |v, _, cx| Settings::update(cx, |s| set(s, v)),
            )
        })
        .on_reset(
            move |cx| get(Settings::get(cx)) != default,
            move |_, cx| {
                let d = get(&Settings::default());
                Settings::update(cx, |s| set(s, &d))
            },
        ),
    )
    .description(hint.into())
}

/// The buttons of a segmented control; `pick` gets the value clicked.
pub fn segments(
    id: &'static str,
    options: &[(String, String)],
    current: &str,
    disabled: bool,
    pick: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let values: Vec<String> = options.iter().map(|(v, _)| v.clone()).collect();
    // The lit one in the accent: a selected outline button read as no different from the rest.
    ButtonGroup::new(id)
        .small()
        .disabled(disabled)
        .children(options.iter().enumerate().map(|(i, (v, label))| {
            let b = Button::new((id, i))
                .label(label.clone())
                .selected(v == current);
            if v == current { b.primary() } else { b }
        }))
        .on_click(move |clicked, window, cx| {
            if let Some(v) = clicked.first().and_then(|i| values.get(*i)) {
                pick(v, window, cx);
            }
        })
        .into_any_element()
}

struct TextState {
    input: Entity<InputState>,
    _sub: Subscription,
}

pub type GetText = Rc<dyn Fn(&Settings) -> String>;
pub type SetText = Rc<dyn Fn(&mut Settings, &str)>;

/// A text field kept as window state under `key`. `on_blur`: saved on Enter or leaving it, as a
/// font name is (each partial name on the way would lay the code out again); else as typed.
pub fn text(
    key: &'static str,
    title: &str,
    hint: impl Into<String>,
    placeholder: &'static str,
    on_blur: bool,
    get: Get<String>,
    set: SetStr,
) -> SettingItem {
    SettingItem::new(
        title.to_string(),
        SettingField::render(move |_, window, cx| {
            let field = TextField::new(key, Rc::new(get), Rc::new(set))
                .placeholder(placeholder)
                .on_blur(on_blur);
            field.render(window, cx)
        }),
    )
    .description(hint.into())
}

/// A text input bound to a setting, for a row of its own.
pub struct TextField {
    key: SharedString,
    placeholder: SharedString,
    on_blur: bool,
    width: gpui_kit::Pixels,
    get: GetText,
    set: SetText,
}

impl TextField {
    pub fn new(key: impl Into<SharedString>, get: GetText, set: SetText) -> Self {
        Self {
            key: key.into(),
            placeholder: SharedString::default(),
            on_blur: false,
            width: px(240.),
            get,
            set,
        }
    }

    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn on_blur(mut self, on_blur: bool) -> Self {
        self.on_blur = on_blur;
        self
    }

    pub fn width(mut self, width: gpui_kit::Pixels) -> Self {
        self.width = width;
        self
    }

    pub fn render(self, window: &mut Window, cx: &mut App) -> AnyElement {
        let TextField {
            key,
            placeholder,
            on_blur,
            width,
            get,
            set,
        } = self;
        let first = get.clone();
        let state = window.use_keyed_state(key, cx, move |window, cx| {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(first(Settings::get(cx)))
            });
            let sub = cx.subscribe(&input, move |_, input, e: &InputEvent, cx| {
                let commit = match e {
                    InputEvent::Change => !on_blur,
                    InputEvent::PressEnter { .. } | InputEvent::Blur => on_blur,
                    InputEvent::Focus => false,
                };
                if commit {
                    let v = input.read(cx).value().to_string();
                    let set = set.clone();
                    Settings::update(cx, move |s| set(s, &v));
                }
            });
            TextState { input, _sub: sub }
        });
        let input = state.read(cx).input.clone();
        // A change from elsewhere (a reset) shows once the field isn't being typed in.
        let current = get(Settings::get(cx));
        let focused = input.focus_handle(cx).is_focused(window);
        if !focused && input.read(cx).value().as_ref() != current {
            input.update(cx, |s, cx| s.set_value(current, window, cx));
        }
        div()
            .w(width)
            .child(Input::new(&input).small())
            .into_any_element()
    }
}

/// − size + and a reset, for a font size.
#[allow(clippy::too_many_arguments)]
pub fn stepper(
    id: &'static str,
    title: &str,
    hint: impl Into<String>,
    get: Get<f32>,
    set: Set<f32>,
    step: f32,
    min: f32,
    max: f32,
) -> SettingItem {
    let default = get(&Settings::default());
    SettingItem::new(
        title.to_string(),
        SettingField::render(move |_, _, cx| {
            let v = get(Settings::get(cx));
            let t = cx.app_theme();
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    Button::new((id, 0usize))
                        .small()
                        .label("−")
                        .disabled(v <= min)
                        .on_click(move |_, _, cx| Settings::update(cx, |s| set(s, v - step))),
                )
                .child(
                    div()
                        .w(px(40.))
                        .text_center()
                        .font_family(t.fonts.code.clone())
                        .text_size(rems(0.75))
                        .child(format!("{v}")),
                )
                .child(
                    Button::new((id, 1usize))
                        .small()
                        .label("+")
                        .disabled(v >= max)
                        .on_click(move |_, _, cx| Settings::update(cx, |s| set(s, v + step))),
                )
                .into_any_element()
        })
        .on_reset(
            move |cx| get(Settings::get(cx)) != default,
            move |_, cx| Settings::update(cx, |s| set(s, default)),
        ),
    )
    .description(hint.into())
}

/// A row with a button on its right.
pub fn action(
    id: &'static str,
    title: &str,
    hint: impl Into<String>,
    label: &'static str,
    run: impl Fn(&mut Window, &mut App) + 'static,
) -> SettingItem {
    let run = std::rc::Rc::new(run);
    SettingItem::new(
        title.to_string(),
        SettingField::render(move |_, _, _| {
            let run = run.clone();
            Button::new(id)
                .small()
                .outline()
                .label(label)
                .on_click(move |_, window, cx| run(window, cx))
                .into_any_element()
        }),
    )
    .description(hint.into())
}

/// Text under a group's rows: what a command gets, where something lives.
pub fn note(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    div()
        .text_size(rems(0.71875))
        .line_height(rems(1.1))
        .text_color(t.muted_foreground)
        .child(text.into())
        .into_any_element()
}

/// A block of fixed text, as a prompt is sent.
pub fn pre(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    div()
        .w_full()
        .p_2()
        .rounded(px(6.))
        .border_1()
        .border_color(t.border)
        .bg(t.code(t.background))
        .font_family(t.fonts.code.clone())
        .text_size(rems(0.6875))
        .line_height(rems(1.05))
        .text_color(t.muted_foreground)
        .child(text.into())
        .into_any_element()
}

/// A link-styled button that opens `url` in the browser.
pub fn link(id: &'static str, label: &'static str, url: &'static str) -> Button {
    Button::new(id)
        .link()
        .small()
        .label(label)
        .icon(IconName::ExternalLink)
        .on_click(move |_, _, cx| crate::help::open_url(url, cx))
}
