//! git's and ssh's questions (a password, a key's passphrase, a new host's fingerprint), asked in
//! a dialog of the window in front and answered through core. A prompt is never logged or kept.

use std::cell::Cell;
use std::rc::Rc;

use gitviber_core::askpass::Prompt;
use gitviber_core::{Event, api};
use gitviber_ui::memory::Live;
use gitviber_ui::{ActiveAppTheme, CoreEvents};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Focusable as _, Global, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, WeakEntity, Window, div, px, rems,
};

/// How a prompt is asked: a field (masked for secrets), a yes/no, or a notice to dismiss.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptForm {
    Text {
        title: String,
        label: String,
    },
    Secret {
        title: String,
        label: String,
    },
    Confirm {
        title: String,
        details: String,
        question: String,
        host_key: bool,
    },
    Notice {
        title: String,
        details: String,
    },
}

/// `https://me@github.com/a/b` → `github.com`.
fn host_of(url: &str) -> String {
    let rest = url
        .split_once("://")
        .filter(|(s, _)| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphabetic() || c == '+' || c == '-')
        })
        .map_or(url, |(_, r)| r);
    let rest = match rest.split_once('@') {
        Some((user, r)) if !user.contains('/') => r,
        _ => rest,
    };
    rest.split(['/', ':']).next().unwrap_or("").to_string()
}

/// "git push → github.com": what is asking, so a prompt naming some other host stands out.
pub fn prompt_context(p: &Prompt) -> String {
    if p.hosts.is_empty() {
        p.label.clone()
    } else {
        format!("{} → {}", p.label, p.hosts.join(", "))
    }
}

/// The question line of a yes/no: "… (yes/no…)?" with nothing after it.
fn yes_no(line: &str) -> bool {
    line.ends_with(")?")
        && line.rfind("(yes/no").is_some_and(|i| {
            !line[i..].contains('\n') && !line[i + 1..line.len() - 2].contains(')')
        })
}

/// git's and OpenSSH's English wording (core runs git in English; ssh has no translations). Every
/// pattern matches a whole prompt: the friendly forms are only for text git or ssh wrote whole.
pub fn prompt_form(p: &Prompt) -> PromptForm {
    let trimmed = p.text.trim();
    let secret = |title: String, label: String| PromptForm::Secret { title, label };
    // OpenSSH 8.4+ puts "(user@host) " before whatever the server asks: the server's own words,
    // which may imitate any prompt, so shown as they are.
    if let Some(rest) = trimmed.strip_prefix('(')
        && let Some((who, _)) = rest.split_once(") ")
        && !who.is_empty()
        && !who.contains(char::is_whitespace)
        && !who.contains('(')
    {
        let host = who.rsplit_once('@').map_or(who, |(_, h)| h);
        return secret(format!("{host} asks"), trimmed.to_string());
    }
    if p.kind.as_deref() == Some("none") {
        return PromptForm::Notice {
            title: "ssh".into(),
            details: trimmed.to_string(),
        };
    }
    let mut lines: Vec<&str> = trimmed.split('\n').collect();
    let host_key = lines.len() >= 2
        && yes_no(lines[lines.len() - 1])
        && lines[0].starts_with("The authenticity of host '")
        && lines[0].contains("' can't be established.")
        && lines[0]["The authenticity of host '".len()..]
            .split(['\'', ' '])
            .next()
            .is_some_and(|h| !h.is_empty());
    let single_yes_no = lines.len() == 1 && yes_no(lines[0]);
    if p.kind.as_deref() == Some("confirm") || host_key || single_yes_no {
        let question = lines.pop().unwrap_or_default().to_string();
        let title = if host_key {
            let host = lines[0]["The authenticity of host '".len()..]
                .split(['\'', ' '])
                .next()
                .unwrap_or_default();
            format!("Connect to {host}?")
        } else {
            "Confirm".into()
        };
        return PromptForm::Confirm {
            title,
            details: lines.join("\n").trim().to_string(),
            question,
            host_key,
        };
    }
    // A login for another host than the command's (a hook's, a redirect's) isn't dressed up.
    let known = |host: &str| p.hosts.iter().any(|h| h == host);
    for (word, label) in [("Username", "Username"), ("Password", "Password or token")] {
        if let Some(url) = trimmed
            .strip_prefix(&format!("{word} for '"))
            .and_then(|r| r.strip_suffix("':"))
            && !url.contains(['\'', '\n'])
            && known(&host_of(url))
        {
            let title = format!("Sign in to {}", host_of(url));
            return if word == "Username" {
                PromptForm::Text {
                    title,
                    label: label.into(),
                }
            } else {
                secret(title, label.into())
            };
        }
    }
    if let Some(key) = trimmed
        .strip_prefix("Enter passphrase for ")
        .and_then(|r| r.strip_prefix("key ").or(Some(r)))
        .and_then(|r| r.strip_prefix('\''))
        .and_then(|r| r.strip_suffix("':"))
        && !key.contains(['\'', '\n'])
    {
        return secret(
            "Unlock your SSH key".into(),
            format!("Passphrase for {key}"),
        );
    }
    if let Some(who) = trimmed.strip_suffix("'s password:")
        && let Some((user, host)) = who.split_once('@')
        && !user.is_empty()
        && !host.is_empty()
        && !who.contains(|c: char| c.is_whitespace() || c == '\'')
        && !host.contains('@')
        && known(host)
    {
        return secret(format!("Sign in to {host}"), "Password".into());
    }
    // Unknown words may still ask for a secret (a PIN, a one-time code): masked is the safe side.
    secret("git asks".into(), trimmed.to_string())
}

