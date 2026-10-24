//! The guide tab against a real repo and core, with a stand-in for the agent CLI that prints a
//! canned guide: never the real one.

use std::path::{Path, PathBuf};
use std::process::Command;

use gitviber_core::api;
use gitviber_core::suggest::Kind;
use gitviber_ui::Settings;
use gitviber_ui::workspace::{GuideOf, Selection, TabGroup, Workspace, selection_key};
use gpui_kit::{
    AnyView, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, TestAppContext, VisualTestContext, Window, div, px, size,
};

use crate::store::{Guides, Run, RunKind};
use crate::view::GuideView;

struct Host(Entity<Workspace>);
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Draws the tab it's given, as the app's window would.
struct Drawn(Entity<Workspace>, Option<AnyView>);
impl Render for Drawn {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.1.clone())
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A window drawing `of`'s guide tab, opened with the project.
fn drawn_guide<'a>(
    cx: &'a mut TestAppContext,
    repo: &Path,
    of: impl FnOnce(&mut VisualTestContext) -> GuideOf,
) -> (
    Entity<Drawn>,
    Entity<Workspace>,
    Entity<GuideView>,
    &'a mut VisualTestContext,
) {
    let (host, cx) = cx.add_window_view(|_, cx| Drawn(cx.new(Workspace::new), None));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, repo, cx);
    let of = of(cx);
    let sel = Selection::Guide { of: of.clone() };
    cx.update(|window, cx| crate::open_guide(of, window, cx));
    cx.run_until_parked();
    let view = view_of(&ws, &sel, cx).expect("the guide's tab");
    host.update(cx, |h, cx| {
        h.1 = Some(view.clone().into());
        cx.notify();
    });
    draw(cx);
    (host, ws, view, cx)
}

fn script(at: &Path, body: &str) {
    std::fs::write(at, format!("#!/bin/sh\ncat >/dev/null\n{body}\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(at, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn rev(dir: &Path, r: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", r])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn kept(text: &str, base: String, head: String) -> crate::SavedRun {
    crate::SavedRun {
        text: text.into(),
        base,
        head,
        stamp: None,
        program: "fake-agent".into(),
        details: vec![],
        at: 0,
    }
}

const GUIDE: &str = r#"{"title": "Retry uploads", "overview": "Uploads retry.",
 "diagram": {"flows": [{"title": "Upload", "steps": [{"id": "u", "label": "upload()", "section": 1}, {"id": "r", "label": "retry()", "section": 1}], "edges": [{"from": "u", "to": "r"}]}]},
 "sections": [
  {"title": "Retry", "category": "core", "summary": "Backoff.", "files": ["src/retry.ts"], "lineNotes": [{"path": "src/retry.ts", "line": 1, "text": "New."}]},
  {"title": "Docs", "category": "docs", "importance": "low", "summary": "Says so.", "files": ["README.md"]}
 ]}"#;
