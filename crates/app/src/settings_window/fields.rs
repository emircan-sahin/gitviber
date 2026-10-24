//! The fields the pages are built of. Each reads and writes one Settings value, so a change made
//! elsewhere (a key, the status bar, a reset) shows here as it's made.

use std::rc::Rc;

use gitviber_ui::{ActiveAppTheme, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _};
use gpui_kit::{
    Anchor, AnyElement, App, AppContext as _, Entity, Focusable as _, IntoElement,
    ParentElement as _, SharedString, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px, rems,
};

pub use super::shell::{Hint, Row};

pub type Get<T> = fn(&Settings) -> T;
pub type Set<T> = fn(&mut Settings, T);
pub type SetStr = for<'a> fn(&mut Settings, &'a str);

/// `hint`, with the keys of `commands` as they're bound now beside the row's title.
pub fn with_keys(hint: &str, commands: &'static [&'static str]) -> Hint {
    Hint {
        text: SharedString::from(hint.to_string()),
        keys: commands,
    }
}

/// A switch. Disabled, it's drawn as an outline: gpui-component's fades the track by half,
/// which left an off switch nearly invisible on a light theme.
pub fn toggle(
    title: SharedString,
    checked: bool,
    disabled: bool,
    on: impl Fn(bool, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    if !disabled {
        return Switch::new("switch")
            .small()
            .accessibility_label(title)
            .checked(checked)
            .on_click(move |v: &bool, _, cx: &mut App| on(*v, cx))
            .into_any_element();
    }
    let t = cx.app_theme();
    div()
        .w(px(28.))
        .h(px(16.))
        .p(px(1.))
        .flex()
        .items_center()
        .when(checked, |d| d.justify_end())
        .rounded_full()
        .border_1()
        .border_color(t.border_strong)
        .bg(if checked {
            t.primary.opacity(0.35)
        } else {
            gpui_kit::transparent_black()
        })
        .child(div().size(px(10.)).rounded_full().bg(if checked {
            t.primary_foreground
        } else {
            t.subtle
        }))
        .into_any_element()
}

pub fn switch(title: &str, hint: impl Into<Hint>, get: Get<bool>, set: Set<bool>) -> Row {
    let default = get(&Settings::default());
    let name = SharedString::from(title.to_string());
    Row::new(name.clone(), move |off, _, cx| {
        toggle(
            name.clone(),
            get(Settings::get(cx)),
            off,
            move |v, cx| Settings::update(cx, |s| set(s, v)),
            cx,
        )
    })
    .description(hint)
    .on_reset(
        move |cx| get(Settings::get(cx)) != default,
        move |cx| Settings::update(cx, |s| set(s, default)),
    )
}

pub fn options<T>(values: &[T], label: impl Fn(&T) -> String) -> Vec<(String, String)>
where
    T: ToString,
{
    values.iter().map(|v| (v.to_string(), label(v))).collect()
}

/// A dropdown's (value, label) pairs, and what takes the value picked.
type Choices = Rc<Vec<(SharedString, SharedString)>>;
type Picked = Rc<dyn Fn(SharedString, &mut App)>;

/// A button showing `current`'s label that drops down `options`.
fn dropdown(
    title: SharedString,
    options: Choices,
    current: SharedString,
    disabled: bool,
    pick: Picked,
) -> AnyElement {
    let label = options
        .iter()
        .find(|(v, _)| *v == current)
        .map_or_else(|| current.clone(), |(_, l)| l.clone());
    Button::new("dropdown")
        .small()
        .outline()
        .accessibility_label(format!("{title}: {label}"))
        .label(label)
        .dropdown_caret(true)
        .disabled(disabled)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            let menu = options.iter().fold(menu, |menu, (value, label)| {
                let (value, pick) = (value.clone(), pick.clone());
                menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(value == current)
                        .on_click(move |_, _, cx| pick(value.clone(), cx)),
                )
            });
            menu.scrollable(true)
        })
        .into_any_element()
}

/// A dropdown row of its own: `get` the value shown, `set` the one picked.
pub fn pick(
    title: impl Into<SharedString>,
    options: Vec<(SharedString, SharedString)>,
    get: impl Fn(&App) -> SharedString + 'static,
    set: impl Fn(SharedString, &mut App) + 'static,
) -> Row {
    let title = title.into();
    let options = Rc::new(options);
    let set: Picked = Rc::new(set);
    Row::new(title.clone(), move |off, _, cx| {
        dropdown(title.clone(), options.clone(), get(cx), off, set.clone())
    })
}

/// A dropdown of `options` (value, label).
pub fn choice(
    title: &str,
    hint: impl Into<Hint>,
    options: Vec<(String, String)>,
    get: Get<String>,
    set: SetStr,
) -> Row {
    let default = get(&Settings::default());
    let reset = default.clone();
    let options = options
        .into_iter()
        .map(|(v, l)| (SharedString::from(v), SharedString::from(l)))
        .collect();
    pick(
        title.to_string(),
        options,
        move |cx| get(Settings::get(cx)).into(),
        move |v, cx| Settings::update(cx, |s| set(s, &v)),
    )
    .description(hint)
    .on_reset(
        move |cx| get(Settings::get(cx)) != default,
        move |cx| Settings::update(cx, |s| set(s, &reset)),
    )
}

/// A row of buttons, one lit: the old app's segmented control, for a few short choices.
pub fn segmented(
    id: &'static str,
    title: &str,
    hint: impl Into<Hint>,
    options: Vec<(String, String)>,
    get: Get<String>,
    set: SetStr,
) -> Row {
    let default = get(&Settings::default());
    Row::new(title.to_string(), move |off, _, cx| {
        segments(
            id,
            &options,
            &get(Settings::get(cx)),
            off,
            move |v, _, cx| Settings::update(cx, |s| set(s, v)),
        )
    })
    .description(hint)
    .on_reset(
        move |cx| get(Settings::get(cx)) != default,
        move |cx| {
            let d = get(&Settings::default());
            Settings::update(cx, |s| set(s, &d))
        },
    )
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
    hint: impl Into<Hint>,
    placeholder: &'static str,
    on_blur: bool,
    get: Get<String>,
    set: SetStr,
) -> Row {
    Row::new(title.to_string(), move |_, window, cx| {
        TextField::new(key, Rc::new(get), Rc::new(set))
            .placeholder(placeholder)
            .on_blur(on_blur)
            .render(window, cx)
    })
    .description(hint)
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
    hint: impl Into<Hint>,
    get: Get<f32>,
    set: Set<f32>,
    step: f32,
    min: f32,
    max: f32,
) -> Row {
    let default = get(&Settings::default());
    Row::new(title.to_string(), move |_, _, cx| {
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
    .description(hint)
    .on_reset(
        move |cx| get(Settings::get(cx)) != default,
        move |cx| Settings::update(cx, |s| set(s, default)),
    )
}

/// A row with a button on its right.
pub fn action(
    id: &'static str,
    title: &str,
    hint: impl Into<Hint>,
    label: &'static str,
    run: impl Fn(&mut Window, &mut App) + 'static,
) -> Row {
    let run = Rc::new(run);
    Row::new(title.to_string(), move |_, _, _| {
        let run = run.clone();
        Button::new(id)
            .small()
            .outline()
            .label(label)
            .on_click(move |_, window, cx| run(window, cx))
            .into_any_element()
    })
    .description(hint)
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
