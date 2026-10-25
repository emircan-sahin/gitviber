//! About → Third-Party Licenses: every crate the app is built from and the fonts, icons and data
//! it ships, each with the license texts it came with (MIT, Apache-2.0 and OFL ask for them to be
//! passed on). The list is generated from Cargo.lock into third-party-licenses.json, checked in;
//! the `licenses_are_current` test writes it again (GITVIBER_WRITE_LICENSES=1) and in CI fails
//! when a dependency change wasn't followed. Parsed when the dialog opens, dropped when it closes.

use serde::{Deserialize, Serialize};

use gitviber_ui::ActiveAppTheme;
use gitviber_ui::memory::Live;
use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _, px, rems, uniform_list,
};

/// The generated list, deflated by build.rs: 0.1 MB in the binary instead of 1.3.
const JSON_Z: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/licenses.json.z"));

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub license: String,
    pub url: String,
    /// "cargo", or "data" for what's shipped without being a crate (fonts, icons, tables).
    pub source: String,
    /// The package ships no license file: this standard text of its license stands in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard: Option<String>,
    /// Indices into `Licenses::texts`.
    pub texts: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Licenses {
    pub packages: Vec<Package>,
    /// Each text once: the Apache-2.0 body and most MIT files are shared.
    pub texts: Vec<String>,
}

/// The inflated JSON lives only while it's parsed.
pub fn load() -> Licenses {
    miniz_oxide::inflate::decompress_to_vec_zlib(JSON_Z)
        .ok()
        .and_then(|json| serde_json::from_slice(&json).ok())
        .unwrap_or_default()
}

/// The packages a filter keeps: by name or license, ignoring case.
pub fn filtered<'a>(list: &'a Licenses, query: &str) -> Vec<&'a Package> {
    let q = query.trim().to_lowercase();
    list.packages
        .iter()
        .filter(|p| {
            q.is_empty()
                || p.name.to_lowercase().contains(&q)
                || p.license.to_lowercase().contains(&q)
        })
        .collect()
}

pub struct LicensesView {
    list: Licenses,
    query: Entity<InputState>,
    /// The package open, by `source:name@version`.
    expanded: Option<String>,
    _live: Live,
    _sub: Subscription,
}

fn key(p: &Package) -> String {
    format!("{}:{}@{}", p.source, p.name, p.version)
}

pub fn open(window: &mut Window, cx: &mut App) {
    let query = cx.new(|cx| InputState::new(window, cx).placeholder("Filter by name or license"));
    let view = cx.new(|cx| {
        let sub = cx.subscribe(&query, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        });
        LicensesView {
            list: load(),
            query: query.clone(),
            expanded: None,
            _live: Live::new("LicensesDialog"),
            _sub: sub,
        }
    });
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            query.update(cx, |q, cx| q.focus(window, cx));
        }
        dialog
            .title("Third-Party Licenses")
            .w(px(672.))
            .margin_top(px(60.))
            .child(view.clone())
    });
}

