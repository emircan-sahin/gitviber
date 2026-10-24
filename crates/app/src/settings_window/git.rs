//! The git sections: background fetch, the worktrees' folder, this repository's identity, GitHub
//! account and remotes; the agent CLI that writes commit messages; and the one that writes
//! guided reviews.

use std::rc::Rc;

use gitviber_ui::settings::*;
use gitviber_ui::{ActiveAppTheme, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement as _, SharedString, Styled as _, div,
    prelude::FluentBuilder as _, px, rems,
};

use super::SettingsWindow;
use super::fields::{GetText, SetText, TextField, note, options, pre, segments, switch};

pub fn git(w: &SettingsWindow, cx: &App) -> SettingPage {
    let mut groups =
        vec![
        SettingGroup::new().title("Background").item(
            SettingItem::new(
                "Fetch in the background",
                SettingField::render(|opts, _, cx| {
                    let list = options(FETCH_INTERVALS, |m| {
                        if *m == 0 {
                            "Off".into()
                        } else {
                            format!("{m} min")
                        }
                    });
                    let current = Settings::get(cx).background_fetch.to_string();
                    segments("fetch", &list, &current, opts.is_disabled(), |v, _, cx| {
                        let v = v.parse().unwrap_or(0);
                        Settings::update(cx, |s| s.background_fetch = v)
                    })
                }),
            )
            .description(
                "Keeps ahead / behind and the remote branches current for the open repository. A \
                 fetch that fails, say while offline, stays quiet.",
            ),
        ),
        SettingGroup::new().title("Worktrees").item(worktree_root(w)),
    ];
    if w.data.git.identity.is_some() || w.data.git.remotes.is_some() {
        let mut repo = Vec::new();
        if w.data.git.identity.is_some() {
            repo.push(identity(w, cx));
        }
        if let Some(item) = github_account(w) {
            repo.push(item);
        }
        if w.data.git.remotes.is_some() {
            repo.push(remotes(w));
        }
        groups.push(SettingGroup::new().title("This repository").items(repo));
    }
    SettingPage::new("Git")
        .icon(IconName::GitBranch)
        .groups(groups)
}

/// Off by default: worktrees go in `<project>.worktrees` beside the project. Turning it on is
/// picking the folder; a cancelled picker leaves it as it was.
fn worktree_root(w: &SettingsWindow) -> SettingItem {
    let weak = w.weak.clone();
    SettingItem::new(
        "One folder for all worktrees",
        SettingField::render(move |_, _, cx| {
            let t = cx.app_theme();
            let root = Settings::get(cx).worktree_root.clone();
            let (pick, toggle) = (weak.clone(), weak.clone());
            div()
                .flex()
                .items_center()
                .gap_2()
                .when_some(root.clone(), |d, root| {
                    d.child(
                        div()
                            .max_w(px(176.))
                            .truncate()
                            .font_family(t.fonts.code.clone())
                            .text_size(rems(0.6875))
                            .text_color(t.muted_foreground)
                            .child(root),
                    )
                    .child(
                        Button::new("worktree-root-change")
                            .small()
                            .outline()
                            .label("Change…")
                            .on_click(move |_, _, cx| {
                                let _ = pick.update(cx, |w, cx| w.pick_worktree_root(cx));
                            }),
                    )
                })
                .child(
                    Switch::new("worktree-root")
                        .checked(root.is_some())
                        .on_click(move |on, _, cx| {
                            if *on {
                                let _ = toggle.update(cx, |w, cx| w.pick_worktree_root(cx));
                            } else {
                                Settings::update(cx, |s| s.worktree_root = None);
                            }
                        }),
                )
                .into_any_element()
        }),
    )
    .description(
        "New worktrees go in a subfolder named after the project, instead of a .worktrees folder \
         beside each project. A folder picked for a project when making a worktree still comes first.",
    )
}

