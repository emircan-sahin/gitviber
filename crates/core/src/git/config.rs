//! The commit identity (user.name / user.email), global and per repository, and the settings a
//! refresh reads.

use super::{run, run_text, run_with, validate_one_line};
use serde::Serialize;
use std::path::Path;

/// Every setting git resolves in a repo, from one `git config --list -z`: a refresh reads
/// several, and each `git config --get` was a git process of its own. Keys as git prints them:
/// section and name lowercased, a subsection as written. None: a key with no `=` at all.
pub(crate) struct ConfigList(Vec<(String, Option<String>)>);

impl ConfigList {
    pub(crate) fn read(repo: &Path) -> Result<Self, String> {
        run(repo, &["config", "--list", "-z"]).map(|raw| Self::parse(&raw))
    }

    fn parse(raw: &[u8]) -> Self {
        // `key\nvalue\0`, or `key\0` for a key with no value.
        let entry = |e: &[u8]| {
            let e = String::from_utf8_lossy(e);
            match e.split_once('\n') {
                Some((k, v)) => (k.to_string(), Some(v.to_string())),
                None => (e.into_owned(), None),
            }
        };
        ConfigList(
            raw.split(|b| *b == 0)
                .filter(|e| !e.is_empty())
                .map(entry)
                .collect(),
        )
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_deref()))
    }

    /// As `config_value` reads it without a scope: the last one, trimmed, None when empty.
    pub(crate) fn value(&self, key: &str) -> Option<String> {
        let (_, value) = self.0.iter().rev().find(|(k, _)| k == key)?;
        Some(value.as_deref()?.trim().to_string()).filter(|v| !v.is_empty())
    }

    /// As git reads a boolean: a key with no `=` is true, an empty value false, a number true
    /// unless 0. None when unset or not a boolean.
    pub(crate) fn bool(&self, key: &str) -> Option<bool> {
        let (_, value) = self.0.iter().rev().find(|(k, _)| k == key)?;
        let Some(v) = value else {
            return Some(true);
        };
        match v.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" => Some(true),
            "false" | "no" | "off" | "" => Some(false),
            n => n.parse::<i64>().ok().map(|n| n != 0),
        }
    }

    /// `remote_urls` without a git call: each remote by name with the URL it fetches from,
    /// the longest `url.<base>.insteadOf` applied. None where git's own reading would differ
    /// (an empty URL resets the list only since git 2.46; a string key with no value makes git
    /// fail): `remote -v` answers then.
    pub(crate) fn remote_urls(&self) -> Option<Vec<(String, Option<String>)>> {
        let mut remotes: Vec<(&str, Option<&str>)> = vec![];
        // By base, in the order git meets them: the first base wins a tie.
        let mut rewrites: Vec<(&str, Vec<&str>)> = vec![];
        for (key, value) in &self.0 {
            let section = |s: &str| key.strip_prefix(s);
            if value.is_none()
                && ["remote.", "url.", "branch."]
                    .iter()
                    .any(|s| key.starts_with(s))
            {
                return None;
            }
            if let Some((name, var)) = section("remote.").and_then(|k| k.rsplit_once('.')) {
                let i = match remotes.iter().position(|(n, _)| *n == name) {
                    Some(i) => i,
                    None => {
                        remotes.push((name, None));
                        remotes.len() - 1
                    }
                };
                if var == "url" {
                    let url = value.as_deref().filter(|u| !u.is_empty())?;
                    remotes[i].1.get_or_insert(url);
                }
            } else if let Some(base) = section("url.").and_then(|k| k.strip_suffix(".insteadof")) {
                let prefix = value.as_deref().unwrap_or_default();
                match rewrites.iter_mut().find(|(b, _)| *b == base) {
                    Some((_, prefixes)) => prefixes.push(prefix),
                    None => rewrites.push((base, vec![prefix])),
                }
            }
        }
        let rewrite = |url: &str| {
            let mut longest: Option<(&str, &str)> = None;
            for (base, prefixes) in &rewrites {
                for prefix in prefixes {
                    if url.starts_with(prefix)
                        && longest.is_none_or(|(_, l)| l.len() < prefix.len())
                    {
                        longest = Some((base, prefix));
                    }
                }
            }
            match longest {
                Some((base, prefix)) => format!("{base}{}", &url[prefix.len()..]),
                None => url.to_string(),
            }
        };
        let mut list: Vec<(String, Option<String>)> = remotes
            .into_iter()
            .map(|(name, url)| (name.to_string(), url.map(rewrite)))
            .collect();
        // `remote -v` lists them by name.
        list.sort_by(|a, b| a.0.cmp(&b.0));
        Some(list)
    }
}

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
