//! A pane's coding agent (core's agents.rs): what it is, how to resume it, the dot it shows, and
//! what a change of its state is worth telling; the notification escapes programs send (OSC 9,
//! 777, 99); and which panes a session save round serializes. Pure, so it's tested here.

use std::collections::HashMap;
use std::collections::HashSet;

/// "working" is Claude Code's busy; "idle" covers its shell (the turn is over, a background shell
/// runs on); "waiting" is its asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Waiting,
    Idle,
}

impl AgentState {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "working" => Some(Self::Working),
            "waiting" => Some(Self::Waiting),
            "idle" => Some(Self::Idle),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneAgent {
    /// What the user calls it ("Claude Code").
    pub name: String,
    /// What resumes its conversation; None while that can't be told.
    pub command: Option<String>,
    pub session: Option<String>,
    pub cwd: Option<String>,
    /// None for an agent that doesn't report one.
    pub state: Option<AgentState>,
}

impl PaneAgent {
    pub fn from_core(a: &gitviber_core::agents::Agent) -> Self {
        PaneAgent {
            name: a.name.clone(),
            command: a.command.clone(),
            session: a.session.clone(),
            cwd: a.cwd.clone(),
            state: a.state.and_then(AgentState::parse),
        }
    }
}

/// The dot a pane, a tab, a worktree or a row of the agents list shows: most urgent first. It
/// asked, news not looked at, working, done and looked at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Look {
    Needs,
    Unread,
    Working,
    Done,
}

impl Look {
    pub const ALL: [Look; 4] = [Look::Needs, Look::Unread, Look::Working, Look::Done];

    /// For a screen reader and a tooltip: the color is never the only telling.
    pub fn label(self, agent: Option<&str>) -> String {
        match self {
            Look::Needs => "Needs you".into(),
            Look::Unread => "Not viewed".into(),
            Look::Working => format!("{} working", agent.unwrap_or("Agent")),
            Look::Done => format!("{} finished", agent.unwrap_or("Agent")),
        }
    }

    /// News for the user: what another worktree's mark, or the Dock badge, tells.
    pub fn is_news(self) -> bool {
        matches!(self, Look::Needs | Look::Unread)
    }
}

/// An agent's state as the agents list says it: idle is finished, one that doesn't say is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Working,
    Waiting,
    Finished,
    Running,
}

impl Shown {
    pub fn label(self) -> &'static str {
        match self {
            Shown::Working => "Working",
            Shown::Waiting => "Needs you",
            Shown::Finished => "Finished",
            Shown::Running => "Running",
        }
    }
}

pub fn shown_state(s: Option<AgentState>) -> Shown {
    match s {
        Some(AgentState::Working) => Shown::Working,
        Some(AgentState::Waiting) => Shown::Waiting,
        Some(AgentState::Idle) => Shown::Finished,
        None => Shown::Running,
    }
}

/// A question outranks the news, and new work makes it old; news from a terminal with no agent,
/// or one that doesn't say its state, can't be told apart from a finish.
pub fn look_of(state: Option<Shown>, unseen: bool) -> Option<Look> {
    match state {
        Some(Shown::Waiting) => Some(Look::Needs),
        Some(Shown::Working) => Some(Look::Working),
        _ if unseen => Some(Look::Unread),
        Some(Shown::Finished) => Some(Look::Done),
        _ => None,
    }
}

/// A pane's look, from its agent and whether it needs the user.
pub fn pane_look(agent: Option<&PaneAgent>, needs_you: bool) -> Option<Look> {
    look_of(agent.map(|a| shown_state(a.state)), needs_you)
}

/// The most urgent of several looks: a tab's, a worktree's.
pub fn most_urgent(looks: impl IntoIterator<Item = Option<Look>>) -> Option<Look> {
    looks.into_iter().flatten().min()
}

/// The pane's agent once `read` (None: none runs there now) is taken in, and what that's worth: a
/// `note` when it finished or stopped to ask, `first` when it's newly seen and reports its state.
/// `live`: `read` is the state file's change as it happened. A lookup's state only starts the agent
/// off: answered after a change that came since, it would undo it.
pub struct Next {
    pub agent: Option<PaneAgent>,
    pub note: Option<String>,
    pub first: bool,
    pub changed: bool,
}