fn who(name: &Option<String>, email: &Option<String>) -> String {
    if name.is_none() && email.is_none() {
        return "not set".into();
    }
    format!(
        "{} <{}>",
        name.as_deref().unwrap_or("no name"),
        email.as_deref().unwrap_or("no email")
    )
}

/// Commit here as someone else than everywhere else.
fn identity(w: &SettingsWindow, _cx: &App) -> SettingItem {
    let id = w.data.git.identity.clone().unwrap_or_default();
    let weak = w.weak.clone();
    let inputs = w.identity_inputs.clone();
    let own = w.data.git.own;
    SettingItem::new(
        "Commit as",
        SettingField::render(move |_, _, cx| {
            let pick = weak.clone();
            let list = vec![
                ("global".to_string(), "Global".to_string()),
                ("own".to_string(), "This repository".to_string()),
            ];
            let picker = segments(
                "identity-scope",
                &list,
                if own { "own" } else { "global" },
                false,
                move |v, window, cx| {
                    let own = v == "own";
                    let _ = pick.update(cx, |w, cx| w.set_identity_scope(own, window, cx));
                },
            );
            let form = inputs.clone().filter(|_| own).map(|(name, email)| {
                let save = weak.clone();
                let typed_name = name.read(cx).value().trim().to_string();
                let typed_email = email.read(cx).value().trim().to_string();
                let changed = typed_name != id.own_name.clone().unwrap_or_default()
                    || typed_email != id.own_email.clone().unwrap_or_default();
                let ready = changed && !typed_name.is_empty() && !typed_email.is_empty();
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(Input::new(&name).small())
                    .child(Input::new(&email).small())
                    .child(
                        Button::new("identity-save")
                            .small()
                            .primary()
                            .label("Save")
                            .disabled(!ready)
                            .on_click(move |_, _, cx| {
                                let _ = save.update(cx, |w, cx| w.save_identity(cx));
                            }),
                    )
            });
            div()
                .flex()
                .flex_col()
                .gap_2()
                .w(px(256.))
                .child(picker)
                .children(form)
                .into_any_element()
        }),
    )
    .description(format!(
        "Who commits in this repository. Everywhere else: {} (your global git config).",
        who(
            &w.data
                .git
                .identity
                .as_ref()
                .and_then(|i| i.global_name.clone()),
            &w.data
                .git
                .identity
                .as_ref()
                .and_then(|i| i.global_email.clone())
        )
    ))
}

/// With more than one account signed in to gh, which one this project's GitHub calls use. Also
/// while one is picked, even if gh has fewer now (or is gone), so the pick can be undone.
fn github_account(w: &SettingsWindow) -> Option<SettingItem> {
    let main = w.data.git.main.clone()?;
    let accounts = w.data.git.accounts.clone();
    let picked = w.data.git.picked_account.clone();
    if picked.is_none() && accounts.as_ref().is_none_or(|a| a.len() < 2) {
        return None;
    }
    let active = accounts
        .as_ref()
        .and_then(|a| a.iter().find(|a| a.active))
        .map(|a| a.login.clone());
    let mut list: Vec<(SharedString, SharedString)> = vec![(
        SharedString::default(),
        match &active {
            Some(login) => format!("Active in gh ({login})").into(),
            None => "Active in gh".into(),
        },
    )];
    for a in accounts.iter().flatten() {
        let label = if a.ok {
            a.login.clone()
        } else {
            format!("{} (sign-in expired)", a.login)
        };
        list.push((a.login.clone().into(), label.into()));
    }
    // Signed out of gh since, or gh gone: still the pick until it's undone.
    if let Some(p) = &picked
        && !accounts.iter().flatten().any(|a| &a.login == p)
    {
        let label = if accounts.is_some() {
            format!("{p} (not in gh)")
        } else {
            p.clone()
        };
        list.push((p.clone().into(), label.into()));
    }
    let weak = w.weak.clone();
    Some(
        SettingItem::new(
            "GitHub account",
            SettingField::scrollable_dropdown(
                list,
                move |cx| {
                    gitviber_github::store::Store::get(cx)
                        .account(&main)
                        .unwrap_or_default()
                        .into()
                },
                move |v: SharedString, cx| {
                    let login = (!v.is_empty()).then(|| v.to_string());
                    let _ = weak.update(cx, |w, cx| w.pick_account(login, cx));
                },
            ),
        )
        .description(
            "Pull requests, issues and checks here use this gh account. Push and fetch sign in \
             as git does.",
        ),
    )
}