/// A prompt's dialog, as it opened.
struct Showing {
    id: u64,
    window: AnyWindowHandle,
    /// Answered elsewhere, timed out, or its command stopped waiting: it goes once uncovered.
    done: bool,
    view: WeakEntity<AskView>,
}

/// The prompts showing, in the order they opened: a later one covers an earlier one, and only
/// the top dialog can be closed. Forms that finished under a prompt close once it's gone.
#[derive(Default)]
struct Asking {
    showing: Vec<Showing>,
    waiting: Vec<AnyWindowHandle>,
    _events: Option<Subscription>,
}
impl Global for Asking {}

/// Listens for git's prompts, once; until this runs, core declines them at once.
pub fn install(cx: &mut App) {
    if !gitviber_ui::has_core(cx)
        || cx
            .try_global::<Asking>()
            .is_some_and(|a| a._events.is_some())
    {
        return;
    }
    let events = CoreEvents::global(cx);
    let sub = cx.subscribe(&events, |_, event: &Event, cx| match event {
        Event::Askpass(p) => show(p.clone(), cx),
        Event::AskpassDone(id) => done(*id, cx),
        _ => {}
    });
    cx.set_global(Asking {
        showing: vec![],
        waiting: vec![],
        _events: Some(sub),
    });
    let core = gitviber_ui::core(cx);
    cx.background_spawn(async move { api::sync::askpass_ready(&core) })
        .detach();
}

fn answer(id: u64, value: Option<String>, cx: &mut App) {
    let core = gitviber_ui::core(cx);
    cx.background_spawn(async move { api::sync::askpass_answer(&core, id, value) })
        .detach();
}

/// Closes the form a dialog shows once its action went through. A prompt that opened over it
/// meanwhile (a background fetch's) is the top dialog: the form waits for it to go.
pub fn close_form(window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    let covered = cx
        .try_global::<Asking>()
        .is_some_and(|a| a.showing.iter().any(|s| s.window == handle));
    if covered {
        cx.default_global::<Asking>().waiting.push(handle);
    } else {
        window.close_dialog(cx);
    }
}

/// The prompt `id`'s dialog closed (its buttons, Escape): whatever it covered may go now.
fn closed(id: u64, window: &mut Window, cx: &mut App) {
    cx.default_global::<Asking>().showing.retain(|s| s.id != id);
    // After the close that's running: closing more under it would take the wrong one.
    window.defer(cx, settle);
}

