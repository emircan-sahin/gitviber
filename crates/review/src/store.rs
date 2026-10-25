//! Guides and risk lists as written, kept by guide id (per worktree, then per commit, pull
//! request, the uncommitted changes, or branch and base), and the runs writing them. Both are
//! the app's, not a tab's: a tab out of sight may give its view back, and its run goes on. A run
//! stops when it's cancelled or its tab closes (the workspace's tabs are watched while it runs).

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use gitviber_core::Core;
use gitviber_core::api;
use gitviber_core::suggest::{self, Guided, Kind, Target};
use gitviber_ui::workspace::{
    GuideOf, Selection, Toast, Workspace, WorkspaceEvent, selection_path,
};
use gpui_kit::{
    App, AppContext as _, BorrowAppContext as _, Entity, Global, Subscription, Task, WeakEntity,
};
use serde::{Deserialize, Serialize};

use crate::model::agent::{ReviewAgent, is_unknown_option, run_details};
use crate::model::guide::{GUIDE_SCHEMA, guide_prompt};
use crate::model::risks::{RISKS_SCHEMA, risks_prompt};

/// A run's answer as kept: as printed (parsed when shown), the range it read, who wrote it (with
/// the model and effort its command named) and when (ms).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedRun {
    pub text: String,
    pub base: String,
    pub head: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<String>,
    pub program: String,
    #[serde(default)]
    pub details: Vec<String>,
    pub at: i64,
}

/// A guide as kept, with the sections marked reviewed (their indexes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedGuide {
    #[serde(flatten)]
    pub run: SavedRun,
    #[serde(default)]
    pub done: Vec<usize>,
}

/// Guides or risk lists, the most recent last, at most `max` of them in the file; one over
/// `max_size` is kept only until GitViber quits.
struct Kept<T> {
    list: VecDeque<(String, T)>,
    unsaved: VecDeque<(String, T)>,
    max: usize,
    max_size: usize,
}

impl<T: Clone> Kept<T> {
    fn new(max: usize, max_size: usize) -> Self {
        Kept {
            list: VecDeque::new(),
            unsaved: VecDeque::new(),
            max,
            max_size,
        }
    }

    fn get(&self, id: &str) -> Option<&T> {
        self.unsaved
            .iter()
            .chain(self.list.iter())
            .find(|(k, _)| k == id)
            .map(|(_, v)| v)
    }

    /// Puts `value` last; false when it's too big for the file.
    fn put(&mut self, id: &str, value: T, size: usize) -> bool {
        self.list.retain(|(k, _)| k != id);
        self.unsaved.retain(|(k, _)| k != id);
        let fits = size <= self.max_size;
        let to = if fits {
            &mut self.list
        } else {
            &mut self.unsaved
        };
        to.push_back((id.to_string(), value));
        while to.len() > self.max {
            to.pop_front();
        }
        fits
    }

    /// Changes the one kept under `id` in place, keeping its place.
    fn update(&mut self, id: &str, f: impl FnOnce(&mut T)) -> bool {
        match self
            .unsaved
            .iter_mut()
            .chain(self.list.iter_mut())
            .find(|(k, _)| k == id)
        {
            Some((_, v)) => {
                f(v);
                true
            }
            None => false,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default)]
    guides: Vec<(String, serde_json::Value)>,
    #[serde(default)]
    risks: Vec<(String, serde_json::Value)>,
    /// The model Ask Agent's box last started Claude Code with; None: Settings' until one's picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ask_model: Option<String>,
}

/// What a run over a guide's range is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RunKind {
    Guide,
    Risks,
}

impl RunKind {
    fn core(self) -> Kind {
        match self {
            RunKind::Guide => Kind::Guide,
            RunKind::Risks => Kind::Risks,
        }
    }

    /// What it's called in messages.
    pub fn noun(self) -> &'static str {
        match self {
            RunKind::Guide => "guided review",
            RunKind::Risks => "risk list",
        }
    }
}

/// A run on its way, stopped before it was written, or why it failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Running {
        since: Instant,
        /// Fetching a pull request's commits, before the agent starts.
        fetching: bool,
        /// What the agent CLI has printed so far, its end.
        tail: Tail,
    },
    Stopped,
    Failed(String),
}