const RISKS: &str = r#"{"risks": [{"title": "Retries forever", "severity": "high", "path": "src/retry.ts", "line": 1, "why": "No cap."}]}"#;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// A repo with a commit to explain and an uncommitted change, and the stand-in CLI beside it.
fn setup(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("gitviber-review-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let repo = dir.join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "# Up\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "first"]);
    std::fs::write(repo.join("src/retry.ts"), "export const tries = 3;\n").unwrap();
    std::fs::write(repo.join("README.md"), "# Up\n\nRetries.\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "Retry uploads"]);
    std::fs::write(repo.join("src/retry.ts"), "export const tries = 5;\n").unwrap();
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("guide.json"), GUIDE).unwrap();
    std::fs::write(bin.join("risks.json"), RISKS).unwrap();
    let agent = bin.join("fake-agent");
    std::fs::write(
        &agent,
        "#!/bin/sh\nhere=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\ninput=\"$(cat)\"\necho $$ >> \"$here/calls\"\n[ -f \"$here/delay\" ] && sleep \"$(cat \"$here/delay\")\"\ncase \"$input\" in *\"Hunt this change for bugs\"*) cat \"$here/risks.json\" ;; *) cat \"$here/guide.json\" ;; esac\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: each test runs in its own process (nextest), before any thread reads it.
    unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", dir.join("config")) };
    (repo.canonicalize().unwrap(), agent)
}

fn boot(cx: &mut TestAppContext, agent: &Path) {
    let agent = agent.to_string_lossy().into_owned();
    cx.update(|cx| {
        gitviber_ui::init(cx);
        let core = gitviber_ui::install_core(cx);
        gitviber_diff::init(cx);
        gitviber_diff::set_core(core, cx);
        gitviber_refs::init(cx);
        crate::init(cx);
        Settings::update(cx, |s| s.review_command = Some(agent));
    });
}

fn open_project(ws: &Entity<Workspace>, repo: &Path, cx: &mut VisualTestContext) {
    let root = repo.to_path_buf();
    cx.update(|window, cx| {
        Workspace::register_window(ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(root, window, cx));
    });
    cx.run_until_parked();
}

fn view_of(
    ws: &Entity<Workspace>,
    sel: &Selection,
    cx: &mut VisualTestContext,
) -> Option<Entity<GuideView>> {
    cx.update(|_, cx| {
        ws.read(cx)
            .tab_for(sel)
            .and_then(|t| t.item.handle())
            .and_then(|h| h.view().downcast::<GuideView>().ok())
    })
}

fn calls(agent: &Path) -> usize {
    std::fs::read_to_string(agent.with_file_name("calls"))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// A commit to explain. Its guide reads only what git keeps: nothing writes in the repo, so
/// core's watcher has no news for the test's single thread.
fn head_commit(cx: &mut VisualTestContext) -> GuideOf {
    let commit = cx.update(|_, cx| {
        let core = gitviber_ui::core(cx);
        api::history::log(&core, None, 0, 1, None, None)
            .unwrap()
            .remove(0)
    });
    GuideOf::Commit {
        commit: std::sync::Arc::new(commit),
    }
}

#[gpui_kit::test]
fn a_guide_is_written_kept_and_its_tab_gives_its_entities_back(cx: &mut TestAppContext) {
    let (repo, agent) = setup("flow");
    boot(cx, &agent);
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, &repo, cx);
    let of = head_commit(cx);
    let sel = Selection::Guide { of: of.clone() };
    let open = |cx: &mut VisualTestContext| {
        let of = of.clone();
        cx.update(|window, cx| crate::open_guide(of, window, cx));
        cx.run_until_parked();
    };
    open(cx);
    let view = view_of(&ws, &sel, cx).expect("the guide's tab");
    assert_eq!(calls(&agent), 1, "opening it asked once");
    view.read_with(cx, |v, _| {
        assert_eq!(v.guide().map(|g| g.title.as_str()), Some("Retry uploads"));
        // In the sections' order.
        assert_eq!(v.diff_files(), ["src/retry.ts", "README.md"]);
    });
    // Find Risks, then a section reviewed: kept in the file.
    view.update(cx, |v, cx| v.find_risks(cx));
    cx.run_until_parked();
    assert_eq!(calls(&agent), 2);
    view.update(cx, |v, cx| v.mark_section(0, true, cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.diff_files()),
        ["README.md"],
        "a reviewed section's diffs fold away"
    );
    let file = std::env::var("GITVIBER_CONFIG_DIR")
        .map(|d| PathBuf::from(d).join("guides.json"))
        .unwrap();
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(
        kept.contains("Retry uploads")
            && kept.contains("Retries forever")
            && kept.contains("\"done\":[0]")
    );
    ws.update(cx, |ws, cx| ws.close_group(TabGroup::All, cx));
    drop(view);
    cx.run_until_parked();

    // Opened again: the kept guide, no second ask. Then open and closed, nothing stays.
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    for i in 0..4 {
        open(cx);
        let view = view_of(&ws, &sel, cx).expect("the guide's tab");
        assert!(view.read_with(cx, |v, _| v.guide().is_some()));
        view.update(cx, |v, cx| v.mark_section(0, i % 2 == 0, cx));
        cx.run_until_parked();
        let files = if i % 2 == 0 {
            vec!["README.md"]
        } else {
            vec!["src/retry.ts", "README.md"]
        };
        assert_eq!(view.read_with(cx, |v, _| v.diff_files()), files);
        ws.update(cx, |ws, cx| ws.close_group(TabGroup::All, cx));
        let weak = view.downgrade();
        drop(view);
        cx.run_until_parked();
        assert!(weak.upgrade().is_none(), "the guide's view is released");
    }
    assert_eq!(calls(&agent), 2, "a kept guide isn't asked for again");
    cx.update(|_, _| {});
    cx.run_until_parked();
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    assert_eq!(selection_key(&sel).split(':').next(), Some("guide"));
}

#[gpui_kit::test]
fn closing_its_tab_stops_the_run_before_the_agent_starts(cx: &mut TestAppContext) {
    let (repo, agent) = setup("close");
    boot(cx, &agent);
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, &repo, cx);
    // Opened and closed before anything ran: the run goes with the tab.
    let of = head_commit(cx);
    let id = crate::store::guide_id(&repo.to_string_lossy(), &of, Some("main"));
    cx.update(|window, cx| crate::open_guide(of, window, cx));
    ws.update(cx, |ws, cx| ws.close_group(TabGroup::All, cx));
    cx.run_until_parked();
    assert_eq!(calls(&agent), 0, "the agent never started");
    let run = cx.update(|_, cx| Guides::run(cx, RunKind::Guide, &id));
    assert_eq!(run, Some(Run::Stopped));
}

#[gpui_kit::test]
fn a_cancel_kills_the_agent_and_what_it_started(cx: &mut TestAppContext) {
    let (repo, agent) = setup("cancel");
    std::fs::write(agent.with_file_name("delay"), "30").unwrap();
    boot(cx, &agent);
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, &repo, cx);
    let core = cx.update(|_, cx| gitviber_ui::core(cx));
    // Cancel as the button does, once the stand-in is running (core's own threads keep going
    // while this test's thread waits in the run).
    let calls_file = agent.with_file_name("calls");
    let canceller = std::thread::spawn(move || {
        for _ in 0..500 {
            if calls_file.exists() {
                std::thread::sleep(std::time::Duration::from_millis(200));
                api::changes::suggest_cancel(&core, Kind::Guide);
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    });
    let of = head_commit(cx);
    let started = std::time::Instant::now();
    let sel = Selection::Guide { of: of.clone() };
    cx.update(|window, cx| crate::open_guide(of, window, cx));
    cx.run_until_parked();
    assert!(canceller.join().unwrap(), "the stand-in ran");
    assert!(
        started.elapsed().as_secs() < 20,
        "the run stopped at the cancel, not after the sleep"
    );
    let view = view_of(&ws, &sel, cx).expect("the guide's tab");
    view.read_with(cx, |v, _| assert!(v.guide().is_none()));
    // The stand-in, and the sleep it started, are gone.
    let pid = std::fs::read_to_string(agent.with_file_name("calls")).unwrap();
    let alive = Command::new("kill")
        .args(["-0", pid.trim()])
        .status()
        .unwrap()
        .success();
    assert!(!alive, "the agent CLI was killed");
    let sleeping = Command::new("pgrep")
        .args(["-f", "sleep 30"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&sleeping.stdout).trim().is_empty(),
        "what it started was killed too"
    );
}

#[gpui_kit::test]
fn a_note_written_on_a_commits_lines_is_kept_and_drawn_until_resolved(cx: &mut TestAppContext) {
    use gitviber_diff::{Compare, DiffEvent, DiffSource, DiffView, LineRange, Side};
    let (repo, agent) = setup("notes");
    boot(cx, &agent);
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, &repo, cx);
    let GuideOf::Commit { commit } = head_commit(cx) else {
        unreachable!()
    };
    let source = DiffSource::new(
        Compare::Commit {
            sha: commit.sha.clone(),
        },
        "src/retry.ts",
    );
    let diff = cx.update(|_, cx| cx.new(|cx| DiffView::new(source, cx)));
    cx.run_until_parked();
    let notes = cx.update(|_, cx| crate::attach_notes(&diff, Some("commit 1a2b3c4".into()), cx));
    let range = LineRange {
        side: Side::New,
        start: 1,
        end: 1,
    };
    diff.update(cx, |_, cx| cx.emit(DiffEvent::AddNote(range)));
    cx.run_until_parked();
    assert!(
        notes.read_with(cx, |n, _| n.has_draft()),
        "C opened the composer"
    );
    notes.update_in(cx, |n, window, cx| n.write_draft("Why 3?", window, cx));
    cx.run_until_parked();
    let root = repo.to_string_lossy().into_owned();
    let kept = cx.update(|_, cx| crate::ReviewNotes::get(cx, &root));
    assert_eq!(kept.len(), 1);
    let n = &kept[0];
    assert_eq!(
        (n.path.as_str(), n.anchor.start, n.at.as_deref()),
        ("src/retry.ts", 1, Some("commit 1a2b3c4"))
    );
    assert_eq!(n.anchor.code, ["export const tries = 3;"]);
    assert!(notes.read_with(cx, |n, _| n.drawn() && !n.has_draft()));
    // Resolved, it leaves the diff.
    let id = n.id.clone();
    cx.update(|_, cx| crate::ReviewNotes::update(cx, &root, &id, |n| n.resolved = true));
    cx.run_until_parked();
    assert!(!notes.read_with(cx, |n, _| n.drawn()));
    let weak = notes.downgrade();
    drop(notes);
    drop(diff);
    cx.run_until_parked();
    assert!(weak.upgrade().is_none());
}

#[gpui_kit::test]
fn notes_on_the_files_on_disk_follow_their_lines(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("gitviber-review-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: each test runs in its own process (nextest), before any thread reads it.
    unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", dir.join("config")) };
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let root = dir.to_string_lossy().into_owned();
    let lines: Vec<String> = ["one", "two", "three"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let note = crate::model::notes::ReviewNote {
        id: String::new(),
        path: "a.txt".into(),
        body: "b".into(),
        anchor: crate::model::notes::anchor_at(&lines, 2, 2),
        ..Default::default()
    };
    cx.update(|cx| crate::ReviewNotes::add(cx, &root, note));
    let check = |cx: &mut TestAppContext| {
        let task = cx.update(|cx| crate::ReviewNotes::check(cx, &root));
        cx.run_until_parked();
        drop(task);
        cx.update(|cx| crate::ReviewNotes::get(cx, &root)).remove(0)
    };
    assert_eq!(check(cx).anchor.start, 2);
    std::fs::write(&file, "zero\none\ntwo\nthree\n").unwrap();
    let moved = check(cx);
    assert_eq!((moved.anchor.start, moved.outdated), (3, false));
    std::fs::write(&file, "zero\none\nTWO\nthree\n").unwrap();
    assert!(check(cx).outdated);
}

#[gpui_kit::test]
fn the_guided_review_command_opens_the_uncommitted_changes_guide(cx: &mut TestAppContext) {
    let (repo, agent) = setup("command");
    boot(cx, &agent);
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, &repo, cx);
    // Kept already, so the command shows it rather than asking (which would write in the repo).
    let id = crate::store::guide_id(&repo.to_string_lossy(), &GuideOf::Changes, Some("main"));
    let kept = crate::SavedRun {
        text: GUIDE.into(),
        base: "HEAD".into(),
        head: "HEAD".into(),
        stamp: None,
        program: "fake-agent".into(),
        details: vec![],
        at: 0,
    };
    cx.update(|_, cx| Guides::keep(cx, RunKind::Guide, &id, kept));
    cx.dispatch_action(gitviber_ui::actions::review::GuideUncommitted);
    cx.run_until_parked();
    let sel = Selection::Guide {
        of: GuideOf::Changes,
    };
    let view = view_of(&ws, &sel, cx).expect("the command opened the guide");
    assert!(view.read_with(cx, |v, _| v.guide().is_some()));
    assert_eq!(calls(&agent), 0, "a kept guide isn't asked for");
}

#[gpui_kit::test]
fn closing_the_window_stops_its_run(cx: &mut TestAppContext) {
    let (repo, agent) = setup("window");
    boot(cx, &agent);
    let (host, vcx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(vcx, |h, _| h.0.clone());
    open_project(&ws, &repo, vcx);
    let of = head_commit(vcx);
    let id = crate::store::guide_id(&repo.to_string_lossy(), &of, Some("main"));
    vcx.update(|window, cx| crate::open_guide(of, window, cx));
    // The window closes before the run gets going: its workspace goes, and the run with it.
    drop(ws);
    drop(host);
    vcx.update(|window, _| window.remove_window());
    cx.update(|_| {});
    cx.run_until_parked();
    assert_eq!(calls(&agent), 0, "the agent never started");
    assert_eq!(
        cx.update(|cx| Guides::run(cx, RunKind::Guide, &id)),
        Some(Run::Stopped)
    );
}

/// A stand-in `claude` on PATH, first of every `claude` the run's PATH would find, or the test
/// stops before anything runs: it must never reach the real one.
fn fake_claude(bin: &Path) -> PathBuf {
    let fake = bin.join("claude");
    std::fs::write(
        &fake,
        "#!/bin/sh\nhere=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\ncat >/dev/null\n{ for a in \"$@\"; do printf '%s\\n' \"$a\"; done; echo --END--; } >> \"$here/claude-calls\"\nif [ -f \"$here/old\" ]; then for a in \"$@\"; do [ \"$a\" = --strict-mcp-config ] && { echo \"error: unknown option '--strict-mcp-config'\" >&2; exit 1; }; done; fi\ncat \"$here/guide.json\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    // SAFETY: each test runs in its own process (nextest), before any thread reads it.
    unsafe { std::env::set_var("PATH", &path) };
    // As core searches: the login shell's PATH (never read in a test), Homebrew's, then ours.
    let first = ["/opt/homebrew/bin", "/usr/local/bin"]
        .into_iter()
        .map(PathBuf::from)
        .chain(std::env::split_paths(&path))
        .map(|d| d.join("claude"))
        .find(|p| p.exists());
    assert_eq!(
        first.as_deref(),
        Some(fake.as_path()),
        "a real claude would run"
    );
    fake
}

/// The calls the stand-in got, each one's arguments.
fn claude_calls(bin: &Path) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(bin.join("claude-calls")).unwrap_or_default();
    text.split("--END--\n")
        .filter(|c| !c.is_empty())
        .map(|c| c.lines().map(str::to_string).collect())
        .collect()
}

#[gpui_kit::test]
fn the_claude_preset_runs_lean_with_its_schema_and_retries_an_old_claude_without(
    cx: &mut TestAppContext,
) {
    let (repo, agent) = setup("claude");
    let bin = agent.parent().unwrap().to_path_buf();
    fake_claude(&bin);
    // A lockfile is generated: the whole patch goes in a file, which Claude Code is let read.
    std::fs::write(repo.join("package-lock.json"), "{\"lockfileVersion\": 3}\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "Lock"]);
    boot(cx, &agent);
    cx.update(|cx| Settings::update(cx, |s| s.review_command = Some("claude -p".into())));
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, &repo, cx);
    let of = head_commit(cx);
    let sel = Selection::Guide { of: of.clone() };
    cx.update(|window, cx| crate::open_guide(of, window, cx));
    cx.run_until_parked();
    let view = view_of(&ws, &sel, cx).expect("the guide's tab");
    assert!(view.read_with(cx, |v, _| v.guide().is_some()), "written");
    let calls = claude_calls(&bin);
    assert_eq!(calls.len(), 1);
    let args = &calls[0];
    let has = |a: &str| args.iter().any(|x| x == a);
    for flag in [
        "-p",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--disable-slash-commands",
        "--tools",
        "Read,Grep,Glob",
        "--model",
        "sonnet",
        "--effort",
        "medium",
        "--json-schema",
    ] {
        assert!(has(flag), "{flag} in {args:?}");
    }
    let schema = &args[args.iter().position(|a| a == "--json-schema").unwrap() + 1];
    assert!(
        serde_json::from_str::<serde_json::Value>(schema)
            .is_ok_and(|v| v["required"][0] == "title")
    );
    assert!(args.iter().any(|a| a.starts_with("--add-dir=")), "{args:?}");

    // A Claude Code too old for the lean flags: once more without them (and the schema), the
    // model and effort kept.
    std::fs::write(bin.join("old"), "").unwrap();
    view.update(cx, |v, cx| v.generate(cx));
    cx.run_until_parked();
    let calls = claude_calls(&bin);
    assert_eq!(
        calls.len(),
        3,
        "a failed try and the one without the lean flags"
    );
    let retry = &calls[2];
    let has = |a: &str| retry.iter().any(|x| x == a);
    assert!(
        !has("--strict-mcp-config") && !has("--json-schema") && !has("--tools"),
        "{retry:?}"
    );
    assert!(
        has("--model") && has("sonnet") && has("--effort") && has("medium"),
        "{retry:?}"
    );
    assert!(retry.iter().any(|a| a.starts_with("--add-dir=")));
    let id = crate::store::guide_id(&repo.to_string_lossy(), &head_commit(cx), Some("main"));
    assert_eq!(
        cx.update(|_, cx| Guides::run(cx, RunKind::Guide, &id)),
        None,
        "no failure left"
    );
}

#[gpui_kit::test]
fn a_branchs_guide_opens_once_its_branch_is_known(cx: &mut TestAppContext) {
    let (repo, agent) = setup("branch");
    boot(cx, &agent);
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, &repo, cx);
    let of = GuideOf::Branch {
        base: "refs/heads/main".into(),
        label: "main".into(),
    };
    // Kept under HEAD's branch, as Branch Review's ✦ asks for it.
    let id = crate::store::guide_id(&repo.to_string_lossy(), &of, Some("main"));
    let kept = crate::SavedRun {
        text: GUIDE.into(),
        base: "HEAD~1".into(),
        head: "HEAD".into(),
        stamp: None,
        program: "fake-agent".into(),
        details: vec![],
        at: 0,
    };
    cx.update(|_, cx| Guides::keep(cx, RunKind::Guide, &id, kept));
    let sel = Selection::Guide { of: of.clone() };
    cx.update(|window, cx| crate::open_guide(of, window, cx));
    cx.run_until_parked();
    let view = view_of(&ws, &sel, cx).expect("the branch's guide tab");
    assert!(view.read_with(cx, |v, _| v.guide().is_some()));
    assert_eq!(calls(&agent), 0);
}

#[gpui_kit::test]
fn the_ask_box_starts_claude_code_with_the_model_last_picked(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("gitviber-review-model-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: each test runs in its own process (nextest), before any thread reads it.
    unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
    cx.update(|cx| {
        gitviber_ui::init(cx);
        Settings::update(cx, |s| s.review_command = Some("claude -p".into()));
        let (choices, value) = GuideView::ask_models(cx).unwrap();
        assert_eq!(
            (choices.as_slice(), value.as_str()),
            (&["sonnet", "opus", "haiku"].map(String::from)[..], "sonnet")
        );
        Guides::set_ask_model(cx, "haiku".into());
        assert_eq!(GuideView::ask_models(cx).unwrap().1, "haiku");
        // Only Claude Code starts with the question in hand.
        Settings::update(cx, |s| s.review_command = Some("codex exec".into()));
        assert!(GuideView::ask_models(cx).is_none());
    });
    cx.run_until_parked();
    let kept = std::fs::read_to_string(dir.join("guides.json")).unwrap();
    assert!(kept.contains("\"ask_model\":\"haiku\""), "{kept}");
}

#[gpui_kit::test]
fn a_note_on_a_commit_guides_diffs_is_kept_and_drawn_until_resolved(cx: &mut TestAppContext) {
    use gitviber_diff::{LineRange, Side, StackedEvent};
    let (repo, agent) = setup("guide-notes");
    boot(cx, &agent);
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, &repo, cx);
    let of = head_commit(cx);
    let GuideOf::Commit { commit } = &of else {
        unreachable!()
    };
    let at = format!("commit {}", commit.short_sha);
    let sel = Selection::Guide { of: of.clone() };
    cx.update(|window, cx| crate::open_guide(of.clone(), window, cx));
    cx.run_until_parked();
    let view = view_of(&ws, &sel, cx).expect("the guide's tab");
    let stacked = view.read_with(cx, |v, _| v.stacked().clone());
    let source = stacked.read_with(cx, |s, _| {
        s.files()
            .find(|f| f.source.path == "src/retry.ts")
            .map(|f| f.source.clone())
            .expect("its diff")
    });
    // C on the commit's first line: the composer opens once the file's sides are read.
    let range = LineRange {
        side: Side::New,
        start: 1,
        end: 1,
    };
    stacked.update(cx, |_, cx| cx.emit(StackedEvent::AddNote { source, range }));
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.has_note_draft()), "C opened it");
    assert_eq!(
        view.update(cx, |v, cx| v.drawn_notes("src/retry.ts", cx)),
        ["note-draft"]
    );
    view.update_in(cx, |v, window, cx| v.write_note("Why 3?", window, cx));
    cx.run_until_parked();
    let root = repo.to_string_lossy().into_owned();
    let notes = cx.update(|_, cx| crate::ReviewNotes::get(cx, &root));
    assert_eq!(notes.len(), 1);
    let n = &notes[0];
    assert_eq!(
        (
            n.path.as_str(),
            n.anchor.start,
            n.at.as_deref(),
            n.body.as_str()
        ),
        ("src/retry.ts", 1, Some(at.as_str()), "Why 3?")
    );
    assert_eq!(n.anchor.code, ["export const tries = 3;"]);
    let id = format!("note-{}", n.id);
    assert_eq!(
        view.update(cx, |v, cx| v.drawn_notes("src/retry.ts", cx)),
        [id],
        "drawn under its line, the composer gone"
    );
    // Resolved, it leaves the diffs.
    let note = n.id.clone();
    cx.update(|_, cx| crate::ReviewNotes::update(cx, &root, &note, |n| n.resolved = true));
    cx.run_until_parked();
    assert!(
        view.update(cx, |v, cx| v.drawn_notes("src/retry.ts", cx))
            .is_empty()
    );
}