pub fn next_agent(prev: Option<&PaneAgent>, read: Option<PaneAgent>, live: bool) -> Next {
    let Some(mut read) = read else {
        return Next {
            agent: None,
            note: None,
            first: false,
            changed: prev.is_some(),
        };
    };
    if !live && let Some(p) = prev.filter(|p| p.name == read.name && p.state.is_some()) {
        read.state = p.state;
    }
    let changed = prev != Some(&read);
    let prev_state = prev.and_then(|p| p.state);
    let note = match read.state {
        Some(AgentState::Waiting) if prev_state != Some(AgentState::Waiting) => {
            Some(format!("{} is waiting for you", read.name))
        }
        Some(AgentState::Idle) if prev_state == Some(AgentState::Working) => {
            Some(format!("{} finished", read.name))
        }
        _ => None,
    };
    let first = prev.is_none() && read.state.is_some();
    Next {
        agent: Some(read),
        note,
        first,
        changed,
    }
}

/// A pane's agent as the quit's save keeps it: as read now while its shell lives (one that exited
/// isn't resumed); its last known once the shell is gone, as the app ends the shells first.
pub fn at_quit(
    prev: Option<&PaneAgent>,
    alive: bool,
    read: Option<PaneAgent>,
) -> Option<PaneAgent> {
    if !alive {
        return prev.cloned();
    }
    next_agent(prev, read, false).agent
}

/// What the session save keeps of a pane's agent: one that can be resumed, and where it ran.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SavedAgent {
    pub name: String,
    pub command: String,
    pub cwd: String,
}

/// The agents the save keeps, pane by pane: resumable ones, and of two that read the same
/// conversation, the first. Without a session id an agent continues its folder's last
/// conversation: one pane per folder.
pub fn saved_agents(agents: &[Option<&PaneAgent>]) -> Vec<Option<SavedAgent>> {
    let mut taken = HashSet::new();
    agents
        .iter()
        .map(|a| {
            let a = (*a)?;
            let key = a
                .session
                .clone()
                .unwrap_or_else(|| format!("{}\0{}", a.name, a.cwd.as_deref().unwrap_or("")));
            let (command, cwd) = (a.command.clone()?, a.cwd.clone()?);
            if !taken.insert(key) {
                return None;
            }
            Some(SavedAgent {
                name: a.name.clone(),
                command,
                cwd,
            })
        })
        .collect()
}

/// A saved agent as read back, which an older or broken save may hold anything in.
pub fn restored_agent(raw: Option<&serde_json::Value>) -> Option<SavedAgent> {
    let a: SavedAgent = serde_json::from_value(raw?.clone()).ok()?;
    (!a.command.is_empty() && !a.command.chars().any(|c| c.is_control())).then_some(a)
}

/// Settings → Terminal: on a restore, type the resume command at the prompt, run it, or neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeMode {
    Type,
    Run,
    Off,
}

/// The dim line a restored pane shows under its history, and what's typed at its first prompt.
pub fn resume_of(agent: &SavedAgent, mode: ResumeMode) -> Option<(String, String)> {
    match mode {
        ResumeMode::Off => None,
        ResumeMode::Run => Some((
            format!("Resuming {}, which was running here.", agent.name),
            format!("{}\r", agent.command),
        )),
        ResumeMode::Type => Some((
            format!("{} was running here. Press Enter to resume.", agent.name),
            agent.command.clone(),
        )),
    }
}

/// An agent in the agents list.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentEntry {
    pub pane: u64,
    pub name: String,
    pub state: Shown,
    /// Its pane needs the user: a finish or a question not looked at yet.
    pub unseen: bool,
    pub cwd: String,
    /// Since when it's been in `state`, seconds of the clock; 0 when not known.
    pub since: f64,
    /// Its terminal tab, and its pane's number there (1-based, reading order) of how many.
    pub tab: u64,
    pub at: usize,
    pub of: usize,
    pub pane_name: Option<String>,
    pub tab_name: Option<String>,
    pub tab_cwd: String,
}

