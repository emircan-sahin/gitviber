//! "Open in…": the editors, terminals and git apps installed here, and opening the open
//! worktree (or a file in it, at a line) in one. Everything runs as an executable with an
//! argument list, never through a shell: paths can hold quotes, spaces and `$`.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    Editor,
    Terminal,
    Other,
}
use Group::*;

/// One known app. Argument templates take `{app}` (its bundle), `{path}` (a folder), `{file}`
/// and `{line}`; see `expand`. Windows and Linux would add their own detection and templates
/// beside the macOS ones.
struct App {
    id: &'static str,
    name: &'static str,
    group: Group,
    /// macOS: the first one installed is used, so stable builds go before previews.
    bundle_ids: &'static [&'static str],
    folder: &'static [&'static str],
    /// A file at a line, for apps with a CLI for it; the rest get the file without a line.
    file_line: Option<&'static [&'static str]>,
}

const OPEN: &[&str] = &["open", "-a", "{app}", "{path}"];
const OPEN_FILE: &[&str] = &["open", "-a", "{app}", "{file}"];
// -n: a running IDE ignores the arguments of a plain `open`; a new launcher hands them over.
const JETBRAINS: &[&str] = &[
    "open", "-na", "{app}", "--args", "--line", "{line}", "{file}",
];

const fn app(
    id: &'static str,
    name: &'static str,
    group: Group,
    bundle_ids: &'static [&'static str],
) -> App {
    App {
        id,
        name,
        group,
        bundle_ids,
        folder: OPEN,
        file_line: None,
    }
}

const fn vscode_like(
    id: &'static str,
    name: &'static str,
    bundle_id: &'static [&'static str],
    cli: &'static [&'static str],
) -> App {
    App {
        file_line: Some(cli),
        ..app(id, name, Editor, bundle_id)
    }
}

const fn jetbrains(
    id: &'static str,
    name: &'static str,
    bundle_ids: &'static [&'static str],
) -> App {
    App {
        file_line: Some(JETBRAINS),
        ..app(id, name, Editor, bundle_ids)
    }
}

/// Terminals that take the folder as a flag rather than as a document to open.
const fn terminal_args(
    id: &'static str,
    name: &'static str,
    bundle_id: &'static [&'static str],
    folder: &'static [&'static str],
) -> App {
    App {
        folder,
        ..app(id, name, Terminal, bundle_id)
    }
}

