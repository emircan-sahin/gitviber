use crate::journal::{files_label, Action, Mode};
use crate::state::{with_index_lock, Core, Res};
use crate::{git, github, handoff, lines, network, suggest};
use std::path::Path;

pub fn status(core: &Core) -> Res<git::RepoStatus> {
    core.read_repo(|r| {
        let mut st = git::status_in(r, core.git_dirs(r).as_ref())?;
        st.web_url = st.origin.as_ref().map(|url| {
            url.as_deref()
                .and_then(github::parse_remote)
                .map(|g| format!("https://github.com/{}/{}", g.owner, g.name))
        });
        Ok(st)
    })
}

pub fn branch_review(core: &Core, base: String) -> Res<git::BranchReview> {
    core.in_repo(move |r| git::branch_review(r, &base))
}

pub fn stage(core: &Core, paths: Vec<String>, allow_nested: bool) -> Res<()> {
    core.indexed(move |r| git::stage_with(r, &paths, allow_nested))
}

pub fn unstage(core: &Core, paths: Vec<String>) -> Res<()> {
    core.indexed(move |r| git::unstage(r, &paths))
}

/// Removes a stale index.lock that `path`, from git's error, names; see git::remove_index_lock.
/// Not while one of ours writes the index.
pub fn remove_index_lock(core: &Core, path: String) -> Res<()> {
    core.indexed_once(move |r| git::remove_index_lock(r, &path))
}

pub fn discard(core: &Core, paths: Vec<String>) -> Res<()> {
    let (journal, index) = (core.journal.clone(), core.index.clone());
    core.in_repo(move |r| {
        let label = files_label("Discard", &paths);
        let list = paths.clone();
        let write = move |r: &Path| git::discard(r, &list).map(|_| vec![]);
        journal
            .replace(r, label, "discarded", &paths, &index, write)
            .map(|_| ())
    })
}

/// Stages, unstages or discards some of a file's changed lines; a discard can be undone.
pub fn change_lines(core: &Core, request: lines::Request) -> Res<()> {
    if request.action != "discard" {
        return core.indexed(move |r| lines::run(r, &request));
    }
    let (journal, index) = (core.journal.clone(), core.index.clone());
    core.in_repo(move |r| {
        let paths = [request.path.clone()];
        let label = files_label("Discard", &paths);
        let write = move |r: &Path| lines::run(r, &request).map(|_| vec![]);
        journal
            .replace(r, label, "discarded", &paths, &index, write)
            .map(|_| ())
    })
}

