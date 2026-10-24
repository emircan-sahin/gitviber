//! Opening what History shows in the center, as selections the workspace keeps and restores:
//! a commit's file ("commit"), all of a commit's or a range's files ("changes"), and the Compare
//! screen ("compare"). The openers for those kinds are here; a range's or the working tree's
//! single file belongs to other features, with a plain diff tab while none is registered.

use std::rc::Rc;
use std::sync::Arc;

use gitviber_core::{api, git};
use gitviber_diff::{Compare, DiffSource};
use gitviber_refs::RepoState;
use gitviber_refs::points::{ComparePoint, default_points};
use gitviber_refs::toast::Note;
use gitviber_ui::Workspace;
use gitviber_ui::workspace::{
    ChangesOf, ComparePoint as UiPoint, PullRange, Selection, TabHandle, register_opener,
};
use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, Entity, SharedString, WeakEntity, Window,
};

use crate::commit::Commit;
use crate::commit_view::{CommitView, Of};
use crate::compare::CompareView;
use crate::diff_tab::DiffTab;
use crate::list::CommitList;

fn url_of(list: &CommitList, c: &Commit, cx: &App) -> Option<String> {
    crate::act::commit_url(list, c, cx)
}

fn open(
    workspace: &WeakEntity<Workspace>,
    sel: Selection,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    workspace
        .update(cx, |ws, cx| ws.open(sel, preview, window, cx))
        .unwrap_or(false)
}

/// One file of a commit. `preview`: a single click's tab, which the next preview open replaces.
pub fn open_commit_file(
    list: &CommitList,
    c: &Commit,
    file: &git::FileChange,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let sel = Selection::Commit {
        commit: Arc::new(c.to_core()),
        file: Arc::new(file.clone()),
        url: url_of(list, c, cx),
    };
    open(&list.workspace, sel, preview, window, cx);
}

/// Every file of a commit, one at a time beside its list.
pub fn open_commit(list: &CommitList, c: &Commit, window: &mut Window, cx: &mut App) {
    let sel = Selection::Changes {
        of: ChangesOf::Commit {
            commit: Arc::new(c.to_core()),
            url: url_of(list, c, cx),
        },
    };
    open(&list.workspace, sel, false, window, cx);
}

/// Every file between two commits; `label` names the comparison.
pub fn open_range(
    workspace: &WeakEntity<Workspace>,
    base: String,
    head: String,
    label: String,
    window: &mut Window,
    cx: &mut App,
) {
    let range = PullRange {
        number: None,
        pull_url: None,
        label: Some(label),
        base,
        head,
    };
    open(
        workspace,
        Selection::Changes {
            of: ChangesOf::Range { range },
        },
        false,
        window,
        cx,
    );
}

/// Two commits a comparison's files are diffed between, and its name.
pub struct Range {
    pub base: String,
    pub head: String,
    pub label: String,
}

/// A range's file, as a pull request's is opened.
pub fn open_range_file(
    workspace: &WeakEntity<Workspace>,
    range: Range,
    file: &git::FileChange,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Range { base, head, label } = range;
    let range = PullRange {
        number: None,
        pull_url: None,
        label: Some(label.clone()),
        base: base.clone(),
        head: head.clone(),
    };
    if !open(
        workspace,
        Selection::PrFile {
            range,
            file: Arc::new(file.clone()),
        },
        preview,
        window,
        cx,
    ) {
        open_diff(
            workspace,
            DiffSource::from_change(Compare::Range { base, head }, file),
            label.into(),
            preview,
            window,
            cx,
        );
    }
}

/// The working tree's file against a commit: History's Compare with Working Tree.
pub fn open_worktree_file(
    workspace: &WeakEntity<Workspace>,
    base: String,
    label: String,
    file: &git::FileChange,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let sel = Selection::Branch {
        base: base.clone(),
        label: label.clone(),
        file: Arc::new(file.clone()),
        fixed: true,
    };
    if !open(workspace, sel, preview, window, cx) {
        let text: SharedString = format!("{label} → working tree").into();
        open_diff(
            workspace,
            DiffSource::from_change(Compare::Base { base }, file),
            text,
            preview,
            window,
            cx,
        );
    }
}

/// One file's diff in a tab of its own, not saved with the worktree's tabs.
fn open_diff(
    workspace: &WeakEntity<Workspace>,
    source: DiffSource,
    label: SharedString,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(ws) = workspace.upgrade() else {
        return;
    };
    let key = format!("diff:{}", source.key().replace('\0', ":"));
    ws.update(cx, |ws, cx| {
        ws.open_tab(
            key,
            preview,
            move |_, cx| cx.new(|cx| DiffTab::new(source, label, cx)),
            window,
            cx,
        );
    });
}

fn to_ui(p: &ComparePoint) -> UiPoint {
    UiPoint {
        reference: p.r#ref.clone(),
        label: p.label.clone(),
    }
}