/// In menu order within each group. One row per app, kept on one line each.
#[rustfmt::skip]
const APPS: &[App] = &[
    vscode_like("vscode", "VS Code", &["com.microsoft.VSCode"], &["{app}/Contents/Resources/app/bin/code", "-g", "{file}:{line}"]),
    vscode_like("vscode-insiders", "VS Code Insiders", &["com.microsoft.VSCodeInsiders"], &["{app}/Contents/Resources/app/bin/code-insiders", "-g", "{file}:{line}"]),
    vscode_like("cursor", "Cursor", &["com.todesktop.230313mzl4w4u92"], &["{app}/Contents/Resources/app/bin/cursor", "-g", "{file}:{line}"]),
    vscode_like("windsurf", "Windsurf", &["com.exafunction.windsurf"], &["{app}/Contents/Resources/app/bin/windsurf", "-g", "{file}:{line}"]),
    vscode_like("zed", "Zed", &["dev.zed.Zed", "dev.zed.Zed-Preview"], &["{app}/Contents/MacOS/cli", "{file}:{line}"]),
    vscode_like("sublime", "Sublime Text", &["com.sublimetext.4", "com.sublimetext.3"], &["{app}/Contents/SharedSupport/bin/subl", "{file}:{line}"]),
    app("nova", "Nova", Editor, &["com.panic.Nova"]),
    app("xcode", "Xcode", Editor, &["com.apple.dt.Xcode"]),
    jetbrains("intellij", "IntelliJ IDEA", &["com.jetbrains.intellij", "com.jetbrains.intellij.ce"]),
    jetbrains("webstorm", "WebStorm", &["com.jetbrains.WebStorm"]),
    jetbrains("pycharm", "PyCharm", &["com.jetbrains.pycharm", "com.jetbrains.pycharm.ce"]),
    jetbrains("rustrover", "RustRover", &["com.jetbrains.rustrover"]),
    jetbrains("goland", "GoLand", &["com.jetbrains.goland"]),
    jetbrains("clion", "CLion", &["com.jetbrains.CLion"]),
    jetbrains("phpstorm", "PhpStorm", &["com.jetbrains.PhpStorm"]),
    jetbrains("rider", "Rider", &["com.jetbrains.rider"]),
    app("terminal", "Terminal", Terminal, &["com.apple.Terminal"]),
    app("iterm", "iTerm2", Terminal, &["com.googlecode.iterm2"]),
    terminal_args("ghostty", "Ghostty", &["com.mitchellh.ghostty"], &["open", "-na", "{app}", "--args", "--working-directory={path}"]),
    app("warp", "Warp", Terminal, &["dev.warp.Warp-Stable"]),
    terminal_args("wezterm", "WezTerm", &["com.github.wez.wezterm"], &["open", "-na", "{app}", "--args", "start", "--cwd", "{path}"]),
    terminal_args("kitty", "kitty", &["net.kovidgoyal.kitty"], &["open", "-na", "{app}", "--args", "--directory", "{path}"]),
    terminal_args("alacritty", "Alacritty", &["org.alacritty"], &["open", "-na", "{app}", "--args", "--working-directory", "{path}"]),
    app("github-desktop", "GitHub Desktop", Other, &["com.github.GitHubClient"]),
    app("fork", "Fork", Other, &["com.DanPristupov.Fork"]),
    app("tower", "Tower", Other, &["com.fournova.Tower3"]),
    app("sourcetree", "Sourcetree", Other, &["com.torusknot.SourceTreeNotMAS"]),
];

#[derive(Serialize)]
pub struct Installed {
    id: &'static str,
    name: &'static str,
    group: Group,
}

pub fn installed() -> Vec<Installed> {
    APPS.iter()
        .filter(|a| bundle_path(a).is_some())
        .map(|a| Installed {
            id: a.id,
            name: a.name,
            group: a.group,
        })
        .collect()
}

fn bundle_path(app: &App) -> Option<PathBuf> {
    app.bundle_ids.iter().find_map(|id| app_path(id))
}

/// Where LaunchServices has the app, as `open -b` would find it.
#[cfg(target_os = "macos")]
fn app_path(bundle_id: &str) -> Option<PathBuf> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::{c_char, CStr, CString};

    let id = CString::new(bundle_id).ok()?;
    let path = objc2::rc::autoreleasepool(|_| unsafe {
        let workspace: *mut AnyObject = msg_send![AnyClass::get(c"NSWorkspace")?, sharedWorkspace];
        let id: *mut AnyObject =
            msg_send![AnyClass::get(c"NSString")?, stringWithUTF8String: id.as_ptr()];
        let url: *mut AnyObject = msg_send![workspace, URLForApplicationWithBundleIdentifier: id];
        if url.is_null() {
            return None;
        }
        let path: *mut AnyObject = msg_send![url, path];
        let utf8: *const c_char = msg_send![path, UTF8String];
        (!utf8.is_null())
            .then(|| PathBuf::from(CStr::from_ptr(utf8).to_string_lossy().into_owned()))
    })?;
    // LaunchServices still knows an app that was just moved to the Trash.
    (!path.to_string_lossy().contains("/.Trash/")).then_some(path)
}

#[cfg(not(target_os = "macos"))]
fn app_path(_bundle_id: &str) -> Option<PathBuf> {
    None
}

/// What the placeholders stand for.
struct Context {
    app: Option<PathBuf>,
    path: PathBuf,
    file: Option<PathBuf>,
    line: u32,
}