/// Closes the dialogs now on top that were only waiting to go: finished prompts, then forms.
fn settle(window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    loop {
        let a = cx.default_global::<Asking>();
        let top = a.showing.iter().rposition(|s| s.window == handle);
        match top {
            Some(i) if a.showing[i].done => {
                a.showing.remove(i);
                window.close_dialog(cx);
            }
            Some(_) => return,
            None => break,
        }
    }
    let a = cx.default_global::<Asking>();
    let forms = a.waiting.iter().filter(|w| **w == handle).count();
    a.waiting.retain(|w| *w != handle);
    for _ in 0..forms {
        window.close_dialog(cx);
    }
}

fn done(id: u64, cx: &mut App) {
    let a = cx.default_global::<Asking>();
    let Some(i) = a.showing.iter().position(|s| s.id == id) else {
        return;
    };
    a.showing[i].done = true;
    let (handle, view) = (a.showing[i].window, a.showing[i].view.clone());
    let top = a.showing.iter().rposition(|s| s.window == handle) == Some(i);
    let _ = view.update(cx, |v, cx| {
        v.replied = true;
        v.stopped = true;
        cx.notify();
    });
    if top {
        let _ = handle.update(cx, |_, window, cx| settle(window, cx));
    }
}

fn show(p: Prompt, cx: &mut App) {
    let Some(handle) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
    else {
        return answer(p.id, None, cx);
    };
    let id = p.id;
    let opened = handle.update(cx, |_, window, cx| {
        let form = prompt_form(&p);
        let view = cx.new(|cx| AskView::new(p, form.clone(), window, cx));
        let weak = view.downgrade();
        let focus = view.read(cx).input.as_ref().map(|i| i.focus_handle(cx));
        let first = Cell::new(true);
        let answered = Rc::new(Cell::new(false));
        let title = match &form {
            PromptForm::Text { title, .. }
            | PromptForm::Secret { title, .. }
            | PromptForm::Confirm { title, .. }
            | PromptForm::Notice { title, .. } => title.clone(),
        };
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false)
                && let Some(f) = &focus
            {
                f.focus(window, cx);
            }
            let (v1, v2, v3) = (view.clone(), view.clone(), view.clone());
            let (a1, a2, a3) = (answered.clone(), answered.clone(), answered.clone());
            let (ok, cancel) = match &view.read(cx).form {
                PromptForm::Confirm { host_key, .. } => (
                    if *host_key {
                        "Trust and connect"
                    } else {
                        "Yes"
                    },
                    "No",
                ),
                PromptForm::Notice { .. } => ("OK", ""),
                _ => ("Continue", "Cancel"),
            };
            let footer = DialogFooter::new()
                .when_cancel(!cancel.is_empty(), move |f| {
                    f.child(Button::new("askpass-cancel").label(cancel).on_click(
                        move |_, window, cx| {
                            a1.set(true);
                            v1.update(cx, |v, cx| v.reply(false, cx));
                            window.close_dialog(cx);
                            closed(id, window, cx);
                        },
                    ))
                })
                .child(Button::new("askpass-ok").primary().label(ok).on_click(
                    move |_, window, cx| {
                        a2.set(true);
                        v2.update(cx, |v, cx| v.reply(true, cx));
                        window.close_dialog(cx);
                        closed(id, window, cx);
                    },
                ));
            dialog
                .title(title.clone())
                .w(px(440.))
                .child(view.clone())
                .footer(footer)
                .on_close(move |_, window, cx| {
                    if !a3.replace(true) {
                        v3.update(cx, |v, cx| v.reply(false, cx));
                    }
                    closed(id, window, cx);
                })
        });
        weak
    });
    match opened {
        Ok(view) => cx.default_global::<Asking>().showing.push(Showing {
            id,
            window: handle,
            done: false,
            view,
        }),
        Err(_) => answer(id, None, cx),
    }
}

/// The footer's Cancel only where there's a choice.
trait WhenCancel: Sized {
    fn when_cancel(self, on: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if on { f(self) } else { self }
    }
}
impl WhenCancel for DialogFooter {}

