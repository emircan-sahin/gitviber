//! Shell integration: zsh and bash mark each prompt and command with OSC 133, for the terminal's
//! command marks and jumps. Loaded the way Ghostty loads its own, without a line in the user's
//! dotfiles: zsh through ZDOTDIR, bash 4.4+ in POSIX mode through ENV. fish 4 marks its own prompts.

use std::ffi::OsString;
use std::io::Write;
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
    // macOS's own bash is 3.2, without PS0; Ghostty leaves it alone too.
    let old_bash = cfg!(target_os = "macos") && shell == Path::new("/bin/bash");
    if name != "zsh" && (name != "bash" || old_bash) {
        return None;
    }
    write(dir).ok()?;
    if name == "zsh" {
        return Some(Injection {
            args: vec![],
            env: vec![("ZDOTDIR".into(), dir.join("zsh").into())],
        });
    }
    // POSIX mode reads ENV instead of the startup files; the script reads those itself.
    let mut env = vec![("ENV".into(), dir.join("bash/gitviber.bash").into())];
    // Else POSIX mode would keep its history in ~/.sh_history.
    if let Some(home) = std::env::var_os("HOME") {
        env.push((
            "HISTFILE".into(),
            Path::new(&home).join(".bash_history").into(),
        ));
    }
    Some(Injection {
        args: vec![shell.into(), "--login".into(), "--posix".into()],
        env,
    })
}

/// Written at each spawn where they differ: an update brings new ones, and a cleared cache gets
/// them back. The folders are the user's alone, and a file is replaced whole, never through a link.
fn write(dir: &Path) -> std::io::Result<()> {
    for (name, text) in FILES {
        let path = dir.join(name);
        let parent = path.parent().unwrap_or(dir);
        let mut folders = std::fs::DirBuilder::new();
        folders.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut folders, 0o700);
        folders.create(parent)?;
        for folder in [dir, parent] {
            if !std::fs::symlink_metadata(folder)?.is_dir() {
                return Err(std::io::ErrorKind::InvalidInput.into());
            }
            // Made by an older version, before the mode was set.
            #[cfg(unix)]
            std::fs::set_permissions(folder, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
        }
        let current = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file())
            && std::fs::read(&path).is_ok_and(|had| had == text.as_bytes());
        if current {
            continue;
        }
        let temp = parent.join(format!(
            ".{}.{}",
            std::process::id(),
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let _ = std::fs::remove_file(&temp);
        let mut file = std::fs::OpenOptions::new();
        file.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o600);
        file.open(&temp)?.write_all(text.as_bytes())?;
        // A rename replaces what's at `path`, a link included, rather than writing where it points.
        std::fs::rename(&temp, &path)?;
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
        assert_eq!(bash.args[2], "--posix");
        assert_eq!(
            bash.env[0].1,
            dir.join("bash/gitviber.bash").into_os_string()
        );
        if cfg!(target_os = "macos") {
            assert!(injection(Path::new("/bin/bash"), &dir).is_none());
        }
        assert!(injection(Path::new("/opt/homebrew/bin/fish"), &dir).is_none());
        // A link planted in place of a script is replaced, not written through.
        #[cfg(unix)]
        {
            let outside = dir.join("outside");
            std::fs::write(&outside, "mine").unwrap();
            let script = dir.join("zsh/gitviber.zsh");
            std::fs::remove_file(&script).unwrap();
            std::os::unix::fs::symlink(&outside, &script).unwrap();
            injection(Path::new("/bin/zsh"), &dir).unwrap();
            assert_eq!(std::fs::read_to_string(&outside).unwrap(), "mine");
            assert!(std::fs::symlink_metadata(&script).unwrap().is_file());
        }
        assert!(injection(Path::new("/bin/sh"), &dir).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