/// The repository's remotes: add, rename, repoint or remove one.
fn remotes(w: &SettingsWindow) -> SettingItem {
    let list = w.data.git.remotes.clone().unwrap_or_default();
    let editing = w.data.git.editing.clone();
    let inputs = w.remote_inputs.clone();
    let weak = w.weak.clone();
    SettingItem::new(
        "Remotes",
        SettingField::render(move |_, _, cx| {
            let t = cx.app_theme().clone();
            let form = |cx: &App| -> AnyElement {
                let Some((name, url)) = inputs.clone() else {
                    return div().into_any_element();
                };
                let ready = !name.read(cx).value().trim().is_empty()
                    && !url.read(cx).value().trim().is_empty();
                let (cancel, save) = (weak.clone(), weak.clone());
                let adding = editing.as_deref() == Some("");
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.border)
                    .child(Input::new(&name).small())
                    .child(Input::new(&url).small())
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("remote-cancel")
                                    .small()
                                    .label("Cancel")
                                    .on_click(move |_, window, cx| {
                                        let _ = cancel
                                            .update(cx, |w, cx| w.edit_remote(None, window, cx));
                                    }),
                            )
                            .child(
                                Button::new("remote-save")
                                    .small()
                                    .primary()
                                    .label(if adding { "Add" } else { "Save" })
                                    .disabled(!ready)
                                    .on_click(move |_, _, cx| {
                                        let _ = save.update(cx, |w, cx| w.save_remote(cx));
                                    }),
                            ),
                    )
                    .into_any_element()
            };
            let rows: Vec<AnyElement> = list
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    if editing.as_deref() == Some(r.name.as_str()) {
                        return form(cx);
                    }
                    let (edit, remove) = (weak.clone(), weak.clone());
                    let (name, gone, url) = (r.name.clone(), r.name.clone(), r.url.clone());
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(rems(0.75))
                        .child(
                            div()
                                .flex_none()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child(r.name.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(t.fonts.code.clone())
                                .text_size(rems(0.6875))
                                .text_color(t.muted_foreground)
                                .child(r.url.clone()),
                        )
                        .child(
                            Button::new(("remote-edit", i))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Pencil)
                                .tooltip("Edit")
                                .on_click(move |_, window, cx| {
                                    let name = name.clone();
                                    let _ = edit
                                        .update(cx, |w, cx| w.edit_remote(Some(name), window, cx));
                                }),
                        )
                        .child(
                            Button::new(("remote-remove", i))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Trash)
                                .tooltip("Remove…")
                                .on_click(move |_, window, cx| {
                                    let (name, url) = (gone.clone(), url.clone());
                                    let _ = remove
                                        .update(cx, |w, cx| w.remove_remote(name, url, window, cx));
                                }),
                        )
                        .into_any_element()
                })
                .collect();
            let add = if editing.as_deref() == Some("") {
                form(cx)
            } else {
                let add = weak.clone();
                div()
                    .child(
                        Button::new("remote-add")
                            .small()
                            .outline()
                            .icon(IconName::Plus)
                            .label("Add remote")
                            .on_click(move |_, window, cx| {
                                let _ = add.update(cx, |w, cx| {
                                    w.edit_remote(Some(String::new()), window, cx)
                                });
                            }),
                    )
                    .into_any_element()
            };
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .w(px(272.))
                .children(rows)
                .child(add)
                .into_any_element()
        }),
    )
    .description("Where this repository fetches from and pushes to.")
}

