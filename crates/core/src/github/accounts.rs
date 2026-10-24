//! The gh accounts on github.com, and which one the open repository's GitHub calls use. Since
//! gh 2.40 one host can hold several; the token stays in gh, asked for by account
//! (`gh auth token --user`). The pick per project is the app's, kept with its other settings.

use super::Session;
use crate::process;
use serde::Serialize;
use serde_json::Value;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GhAccount {
    pub login: String,
    /// gh's active account, which `gh` and an unpicked repository use.
    pub active: bool,
    /// False: gh holds a token for it that GitHub refused (expired, revoked).
    pub ok: bool,
}

/// gh's accounts on github.com. None of them, or no gh (or one older than multi-account
/// support, whose output names no "account"): an empty list, as there's nothing to pick.
pub fn accounts() -> Vec<GhAccount> {
    let status = |json: bool| {
        let mut cmd = Command::new("gh");
        cmd.args(["auth", "status", "--hostname", "github.com"]);
        if json {
            cmd.args(["--json", "hosts"]);
        }
        cmd.env("PATH", process::search_path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // It checks every account's token with GitHub, one request each.
        process::exec(
            cmd,
            "gh auth status",
            &[],
            None,
            Some(Duration::from_secs(20)),
        )
        .map(|out| String::from_utf8_lossy(&out).into_owned())
    };
    // --json came after multi-account support (gh 2.40); before it, the text says the same. That
    // text goes to stderr, with exit code 1, when any account's token is refused.
    match status(true).ok().and_then(|out| from_json(&out)) {
        Some(list) => list,
        None => from_text(&status(false).unwrap_or_else(|e| e)),
    }
}

fn from_json(out: &str) -> Option<Vec<GhAccount>> {
    let v: Value = serde_json::from_str(out).ok()?;
    let entries = v["hosts"]["github.com"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Some(
        entries
            .iter()
            .filter_map(|e| {
                Some(GhAccount {
                    login: e["login"].as_str().filter(|l| !l.is_empty())?.to_string(),
                    active: e["active"].as_bool().unwrap_or(false),
                    ok: e["state"].as_str() == Some("success"),
                })
            })
            .collect(),
    )
}

/// gh 2.40's `auth status`: per account a "Logged in to github.com account <login> (<where>)"
/// line, or "Failed to log in" / "Timeout trying to log in", then "- Active account: true|false".
fn from_text(out: &str) -> Vec<GhAccount> {
    let mut list: Vec<GhAccount> = vec![];
    for line in out.lines().map(str::trim) {
        if let Some(rest) = line.split_once(" to github.com account ").map(|(_, r)| r) {
            let Some(login) = rest.split_whitespace().next() else {
                continue;
            };
            list.push(GhAccount {
                login: login.to_string(),
                active: false,
                ok: line.contains("Logged in to"),
            });
        } else if let Some(active) = line.strip_prefix("- Active account: ") {
            if let Some(last) = list.last_mut() {
                last.active = active.trim() == "true";
            }
        }
    }
    list
}

/// A login as GitHub allows it: letters, digits and hyphens, not first, at most 39 characters;
/// a managed user's adds `_` and its enterprise's shortcode (`mona-cat_acme`).
fn valid_login(login: &str) -> bool {
    login.starts_with(|c: char| c.is_ascii_alphanumeric())
        && login.len() <= 39 + 1 + 39
        && login
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The account the open repository's GitHub calls use from now on; None: gh's active one.
pub fn use_account(session: &Session, login: Option<String>) -> Result<(), String> {
    if let Some(login) = login.as_deref().filter(|l| !valid_login(l)) {
        // Never left on the last project's account.
        session.pick(None);
        return Err(format!("Not a GitHub login: {login}"));
    }
    session.pick(login);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(login: &str, active: bool, ok: bool) -> GhAccount {
        GhAccount {
            login: login.into(),
            active,
            ok,
        }
    }

    #[test]
    fn reads_json_status() {
        let out = r#"{"hosts":{"github.com":[
            {"state":"success","active":true,"host":"github.com","login":"octo-one","tokenSource":"keyring","gitProtocol":"https"},
            {"state":"error","error":"bad","active":false,"host":"github.com","login":"octo-two","tokenSource":"keyring","gitProtocol":"https"}
        ],"ghe.example.com":[{"state":"success","active":true,"login":"elsewhere"}]}}"#;
        assert_eq!(
            from_json(out),
            Some(vec![
                account("octo-one", true, true),
                account("octo-two", false, false)
            ])
        );
        assert_eq!(from_json(r#"{"hosts":{}}"#), Some(vec![]));
        // Not JSON: a gh without --json printed its usage.
        assert_eq!(from_json("unknown flag: --json"), None);
    }

    #[test]
    fn reads_text_status() {
        let out = "github.com
  ✓ Logged in to github.com account octo-one (keyring)
  - Active account: true
  - Git operations protocol: https
  - Token: gho_************************************

  X Failed to log in to github.com account octo-two (keyring)
  - Active account: false
  - The token in keyring is invalid.

  X Timeout trying to log in to github.com account octo-three (keyring)
  - Active account: false
";
        assert_eq!(
            from_text(out),
            vec![
                account("octo-one", true, true),
                account("octo-two", false, false),
                account("octo-three", false, false),
            ]
        );
        // gh before 2.40 knew one account per host and named it differently: nothing to pick.
        assert!(from_text("  ✓ Logged in to github.com as octo-one (keyring)").is_empty());
        assert!(from_text("You are not logged into any GitHub hosts.").is_empty());
    }

    /// Enterprise Managed Users sign in to github.com as `<handle>_<shortcode>`: the work account
    /// next to a personal one, the case a per-repository pick is for.
    #[test]
    fn managed_user_logins_can_be_picked() {
        let session = Session::default();
        assert!(use_account(&session, Some("mona-cat_acme".into())).is_ok());
        assert!(valid_login("octo_corp"));
    }

    /// What gh prints as it is today (cli/cli status.go): every state, a token gh holds for no
    /// named user, and the JSON kept as it is once a token is refused (it exits 0 in JSON mode).
    #[test]
    fn reads_every_status_gh_prints() {
        let text = "github.com
  ✓ Logged in to github.com account octo-one (keyring)
  - Active account: false
  - Git operations protocol: ssh
  - Token: gho_************************************
  - Token scopes: 'gist', 'read:org', 'repo'

  ✓ Logged in to github.com account octo-two (/Users/someone/.config/gh/hosts.yml)
  - Active account: true
  - Git operations protocol: https
  - Token: ghp_************************************
  ! Missing required token scopes: 'read:org'
  - To request missing scopes, run: gh auth refresh -h github.com

  X Failed to log in to github.com using token (GH_TOKEN)
  - Active account: true
  - The token in GH_TOKEN is invalid.
";
        assert_eq!(
            from_text(text),
            vec![
                account("octo-one", false, true),
                account("octo-two", true, true)
            ]
        );
        let json = r#"{"hosts":{"github.com":[
            {"state":"timeout","active":true,"host":"github.com","login":"octo-one","tokenSource":"keyring","gitProtocol":"https"},
            {"state":"error","error":"bad","active":false,"host":"github.com","login":"","tokenSource":"GH_TOKEN","gitProtocol":"https"},
            {"state":"success","active":false,"host":"github.com","login":"octo-two","tokenSource":"keyring","gitProtocol":"ssh"}
        ]}}"#;
        assert_eq!(
            from_json(json),
            Some(vec![
                account("octo-one", true, false),
                account("octo-two", false, true)
            ])
        );
    }

    #[test]
    fn logins() {
        assert!(valid_login("octo-one"));
        assert!(!valid_login(""));
        assert!(!valid_login("-octo"));
        assert!(!valid_login(&"o".repeat(80)));
        assert!(!valid_login("a b"));
        assert!(!valid_login("a\nb"));
    }
}