#[gpui_kit::test]
fn a_failed_run_says_so_in_its_window_with_open_settings(cx: &mut TestAppContext) {
    use std::cell::Cell;
    use std::rc::Rc;
    let (repo, agent) = setup("failed");
    let failing = agent.with_file_name("failing-agent");
    script(&failing, "echo 'Out of credits.' >&2\nexit 3");
    let silent = agent.with_file_name("silent-agent");
    script(&silent, "exit 0");
    boot(cx, &agent);
    let command = failing.to_string_lossy().into_owned();
    cx.update(|cx| Settings::update(cx, |s| s.review_command = Some(command)));
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, &repo, cx);
    let of = head_commit(cx);
    let sel = Selection::Guide { of: of.clone() };
    cx.update(|window, cx| crate::open_guide(of, window, cx));
    cx.run_until_parked();
    let toasts = ws.update(cx, |ws, _| ws.take_toasts());
    let failed = toasts
        .iter()
        .find(|t| t.title.as_deref() == Some("Couldn't write a guided review"))
        .unwrap_or_else(|| panic!("{toasts:?}"));
    assert!(matches!(
        failed.kind,
        gitviber_ui::workspace::ToastKind::Error
    ));
    assert!(failed.message.contains("Out of credits."), "{failed:?}");
    let action = failed.action.clone().expect("a button");
    assert_eq!(action.label.as_ref(), "Open Settings");
    // Its button opens Settings, where the agent is picked.
    let opened = Rc::new(Cell::new(false));
    let seen = opened.clone();
    cx.update(|_, cx| {
        cx.on_action(move |_: &gitviber_ui::actions::workbench::OpenSettings, _| seen.set(true));
    });
    draw(cx);
    cx.update(|window, cx| (action.run)(window, cx));
    cx.run_until_parked();
    assert!(opened.get(), "Open Settings dispatched");

    // An agent that prints nothing: told too.
    let command = silent.to_string_lossy().into_owned();
    cx.update(|_, cx| Settings::update(cx, |s| s.review_command = Some(command)));
    let view = view_of(&ws, &sel, cx).expect("the guide's tab");
    view.update(cx, |v, cx| v.generate(cx));
    cx.run_until_parked();
    let toasts = ws.update(cx, |ws, _| ws.take_toasts());
    let empty = toasts
        .iter()
        .find(|t| t.title.as_deref() == Some("No guided review written"))
        .unwrap_or_else(|| panic!("{toasts:?}"));
    assert!(empty.message.contains("printed nothing"), "{empty:?}");
    assert!(empty.action.is_some());
}