/// The agent CLI picker: a preset types its command; Custom stays picked while its text happens
/// to match one.
fn agent_item(
    w: &SettingsWindow,
    which: Which,
    title: &'static str,
    hint: &'static str,
) -> SettingItem {
    let weak = w.weak.clone();
    let custom = w.custom_agent[which as usize];
    let mut list: Vec<(SharedString, SharedString)> = AGENT_PRESETS
        .iter()
        .map(|p| (p.key.into(), p.label.into()))
        .collect();
    list.push(("custom".into(), "Custom".into()));
    let command = which.getter();
    let get = command.clone();
    SettingItem::new(
        title,
        SettingField::scrollable_dropdown(
            list,
            move |cx| {
                let c = get(Settings::get(cx));
                match AgentPreset::of(&c) {
                    Some(p) if !custom => p.key.into(),
                    _ => "custom".into(),
                }
            },
            move |v: SharedString, cx| {
                let _ = weak.update(cx, |w, cx| w.pick_agent(which, &v, cx));
            },
        ),
    )
    .description(hint)
}

/// The command a picker sets: Commit Messages' or Guided Review's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Which {
    Suggest = 0,
    Review = 1,
}

impl Which {
    pub fn getter(self) -> Rc<dyn Fn(&Settings) -> String> {
        match self {
            Which::Suggest => Rc::new(|s: &Settings| s.suggest_command.clone()),
            Which::Review => Rc::new(|s: &Settings| {
                s.review_command
                    .clone()
                    .unwrap_or_else(|| s.suggest_command.clone())
            }),
        }
    }

    pub fn set(self, s: &mut Settings, command: &str) {
        match self {
            Which::Suggest => s.suggest_command = command.to_string(),
            Which::Review => s.review_command = Some(command.to_string()),
        }
    }
}

fn command_field(which: Which, hint: &'static str) -> SettingItem {
    SettingItem::new(
        "Command",
        SettingField::render(move |_, window, cx| {
            TextField::new(
                match which {
                    Which::Suggest => "suggest-command",
                    Which::Review => "review-command",
                },
                which.getter(),
                Rc::new(move |s, v| which.set(s, v)),
            )
            .placeholder("claude -p")
            .width(px(320.))
            .render(window, cx)
        }),
    )
    .description(hint)
}

fn models_link(id: &'static str, (label, url): (&'static str, &'static str)) -> AnyElement {
    super::fields::link(id, label, url).into_any_element()
}

