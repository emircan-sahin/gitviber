//! The guide tab against a real repo and core, with a stand-in for the agent CLI that prints a
//! canned guide: never the real one.

use std::path::{Path, PathBuf};
use std::process::Command;

use gitviber_core::api;
use gitviber_core::suggest::Kind;
use gitviber_ui::Settings;
use gitviber_ui::workspace::{GuideOf, Selection, TabGroup, Workspace, selection_key};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, VisualTestContext,
    Window, div,
};

use crate::store::{Guides, Run, RunKind};
use crate::view::GuideView;

struct Host(Entity<Workspace>);
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
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