/// The end of what a run's command printed, as it prints it: at most MAX_TAIL bytes, its tab
/// shows the last lines. Written from core's reader threads, read when the tab draws.
#[derive(Clone, Debug, Default)]
pub struct Tail(Arc<std::sync::Mutex<Vec<u8>>>);

const MAX_TAIL: usize = 8 * 1024;

impl PartialEq for Tail {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Tail {
    pub fn push(&self, chunk: &[u8]) {
        let mut b = self.0.lock().unwrap_or_else(|e| e.into_inner());
        b.extend_from_slice(chunk);
        if b.len() > MAX_TAIL {
            let cut = b.len() - MAX_TAIL;
            b.drain(..cut);
        }
    }

    /// The last `n` lines, as text: terminal escapes and other control characters left out.
    pub fn lines(&self, n: usize) -> Vec<String> {
        let b = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let text = String::from_utf8_lossy(&b);
        let mut clean = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                // A CSI sequence (colors, cursor moves): ESC [ params, then its final letter.
                '\x1b' => {
                    if chars.peek() == Some(&'[') {
                        chars.next();
                        for c in chars.by_ref() {
                            if ('@'..='~').contains(&c) {
                                break;
                            }
                        }
                    }
                }
                // A carriage return redraws its line: what follows replaces it.
                '\r' => {
                    if chars.peek() != Some(&'\n') {
                        let start = clean.rfind('\n').map_or(0, |i| i + 1);
                        clean.truncate(start);
                    }
                }
                '\n' | '\t' => clean.push(c),
                c if c.is_control() => {}
                c => clean.push(c),
            }
        }
        let lines: Vec<&str> = clean.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(n)..]
            .iter()
            .map(|l| l.to_string())
            .collect()
    }
}

/// The run of a kind in progress: core writes one of each kind at a time.
struct Active {
    id: String,
    /// What it's of, as its tab is titled.
    label: String,
    token: u64,
    _task: Task<()>,
    /// Where its news goes.
    workspace: Option<WeakEntity<Workspace>>,
    /// The workspace's tabs, to stop it when its tab closes, and the workspace itself.
    _tabs: Option<Subscription>,
    _window: Option<Subscription>,
}

pub struct Guides {
    loaded: bool,
    guides: Kept<SavedGuide>,
    risks: Kept<SavedRun>,
    runs: HashMap<(RunKind, String), Run>,
    active: HashMap<RunKind, Active>,
    next: u64,
    ask_model: Option<String>,
}

impl Global for Guides {}

impl Default for Guides {
    fn default() -> Self {
        Guides {
            loaded: false,
            // A dozen guides of the usual 5-30 KB.
            guides: Kept::new(12, 64 * 1024),
            // At most eight risks of a few sentences: past this, it isn't the short list asked for.
            risks: Kept::new(12, 32 * 1024),
            runs: HashMap::new(),
            active: HashMap::new(),
            next: 0,
            ask_model: None,
        }
    }
}

fn path() -> PathBuf {
    gitviber_ui::store::config_dir().join("guides.json")
}

/// The old app's guides and risk lists, by the same ids, oldest first.
fn from_legacy(value: impl Fn(&str) -> Option<serde_json::Value>) -> File {
    let entries = |key: &str| match value(key) {
        Some(serde_json::Value::Object(map)) => map.into_iter().collect(),
        _ => vec![],
    };
    File {
        guides: entries("gitviber.guides"),
        risks: entries("gitviber.guideRisks"),
        ask_model: value("gitviber.askModel.review").and_then(|v| v.as_str().map(str::to_string)),
    }
}

/// On the first launch, what the old app kept, unless this build has a file of its own.
pub(crate) fn import_legacy(cx: &mut App) {
    import_from(cx, gitviber_ui::legacy::value);
}