/// The models field of a preset: an id typed, empty for the CLI's own default.
fn model_field(p: &'static AgentPreset, which: Which) -> SettingItem {
    let lean = if p.lean.is_empty() {
        String::new()
    } else {
        format!(
            " Every run also gets {} (a guided review --tools Read,Grep,Glob, to read the patch): \
             no MCP servers, skills, saved session or tools beyond reading files, so it starts and \
             answers faster.",
            p.lean
        )
    };
    let hint = match which {
        Which::Suggest => format!(
            "Passed as {} {}; empty uses the CLI's own default. New models come out often, and \
             the link has the current IDs.{lean}",
            p.model_flag, p.model
        ),
        Which::Review => {
            "Empty runs Commit Messages' model for it. The link has the current IDs.".to_string()
        }
    };
    SettingItem::new(
        "Model",
        SettingField::render(move |_, window, cx| {
            let key = p.key;
            let (get, set): (GetText, SetText) = match which {
                Which::Suggest => (
                    Rc::new(move |s: &Settings| p.model_in(&s.suggest_models)),
                    Rc::new(move |s: &mut Settings, v: &str| {
                        let v: String = v.chars().filter(|c| !c.is_whitespace()).collect();
                        s.suggest_models.insert(key.into(), v);
                    }),
                ),
                Which::Review => (
                    Rc::new(move |s: &Settings| {
                        s.review_models.get(key).cloned().unwrap_or_default()
                    }),
                    Rc::new(move |s: &mut Settings, v: &str| {
                        let v: String = v.chars().filter(|c| !c.is_whitespace()).collect();
                        if v.is_empty() {
                            s.review_models.remove(key);
                        } else {
                            s.review_models.insert(key.into(), v);
                        }
                    }),
                ),
            };
            let placeholder: SharedString = match which {
                Which::Suggest => "CLI default".into(),
                Which::Review => {
                    let m = p.model_in(&Settings::get(cx).suggest_models);
                    format!(
                        "Same as Commit Messages: {}",
                        if m.is_empty() { "CLI default" } else { &m }
                    )
                    .into()
                }
            };
            let field_key: SharedString = format!("{which:?}-model-{key}").into();
            div()
                .flex()
                .flex_col()
                .items_end()
                .gap_1()
                .child(
                    TextField::new(field_key, get, set)
                        .placeholder(placeholder)
                        .width(px(320.))
                        .render(window, cx),
                )
                .child(models_link("models-link", p.models))
                .into_any_element()
        }),
    )
    .description(hint)
}

/// A custom command's model, which goes in the command itself.
fn custom_model() -> SettingItem {
    SettingItem::new(
        "Model",
        SettingField::render(|_, _, _| models_link("all-models", ALL_MODELS)),
    )
    .description("Goes in the command itself. models.dev lists every provider's current model IDs.")
}

fn effort_field(p: &'static AgentPreset, which: Which) -> Option<SettingItem> {
    let (flag, levels, default) = p.effort?;
    let key = p.key;
    let mut list: Vec<(SharedString, SharedString)> = Vec::new();
    if which == Which::Review {
        list.push(("same".into(), "Same as Commit Messages".into()));
    }
    list.push(("".into(), "CLI default".into()));
    list.extend(
        levels
            .iter()
            .map(|l| (SharedString::from(*l), SharedString::from(*l))),
    );
    let hint = match which {
        Which::Suggest => format!(
            "Passed as {}{}; CLI default passes none, leaving it to the CLI's own settings. More \
             effort writes more considered answers, slower and at a higher cost.{}",
            p.effort_arg("<level>"),
            if default.is_empty() {
                String::new()
            } else {
                format!(", {default} until you pick another")
            },
            if key == "opencode" {
                " Levels are per model, and not every model has every one."
            } else {
                ""
            }
        ),
        Which::Review => "A guided review reads the whole change: more effort can catch more, \
                          slower and at a higher cost."
            .to_string(),
    };
    let _ = flag;
    Some(
        SettingItem::new(
            "Effort",
            SettingField::scrollable_dropdown(
                list,
                move |cx| {
                    let s = Settings::get(cx);
                    match which {
                        Which::Suggest => p.effort_in(&s.suggest_efforts).into(),
                        Which::Review => s
                            .review_efforts
                            .get(key)
                            .cloned()
                            .unwrap_or_else(|| "same".into())
                            .into(),
                    }
                },
                move |v: SharedString, cx| {
                    Settings::update(cx, |s| match which {
                        Which::Suggest => {
                            s.suggest_efforts.insert(key.into(), v.to_string());
                        }
                        Which::Review if v.as_ref() == "same" => {
                            s.review_efforts.remove(key);
                        }
                        Which::Review => {
                            s.review_efforts.insert(key.into(), v.to_string());
                        }
                    })
                },
            ),
        )
        .description(hint),
    )
}

const SUGGEST_LIMIT_KB: usize = gitviber_core::suggest::MAX_DIFF / 1024;
const GUIDE_LIMIT_KB: usize = gitviber_core::suggest::MAX_GUIDE_INPUT / 1024;

