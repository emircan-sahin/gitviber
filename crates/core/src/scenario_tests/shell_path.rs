//! `gitviber` in a pane with nothing installed: the integration puts GitViber's command folder
//! (GITVIBER_BIN_DIR, pty.rs) last on PATH after the user's rc files, from a real `zsh -i` and
//! bash 4.4+, with a made-up HOME so no one's own files run.

use super::*;
use std::io::Write;
use std::process::{Command, Stdio};

/// The folder the command would be found in, and PATH, once `shell` has read `rc` (in HOME as
/// `rc_name`) and shows its first prompt; None where that shell or its integration isn't there.
fn after_startup(
    name: &str,
    shell: &str,
    rc_name: &str,
    rc: &str,
    path: &str,
) -> Option<(String, String)> {
    let out = session(
        name,
        shell,
        (rc_name, rc),
        path,
        "bin",
        "echo \"@$(command -v gitviber)@$PATH@\"",
    )?;
    let mut parts = out.split('@').skip(1);
    let found = parts.next().expect("the shell's answer").to_string();
    let path = parts.next().expect("its PATH").to_string();
    Some((found, path))
}

/// What `shell` prints running `lines` after its startup, with the app's command folder at
/// `bin` in the sandbox; the sandbox's path reads `$SANDBOX`.
fn session(
    name: &str,
    shell: &str,
    (rc_name, rc): (&str, &str),
    path: &str,
    bin: &str,
    lines: &str,
) -> Option<String> {
    let shell = Path::new(shell);
    if !shell.exists() {
        return None;
    }
    let sb = Sandbox::new(name);
    let home = sb.path("home");
    let bin = sb.path(bin);
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(home.join(rc_name), rc.replace("$SANDBOX", sb.0.to_str()?)).unwrap();
    for dir in [&bin, &sb.path("own")] {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("gitviber"), "#!/bin/sh\n").unwrap();
        fs::set_permissions(
            dir.join("gitviber"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
    }
    let inject = crate::shell_integration::injection(shell, &sb.path("si"))?;
    let mut cmd = match inject.args.split_first() {
        Some((program, args)) => {
            let mut c = Command::new(program);
            c.args(args);
            c
        }
        None => Command::new(shell),
    };
    let mut child = cmd
        .arg("-i")
        .current_dir(&home)
        .env_clear()
        .env("PATH", path.replace("$SANDBOX", sb.0.to_str()?))
        .env("HOME", &home)
        .env("GITVIBER_BIN_DIR", &bin)
        .envs(inject.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{lines}\nexit\n").as_bytes())
        .unwrap();
    let out = String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).into_owned();
    Some(out.replace(sb.0.to_str()?, "$SANDBOX"))
}

/// zsh, and the bash 4.4+ the app loads its integration into (not macOS's own 3.2).
fn shells() -> Vec<(&'static str, &'static str)> {
    let bash = [
        "/opt/homebrew/bin/bash",
        "/usr/local/bin/bash",
        "/usr/bin/bash",
        "/bin/bash",
    ]
    .into_iter()
    .find(|p| Path::new(p).exists());
    let mut shells = vec![("/bin/zsh", ".zshrc"), ("/usr/bin/zsh", ".zshrc")];
    shells.extend(bash.map(|b| (b, ".bash_profile")));
    shells
}

#[test]
fn an_rc_that_sets_path_afresh_still_finds_the_command() {
    for (shell, rc) in shells() {
        let Some((found, path)) = after_startup(
            "path-reset",
            shell,
            rc,
            "PATH=/usr/bin:/bin",
            "/usr/bin:/bin:$SANDBOX/bin",
        ) else {
            continue;
        };
        assert_eq!(found, "$SANDBOX/bin/gitviber", "{shell}");
        assert_eq!(path, "/usr/bin:/bin:$SANDBOX/bin", "{shell}");
    }
}

/// Last on PATH, and once: a `gitviber` of the user's own comes first.
#[test]
fn the_users_own_command_comes_first() {
    for (shell, rc) in shells() {
        let Some((found, path)) = after_startup(
            "path-own",
            shell,
            rc,
            "PATH=$SANDBOX/own:$PATH",
            "/usr/bin:/bin:$SANDBOX/bin",
        ) else {
            continue;
        };
        assert_eq!(found, "$SANDBOX/own/gitviber", "{shell}");
        // What's around is the system's: a Debian /etc/profile sets its own for bash, and snapd's
        // profile.d appends /snap/bin after ours.
        assert!(path.starts_with("$SANDBOX/own:"), "{shell}: {path}");
        assert!(path.contains(":$SANDBOX/bin"), "{shell}: {path}");
        assert_eq!(path.matches("$SANDBOX/bin").count(), 1, "{shell}: {path}");
    }
}

/// An app kept in a folder with a space and glob characters in its name: one entry, found.
#[test]
fn an_app_path_with_spaces_and_brackets_goes_on_path_once() {
    let bin = "My Apps [1]/bin";
    let echo = "echo \"@$(command -v gitviber)@$PATH@\"";
    for (shell, rc) in shells() {
        for (rc_text, kept) in [("PATH=/usr/bin:/bin", false), ("", true)] {
            let start = "/usr/bin:/bin:$SANDBOX/My Apps [1]/bin";
            let Some(out) = session("path-space", shell, (rc, rc_text), start, bin, echo) else {
                continue;
            };
            let mut parts = out.split('@').skip(1);
            let found = parts.next().expect("the shell's answer");
            let path = parts.next().expect("its PATH");
            assert_eq!(
                found, "$SANDBOX/My Apps [1]/bin/gitviber",
                "{shell} kept={kept}"
            );
            // Not necessarily last: snapd's profile.d appends /snap/bin after it.
            assert!(
                path.contains(":$SANDBOX/My Apps [1]/bin"),
                "{shell}: {path}"
            );
            assert_eq!(path.matches("My Apps").count(), 1, "{shell}: {path}");
        }
    }
}

/// zsh's hook runs at the first prompt only, before the user's own precmd hooks, which still run
/// and still see the last command's status and `$_`.
#[test]
fn the_zsh_hook_runs_once_and_leaves_the_users_hooks_alone() {
    let rc = "mine() { print -r -- \"mine:$?:${PATH##*:}\"; }\n\
              precmd_functions+=(mine)\nPATH=/usr/bin:/bin\nfalse lastarg";
    let lines = "print -r -- \"under:$_:${precmd_functions[(I)_gitviber_path]}\"\nPATH=/usr/bin";
    for shell in ["/bin/zsh", "/usr/bin/zsh"] {
        let start = "/usr/bin:/bin:$SANDBOX/bin";
        let Some(out) = session("path-hook", shell, (".zshrc", rc), start, "bin", lines) else {
            continue;
        };
        assert!(out.contains("mine:1:$SANDBOX/bin"), "{shell}: {out}");
        // Gone from precmd_functions once run.
        assert!(out.contains("under:lastarg:0"), "{shell}: {out}");
        // A PATH set afresh at the prompt is the user's choice: not put back.
        assert!(out.contains("mine:0:/usr/bin"), "{shell}: {out}");
    }
}
