//! How the guide tab draws: its header and notices, the guide column and the diffs beside it.

use super::*;
use crate::model::agent::{PRESETS, effort_of, model_choices, model_of, preset_of, program_of};
use gitviber_ui::settings::REVIEW_LANGUAGES;

fn elapsed(run: &Option<Run>) -> String {
    match run {
        Some(Run::Running { since, .. }) => {
            let s = since.elapsed().as_secs();
            format!("{}:{:02}", s / 60, s % 60)
        }
        _ => String::new(),
    }
}

/// The last lines the agent CLI printed, as it prints them; none until it does.
fn printed(tail: &crate::store::Tail, t: &AppTheme) -> Option<AnyElement> {
    let lines = tail.lines(8);
    (!lines.is_empty()).then(|| {
        div()
            .px(px(12.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(t.panel)
            .font_family(t.fonts.code.clone())
            .text_size(rems(0.68))
            .text_color(t.muted_foreground)
            .children(lines.into_iter().map(|l| div().truncate().child(l)))
            .into_any_element()
    })
}

pub(super) fn title_text(s: &Section) -> String {
    if s.title.is_empty() {
        "Untitled".into()
    } else {
        s.title.clone()
    }
}

/// How the view speaks of what it reviews, by its kind.
struct Copy {
    what: &'static str,
    title: String,
    reads: &'static str,
}

fn copy(of: &GuideOf, branch: Option<&str>) -> Copy {
    match of {
        GuideOf::Commit { commit } => Copy {
            what: "this commit",
            title: commit.subject.clone(),
            reads: "the commit's message and diff",
        },
        GuideOf::Pull { pull, .. } => Copy {
            what: "this pull request",
            title: pull.title.clone(),
            reads: "the pull request's commits and their diff, fetching them first when they're missing,",
        },
        GuideOf::Changes => Copy {
            what: "these changes",
            title: "Uncommitted changes".into(),
            reads: "the uncommitted changes (staged, unstaged and new files)",
        },
        GuideOf::Branch { label, .. } => Copy {
            what: "this branch",
            title: format!("{} since {label}", branch.unwrap_or("HEAD")),
            reads: "the branch's commits and their diff (not uncommitted changes)",
        },
    }
}

impl GuideView {
    /// The Guided Review agent and its effort, picked here as in Settings (the same keys).
    fn agent_menu() -> impl IntoElement {
        Button::new("review-agent")
            .small()
            .ghost()
            .icon(IconName::Settings2)
            .tooltip("Guided Review agent")
            .dropdown_menu(|menu, window, cx| {
                let s = Settings::get(cx);
                let current = s.review_command.clone();
                let fallback = agent_name(&s.suggest_command);
                let mut menu = menu.label("Agent").item(
                    PopupMenuItem::new(format!("As Commit Messages ({fallback})"))
                        .checked(current.is_none())
                        .on_click(|_, _, cx| Settings::update(cx, |s| s.review_command = None)),
                );
                for p in PRESETS {
                    let command = p.command;
                    menu = menu.item(
                        PopupMenuItem::new(p.label)
                            .checked(current.as_deref() == Some(command))
                            .on_click(move |_, _, cx| {
                                Settings::update(cx, |s| s.review_command = Some(command.into()))
                            }),
                    );
                }
                if let Some(custom) = current.as_deref().filter(|c| preset_of(c).is_none()) {
                    menu = menu.item(
                        PopupMenuItem::new(format!("Custom: {}", program_of(custom)))
                            .checked(true)
                            .disabled(true),
                    );
                }
                let agent = ReviewAgent::from_settings(s);
                if let Some(p) = preset_of(&agent.command)
                    && let Some((_, levels, _)) = p.effort
                {
                    let now = effort_of(p, &agent.efforts);
                    let key = p.key;
                    menu = menu.separator().label("Effort");
                    for level in levels {
                        menu =
                            menu.item(PopupMenuItem::new(*level).checked(now == *level).on_click(
                                move |_, _, cx| {
                                    Settings::update(cx, |s| {
                                        s.review_efforts.insert(key.into(), level.to_string());
                                    })
                                },
                            ));
                    }
                }
                if let Some(p) = preset_of(&agent.command) {
                    let now = model_of(p, &agent.models);
                    let key = p.key;
                    menu = menu.separator().label("Model");
                    for m in model_choices(p, &now) {
                        let label = if m.is_empty() {
                            "CLI default".to_string()
                        } else {
                            m.clone()
                        };
                        let checked = m == now;
                        menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                            move |_, _, cx| {
                                Settings::update(cx, |s| {
                                    s.review_models.insert(key.into(), m.clone());
                                })
                            },
                        ));
                    }
                }
                let language = agent.language.clone();
                menu.separator().submenu(
                    format!(
                        "Language: {}",
                        crate::model::answer::language_name(&language)
                    ),
                    window,
                    cx,
                    move |mut menu, _, _| {
                        let known = REVIEW_LANGUAGES.iter().any(|(name, _)| *name == language);
                        if !known {
                            menu = menu.item(
                                PopupMenuItem::new(language.clone())
                                    .checked(true)
                                    .disabled(true),
                            );
                        }
                        for (name, own) in REVIEW_LANGUAGES {
                            let label = if name == own {
                                name.to_string()
                            } else {
                                format!("{own} ({name})")
                            };
                            menu = menu.item(
                                PopupMenuItem::new(label)
                                    .checked(*name == language)
                                    .on_click(move |_, _, cx| {
                                        Settings::update(cx, |s| {
                                            s.review_language = name.to_string()
                                        })
                                    }),
                            );
                        }
                        menu
                    },
                )
            })
    }

    fn ask_button(
        &self,
        id: &str,
        at: Option<AskAt>,
        label: &'static str,
        what: &'static str,
        risk: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let agent = agent_name(&ReviewAgent::from_settings(Settings::get(cx)).command);
        let tip = if agent == "Claude Code" {
            format!("A new Claude Code session in the terminal, with {what} in its prompt box")
        } else {
            format!("Copies {what} for {agent} to paste")
        };
        let main = Button::new(SharedString::from(format!("{id}-ask")))
            .small()
            .icon(IconName::MessageSquareText)
            .label(label)
            .tooltip(tip)
            .on_click(cx.listener(move |this, _, window, cx| match (at, risk) {
                (Some(at), _) => this.open_ask(at, window, cx),
                (None, Some(i)) => this.risk_ask(i, How::Ask, window, cx),
                _ => {}
            }));
        let weak = cx.weak_entity();
        let more = Button::new(SharedString::from(format!("{id}-more")))
            .small()
            .ghost()
            .icon(IconName::ChevronDown)
            .tooltip(format!("More ways to {}", label.to_lowercase()))
            .dropdown_menu(move |menu, _, _| {
                let item = |name: &'static str, how: How| {
                    let weak = weak.clone();
                    PopupMenuItem::new(name).on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| match (at, risk) {
                            (Some(at), _) => this.other_way(at, how, window, cx),
                            (None, Some(i)) => this.risk_ask(i, how, window, cx),
                            _ => {}
                        });
                    })
                };
                menu.item(
                    item("Paste into Running Agent", How::Paste).icon(IconName::SquareTerminal),
                )
                .item(item("Copy as Prompt", How::Copy).icon(IconName::Copy))
            });
        div()
            .flex()
            .flex_none()
            .items_center()
            .child(main)
            .child(more)
            .into_any_element()
    }

    /// The model Claude Code starts with, each box remembering the last pick; Settings' until one
    /// is picked. None for another agent, which doesn't start with the question in hand.
    pub(crate) fn ask_models(cx: &mut App) -> Option<(Vec<String>, String)> {
        let agent = ReviewAgent::from_settings(Settings::get(cx));
        let p = preset_of(&agent.command).filter(|p| p.key == "claude")?;
        let settings = model_of(p, &agent.models);
        let value = Guides::ask_model(cx).unwrap_or_else(|| settings.clone());
        let mut choices = vec![value.clone(), settings];
        choices.extend(model_choices(p, &value));
        let mut seen = HashSet::new();
        choices.retain(|m| seen.insert(m.clone()));
        Some((choices, value))
    }

    fn ask_model_picker(cx: &mut Context<Self>) -> Option<AnyElement> {
        let (choices, value) = Self::ask_models(cx)?;
        let shown = if value.is_empty() {
            "Default".to_string()
        } else {
            value.clone()
        };
        Some(
            Button::new("ask-model")
                .small()
                .ghost()
                .label(shown)
                .icon(IconName::ChevronDown)
                .tooltip("The model it starts with")
                .dropdown_menu(move |menu, _, _| {
                    choices.iter().fold(menu, |menu, m| {
                        let pick = m.clone();
                        let label = if m.is_empty() {
                            "Default".to_string()
                        } else {
                            m.clone()
                        };
                        menu.item(
                            PopupMenuItem::new(label)
                                .checked(*m == value)
                                .on_click(move |_, _, cx| Guides::set_ask_model(cx, pick.clone())),
                        )
                    })
                })
                .into_any_element(),
        )
    }

    fn ask_box(&self, at: AskAt, cx: &mut Context<Self>) -> Option<AnyElement> {
        let asking = self.asking.as_ref().filter(|a| a.at == at)?;
        let t = cx.app_theme().clone();
        let input = asking.input.clone();
        let send = input.clone();
        Some(
            div()
                .on_key_down(cx.listener(|this, e: &gpui_kit::KeyDownEvent, window, cx| {
                    match e.keystroke.key.as_str() {
                        "escape" => this.close_ask(window, cx),
                        // The box's ↵ asked: where an unhandled key is typed as text (Linux),
                        // it would add a line to the box too.
                        "enter" if !e.keystroke.modifiers.shift => cx.stop_propagation(),
                        _ => {}
                    }
                }))
                .mt(px(8.))
                .p(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.panel)
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(rems(0.75))
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child(format!(
                            "Ask {} about {}",
                            agent_name(&ReviewAgent::from_settings(Settings::get(cx)).command),
                            match at {
                                AskAt::Whole => "this review",
                                AskAt::Section(_) => "this section",
                            }
                        )),
                )
                .child(Textarea::new(&input).small())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(6.))
                        .children(Self::ask_model_picker(cx))
                        .child(div().flex_1())
                        .child(
                            Button::new("ask-skip")
                                .small()
                                .ghost()
                                .label("Skip")
                                .tooltip("Start the session; ask in its prompt box")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.ask(at, String::new(), window, cx)
                                })),
                        )
                        .child(
                            Button::new("ask-send")
                                .small()
                                .primary()
                                .label("Ask")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let q = send.read(cx).value().to_string();
                                    this.ask(at, q, window, cx)
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    fn header(
        &self,
        run: &Option<Run>,
        risks_run: &Option<Run>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.app_theme().clone();
        let program = ReviewAgent::from_settings(Settings::get(cx)).program();
        let branch = self
            .repo
            .as_ref()
            .and_then(|r| r.read(cx).branch().map(str::to_string));
        let c = copy(&self.of, branch.as_deref());
        let title = self
            .guide
            .as_ref()
            .map(|g| g.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or(c.title);
        let running = matches!(run, Some(Run::Running { .. }));
        let finding = matches!(risks_run, Some(Run::Running { .. }));
        let mut buttons = div().flex().flex_none().items_center().gap(px(6.));
        if self.saved.is_some() {
            buttons = buttons.child(self.ask_button(
                "whole",
                Some(AskAt::Whole),
                "Ask Agent",
                "this review",
                None,
                cx,
            ));
        }
        if self.found.is_none() && !finding {
            buttons = buttons.child(
                Button::new("find-risks")
                    .small()
                    .icon(IconName::Bug)
                    .label("Find Risks")
                    .on_click(cx.listener(|this, _, _, cx| this.find_risks(cx))),
            );
        }
        buttons = buttons.child(Self::agent_menu());
        buttons = buttons.child(if running {
            Button::new("cancel")
                .small()
                .icon(IconName::Square)
                .label("Cancel")
                .on_click(cx.listener(|this, _, _, cx| this.cancel(RunKind::Guide, cx)))
        } else if self.saved.is_some() {
            Button::new("regenerate")
                .small()
                .icon(IconName::RotateCw)
                .label("Regenerate")
                .on_click(cx.listener(|this, _, _, cx| this.generate(cx)))
        } else {
            Button::new("generate")
                .small()
                .primary()
                .icon(IconName::Sparkles)
                .label(format!("Ask {program}"))
                .on_click(cx.listener(|this, _, _, cx| this.generate(cx)))
        });
        let saved = self.saved.as_ref();
        let now = now_unix();
        let mut meta: Vec<String> = vec![];
        meta.push(match &self.of {
            GuideOf::Commit { commit } => format!(
                "{} by {}, {}",
                commit.short_sha,
                commit.author_name,
                relative_time(commit.timestamp, now)
            ),
            GuideOf::Pull { pull, .. } => {
                let at = saved
                    .map(|s| format!(" at {}", short(&s.run.head)))
                    .unwrap_or_default();
                format!(
                    "#{} by {}: {} into {}{at}",
                    pull.number, pull.author, pull.head_ref, pull.base_ref
                )
            }
            GuideOf::Changes => {
                let on = branch
                    .as_ref()
                    .map(|b| format!(" on {b}"))
                    .unwrap_or_default();
                let against = saved
                    .map(|s| {
                        if s.run.base == EMPTY_TREE {
                            " against no commit yet".to_string()
                        } else {
                            format!(" against {}", short(&s.run.base))
                        }
                    })
                    .unwrap_or_default();
                format!("Uncommitted changes{on}{against}")
            }
            GuideOf::Branch { label, .. } => {
                let at = saved
                    .map(|s| format!(" at {}", short(&s.run.head)))
                    .unwrap_or_default();
                format!("{} since {label}{at}", branch.as_deref().unwrap_or("HEAD"))
            }
        });
        if let Some(s) = saved {
            let details = if s.run.details.is_empty() {
                String::new()
            } else {
                format!(" ({})", s.run.details.join(", "))
            };
            meta.push(format!(
                "written by {}{details}, {}",
                s.run.program,
                relative_time(s.run.at / 1000, now)
            ));
        }
        let mut counts = div().flex().items_center().gap(px(8.));
        if let Some(g) = &self.guide
            && !g.sections.is_empty()
        {
            let done = (0..g.sections.len()).filter(|i| self.is_done(*i)).count();
            counts = counts.child(
                div()
                    .flex()
                    .gap(px(4.))
                    .child("·")
                    .child(count(done, g.sections.len(), &t))
                    .child("sections reviewed"),
            );
        }
        if let Some(list) = self.file_list()
            && !list.is_empty()
        {
            let viewed = self.viewed;
            counts = counts.child(
                div()
                    .flex()
                    .gap(px(4.))
                    .child("·")
                    .child(count(viewed, list.len(), &t))
                    .child("files viewed"),
            );
        }
        div()
            .flex_none()
            .px(px(20.))
            .pt(px(14.))
            .pb(px(10.))
            .border_b_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(rems(1.2))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(buttons),
            )
            .child(
                div()
                    .mt(px(4.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .text_size(rems(0.75))
                    .text_color(t.muted_foreground)
                    .child(icon(IconName::Sparkles, t.subtle))
                    .child(meta.join(" · "))
                    .child(counts),
            )
            .children(self.ask_box(AskAt::Whole, cx))
            .into_any_element()
    }

    fn notices(
        &self,
        run: &Option<Run>,
        risks_run: &Option<Run>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = cx.app_theme().clone();
        let program = ReviewAgent::from_settings(Settings::get(cx)).program();
        let branch = self
            .repo
            .as_ref()
            .and_then(|r| r.read(cx).branch().map(str::to_string));
        let c = copy(&self.of, branch.as_deref());
        let mut out = vec![];
        match run {
            Some(Run::Running { fetching: true, .. }) => out.push(notice(
                IconName::LoaderCircle,
                t.foreground,
                &t,
                format!(
                    "Fetching the pull request's commits for {program}… {}",
                    elapsed(run)
                ),
            )),
            Some(Run::Running { tail, .. }) => {
                out.push(notice(
                    IconName::LoaderCircle,
                    t.foreground,
                    &t,
                    format!(
                        "Asking {program} to explain {}. It can take a few minutes, and it goes on while you look at other tabs. {}",
                        c.what,
                        elapsed(run)
                    ),
                ));
                out.extend(printed(tail, &t));
            }
            Some(Run::Stopped) => out.push(notice(
                IconName::Square,
                t.foreground,
                &t,
                if self.saved.is_some() {
                    format!(
                        "Stopped before {program} was done: this is the guided review from before."
                    )
                } else {
                    format!("Stopped before {program} was done.")
                },
            )),
            Some(Run::Failed(why)) => out.push(notice(
                IconName::TriangleAlert,
                t.destructive,
                &t,
                format!("Couldn't write a guided review: {why}"),
            )),
            None => {}
        }
        let running = matches!(run, Some(Run::Running { .. }));
        if let Some(s) = &self.saved
            && !running
            && self.moved_from(&s.run.head, s.run.stamp.as_deref(), cx)
        {
            let text = match &self.of {
                GuideOf::Changes => "Outdated: the changes have moved since this review read them. Regenerate to review them as they are now.".to_string(),
                GuideOf::Pull { pull, .. } => format!("Outdated: #{} has moved since this review read it at {}. Regenerate to review where it is now.", pull.number, short(&s.run.head)),
                _ => format!("Outdated: {} has moved since this review read it at {}. Regenerate to review where it is now.", branch.as_deref().unwrap_or("HEAD"), short(&s.run.head)),
            };
            out.push(notice(IconName::TriangleAlert, t.modified, &t, text));
        }
        if let Files::Failed { error, .. } = &self.files {
            out.push(notice(
                IconName::TriangleAlert,
                t.destructive,
                &t,
                format!("Couldn't read the files: {error}"),
            ));
        }
        match risks_run {
            Some(Run::Running { tail, .. }) => out.extend([Some(notice(
                IconName::LoaderCircle,
                t.foreground,
                &t,
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(format!(
                        "Asking {program} to find risks in this change. It goes on while you look at other tabs. {}",
                        elapsed(risks_run)
                    ))
                    .child(
                        Button::new("cancel-risks")
                            .xsmall()
                            .ghost()
                            .icon(IconName::Square)
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(RunKind::Risks, cx))),
                    ),
            )), printed(tail, &t)].into_iter().flatten()),
            Some(Run::Stopped) => out.push(notice(IconName::Square, t.foreground, &t, format!("Stopped before {program} found the risks."))),
            Some(Run::Failed(why)) => out.push(notice(IconName::TriangleAlert, t.destructive, &t, format!("Couldn't find risks: {why}"))),
            None => {}
        }
        out
    }

    /// Markdown the agent wrote. Its parsed state is the view's, made once per text and dropped
    /// with it: a keyed state of the element's, made again for each new tab, kept memory growing.
    fn markdown(
        &self,
        id: impl Into<SharedString>,
        text: &str,
        lines: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.into();
        let state = self
            .md
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| cx.new(|cx| TextViewState::markdown(text, cx)))
            .clone();
        let mut view = TextView::new(&state);
        if let Some(n) = lines {
            view = view.max_lines(n);
        }
        view.into_any_element()
    }

    fn risks_block(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let found = self.found.as_ref()?;
        let t = cx.app_theme().clone();
        let finding = self
            .id
            .as_ref()
            .is_some_and(|id| Guides::is_running(cx, RunKind::Risks, id));
        let now = now_unix();
        let at = if matches!(self.of, GuideOf::Changes) {
            String::new()
        } else {
            format!(" at {}", short(&found.head))
        };
        let mut block = div()
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border)
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(IconName::Bug, t.subtle))
                    .child(
                        div()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_size(rems(0.82))
                            .child(match &self.risks {
                                Some(r) => format!("Risks ({})", r.len()),
                                None => "Risks".into(),
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.75))
                            .text_color(t.muted_foreground)
                            .child(format!(
                                "· found by {}{at}, {}",
                                found.program,
                                relative_time(found.at / 1000, now)
                            )),
                    )
                    .when(!finding, |d| {
                        d.child(
                            Button::new("find-again")
                                .xsmall()
                                .ghost()
                                .icon(IconName::RotateCw)
                                .label("Find Again")
                                .on_click(cx.listener(|this, _, _, cx| this.find_risks(cx))),
                        )
                    }),
            );
        if !finding && self.moved_from(&found.head, found.stamp.as_deref(), cx) {
            block = block.child(notice(
                IconName::TriangleAlert,
                t.modified,
                &t,
                "Outdated: the change has moved since these were found. Find again to check where it is now.",
            ));
        }
        block = match &self.risks {
            None => block
                .child(notice(
                    IconName::TriangleAlert,
                    t.foreground,
                    &t,
                    "The answer wasn't in the shape asked for; here it is as written.",
                ))
                .child(self.markdown("risks-raw", &found.text, None, cx)),
            Some(r) if r.is_empty() => block.child(
                div()
                    .text_size(rems(0.78))
                    .text_color(t.muted_foreground)
                    .child("No risks found in this change."),
            ),
            Some(risks) => {
                let risks = risks.clone();
                block.children(
                    risks
                        .iter()
                        .enumerate()
                        .map(|(i, r)| self.risk_row(i, r, cx))
                        .collect::<Vec<_>>(),
                )
            }
        };
        Some(block.into_any_element())
    }

    fn risk_row(&self, i: usize, r: &Risk, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let place = if r.path.is_empty() {
            None
        } else {
            let line = r.line.map(|l| format!(":{l}")).unwrap_or_default();
            let removed = if r.line.is_some() && r.side == NoteSide::Old {
                " (removed)"
            } else {
                ""
            };
            Some(format!("{}{line}{removed}", r.path))
        };
        let path = r.path.clone();
        div()
            .pt(px(8.))
            .border_t_1()
            .border_color(t.border)
            .flex()
            .items_start()
            .gap(px(10.))
            .child(
                div()
                    .mt(px(5.))
                    .size(px(8.))
                    .flex_none()
                    .rounded_full()
                    .bg(parts::severity_color(r.severity, &t)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(rems(0.78))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap(px(8.))
                            .child(div().font_weight(gpui_kit::FontWeight::MEDIUM).child(
                                if r.title.is_empty() {
                                    "Untitled".to_string()
                                } else {
                                    r.title.clone()
                                },
                            ))
                            .children(place.map(|p| {
                                div()
                                    .id(SharedString::from(format!("risk-at-{i}")))
                                    .min_w_0()
                                    .truncate()
                                    .font_family(t.fonts.code.clone())
                                    .text_size(rems(0.72))
                                    .text_color(t.primary)
                                    .cursor_pointer()
                                    .hover(|s| s.underline())
                                    .child(p)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.go_to_file(&path, window, cx)
                                    }))
                            })),
                    )
                    .when(!r.why.is_empty(), |d| {
                        d.child(
                            div()
                                .mt(px(2.))
                                .text_color(t.muted_foreground)
                                .child(r.why.clone()),
                        )
                    })
                    .when(!r.check.is_empty(), |d| {
                        d.child(
                            div()
                                .mt(px(2.))
                                .text_size(rems(0.75))
                                .text_color(t.subtle)
                                .child(format!("Check: {}", r.check)),
                        )
                    }),
            )
            .child(self.ask_button(
                &format!("risk-{i}"),
                None,
                "Send to Agent",
                "this risk",
                Some(i),
                cx,
            ))
            .into_any_element()
    }

    /// A file's diff at the top, where the guide shows it; else its own tab.
    pub(super) fn go_to_file(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(list) = self.file_list() else { return };
        let all: HashSet<&str> = list.iter().map(|f| f.path.as_str()).collect();
        let Some(path) = match_path(path, &all).map(str::to_string) else {
            return;
        };
        if !self.in_diff.iter().any(|(p, _, _)| *p == path)
            && self.active_filter() != Filter::default()
        {
            self.filter = Filter::default();
            self.fill_diff(cx);
        }
        if let Some((_, _, key)) = self.in_diff.iter().find(|(p, _, _)| *p == path) {
            let key = key.clone();
            self.stacked
                .update(cx, |s, cx| s.restore_scroll(&key, px(0.), cx));
        } else if let Some(sel) = list
            .iter()
            .find(|f| f.path == path)
            .and_then(|f| self.file_selection(f))
            && let Some(ws) = self.workspace.upgrade()
        {
            ws.update(cx, |ws, cx| ws.open(sel, false, window, cx));
        }
    }

    fn filter_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let f = self.active_filter();
        let cats = self.categories();
        let (highs, total) = (self.highs(), self.total());
        let chip = |id: SharedString, on: bool, t: &AppTheme| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(5.))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(5.))
                .border_1()
                .text_size(rems(0.75))
                .cursor_pointer()
                .when(on, |d| {
                    d.border_color(t.border_strong)
                        .bg(t.active)
                        .text_color(t.foreground)
                })
                .when(!on, |d| {
                    d.border_color(t.border)
                        .text_color(t.muted_foreground)
                        .hover(|s| s.bg(t.hover).text_color(t.foreground))
                })
        };
        let many = cats.len() > 1;
        let mut row = div().flex().flex_wrap().items_center().gap(px(6.));
        if many {
            row = row.child(
                chip("cat-all".into(), f.category.is_none(), &t)
                    .child("All")
                    .child(div().text_color(t.subtle).child(total.to_string()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let high = this.filter.high;
                        this.set_filter(
                            Filter {
                                category: None,
                                high,
                            },
                            cx,
                        )
                    })),
            );
            for (c, n) in cats {
                row = row.child(
                    chip(format!("cat-{}", c.key()).into(), f.category == Some(c), &t)
                        .child(icon(category_icon(c), category_tone(c, &t)))
                        .child(c.label())
                        .child(div().text_color(t.subtle).child(n.to_string()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let high = this.filter.high;
                            this.set_filter(
                                Filter {
                                    category: Some(c),
                                    high,
                                },
                                cx,
                            )
                        })),
                );
            }
        }
        if highs > 0 && highs < total {
            row = row.child(
                chip("high".into(), f.high, &t)
                    .when(many, |d| d.ml(px(8.)))
                    .child(icon(IconName::Signal, t.destructive))
                    .child("High")
                    .child(div().text_color(t.subtle).child(highs.to_string()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let f = Filter {
                            high: !this.filter.high,
                            ..this.filter
                        };
                        this.set_filter(f, cx)
                    })),
            );
        }
        row.into_any_element()
    }

    fn card(&self, i: usize, s: &Section, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let n = i + 1;
        let total = self.total();
        let done = self.is_done(i);
        let named = self
            .placed
            .as_ref()
            .and_then(|p| p.named.get(i))
            .cloned()
            .unwrap_or_default();
        let open = self.open.contains(&i);
        let long = s.summary.lines().count() > 3 || s.summary.chars().count() > 260;
        let title = title_text(s);
        let mut body = div().flex().flex_col().gap(px(8.)).mt(px(10.));
        if !done {
            if !s.summary.is_empty() {
                body = body.child(
                    div()
                        .text_size(rems(0.8))
                        .child(self.markdown(
                            format!("summary-{i}"),
                            &s.summary,
                            (!open).then_some(3),
                            cx,
                        ))
                        .when(long || open, |d| {
                            d.child(
                                div()
                                    .id(SharedString::from(format!("more-{i}")))
                                    .mt(px(2.))
                                    .text_size(rems(0.75))
                                    .text_color(t.primary)
                                    .cursor_pointer()
                                    .child(if open { "Less" } else { "More" })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.open.remove(&i) {
                                            this.open.insert(i);
                                        }
                                        cx.notify();
                                    })),
                            )
                        }),
                );
            }
            if !s.check.is_empty() {
                body = body.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(6.))
                        .text_size(rems(0.75))
                        .text_color(t.muted_foreground)
                        .child(
                            div()
                                .pt(px(1.))
                                .child(icon(IconName::CircleCheck, t.subtle)),
                        )
                        .child(
                            div().min_w_0().flex_1().child(
                                div()
                                    .flex()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_color(t.foreground)
                                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                                            .child("Check:"),
                                    )
                                    .child(s.check.clone()),
                            ),
                        ),
                );
            }
            if !s.risk.is_empty() {
                body = body.child(notice(
                    IconName::TriangleAlert,
                    t.modified,
                    &t,
                    s.risk.clone(),
                ));
            }
            for note in &s.file_notes {
                body = body.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(6.))
                        .text_size(rems(0.75))
                        .child(div().pt(px(1.)).child(icon(
                            IconName::MessageSquareText,
                            if note.critical {
                                t.destructive
                            } else {
                                t.subtle
                            },
                        )))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .font_family(t.fonts.code.clone())
                                        .text_size(rems(0.7))
                                        .text_color(t.subtle)
                                        .child(note.path.clone()),
                                )
                                .child(note.text.clone()),
                        ),
                );
            }
            for (j, f) in named.into_iter().enumerate() {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(rems(0.75))
                        .child(gitviber_ui::widgets::FileIcon::file(&f.path))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_family(t.fonts.code.clone())
                                .text_color(t.subtle)
                                .child(f.path.clone()),
                        )
                        .child(match f.at {
                            Some(at) => div()
                                .id(SharedString::from(format!("named-{i}-{j}")))
                                .flex_none()
                                .text_color(t.primary)
                                .cursor_pointer()
                                .child(format!("in section {}", section_badge(at)))
                                .on_click(cx.listener(move |this, _, _, cx| this.go_to(at, cx)))
                                .into_any_element(),
                            None => div()
                                .flex_none()
                                .text_color(t.subtle)
                                .child("not in this diff")
                                .into_any_element(),
                        }),
                );
            }
        }
        let current = self.current == Some(n);
        div()
            .px(px(16.))
            .py(px(12.))
            .border_t_1()
            .border_color(t.border)
            .when(current, |d| d.bg(t.hover))
            .when(done, |d| d.opacity(0.6))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .child(
                        div()
                            .id(SharedString::from(format!("title-{i}")))
                            .flex_1()
                            .min_w_0()
                            .text_size(rems(0.92))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .hover(|s| s.text_color(t.primary))
                            .child(title.clone())
                            .on_click(cx.listener(move |this, _, _, cx| this.go_to(n, cx))),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_family(t.fonts.code.clone())
                            .text_size(rems(0.68))
                            .text_color(t.subtle)
                            .child(format!("{} / {}", section_badge(n), section_badge(total))),
                    ),
            )
            .child(
                div()
                    .mt(px(6.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(12.))
                    .child(category_tag(s.category, &t))
                    .child(importance_tag(s.importance, &t))
                    .child(
                        Checkbox::new(SharedString::from(format!("done-{i}")))
                            .checked(done)
                            .label("Reviewed")
                            .tooltip(format!("Reviewed: {title}"))
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.mark_done(i, *checked, cx)
                            })),
                    )
                    .child(div().ml_auto().child(self.ask_button(
                        &format!("section-{i}"),
                        Some(AskAt::Section(i)),
                        "Ask Agent",
                        "this section",
                        None,
                        cx,
                    ))),
            )
            .child(body)
            .children(self.ask_box(AskAt::Section(i), cx))
            .into_any_element()
    }

    fn other_card(&self, rest: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let other = Section::other();
        div()
            .px(px(16.))
            .py(px(12.))
            .border_t_1()
            .border_color(t.border)
            .when(self.current == Some(0), |d| d.bg(t.hover))
            .child(
                div()
                    .id("title-other")
                    .text_size(rems(0.92))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(|s| s.text_color(t.primary))
                    .child(other.title)
                    .on_click(cx.listener(|this, _, _, cx| this.go_to(0, cx))),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_size(rems(0.78))
                    .text_color(t.muted_foreground)
                    .child(format!(
                        "{} · {rest} {}",
                        other.summary,
                        if rest == 1 { "file" } else { "files" }
                    )),
            )
            .into_any_element()
    }

    fn guide_column(&self, guide: &Rc<Guide>, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let mut children: Vec<AnyElement> = vec![];
        let section_h = |s: &'static str, t: &AppTheme| {
            div()
                .mb(px(6.))
                .text_size(rems(0.82))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(t.foreground)
                .child(s)
        };
        if !guide.summary.is_empty() {
            children.push(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .child(section_h("Overview", &t))
                    .child(div().text_size(rems(0.82)).child(self.markdown(
                        "overview",
                        &guide.summary,
                        None,
                        cx,
                    )))
                    .into_any_element(),
            );
        }
        if guide.has_diagram() {
            let weak = cx.weak_entity();
            let go: diagram::OnSection = Rc::new(move |n, _, cx| {
                let _ = weak.update(cx, |this, cx| this.go_to(n, cx));
            });
            children.push(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .child(section_h("Before / after", &t))
                    .child(diagram::diagram(
                        &guide.models,
                        &guide.flows,
                        self.legacy.clone().map(|v| v.into_any_element()),
                        &t,
                        &go,
                    ))
                    .into_any_element(),
            );
        }
        if let Some(risks) = self.risks_block(cx) {
            children.push(div().px(px(16.)).py(px(8.)).child(risks).into_any_element());
        }
        if self.filter_shown() {
            children.push(
                div()
                    .px(px(16.))
                    .py(px(8.))
                    .child(self.filter_row(cx))
                    .into_any_element(),
            );
        }
        let program = self
            .saved
            .as_ref()
            .map(|s| s.run.program.clone())
            .unwrap_or_default();
        let rest = self.placed.as_ref().map_or(0, |p| p.rest.len());
        if self.rest_notice() {
            let text = if rest == 1 {
                format!("1 file wasn't placed in a section by {program}.")
            } else {
                format!("{rest} files weren't placed in a section by {program}.")
            };
            children.push(
                div()
                    .px(px(16.))
                    .py(px(6.))
                    .child(notice(
                        IconName::TriangleAlert,
                        t.foreground,
                        &t,
                        div().flex().flex_wrap().gap(px(6.)).child(text).child(
                            div()
                                .id("see-other")
                                .text_color(t.primary)
                                .cursor_pointer()
                                .child("See them under Other changes")
                                .on_click(cx.listener(|this, _, _, cx| this.go_to(0, cx))),
                        ),
                    ))
                    .into_any_element(),
            );
        }
        let mut shown = 0;
        for (i, s) in guide.sections.iter().enumerate() {
            if self.fits(s) {
                shown += 1;
                children.push(self.card(i, s, cx));
            }
        }
        let f = self.active_filter();
        if f != Filter::default() && shown == 0 {
            let what = f
                .category
                .map(|c| format!("No {} section is high importance.", c.label()))
                .unwrap_or_else(|| "No section fits.".into());
            children.push(
                div()
                    .px(px(16.))
                    .py(px(10.))
                    .flex()
                    .gap(px(6.))
                    .text_size(rems(0.8))
                    .text_color(t.muted_foreground)
                    .child(what)
                    .child(
                        div()
                            .id("show-all")
                            .text_color(t.primary)
                            .cursor_pointer()
                            .child("Show all")
                            .on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.set_filter(Filter::default(), cx)
                                }),
                            ),
                    )
                    .into_any_element(),
            );
        }
        if rest > 0 && f == Filter::default() {
            children.push(self.other_card(rest, cx));
        }
        div()
            .id("guide-column")
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pb(px(24.))
            .children(children)
            .into_any_element()
    }

    fn diffs(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let text = match &self.files {
            Files::Ready { .. } if !self.in_diff.is_empty() => {
                return div()
                    .size_full()
                    .child(self.stacked.clone())
                    .into_any_element();
            }
            Files::Ready { .. } => "Every section shown is reviewed.",
            Files::Loading { .. } | Files::None => "Loading files…",
            Files::Failed { .. } => "",
        };
        div()
            .size_full()
            .p(px(16.))
            .text_size(rems(0.78))
            .text_color(t.subtle)
            .child(text)
            .into_any_element()
    }
}