pub fn commit(w: &SettingsWindow, cx: &App) -> SettingPage {
    let s = Settings::get(cx);
    let preset = AgentPreset::of(&s.suggest_command).filter(|_| !w.custom_agent[0]);
    let mut items = vec![
        switch(
            "Suggest commit messages",
            "Adds a ✦ button to the commit box that asks your own agent CLI to write the message, \
             and one to New pull request for its title and description. GitViber sends nothing \
             itself and keeps no keys: the command runs on this computer, and it decides where \
             the diff goes.",
            |s| s.suggest_enabled,
            |s, v| s.suggest_enabled = v,
        ),
        agent_item(
            w,
            Which::Suggest,
            "Agent",
            "The CLI that writes the message.",
        ),
        command_field(
            Which::Suggest,
            "Runs in the repository's folder, directly, not through a shell. The prompt and the \
             diff arrive on stdin; put {prompt} in the command to pass the prompt as an argument \
             instead. If it isn't found, give its full path (`which claude` in Terminal prints it).",
        ),
    ];
    match preset {
        Some(p) => {
            items.push(model_field(p, Which::Suggest));
            items.extend(effort_field(p, Which::Suggest));
        }
        None => items.push(custom_model()),
    }
    let prompt = gitviber_changes::model::text::SUGGEST_PROMPT;
    let pull = gitviber_github::logic::suggest::PULL_PROMPT;
    SettingPage::new("Commit Messages")
        .icon(IconName::Sparkles)
        .groups([
            SettingGroup::new().title("Suggestions").items(items),
            SettingGroup::new().title("What the command gets").item(
                SettingItem::render(move |_, _, cx| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .w_full()
                        .child(note(
                            "Only when you click ✦, and nothing else from the app (Guided Review \
                             shows what a guided review gets):",
                            cx,
                        ))
                        .child(pre(
                            format!(
                                "{prompt}\n\n[the diff: the staged changes, or every change when \
                                 nothing is staged (Commit all), or the whole commit when \
                                 amending; up to {SUGGEST_LIMIT_KB} KB, with a note in the prompt \
                                 when it's cut]"
                            ),
                            cx,
                        ))
                        .child(note("For a pull request:", cx))
                        .child(pre(
                            format!(
                                "{pull}\n\n[the subjects of the branch's commits, the \
                                 repository's pull request template if it has one, and the \
                                 branch's diff since it left the base; up to {SUGGEST_LIMIT_KB} KB \
                                 in all]"
                            ),
                            cx,
                        ))
                        .into_any_element()
                })
                .keywords(["prompt", "diff"]),
            ),
        ])
}