impl AgentEntry {
    pub fn look(&self) -> Option<Look> {
        look_of(Some(self.state), self.unseen)
    }

    fn rank(&self) -> usize {
        let look = self.look().unwrap_or(Look::Working);
        Look::ALL.iter().position(|&l| l == look).unwrap_or(2)
    }
}

/// The agents that need the user first, then the working ones, then the finished; the longest in
/// its state first.
pub fn by_urgency(a: &AgentEntry, b: &AgentEntry) -> std::cmp::Ordering {
    a.rank().cmp(&b.rank()).then(a.since.total_cmp(&b.since))
}

/// The agents by terminal tab: the tab whose agent is most urgent first, each tab's by pane number.
pub fn by_tab(list: &[AgentEntry]) -> Vec<Vec<AgentEntry>> {
    let mut sorted = list.to_vec();
    sorted.sort_by(by_urgency);
    let mut order: Vec<u64> = vec![];
    let mut tabs: HashMap<u64, Vec<AgentEntry>> = HashMap::new();
    for e in sorted {
        if !tabs.contains_key(&e.tab) {
            order.push(e.tab);
        }
        tabs.entry(e.tab).or_default().push(e);
    }
    order
        .into_iter()
        .filter_map(|t| tabs.remove(&t))
        .map(|mut t| {
            t.sort_by_key(|e| e.at);
            t
        })
        .collect()
}

/// How many agents wait for the user, for the Dock badge.
pub fn agents_waiting(list: &[AgentEntry]) -> usize {
    list.iter()
        .filter(|e| e.look().is_some_and(Look::is_news))
        .count()
}

/// What quitting stops that's worth asking about, from each pane's `busy` (a program in the
/// foreground) and agent: an agent mid-turn, or a command. Not an agent done with its turn, which
/// the next run resumes; one that doesn't say its state counts as a command.
pub fn quit_stops(panes: &[(bool, Option<&PaneAgent>)]) -> Vec<String> {
    let (mut agents, mut commands) = (0, 0);
    for (busy, agent) in panes {
        let state = agent.and_then(|a| a.state);
        if !busy || state == Some(AgentState::Idle) {
            continue;
        }
        if state.is_some() {
            agents += 1;
        } else {
            commands += 1;
        }
    }
    let plural = |n: usize, w: &str| {
        if n == 1 {
            format!("1 {w}")
        } else {
            format!("{n} {w}s")
        }
    };
    let mut out = vec![];
    if agents > 0 {
        out.push(format!("{} working", plural(agents, "agent")));
    }
    if commands > 0 {
        out.push(format!("{} running", plural(commands, "command")));
    }
    out
}

/// What text for a worktree's agent does with the one running there: goes in, waits while it works
/// or asks something, or a new session starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handoff {
    Paste,
    Wait { name: String, working: bool },
    Start,
}

pub fn handoff_to(agent: Option<&PaneAgent>) -> Handoff {
    match agent {
        None => Handoff::Start,
        Some(a) if matches!(a.state, Some(AgentState::Working | AgentState::Waiting)) => {
            Handoff::Wait {
                name: a.name.clone(),
                working: a.state == Some(AgentState::Working),
            }
        }
        Some(_) => Handoff::Paste,
    }
}

/// A notification a program sent.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Note {
    pub title: Option<String>,
    pub body: String,
}

/// A notification is a line or two; a program printing megabytes into one mustn't reach the OS.
const MAX_TEXT: usize = 1000;

fn cut(s: &str) -> String {
    s.trim().chars().take(MAX_TEXT).collect()
}

/// OSC 9: a notification, its text the body. ConEmu's `9;<1-12>` commands aren't one (`9;4` is the
/// progress bar).
pub fn osc9_note(data: &str) -> Option<Note> {
    let head: String = data.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &data[head.len()..];
    let command = head.parse::<u32>().is_ok_and(|n| (1..=12).contains(&n))
        && (rest.is_empty() || rest.starts_with(';'))
        && !head.starts_with('0');
    (!command).then(|| Note {
        title: None,
        body: cut(data),
    })
}

