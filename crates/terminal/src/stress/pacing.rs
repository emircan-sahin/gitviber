//! A measurement, run by hand: four panes in a split stream as agents do (about 50 writes a
//! second each) with the window in the background. Prints how often each pane asked for a frame and the
//! app's CPU time, to compare a pacing change against.
//! `cargo nextest run -p gitviber-terminal background_panes_frame_rate --run-ignored only --no-capture`

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use gitviber_core::Core;
use gpui::{AnyView, AnyWindowHandle, AppContext as _, Entity, HeadlessAppContext, px, size};

use super::{FRAME, Host};
use crate::layout::Dir;
use crate::{TerminalConfig, TerminalPane, TerminalPanel, TerminalTheme};

const SECS: u64 = 5;

fn cpu_secs() -> f64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: a plain query into a buffer of ours.
    let usage = unsafe {
        libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr());
        usage.assume_init()
    };
    let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    secs(usage.ru_utime) + secs(usage.ru_stime)
}

/// Like the stress run's pump, but a frame's tasks are run one at a time for at most a frame:
/// with output streaming the scheduler never parks.
fn pump(cx: &mut HeadlessAppContext, clock: &mut Instant, ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < until {
        std::thread::sleep(FRAME);
        let now = Instant::now();
        cx.advance_clock(now - *clock);
        *clock = now;
        while now.elapsed() < FRAME && cx.background_executor().tick() {}
    }
}

#[test]
#[ignore = "a measurement, run by hand"]
fn background_panes_frame_rate() {
    let dir = std::env::temp_dir().join(format!("gitviber-pacing-{}", std::process::id()));
    let (home, config, work) = (dir.join("home"), dir.join("config"), dir.join("work"));
    for d in [&home, &config, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    let script = dir.join("stream.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nexec perl -e '$|=1; while (1) { select(undef,undef,undef,0.02); print \"token \" x 8, \"\\n\" }'\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // SAFETY: nextest gives each test its own process; no thread reads these yet.
    unsafe {
        std::env::set_var("GITVIBER_CONFIG_DIR", &config);
        std::env::set_var("HOME", &home);
        std::env::set_var("SHELL", &script);
    }
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(gpui_apple::AppleTextSystem::new()),
        Arc::new(gitviber_ui::assets::Assets),
        gpui_snapshot_platform::current_headless_renderer,
    );
    cx.allow_parking();
    cx.forbid_parking();
    cx.update(|cx| {
        gitviber_ui::init(cx);
        crate::init(cx);
    });
    let core = Arc::new(Core::new(Arc::new(|_| {})));
    let terminal = TerminalConfig {
        cursor_blink: false,
        shell_integration: false,
        ..Default::default()
    };
    let made: std::rc::Rc<std::cell::RefCell<Option<Entity<TerminalPanel>>>> = Default::default();
    let slot = made.clone();
    let root = work.clone();
    let window: AnyWindowHandle = cx
        .open_window(size(px(1100.), px(760.)), move |window, cx| {
            let panel = cx.new(|cx| {
                TerminalPanel::new(core, root, terminal, TerminalTheme::dark(), window, cx)
            });
            let host = cx.new(|_| Host {
                panel: panel.clone(),
                shown: true,
            });
            *slot.borrow_mut() = Some(panel);
            cx.new(|cx| gpui_kit::base::Root::new(AnyView::from(host), window, cx))
        })
        .unwrap()
        .into();
    let panel = made.borrow_mut().take().unwrap();
    let _ = cx.update_window(window, |_, w, _| w.activate_window());
    let mut clock = Instant::now();
    pump(&mut cx, &mut clock, 50);
    let _ = cx.update_window(window, |_, w, cx| {
        panel.update(cx, |p, cx| p.open_terminal(work.clone(), None, w, cx))
    });
    pump(&mut cx, &mut clock, 200);
    let mut panes: Vec<Entity<TerminalPane>> =
        vec![cx.update(|cx| panel.read(cx).active_pane()).unwrap()];
    for _ in 0..3 {
        let _ = cx.update_window(window, |_, w, cx| {
            panel.update(cx, |p, cx| p.split(Dir::Row, None, w, cx))
        });
        pump(&mut cx, &mut clock, 200);
        panes.push(cx.update(|cx| panel.read(cx).active_pane()).unwrap());
    }
    pump(&mut cx, &mut clock, 1000);
    let counts: Vec<Arc<AtomicUsize>> = panes.iter().map(|_| Arc::default()).collect();
    let _subs: Vec<_> = panes
        .iter()
        .zip(&counts)
        .map(|(p, n)| {
            let n = n.clone();
            cx.update(|cx| cx.observe(p, move |_, _| _ = n.fetch_add(1, Ordering::Relaxed)))
        })
        .collect();
    let other: AnyWindowHandle = cx
        .open_window(size(px(200.), px(100.)), |window, cx| {
            let view = cx.new(|_| super::Other);
            cx.new(|cx| gpui_kit::base::Root::new(AnyView::from(view), window, cx))
        })
        .unwrap()
        .into();
    let _ = cx.update_window(other, |_, w, _| w.activate_window());
    pump(&mut cx, &mut clock, 500);
    let looked: Vec<bool> = panes
        .iter()
        .map(|p| cx.update(|cx| p.read(cx).looked_at()))
        .collect();
    let (cpu0, t0) = (cpu_secs(), Instant::now());
    pump(&mut cx, &mut clock, SECS * 1000);
    let (cpu, wall) = (cpu_secs() - cpu0, t0.elapsed().as_secs_f64());
    for (i, n) in counts.iter().enumerate() {
        eprintln!(
            "pane {i} (keys: {}): {:.0} notifies/s",
            looked[i],
            n.load(Ordering::Relaxed) as f64 / wall
        );
    }
    eprintln!(
        "app CPU: {:.0}% of a core over {wall:.1} s",
        cpu / wall * 100.
    );
    drop(_subs);
    let _ = std::fs::remove_dir_all(&dir);
}