pub fn from_ui(p: &UiPoint) -> ComparePoint {
    ComparePoint {
        r#ref: p.reference.clone(),
        label: p.label.clone(),
    }
}

/// The Compare screen, one tab: opened again it shows the sides asked for; a side left out is
/// what it showed, or the default (origin's default branch against the branch checked out).
pub fn open_compare(
    workspace: &WeakEntity<Workspace>,
    repo: &Entity<RepoState>,
    base: Option<ComparePoint>,
    head: Option<ComparePoint>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(ws) = workspace.upgrade() else {
        return;
    };
    let (b, h) = {
        let r = repo.read(cx);
        default_points(&r.branches, r.branch())
    };
    let open_now = ws
        .read(cx)
        .tabs()
        .get("compare::Compare")
        .and_then(|t| t.item.handle()?.view().downcast::<CompareView>().ok());
    if let Some(view) = open_now {
        view.update(cx, |v, cx| v.set_points(base, head, cx));
        let sel = view.read(cx).selection();
        ws.update(cx, |ws, cx| ws.open(sel, false, window, cx));
        return;
    }
    let (base, head) = (base.unwrap_or(b), head.unwrap_or(h));
    let sel = Selection::Compare {
        base: to_ui(&base),
        head: to_ui(&head),
        merge_base: true,
    };
    ws.update(cx, |ws, cx| ws.open(sel, false, window, cx));
}

/// A file of a commit: restore it as the commit has it, revert the commit's change to it, or
/// copy that change as a patch.
pub fn file_action(
    list: &mut CommitList,
    c: &Rc<Commit>,
    f: &git::FileChange,
    name: &str,
    verb: &str,
    window: &mut Window,
    cx: &mut Context<CommitList>,
) {
    let core = gitviber_ui::core(cx);
    let (sha, path, old) = (c.sha.clone(), f.path.clone(), f.old_path.clone());
    let short = c.short.to_string();
    match verb {
        "patch" => {
            let paths: Vec<String> = old.into_iter().chain([path]).collect();
            let read = cx.background_spawn(async move {
                api::patch::changes_patch(&core, "commit".into(), paths, Some(sha))
            });
            list.flow = Some(cx.spawn_in(window, async move |_, cx| {
                let patch = read.await;
                let _ = cx.update(|window, cx| match patch {
                    Ok(p) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(p));
                        Note::success("Patch copied").show(window, cx);
                    }
                    Err(e) => Note::error("Could not copy the patch")
                        .detail(e)
                        .show(window, cx),
                });
            }));
        }
        "restore" => {
            let a = gitviber_refs::GitAction::new("Restore")
                .done(format!("Restored {name} from {short}"))
                .detail("The version it replaced is in the Trash.");
            drop(crate::act::run(
                list,
                a,
                move |core, _| {
                    gitviber_refs::repo::plain(api::patch::restore_file(core, sha, path))
                },
                window,
                cx,
            ));
        }
        _ => {
            let a =
                gitviber_refs::GitAction::new("Revert").done(format!("Reverted {short} in {name}"));
            // Conflicts it left are marked in the file: told as a stop on conflicts.
            let work = move |core: &gitviber_core::Core, _: &gitviber_refs::net::NetOp| {
                api::patch::revert_file(core, sha, path, old).map(|c| !c.is_empty())
            };
            drop(crate::act::run(list, a, work, window, cx));
        }
    }
}

/// History's openers: a commit's file, a commit's or a range's whole list, and Compare.
pub fn register(cx: &mut App) {
    register_opener(cx, "commit", |sel, _, cx| {
        let Selection::Commit { commit, file, url } = sel else {
            return None;
        };
        let of = Of::File {
            commit: Rc::new(Commit::from_core(commit)),
            file: (**file).clone(),
        };
        let url = url.clone();
        Some(TabHandle::new(cx.new(|cx| CommitView::new(of, url, cx))))
    });
    register_opener(cx, "changes-commit", |sel, _, cx| {
        let Selection::Changes {
            of: ChangesOf::Commit { commit, url },
        } = sel
        else {
            return None;
        };
        let of = Of::Commit(Rc::new(Commit::from_core(commit)));
        let url = url.clone();
        Some(TabHandle::new(cx.new(|cx| CommitView::new(of, url, cx))))
    });
    register_opener(cx, "changes-range", |sel, _, cx| {
        let Selection::Changes {
            of: ChangesOf::Range { range },
        } = sel
        else {
            return None;
        };
        let of = Of::Range {
            base: range.base.clone(),
            head: range.head.clone(),
            label: range.label(),
        };
        Some(TabHandle::new(cx.new(|cx| CommitView::new(of, None, cx))))
    });
    register_opener(cx, "compare", |sel, window, cx| {
        let Selection::Compare {
            base,
            head,
            merge_base,
        } = sel
        else {
            return None;
        };
        let (base, head, mb) = (from_ui(base), from_ui(head), *merge_base);
        Some(TabHandle::new(
            cx.new(|cx| CompareView::new(base, head, mb, window, cx)),
        ))
    });
}