/// OSC 777: rxvt's `notify;<title>;<body>`; its other commands aren't one.
pub fn osc777_note(data: &str) -> Option<Note> {
    let rest = data.strip_prefix("notify;")?;
    let (title, body) = rest.split_once(';').unwrap_or((rest, ""));
    Some(Note {
        title: Some(cut(title)),
        body: cut(body),
    })
}

/// OSC 99, kitty's: `<key=value:…>;<payload>`, the title (`p=title`, the default) and the body
/// (`p=body`) in chunks of one id until `d=1` (the default), base64 with `e=1`. Queries, icons,
/// buttons and closes aren't a notification. One per terminal: it keeps the chunks.
#[derive(Default)]
pub struct KittyNotes {
    id: String,
    title: String,
    body: String,
}

impl KittyNotes {
    pub fn read(&mut self, data: &str) -> Option<Note> {
        let at = data.find(';')?;
        let meta: HashMap<&str, &str> = data[..at]
            .split(':')
            .map(|kv| kv.split_once('=').unwrap_or((kv, "")))
            .collect();
        let kind = meta
            .get("p")
            .copied()
            .filter(|p| !p.is_empty())
            .unwrap_or("title");
        if kind != "title" && kind != "body" {
            return None;
        }
        let mut text: String = data[at + 1..].chars().take(MAX_TEXT * 2).collect();
        if meta.get("e") == Some(&"1") {
            text = String::from_utf8(base64(&text)?).ok()?;
        }
        let of = meta.get("i").copied().unwrap_or("");
        if of != self.id {
            *self = KittyNotes {
                id: of.to_string(),
                ..Default::default()
            };
        }
        let into = if kind == "title" {
            &mut self.title
        } else {
            &mut self.body
        };
        into.push_str(&text);
        if into.len() > MAX_TEXT * 8 {
            into.truncate(
                into.char_indices()
                    .nth(MAX_TEXT * 2)
                    .map_or(into.len(), |(i, _)| i),
            );
        }
        if meta.get("d") == Some(&"0") {
            return None;
        }
        let note = Note {
            title: Some(cut(&self.title)),
            body: cut(&self.body),
        };
        *self = KittyNotes::default();
        Some(note)
    }
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let v: Vec<u8> = chunk.iter().map(|&b| val(b)).collect::<Option<_>>()?;
        let n = v
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &x)| acc | (x as u32) << (18 - 6 * i));
        let take = match v.len() {
            4 => 3,
            3 => 2,
            2 => 1,
            _ => return None,
        };
        out.extend_from_slice(&n.to_be_bytes()[1..1 + take]);
    }
    Some(out)
}

/// Whether a pane that opened in `cwd` is `dir`'s: the deepest worktree holding it is, so main's
/// don't take in a worktree nested inside it (`.claude/worktrees/x`).
pub fn in_worktree(
    cwd: &std::path::Path,
    dir: &std::path::Path,
    worktrees: &[std::path::PathBuf],
) -> bool {
    let owner = worktrees
        .iter()
        .map(|w| w.as_path())
        .chain(std::iter::once(dir))
        .filter(|w| cwd.starts_with(w))
        .max_by_key(|w| w.components().count());
    owner == Some(dir)
}

/// How often the session save runs while something changed.
pub const SAVE_MS: u64 = 2000;
/// A printing pane (an agent's spinner) is serialized once it stops, else this often.
const STALE_MS: u64 = 30_000;

/// What a round needs of a pane: output or a resize since it was last serialized, when it last
/// printed, when it was last serialized (milliseconds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveState {
    pub dirty: bool,
    pub wrote_at: u64,
    pub serialized_at: u64,
}

