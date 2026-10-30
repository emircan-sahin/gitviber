//! A project row's right-click menu: open, pin, color, folder, remove.

use super::*;

/// A project row's right-click menu.
#[allow(clippy::too_many_arguments)]
pub(super) fn project_menu(
    menu: PopupMenu,
    path: &str,
    place: Place,
    current: bool,
    color: Option<&str>,
    folders: &[(u64, String)],
    picker: &WeakEntity<ProjectPicker>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let p = path.to_string();
    let (open, pin, forget) = (picker.clone(), p.clone(), p.clone());
    let color = color.map(str::to_string);
    let (cp, fp, fw) = (p.clone(), p.clone(), picker.clone());
    let folders = folders.to_vec();
    menu.item(
        PopupMenuItem::new("Open")
            .icon(IconName::FolderOpen)
            .disabled(current)
            .on_click({
                let p = p.clone();
                move |_, window, cx| {
                    let _ = open.update(cx, |this, cx| this.open(p.clone(), window, cx));
                }
            }),
    )
    .separator()
    .item(
        PopupMenuItem::new(if place == Place::Pinned {
            "Unpin"
        } else {
            "Pin"
        })
        .icon(if place == Place::Pinned {
            IconName::PinOff
        } else {
            IconName::Pin
        })
        .on_click(move |_, _, cx| arrange(cx, |o, _| o.toggle_pin(&pin))),
    )
    .submenu_with_icon(
        Some(Icon::new(IconName::Palette)),
        "Color",
        window,
        cx,
        move |mut sub, _, _| {
            let auto = cp.clone();
            sub = sub
                .item(
                    PopupMenuItem::new("Automatic")
                        .checked(color.is_none())
                        .on_click(move |_, _, cx| {
                            arrange(cx, |o, recent| {
                                o.set_color(&auto, None);
                                o.assign_colors(recent);
                            })
                        }),
                )
                .separator();
            for s in &SWATCHES {
                let p = cp.clone();
                sub = sub.item(
                    PopupMenuItem::element(move |_, cx| {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(swatch_tile(s, "", 14., cx))
                            .child(s.name)
                    })
                    .checked(color.as_deref() == Some(s.key))
                    .on_click(move |_, _, cx| arrange(cx, |o, _| o.set_color(&p, Some(s.key)))),
                );
            }
            sub
        },
    )
    .submenu_with_icon(
        Some(Icon::new(IconName::Folder)),
        "Move to Folder",
        window,
        cx,
        move |mut sub, _, _| {
            for (id, name) in &folders {
                let (id, p) = (*id, fp.clone());
                sub = sub.item(
                    PopupMenuItem::new(name.clone())
                        .checked(place == Place::Folder(id))
                        .on_click(move |_, _, cx| {
                            arrange(cx, |o, _| o.move_to(&p, Place::Folder(id)))
                        }),
                );
            }
            if !folders.is_empty() {
                sub = sub.separator();
            }
            let (p, w) = (fp.clone(), fw.clone());
            sub = sub.item(
                PopupMenuItem::new("New Folder…")
                    .icon(IconName::FolderPlus)
                    .on_click(move |_, window, cx| {
                        let _ =
                            w.update(cx, |this, cx| this.new_folder(Some(p.clone()), window, cx));
                    }),
            );
            if matches!(place, Place::Folder(_)) {
                let p = fp.clone();
                sub = sub.item(
                    PopupMenuItem::new("Remove from Folder")
                        .on_click(move |_, _, cx| arrange(cx, |o, _| o.move_to(&p, Place::Loose))),
                );
            }
            sub
        },
    )
    .separator()
    .item(
        PopupMenuItem::new("Remove from List")
            .icon(IconName::X)
            .disabled(current)
            .on_click(move |_, _, cx| UiState::update(cx, |s| s.forget_project(&forget))),
    )
}