impl Render for LicensesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let query = self.query.read(cx).value().to_string();
        let shown: Vec<Package> = filtered(&self.list, &query).into_iter().cloned().collect();
        let expanded = self.expanded.clone();
        let height = (f32::from(window.viewport_size().height) * 0.62).max(240.);
        let open = expanded
            .as_ref()
            .and_then(|k| shown.iter().find(|p| key(p) == *k).cloned());
        let texts: Vec<SharedString> = open
            .as_ref()
            .map(|p| {
                p.texts
                    .iter()
                    .filter_map(|i| self.list.texts.get(*i))
                    .map(|s| SharedString::from(s.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let count = shown.len();
        let this = cx.entity().downgrade();
        let list = uniform_list("licenses", count, move |range, _, cx| {
            let t = cx.app_theme().clone();
            range
                .map(|i| {
                    let p = &shown[i];
                    let k = key(p);
                    let is_open = expanded.as_deref() == Some(k.as_str());
                    let this = this.clone();
                    div()
                        .id(("license", i))
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(px(26.))
                        .px_2()
                        .rounded(px(3.))
                        .cursor_pointer()
                        .text_size(rems(0.75))
                        .when(is_open, |d| d.bg(t.hover))
                        .hover(|s| s.bg(t.hover))
                        .child(
                            Icon::new(if is_open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .xsmall()
                            .text_color(t.subtle),
                        )
                        .child(div().min_w_0().truncate().child(p.name.clone()))
                        .child(
                            div()
                                .flex_none()
                                .font_family(t.fonts.code.clone())
                                .text_size(rems(0.6875))
                                .text_color(t.subtle)
                                .child(p.version.clone()),
                        )
                        .child(
                            div()
                                .ml_auto()
                                .pl_3()
                                .min_w_0()
                                .truncate()
                                .text_size(rems(0.6875))
                                .text_color(t.muted_foreground)
                                .child(p.license.clone()),
                        )
                        .child(
                            div()
                                .w(px(36.))
                                .flex_none()
                                .text_right()
                                .text_size(rems(0.65625))
                                .text_color(t.subtle)
                                .child(if p.source == "cargo" { "crate" } else { "data" }),
                        )
                        .on_click(move |_, _, cx| {
                            let k = k.clone();
                            let _ = this.update(cx, |v, cx| {
                                v.expanded = (v.expanded.as_deref() != Some(&k)).then_some(k);
                                cx.notify();
                            });
                        })
                })
                .collect()
        })
        .h(px(if open.is_some() {
            height * 0.45
        } else {
            height
        }));
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(rems(0.78125))
                    .text_color(t.muted_foreground)
                    .child(
                        "GitViber is built on these open-source packages and data. Click one to \
                         read its license.",
                    ),
            )
            .child(Input::new(&self.query).small())
            .child(if count == 0 {
                div()
                    .py_6()
                    .text_center()
                    .text_size(rems(0.75))
                    .text_color(t.subtle)
                    .child("No packages match")
                    .into_any_element()
            } else {
                list.into_any_element()
            })
            .when_some(open, |d, p| {
                let url = p.url.clone();
                d.child(
                    div()
                        .id("license-texts")
                        .flex()
                        .flex_col()
                        .gap_2()
                        .h(px(height * 0.55))
                        .overflow_y_scroll()
                        .border_t_1()
                        .border_color(t.border)
                        .pt_2()
                        .child(
                            Button::new("license-url")
                                .link()
                                .xsmall()
                                .icon(IconName::ExternalLink)
                                .label(p.url.clone())
                                .on_click(move |_, _, cx| crate::help::open_url(url.clone(), cx)),
                        )
                        .when_some(p.standard.clone(), |d, id| {
                            d.child(div().text_size(rems(0.6875)).text_color(t.subtle).child(
                                format!(
                                    "This package ships no license file, so this is the \
                                         standard {id} text."
                                ),
                            ))
                        })
                        .children(texts.into_iter().map(|text| {
                            div()
                                .p_2()
                                .rounded(px(3.))
                                .border_1()
                                .border_color(t.border)
                                .bg(t.code(t.background))
                                .font_family(t.fonts.code.clone())
                                .text_size(rems(0.65625))
                                .text_color(t.muted_foreground)
                                .child(text)
                        })),
                )
            })
    }
}

#[cfg(test)]
pub(crate) mod generate {
    //! Writes third-party-licenses.json from `cargo metadata`, as scripts/licenses.mjs did for
    //! the old app: the app's normal dependencies on every platform, with the license files each
    //! one includes, and the data the app ships that isn't a crate.

    use super::{Licenses, Package};
    use serde_json::Value;
    use std::collections::{BTreeSet, HashMap};
    use std::path::{Path, PathBuf};

    pub fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn is_license_file(name: &str) -> bool {
        let lower = name.to_lowercase();
        ["license", "licence", "copying", "notice", "unlicense"]
            .iter()
            .any(|stem| {
                lower
                    .strip_prefix(stem)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with(['-', '.', '_']))
            })
    }

    fn read_text(path: &Path) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    /// The license files in `dir`, and `extra`, each read once, in name order.
    fn texts(dir: &Path, extra: Option<PathBuf>) -> Vec<String> {
        let mut files: BTreeSet<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter(|e| is_license_file(&e.file_name().to_string_lossy()))
            .map(|e| e.path())
            .collect();
        files.extend(extra);
        files.iter().filter_map(|f| read_text(f)).collect()
    }

    fn cargo_packages() -> Result<Vec<(Package, Vec<String>)>, String> {
        let out = std::process::Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--manifest-path",
            ])
            .arg(root().join("Cargo.toml"))
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
        let packages: HashMap<&str, &Value> = meta["packages"]
            .as_array()
            .ok_or("no packages")?
            .iter()
            .filter_map(|p| Some((p["id"].as_str()?, p)))
            .collect();
        let members: BTreeSet<&str> = meta["workspace_members"]
            .as_array()
            .ok_or("no members")?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let nodes: HashMap<&str, &Value> = meta["resolve"]["nodes"]
            .as_array()
            .ok_or("no resolve")?
            .iter()
            .filter_map(|n| Some((n["id"].as_str()?, n)))
            .collect();
        let app = packages
            .values()
            .find(|p| p["name"] == "gitviber")
            .and_then(|p| p["id"].as_str())
            .ok_or("no gitviber package")?;
        // Normal (not build, not dev) dependencies, on every target platform.
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut queue = vec![app];
        while let Some(id) = queue.pop() {
            let Some(node) = nodes.get(id) else { continue };
            for dep in node["deps"].as_array().into_iter().flatten() {
                let Some(pkg) = dep["pkg"].as_str() else {
                    continue;
                };
                let normal = dep["dep_kinds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|k| k["kind"].is_null());
                if normal && seen.insert(pkg) {
                    queue.push(pkg);
                }
            }
        }
        Ok(seen
            .into_iter()
            .filter(|id| !members.contains(id))
            .filter_map(|id| packages.get(id))
            .map(|p| {
                let manifest = PathBuf::from(p["manifest_path"].as_str().unwrap_or_default());
                let dir = manifest.parent().unwrap_or(Path::new(".")).to_path_buf();
                let extra = p["license_file"].as_str().map(|f| dir.join(f));
                let name = p["name"].as_str().unwrap_or_default().to_string();
                let url = p["repository"]
                    .as_str()
                    .map(|r| r.trim_end_matches(".git").to_string())
                    .or_else(|| p["homepage"].as_str().map(str::to_string))
                    .unwrap_or_else(|| format!("https://crates.io/crates/{name}"));
                let license = p["license"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        if extra.is_some() {
                            "See license text".into()
                        } else {
                            "UNKNOWN".into()
                        }
                    });
                let package = Package {
                    version: p["version"].as_str().unwrap_or_default().to_string(),
                    license,
                    url,
                    source: "cargo".into(),
                    standard: None,
                    texts: Vec::new(),
                    name,
                };
                (package, texts(&dir, extra))
            })
            .collect())
    }

    /// What the app ships that isn't a crate: the bundled fonts, the file icons, the browser's
    /// device table.
    fn data() -> Vec<(Package, Vec<String>)> {
        let ui = root().join("crates/ui/assets");
        let entry = |name: &str, version: &str, license: &str, url: &str, files: &[PathBuf]| {
            (
                Package {
                    name: name.into(),
                    version: version.into(),
                    license: license.into(),
                    url: url.into(),
                    source: "data".into(),
                    standard: None,
                    texts: Vec::new(),
                },
                files.iter().filter_map(|f| read_text(f)).collect(),
            )
        };
        vec![
            entry(
                "Geist",
                "1.4",
                "OFL-1.1",
                "https://github.com/vercel/geist-font",
                &[ui.join("fonts/geist/OFL.txt")],
            ),
            entry(
                "Geist Mono",
                "1.4",
                "OFL-1.1",
                "https://github.com/vercel/geist-font",
                &[ui.join("fonts/geist-mono/OFL.txt")],
            ),
            entry(
                "JetBrains Mono",
                "2.304",
                "OFL-1.1",
                "https://github.com/JetBrains/JetBrainsMono",
                &[ui.join("fonts/jetbrains-mono/OFL.txt")],
            ),
            entry(
                "material-icon-theme",
                "5.38.1",
                "MIT",
                "https://github.com/material-extensions/vscode-material-icon-theme",
                &[ui.join("file-icons/LICENSE")],
            ),
            // The browser's device sizes, pixel ratios and user agents, as of the day they were
            // taken (its version).
            entry(
                "chrome-devtools-frontend",
                "2026-10-09",
                "BSD-3-Clause",
                "https://chromium.googlesource.com/devtools/devtools-frontend/+/a23d72d27dd00adef0dd7130fcc0b2a99e5495bc/front_end/models/emulation/EmulatedDevices.ts",
                &[root().join("crates/app/licenses/standard/chrome-devtools-frontend.txt")],
            ),
        ]
    }

    /// A package without a license file gets the standard text of the license it offers, MIT
    /// when that's one of the choices.
    fn with_standard(p: (Package, Vec<String>)) -> (Package, Vec<String>) {
        let (mut p, texts) = p;
        if !texts.is_empty() {
            return (p, texts);
        }
        let ids: Vec<String> = p
            .license
            .split(" OR ")
            .flat_map(|s| s.split('/'))
            .map(|s| s.replace(['(', ')'], "").trim().to_string())
            .collect();
        let id = if ids.iter().any(|i| i == "MIT") {
            "MIT".to_string()
        } else {
            ids.first().cloned().unwrap_or_default()
        };
        let file = root().join(format!("crates/app/licenses/standard/{id}.txt"));
        match read_text(&file) {
            Some(text) if !p.license.contains(" AND ") => {
                p.standard = Some(id);
                (p, vec![text])
            }
            _ => (p, texts),
        }
    }

    pub fn build() -> Result<String, String> {
        let mut all: Vec<(Package, Vec<String>)> = cargo_packages()?
            .into_iter()
            .chain(data())
            .map(with_standard)
            .collect();
        all.sort_by(|a, b| {
            (a.0.name.as_str(), a.0.source.as_str(), a.0.version.as_str()).cmp(&(
                b.0.name.as_str(),
                b.0.source.as_str(),
                b.0.version.as_str(),
            ))
        });
        let mut out = Licenses::default();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut refer = |text: &String, out: &mut Licenses| -> usize {
            *index.entry(text.clone()).or_insert_with(|| {
                out.texts.push(text.clone());
                out.texts.len() - 1
            })
        };
        // The data's texts numbered last: adding an entry there doesn't renumber every crate's.
        for (_, texts) in all.iter().filter(|(p, _)| p.source != "data") {
            for t in texts {
                refer(t, &mut out);
            }
        }
        for (mut p, texts) in all {
            p.texts = texts.iter().map(|t| refer(t, &mut out)).collect();
            out.packages.push(p);
        }
        let mut text = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
        text.push('\n');
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_reads_and_filters() {
        let list = load();
        assert!(!list.packages.is_empty(), "the deflated list didn't read");
        for p in &list.packages {
            assert!(p.texts.iter().all(|i| *i < list.texts.len()), "{}", p.name);
        }
        let sample = Licenses {
            packages: vec![
                Package {
                    name: "serde".into(),
                    version: "1".into(),
                    license: "MIT OR Apache-2.0".into(),
                    url: String::new(),
                    source: "cargo".into(),
                    standard: None,
                    texts: vec![],
                },
                Package {
                    name: "Geist".into(),
                    version: "1".into(),
                    license: "OFL-1.1".into(),
                    url: String::new(),
                    source: "data".into(),
                    standard: None,
                    texts: vec![],
                },
            ],
            texts: vec![],
        };
        assert_eq!(filtered(&sample, "ofl").len(), 1);
        assert_eq!(filtered(&sample, "SERDE").len(), 1);
        assert_eq!(filtered(&sample, " ").len(), 2);
    }

    /// GITVIBER_WRITE_LICENSES=1 writes the list again; in CI a stale one fails.
    #[test]
    fn licenses_are_current() {
        let built = match generate::build() {
            Ok(b) => b,
            Err(err) => {
                eprintln!("cargo metadata isn't available here, list not checked: {err}");
                return;
            }
        };
        let path = generate::root().join("crates/app/licenses/third-party-licenses.json");
        if std::env::var("GITVIBER_WRITE_LICENSES").is_ok_and(|v| v == "1") {
            std::fs::write(&path, &built).unwrap();
            return;
        }
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current != built {
            let note = "crates/app/licenses/third-party-licenses.json is out of date: run \
                        GITVIBER_WRITE_LICENSES=1 cargo nextest run -p gitviber licenses_are_current";
            assert!(std::env::var_os("CI").is_none(), "{note}");
            eprintln!("{note}");
        }
    }
}