/// The panes (by index) to serialize now: `all` (the app quitting), or the stalest one due.
pub fn due_for_save(panes: &[SaveState], now: u64, all: bool) -> Vec<usize> {
    let mut due: Vec<usize> = (0..panes.len())
        .filter(|&i| {
            let p = panes[i];
            p.dirty
                && (all
                    || now.saturating_sub(p.wrote_at) >= SAVE_MS
                    || now.saturating_sub(p.serialized_at) >= STALE_MS)
        })
        .collect();
    if !all {
        due.sort_by_key(|&i| panes[i].serialized_at);
        due.truncate(1);
    }
    due
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude(state: Option<AgentState>) -> PaneAgent {
        claude_in(state, Some("claude --resume a-1"), Some("a-1"))
    }
    fn claude_in(
        state: Option<AgentState>,
        command: Option<&str>,
        session: Option<&str>,
    ) -> PaneAgent {
        PaneAgent {
            name: "Claude Code".into(),
            command: command.map(Into::into),
            session: session.map(Into::into),
            cwd: Some("/w".into()),
            state,
        }
    }
    use AgentState::*;
    fn live(prev: &PaneAgent, read: PaneAgent) -> Next {
        next_agent(Some(prev), Some(read), true)
    }

    #[test]
    fn an_agent_that_finishes_or_stops_to_ask_is_worth_a_note_once() {
        assert_eq!(
            live(&claude(Some(Working)), claude(Some(Idle)))
                .note
                .as_deref(),
            Some("Claude Code finished")
        );
        assert_eq!(
            live(&claude(Some(Working)), claude(Some(Waiting)))
                .note
                .as_deref(),
            Some("Claude Code is waiting for you")
        );
        assert_eq!(
            live(&claude(Some(Idle)), claude(Some(Waiting)))
                .note
                .as_deref(),
            Some("Claude Code is waiting for you")
        );
        assert_eq!(
            live(&claude(Some(Waiting)), claude(Some(Waiting))).note,
            None
        );
        assert_eq!(live(&claude(Some(Idle)), claude(Some(Idle))).note, None);
        assert_eq!(live(&claude(Some(Waiting)), claude(Some(Idle))).note, None);
        assert_eq!(next_agent(None, Some(claude(Some(Idle))), false).note, None);
        assert_eq!(
            live(&claude(Some(Working)), claude(Some(Working))).note,
            None
        );
    }

    #[test]
    fn a_lookup_answered_after_a_change_doesnt_undo_it() {
        let idle = live(&claude(Some(Working)), claude(Some(Idle)))
            .agent
            .unwrap();
        let stale = next_agent(Some(&idle), Some(claude(Some(Working))), false);
        assert_eq!(stale.agent.as_ref(), Some(&idle));
        assert!(!stale.changed && stale.note.is_none());
        assert_eq!(
            live(stale.agent.as_ref().unwrap(), claude(Some(Idle))).note,
            None
        );
        let moved = next_agent(
            Some(&idle),
            Some(claude_in(
                Some(Working),
                Some("claude --resume b-2"),
                Some("b-2"),
            )),
            false,
        );
        assert_eq!(
            moved.agent,
            Some(claude_in(
                Some(Idle),
                Some("claude --resume b-2"),
                Some("b-2")
            ))
        );
        let gone = next_agent(Some(&claude(Some(Working))), None, false);
        assert!(gone.agent.is_none() && gone.note.is_none() && gone.changed);
    }

    #[test]
    fn first_is_a_newly_seen_agent_that_reports_its_state() {
        assert!(next_agent(None, Some(claude(Some(Working))), false).first);
        assert!(
            !next_agent(
                Some(&claude(Some(Idle))),
                Some(claude(Some(Working))),
                false
            )
            .first
        );
        assert!(!next_agent(None, Some(claude(None)), false).first);
    }

    #[test]
    fn the_save_keeps_resumable_agents_one_per_conversation() {
        let all = [
            claude(Some(Working)),
            claude(Some(Idle)),
            claude_in(Some(Idle), Some("claude --resume b-2"), Some("b-2")),
            claude_in(Some(Working), None, None),
        ];
        let saved = saved_agents(&[
            Some(&all[0]),
            Some(&all[1]),
            Some(&all[2]),
            Some(&all[3]),
            None,
        ]);
        assert_eq!(
            saved[0],
            Some(SavedAgent {
                name: "Claude Code".into(),
                command: "claude --resume a-1".into(),
                cwd: "/w".into()
            })
        );
        assert_eq!(saved[1], None, "the same conversation in a second pane");
        assert_eq!(
            saved[2].as_ref().map(|a| a.command.as_str()),
            Some("claude --resume b-2")
        );
        assert_eq!(saved[3], None, "its id unread yet");
        assert_eq!(saved[4], None);
        let opencode = PaneAgent {
            name: "opencode".into(),
            command: Some("opencode --continue".into()),
            session: None,
            cwd: Some("/w".into()),
            state: None,
        };
        let other = PaneAgent {
            cwd: Some("/v".into()),
            ..opencode.clone()
        };
        assert_eq!(
            saved_agents(&[Some(&opencode), Some(&opencode)])
                .iter()
                .flatten()
                .count(),
            1
        );
        assert_eq!(
            saved_agents(&[Some(&opencode), Some(&other)])
                .iter()
                .flatten()
                .count(),
            2
        );
        let json = serde_json::to_value(saved[0].clone()).unwrap();
        assert_eq!(restored_agent(Some(&json)), saved[0]);
        assert_eq!(restored_agent(None), None);
        assert_eq!(
            restored_agent(Some(&serde_json::json!({"name": "x", "command": "x"}))),
            None
        );
        assert_eq!(
            restored_agent(Some(
                &serde_json::json!({"name": "x", "command": "", "cwd": "/w"})
            )),
            None
        );
        assert_eq!(
            restored_agent(Some(
                &serde_json::json!({"name": "x", "command": "rm -rf ~\rclaude", "cwd": "/w"})
            )),
            None
        );
    }

    #[test]
    fn a_restored_agents_command_is_typed_for_enter_or_run() {
        let a = SavedAgent {
            name: "Claude Code".into(),
            command: "claude --resume a-1".into(),
            cwd: "/w".into(),
        };
        assert_eq!(
            resume_of(&a, ResumeMode::Type),
            Some((
                "Claude Code was running here. Press Enter to resume.".into(),
                "claude --resume a-1".into()
            ))
        );
        assert_eq!(
            resume_of(&a, ResumeMode::Run),
            Some((
                "Resuming Claude Code, which was running here.".into(),
                "claude --resume a-1\r".into()
            ))
        );
        assert_eq!(resume_of(&a, ResumeMode::Off), None);
    }

    fn entry(pane: u64, tab: u64, at: usize, state: Shown, since: f64, unseen: bool) -> AgentEntry {
        AgentEntry {
            pane,
            name: "Claude Code".into(),
            state,
            unseen,
            cwd: "/w".into(),
            since,
            tab,
            at,
            of: 6,
            pane_name: None,
            tab_name: None,
            tab_cwd: "/w".into(),
        }
    }

    #[test]
    fn the_list_puts_the_ones_that_need_the_user_first_and_counts_them() {
        use Shown::*;
        let list = vec![
            entry(1, 1, 1, Finished, 10., false),
            entry(2, 1, 2, Working, 30., false),
            entry(3, 1, 3, Finished, 50., true),
            entry(4, 1, 4, Waiting, 40., false),
            entry(5, 1, 5, Running, 20., false),
            entry(6, 1, 6, Waiting, 5., false),
        ];
        let mut sorted = list.clone();
        sorted.sort_by(by_urgency);
        assert_eq!(
            sorted.iter().map(|e| e.pane).collect::<Vec<_>>(),
            [6, 4, 3, 5, 2, 1]
        );
        assert_eq!(agents_waiting(&list), 3);
        assert_eq!(agents_waiting(&[]), 0);
        let tabs = vec![
            entry(13, 1, 3, Working, 10., false),
            entry(11, 1, 1, Working, 20., false),
            entry(12, 1, 2, Working, 30., false),
            entry(21, 2, 1, Finished, 5., false),
            entry(22, 2, 2, Waiting, 40., false),
            entry(31, 3, 1, Finished, 50., true),
        ];
        let grouped = by_tab(&tabs);
        let shape: Vec<Vec<String>> = grouped
            .iter()
            .map(|t| t.iter().map(|e| format!("{}.{}", e.tab, e.at)).collect())
            .collect();
        assert_eq!(
            shape,
            [vec!["2.1", "2.2"], vec!["3.1"], vec!["1.1", "1.2", "1.3"]]
        );
        assert_eq!(grouped.iter().flatten().count(), tabs.len());
        let many: Vec<AgentEntry> = (0..500)
            .map(|i| {
                entry(
                    i,
                    1,
                    1,
                    if i % 2 == 1 { Waiting } else { Working },
                    0.,
                    i % 3 == 0,
                )
            })
            .collect();
        assert_eq!(agents_waiting(&many), 250);
    }

    #[test]
    fn a_quit_asks_about_agents_mid_turn_and_running_commands() {
        let (w, i, q, n) = (
            claude(Some(Working)),
            claude(Some(Idle)),
            claude(Some(Waiting)),
            claude(None),
        );
        assert!(quit_stops(&[]).is_empty());
        assert!(quit_stops(&[(false, None), (false, Some(&w))]).is_empty());
        assert!(quit_stops(&[(true, Some(&i))]).is_empty());
        assert_eq!(quit_stops(&[(true, Some(&w))]), ["1 agent working"]);
        assert_eq!(
            quit_stops(&[(true, Some(&q)), (true, Some(&w))]),
            ["2 agents working"]
        );
        assert_eq!(quit_stops(&[(true, None)]), ["1 command running"]);
        assert_eq!(
            quit_stops(&[
                (true, Some(&n)),
                (true, None),
                (true, Some(&w)),
                (true, Some(&i))
            ]),
            ["1 agent working", "2 commands running"]
        );
    }

    #[test]
    fn text_for_a_worktrees_agent() {
        assert_eq!(handoff_to(None), Handoff::Start);
        assert_eq!(handoff_to(Some(&claude(Some(Idle)))), Handoff::Paste);
        assert_eq!(handoff_to(Some(&claude(None))), Handoff::Paste);
        assert_eq!(
            handoff_to(Some(&claude(Some(Working)))),
            Handoff::Wait {
                name: "Claude Code".into(),
                working: true
            }
        );
        assert_eq!(
            handoff_to(Some(&claude(Some(Waiting)))),
            Handoff::Wait {
                name: "Claude Code".into(),
                working: false
            }
        );
    }

    #[test]
    fn looks() {
        assert_eq!(look_of(Some(Shown::Waiting), false), Some(Look::Needs));
        assert_eq!(look_of(Some(Shown::Working), true), Some(Look::Working));
        assert_eq!(look_of(None, true), Some(Look::Unread));
        assert_eq!(look_of(Some(Shown::Finished), false), Some(Look::Done));
        assert_eq!(look_of(None, false), None);
        assert_eq!(
            most_urgent([None, Some(Look::Done), Some(Look::Working)]),
            Some(Look::Working)
        );
        assert_eq!(
            Look::Working.label(Some("Claude Code")),
            "Claude Code working"
        );
    }

    #[test]
    fn osc_9_and_777_notifications_not_conemus_commands_or_rxvts_others() {
        let body = |b: &str| {
            Some(Note {
                title: None,
                body: b.into(),
            })
        };
        assert_eq!(
            osc9_note("Claude is waiting for your input"),
            body("Claude is waiting for your input")
        );
        assert_eq!(osc9_note("hello; world"), body("hello; world"));
        assert_eq!(
            osc9_note("\n\nClaude needs your permission to use Bash"),
            body("Claude needs your permission to use Bash")
        );
        assert_eq!(osc9_note("4;1;50"), None);
        assert_eq!(osc9_note("4;0"), None);
        assert_eq!(osc9_note("12"), None);
        assert_eq!(osc9_note("42 files changed"), body("42 files changed"));
        assert_eq!(osc9_note(&"x".repeat(5000)).unwrap().body.len(), 1000);
        assert_eq!(
            osc777_note("notify;Claude Code;Needs your permission; to run ls"),
            Some(Note {
                title: Some("Claude Code".into()),
                body: "Needs your permission; to run ls".into()
            })
        );
        assert_eq!(
            osc777_note("notify;Done"),
            Some(Note {
                title: Some("Done".into()),
                body: String::new()
            })
        );
        assert_eq!(osc777_note("preexec"), None);
    }

    #[test]
    fn osc_99_comes_in_chunks_and_a_query_isnt_a_notification() {
        let mut read = KittyNotes::default();
        let note = |t: &str, b: &str| {
            Some(Note {
                title: Some(t.into()),
                body: b.into(),
            })
        };
        assert_eq!(read.read(";Hello"), note("Hello", ""));
        assert_eq!(read.read("i=7:d=0;Claude "), None);
        assert_eq!(read.read("i=7:d=0;Code"), None);
        // "Waiting ✅" in base64.
        assert_eq!(
            read.read("i=7:p=body:e=1;V2FpdGluZyDinIU="),
            note("Claude Code", "Waiting ✅")
        );
        assert_eq!(read.read("i=1:p=?;"), None);
        assert_eq!(read.read("i=1:p=close;"), None);
        assert_eq!(read.read("i=2:e=1;not base64!"), None);
        assert_eq!(read.read("no payload"), None);
        assert_eq!(read.read("i=3:p;Hi"), note("Hi", ""));
    }

    #[test]
    fn a_printing_pane_is_saved_once_it_stops_or_every_30_s_one_a_round() {
        let p = |dirty, wrote_at, serialized_at| SaveState {
            dirty,
            wrote_at,
            serialized_at,
        };
        let now = 100_000;
        let (quiet, busy, stale, older, clean) = (
            p(true, now - SAVE_MS, now - 5000),
            p(true, now - 10, now - 5000),
            p(true, now - 10, now - 31_000),
            p(true, now - 10, now - 40_000),
            p(false, 0, 0),
        );
        assert!(due_for_save(&[busy, clean], now, false).is_empty());
        assert_eq!(due_for_save(&[busy, quiet], now, false), [1]);
        assert_eq!(due_for_save(&[quiet, stale, older, busy], now, false), [2]);
        assert_eq!(due_for_save(&[quiet, busy, clean], now, true), [0, 1]);
        let mut agents = [p(true, 0, 0); 3];
        let mut serialized = 0;
        let mut t = now + SAVE_MS;
        while t <= now + 60_000 {
            for a in &mut agents {
                a.dirty = true;
                a.wrote_at = t - 5;
            }
            let due = due_for_save(&agents, t, false);
            assert!(due.len() <= 1);
            for i in &due {
                agents[*i].dirty = false;
                agents[*i].serialized_at = t;
            }
            serialized += due.len();
            t += SAVE_MS;
        }
        assert_eq!(serialized, 6);
    }

    #[test]
    fn a_pane_is_the_deepest_worktrees_it_opened_in() {
        use std::path::{Path, PathBuf};
        let wts = [PathBuf::from("/r"), PathBuf::from("/r/.claude/worktrees/x")];
        let (main, x) = (Path::new("/r"), Path::new("/r/.claude/worktrees/x"));
        assert!(in_worktree(Path::new("/r/src"), main, &wts));
        assert!(!in_worktree(
            Path::new("/r/.claude/worktrees/x/src"),
            main,
            &wts
        ));
        assert!(in_worktree(
            Path::new("/r/.claude/worktrees/x/src"),
            x,
            &wts
        ));
        assert!(!in_worktree(Path::new("/elsewhere"), main, &wts));
        // A worktree the list doesn't have yet still holds what's in it.
        assert!(in_worktree(Path::new("/s/a"), Path::new("/s"), &[]));
        assert!(!in_worktree(Path::new("/rx"), main, &wts));
    }

    #[test]
    fn the_quit_keeps_an_agent_whose_shell_the_app_ended_first() {
        let busy = claude(Some(AgentState::Working));
        // Its shell killed before the save: what was known stands.
        assert_eq!(at_quit(Some(&busy), false, None), Some(busy.clone()));
        // Alive, and the agent gone back to the prompt: nothing to resume.
        assert_eq!(at_quit(Some(&busy), true, None), None);
        // Alive and still there: its state as last heard, the read's command.
        let read = claude_in(
            Some(AgentState::Idle),
            Some("claude --resume b-2"),
            Some("b-2"),
        );
        let kept = at_quit(Some(&busy), true, Some(read)).unwrap();
        assert_eq!(
            (kept.state, kept.command.as_deref()),
            (Some(AgentState::Working), Some("claude --resume b-2"))
        );
        assert_eq!(at_quit(None, false, None), None);
    }
}
