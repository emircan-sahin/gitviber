//! End-to-end git scenarios against real repositories and a local bare "remote".

mod branches;
mod changes;
mod commits;
mod content;
mod history;
mod journal;
mod lines;
mod operations;
mod pr_checkout;
mod rewrite;
mod search;
mod stash;
mod suggest;
mod sync;
mod watcher;
mod worktrees;

use crate::journal::{Action, Journal, Mode};
use crate::network::Net;
use crate::{fs as vfs, git::*};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gitviber-scn-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Sandbox(dir)
    }
    fn path(&self, p: &str) -> PathBuf {
        self.0.join(p)
    }
    /// A bare origin with one commit on main, plus `n` clones.
    fn remote_with_clones(&self, n: usize) -> Vec<PathBuf> {
        let origin = self.path("origin.git");
        run(
            &self.0,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        )
        .unwrap();
        let seed = self.clone_of("seed");
        write_commit(&seed, "a.txt", "one\ntwo\nthree\n", "base");
        run(&seed, &["push", "-q", "-u", "origin", "main"]).unwrap();
        (0..n).map(|i| self.clone_of(&format!("c{i}"))).collect()
    }
    fn clone_of(&self, name: &str) -> PathBuf {
        let dir = self.path(name);
        run(
            &self.0,
            &[
                "clone",
                "-q",
                self.path("origin.git").to_str().unwrap(),
                dir.to_str().unwrap(),
            ],
        )
        .unwrap();
        identity(&dir);
        dir
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn identity(repo: &Path) {
    for (k, v) in [
        ("user.name", "T"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
        ("pull.rebase", "false"),
    ] {
        run(repo, &["config", k, v]).unwrap();
    }
}

fn init(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    run(dir, &["init", "-q", "-b", "main"]).unwrap();
    identity(dir);
}

fn write_commit(repo: &Path, path: &str, content: &str, msg: &str) {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(repo.join(parent)).unwrap();
    }
    fs::write(repo.join(path), content).unwrap();
    stage(repo, &[path.into()]).unwrap();
    commit(repo, msg, &CommitOptions::default()).unwrap();
}

const AMEND: CommitOptions = CommitOptions {
    amend: true,
    sign_off: false,
    no_verify: false,
    co_authors: Vec::new(),
};

/// A repo with an agent-style worktree inside it (.claude/worktrees/agent), a detached one
/// next to it, and one whose folder was deleted (prunable).
fn repo_with_worktrees(sb: &Sandbox) -> PathBuf {
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let add = |args: &[&str]| {
        let mut all = vec!["worktree", "add", "-q"];
        all.extend(args);
        run(&r, &all).unwrap();
    };
    add(&["-b", "agent", ".claude/worktrees/agent"]);
    add(&["--detach", sb.path("det").to_str().unwrap()]);
    add(&["-b", "gone", sb.path("gone").to_str().unwrap()]);
    fs::remove_dir_all(sb.path("gone")).unwrap();
    r
}

fn git_url(repo: &Path) -> String {
    git_url_of(repo, "origin")
}

fn git_url_of(repo: &Path, remote: &str) -> String {
    let out = run(repo, &["remote", "get-url", remote]).unwrap();
    String::from_utf8_lossy(&out).trim().to_string()
}

fn same_dir(a: &str, b: &Path) -> bool {
    Path::new(a)
        .canonicalize()
        .is_ok_and(|a| a == b.canonicalize().unwrap())
}

/// `r` with a submodule at `sub` (its `.git` is a file pointing into .git/modules/).
fn repo_with_submodule(sb: &Sandbox) -> PathBuf {
    let lib = sb.path("lib");
    init(&lib);
    write_commit(&lib, "l.txt", "l\n", "lib");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let url = lib.to_str().unwrap();
    let add = [
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        url,
        "sub",
    ];
    run(&r, &add).unwrap();
    // A clone made by git, not `clone_of`: without its own identity a commit in it
    // fails where the machine has none (the Linux CI runner).
    identity(&r.join("sub"));
    commit(&r, "add sub", &CommitOptions::default()).unwrap();
    r
}

fn rev(repo: &Path, spec: &str) -> String {
    run_text(repo, &["rev-parse", spec])
        .unwrap()
        .trim()
        .to_string()
}

fn exists(repo: &Path, branch: &str) -> bool {
    let full = format!("refs/heads/{branch}");
    run(repo, &["rev-parse", "--verify", "-q", &full]).is_ok()
}

fn on_branch(repo: &Path) -> String {
    run_text(repo, &["symbolic-ref", "--short", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

/// Undo (`forward` false) or redo the next entry.
fn step(j: &Journal, repo: &Path, forward: bool) -> Result<(), String> {
    j.step(repo, forward, None, &Mutex::new(())).map(|_| ())
}
