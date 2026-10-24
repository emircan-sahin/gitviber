//! Settings → Keyboard Shortcuts: every command by category, its keys as keycaps. A click on a
//! key records a new one in its place, + adds one; Esc cancels. Rebinds are kept in settings'
//! `keybindings` (command id → chords, an empty list unbinds), only while they differ from the
//! default, as the old app kept them.

use gitviber_ui::keymap::edit::{clash_note, recorded, with_binding};
use gitviber_ui::keymap::{self, IS_MAC, bindings_for, chord_keys, format_chord};
use gitviber_ui::{ActiveAppTheme, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::setting::{SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, div, prelude::FluentBuilder as _, px,
    rems,
};

use super::fields::{note, switch, with_keys};
use super::{Recording, SettingsWindow};

/// The categories in the order they first appear, which puts General first.
pub fn categories() -> Vec<keymap::Category> {
    let mut out: Vec<keymap::Category> = Vec::new();
    for c in keymap::all_commands() {
        if !out.contains(&c.category) {
            out.push(c.category);
        }
    }
    out
}

fn keycaps(chord: &str, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    div()
        .flex()
        .gap(px(2.))
        .children(chord_keys(chord, IS_MAC).into_iter().map(|k| {
            div()
                .min_w(px(18.))
                .px(px(4.))
                .rounded(px(3.))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.elevated)
                .text_color(t.foreground)
                .text_size(rems(0.6875))
                .line_height(rems(1.))
                .text_center()
                .child(SharedString::from(k))
        }))
        .into_any_element()
}

/// One key of a command (or the + after them): a click records over it.
fn recorder(
    weak: &WeakEntity<SettingsWindow>,
    id: &'static str,
    index: usize,
    label: Option<&str>,
    recording: Option<&Recording>,
    cx: &App,
) -> AnyElement {
    let t = cx.app_theme();
    let active = recording.is_some_and(|r| r.id == id && r.index == index);
    let tip = if label.is_some() {
        "Change this key"
    } else {
        "Add a key"
    };
    let weak = weak.clone();
    let body = if active {
        let (text, color) = match recording.and_then(|r| r.refused.clone()) {
            Some(why) => (why, t.destructive),
            None => ("Press keys…".to_string(), t.primary),
        };
        div()
            .px_1()
            .text_size(rems(0.71875))
            .text_color(color)
            .child(text)
            .into_any_element()
    } else {
        match label {
            Some(chord) => keycaps(chord, cx),
            None => Icon::new(IconName::Plus)
                .xsmall()
                .text_color(t.subtle)
                .into_any_element(),
        }
    };
    div()
        .id(SharedString::from(format!("key-{id}-{index}")))
        .flex()
        .items_center()
        .justify_center()
        .h(px(26.))
        .min_w(px(26.))
        .px_1()
        .rounded(px(5.))
        .cursor_pointer()
        .when(active, |d| d.border_1().border_color(t.primary))
        .when(!active, |d| d.hover(|s| s.bg(t.active)))
        .child(body)
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |w, cx| {
                if active {
                    w.stop_recording(cx);
                } else {
                    w.start_recording(id, index, window, cx);
                }
            });
        })
        .into_any_element()
}

fn row(c: &'static keymap::Command, w: &WeakEntity<SettingsWindow>, cx: &App) -> AnyElement {
    let t = cx.app_theme().clone();
    let overrides = &Settings::get(cx).keybindings;
    let keys = bindings_for(c, overrides, IS_MAC);
    let recording = w.upgrade().and_then(|w| w.read(cx).recording.clone());
    let clashes: Vec<String> = keys
        .iter()
        .filter_map(|k| clash_note(c, k, overrides, IS_MAC))
        .collect();
    let overridden = overrides.contains_key(c.id);
    let mut caps: Vec<AnyElement> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| recorder(w, c.id, i, Some(k), recording.as_ref(), cx))
        .collect();
    let adding = recording
        .as_ref()
        .is_some_and(|r| r.id == c.id && r.index == keys.len());
    caps.push(if keys.is_empty() && !adding {
        let weak = w.clone();
        let index = keys.len();
        div()
            .id(SharedString::from(format!("unbound-{}", c.id)))
            .px_1()
            .h(px(26.))
            .flex()
            .items_center()
            .rounded(px(5.))
            .cursor_pointer()
            .hover(|s| s.bg(t.active))
            .text_size(rems(0.71875))
            .text_color(t.subtle)
            .child("Unbound")
            .on_click(move |_, window, cx| {
                let _ = weak.update(cx, |w, cx| w.start_recording(c.id, index, window, cx));
            })
            .into_any_element()
    } else {
        recorder(w, c.id, keys.len(), None, recording.as_ref(), cx)
    });
    let (reset, clear) = (w.clone(), w.clone());
    let note_text = clashes.join(". ");
    div()
        .flex()
        .items_center()
        .gap_3()
        .w_full()
        .min_h(px(34.))
        .child(div().flex_1().min_w_0().truncate().child(c.title))
        .when(!clashes.is_empty(), |d| {
            d.child(
                div()
                    .id(SharedString::from(format!("clash-{}", c.id)))
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .small()
                            .text_color(t.modified),
                    )
                    .tooltip(move |window, cx| Tooltip::new(note_text.clone()).build(window, cx)),
            )
        })
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .children(caps),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .justify_end()
                .gap_0p5()
                .w(px(56.))
                .when(overridden, |d| {
                    d.child(
                        Button::new(SharedString::from(format!("reset-{}", c.id)))
                            .ghost()
                            .xsmall()
                            .icon(IconName::RotateCcw)
                            .tooltip("Reset to default")
                            .on_click(move |_, _, cx| {
                                let _ = reset.update(cx, |_, cx| set_keys(c.id, None, cx));
                            }),
                    )
                })
                .when(!keys.is_empty(), |d| {
                    d.child(
                        Button::new(SharedString::from(format!("unbind-{}", c.id)))
                            .ghost()
                            .xsmall()
                            .icon(IconName::X)
                            .tooltip("Remove all keys")
                            .on_click(move |_, _, cx| {
                                let _ = clear.update(cx, |_, cx| set_keys(c.id, Some(vec![]), cx));
                            }),
                    )
                }),
        )
        .into_any_element()
}

