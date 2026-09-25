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
