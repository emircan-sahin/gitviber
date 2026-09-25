use crate::journal::{Action, Mode};
use crate::state::{in_repo, journaled, short, AppState, Res};
use crate::{fs, git, rewrite};
use tauri::State;

#[tauri::command]
pub async fn log(
    state: State<'_, AppState>,
    rev: Option<String>,
    skip: u32,
    limit: u32,
    filter: Option<git::LogFilter>,
    all: Option<git::GraphRefs>,
) -> Res<Vec<git::Commit>> {
    let filter = filter.unwrap_or_default();
    in_repo(&state, move |r| {
        // Only pathspecs, but the rule holds: no path from the frontend reaches outside the repo.
        for p in &filter.paths {
            fs::resolve(r, p)?;
        }
        match (all, rev) {
            (Some(_), Some(_)) => Err("all branches, or one: not both".into()),
            (Some(refs), None) => git::log_all(r, &refs, skip, limit, &filter),
            (None, rev) => git::log_filtered(r, rev.as_deref(), skip, limit, &filter),
        }
    })
    .await
}

#[tauri::command]
pub async fn log_compare(
    state: State<'_, AppState>,
    with: String,
    incoming: bool,
    skip: u32,
    limit: u32,
) -> Res<Vec<git::Commit>> {
    in_repo(&state, move |r| {
        git::log_compare(r, &with, incoming, skip, limit)
    })
    .await
}

#[tauri::command]
pub async fn compare_counts(state: State<'_, AppState>, with: String) -> Res<(u32, u32)> {
    in_repo(&state, move |r| git::compare_counts(r, &with)).await
}

#[tauri::command]
pub async fn bisect_start(state: State<'_, AppState>, good: String) -> Res<git::BisectStep> {
    in_repo(&state, move |r| git::bisect_start(r, &good)).await
}

#[tauri::command]
pub async fn bisect_mark(state: State<'_, AppState>, verdict: String) -> Res<git::BisectStep> {
    in_repo(&state, move |r| git::bisect_mark(r, &verdict)).await
}

#[tauri::command]
pub async fn reflog(state: State<'_, AppState>, limit: u32) -> Res<Vec<git::ReflogEntry>> {
    in_repo(&state, move |r| git::reflog(r, limit)).await
}

#[tauri::command]
pub async fn compare_files(state: State<'_, AppState>, with: String) -> Res<git::CompareFiles> {
    in_repo(&state, move |r| git::compare_files(r, &with)).await
}

#[tauri::command]
pub async fn find_commit(state: State<'_, AppState>, sha: String) -> Res<Option<git::Commit>> {
    in_repo(&state, move |r| git::find_commit(r, &sha)).await
}

#[tauri::command]
pub async fn commit_files(state: State<'_, AppState>, sha: String) -> Res<Vec<git::FileChange>> {
    in_repo(&state, move |r| git::commit_files(r, &sha)).await
}

#[tauri::command]
pub async fn blame(state: State<'_, AppState>, path: String) -> Res<git::Blame> {
    in_repo(&state, move |r| {
        fs::resolve(r, &path)?;
        git::blame(r, &path)
    })
    .await
}

#[tauri::command]
pub async fn commit_details(state: State<'_, AppState>, sha: String) -> Res<git::CommitDetails> {
    in_repo(&state, move |r| git::commit_details(r, &sha)).await
}

/// Rewords, squashes, drops or moves a commit of the branch; true when it stopped on conflicts.
#[tauri::command]
pub async fn rewrite(state: State<'_, AppState>, head: String, edit: rewrite::Edit) -> Res<bool> {
    let label = match &edit {
        rewrite::Edit::Reword { sha, .. } => format!("Reword {}", short(sha)),
        rewrite::Edit::Squash {
            sha,
            message: Some(_),
        } => format!("Squash {}", short(sha)),
        rewrite::Edit::Squash { sha, message: None } => format!("Fixup {}", short(sha)),
        rewrite::Edit::Drop { sha } => format!("Drop {}", short(sha)),
        rewrite::Edit::Move { sha, .. } => format!("Move {}", short(sha)),
    };
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        rewrite::run(r, &head, &edit)
    })
    .await
}

#[tauri::command]
pub async fn undo_commit(state: State<'_, AppState>, sha: String) -> Res<()> {
    let label = format!("Undo commit {}", short(&sha));
    journaled(&state, Action::new(label, Mode::Soft), move |r| {
        git::undo_commit(r, &sha)
    })
    .await
}

#[tauri::command]
pub async fn reset(
    state: State<'_, AppState>,
    sha: String,
    mode: git::ResetMode,
    head: String,
) -> Res<()> {
    let label = format!("Reset to {}", short(&sha));
    // Undone the same way, except that a hard reset's lost changes can't come back.
    let back = match mode {
        git::ResetMode::Soft => Mode::Soft,
        git::ResetMode::Mixed => Mode::Mixed,
        git::ResetMode::Hard => Mode::Keep,
    };
    journaled(&state, Action::new(label, back), move |r| {
        git::reset(r, &sha, mode, &head)
    })
    .await
}

#[tauri::command]
pub async fn drops_pushed(state: State<'_, AppState>, sha: String) -> Res<bool> {
    in_repo(&state, move |r| git::drops_pushed(r, &sha)).await
}

#[tauri::command]
pub async fn revert(state: State<'_, AppState>, sha: String) -> Res<bool> {
    let label = format!("Revert {}", short(&sha));
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::revert(r, &sha)
    })
    .await
}

#[tauri::command]
pub async fn cherry_pick(state: State<'_, AppState>, sha: String) -> Res<bool> {
    let label = format!("Cherry-pick {}", short(&sha));
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::cherry_pick(r, &sha)
    })
    .await
}

/// Picks onto the branch of another worktree (`path`), running git there. The entry goes in
/// that worktree's undo history, where a pick stopped on conflicts is also continued.
#[tauri::command]
pub async fn cherry_pick_into(state: State<'_, AppState>, path: String, sha: String) -> Res<bool> {
    let journal = state.journal.clone();
    in_repo(&state, move |r| {
        let target = git::pick_target(r, &path)?;
        let action = Action::new(format!("Cherry-pick {}", short(&sha)), Mode::Keep);
        journal.record(&target, action, |t| git::cherry_pick_into(t, &sha))
    })
    .await
}

#[tauri::command]
pub async fn checkout_commit(state: State<'_, AppState>, sha: String) -> Res<()> {
    let label = format!("Check out {}", short(&sha));
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::checkout_commit(r, &sha)
    })
    .await
}

#[tauri::command]
pub async fn create_branch_at(state: State<'_, AppState>, name: String, sha: String) -> Res<()> {
    let label = format!("Create branch {name}");
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::create_branch_at(r, &name, &sha)
    })
    .await
}
