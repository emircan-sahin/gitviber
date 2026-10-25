use crate::state::{Core, Res};
use crate::{
    agents, cli, clipboard, conversations, errors, git, launch, notifications, process, pty,
};
use crate::{events, opened, paths};
use std::path::Path;

/// Any folder, unlike the repo commands: the shell can `cd` anywhere the user can anyway.
/// `integration`: load the shell integration, whose scripts live in the app's cache folder.
/// `folder`: where the shell it continues was last (a split, a restore), if that's still there.
/// `output` gets the shell's bytes on its reader thread (false stops reading); past 512 KiB
/// handed over and not yet `pty_ack`ed, reading waits up to a second for the terminal.
#[allow(clippy::too_many_arguments)]
pub fn pty_spawn(
    core: &Core,
    cwd: String,
    cols: u16,
    rows: u16,
    integration: bool,
    folder: Option<String>,
    output: impl FnMut(&[u8]) -> bool + Send + 'static,
    exit: impl FnOnce(Option<pty::Exit>) + Send + 'static,
) -> Res<pty::Spawned> {
    let pane = cwd.clone();
    let cwd = folder.filter(|f| Path::new(f).is_dir()).unwrap_or(cwd);
    let scripts = integration
        .then(paths::cache_dir)
        .flatten()
        .map(|dir| dir.join("shell-integration"));
    core.ptys.spawn(
        Path::new(&pane),
        Path::new(&cwd),
        cols,
        rows,
        scripts.as_deref(),
        Box::new(output),
        Box::new(exit),
    )
}

/// Blocks while a program isn't reading: a paste into one waits until it reads.
pub fn pty_write(core: &Core, id: u32, data: String, binary: bool) -> Res<()> {
    let writer = core.ptys.writer(id)?;
    pty::write(&writer, &pty::input_bytes(&data, binary))
}

/// On the main thread, where AppKit's pasteboard and GTK's clipboard belong. GTK's answers only
/// where GTK runs: elsewhere on Linux this finds nothing, and the app reads its own clipboard.
/// An image is saved off it, with `Image::save`.
pub fn terminal_paste(_core: &Core) -> clipboard::Clip {
    clipboard::read()
}

/// The clipboard's text, for Apply Patch from Clipboard. On the main thread, as `terminal_paste`.
pub fn clipboard_text(_core: &Core) -> Res<String> {
    clipboard::read_text()
}

/// Text worked out after a click (a patch), onto the clipboard. On the main thread, as
/// `terminal_paste`.
pub fn clipboard_write(_core: &Core, text: String) -> Res<()> {
    clipboard::write_text(&text)
}

/// On the main thread, as `terminal_paste`.
pub fn copy_files(_core: &Core, paths: Vec<String>) -> Res<()> {
    clipboard::copy_files(paths)
}

/// What a program in the terminal copies (OSC 52). On the main thread, as `terminal_paste`.
pub fn terminal_copy(_core: &Core, text: String) -> Res<()> {
    clipboard::write_text(&text)
}

/// The `gitviber` command (cli.rs).
pub fn install_cli(_core: &Core) -> Res<String> {
    cli::install()
}

/// Paths opened from outside the window since the app last asked (opened.rs). Off the main
/// thread: resolving a path on a stalled network volume can hang.
pub fn take_opened(core: &Core) -> Res<opened::Taken> {
    Ok(opened::take(core))
}

pub fn keep_dropped(_core: &Core, paths: Vec<String>) -> Vec<String> {
    clipboard::keep_dropped(paths)
}

pub fn pty_resize(core: &Core, id: u32, cols: u16, rows: u16) -> Res<()> {
    core.ptys.resize(id, cols, rows)
}

/// `bytes` of a terminal's output it has parsed, which lets a paused reader go on.
pub fn pty_ack(core: &Core, id: u32, bytes: usize) {
    core.ptys.ack(id, bytes)
}

pub fn pty_cwd(core: &Core, id: u32) -> Option<String> {
    core.ptys.cwd(id).map(|p| p.to_string_lossy().into_owned())
}

/// The app closes the pane's agent browser tab after this: gone first, a command between the
/// two makes no tab for it.
pub fn pty_kill(core: &Core, id: u32) {
    core.agents.forget(id);
    core.ptys.kill(id);
}

/// How many terminals (of `ids`, or all) are running a command, which restarting the app or
/// killing them would stop.
pub fn pty_busy(core: &Core, ids: Option<Vec<u32>>) -> usize {
    core.ptys.busy(ids.as_deref())
}

