//! Typing in a vault note that isn't saved yet, kept across closing its tab, switching repos and
//! quitting (vault-edits.json beside the settings): the note's next code view starts from it,
//! and saving still asks if the file changed on disk since the typing began.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gitviber_ui::store::{self, config_dir, write_in_background};
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Past this an edit (its text and the file's) isn't written on each pause in the typing; it's
/// kept when its view goes and at quit.
pub const MAX_PAUSE_BYTES: usize = 4 << 20;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    /// What's typed, as the file would hold it.
    pub text: String,
    /// The file as it was when the typing began.
    pub base: String,
}

/// By vault (its folder), then the note's vault-relative path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
struct Edits(BTreeMap<String, BTreeMap<String, Edit>>);

impl Global for Edits {}

fn path() -> PathBuf {
    config_dir().join("vault-edits.json")
}

fn edits(cx: &mut App) -> &mut Edits {
    if !cx.has_global::<Edits>() {
        let read = std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        cx.set_global::<Edits>(read);
    }
    cx.global_mut::<Edits>()
}

pub fn get(vault: &str, note: &str, cx: &mut App) -> Option<Edit> {
    edits(cx).0.get(vault)?.get(note).cloned()
}

/// Keeps `edit` for the note (None: there's nothing unsaved), and writes the file.
pub fn set(vault: &str, note: &str, edit: Option<Edit>, cx: &mut App) {
    let all = edits(cx);
    let had = all.0.get(vault).and_then(|v| v.get(note)).cloned();
    if had == edit {
        return;
    }
    match edit {
        Some(e) => {
            all.0
                .entry(vault.to_string())
                .or_default()
                .insert(note.to_string(), e);
        }
        None => {
            if let Some(v) = all.0.get_mut(vault) {
                v.remove(note);
                if v.is_empty() {
                    all.0.remove(vault);
                }
            }
        }
    }
    let text = serde_json::to_string_pretty(&*all).unwrap_or_default();
    write_in_background(cx, path(), text);
}

/// The old app's vault edits: kept per store `obsidian:<vault>`, under `vault:<vault>:<path>`.
fn from_legacy(value: impl Fn(&str) -> Option<Value>) -> Edits {
    let mut out = Edits::default();
    let Some(Value::Object(roots)) = value("gitviber.fileEdits") else {
        return out;
    };
    for (root, files) in roots {
        let (Some(vault), Value::Object(files)) = (root.strip_prefix("obsidian:"), files) else {
            continue;
        };
        let prefix = format!("vault:{vault}:");
        for (key, e) in files {
            let string = |k: &str| e.get(k).and_then(Value::as_str).map(str::to_string);
            let (Some(note), Some(text), Some(base)) =
                (key.strip_prefix(&prefix), string("text"), string("base"))
            else {
                continue;
            };
            if text != base && !note.is_empty() {
                out.0
                    .entry(vault.to_string())
                    .or_default()
                    .insert(note.to_string(), Edit { text, base });
            }
        }
    }
    out
}

/// On the first launch, the old app's unsaved vault typing, unless this build has its file.
pub(crate) fn import_legacy() {
    let old = from_legacy(gitviber_ui::legacy::value);
    let path = path();
    if !old.0.is_empty() && !path.exists() {
        let text = serde_json::to_string_pretty(&old).unwrap_or_default();
        if let Err(e) = store::write_atomic(&path, &text) {
            eprintln!("gitviber: couldn't keep the old app's vault edits: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_old_apps_vault_typing_comes_over() {
        let old = json!({
            "obsidian:/v/Work": {
                "vault:/v/Work:Daily/Today.md": { "text": "mine", "base": "disk" },
                "vault:/v/Work:Same.md": { "text": "same", "base": "same" },
                "vault:/v/Other:Wrong.md": { "text": "x", "base": "y" },
            },
            "/repo": { "src/a.rs": { "text": "x", "base": "y" } },
        });
        let edits = from_legacy(|k| (k == "gitviber.fileEdits").then(|| old.clone()));
        let work = &edits.0["/v/Work"];
        assert_eq!(work.len(), 1);
        assert_eq!(
            work["Daily/Today.md"],
            Edit {
                text: "mine".into(),
                base: "disk".into()
            }
        );
        assert_eq!(edits.0.len(), 1, "a repo's edits are Changes' to keep");
        assert!(from_legacy(|_| None).0.is_empty());
    }
}