#[gpui_kit::test]
fn the_ask_box_asks_on_enter_and_takes_shift_enter_as_a_new_line(cx: &mut TestAppContext) {
    let (repo, agent) = setup("ask-box");
    boot(cx, &agent);
    let (_host, ws, view, cx) = drawn_guide(cx, &repo, head_commit);
    let input = view.update_in(cx, |v, window, cx| v.open_ask_box(window, cx));
    draw(cx);
    // Nothing typed: Enter asks nothing.
    cx.simulate_keystrokes("enter");
    assert!(view.read_with(cx, |v, _| v.asking()), "still open");
    cx.simulate_input("Why retry");
    cx.simulate_keystrokes("shift-enter");
    cx.simulate_input("three times?");
    assert_eq!(
        input.read_with(cx, |i, _| i.value().to_string()),
        "Why retry\nthree times?"
    );
    assert!(view.read_with(cx, |v, _| v.asking()));
    // Enter asks: the stand-in agent isn't Claude Code, so the question goes to the clipboard.
    cx.simulate_keystrokes("enter");
    assert!(!view.read_with(cx, |v, _| v.asking()), "asked, closed");
    let copied = cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .unwrap_or_default();
    assert!(copied.contains("Why retry\nthree times?"), "{copied}");
    let toasts = ws.update(cx, |ws, _| ws.take_toasts());
    assert!(
        toasts
            .iter()
            .any(|t| t.title.as_deref() == Some("Review copied for the agent")),
        "{toasts:?}"
    );
}