/// The coding agents (agents.rs) the terminals `ids` run, by id; a terminal with none isn't
/// listed. Each one's state file is watched from then on, its changes sent as
/// Event::AgentState.
pub fn pty_agents(
    core: &Core,
    ids: Vec<u32>,
) -> Res<std::collections::HashMap<u32, agents::Agent>> {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return Ok(Default::default());
    };
    let mut running = std::collections::HashMap::new();
    for id in ids {
        let leader = core.ptys.foreground(id);
        let sink = || -> agents::Sink {
            let events = core.events();
            std::sync::Arc::new(move |id, state| {
                events(events::Event::AgentState(events::AgentState { id, state }))
            })
        };
        if let Some(agent) = core.agents.agent(id, leader, &home, sink) {
            running.insert(id, agent);
        }
    }
    Ok(running)
}

/// The conversations agents had in `cwd` (conversations.rs), newest first: read now, never kept.
pub fn agent_conversations(_core: &Core, cwd: String) -> Res<Vec<conversations::Conversation>> {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return Ok(vec![]);
    };
    Ok(conversations::list(std::path::Path::new(&cwd), &home))
}

/// Which of `paths` are folders still: a restored agent resumes only where it ran.
pub fn folders_left(_core: &Core, paths: Vec<String>) -> Vec<bool> {
    paths.iter().map(|p| Path::new(p).is_dir()).collect()
}

/// Files and folders dropped on the window away from the terminal panes: opened as `gitviber
/// <path>` opens them. Also what a second launch hands over on Linux and Windows.
pub fn open_dropped(core: &Core, paths: Vec<String>) {
    opened::push(core, paths.into_iter().map(Into::into).collect());
}

/// What a bug report asks for: app version and commit, OS, git, gh and the web view. Nothing
/// about the user or their repos.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    pub version: String,
    pub commit: String,
    pub os: String,
    pub arch: String,
    pub git: Option<String>,
    /// `gh --version`'s version, None when the GitHub CLI isn't installed.
    pub gh: Option<String>,
    /// The browser tab's engine, "WebKit 20621.1.15"; None until a build has one.
    pub webview: Option<String>,
}

pub fn about(core: &Core) -> Res<About> {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let checked = core.git.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let text = |program: &str, args: &[&str]| {
        let mut cmd = std::process::Command::new(process::resolve(program));
        cmd.args(args)
            .env("PATH", process::search_path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        process::spawn(&mut cmd)
            .and_then(|c| c.wait_with_output())
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let os = match std::env::consts::OS {
        "macos" => format!(
            "macOS {}",
            text("sw_vers", &["-productVersion"]).unwrap_or_default()
        ),
        // "Ubuntu 24.04.1 LTS", "Fedora Linux 40 (Workstation Edition)"
        "linux" => std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|f| {
                let name = f.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))?;
                Some(name.trim_matches('"').to_string())
            })
            .unwrap_or_else(|| "Linux".into()),
        other => other.to_string(),
    };
    Ok(About {
        version,
        commit: env!("GITVIBER_COMMIT").to_string(),
        os: os.trim().to_string(),
        arch: std::env::consts::ARCH.to_string(),
        // git_info checks it at launch; asked again only if that found none.
        git: checked
            .and_then(|g| g.version)
            .or_else(|| git::check_install().version),
        // "gh version 2.62.0 (2024-11-14)\nhttps://github.com/cli/cli/releases/…"
        gh: text("gh", &["--version"]).and_then(|v| {
            let first = v.lines().next()?;
            Some(first.trim_start_matches("gh version ").to_string())
        }),
        webview: None,
    })
}

pub fn open_url(_core: &Core, url: String) -> Res<()> {
    launch::open_url(&url)
}

/// Help → Show Logs: the error log selected in the file manager, or its folder before the first
/// error has created it.
pub fn show_logs(_core: &Core) -> Res<()> {
    let file = errors::file().ok_or("The log folder could not be found.")?;
    let path = if file.exists() {
        file
    } else {
        file.parent().unwrap_or(file)
    };
    launch::reveal(path.to_path_buf())
}

/// The UI's errors, into the app's error log. Off the main thread: every error toast writes it.
pub fn log_error(_core: &Core, source: String, message: String) {
    errors::write(&source, &message);
}

/// Whether the OS shows GitViber's notifications (notifications.rs).
pub fn notification_permission(_core: &Core) -> Res<notifications::Permission> {
    notifications::permission()
}

/// Asks the OS for it; waits while the user answers its prompt.
pub fn notification_request(_core: &Core) -> Res<notifications::Permission> {
    notifications::request()
}

/// `target`: given back with Event::NotificationClick when the user clicks it. Fails with
/// notifications::UNPOSTED where the app posts them itself.
pub fn notification_send(
    _core: &Core,
    title: String,
    body: String,
    target: Option<String>,
) -> Res<()> {
    notifications::send(&title, &body, target.as_deref())
}

/// At the running app's id: a dev build's bundle may have its own.
pub fn notification_settings(_core: &Core) -> Res<()> {
    let id = notifications::bundle_id().unwrap_or_else(|| paths::IDENTIFIER.to_string());
    launch::notification_settings(&id)
}
