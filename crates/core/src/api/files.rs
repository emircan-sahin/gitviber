use crate::state::{Core, Res};
use crate::{clipboard, definitions, external, fs, git, grep, open_in};

pub fn diff_pair(
    core: &Core,
    kind: String,
    path: String,
    old_path: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    whitespace: Option<String>,
) -> Res<git::DiffPair> {
    core.in_repo(move |r| {
        // A plain folder has no HEAD: the file against itself, so no line shows as added.
        let pair_kind = if kind == "worktree" && git::outside_repo(r) {
            "files"
        } else {
            kind.as_str()
        };
        git::diff_pair(
            r,
            pair_kind,
            &path,
            old_path.as_deref(),
            sha.as_deref(),
            base.as_deref(),
            whitespace.as_deref(),
            // The file view reads what a link points to, as editors do.
            |p| match kind.as_str() {
                "worktree" => fs::read_file(r, p),
                _ => fs::read_diff_side(r, p),
            },
        )
    })
}

/// Raw bytes: a video can be large.
pub fn media(
    core: &Core,
    kind: String,
    path: String,
    old_path: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    original: bool,
) -> Res<Vec<u8>> {
    core.in_repo(move |r| {
        git::media(
            r,
            &kind,
            &path,
            old_path.as_deref(),
            sha.as_deref(),
            base.as_deref(),
            original,
            |p| fs::read_media(r, p),
        )
    })
}

/// One side of a diff as a file to copy: the working tree's own, or a stored version saved under its name.
pub fn media_file(
    core: &Core,
    kind: String,
    path: String,
    old_path: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    original: bool,
) -> Res<String> {
    core.in_repo(move |r| {
        let side = if original {
            old_path.as_deref().unwrap_or(&path)
        } else {
            &path
        };
        if git::side_on_disk(&kind, sha.as_deref(), base.as_deref(), original)? {
            return Ok(fs::resolve(r, side)?.to_string_lossy().into_owned());
        }
        let name = std::path::Path::new(side)
            .file_name()
            .map_or("file".into(), |n| n.to_string_lossy().into_owned());
        let bytes = git::media(
            r,
            &kind,
            &path,
            old_path.as_deref(),
            sha.as_deref(),
            base.as_deref(),
            original,
            |p| fs::read_media(r, p),
        )?;
        clipboard::save_named(&name, &bytes)
    })
}

pub fn list_dir(core: &Core, path: String) -> Res<Vec<fs::Entry>> {
    core.in_repo(move |r| fs::list_dir(r, &path))
}

pub fn list_files(core: &Core) -> Res<Vec<String>> {
    core.in_repo(fs::list_files)
}

pub fn list_ignored(core: &Core) -> Res<Vec<fs::Entry>> {
    core.in_repo(fs::list_ignored)
}

/// Whether each path (repo-relative) is a file, a folder, or neither: for terminal links.
pub fn path_kinds(core: &Core, paths: Vec<String>) -> Res<Vec<Option<fs::Kind>>> {
    core.in_repo(move |r| Ok(fs::kinds(r, &paths)))
}

/// Absolute and `~` paths outside the repo that terminal links name (lib/links): what each is
/// on disk, to show read-only. Nothing here writes.
pub fn external_find(_core: &Core, paths: Vec<String>) -> Res<Vec<Option<external::Found>>> {
    Ok(external::find(&paths))
}

pub fn external_read_file(_core: &Core, path: String) -> Res<git::FileText> {
    Ok(external::read_file(&path))
}

/// Raw bytes, as `media`.
pub fn external_media(_core: &Core, path: String) -> Res<Vec<u8>> {
    external::read_media(&path)
}

pub fn external_reveal(_core: &Core, path: String) -> Res<()> {
    external::reveal(&path)
}

/// When each path (repo-relative) was last modified, in ms since 1970; None: not on disk.
/// Changes sorts by it, read once per refresh.
pub fn file_mtimes(core: &Core, paths: Vec<String>) -> Res<Vec<Option<f64>>> {
    core.in_repo(move |r| {
        Ok(paths
            .iter()
            .map(|p| {
                let (_, at) = fs::stamp(r, p)?;
                let since = at.duration_since(std::time::UNIX_EPOCH).ok()?;
                Some(since.as_millis() as f64)
            })
            .collect())
    })
}

/// Search in files; a newer search stops this one, which then fails with grep::CANCELLED.
pub fn search_files(core: &Core, query: grep::Query) -> Res<grep::Found> {
    core.in_repo(move |r| grep::search(r, &query))
}

/// Stops the running search: its query was cleared, or the view went.
pub fn cancel_search(_core: &Core) {
    grep::cancel();
}

/// Go to Definition in the code view; a newer lookup stops this one (definitions::CANCELLED).
pub fn definitions(core: &Core, request: definitions::Request) -> Res<Vec<definitions::Location>> {
    core.in_repo(move |r| definitions::find(r, &request))
}

/// Go to References in the code view; a newer lookup stops this one (definitions::CANCELLED).
pub fn references(core: &Core, request: definitions::Request) -> Res<Vec<definitions::Location>> {
    core.in_repo(move |r| definitions::references(r, &request))
}

pub fn read_file(core: &Core, path: String) -> Res<git::FileText> {
    core.in_repo(move |r| Ok(fs::read_file(r, &path)))
}

pub fn tree_paths(core: &Core, rev: String) -> Res<Vec<String>> {
    core.in_repo(move |r| git::tree_paths(r, &rev))
}

pub fn text_at(core: &Core, rev: String, path: String) -> Res<git::FileText> {
    core.in_repo(move |r| git::text_at(r, &rev, &path))
}

pub fn write_file(core: &Core, path: String, content: String) -> Res<()> {
    core.in_repo(move |r| fs::write_file(r, &path, &content))
}

pub fn create_file(core: &Core, path: String) -> Res<()> {
    core.in_repo(move |r| fs::create_file(r, &path))
}

pub fn create_dir(core: &Core, path: String) -> Res<()> {
    core.in_repo(move |r| fs::create_dir(r, &path))
}

/// Holds the index: a case-only rename of a tracked file renames it there too.
pub fn rename_path(core: &Core, from: String, to: String) -> Res<()> {
    core.indexed(move |r| fs::rename_entry(r, &from, &to))
}

pub fn trash_path(core: &Core, path: String) -> Res<()> {
    core.in_repo(move |r| fs::trash(r, &path))
}

pub fn reveal_path(core: &Core, path: String) -> Res<()> {
    core.in_repo(move |r| fs::reveal(r, &path))
}

/// The apps "Open in…" can use, as installed right now.
pub fn open_in_apps(_core: &Core) -> Res<Vec<open_in::Installed>> {
    Ok(open_in::installed())
}

/// An app's icon as a small PNG; empty off macOS. Raw bytes, as `media`.
pub fn open_in_icon(_core: &Core, app: String) -> Res<Vec<u8>> {
    Ok(open_in::icon(&app).unwrap_or_default())
}

/// `path` in the open worktree ("" for all of it) in a known app; editors go to `line`.
/// `project`: the worktree as a project, with the file in it (open_in::open).
pub fn open_in(
    core: &Core,
    app: String,
    path: String,
    line: Option<u32>,
    project: bool,
) -> Res<()> {
    core.in_repo(move |r| open_in::open(r, &path, line, &app, project))
}

/// The same with the user's own command template.
pub fn open_in_custom(
    core: &Core,
    command: String,
    path: String,
    line: Option<u32>,
    project: bool,
) -> Res<()> {
    core.in_repo(move |r| open_in::open_custom(r, &path, line, &command, project))
}