#[gpui_kit::test]
fn the_navigator_lists_the_sections_shown_beside_the_guide_or_over_it(cx: &mut TestAppContext) {
    let (repo, agent) = setup("nav");
    boot(cx, &agent);
    let (_host, _ws, view, cx) = drawn_guide(cx, &repo, head_commit);
    assert_eq!(
        view.read_with(cx, |v, _| v.nav()),
        [
            (1, vec!["src/retry.ts".to_string()]),
            (2, vec!["README.md".to_string()])
        ]
    );
    // Wide: a rail beside the guide, once the tab's width is known; narrow: chips over it.
    cx.simulate_resize(size(px(1600.), px(900.)));
    draw(cx);
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.nav_rail_shown()));
    cx.simulate_resize(size(px(900.), px(900.)));
    draw(cx);
    draw(cx);
    assert!(!view.read_with(cx, |v, _| v.nav_rail_shown()));
    // A reviewed section lists no files, as its diffs fold.
    view.update(cx, |v, cx| v.mark_section(0, true, cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(
        view.read_with(cx, |v, _| v.nav()),
        [(1, vec![]), (2, vec!["README.md".to_string()])]
    );
}

const OLD_GUIDE: &str = r#"{"title": "Retry uploads", "overview": "Uploads retry.",
 "diagram": "graph TD\n  upload --> retry",
 "sections": [{"title": "Retry", "category": "core", "summary": "Backoff.", "files": ["src/retry.ts"]}]}"#;

