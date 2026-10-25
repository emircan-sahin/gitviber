//! "Open in…": the editors, terminals and git apps installed here, and opening the open
//! worktree (or a file in it, at a line) in one. Everything runs as an executable with an
//! argument list, never through a shell: paths can hold quotes, spaces and `$`.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
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
    /// An editor's own command line, which can go to a file's line; the rest get the file alone.
    cli: Option<Cli>,
}

#[derive(Clone, Copy)]
struct Cli {
    run: &'static [&'static str],
    goto: Goto,
}

/// How a CLI takes a file at a line. Each also takes a project folder before it, opening the
/// file in that folder's window (Cli::args).
#[derive(Clone, Copy)]
enum Goto {
    /// `code -g file:line`, the VS Code family.
    Flag,
    /// `zed file:line`, `subl file:line`.
    Suffix,
    /// `idea --line N file`, the JetBrains IDEs.
    Line,
}

impl Cli {
    /// JetBrains' CommandLineProcessor opens a folder argument as a project, then a file after
    /// it in the open project holding it; VS Code opens files in the window of the folders
    /// given with them; Zed and Sublime Text open all their arguments in one window.
    fn args(self, ctx: &Context, project: bool) -> Vec<&'static str> {
        let folder: &[&str] = if project { &["{path}"] } else { &[] };
        let file = ctx.file.as_deref().unwrap_or(&ctx.path).to_string_lossy();
        let at: &[&str] = match self.goto {
            // With -g, VS Code's parseLineAndColumnAware reads every argument split at its
            // colons, any number among them a line: a path with a colon goes without its line.
            Goto::Flag
                if file.contains(':') || (project && ctx.path.to_string_lossy().contains(':')) =>
            {
                &["{file}"]
            }
            Goto::Flag => &["-g", "{file}:{line}"],
            // Zed's PathWithPosition takes the last one or two numbers after colons: with a
            // column too, a name's own `:N` stays in the name.
            Goto::Suffix if ends_in_number(&file) => &["{file}:{line}:1"],
            Goto::Suffix => &["{file}:{line}"],
            Goto::Line => &["--line", "{line}", "{file}"],
        };
        [self.run, folder, at].concat()
    }
}

/// `name:12`, which reads as a line.
fn ends_in_number(path: &str) -> bool {
    path.rsplit_once(':')
        .is_some_and(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

const OPEN: &[&str] = &["open", "-a", "{app}", "{path}"];
const OPEN_FILE: &[&str] = &["open", "-a", "{app}", "{file}"];
// -n: a running IDE ignores the arguments of a plain `open`; a new launcher hands them over.
const JETBRAINS: &[&str] = &["open", "-na", "{app}", "--args"];

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
        cli: None,
    }
}

const fn editor_cli(
    id: &'static str,
    name: &'static str,
    bundle_ids: &'static [&'static str],
    run: &'static [&'static str],
    goto: Goto,
) -> App {
    App {
        cli: Some(Cli { run, goto }),
        ..app(id, name, Editor, bundle_ids)
    }
}

