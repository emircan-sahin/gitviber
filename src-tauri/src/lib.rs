mod commands;
mod definitions;
#[cfg(debug_assertions)]
mod dev_bridge;
mod diff;
mod display;
mod errors;
mod fs;
mod git;
mod github;
mod grep;
mod journal;
mod launch;
mod lfs;
mod lines;
mod menu;
mod navigation;
mod network;
mod open_in;
mod process;
mod pty;
mod rewrite;
#[cfg(test)]
mod scenario_tests;
mod shell;
mod state;
mod suggest;
mod titlebar;
#[cfg(target_os = "linux")]
mod trash;
mod watch;

use state::AppState;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    shell::resolve_in_background();
    let context = tauri::generate_context!();
    // Release builds load the bundled app; only debug builds are served from the dev server.
    let dev_url = if cfg!(debug_assertions) {
        context.config().build.dev_url.clone()
    } else {
        None
    };
    tauri::Builder::default()
        .plugin(navigation::guard(dev_url))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .menu(menu::build)
        .on_menu_event(|app, event| {
            let _ = app.emit("menu", event.id().as_ref());
        })
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                webview.state::<AppState>().ptys.kill_all();
            }
        })
        .manage(AppState::default())
        .setup(|app| {
            if let Ok(dir) = app.path().app_log_dir() {
                errors::init(dir);
            }
            #[cfg(target_os = "macos")]
            menu::keep_typed_key_equivalents();
            #[cfg(debug_assertions)]
            dev_bridge::start(app.handle().clone());
            if let Some(webview) = app.get_webview_window("main") {
                display::unlock_high_refresh_rate(&webview);
                titlebar::setup(&webview);
                // The page shows the window once its theme is applied (main.tsx); if it
                // never gets that far, a visible window beats an app with none.
                let w = webview.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::log_error,
            commands::app::show_logs,
            commands::repo::open_repo,
            commands::repo::git_info,
            commands::repo::install_git,
            commands::repo::git_identity,
            commands::repo::set_git_identity,
            commands::repo::repo_identity,
            commands::repo::remote_list,
            commands::repo::remote_edit,
            commands::repo::set_repo_identity,
            commands::changes::status,
            commands::changes::branch_review,
            commands::history::log,
            commands::history::log_compare,
            commands::history::compare_counts,
            commands::history::find_commit,
            commands::history::commit_files,
            commands::files::diff_pair,
            commands::files::media,
            commands::files::list_dir,
            commands::files::list_files,
            commands::files::search_files,
            commands::changes::change_lines,
            commands::stash::stash_branch,
            commands::history::rewrite,
            commands::github::ci_states,
            commands::history::compare_files,
            commands::history::reflog,
            commands::history::bisect_start,
            commands::history::bisect_mark,
            commands::sync::submodules,
            commands::sync::submodule_update,
            commands::sync::lfs_pull,
            commands::github::pr_review_comments,
            commands::github::gh_own_repos,
            commands::github::pr_comment_line,
            commands::files::definitions,
            commands::files::references,
            commands::files::cancel_search,
            commands::files::read_file,
            commands::files::tree_paths,
            commands::files::text_at,
            commands::history::blame,
            commands::branches::branches,
            commands::branches::switch_branch,
            commands::branches::delete_branches,
            commands::branches::delete_remote_branch,
            commands::branches::create_branch,
            commands::branches::rename_branch,
            commands::branches::set_upstream,
            commands::branches::tags,
            commands::worktrees::worktrees,
            commands::worktrees::worktree_state,
            commands::worktrees::add_worktree,
            commands::worktrees::rename_worktree,
            commands::worktrees::lock_worktree,
            commands::worktrees::unlock_worktree,
            commands::worktrees::remove_worktree,
            commands::changes::stage,
            commands::changes::unstage,
            commands::changes::discard,
            commands::changes::commit,
            commands::changes::commit_template,
            commands::changes::recent_authors,
            commands::changes::suggest_message,
            commands::changes::suggest_cancel,
            commands::history::commit_details,
            commands::sync::push,
            commands::sync::remote_was_ours,
            commands::sync::pull,
            commands::sync::fetch,
            commands::sync::last_fetch,
            commands::sync::cancel_network,
            commands::repo::clone_repo,
            commands::repo::init_repo,
            commands::sync::merge,
            commands::sync::rebase,
            commands::sync::op_continue,
            commands::sync::op_abort,
            commands::sync::rebase_skip,
            commands::changes::resolve_side,
            commands::files::write_file,
            commands::files::create_file,
            commands::files::create_dir,
            commands::files::rename_path,
            commands::files::trash_path,
            commands::files::reveal_path,
            commands::files::open_in_apps,
            commands::files::open_in,
            commands::files::open_in_custom,
            commands::repo::project_info,
            commands::repo::reveal_project,
            commands::history::undo_commit,
            commands::history::reset,
            commands::history::drops_pushed,
            commands::history::revert,
            commands::history::cherry_pick,
            commands::history::cherry_pick_into,
            commands::stash::stashes,
            commands::stash::stash_files,
            commands::stash::stash_push,
            commands::stash::stash_apply,
            commands::stash::stash_drop,
            commands::history::checkout_commit,
            commands::history::create_branch_at,
            commands::branches::create_tag,
            commands::branches::delete_tag,
            commands::branches::push_tags,
            commands::branches::delete_remote_tag,
            commands::branches::remote_tags,
            commands::journal::journal,
            commands::journal::journal_last,
            commands::journal::undo,
            commands::journal::redo,
            commands::github::github_web_url,
            commands::github::gh_account,
            commands::github::gh_protected_branches,
            commands::github::gh_original_remote,
            commands::github::gh_remotes,
            commands::github::pull_draft,
            commands::github::gh_sync_fork,
            commands::branches::set_push_default,
            commands::branches::switch_tracking,
            commands::github::gh_add_original_remote,
            commands::github::pr_list,
            commands::github::pr_detail,
            commands::github::pr_attachments,
            commands::github::pr_files,
            commands::github::pr_create,
            commands::github::pr_merge,
            commands::github::pr_set_open,
            commands::github::pr_review,
            commands::github::pr_checkout,
            commands::github::pr_checkout_worktree,
            commands::github::issue_list,
            commands::github::issue_counts,
            commands::github::issue_labels,
            commands::github::issue_detail,
            commands::github::issue_create,
            commands::github::issue_edit,
            commands::github::issue_set_open,
            commands::github::issue_set_labels,
            commands::github::issue_delete,
            commands::github::issue_comment,
            commands::app::open_url,
            commands::app::about,
            commands::app::set_menu,
            commands::app::pty_spawn,
            commands::app::pty_write,
            commands::app::pty_resize,
            commands::app::pty_kill
        ])
        .run(context)
        .expect("error while running GitViber");
}