#[gpui_kit::test]
fn an_older_guides_mermaid_is_drawn_and_goes_with_its_tab(cx: &mut TestAppContext) {
    let (repo, agent) = setup("mermaid");
    boot(cx, &agent);
    let root = repo.to_string_lossy().into_owned();
    let (host, ws, view, cx) = drawn_guide(cx, &repo, |cx| {
        let of = head_commit(cx);
        let id = crate::store::guide_id(&root, &of, Some("main"));
        let run = kept(OLD_GUIDE, rev(&repo, "HEAD~1"), rev(&repo, "HEAD"));
        cx.update(|_, cx| Guides::keep(cx, RunKind::Guide, &id, run));
        of
    });
    assert_eq!(calls(&agent), 0, "the kept guide shows");
    let drawn = view
        .read_with(cx, |v, _| v.legacy_diagram().cloned())
        .expect("its diagram, as Markdown draws Mermaid");
    draw(cx);
    // Closed, the diagram's view goes with the tab.
    let weak = drawn.downgrade();
    drop(drawn);
    host.update(cx, |h, cx| {
        h.1 = None;
        cx.notify();
    });
    ws.update(cx, |ws, cx| ws.close_group(TabGroup::All, cx));
    drop(view);
    draw(cx);
    assert!(weak.upgrade().is_none(), "the diagram's view is released");
}