/// Binds command `id` to `keys`; None goes back to its defaults.
pub fn set_keys(id: &str, keys: Option<Vec<String>>, cx: &mut App) {
    let next = with_binding(&Settings::get(cx).keybindings, id, keys, IS_MAC);
    Settings::update(cx, |s| s.keybindings = next);
}

/// A recorded chord, put in place of key `index` of command `id` (or after its last).
pub fn record(id: &str, index: usize, chord: &str, cx: &mut App) {
    let Some(c) = keymap::command(id) else { return };
    let keys = bindings_for(c, &Settings::get(cx).keybindings, IS_MAC);
    set_keys(id, Some(recorded(&keys, index, chord)), cx);
}

/// Why a chord can't be recorded: macOS takes it before the app sees it.
pub fn refusal(chord: &str) -> Option<String> {
    keymap::chord::is_reserved(chord, IS_MAC)
        .then(|| format!("{} is reserved by macOS", format_chord(chord, IS_MAC)))
}

pub fn page(w: &SettingsWindow, cx: &App) -> SettingPage {
    let hold = if IS_MAC { "⌘" } else { "Ctrl" };
    let weak = w.weak.clone();
    let any_rebound = !Settings::get(cx).keybindings.is_empty();
    let typing = if IS_MAC {
        "⌘, ⌃ or an F-key apply (not ⌘-arrows or ⌃ with a letter, which edit text)"
    } else {
        "Ctrl or an F-key apply (not Ctrl+arrows, which move by word)"
    };
    let mut groups = vec![SettingGroup::new().items([
        switch(
            &format!("Hold {hold} to show shortcuts"),
            with_keys(
                "Hold it by itself for a second to see the shortcuts that work where you are. The \
                 key below shows them too.",
                &["workbench.shortcutOverlay"],
                cx,
            ),
            |s| s.shortcut_overlay,
            |s, v| s.shortcut_overlay = v,
        ),
        SettingItem::render(move |_, _, cx| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .w_full()
                .child(div().flex_1().min_w_0().child(note(
                    format!(
                        "Click a key to change it, or + to add one; Esc cancels. When two \
                         commands share a key, the warning next to them names the one that runs. \
                         While you type in a text field, only shortcuts with {typing}, and Commit \
                         only applies in the commit message."
                    ),
                    cx,
                )))
                .child(
                    Button::new("shortcuts-reset-all")
                        .small()
                        .outline()
                        .icon(IconName::RotateCcw)
                        .label("Reset all")
                        .disabled(!any_rebound)
                        .on_click(|_, _, cx| Settings::update(cx, |s| s.keybindings.clear())),
                )
                .into_any_element()
        })
        .keywords(["reset all"]),
    ])];
    for category in categories() {
        let items: Vec<SettingItem> = keymap::all_commands()
            .filter(|c| c.category == category)
            .map(|c| {
                let weak = weak.clone();
                let overrides = &Settings::get(cx).keybindings;
                let keys = bindings_for(c, overrides, IS_MAC);
                let mut words: Vec<SharedString> =
                    vec![c.title.into(), c.id.into(), category.label().into()];
                words.extend(keys.iter().map(|k| SharedString::from(k.clone())));
                words.extend(
                    keys.iter()
                        .map(|k| SharedString::from(format_chord(k, IS_MAC))),
                );
                SettingItem::render(move |_, _, cx| row(c, &weak, cx)).keywords(words)
            })
            .collect();
        groups.push(SettingGroup::new().title(category.label()).items(items));
    }
    SettingPage::new("Keyboard Shortcuts")
        .icon(IconName::Keyboard)
        .resettable(false)
        .groups(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_start_with_general_and_list_each_once() {
        let list = categories();
        assert_eq!(list[0], keymap::Category::General);
        let mut sorted = list.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), list.len());
    }

    #[test]
    fn macos_keeps_its_own_chords() {
        if IS_MAC {
            assert_eq!(refusal("cmd+q").as_deref(), Some("⌘Q is reserved by macOS"));
        }
        assert_eq!(refusal("cmd+u"), None);
    }
}
