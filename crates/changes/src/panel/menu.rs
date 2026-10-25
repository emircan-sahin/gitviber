//! A changed file's right-click menu: the file, its list and its history; git's actions on the
//! rows picked; git's own diff or merge tool; revealing it, Open in, and copying it.

use super::*;
use crate::model::conflicts::{tool_can_open, tool_name};
use gitviber_ui::open_in::{self, OpenTarget};

/// A menu item's click: `run` on the panel, while it's there.
fn on_panel(
    weak: &WeakEntity<ChangesPanel>,
    run: impl Fn(&mut ChangesPanel, &mut Window, &mut Context<ChangesPanel>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let weak = weak.clone();
    move |_, window, cx| {
        weak.update(cx, |this, cx| run(this, window, cx)).ok();
    }
}

/// History narrowed to `path`, the History panel shown.
pub(super) fn show_history(
    ws: &WeakEntity<Workspace>,
    path: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(ws) = ws.upgrade() else { return };
    ws.update(cx, |ws, cx| ws.show_panel(gitviber_history::PANEL, cx));
    let panel = Workspace::ensure_panel(&ws, gitviber_history::PANEL, window, cx)
        .and_then(|p| p.view().downcast::<gitviber_history::HistoryPanel>().ok());
    if let Some(p) = panel {
        p.update(cx, |p, cx| p.show_history_of(path.to_string(), true, cx));
    }
}

/// The text copied, and the toast that says so.
fn copy(text: String, what: String, weak: &WeakEntity<ChangesPanel>, cx: &mut App) {
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
    weak.update(cx, |this, cx| this.toast(Toast::success(what), cx))
        .ok();
}

/// "Copy File", "Copy Image" (the pasteboard gets a picture of those), or "Copy N Files".
fn copy_label(paths: &[String]) -> String {
    const IMAGES: [&str; 11] = [
        "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "avif", "ico",
    ];
    match paths {
        [one] => {
            let ext = one.rsplit_once('.').map(|x| x.1.to_lowercase());
            let image = ext.is_some_and(|e| IMAGES.contains(&e.as_str()));
            if image { "Copy Image" } else { "Copy File" }.into()
        }
        _ => format!("Copy {} Files", paths.len()),
    }
}

impl ChangesPanel {
    /// `file`'s row menu; its git actions cover `rows`, the rows picked around it.
    pub(super) fn file_menu(
        &self,
        sel: Selection,
        kind: Kind,
        file: Arc<FileChange>,
        rows: Vec<Arc<FileChange>>,
        viewed: bool,
        cx: &mut Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let weak = cx.entity().downgrade();
        let ws = self.ctx.workspace.clone();
        let root = self.root.clone();
        let can_stash = self.operation.is_none();
        let is_review = matches!(sel, Selection::Branch { .. });
        let (merge_tool, diff_tool) = self
            .tools
            .as_ref()
            .map(|t| (t.merge.clone(), t.diff.clone()))
            .unwrap_or_default();
        let merging = self.in_tool.contains_key(&file.path);
        move |menu, window, cx| {
            let n = rows.len();
            let path = file.path.clone();
            let on_disk = file.status != "D";
            let conflict = kind == Kind::Conflict;
            let s = sel.clone();
            let mut menu = menu.item(
                PopupMenuItem::new(if conflict {
                    "Open Conflict"
                } else {
                    "Open Changes"
                })
                .on_click(on_panel(&weak, move |this, window, cx| {
                    this.open(s.clone(), false, window, cx)
                })),
            );
            let p = path.clone();
            menu = menu.item(PopupMenuItem::new("Open File").disabled(!on_disk).on_click(
                on_panel(&weak, move |this, window, cx| {
                    this.open(Selection::File { path: p.clone() }, false, window, cx)
                }),
            ));
            if is_review {
                return menu;
            }
            if !conflict {
                let staged = kind == Kind::Staged;
                menu = menu.item(
                    PopupMenuItem::new(if staged {
                        "Open All Staged Changes"
                    } else {
                        "Open All Changes"
                    })
                    .on_click(on_panel(&weak, move |this, window, cx| {
                        let of = if staged {
                            ChangesOf::Staged
                        } else {
                            ChangesOf::Unstaged
                        };
                        this.open_all(of, window, cx)
                    })),
                );
            }
            let (w, p) = (ws.clone(), path.clone());
            menu = menu.item(
                PopupMenuItem::new("Show History")
                    .disabled(file.status == "?")
                    .on_click(move |_, window, cx| show_history(&w, &p, window, cx)),
            );
            // One file at a time, as git opens it: mergetool merges text both sides changed,
            // difftool has nothing for an untracked file or a submodule.
            let one = n == 1 && tool_can_open(&path);
            let both_changed = matches!(file.conflict.as_deref(), Some("UU" | "AA"));
            if let Some(tool) = merge_tool
                .clone()
                .filter(|_| one && conflict && both_changed)
            {
                let p = path.clone();
                menu = menu.item(
                    PopupMenuItem::new(format!("Open in {}", tool_name(&tool)))
                        .disabled(merging)
                        .on_click(on_panel(&weak, move |this, _, cx| {
                            this.open_in_tool(p.clone(), tool.clone(), None, cx)
                        })),
                );
            }
            let diffable = !conflict && file.status != "?" && file.nested.is_none();
            if let Some(tool) = diff_tool.clone().filter(|_| one && diffable) {
                let (p, staged) = (path.clone(), kind == Kind::Staged);
                menu = menu.item(
                    PopupMenuItem::new(format!("Open in {}", tool_name(&tool))).on_click(on_panel(
                        &weak,
                        move |this, _, cx| {
                            this.open_in_tool(p.clone(), tool.clone(), Some(staged), cx)
                        },
                    )),
                );
            }
            menu = menu.separator();
            let many = move |one: &str, many: String| if n > 1 { many } else { one.to_string() };
            let r = rows.clone();
            match kind {
                Kind::Unstaged => {
                    menu = menu
                        .item(
                            PopupMenuItem::new(many("Stage Changes", format!("Stage {n} Files")))
                                .on_click(on_panel(&weak, move |this, _, cx| {
                                    this.stage(r.clone(), cx)
                                })),
                        )
                        .item({
                            let r = rows.clone();
                            PopupMenuItem::new(many(
                                "Discard Changes…",
                                format!("Discard {n} Files…"),
                            ))
                            .disabled(rows.iter().all(|f| kept_by_restore(f)))
                            .on_click(on_panel(&weak, move |this, window, cx| {
                                this.discard(r.clone(), window, cx)
                            }))
                        });
                    let untracked: Vec<Arc<FileChange>> =
                        rows.iter().filter(|f| f.status == "?").cloned().collect();
                    if !untracked.is_empty() {
                        let label = many(
                            "Add to .gitignore",
                            format!("Add {} to .gitignore", untracked.len()),
                        );
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .on_click(on_panel(&weak, move |this, _, cx| {
                                    this.ignore(untracked.clone(), cx)
                                })),
                        );
                    }
                }
                Kind::Staged => {
                    menu = menu.item(
                        PopupMenuItem::new(many("Unstage Changes", format!("Unstage {n} Files")))
                            .on_click(on_panel(&weak, move |this, _, cx| {
                                this.unstage_files(r.clone(), cx)
                            })),
                    );
                }
                Kind::Conflict => {
                    let (r2, r3) = (rows.clone(), rows.clone());
                    menu = menu
                        .item(
                            PopupMenuItem::new(many(
                                "Mark as Resolved",
                                format!("Mark {n} as Resolved"),
                            ))
                            .on_click(on_panel(&weak, move |this, window, cx| {
                                this.mark_resolved(r.clone(), window, cx)
                            })),
                        )
                        .item(
                            PopupMenuItem::new(many(
                                "Take Current Version",
                                format!("Take Current Version of {n} Files"),
                            ))
                            .on_click(on_panel(&weak, move |this, _, cx| {
                                this.resolve(r2.clone(), GitSide::Ours, cx)
                            })),
                        )
                        .item(
                            PopupMenuItem::new(many(
                                "Take Incoming Version",
                                format!("Take Incoming Version of {n} Files"),
                            ))
                            .on_click(on_panel(&weak, move |this, _, cx| {
                                this.resolve(r3.clone(), GitSide::Theirs, cx)
                            })),
                        );
                }
            }
            if !conflict && can_stash {
                let paths: Vec<String> = rows
                    .iter()
                    .filter(|f| f.nested.is_none())
                    .map(|f| f.path.clone())
                    .collect();
                menu = menu.item(
                    PopupMenuItem::new(many("Stash Changes…", format!("Stash {n} Files…")))
                        .on_click(on_panel(&weak, move |this, window, cx| {
                            this.stash_dialog(paths.clone(), window, cx)
                        })),
                );
            }
            if kind == Kind::Unstaged {
                let sels: Vec<Selection> = rows
                    .iter()
                    .map(|f| selection_of(Kind::Unstaged, f.clone()))
                    .collect();
                let label = format!(
                    "Mark {}as {}",
                    if n > 1 {
                        format!("{n} ")
                    } else {
                        String::new()
                    },
                    if viewed { "Not Viewed" } else { "Viewed" }
                );
                menu = menu.item(
                    PopupMenuItem::new(label).on_click(on_panel(&weak, move |this, _, cx| {
                        this.mark_viewed(sels.clone(), !viewed, cx)
                    })),
                );
            }
            let (w, p) = (ws.clone(), path.clone());
            menu = menu.separator().item(
                PopupMenuItem::new("Reveal in Explorer View")
                    .disabled(!on_disk)
                    .on_click(move |_, window, cx| {
                        gitviber_files::reveal_in_explorer(&w, &p, window, cx)
                    }),
            );
            let p = path.clone();
            menu = menu.item(
                PopupMenuItem::new(REVEAL_LABEL)
                    .disabled(!on_disk)
                    .on_click(move |_, _, cx| {
                        let (core, path) = (crate::core(cx), p.clone());
                        cx.background_spawn(async move { api::files::reveal_path(&core, path) })
                            .detach();
                    }),
            );
            if on_disk {
                let p = path.clone();
                menu = open_in::menu_items(menu, move |_| OpenTarget::file(p.clone()), window, cx);
            }
            menu = menu.separator();
            let abs = |p: &str| format!("{}/{p}", root.trim_end_matches('/'));
            // A deleted file has nothing on disk to copy, and a submodule is a folder.
            let on_disk_paths: Vec<String> = rows
                .iter()
                .filter(|f| f.status != "D" && f.nested.is_none())
                .map(|f| abs(&f.path))
                .collect();
            if cfg!(target_os = "macos") && !on_disk_paths.is_empty() {
                let (w, label) = (weak.clone(), copy_label(&on_disk_paths));
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                    let (core, paths, w) = (crate::core(cx), on_disk_paths.clone(), w.clone());
                    let n = paths.len();
                    cx.spawn(async move |cx| {
                        let copied = cx
                            .background_spawn(async move { api::app::copy_files(&core, paths) })
                            .await;
                        cx.update(|cx| {
                            let toast = match copied {
                                Ok(()) => Toast::success(if n == 1 {
                                    "File copied".to_string()
                                } else {
                                    format!("{n} files copied")
                                }),
                                Err(e) => Toast::error(e).title("Could not copy"),
                            };
                            w.update(cx, |this, cx| this.toast(toast, cx)).ok();
                        });
                    })
                    .detach();
                }));
            }
            let paths: Vec<String> = rows.iter().map(|f| f.path.clone()).collect();
            let (w, all) = (
                weak.clone(),
                paths.iter().map(|p| abs(p)).collect::<Vec<_>>(),
            );
            menu = menu.item(
                PopupMenuItem::new(many("Copy Path", "Copy Paths".into())).on_click(
                    move |_, _, cx| {
                        let what = many("Path copied", format!("{n} paths copied"));
                        copy(all.join("\n"), what, &w, cx)
                    },
                ),
            );
            let (w, rel) = (weak.clone(), paths.clone());
            menu = menu.item(
                PopupMenuItem::new(many("Copy Relative Path", "Copy Relative Paths".into()))
                    .on_click(move |_, _, cx| {
                        let what =
                            many("Relative path copied", format!("{n} relative paths copied"));
                        copy(rel.join("\n"), what, &w, cx)
                    }),
            );
            // A rename's old path too, so the patch has both sides of it.
            let patch_paths: Vec<String> = rows
                .iter()
                .filter(|f| f.nested.is_none())
                .flat_map(|f| f.old_path.iter().chain([&f.path]).cloned())
                .collect();
            if !conflict && !patch_paths.is_empty() {
                let w = weak.clone();
                let list = kind.name().to_string();
                menu = menu.item(
                    PopupMenuItem::new("Copy as Patch").on_click(move |_, _, cx| {
                        let (core, list, paths, w) = (
                            crate::core(cx),
                            list.clone(),
                            patch_paths.clone(),
                            w.clone(),
                        );
                        cx.spawn(async move |cx| {
                            let patch = cx
                                .background_spawn(async move {
                                    api::patch::changes_patch(&core, list, paths, None)
                                })
                                .await;
                            cx.update(|cx| match patch {
                                Ok(p) => copy(p, "Patch copied".into(), &w, cx),
                                Err(e) => {
                                    w.update(cx, |this, cx| {
                                        this.toast(
                                            Toast::error(e).title("Could not copy the patch"),
                                            cx,
                                        )
                                    })
                                    .ok();
                                }
                            });
                        })
                        .detach();
                    }),
                );
            }
            menu
        }
    }

    /// git's diff tool on `path` (`diff`: Some, on the staged side when true) or its merge tool
    /// (None). Both return once the tool is closed; the repo is read again then, a tool that
    /// edits or merges having changed the file.
    pub(super) fn open_in_tool(
        &mut self,
        path: String,
        tool: String,
        diff: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        if self.in_tool.contains_key(&path) {
            return;
        }
        let core = crate::core(cx);
        let p = path.clone();
        let task = cx.spawn(async move |this, cx| {
            let target = p.clone();
            let result = cx
                .background_spawn(async move {
                    match diff {
                        Some(staged) => api::changes::open_diff_tool(&core, target, staged),
                        None => api::changes::open_merge_tool(&core, target),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                let _done = this.in_tool.remove(&p);
                let name = tool_name(&tool);
                let toast = match (diff, result) {
                    (None, Ok(())) => {
                        Some(Toast::success(format!("{p}, in {name}")).title("Conflict resolved"))
                    }
                    (None, Err(e)) => {
                        Some(Toast::info(e).title(format!("{p} is still in conflict")))
                    }
                    (Some(_), Err(e)) => {
                        Some(Toast::error(e).title(format!("Could not open {name}")))
                    }
                    (Some(_), Ok(())) => None,
                };
                if let Some(t) = toast {
                    this.toast(t, cx);
                }
                this.refresh_project(cx);
            })
            .ok();
        });
        self.in_tool.insert(path, task);
    }
}

#[cfg(test)]
mod tests {
    use super::copy_label;

    #[test]
    fn copy_says_what_the_pasteboard_gets() {
        let one = |p: &str| copy_label(&[p.to_string()]);
        assert_eq!(one("/r/a.PNG"), "Copy Image");
        assert_eq!(one("/r/a.svg"), "Copy File");
        assert_eq!(one("/r/Makefile"), "Copy File");
        assert_eq!(copy_label(&["a".into(), "b".into()]), "Copy 2 Files");
    }
}