impl Render for GuideView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        if self.legacy.is_none()
            && let Some(g) = self.guide.as_ref().filter(|g| !g.diagram.is_empty())
        {
            let source = format!("```mermaid\n{}\n```\n", g.diagram);
            self.legacy = Some(
                cx.new(|cx| MarkdownView::inline(&source, MarkdownBase::default(), window, cx)),
            );
        }
        let id = self.id.clone().unwrap_or_default();
        let run = Guides::run(cx, RunKind::Guide, &id);
        let risks_run = Guides::run(cx, RunKind::Risks, &id);
        let header = self.header(&run, &risks_run, cx);
        let notices = self.notices(&run, &risks_run, cx);
        let running = matches!(run, Some(Run::Running { .. }));
        let body: AnyElement = match (&self.saved, self.guide.clone()) {
            (None, _) => {
                let program = ReviewAgent::from_settings(Settings::get(cx)).program();
                let branch = self
                    .repo
                    .as_ref()
                    .and_then(|r| r.read(cx).branch().map(str::to_string));
                let c = copy(&self.of, branch.as_deref());
                div()
                    .id("guide-empty")
                    .size_full()
                    .overflow_y_scroll()
                    .px(px(20.))
                    .py(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .children(self.risks_block(cx))
                    .when(!running, |d| {
                        d.child(
                            div()
                                .text_size(rems(0.82))
                                .text_color(t.muted_foreground)
                                .child(format!("{program} reads {} and explains it as sections in the order to review them, by category, with a diagram when a flow or data model changes.", c.reads)),
                        )
                    })
                    .into_any_element()
            }
            (Some(saved), None) => div()
                .id("guide-raw")
                .size_full()
                .overflow_y_scroll()
                .px(px(20.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(notice(
                    IconName::TriangleAlert,
                    t.foreground,
                    &t,
                    "The answer wasn't in the shape asked for; here it is as written.",
                ))
                .child(self.markdown("guide-raw-text", &saved.run.text, None, cx))
                .into_any_element(),
            (Some(_), Some(guide)) => {
                let items = self.nav_items();
                let nav = items.len() > 1;
                // The first frame goes by the window's width.
                let width = match self.width.get() {
                    w if w > Pixels::ZERO => w,
                    _ => window.viewport_size().width,
                };
                let rail = nav && width >= nav::RAIL_FROM;
                let (seen, me) = (self.width.clone(), cx.entity_id());
                let measure = gpui_kit::canvas(
                    move |bounds, _, cx| {
                        let was = seen.replace(bounds.size.width);
                        if (was >= nav::RAIL_FROM) != (bounds.size.width >= nav::RAIL_FROM) {
                            // Past the frame: a view told during one isn't drawn again.
                            cx.defer(move |cx| cx.notify(me));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full();
                div()
                    .relative()
                    .flex()
                    .size_full()
                    .min_h_0()
                    .child(measure)
                    .when(rail, |d| d.child(self.nav_rail(&items, &guide, cx)))
                    .child(
                        div()
                            .w(gpui_kit::relative(0.4))
                            .min_w(px(340.))
                            .max_w(px(580.))
                            .h_full()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .border_r_1()
                            .border_color(t.border)
                            .when(nav && !rail, |d| {
                                d.child(self.nav_chips(&items, &guide, cx))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .child(self.guide_column(&guide, cx)),
                            ),
                    )
                    .child(div().flex_1().min_w_0().h_full().child(self.diffs(cx)))
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.code(t.background))
            .text_color(t.foreground)
            .child(header)
            .when(!notices.is_empty(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .px(px(20.))
                        .py(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .border_b_1()
                        .border_color(t.border)
                        .children(notices),
                )
            })
            .child(div().flex_1().min_h_0().child(body))
    }
}
