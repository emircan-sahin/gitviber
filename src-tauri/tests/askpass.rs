//! The askpass helper end to end: git and ssh run the real app binary, which asks the server in
//! this test as it would the running app. That binary must answer and exit before the app starts.
#![cfg(unix)]

use gitviber_lib::askpass::{self, Event};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Once;

const HELPER: &str = env!("CARGO_BIN_EXE_gitviber");

/// The page, as a script: what it types for each prompt (None is Cancel).
fn respond(text: &str) -> Option<String> {
    let answer = if text == "Username for 'https://example.com': " {
        "alice"
    } else if text == "Password for 'https://alice@example.com': " {
        "s3cret"
    } else if text.ends_with("(yes/no/[fingerprint])?") && text.contains("SHA256:abc") {
        "yes"
    } else if text == "Enter passphrase for key '/keys/good':" {
        "hunter2"
    } else {
        return None;
    };
    Some(answer.into())
}

fn serve() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        askpass::start(PathBuf::from(HELPER), |event| {
            if let Event::Ask(p) = event {
                std::thread::spawn(move || askpass::answer(p.id, respond(&p.text)));
            }
            true
        })
    });
}

/// git with no config of the user's (no credential helper to answer first) and no terminal.
fn git(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn run_asking(mut cmd: Command, input: &str) -> Output {
    serve();
    let _asking = askpass::attach(&mut cmd, "git test", None).expect("server running");
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitviber-askpass-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// git asks for a username, then a password, each through the helper, as for an HTTPS remote.
#[test]
fn git_asks_for_a_login_in_two_prompts() {
    let dir = sandbox("login");
    let cmd = git(&dir, &["credential", "fill"]);
    let out = run_asking(cmd, "protocol=https\nhost=example.com\n\n");
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("username=alice\n"), "{stdout}");
    assert!(stdout.contains("password=s3cret\n"), "{stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Cancel fails the command with git's own message, which the page explains (gitErrors.ts).
#[test]
fn a_cancelled_prompt_fails_the_command() {
    let dir = sandbox("cancel");
    let cmd = git(&dir, &["credential", "fill"]);
    let out = run_asking(cmd, "protocol=https\nhost=elsewhere.example\n\n");
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("fatal: could not read Username for 'https://elsewhere.example'"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What ssh asks on a first connection with a locked key: the host key, then the passphrase.
/// The fake ssh asks through SSH_ASKPASS as OpenSSH does, then serves the repo itself.
fn ls_remote_over_ssh(name: &str, key: &str) -> Output {
    let dir = sandbox(name);
    let bare = dir.join("origin.git");
    let init = git(&dir, &["init", "-q", "--bare", bare.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(init.status.success());
    let ssh = dir.join("ssh.sh");
    std::fs::write(
        &ssh,
        format!(
            r#"[ "$SSH_ASKPASS_REQUIRE" = force ] || exit 2
host="The authenticity of host 'git.example (192.0.2.1)' can't be established.
ED25519 key fingerprint is SHA256:abc.
Are you sure you want to continue connecting (yes/no/[fingerprint])?"
[ "$("$SSH_ASKPASS" "$host")" = yes ] || {{ echo "Host key verification failed." >&2; exit 255; }}
[ "$("$SSH_ASKPASS" "Enter passphrase for key '{key}':")" = hunter2 ] ||
  {{ echo "git@git.example: Permission denied (publickey)." >&2; exit 255; }}
exec git upload-pack '{}'
"#,
            bare.display()
        ),
    )
    .unwrap();
    let mut cmd = git(&dir, &["ls-remote", "ssh://git@git.example/origin.git"]);
    cmd.env("GIT_SSH_COMMAND", format!("sh '{}'", ssh.display()))
        .env("GIT_SSH_VARIANT", "simple");
    let out = run_asking(cmd, "");
    let _ = std::fs::remove_dir_all(&dir);
    out
}

#[test]
fn ssh_asks_about_the_host_and_the_key() {
    let out = ls_remote_over_ssh("ssh", "/keys/good");
    assert!(out.status.success(), "{}", text(&out.stderr));
}

#[test]
fn a_refused_passphrase_fails_like_ssh() {
    let out = ls_remote_over_ssh("ssh-refused", "/keys/other");
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("Permission denied (publickey)"));
}

/// With no app to ask (it quit, or the socket is gone), the helper says no and exits: it never
/// starts the app. No time limit here: macOS scans a freshly built binary on its first run.
#[test]
fn the_helper_alone_declines() {
    let out = Command::new(HELPER)
        .arg("Password for 'https://example.com': ")
        .env("GITVIBER_ASKPASS_SOCKET", "/nonexistent/askpass")
        .env("GITVIBER_ASKPASS_TOKEN", "x")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

/// An askpass of the user's own (core.askPass here) keeps answering git; ssh still asks the app.
#[test]
fn leaves_the_users_own_askpass_alone() {
    serve();
    let dir = sandbox("own");
    assert!(git(&dir, &["init", "-q"]).status().unwrap().success());
    let set = git(
        &dir,
        &["config", "core.askPass", "/usr/local/bin/my-askpass"],
    );
    assert!({ set }.status().unwrap().success());
    let mut cmd = Command::new("git");
    cmd.current_dir(&dir);
    let _asking = askpass::attach(&mut cmd, "git fetch", None).unwrap();
    let env = |k: &str| cmd.get_envs().any(|(key, v)| key == k && v.is_some());
    assert!(!env("GIT_ASKPASS"));
    assert!(env("SSH_ASKPASS"));
    let _ = std::fs::remove_dir_all(&dir);
}