fn import_from(cx: &mut App, value: impl Fn(&str) -> Option<serde_json::Value>) {
    let file = from_legacy(value);
    if path().exists()
        || (file.guides.is_empty() && file.risks.is_empty() && file.ask_model.is_none())
    {
        return;
    }
    cx.default_global::<Guides>();
    cx.update_global::<Guides, _>(|g, cx| {
        // There's no file to read: what's imported is all there is.
        g.loaded = true;
        g.take(file);
        g.write(cx);
    });
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Where a guide, and its risks, are kept: per worktree, then per commit, per pull request, its
/// uncommitted changes, or per branch (`branch`: HEAD's, None detached) and base.
pub fn guide_id(root: &str, of: &GuideOf, branch: Option<&str>) -> String {
    match of {
        GuideOf::Commit { commit } => format!("{root}\0{}", commit.sha),
        GuideOf::Pull { pull, .. } => format!("{root}\0{}", pull.url),
        GuideOf::Changes => format!("{root}\0changes"),
        GuideOf::Branch { base, .. } => format!("{root}\0{}\0{base}", branch.unwrap_or("HEAD")),
    }
}

/// Why a run ended without an answer.
pub enum Done {
    Stopped,
    Failed(String),
    /// It ran, and printed nothing.
    Empty(String),
}

/// Said once a launch: an old Claude Code runs every review without the lean flags.
static TOLD_OLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A failed run's news, in the window it was started from, wherever its tab is.
fn tell(ws: Option<&WeakEntity<Workspace>>, toast: Toast, cx: &mut App) {
    if let Some(ws) = ws.and_then(WeakEntity::upgrade) {
        ws.update(cx, |ws, cx| ws.toast(toast, cx));
    }
}

/// Settings, where the agent is picked: what a failed run's toast offers.
fn to_settings(toast: Toast) -> Toast {
    toast.action("Open Settings", |window, cx| {
        window.dispatch_action(Box::new(gitviber_ui::actions::workbench::OpenSettings), cx)
    })
}

impl Guides {
    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Ok(text) = std::fs::read_to_string(path()) else {
            return;
        };
        let Ok(file) = serde_json::from_str::<File>(&text) else {
            gitviber_ui::store::quarantine(&path());
            return;
        };
        self.take(file);
    }

    /// What a file has, oldest first; one that isn't a guide (or risk list) as kept is left out.
    fn take(&mut self, file: File) {
        if file.ask_model.is_some() {
            self.ask_model = file.ask_model;
        }
        for (id, v) in file.guides {
            if let Ok(g) = serde_json::from_value::<SavedGuide>(v) {
                let size = g.run.text.len();
                self.guides.put(&id, g, size);
            }
        }
        for (id, v) in file.risks {
            if let Ok(r) = serde_json::from_value::<SavedRun>(v) {
                let size = r.text.len();
                self.risks.put(&id, r, size);
            }
        }
    }

    fn write(&self, cx: &mut App) {
        let file = File {
            ask_model: self.ask_model.clone(),
            guides: self
                .guides
                .list
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), serde_json::to_value(v).ok()?)))
                .collect(),
            risks: self
                .risks
                .list
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), serde_json::to_value(v).ok()?)))
                .collect(),
        };
        if let Ok(text) = serde_json::to_string(&file) {
            gitviber_ui::store::write_in_background(cx, path(), text);
        }
    }

    /// The kept ones, read from the file the first time: only that first read changes the
    /// global (and tells its observers), so a view can read on every notification.
    fn loaded(cx: &mut App) -> &Guides {
        if !cx.try_global::<Guides>().is_some_and(|g| g.loaded) {
            cx.default_global::<Guides>().ensure_loaded();
        }
        cx.global::<Guides>()
    }

    /// The model Ask Agent's box last started Claude Code with.
    pub fn ask_model(cx: &mut App) -> Option<String> {
        Self::loaded(cx).ask_model.clone()
    }

    pub fn set_ask_model(cx: &mut App, model: String) {
        cx.default_global::<Guides>();
        cx.update_global::<Guides, _>(|g, cx| {
            g.ensure_loaded();
            g.ask_model = Some(model);
            g.write(cx);
        });
    }

    pub fn guide(cx: &mut App, id: &str) -> Option<SavedGuide> {
        Self::loaded(cx).guides.get(id).cloned()
    }

    pub fn risks(cx: &mut App, id: &str) -> Option<SavedRun> {
        Self::loaded(cx).risks.get(id).cloned()
    }

    pub fn run(cx: &App, kind: RunKind, id: &str) -> Option<Run> {
        cx.try_global::<Guides>()?
            .runs
            .get(&(kind, id.to_string()))
            .cloned()
    }

    pub fn is_running(cx: &App, kind: RunKind, id: &str) -> bool {
        matches!(Self::run(cx, kind, id), Some(Run::Running { .. }))
    }

    /// Marks section `i` of the guide under `id` reviewed, or not.
    pub fn mark_done(cx: &mut App, id: &str, i: usize, done: bool) {
        cx.default_global::<Guides>();
        cx.update_global::<Guides, _>(|g, cx| {
            g.ensure_loaded();
            let changed = g.guides.update(id, |s| {
                s.done.retain(|d| *d != i);
                if done {
                    s.done.push(i);
                }
            });
            if changed {
                g.write(cx);
            }
        });
    }

    /// Keeps a guide, as a run would: for a demo's canned one, and tests.
    pub fn keep(cx: &mut App, kind: RunKind, id: &str, run: SavedRun) {
        cx.default_global::<Guides>();
        cx.update_global::<Guides, _>(|g, cx| {
            g.ensure_loaded();
            let size = run.text.len();
            match kind {
                RunKind::Guide => {
                    g.guides.put(id, SavedGuide { run, done: vec![] }, size);
                }
                RunKind::Risks => {
                    g.risks.put(id, run, size);
                }
            }
            g.write(cx);
        });
    }

    /// Asks the Guided Review agent for `kind` of `of`, kept under `id` once it lands. Returns
    /// what the run this stops was of, if any: one of a kind is written at a time.
    pub fn start(
        cx: &mut App,
        kind: RunKind,
        id: String,
        of: GuideOf,
        tab: Option<(Entity<Workspace>, Selection)>,
    ) -> Option<String> {
        let core = gitviber_ui::core(cx);
        let agent = ReviewAgent::from_settings(gitviber_ui::Settings::get(cx));
        let token = {
            let g = cx.default_global::<Guides>();
            g.ensure_loaded();
            g.next += 1;
            g.next
        };
        // The one running stops now: the new run may fail before core would stop it (a pull
        // request's commits not fetched), and it would run to its timeout.
        if cx.global::<Guides>().active.contains_key(&kind) {
            api::changes::suggest_cancel(&core, kind.core());
        }
        let stopped = cx
            .global::<Guides>()
            .active
            .get(&kind)
            .filter(|a| a.id != id)
            .map(|a| (a.id.clone(), a.label.clone()));
        if let Some((old, _)) = &stopped {
            cx.global_mut::<Guides>()
                .runs
                .insert((kind, old.clone()), Run::Stopped);
        }
        let label = selection_path(&Selection::Guide { of: of.clone() });
        let fetching = matches!(of, GuideOf::Pull { .. });
        let tail = Tail::default();
        cx.global_mut::<Guides>().runs.insert(
            (kind, id.clone()),
            Run::Running {
                since: Instant::now(),
                fetching,
                tail: tail.clone(),
            },
        );
        let workspace = tab.as_ref().map(|(ws, _)| ws.downgrade());
        // Its window closed: the workspace goes, and no tab of it is left to tell.
        let window = tab.as_ref().map(|(ws, _)| {
            let id = id.clone();
            cx.observe_release(ws, move |_, cx| {
                cx.defer(move |cx| Guides::cancel(cx, kind, &id));
            })
        });
        let tabs = tab.map(|(ws, sel)| {
            let id = id.clone();
            cx.subscribe(&ws, move |ws, e: &WorkspaceEvent, cx| {
                if matches!(
                    e,
                    WorkspaceEvent::TabsChanged | WorkspaceEvent::ProjectChanged
                ) && ws.read(cx).tab_for(&sel).is_none()
                {
                    let id = id.clone();
                    // Not from inside the workspace's notification: the run's subscription goes.
                    cx.defer(move |cx| Guides::cancel(cx, kind, &id));
                }
            })
        });
        let run_id = id.clone();
        let task = cx.spawn(async move |cx| {
            let (done, old) = run(&core, &agent, kind, &of, token, tail, cx).await;
            cx.update(|cx| Guides::finish(cx, kind, &run_id, token, done, old));
        });
        cx.global_mut::<Guides>().active.insert(
            kind,
            Active {
                id,
                label,
                token,
                _task: task,
                workspace,
                _tabs: tabs,
                _window: window,
            },
        );
        stopped.map(|(_, label)| label)
    }

    fn still(cx: &App, kind: RunKind, token: u64) -> bool {
        cx.try_global::<Guides>()
            .and_then(|g| g.active.get(&kind))
            .is_some_and(|a| a.token == token)
    }

    fn finish(
        cx: &mut App,
        kind: RunKind,
        id: &str,
        token: u64,
        done: Result<SavedRun, Done>,
        old: bool,
    ) {
        if !Self::still(cx, kind, token) {
            return;
        }
        let ws = cx
            .global::<Guides>()
            .active
            .get(&kind)
            .and_then(|a| a.workspace.clone());
        let noun = kind.noun();
        match &done {
            Err(Done::Failed(why)) => tell(
                ws.as_ref(),
                to_settings(Toast::error(why.clone()).title(format!("Couldn't write a {noun}"))),
                cx,
            ),
            Err(Done::Empty(why)) => tell(
                ws.as_ref(),
                to_settings(Toast::error(why.clone()).title(format!("No {noun} written"))),
                cx,
            ),
            _ => {}
        }
        if old && !TOLD_OLD.swap(true, std::sync::atomic::Ordering::Relaxed) {
            tell(
                ws.as_ref(),
                Toast::info("It doesn't know the flags that make suggestions faster, so it ran without them. Run `claude update` in Terminal to get them.")
                    .title("Claude Code is out of date"),
                cx,
            );
        }
        cx.update_global::<Guides, _>(|g, cx| {
            g.active.remove(&kind);
            let key = (kind, id.to_string());
            match done {
                Ok(run) => {
                    g.runs.remove(&key);
                    let size = run.text.len();
                    match kind {
                        RunKind::Guide => {
                            g.guides.put(id, SavedGuide { run, done: vec![] }, size);
                        }
                        RunKind::Risks => {
                            g.risks.put(id, run, size);
                        }
                    }
                    g.write(cx);
                }
                Err(Done::Stopped) => {
                    g.runs.insert(key, Run::Stopped);
                }
                Err(Done::Failed(why) | Done::Empty(why)) => {
                    g.runs.insert(key, Run::Failed(why));
                }
            }
        });
    }

    /// Stops `id`'s run of `kind`, killing the agent CLI and what it started.
    pub fn cancel(cx: &mut App, kind: RunKind, id: &str) {
        let Some(g) = cx.try_global::<Guides>() else {
            return;
        };
        if !g.active.get(&kind).is_some_and(|a| a.id == id) {
            return;
        }
        // Core's flag first: its loop sees it within 20 ms and kills the process group.
        api::changes::suggest_cancel(&gitviber_ui::core(cx), kind.core());
        cx.update_global::<Guides, _>(|g, _| {
            g.active.remove(&kind);
            g.runs.insert((kind, id.to_string()), Run::Stopped);
        });
    }

    /// Forgets a run's stop or failure, once its tab has said so.
    pub fn clear_run(cx: &mut App, kind: RunKind, id: &str) {
        let key = (kind, id.to_string());
        let done = cx
            .try_global::<Guides>()
            .and_then(|g| g.runs.get(&key))
            .is_some_and(|r| !matches!(r, Run::Running { .. }));
        if done {
            cx.global_mut::<Guides>().runs.remove(&key);
        }
    }

    /// The pull request's commits fetched, so its run can start.
    fn fetched(cx: &mut App, kind: RunKind, id: &str) {
        if let Some(Run::Running { fetching, .. }) = cx
            .default_global::<Guides>()
            .runs
            .get_mut(&(kind, id.to_string()))
        {
            *fetching = false;
        }
    }
}

