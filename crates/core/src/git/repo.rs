//! Cloning and initialising repositories.

use super::{run, run_network, toplevel};
use crate::network::Net;
use std::path::Path;

/// Clones `url` into `parent/name` and returns that path. Never into a folder that already
/// holds something: git would refuse too, but only after the user waited for the network.
pub fn clone(parent: &Path, url: &str, name: &str, net: &Net) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("Enter a repository URL.".into());
    }
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(format!("invalid folder name: {name}"));
    }
    if !parent.is_dir() {
        return Err(format!("folder not found: {}", parent.display()));
    }
    let target = parent.join(name);
    let empty = std::fs::read_dir(&target).is_ok_and(|mut d| d.next().is_none());
    if target.exists() && !empty {
        return Err(format!(
            "{} already exists and isn't empty. Choose another folder name.",
            target.display()
        ));
    }
    let dest = target.to_string_lossy();
    run_network(parent, &["clone", "--", url, &dest], net)?;
    Ok(dest.into_owned())
}

/// Makes `dir` a new repository. Its first branch is the user's init.defaultBranch, else main.
pub fn init(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("folder not found: {}", dir.display()));
    }
    if toplevel(dir).is_ok() {
        return Err(format!("{} is already in a git repository", dir.display()));
    }
    let configured = run(dir, &["config", "--get", "init.defaultBranch"]).is_ok();
    let args: &[&str] = if configured {
        &["init", "-q"]
    } else {
        &["init", "-q", "-b", "main"]
    };
    run(dir, args).map(|_| ())
}

/// How git starts refusing a repository another user owns: a disk from another Mac, a folder
/// made with sudo.
const DUBIOUS: &str = "fatal: detected dubious ownership in repository at '";

/// The folder git's refusal names, which is what safe.directory has to list.
fn refused_folder(error: &str) -> Option<&str> {
    error
        .lines()
        .find_map(|l| l.strip_prefix(DUBIOUS)?.strip_suffix('\''))
}

/// Adds the repository git refuses to open at `path` to the global safe.directory, as git's own
/// error says to: only while git refuses it, and exactly the folder it names.
pub fn trust_folder(path: &Path) -> Result<(), String> {
    let Err(refused) = toplevel(path) else {
        return Ok(());
    };
    let folder = refused_folder(&refused).ok_or(refused.clone())?;
    // Not run inside the folder: git would refuse it there too.
    run(
        Path::new("/"),
        &["config", "--global", "--add", "safe.directory", folder],
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusts_the_folder_git_names_and_nothing_else() {
        let refused = "fatal: detected dubious ownership in repository at '/Volumes/Disk/my app'
To add an exception for this directory, call:

	git config --global --add safe.directory '/Volumes/Disk/my app'";
        assert_eq!(refused_folder(refused), Some("/Volumes/Disk/my app"));
        assert_eq!(
            refused_folder("fatal: not a git repository (or any of the parent directories): .git"),
            None
        );
        // A folder git doesn't refuse is left alone; a missing one isn't trusted.
        let dir = std::env::temp_dir().join(format!("gitviber-trust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q"]).unwrap();
        trust_folder(&dir).unwrap();
        assert!(trust_folder(&dir.join("gone")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