pub fn review(w: &SettingsWindow, cx: &App) -> SettingPage {
    let s = Settings::get(cx);
    let command = s
        .review_command
        .clone()
        .unwrap_or_else(|| s.suggest_command.clone());
    let own_custom = s.review_command.is_some() && w.custom_agent[1];
    let preset = AgentPreset::of(&command).filter(|_| !own_custom);
    let program = program_of(&s.suggest_command).to_string();
    let same: SharedString = format!(
        "Same as Commit Messages ({})",
        if program.is_empty() { "none" } else { &program }
    )
    .into();
    let weak = w.weak.clone();
    let mut agent =
        vec![SettingItem::new(
        "Run with",
        SettingField::scrollable_dropdown(
            vec![
                ("same".into(), same),
                ("own".into(), "Its own command".into()),
            ],
            |cx| {
                if Settings::get(cx).review_command.is_some() {
                    "own".into()
                } else {
                    "same".into()
                }
            },
            move |v: SharedString, cx| {
                let _ = weak.update(cx, |w, cx| w.set_review_own(v.as_ref() == "own", cx));
            },
        ),
    )
    .description(
        "The agent CLI that writes Explain Commit (History) and Guided Review (branch review).",
    )];
    if s.review_command.is_some() {
        agent.push(agent_item(w, Which::Review, "Agent", "Its own CLI."));
        agent.push(command_field(
            Which::Review,
            "Runs as Commit Messages' does: in the repository's folder, directly, not through a shell.",
        ));
    }
    match preset {
        Some(p) => {
            agent.push(model_field(p, Which::Review));
            agent.extend(effort_field(p, Which::Review));
        }
        None => agent.push(custom_model()),
    }
    let custom_language = w.custom_language
        || !REVIEW_LANGUAGES
            .iter()
            .any(|(name, _)| *name == s.review_language);
    let weak = w.weak.clone();
    let mut list: Vec<(SharedString, SharedString)> = REVIEW_LANGUAGES
        .iter()
        .map(|(name, own)| (SharedString::from(*name), SharedString::from(*own)))
        .collect();
    list.push(("custom".into(), "Custom…".into()));
    let mut writing =
        vec![SettingItem::new(
        "Language",
        SettingField::scrollable_dropdown(
            list,
            move |cx| {
                if custom_language {
                    "custom".into()
                } else {
                    Settings::get(cx).review_language.clone().into()
                }
            },
            move |v: SharedString, cx| {
                let _ = weak.update(cx, |w, cx| w.pick_language(&v, cx));
            },
        ),
    )
    .description(
        "Guides and risks are written in it, and Ask Agent asks for replies in it; code, paths \
         and names stay as they are. Commit messages and pull requests stay in English.",
    )];
    if custom_language {
        writing.push(
            SettingItem::new(
                "Custom language",
                SettingField::render(|_, window, cx| {
                    TextField::new(
                        "review-language",
                        Rc::new(|s: &Settings| s.review_language.clone()),
                        Rc::new(|s: &mut Settings, v: &str| {
                            s.review_language = v.chars().take(40).collect()
                        }),
                    )
                    .placeholder("English")
                    .width(px(320.))
                    .render(window, cx)
                }),
            )
            .description(
                "Its name, as you'd say it to the agent: Ukrainian, Brazilian Portuguese, Bahasa \
                 Indonesia.",
            ),
        );
    }
    SettingPage::new("Guided Review")
        .icon(IconName::ScanSearch)
        .groups([
            SettingGroup::new().title("Agent").items(agent),
            SettingGroup::new().title("Writing").items(writing),
            SettingGroup::new().title("What the command gets").item(
                SettingItem::render(|_, _, cx| {
                    let language = Settings::get(cx).review_language.clone();
                    let guide = gitviber_review::model::guide::guide_prompt(&language);
                    let risks = gitviber_review::model::risks::risks_prompt(&language);
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .w_full()
                        .child(note(
                            "Only when you click Explain Commit, Guided Review or Find Risks, and \
                             nothing else from the app:",
                            cx,
                        ))
                        .child(pre(
                            format!(
                                "{guide}\n\n[a commit's message, the subjects of the branch's or \
                                 pull request's commits since it left the base, or for uncommitted \
                                 changes what they're against; a list of every changed file; then \
                                 as many whole diffs as fit in {GUIDE_LIMIT_KB} KB, lockfiles and \
                                 generated files left out. When any diff is left out, the whole \
                                 patch goes in a temporary file only you can read, named in the \
                                 prompt and deleted when the run ends.]"
                            ),
                            cx,
                        ))
                        .child(note(
                            "Find Risks sends the same change after this instead:",
                            cx,
                        ))
                        .child(pre(risks, cx))
                        .child(note(
                            "Ask Agent takes your question, or Skip, then opens a new terminal tab \
                             where Claude Code starts with that part of the review and the \
                             question sent, with this page's model and effort; Send to Agent \
                             sends a risk right away. codex, opencode and pi start with it on the \
                             clipboard.",
                            cx,
                        ))
                        .into_any_element()
                })
                .keywords(["prompt", "risks"]),
            ),
        ])
}