pub struct AskView {
    prompt: Prompt,
    form: PromptForm,
    input: Option<Entity<InputState>>,
    replied: bool,
    /// git stopped waiting (timed out, its command ended) while another dialog covered this one.
    stopped: bool,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl AskView {
    fn new(prompt: Prompt, form: PromptForm, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = match &form {
            PromptForm::Text { .. } => Some(cx.new(|cx| InputState::new(window, cx))),
            PromptForm::Secret { .. } => {
                Some(cx.new(|cx| InputState::new(window, cx).masked(true)))
            }
            _ => None,
        };
        let subs = input
            .iter()
            .map(|i| {
                cx.subscribe_in(
                    i,
                    window,
                    |this: &mut Self, _, e: &InputEvent, window, cx| {
                        if let InputEvent::PressEnter { .. } = e {
                            this.reply(true, cx);
                            window.close_dialog(cx);
                            closed(this.prompt.id, window, cx);
                        }
                    },
                )
            })
            .collect();
        AskView {
            prompt,
            form,
            input,
            replied: false,
            stopped: false,
            _subs: subs,
            _live: Live::new("Askpass"),
        }
    }

    fn reply(&mut self, ok: bool, cx: &mut Context<Self>) {
        if self.replied {
            return;
        }
        self.replied = true;
        let value = match (&self.form, ok) {
            (_, false) => None,
            (PromptForm::Confirm { .. }, true) => Some("yes".to_string()),
            (PromptForm::Notice { .. }, true) => Some(String::new()),
            (_, true) => self.input.as_ref().map(|i| i.read(cx).value().to_string()),
        };
        // The typed secret goes out once and isn't kept in the field.
        answer(self.prompt.id, value, cx);
    }
}

impl Render for AskView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let context = if self.stopped {
            "git stopped waiting for this answer: close it.".to_string()
        } else {
            prompt_context(&self.prompt)
        };
        let body = match &self.form {
            PromptForm::Text { label, .. } | PromptForm::Secret { label, .. } => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_color(t.muted_foreground).child(label.clone()))
                .children(self.input.as_ref().map(Input::new)),
            PromptForm::Confirm {
                details, question, ..
            } => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .font_family(t.fonts.code.clone())
                        .text_size(rems(0.6875))
                        .text_color(t.muted_foreground)
                        .child(details.clone()),
                )
                .child(div().child(question.clone())),
            PromptForm::Notice { details, .. } => div().child(details.clone()),
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .text_size(rems(0.75))
            .child(
                div()
                    .text_size(rems(0.6875))
                    .text_color(t.subtle)
                    .child(context),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(text: &str, kind: Option<&str>, hosts: &[&str]) -> PromptForm {
        prompt_form(&Prompt {
            id: 1,
            text: text.into(),
            kind: kind.map(Into::into),
            label: "git push".into(),
            op: None,
            hosts: hosts.iter().map(|h| h.to_string()).collect(),
        })
    }
    const HOSTS: &[&str] = &["github.com", "git.example.com"];

    fn title(f: &PromptForm) -> &str {
        match f {
            PromptForm::Text { title, .. }
            | PromptForm::Secret { title, .. }
            | PromptForm::Confirm { title, .. }
            | PromptForm::Notice { title, .. } => title,
        }
    }

    #[test]
    fn gits_https_login_asks_a_username_then_a_masked_password() {
        assert_eq!(
            form("Username for 'https://github.com': ", None, HOSTS),
            PromptForm::Text {
                title: "Sign in to github.com".into(),
                label: "Username".into()
            }
        );
        assert_eq!(
            form(
                "Password for 'https://me@git.example.com:8443': ",
                None,
                HOSTS
            ),
            PromptForm::Secret {
                title: "Sign in to git.example.com".into(),
                label: "Password or token".into()
            }
        );
    }

    #[test]
    fn sshs_passphrase_and_password_prompts_are_masked() {
        assert_eq!(
            form(
                "Enter passphrase for key '/Users/me/.ssh/id_ed25519': ",
                None,
                HOSTS
            ),
            PromptForm::Secret {
                title: "Unlock your SSH key".into(),
                label: "Passphrase for /Users/me/.ssh/id_ed25519".into()
            }
        );
        assert_eq!(
            form("git@git.example.com's password: ", None, HOSTS),
            PromptForm::Secret {
                title: "Sign in to git.example.com".into(),
                label: "Password".into()
            }
        );
        assert!(matches!(
            form("Verification code: ", None, HOSTS),
            PromptForm::Secret { .. }
        ));
    }

    #[test]
    fn a_new_hosts_key_is_a_yes_no_with_its_fingerprint() {
        let text = "The authenticity of host 'github.com (140.82.121.4)' can't be established.
ED25519 key fingerprint is: SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU
This key is not known by any other names.
Are you sure you want to continue connecting (yes/no/[fingerprint])?";
        let PromptForm::Confirm {
            title,
            details,
            question,
            host_key,
        } = form(text, None, HOSTS)
        else {
            panic!()
        };
        assert_eq!(title, "Connect to github.com?");
        assert!(host_key);
        assert!(details.contains("SHA256:+DiY3"));
        assert_eq!(
            question,
            "Are you sure you want to continue connecting (yes/no/[fingerprint])?"
        );
        assert!(matches!(
            form("Continue connecting (yes/no)? ", None, HOSTS),
            PromptForm::Confirm { .. }
        ));
        assert!(matches!(
            form("Allow use of key id_ed25519?", Some("confirm"), HOSTS),
            PromptForm::Confirm { .. }
        ));
    }

    #[test]
    fn sshs_notices_only_need_dismissing() {
        assert_eq!(
            form(
                "Confirm user presence for key ED25519-SK SHA256:x",
                Some("none"),
                HOSTS
            ),
            PromptForm::Notice {
                title: "ssh".into(),
                details: "Confirm user presence for key ED25519-SK SHA256:x".into()
            }
        );
    }

    #[test]
    fn a_servers_own_question_is_shown_as_it_is() {
        let spoof = "(git@evil.example) Enter passphrase for key '/Users/me/.ssh/id_ed25519':";
        assert_eq!(
            form(spoof, None, HOSTS),
            PromptForm::Secret {
                title: "evil.example asks".into(),
                label: spoof.into()
            }
        );
        assert_eq!(
            title(&form(
                "(git@evil.example) Password for 'https://github.com': ",
                None,
                HOSTS
            )),
            "evil.example asks"
        );
        let host_key = "(git@evil.example) The authenticity of host 'github.com' can't be established.\nAre you sure you want to continue connecting (yes/no)?";
        assert!(matches!(
            form(host_key, None, HOSTS),
            PromptForm::Secret { .. }
        ));
    }

    #[test]
    fn a_login_for_a_host_the_command_isnt_talking_to_isnt_dressed_up() {
        let spoof = "Password for 'https://github.com': ";
        assert_eq!(
            form(spoof, None, &["evil.example"]),
            PromptForm::Secret {
                title: "git asks".into(),
                label: spoof.trim().into()
            }
        );
        assert_eq!(
            title(&form(
                "git@github.com's password: ",
                None,
                &["evil.example"]
            )),
            "git asks"
        );
        assert_eq!(
            title(&form("Username for 'https://github.com': ", None, &[])),
            "git asks"
        );
    }

    #[test]
    fn patterns_only_match_whole_prompts() {
        assert_eq!(
            title(&form(
                "Please type: Enter passphrase for key '/k': now",
                None,
                HOSTS
            )),
            "git asks"
        );
        assert_eq!(
            title(&form("x Username for 'https://github.com': ", None, HOSTS)),
            "git asks"
        );
        assert_eq!(
            title(&form(
                "Password for 'https://github.com': \nextra",
                None,
                HOSTS
            )),
            "git asks"
        );
    }

    #[test]
    fn the_dialog_names_the_command_and_its_hosts() {
        let p = |label: &str, hosts: &[&str]| Prompt {
            id: 1,
            text: String::new(),
            kind: None,
            label: label.into(),
            op: None,
            hosts: hosts.iter().map(|h| h.to_string()).collect(),
        };
        assert_eq!(
            prompt_context(&p("git fetch", &["github.com", "gitlab.com"])),
            "git fetch → github.com, gitlab.com"
        );
        assert_eq!(
            prompt_context(&p("git submodule update", &[])),
            "git submodule update"
        );
    }
}
