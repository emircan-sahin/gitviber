//! "Open in" with the user's own command: what the program it runs is handed, for the paths
//! real worktrees hold. A recorder stands in for the editor, so nothing is opened.

use super::*;
use crate::open_in::open_custom;

/// A worktree whose own folder needs quoting, with a file in nested folders whose name could
/// pass for a flag, a placeholder and a shell expansion.
const ODD: &str = "src/deep er/ü/--foo 'q' \"d\" $HOME {line}.ts";

struct Recorder {
    out: PathBuf,
}

impl Recorder {
    fn new(sb: &Sandbox) -> Self {
        Recorder {
            out: sb.path("argv"),
        }
    }
    /// `sh -c` writing each argument it gets NUL-terminated, then `template`.
    fn command(&self, template: &str) -> String {
        format!(
            "/bin/sh -c 'printf \"%s\\0\" \"$@\" > \"$0\"' {} {template}",
            self.out.display()
        )
    }
    fn argv(&self) -> Vec<String> {
        let raw = fs::read(&self.out).unwrap();
        let _ = fs::remove_file(&self.out);
        raw.split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8(a.to_vec()).unwrap())
            .collect()
    }
}

fn worktree(sb: &Sandbox) -> PathBuf {
    let r = sb.path("my repo ü $x");
    init(&r);
    write_commit(&r, ODD, "x\n", "odd");
    write_commit(&r, "top.txt", "x\n", "top");
    r
}

#[test]
fn a_file_with_an_odd_name_reaches_the_command_whole_and_absolute() {
    let sb = Sandbox::new("openin-odd");
    let r = worktree(&sb);
    let rec = Recorder::new(&sb);
    let file = r.join(ODD).to_string_lossy().into_owned();

    // A row's menu: the file at its line, as one argument.
    open_custom(&r, ODD, Some(12), &rec.command("-g {file}:{line}"), false).unwrap();
    assert_eq!(rec.argv(), ["-g".to_string(), format!("{file}:12")]);

    // No placeholder: the file goes last. Absolute, so a name starting with `-` is never a flag.
    open_custom(&r, ODD, None, &rec.command(""), false).unwrap();
    let argv = rec.argv();
    assert_eq!(argv, std::slice::from_ref(&file));
    assert!(argv[0].starts_with('/'));

    // No line known: line 1.
    open_custom(&r, ODD, None, &rec.command("{file}:{line}"), false).unwrap();
    assert_eq!(rec.argv(), [format!("{file}:1")]);
}

#[test]
fn the_status_bar_hands_a_custom_command_the_worktree() {
    let sb = Sandbox::new("openin-project");
    let r = worktree(&sb);
    let rec = Recorder::new(&sb);
    let root = r.to_string_lossy().into_owned();
    let file = r.join(ODD).to_string_lossy().into_owned();

    // No placeholder: the worktree, not the focused file's folder.
    open_custom(&r, ODD, Some(3), &rec.command(""), true).unwrap();
    assert_eq!(rec.argv(), std::slice::from_ref(&root));
    // {path} is the worktree too; {file} still the focused file.
    open_custom(&r, ODD, Some(3), &rec.command("{path} {file}:{line}"), true).unwrap();
    assert_eq!(rec.argv(), [root.clone(), format!("{file}:3")]);
    // Nothing focused: {file} stands for the worktree.
    open_custom(&r, "", None, &rec.command("-g {file}:{line}"), true).unwrap();
    assert_eq!(rec.argv(), ["-g".to_string(), root.clone()]);
}

/// The focused file was deleted (by an agent, a checkout) while its tab stayed open: the status
/// bar's Open in still has the worktree to open.
#[test]
fn the_worktree_opens_when_the_focused_file_is_gone() {
    let sb = Sandbox::new("openin-gone");
    let r = worktree(&sb);
    let rec = Recorder::new(&sb);
    fs::remove_file(r.join("top.txt")).unwrap();
    let opened = open_custom(&r, "top.txt", Some(5), &rec.command(""), true);
    assert_eq!(opened, Ok(()), "the worktree should open without the file");
    assert_eq!(rec.argv(), [r.to_string_lossy().into_owned()]);
}

/// A row for a file that was deleted meanwhile says so, and runs nothing.
#[test]
fn a_deleted_file_from_a_row_is_refused() {
    let sb = Sandbox::new("openin-row-gone");
    let r = worktree(&sb);
    let rec = Recorder::new(&sb);
    fs::remove_file(r.join("top.txt")).unwrap();
    let err = open_custom(&r, "top.txt", None, &rec.command(""), false).unwrap_err();
    assert!(err.contains("not on disk"), "{err}");
    assert!(!rec.out.exists());
}

/// Paths outside the worktree, through `..` or a symlink, never reach the command.
#[test]
fn nothing_outside_the_worktree_is_opened() {
    let sb = Sandbox::new("openin-outside");
    let r = worktree(&sb);
    let rec = Recorder::new(&sb);
    fs::write(sb.path("secret.txt"), "s").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(sb.path("secret.txt"), r.join("link.txt")).unwrap();
    for project in [false, true] {
        for rel in ["../secret.txt", "/etc/hosts", "link.txt", ".git/config"] {
            let res = open_custom(&r, rel, None, &rec.command(""), project);
            assert!(res.is_err(), "{rel} (project {project}) opened: {res:?}");
            assert!(!rec.out.exists(), "{rel} ran the command");
        }
    }
}

/// A folder from the explorer opens as itself; `{file}` stands for it.
#[test]
fn a_nested_folder_opens_as_itself() {
    let sb = Sandbox::new("openin-dir");
    let r = worktree(&sb);
    let rec = Recorder::new(&sb);
    let dir = r.join("src/deep er").to_string_lossy().into_owned();
    open_custom(
        &r,
        "src/deep er",
        None,
        &rec.command("{path} {file}"),
        false,
    )
    .unwrap();
    assert_eq!(rec.argv(), [dir.clone(), dir]);
}

#[test]
fn a_broken_template_runs_nothing() {
    let sb = Sandbox::new("openin-broken");
    let r = worktree(&sb);
    for command in ["", "   ", "code 'unclosed", "code \\"] {
        for project in [false, true] {
            assert!(
                open_custom(&r, "top.txt", None, command, project).is_err(),
                "{command:?}"
            );
        }
    }
}

/// The menu draws every app's icon at once: concurrent asks get the same picture, drawn once.
#[cfg(target_os = "macos")]
#[test]
fn icons_drawn_at_once_agree() {
    use crate::open_in::icon;
    let all: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(|| icon("terminal")))
        .collect();
    let all: Vec<_> = all.into_iter().map(|t| t.join().unwrap()).collect();
    let first = all[0].clone().expect("Terminal's icon");
    assert!(first.starts_with(b"\x89PNG"));
    assert!(all.iter().all(|p| p.as_ref() == Some(&first)));
    // Not one of ours, or not installed: none, and asking again doesn't change that.
    for _ in 0..2 {
        assert_eq!(icon("sourcetree-not-an-id"), None);
        assert_eq!(icon(""), None);
    }
}