pub fn commit(
    core: &Core,
    message: String,
    options: git::CommitOptions,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<()> {
    let subject = message.lines().next().unwrap_or("").trim();
    let label = match (options.amend, subject) {
        (true, "") => "Amend last commit".to_string(),
        (true, s) => format!("Amend \"{s}\""),
        (false, s) => format!("Commit \"{s}\""),
    };
    let lock = core.index.clone();
    let net = core.watch_network(op, progress);
    core.journaled(Action::new(label, Mode::Soft).ok_only(), move |r| {
        with_index_lock(&lock, r, |r| git::commit(r, &message, &options, &net))
    })
}

/// Staged files GitHub would refuse, asked about before they're committed.
pub fn large_staged(core: &Core) -> Res<Vec<git::LargeFile>> {
    core.in_repo(git::large_staged)
}

pub fn commit_template(core: &Core) -> Res<Option<String>> {
    core.in_repo(move |r| Ok(git::commit_template(r)))
}

pub fn recent_authors(core: &Core) -> Res<Vec<String>> {
    core.in_repo(git::recent_authors)
}

/// Runs the user's own agent CLI for a commit message (off unless they set one up).
pub fn suggest_message(
    core: &Core,
    command: String,
    prompt: String,
    scope: suggest::Scope,
) -> Res<String> {
    let cancel = core.suggest.start(suggest::Kind::Message);
    let flag = cancel.clone();
    let out = core.in_repo(move |r| suggest::run(r, &command, &prompt, scope, &flag));
    core.suggest.finish(suggest::Kind::Message, &cancel);
    out
}

/// The same, for a pull request's title and description from HEAD into `base`.
pub fn suggest_pull(core: &Core, command: String, prompt: String, base: String) -> Res<String> {
    let cancel = core.suggest.start(suggest::Kind::Pull);
    let flag = cancel.clone();
    let out = core.in_repo(move |r| suggest::run_pull(r, &command, &prompt, &base, &flag));
    core.suggest.finish(suggest::Kind::Pull, &cancel);
    out
}

/// The same, for a guided review of a commit, a pull request or the branch since it left its
/// base, or for its risks (`kind`): the two run side by side, on the same range.
pub fn suggest_guide(
    core: &Core,
    command: String,
    prompt: String,
    target: suggest::Target,
    agent: suggest::Agent,
    kind: suggest::Kind,
) -> Res<suggest::Guided> {
    if !matches!(kind, suggest::Kind::Guide | suggest::Kind::Risks) {
        return Err("not a guided review's run".into());
    }
    let cancel = core.suggest.start(kind);
    let flag = cancel.clone();
    let out =
        core.in_repo(move |r| suggest::run_guide(r, &command, &prompt, &target, &agent, &flag));
    core.suggest.finish(kind, &cancel);
    out
}

/// `suggest_guide`, with what the command prints handed to `output` as it prints it.
pub fn suggest_guide_streaming(
    core: &Core,
    command: String,
    prompt: String,
    target: suggest::Target,
    agent: suggest::Agent,
    kind: suggest::Kind,
    output: suggest::Output,
) -> Res<suggest::Guided> {
    if !matches!(kind, suggest::Kind::Guide | suggest::Kind::Risks) {
        return Err("not a guided review's run".into());
    }
    let cancel = core.suggest.start(kind);
    let flag = cancel.clone();
    let out = core.in_repo(move |r| {
        suggest::run_guide_streaming(r, &command, &prompt, &target, &agent, &flag, output)
    });
    core.suggest.finish(kind, &cancel);
    out
}

/// What the uncommitted changes are now, cheaply: a guide of them is outdated once it differs.
pub fn changes_stamp(core: &Core) -> Res<String> {
    core.read_repo(git::changes_stamp)
}

/// A guided review handed to Claude Code in a terminal: the line to type at the shell's prompt.
pub fn handoff_command(
    _core: &Core,
    command: String,
    model: Option<String>,
    effort: Option<String>,
    name: String,
    text: String,
    send: bool,
) -> Res<String> {
    handoff::command(&handoff::Handoff {
        command: &command,
        model: model.as_deref(),
        effort: effort.as_deref(),
        name: &name,
        text: &text,
        send,
    })
}

pub fn suggest_cancel(core: &Core, kind: suggest::Kind) {
    core.suggest.cancel(kind)
}

pub fn resolve_side(core: &Core, path: String, side: git::Side) -> Res<()> {
    core.indexed(move |r| git::resolve_side(r, &path, side))
}

/// A conflicted file's sides: merged again in diff3 style for each conflict's base, and how
/// each ends.
pub fn conflict_base(core: &Core, path: String) -> Res<Option<git::ConflictSides>> {
    core.read_repo(move |r| git::conflict_sides(r, &path))
}

/// The merge and diff tools git's config names, for "Open in <tool>"; read when a menu or a
/// conflict asks.
pub fn external_tools(core: &Core) -> Res<git::ExternalTools> {
    core.read_repo(|r| Ok(git::external_tools(r)))
}

/// Returns once the tool is closed, so the page reads the file and the index again.
pub fn open_merge_tool(core: &Core, path: String) -> Res<()> {
    core.in_repo(move |r| git::open_merge_tool(r, &path))
}

pub fn open_diff_tool(core: &Core, path: String, staged: bool) -> Res<()> {
    core.in_repo(move |r| git::open_diff_tool(r, &path, staged))
}
