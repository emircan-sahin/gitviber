//! The commit identity (user.name / user.email), global and per repository.

use super::{run, run_text, run_with, validate_one_line};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone, Default)]
pub struct Identity {
    pub name: Option<String>,
    pub email: Option<String>,
}

/// A config value, trimmed; None when unset or empty. `scope`: `--local` or `--global` to
/// read one file, else git's resolved value.
pub fn config_value(repo: &Path, scope: Option<&str>, key: &str) -> Option<String> {
    let mut args = vec!["config"];
    args.extend(scope);
    args.extend(["--get", key]);
    run_text(repo, &args)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn identity_in(repo: &Path, scope: Option<&str>) -> Identity {
    Identity {
        name: config_value(repo, scope, "user.name"),
        email: config_value(repo, scope, "user.email"),
    }
}

/// Who commits here, as git resolves it in this repo (so `includeIf` sections apply).
pub fn identity(repo: &Path) -> Identity {
    identity_in(repo, None)
}

/// Sets the given parts of the user's global identity (~/.gitconfig), never the repo's.
pub fn set_global_identity(
    repo: &Path,
    name: Option<&str>,
    email: Option<&str>,
) -> Result<(), String> {
    for (key, value) in [("user.name", name), ("user.email", email)] {
        let Some(value) = value.map(str::trim) else {
            continue;
        };
        validate_one_line(key, value)?;
        run(repo, &["config", "--global", key, value])?;
    }
    Ok(())
}

/// The identity this repository sets itself (its .git/config), whatever the global one is.
pub fn repo_identity(repo: &Path) -> Identity {
    identity_in(repo, Some("--local"))
}

/// The identity in the user's global config, which a repository without its own uses.
pub fn global_identity(repo: &Path) -> Identity {
    identity_in(repo, Some("--global"))
}

/// Gives this repository its own identity, or with None takes it away (the global one applies).
pub fn set_repo_identity(repo: &Path, identity: Option<(&str, &str)>) -> Result<(), String> {
    let Some((name, email)) = identity else {
        for key in ["user.name", "user.email"] {
            // 5: it wasn't set.
            run_with(repo, &["config", "--local", "--unset", key], &[5], None)?;
        }
        return Ok(());
    };
    for (key, value) in [("user.name", name.trim()), ("user.email", email.trim())] {
        validate_one_line(key, value)?;
        run(repo, &["config", "--local", key, value])?;
    }
    Ok(())
}