/// Fills in a template. Without a file, an argument naming `{file}` becomes the folder as a
/// whole, so `-g {file}:{line}` degrades to `-g <folder>` instead of `<folder>:1`.
fn expand<S: AsRef<str>>(template: &[S], ctx: &Context) -> Vec<String> {
    let path = ctx.path.to_string_lossy();
    let app = ctx.app.as_ref().map(|a| a.to_string_lossy());
    let line = ctx.line.to_string();
    template
        .iter()
        .map(|arg| {
            let arg = arg.as_ref();
            let file = match &ctx.file {
                Some(f) => f.to_string_lossy(),
                None if arg.contains("{file}") => return path.to_string(),
                None => path.clone(),
            };
            let mut values = vec![("{path}", &*path), ("{file}", &*file), ("{line}", &*line)];
            if let Some(app) = &app {
                values.push(("{app}", app));
            }
            fill(arg, &values)
        })
        .collect()
}

/// One pass, so a path that happens to contain `{line}` is left alone.
fn fill(arg: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = arg;
    'scan: while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        for (name, value) in values {
            if let Some(after) = rest.strip_prefix(name) {
                out.push_str(value);
                rest = after;
                continue 'scan;
            }
        }
        out.push('{');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

/// `rel` in the worktree ("" is the worktree itself): the folder to open, and the file if
/// it is one. Both go through fs::resolve, which keeps them inside the repo.
fn target(root: &Path, rel: &str) -> Result<(PathBuf, Option<PathBuf>), String> {
    if rel.is_empty() {
        return Ok((root.to_path_buf(), None));
    }
    let full = crate::fs::resolve(root, rel)?;
    if full.is_dir() {
        Ok((full, None))
    } else if full.is_file() {
        Ok((root.to_path_buf(), Some(full)))
    } else {
        Err(format!("{rel} is not on disk"))
    }
}

pub fn open(root: &Path, rel: &str, line: Option<u32>, id: &str) -> Result<(), String> {
    let app = APPS
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| format!("unknown app: {id}"))?;
    let bundle = bundle_path(app).ok_or_else(|| format!("{} is not installed", app.name))?;
    let (dir, file) = target(root, rel)?;
    // Terminals open where they're pointed; a git client wants the repository itself.
    let (path, file) = match app.group {
        Editor => (dir, file),
        Terminal => (dir, None),
        Other => (root.to_path_buf(), None),
    };
    let ctx = Context {
        app: Some(bundle),
        path,
        file,
        line: line.unwrap_or(1),
    };
    let argv = match (&ctx.file, app.file_line) {
        (None, _) => expand(app.folder, &ctx),
        (Some(_), Some(t)) => {
            let cli = expand(t, &ctx);
            // The CLI moved in some version of the app: the file without its line beats failing.
            if Path::new(&cli[0]).is_absolute() && !Path::new(&cli[0]).exists() {
                expand(OPEN_FILE, &ctx)
            } else {
                cli
            }
        }
        (Some(_), None) => expand(OPEN_FILE, &ctx),
    };
    launch(&argv, root)
}

/// A user's own command (Settings → General → Open In). With no placeholder it gets the
/// file, or the folder, as its last argument.
pub fn open_custom(root: &Path, rel: &str, line: Option<u32>, command: &str) -> Result<(), String> {
    let mut template = crate::process::split_command(command)?;
    if template.is_empty() {
        return Err("the command is empty".into());
    }
    if !template
        .iter()
        .any(|a| ["{path}", "{file}", "{line}"].iter().any(|p| a.contains(p)))
    {
        template.push("{file}".into());
    }
    let (path, file) = target(root, rel)?;
    let ctx = Context {
        app: None,
        path,
        file,
        line: line.unwrap_or(1),
    };
    launch(&expand(&template, &ctx), root)
}