/// What core is asked to read, made again for a second try.
#[derive(Clone)]
enum Of {
    Changes,
    Commit(String),
    Branch(String),
    Pull {
        base: String,
        head: String,
        title: String,
    },
}

impl Of {
    fn target(&self) -> Target {
        match self.clone() {
            Of::Changes => Target::Changes,
            Of::Commit(sha) => Target::Commit { sha },
            Of::Branch(base) => Target::Branch { base },
            Of::Pull { base, head, title } => Target::Pull { base, head, title },
        }
    }
}

/// One run: a pull request's commits fetched first when they're missing (as its page does),
/// then the agent, once more without the lean flags when its Claude Code is too old for them.
/// A run, and whether it took the second try an old Claude Code needs.
async fn run(
    core: &Arc<Core>,
    agent: &ReviewAgent,
    kind: RunKind,
    of: &GuideOf,
    token: u64,
    tail: Tail,
    cx: &mut gpui_kit::AsyncApp,
) -> (Result<SavedRun, Done>, bool) {
    let old = std::cell::Cell::new(false);
    let done = run_inner(core, agent, kind, of, token, tail, &old, cx).await;
    (done, old.get())
}

#[allow(clippy::too_many_arguments)]
async fn run_inner(
    core: &Arc<Core>,
    agent: &ReviewAgent,
    kind: RunKind,
    of: &GuideOf,
    token: u64,
    tail: Tail,
    old: &std::cell::Cell<bool>,
    cx: &mut gpui_kit::AsyncApp,
) -> Result<SavedRun, Done> {
    let program = agent.program();
    let target = match of {
        GuideOf::Changes => Of::Changes,
        GuideOf::Commit { commit } => Of::Commit(commit.sha.clone()),
        GuideOf::Branch { base, .. } => Of::Branch(base.clone()),
        GuideOf::Pull { pull, target } => {
            let (core, pull, target) = (core.clone(), pull.clone(), target.clone());
            let files = cx
                .background_spawn(async move {
                    api::github::pr_files(
                        &core,
                        target,
                        pull.number,
                        pull.base_ref.clone(),
                        pull.base_sha.clone(),
                        pull.head_sha.clone(),
                        "guide".into(),
                        |_| {},
                    )
                    .map(|f| Of::Pull {
                        base: f.base,
                        head: f.head,
                        title: pull.title.clone(),
                    })
                })
                .await;
            let id = cx.update(|cx| {
                cx.try_global::<Guides>()
                    .and_then(|g| g.active.get(&kind))
                    .filter(|a| a.token == token)
                    .map(|a| a.id.clone())
            });
            // Cancelled, or another started, while the commits were fetched.
            let Some(id) = id else {
                return Err(Done::Stopped);
            };
            cx.update(|cx| Guides::fetched(cx, kind, &id));
            files.map_err(Done::Failed)?
        }
    };
    let (prompt, schema) = match kind {
        RunKind::Guide => (guide_prompt(&agent.language), GUIDE_SCHEMA.to_json()),
        RunKind::Risks => (risks_prompt(&agent.language), RISKS_SCHEMA.to_json()),
    };
    let exec = cx.background_executor().clone();
    let ask = |lean: bool| {
        let (core, line, prompt, target) = (
            core.clone(),
            agent.line(lean),
            prompt.clone(),
            target.target(),
        );
        let args = agent.agent(lean, &schema);
        let ran = line.clone();
        let tail = tail.clone();
        let output: suggest::Output = Arc::new(move |chunk: &[u8]| tail.push(chunk));
        let task = exec.spawn(async move {
            api::changes::suggest_guide_streaming(
                &core,
                line,
                prompt,
                target,
                args,
                kind.core(),
                output,
            )
        });
        async move { (task.await, ran) }
    };
    let (mut answer, mut ran) = ask(true).await;
    if let Err(e) = &answer
        && agent.is_claude_preset()
        && is_unknown_option(e)
    {
        // A second try started after another run would stop that one.
        let still = cx.update(|cx| Guides::still(cx, kind, token));
        if !still {
            return Err(Done::Stopped);
        }
        (answer, ran) = ask(false).await;
        old.set(answer.is_ok());
    }
    match answer {
        Ok(Guided {
            text,
            base,
            head,
            stamp,
        }) => {
            if text.trim().is_empty() {
                return Err(Done::Empty(format!("{program} printed nothing.")));
            }
            Ok(SavedRun {
                text,
                base,
                head,
                stamp,
                program,
                details: run_details(&ran),
                at: now_ms(),
            })
        }
        Err(e) if e == suggest::CANCELLED => Err(Done::Stopped),
        Err(e) => Err(Done::Failed(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_lists_keep_the_latest_and_hold_big_ones_aside() {
        let mut k: Kept<u32> = Kept::new(2, 10);
        assert!(k.put("a", 1, 5));
        assert!(k.put("b", 2, 5));
        assert!(k.put("a", 3, 5));
        assert!(k.put("c", 4, 5));
        let ids: Vec<&str> = k.list.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
        // Too big for the file: kept aside, taking the old one's place.
        assert!(!k.put("a", 5, 50));
        assert_eq!(k.get("a"), Some(&5));
        assert_eq!(k.list.len(), 1);
        assert!(k.update("c", |v| *v = 9));
        assert_eq!(k.get("c"), Some(&9));
    }

    #[test]
    fn the_tail_keeps_the_last_lines_without_terminal_codes() {
        let t = Tail::default();
        t.push(b"one\n\x1b[32mtwo\x1b[0m\nthree 10%\rthree 90%\n");
        assert_eq!(t.lines(2), ["two", "three 90%"]);
        t.push(&vec![b'x'; 20_000]);
        assert_eq!(t.0.lock().unwrap().len(), MAX_TAIL);
        // A cut through a character reads as text still.
        t.push("é\nlast\n".as_bytes());
        assert_eq!(t.lines(1), ["last"]);
    }

    #[gpui_kit::test]
    fn the_old_apps_guides_and_risks_come_over_once(cx: &mut gpui_kit::TestAppContext) {
        let dir = gitviber_ui::test_support::config_dir("review-legacy");
        // As localStorage had them: id → run, oldest first, and one an older build mangled.
        let old = serde_json::json!({
            "gitviber.guides": {
                "/r\u{0}aaa": { "text": "{\"title\": \"A\"}", "base": "aaa^", "head": "aaa", "program": "claude", "details": ["sonnet", "medium"], "at": 1700000000000i64, "done": [1] },
                "/r\u{0}changes": { "text": "{}", "base": "b", "head": "t", "stamp": "s1", "program": "codex", "at": 1700000001000i64, "done": [] },
                "/r\u{0}bad": { "text": 3 }
            },
            "gitviber.askModel.review": "opus",
            "gitviber.guideRisks": {
                "/r\u{0}aaa": { "text": "{\"risks\": []}", "base": "aaa^", "head": "aaa", "program": "claude", "at": 1700000002000i64 }
            }
        });
        let value = |key: &str| old.get(key).cloned();
        cx.update(|cx| {
            import_from(cx, value);
            let a = Guides::guide(cx, "/r\0aaa").expect("a guide came over");
            assert_eq!(
                (a.done.as_slice(), a.run.details.len(), a.run.at),
                (&[1][..], 2, 1_700_000_000_000)
            );
            assert_eq!(
                Guides::guide(cx, "/r\0changes")
                    .unwrap()
                    .run
                    .stamp
                    .as_deref(),
                Some("s1")
            );
            assert!(Guides::guide(cx, "/r\0bad").is_none());
            assert_eq!(
                Guides::risks(cx, "/r\0aaa").unwrap().text,
                "{\"risks\": []}"
            );
            assert_eq!(Guides::ask_model(cx).as_deref(), Some("opus"));
        });
        cx.run_until_parked();
        let kept =
            std::fs::read_to_string(dir.join("guides.json")).expect("written to its own file");
        assert!(kept.contains("codex") && !kept.contains("bad"));
        // Once there's a file, the old app's are never read again.
        let other = serde_json::json!({ "gitviber.guides": { "/r\u{0}zzz": { "text": "{}", "base": "a", "head": "b", "program": "x", "at": 1 } } });
        cx.update(|cx| {
            import_from(cx, |key| other.get(key).cloned());
            assert!(Guides::guide(cx, "/r\0zzz").is_none());
        });
        assert_eq!(from_legacy(|_| None).guides.len(), 0);
    }

    #[test]
    fn a_saved_guide_reads_as_the_old_app_wrote_it() {
        let old = serde_json::json!({
            "text": "{}", "base": "a", "head": "b", "program": "claude", "at": 1700000000000i64, "done": [0, 2]
        });
        let g: SavedGuide = serde_json::from_value(old).unwrap();
        assert_eq!(g.done, [0, 2]);
        assert!(g.run.details.is_empty() && g.run.stamp.is_none());
        let back = serde_json::to_value(&g).unwrap();
        assert_eq!(back["done"], serde_json::json!([0, 2]));
        assert!(back.get("stamp").is_none());
    }
}
