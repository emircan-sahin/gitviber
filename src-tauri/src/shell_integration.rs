//! Shell integration: zsh and bash mark each prompt and command with OSC 133, for the terminal's
//! command marks and jumps. Loaded the way VS Code and Ghostty load theirs, without a line in the
//! user's dotfiles: zsh through ZDOTDIR, bash through --init-file. fish 4 marks its own prompts.

use std::ffi::OsString;
use std::path::Path;

const FILES: [(&str, &str); 3] = [
    ("zsh/.zshenv", include_str!("shell_integration/zshenv")),
    (
        "zsh/gitviber.zsh",
        include_str!("shell_integration/gitviber.zsh"),
    ),
    (
        "bash/gitviber.bash",
        include_str!("shell_integration/gitviber.bash"),
    ),
];

/// How to start a shell with the scripts loaded: its arguments (none for the default login
/// shell) and the variables that load them.
pub struct Injection {
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

/// For `shell`, with the scripts kept in `dir`; None for any other shell, or if they can't be written there.
pub fn injection(shell: &Path, dir: &Path) -> Option<Injection> {
    let name = shell.file_name()?.to_str()?;
    if name != "zsh" && name != "bash" {
        return None;
    }
    write(dir).ok()?;
    Some(if name == "zsh" {
        Injection {
            args: vec![],
            env: vec![("ZDOTDIR".into(), dir.join("zsh").into())],
        }
    } else {
        Injection {
            args: vec![
                shell.into(),
                "--init-file".into(),
                dir.join("bash/gitviber.bash").into(),
            ],
            env: vec![],
        }
    })
}

/// Written at each spawn where they differ: an update brings new ones, and a cleared cache gets them back.
fn write(dir: &Path) -> std::io::Result<()> {
    for (name, text) in FILES {
        let path = dir.join(name);
        if std::fs::read(&path).is_ok_and(|had| had == text.as_bytes()) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::injection;
    use std::path::Path;

    #[test]
    fn zsh_and_bash_get_their_scripts_and_other_shells_nothing() {
        let dir = std::env::temp_dir().join(format!("gitviber-si-{}", std::process::id()));
        let zsh = injection(Path::new("/bin/zsh"), &dir).unwrap();
        assert!(zsh.args.is_empty());
        assert_eq!(zsh.env[0].1, dir.join("zsh").into_os_string());
        assert!(dir.join("zsh/.zshenv").is_file());
        let bash = injection(Path::new("/usr/local/bin/bash"), &dir).unwrap();
        assert_eq!(
            bash.args[2],
            dir.join("bash/gitviber.bash").into_os_string()
        );
        assert!(injection(Path::new("/opt/homebrew/bin/fish"), &dir).is_none());
        assert!(injection(Path::new("/bin/sh"), &dir).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