fn launch(argv: &[String], dir: &Path) -> Result<(), String> {
    let (program, args) = argv.split_first().ok_or("nothing to run")?;
    let shown = Path::new(program)
        .file_name()
        .map_or(program.clone(), |n| n.to_string_lossy().into_owned());
    let mut child = Command::new(program)
        .args(args)
        .current_dir(dir)
        // Also where a bare program name is looked up: Homebrew's bin isn't on a Finder app's PATH.
        .env("PATH", crate::process::search_path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!("{shown}: command not found"),
            _ => format!("{shown}: {e}"),
        })?;
    // `open` and the editors' CLIs hand over and exit at once, so their failures show. A custom
    // command may be the app itself and keep running; it's waited on elsewhere, leaving no zombie.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("{shown} failed ({status})"))
            };
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(file: Option<&str>) -> Context {
        Context {
            app: Some("/Applications/Visual Studio Code.app".into()),
            path: "/work/my repo".into(),
            file: file.map(PathBuf::from),
            line: 42,
        }
    }

    #[test]
    fn expands_a_file_at_a_line() {
        let t = [
            "{app}/Contents/Resources/app/bin/code",
            "-g",
            "{file}:{line}",
        ];
        assert_eq!(
            expand(&t, &ctx(Some("/work/my repo/src/a b.ts"))),
            [
                "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
                "-g",
                "/work/my repo/src/a b.ts:42"
            ]
        );
    }

    #[test]
    fn without_a_file_the_argument_is_the_folder() {
        let t = ["code", "-g", "{file}:{line}"];
        assert_eq!(expand(&t, &ctx(None)), ["code", "-g", "/work/my repo"]);
        assert_eq!(
            expand(&["--working-directory={path}"], &ctx(None)),
            ["--working-directory=/work/my repo"]
        );
    }

    #[test]
    fn values_are_not_expanded_again() {
        let c = Context {
            app: None,
            path: "/w/{line}$(rm -rf ~)".into(),
            file: None,
            line: 7,
        };
        assert_eq!(
            expand(&["{path}", "{app}", "{nope}"], &c),
            ["/w/{line}$(rm -rf ~)", "{app}", "{nope}"]
        );
    }

    #[test]
    fn the_table_is_well_formed() {
        let mut ids: Vec<_> = APPS.iter().map(|a| a.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), APPS.len(), "app ids must be unique");
        for a in APPS {
            assert!(!a.bundle_ids.is_empty(), "{}", a.id);
            assert!(a.folder.iter().any(|x| x.contains("{path}")), "{}", a.id);
            if let Some(t) = a.file_line {
                assert!(t.iter().any(|x| x.contains("{file}")), "{}", a.id);
            }
        }
    }

    /// Terminal ships with every Mac, so LaunchServices must find it.
    #[cfg(target_os = "macos")]
    #[test]
    fn finds_installed_apps() {
        let found = installed();
        assert!(found.iter().any(|a| a.id == "terminal"));
        assert!(app_path("com.example.not-an-app").is_none());
    }

    #[test]
    fn stays_inside_the_repo() {
        let dir = std::env::temp_dir().join(format!("gitviber-open-in-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.txt"), "a").unwrap();
        assert_eq!(target(&dir, "").unwrap(), (dir.clone(), None));
        assert_eq!(target(&dir, "src").unwrap(), (dir.join("src"), None));
        assert_eq!(
            target(&dir, "src/a.txt").unwrap(),
            (dir.clone(), Some(dir.join("src/a.txt")))
        );
        assert!(target(&dir, "../etc").is_err());
        assert!(target(&dir, "/etc").is_err());
        assert!(target(&dir, "missing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reports_a_custom_command_that_fails() {
        let dir = std::env::temp_dir();
        assert_eq!(open_custom(&dir, "", None, "true {path}"), Ok(()));
        assert!(open_custom(&dir, "", None, "false")
            .unwrap_err()
            .contains("false failed"));
        assert_eq!(
            open_custom(&dir, "", None, "gitviber-no-such-editor"),
            Err("gitviber-no-such-editor: command not found".into())
        );
        assert!(open_custom(&dir, "", None, "  ").is_err());
    }
}