#[gpui_kit::test]
fn a_branch_guide_shares_viewed_marks_with_branch_review(cx: &mut TestAppContext) {
    use gitviber_diff::StackedEvent;
    let (repo, agent) = setup("shared-marks");
    // HEAD's branch since `old`: the commit with both files; src/retry.ts is edited since.
    git(&repo, &["branch", "old", "HEAD~1"]);
    let (base, head) = (rev(&repo, "HEAD~1"), rev(&repo, "HEAD"));
    boot(cx, &agent);
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    open_project(&ws, &repo, cx);
    let of = GuideOf::Branch {
        base: "refs/heads/old".into(),
        label: "old".into(),
    };
    let id = crate::store::guide_id(&repo.to_string_lossy(), &of, Some("main"));
    let run = kept(GUIDE, base.clone(), head);
    cx.update(|_, cx| Guides::keep(cx, RunKind::Guide, &id, run));
    let sel = Selection::Guide { of: of.clone() };
    cx.update(|window, cx| crate::open_guide(of, window, cx));
    cx.run_until_parked();
    let view = view_of(&ws, &sel, cx).expect("the branch's guide");
    assert_eq!(
        view.read_with(cx, |v, _| v.diff_files()),
        ["src/retry.ts", "README.md"]
    );
    // README.md as Branch Review has it, marked there.
    let review = cx
        .update(|_, cx| {
            api::changes::branch_review(&gitviber_ui::core(cx), "refs/heads/old".into())
        })
        .unwrap();
    assert_eq!(review.base, base);
    let row = review.files.iter().find(|f| f.path == "README.md").unwrap();
    let key = format!("branch:{base}:README.md");
    let sig = crate::view::file_sig(row);
    ws.update(cx, |ws, cx| {
        ws.set_viewed(key.clone(), Some(sig.clone()), cx)
    });
    cx.run_until_parked();
    let stacked = view.read_with(cx, |v, _| v.stacked().clone());
    let viewed = |cx: &mut VisualTestContext, path: &str| {
        stacked.read_with(cx, |s, _| {
            s.files().find(|f| f.source.path == path).map(|f| f.viewed)
        })
    };
    assert_eq!(view.read_with(cx, |v, _| v.viewed_count()), 1);
    assert_eq!(viewed(cx, "README.md"), Some(true), "viewed here too");
    let source = |cx: &mut VisualTestContext, path: &str| {
        stacked.read_with(cx, |s, _| {
            s.files()
                .find(|f| f.source.path == path)
                .map(|f| f.source.clone())
                .unwrap()
        })
    };
    // Unmarked here: unmarked in Branch Review.
    let readme = source(cx, "README.md");
    stacked.update(cx, |_, cx| cx.emit(StackedEvent::ToggleViewed(readme)));
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.viewed(&key).map(str::to_string)),
        None
    );
    let readme = source(cx, "README.md");
    stacked.update(cx, |_, cx| cx.emit(StackedEvent::ToggleViewed(readme)));
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.viewed(&key).map(str::to_string)),
        Some(sig)
    );
    // A file edited since shows another diff there: it keeps the guide's own mark.
    let retry = source(cx, "src/retry.ts");
    stacked.update(cx, |_, cx| cx.emit(StackedEvent::ToggleViewed(retry)));
    cx.run_until_parked();
    let marks: Vec<String> = ws.read_with(cx, |ws, _| ws.viewed_marks().keys().cloned().collect());
    assert!(
        marks
            .iter()
            .any(|k| k.starts_with("pr-file:") && k.ends_with(":src/retry.ts")),
        "{marks:?}"
    );
    assert!(!marks.contains(&format!("branch:{base}:src/retry.ts")));
    assert_eq!(view.read_with(cx, |v, _| v.viewed_count()), 2);
}