const fn jetbrains(
    id: &'static str,
    name: &'static str,
    bundle_ids: &'static [&'static str],
) -> App {
    editor_cli(id, name, bundle_ids, JETBRAINS, Goto::Line)
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
    editor_cli("vscode", "VS Code", &["com.microsoft.VSCode"], &["{app}/Contents/Resources/app/bin/code"], Goto::Flag),
    editor_cli("vscode-insiders", "VS Code Insiders", &["com.microsoft.VSCodeInsiders"], &["{app}/Contents/Resources/app/bin/code-insiders"], Goto::Flag),
    editor_cli("cursor", "Cursor", &["com.todesktop.230313mzl4w4u92"], &["{app}/Contents/Resources/app/bin/cursor"], Goto::Flag),
    editor_cli("windsurf", "Windsurf", &["com.exafunction.windsurf"], &["{app}/Contents/Resources/app/bin/windsurf"], Goto::Flag),
    editor_cli("zed", "Zed", &["dev.zed.Zed", "dev.zed.Zed-Preview"], &["{app}/Contents/MacOS/cli"], Goto::Suffix),
    editor_cli("sublime", "Sublime Text", &["com.sublimetext.4", "com.sublimetext.3"], &["{app}/Contents/SharedSupport/bin/subl"], Goto::Suffix),
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

#[cfg(target_os = "macos")]
use mac::{app_icon, app_path};

#[cfg(not(target_os = "macos"))]
fn app_path(_bundle_id: &str) -> Option<PathBuf> {
    None
}

#[cfg(not(target_os = "macos"))]
fn app_icon(_bundle: &Path, _px: u32) -> Option<Vec<u8>> {
    None
}

/// The app's icon as a 32 px PNG (the menu shows it at 16, on a Retina screen), drawn once per
/// build of the app: an update that brings a new icon changes the bundle's modification time.
pub fn icon(id: &str) -> Option<Vec<u8>> {
    type Key = (PathBuf, Option<std::time::SystemTime>);
    static ICONS: LazyLock<Mutex<HashMap<Key, Vec<u8>>>> = LazyLock::new(Default::default);
    let icons = || ICONS.lock().unwrap_or_else(|e| e.into_inner());
    let bundle = APPS.iter().find(|a| a.id == id).and_then(bundle_path)?;
    let modified = bundle.metadata().and_then(|m| m.modified()).ok();
    let key = (bundle, modified);
    if let Some(png) = icons().get(&key) {
        return Some(png.clone());
    }
    // Drawn outside the lock: the menu asks for every app's at once.
    let png = app_icon(&key.0, 32)?;
    icons().insert(key, png.clone());
    Some(png)
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
/// it is one. Both go through fs::resolve, which keeps them inside the repo. A `project` is the
/// worktree, with `rel` as its file while that is one: the focused tab's file may have been
/// deleted since (by an agent, a checkout), which leaves the worktree to open.
fn target(root: &Path, rel: &str, project: bool) -> Result<(PathBuf, Option<PathBuf>), String> {
    if rel.is_empty() {
        return Ok((root.to_path_buf(), None));
    }
    let full = crate::fs::resolve(root, rel)?;
    if project {
        Ok((root.to_path_buf(), full.is_file().then_some(full)))
    } else if full.is_dir() {
        Ok((full, None))
    } else if full.is_file() {
        Ok((root.to_path_buf(), Some(full)))
    } else {
        Err(format!("{rel} is not on disk"))
    }
}

/// `project`: the worktree as a project (the status bar's Open in), an editor also going to the
/// file `rel` names when its CLI can take both; else `rel` itself, as a row's menu opens it.
pub fn open(
    root: &Path,
    rel: &str,
    line: Option<u32>,
    id: &str,
    project: bool,
) -> Result<(), String> {
    let app = APPS
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| format!("unknown app: {id}"))?;
    let bundle = bundle_path(app).ok_or_else(|| format!("{} is not installed", app.name))?;
    let (dir, file) = target(root, rel, project)?;
    // Terminals open where they're pointed; a git client wants the repository itself.
    let (path, file) = match app.group {
        Editor if project => (dir, file.filter(|_| app.cli.is_some())),
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
    let argv = match (&ctx.file, app.cli) {
        (None, _) => expand(app.folder, &ctx),
        (Some(_), Some(cli)) => {
            let cli = expand(&cli.args(&ctx, project), &ctx);
            // The CLI moved in some version of the app: the project, or the file without its
            // line, beats failing.
            if Path::new(&cli[0]).is_absolute() && !Path::new(&cli[0]).exists() {
                expand(if project { app.folder } else { OPEN_FILE }, &ctx)
            } else {
                cli
            }
        }
        (Some(_), None) => expand(OPEN_FILE, &ctx),
    };
    launch(&argv, root)
}

/// A user's own command (Settings → General → Open In). With no placeholder it gets the
/// file, or the folder (always, as a `project`), as its last argument.
pub fn open_custom(
    root: &Path,
    rel: &str,
    line: Option<u32>,
    command: &str,
    project: bool,
) -> Result<(), String> {
    let mut template = crate::process::split_command(command)?;
    if template.is_empty() {
        return Err("the command is empty".into());
    }
    if !template
        .iter()
        .any(|a| ["{path}", "{file}", "{line}"].iter().any(|p| a.contains(p)))
    {
        template.push(if project { "{path}" } else { "{file}" }.into());
    }
    let (path, file) = target(root, rel, project)?;
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
    let mut child = crate::process::spawn(
        Command::new(program)
            .args(args)
            .current_dir(dir)
            // Also where a bare program name is looked up: Homebrew's bin isn't on a Finder app's PATH.
            .env("PATH", crate::process::search_path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
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

/// LaunchServices and AppKit, through raw objc2 messages (objc.rs).
#[cfg(target_os = "macos")]
mod mac {
    use crate::objc::{bytes, c_string, ns_string, rust_string};
    use objc2::encode::{Encode, Encoding};
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use std::ffi::CString;
    use std::path::{Path, PathBuf};

    /// Where LaunchServices has the app, as `open -b` would find it.
    pub(super) fn app_path(bundle_id: &str) -> Option<PathBuf> {
        let id = CString::new(bundle_id).ok()?;
        let path = objc2::rc::autoreleasepool(|_| unsafe {
            let workspace: *mut AnyObject =
                msg_send![AnyClass::get(c"NSWorkspace")?, sharedWorkspace];
            let url: *mut AnyObject =
                msg_send![workspace, URLForApplicationWithBundleIdentifier: ns_string(&id)];
            if url.is_null() {
                return None;
            }
            rust_string(msg_send![url, path]).map(PathBuf::from)
        })?;
        // LaunchServices still knows an app that was just moved to the Trash.
        (!path.to_string_lossy().contains("/.Trash/")).then_some(path)
    }

    #[repr(C)]
    struct Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    const PAIR: [Encoding; 2] = [f64::ENCODING, f64::ENCODING];
    unsafe impl Encode for Rect {
        const ENCODING: Encoding = Encoding::Struct(
            "CGRect",
            &[
                Encoding::Struct("CGPoint", &PAIR),
                Encoding::Struct("CGSize", &PAIR),
            ],
        );
    }

    /// Finder's icon for `bundle`, as its Dock and the Open With menu show it: a PNG `px` wide,
    /// drawn into a bitmap of that size, as the icon's own TIFF holds every size up to 1024.
    pub(super) fn app_icon(bundle: &Path, px: u32) -> Option<Vec<u8>> {
        const PNG: usize = 4; // NSBitmapImageFileTypePNG
        const SOURCE_OVER: usize = 2; // NSCompositingOperationSourceOver
        let path = c_string(&bundle.to_string_lossy());
        let side = f64::from(px);
        objc2::rc::autoreleasepool(|_| unsafe {
            let workspace: *mut AnyObject =
                msg_send![AnyClass::get(c"NSWorkspace")?, sharedWorkspace];
            let image: *mut AnyObject = msg_send![workspace, iconForFile: ns_string(&path)];
            if image.is_null() {
                return None;
            }
            let rep: *mut AnyObject = msg_send![AnyClass::get(c"NSBitmapImageRep")?, alloc];
            let rep: *mut AnyObject = msg_send![
                rep,
                initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(),
                pixelsWide: px as isize,
                pixelsHigh: px as isize,
                bitsPerSample: 8isize,
                samplesPerPixel: 4isize,
                hasAlpha: Bool::YES,
                isPlanar: Bool::NO,
                colorSpaceName: ns_string(c"NSDeviceRGBColorSpace"),
                bytesPerRow: 0isize,
                bitsPerPixel: 0isize
            ];
            if rep.is_null() {
                return None;
            }
            let png = (|| {
                let graphics = AnyClass::get(c"NSGraphicsContext")?;
                let context: *mut AnyObject =
                    msg_send![graphics, graphicsContextWithBitmapImageRep: rep];
                if context.is_null() {
                    return None;
                }
                let _: () = msg_send![graphics, saveGraphicsState];
                let _: () = msg_send![graphics, setCurrentContext: context];
                let whole = Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                };
                let into = Rect {
                    x: 0.0,
                    y: 0.0,
                    w: side,
                    h: side,
                };
                let _: () = msg_send![image, drawInRect: into, fromRect: whole, operation: SOURCE_OVER, fraction: 1.0f64];
                let _: () = msg_send![graphics, restoreGraphicsState];
                let none: *mut AnyObject = msg_send![AnyClass::get(c"NSDictionary")?, dictionary];
                bytes(msg_send![rep, representationUsingType: PNG, properties: none])
            })();
            let _: () = msg_send![rep, release];
            png
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Terminal ships with every Mac.
        #[test]
        fn draws_an_apps_icon() {
            let terminal = app_path("com.apple.Terminal").unwrap();
            let png = app_icon(&terminal, 32).unwrap();
            assert!(png.starts_with(b"\x89PNG"));
            // IHDR's width and height.
            assert_eq!(png[16..24], [0, 0, 0, 32, 0, 0, 0, 32]);
            assert!(app_path("com.example.not-an-app").is_none());
        }
    }
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
    fn a_project_opens_with_the_file_in_it() {
        let file = ctx(Some("/work/my repo/src/a.ts"));
        let cli = |run, goto| Cli { run, goto };
        let code = cli(&["code"], Goto::Flag);
        assert_eq!(
            expand(&code.args(&file, true), &file),
            ["code", "/work/my repo", "-g", "/work/my repo/src/a.ts:42"]
        );
        assert_eq!(
            expand(&code.args(&file, false), &file),
            ["code", "-g", "/work/my repo/src/a.ts:42"]
        );
        assert_eq!(
            expand(&cli(&["subl"], Goto::Suffix).args(&file, true), &file),
            ["subl", "/work/my repo", "/work/my repo/src/a.ts:42"]
        );
        assert_eq!(
            expand(&cli(JETBRAINS, Goto::Line).args(&file, true), &file),
            [
                "open",
                "-na",
                "/Applications/Visual Studio Code.app",
                "--args",
                "/work/my repo",
                "--line",
                "42",
                "/work/my repo/src/a.ts"
            ]
        );
    }

    /// A colon in a path would be read as the line's: VS Code gets the file without one, and
    /// Zed a column after the line, so the name's own `:12` stays in it.
    #[test]
    fn a_name_with_a_colon_keeps_it() {
        let file = ctx(Some("/work/my repo/notes:12"));
        let code = Cli {
            run: &["code"],
            goto: Goto::Flag,
        };
        assert_eq!(
            expand(&code.args(&file, true), &file),
            ["code", "/work/my repo", "/work/my repo/notes:12"]
        );
        let zed = Cli {
            run: &["zed"],
            goto: Goto::Suffix,
        };
        assert_eq!(
            expand(&zed.args(&file, false), &file),
            ["zed", "/work/my repo/notes:12:42:1"]
        );
        let plain = ctx(Some("/work/a:b/c.ts"));
        assert_eq!(
            expand(&zed.args(&plain, false), &plain),
            ["zed", "/work/a:b/c.ts:42"]
        );
        assert_eq!(
            expand(&code.args(&plain, false), &plain),
            ["code", "/work/a:b/c.ts"]
        );
        assert!(!ends_in_number("a:") && !ends_in_number("a") && ends_in_number("a:7"));
    }

    /// A custom command with no placeholder gets the project's folder, or the file a row names.
    #[test]
    fn a_custom_command_gets_the_project() {
        let dir = std::env::temp_dir().join(format!("gitviber-open-custom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        assert_eq!(open_custom(&dir, "a.txt", None, "test -d", true), Ok(()));
        assert!(open_custom(&dir, "a.txt", None, "test -d", false).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
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
            if let Some(cli) = a.cli {
                assert!(!cli.run.is_empty() && a.group == Editor, "{}", a.id);
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
        let png = icon("terminal").unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        // IHDR's width and height.
        assert_eq!(png[16..24], [0, 0, 0, 32, 0, 0, 0, 32]);
        assert_eq!(icon("terminal"), Some(png));
        assert_eq!(icon("not-an-app"), None);
    }

    #[test]
    fn stays_inside_the_repo() {
        let dir = std::env::temp_dir().join(format!("gitviber-open-in-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.txt"), "a").unwrap();
        assert_eq!(target(&dir, "", false).unwrap(), (dir.clone(), None));
        assert_eq!(target(&dir, "src", false).unwrap(), (dir.join("src"), None));
        assert_eq!(
            target(&dir, "src/a.txt", false).unwrap(),
            (dir.clone(), Some(dir.join("src/a.txt")))
        );
        assert!(target(&dir, "../etc", false).is_err());
        assert!(target(&dir, "/etc", false).is_err());
        assert!(target(&dir, "missing", false).is_err());
        // The status bar's project: the worktree, with the file while it's there.
        assert_eq!(target(&dir, "missing", true).unwrap(), (dir.clone(), None));
        assert_eq!(target(&dir, "src", true).unwrap(), (dir.clone(), None));
        assert!(target(&dir, "../etc", true).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reports_a_custom_command_that_fails() {
        let dir = std::env::temp_dir();
        assert_eq!(open_custom(&dir, "", None, "true {path}", false), Ok(()));
        assert!(open_custom(&dir, "", None, "false", false)
            .unwrap_err()
            .contains("false failed"));
        assert_eq!(
            open_custom(&dir, "", None, "gitviber-no-such-editor", false),
            Err("gitviber-no-such-editor: command not found".into())
        );
        assert!(open_custom(&dir, "", None, "  ", false).is_err());
    }
}
