//! The old app's terminal, under Claude Code, piled words nobody typed into its input box and
//! lost parts of the screen until a selection repainted them. This runs six fake agents
//! (agent.rs) in five tabs and a split while it switches tabs and panes every 100–300 ms, hides
//! and shows the panel, zooms, selects, types, pastes and moves the window's focus, through the
//! real key bindings and an off-screen window with the real text system and renderer. At each
//! checkpoint the agents hold still and every pane's grid is held against what its agent drew, and
//! its paint against a repaint from nothing; at the end each agent's input against what was meant
//! for it. `GITVIBER_STRESS_SECS=300` makes the five-minute run; GITVIBER_STRESS_SEED replays one.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use gitviber_core::Core;
use gpui::{
    AnyView, AnyWindowHandle, AppContext as _, Context, Entity, HeadlessAppContext,
    InteractiveElement as _, IntoElement, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, PlatformInput, Point, Render, Styled as _,
    Window, div, point, prelude::FluentBuilder as _, px, size,
};

use crate::layout::Dir;
use crate::{TerminalConfig, TerminalPane, TerminalPanel, TerminalTheme};

mod agent;

use agent::now_ms;

/// The default run, short enough for CI.
const DEFAULT_SECS: u64 = 20;
const PANEL_HEIGHT: f32 = 330.;
const FRAME: Duration = Duration::from_millis(16);

/// The workspace's column as the app draws it: the code above, the terminal panel below at a fixed
/// height while shown, not drawn at all while hidden.
struct Host {
    panel: Entity<TerminalPanel>,
    shown: bool,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(gitviber_ui::keymap::ROOT_CONTEXT)
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1().min_h_0())
            .when(self.shown, |d| {
                d.child(
                    div()
                        .flex_none()
                        .h(px(PANEL_HEIGHT))
                        .flex()
                        .flex_col()
                        .child(div().flex_1().min_h_0().child(self.panel.clone())),
                )
            })
    }
}

/// Another window, to take the focus from ours.
struct Other;

impl Render for Other {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// xorshift: the run is replayable from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A pane and what the test meant for its program.
struct Track {
    pane: Entity<TerminalPane>,
    pid: u32,
    typed: String,
    pastes: Vec<String>,
    /// Its focus (keys in an active window) by the model, at the start and at each change.
    focus: Vec<bool>,
    /// Its size on screen, each time it changed: what its program may be told, in order.
    sizes: Vec<(usize, usize)>,
}

/// Where the keys are and what shows, as the actions taken so far say.
struct Model {
    /// Tracks by tab, in pane order.
    tabs: Vec<Vec<usize>>,
    focused: Vec<usize>,
    active: usize,
    shown: bool,
    window_active: bool,
    zoomed: bool,
}

impl Model {
    fn keys(&self) -> usize {
        self.tabs[self.active][self.focused[self.active]]
    }

    /// GPUI blurs the focused element while its window isn't active.
    fn focused(&self, t: usize) -> bool {
        self.shown && self.window_active && self.keys() == t
    }

    fn visible(&self, t: usize) -> bool {
        let tab = &self.tabs[self.active];
        self.shown && tab.contains(&t) && (!self.zoomed || tab.len() == 1 || self.keys() == t)
    }

    fn split_open(&self) -> bool {
        self.tabs[self.active].len() > 1
    }
}

struct Run {
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
    /// Device pixels a point, in the captures.
    scale: f32,
    other: AnyWindowHandle,
    host: Entity<Host>,
    panel: Entity<TerminalPanel>,
    tracks: Vec<Track>,
    model: Model,
    rng: Rng,
    dir: PathBuf,
    agents: PathBuf,
    failures: Vec<String>,
    /// The last actions taken, for a failure's context.
    trace: Vec<String>,
    /// From here on the agents' focus reports are held against the model's changes.
    t0: u128,
    typed_nowhere: usize,
    actions: usize,
    checkpoints: usize,
    /// The real time the app's clock has been brought up to.
    clock: Instant,
}

fn key(cx: &mut HeadlessAppContext, window: AnyWindowHandle, keys: &str) {
    for k in keys.split(' ') {
        let k = Keystroke::parse(k).expect("a keystroke");
        let _ = cx.update_window(window, |_, w, cx| w.dispatch_keystroke(k, cx));
    }
}

fn event(cx: &mut HeadlessAppContext, window: AnyWindowHandle, e: PlatformInput) {
    let _ = cx.update_window(window, |_, w, cx| w.dispatch_event(e, cx));
}

impl Run {
    /// Frames for `ms` of real time, the app's clock going on with it.
    fn pump(&mut self, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        loop {
            std::thread::sleep(FRAME);
            // By the time that passed, not a frame's worth: each frame's work kept the app's
            // clock behind, and on a loaded CI runner the 700 ms pane number outlived the
            // checkpoint's 800 ms wait and covered the cells it checked.
            let now = Instant::now();
            self.cx.advance_clock(now - self.clock);
            self.clock = now;
            self.cx.run_until_parked();
            if Instant::now() >= until {
                break;
            }
        }
    }

    fn fail(&mut self, what: String) {
        if self.failures.is_empty() {
            eprintln!("last actions:\n  {}", self.trace.join("\n  "));
        }
        eprintln!("FAIL: {what}");
        self.failures.push(what);
    }

    fn pane_read<R>(&mut self, t: usize, f: impl FnOnce(&TerminalPane) -> R) -> R {
        let pane = self.tracks[t].pane.clone();
        self.cx.update(|cx| f(pane.read(cx)))
    }

    /// The panel shown or hidden, as the app's terminal toggle: shown, the keys go to its open
    /// pane.
    fn show(&mut self, shown: bool) {
        self.model.shown = shown;
        let (host, panel) = (self.host.clone(), self.panel.clone());
        let _ = self.cx.update_window(self.window, |_, window, cx| {
            host.update(cx, |h, cx| {
                h.shown = shown;
                cx.notify();
            });
            if shown {
                panel.update(cx, |p, cx| p.focus_active(window, cx));
            }
        });
    }

    /// Our window in front, or the other one.
    fn activate(&mut self, ours: bool) {
        let to = if ours { self.window } else { self.other };
        let _ = self
            .cx
            .update_window(to, |_, window, _| window.activate_window());
        self.model.window_active = ours;
    }

    fn window_active(&mut self) -> bool {
        self.cx
            .update_window(self.window, |_, w, _| w.is_window_active())
            .unwrap_or(false)
    }

    /// After each action: the panes' focus as GPUI has it against the model, the focus changes
    /// the agents should hear of, and the sizes the panes show.
    fn observe(&mut self) {
        let active = self.window_active();
        if active != self.model.window_active {
            self.fail(format!(
                "window active {active}, model {}",
                self.model.window_active
            ));
        }
        for t in 0..self.tracks.len() {
            let (focused, metrics) = self.pane_read(t, |p| (p.focused, p.metrics));
            let want = self.model.focused(t);
            if focused != want {
                self.fail(format!(
                    "pane {t} focused: {focused}, model {want} (shown {}, tab {}, zoomed {}, window {})",
                    self.model.shown,
                    self.model.active,
                    self.model.zoomed,
                    self.model.window_active
                ));
            }
            if self.tracks[t].focus.last() != Some(&want) {
                self.tracks[t].focus.push(want);
            }
            if self.model.visible(t)
                && let Some(m) = metrics
            {
                let s = (m.cols, m.rows);
                if self.tracks[t].sizes.last() != Some(&s) {
                    self.tracks[t].sizes.push(s);
                }
            }
        }
    }

    fn act(&mut self) {
        self.actions += 1;
        let w = self.window;
        // Keys reach only the window in front.
        let front = self.model.window_active;
        // Hidden or in the back for a moment, as a user would be: half the time the next
        // action brings it back.
        let pick = match self.rng.below(100) {
            _ if !self.model.shown && self.rng.below(2) == 0 => 60,
            _ if !self.model.window_active && self.rng.below(2) == 0 => 88,
            n => n,
        };
        let what = match pick {
            0..30 if front => {
                let back = self.rng.below(3) == 0;
                // ⇧⌘[ and ⇧⌘], as macOS gives them.
                key(&mut self.cx, w, if back { "cmd-{" } else { "cmd-}" });
                if self.model.shown {
                    let n = self.model.tabs.len();
                    self.model.active = (self.model.active + if back { n - 1 } else { 1 }) % n;
                }
                if back { "previous tab" } else { "next tab" }
            }
            30..45 if front => {
                key(&mut self.cx, w, "shift-cmd-right");
                if self.model.shown && self.model.split_open() {
                    let a = self.model.active;
                    self.model.focused[a] = (self.model.focused[a] + 1) % 2;
                }
                "next pane"
            }
            45..60 if front => {
                let text = format!("k{}z", self.actions);
                let chars: Vec<String> = text.chars().map(String::from).collect();
                key(&mut self.cx, w, &chars.join(" "));
                if self.model.shown {
                    let t = self.model.keys();
                    self.tracks[t].typed.push_str(&text);
                } else {
                    self.typed_nowhere += text.len();
                }
                "type"
            }
            60..68 => {
                let shown = !self.model.shown;
                self.show(shown);
                if shown { "show panel" } else { "hide panel" }
            }
            68..76 if front => {
                key(&mut self.cx, w, "shift-cmd-enter");
                if self.model.shown && self.model.split_open() {
                    self.model.zoomed = !self.model.zoomed;
                }
                "zoom"
            }
            76..88 if self.model.shown => {
                self.select();
                "select"
            }
            88..94 => {
                self.activate(!self.model.window_active);
                "window focus"
            }
            94..100 if front && self.model.shown => {
                let text = format!("pasted {}", self.actions);
                let t = self.model.keys();
                let pane = self.tracks[t].pane.clone();
                let _ = self.cx.update_window(w, |_, window, cx| {
                    pane.update(cx, |p, cx| p.paste_text(text.clone(), window, cx))
                });
                self.tracks[t].pastes.push(text);
                "paste"
            }
            _ => return,
        };
        let m = &self.model;
        if self.trace.len() == 6 {
            self.trace.remove(0);
        }
        self.trace.push(format!(
            "{} {what}: tab {} pane {} shown {} zoomed {} window {}",
            self.actions,
            m.active,
            m.keys(),
            m.shown,
            m.zoomed,
            m.window_active
        ));
    }

    /// A drag across the focused pane's rows, or a click that clears what one selected.
    fn select(&mut self) {
        let t = self.model.keys();
        let (m, had) = self.pane_read(t, |p| (p.metrics, p.has_selection()));
        let Some(m) = m else { return };
        let at = |col: f32, row: f32| -> Point<gpui::Pixels> {
            m.origin + point(m.cell.width * (col + 0.5), m.cell.height * (row + 0.5))
        };
        let (from, to) = if had {
            (at(3., 1.), at(3., 1.))
        } else {
            (at(1., 0.), at(14., 2.))
        };
        let w = self.window;
        let none = Modifiers::default();
        event(
            &mut self.cx,
            w,
            PlatformInput::MouseMove(MouseMoveEvent {
                position: from,
                pressed_button: None,
                modifiers: none,
            }),
        );
        event(
            &mut self.cx,
            w,
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: from,
                modifiers: none,
                click_count: 1,
                first_mouse: false,
            }),
        );
        if !had {
            event(
                &mut self.cx,
                w,
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: to,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: none,
                }),
            );
        }
        event(
            &mut self.cx,
            w,
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: to,
                modifiers: none,
                click_count: 1,
            }),
        );
    }

    fn capture(&mut self) -> Option<Image> {
        let img = self.cx.capture_screenshot(self.window).ok()?;
        Some(Image {
            width: img.width() as usize,
            pixels: img.as_raw().clone(),
        })
    }

    /// The agents hold still and say what they drew; once every pane shows it, the panes' grids
    /// and paint are checked, the open tab first (painted bit by bit all along), then the others.
    fn checkpoint(&mut self) {
        self.checkpoints += 1;
        let id = self.checkpoints.to_string();
        if !self.model.window_active {
            self.activate(true);
        }
        if !self.model.shown {
            self.show(true);
        }
        let _ = std::fs::write(self.agents.join(agent::FREEZE), &id);
        // Until every pane shows what its agent drew: a loaded machine can starve a pane's
        // reader for longer than any quiet spell taken to mean "done".
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            self.pump(16);
            if (0..self.tracks.len()).all(|t| self.grid_mismatch(t, &id).is_none()) {
                break;
            }
        }
        // The pane number a pane switch shows goes after 700 ms.
        self.pump(800);
        self.observe();
        let tabs = self.model.tabs.len();
        let first = self.model.active;
        for i in (0..tabs).map(|i| (first + i) % tabs) {
            if i != self.model.active {
                key(&mut self.cx, self.window, &format!("cmd-{}", i + 1));
                self.model.active = i;
                self.pump(400);
                self.observe();
            }
            self.check_paint(&id);
        }
        for t in 0..self.tracks.len() {
            self.check_grid(t, &id);
        }
        let _ = std::fs::remove_file(self.agents.join(agent::FREEZE));
        self.pump(50);
    }

    /// Each pane on screen as the frames left it, against the same pane painted from nothing.
    fn check_paint(&mut self, id: &str) {
        let visible: Vec<usize> = (0..self.tracks.len())
            .filter(|&t| self.model.visible(t))
            .collect();
        let Some(before) = self.capture() else {
            return self.fail("no off-screen picture: the renderer didn't start".into());
        };
        for &t in &visible {
            let pane = self.tracks[t].pane.clone();
            self.cx
                .update(|cx| pane.read(cx).cache.borrow_mut().clear());
        }
        let _ = self
            .cx
            .update_window(self.window, |_, window, _| window.refresh());
        self.cx.run_until_parked();
        let Some(after) = self.capture() else {
            return self.fail("no off-screen picture: the renderer didn't start".into());
        };
        for t in visible {
            let Some(m) = self.pane_read(t, |p| p.metrics) else {
                self.fail(format!("pane {t} on screen but never laid out"));
                continue;
            };
            let (b, k) = (m.bounds, self.scale);
            let (x0, y0) = (
                (f32::from(b.origin.x) * k) as usize,
                (f32::from(b.origin.y) * k) as usize,
            );
            let (x1, y1) = (
                x0 + (f32::from(b.size.width) * k) as usize,
                y0 + (f32::from(b.size.height) * k) as usize,
            );
            let differ = before.differ(&after, x0, y0, x1, y1);
            if differ > 0 {
                let shot = self.dir.join(format!("paint-{id}-pane{t}"));
                self.fail(format!(
                    "checkpoint {id}: pane {t} painted {differ} pixels unlike its repaint ({}.*.ppm)",
                    shot.display()
                ));
                before.save(&shot.with_extension("before.ppm"));
                after.save(&shot.with_extension("after.ppm"));
            }
            self.check_ink(t, id, &after, m);
        }
    }

    /// Ink where the grid has a character, none where it and all around it are blank.
    fn check_ink(&mut self, t: usize, id: &str, img: &Image, m: crate::pane::Metrics) {
        let pane = self.tracks[t].pane.clone();
        let cells: Vec<Vec<(char, bool)>> = self.cx.update(|cx| {
            let p = pane.read(cx);
            let emu = p.session.emu.lock();
            let term = &emu.term;
            let selected = term.selection.as_ref().and_then(|s| s.to_range(term));
            (0..term.screen_lines())
                .map(|r| {
                    let line = Line(r as i32);
                    let skip = selected
                        .as_ref()
                        .is_some_and(|s| s.start.line <= line && line <= s.end.line);
                    (0..term.columns())
                        .map(|c| {
                            let cell = &term.grid()[line][Column(c)];
                            let spacer = cell.flags.intersects(
                                Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER,
                            );
                            let blank = cell.c == ' ' || spacer;
                            (if blank { ' ' } else { cell.c }, skip)
                        })
                        .collect()
                })
                .collect()
        });
        let scale = self.scale;
        let bg = img.at(
            ((f32::from(m.bounds.origin.x) + 4.) * scale) as usize,
            ((f32::from(m.bounds.origin.y) + 3.) * scale) as usize,
        );
        let (cw, ch) = (f32::from(m.cell.width), f32::from(m.cell.height));
        let ink = |r: usize, c: usize| {
            let x0 = ((f32::from(m.origin.x) + cw * c as f32) * scale).ceil() as usize;
            let y0 = ((f32::from(m.origin.y) + ch * r as f32) * scale).ceil() as usize;
            let x1 = ((f32::from(m.origin.x) + cw * (c + 1) as f32) * scale).floor() as usize;
            let y1 = ((f32::from(m.origin.y) + ch * (r + 1) as f32) * scale).floor() as usize;
            (y0..y1).any(|y| {
                (x0..x1).any(|x| img.at(x, y).iter().zip(bg).any(|(a, b)| a.abs_diff(b) > 48))
            })
        };
        let blank = |r: isize, c: isize| {
            r < 0
                || c < 0
                || cells
                    .get(r as usize)
                    .and_then(|row| row.get(c as usize))
                    .is_none_or(|(ch, _)| *ch == ' ')
        };
        let mut missing = Vec::new();
        let mut stray = Vec::new();
        for (r, row) in cells.iter().enumerate() {
            for (c, &(chr, skip)) in row.iter().enumerate() {
                if skip {
                    continue;
                }
                let has = ink(r, c);
                if chr != ' ' && !has && !(c + 1 < row.len() && ink(r, c + 1)) {
                    missing.push(format!("({r},{c}) {chr:?}"));
                }
                let (ri, ci) = (r as isize, c as isize);
                let quiet = (-1..=1).all(|dr| (-1..=1).all(|dc| blank(ri + dr, ci + dc)));
                if quiet && has {
                    stray.push(format!("({r},{c})"));
                }
            }
        }
        if !missing.is_empty() || !stray.is_empty() {
            let shot = self.dir.join(format!("ink-{id}-pane{t}.ppm"));
            img.save(&shot);
            self.fail(format!(
                "checkpoint {id}: pane {t} paint against its grid: no ink at {} cells {:?}, ink on {} blank cells {:?} ({})",
                missing.len(),
                missing.iter().take(8).collect::<Vec<_>>(),
                stray.len(),
                stray.iter().take(8).collect::<Vec<_>>(),
                shot.display()
            ));
        }
    }

    /// The pane's rows against the screen its agent says it drew.
    fn check_grid(&mut self, t: usize, id: &str) {
        if let Some(what) = self.grid_mismatch(t, id) {
            self.fail(what);
        }
    }

    /// How the pane's rows differ from the screen its agent says it drew, if they do.
    fn grid_mismatch(&mut self, t: usize, id: &str) -> Option<String> {
        let pid = self.tracks[t].pid;
        let Ok(text) = std::fs::read_to_string(self.agents.join(format!("{pid}.screen"))) else {
            return Some(format!(
                "checkpoint {id}: agent of pane {t} wrote no screen"
            ));
        };
        let mut lines = text.lines();
        if lines.next() != Some(id) {
            return Some(format!(
                "checkpoint {id}: agent of pane {t} didn't hold still"
            ));
        }
        let drawn: Vec<&str> = lines.collect();
        let rows = self.pane_read(t, |p| {
            p.visible_lines()
                .map(|l| p.row_text(l).unwrap_or_default())
                .collect::<Vec<_>>()
        });
        rows.iter().enumerate().find_map(|(r, got)| {
            let want = drawn.get(r).copied().unwrap_or("").trim_end();
            (got.trim_end() != want).then(|| {
                format!(
                    "checkpoint {id}: pane {t} row {r} shows {:?}, its agent drew {want:?}",
                    got.trim_end()
                )
            })
        })
    }
}

struct Image {
    width: usize,
    pixels: Vec<u8>,
}

impl Image {
    fn at(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 4;
        match self.pixels.get(i..i + 3) {
            Some(p) => [p[0], p[1], p[2]],
            None => [0; 3],
        }
    }

    fn differ(&self, other: &Image, x0: usize, y0: usize, x1: usize, y1: usize) -> usize {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| self.at(x, y) != other.at(x, y))
            .count()
    }

    /// As a PPM (Preview opens it), for looking at a failure.
    fn save(&self, path: &Path) {
        let height = self.pixels.len() / 4 / self.width.max(1);
        let mut out = format!("P6\n{} {height}\n255\n", self.width).into_bytes();
        for p in self.pixels.as_chunks::<4>().0 {
            out.extend_from_slice(&p[..3]);
        }
        let _ = std::fs::write(path, out);
    }
}

/// Whether `part` appears in `whole` in order, gaps allowed.
fn in_order<T: PartialEq>(part: &[T], whole: &[T]) -> bool {
    let mut it = whole.iter();
    part.iter().all(|p| it.any(|w| w == p))
}

fn ios(v: &[bool]) -> String {
    v.iter().map(|&f| if f { 'I' } else { 'O' }).collect()
}

impl Run {
    /// Each agent's log against what the test meant for it.
    fn analyze(&mut self) {
        for t in 0..self.tracks.len() {
            let pid = self.tracks[t].pid;
            let log =
                std::fs::read_to_string(self.agents.join(format!("{pid}.ev"))).unwrap_or_default();
            let (mut typed, mut pastes) = (String::new(), Vec::new());
            let mut sizes: Vec<(usize, usize)> = Vec::new();
            let (mut toggles, mut offs, mut queries) = (false, 0, 0);
            let mut replies = std::collections::HashMap::<String, usize>::new();
            let mut asked = std::collections::VecDeque::new();
            let mut slowest = 0;
            let mut last: Option<bool> = None;
            let mut reports = Vec::new();
            for line in log.lines() {
                let Some((ms, rest)) = line.split_once(' ') else {
                    continue;
                };
                let ms: u128 = ms.parse().unwrap_or(0);
                let (kind, arg) = rest.split_once(' ').unwrap_or((rest, ""));
                let nums: Vec<usize> = arg
                    .split([' ', '='])
                    .filter_map(|w| w.parse().ok())
                    .collect();
                match kind {
                    "start" => {
                        sizes.push((nums[0], nums[1]));
                        toggles = arg.ends_with("toggles=true");
                    }
                    "size" => {
                        let s = (nums[0], nums[1]);
                        if s.0 == 0 || s.1 == 0 {
                            self.fail(format!("pane {t} was resized to {s:?}"));
                        }
                        if sizes.last() == Some(&s) {
                            self.fail(format!(
                                "pane {t} got SIGWINCH ({} signals) and kept {s:?}: a size and back",
                                nums[2]
                            ));
                        }
                        sizes.push(s);
                    }
                    "mode1004" if arg == "on" => last = None,
                    "mode1004" => offs += 1,
                    "focus" => {
                        let (which, mode) = arg.split_once(' ').unwrap_or((arg, ""));
                        let v = which == "I";
                        if mode == "off" {
                            self.fail(format!(
                                "pane {t} got a focus report ({which}) with 1004 off"
                            ));
                        }
                        if last == Some(v) {
                            self.fail(format!("pane {t} got focus {which} twice in a row"));
                        }
                        last = Some(v);
                        if ms >= self.t0 {
                            reports.push(v);
                        }
                    }
                    "query" => {
                        queries += 1;
                        asked.push_back(ms);
                    }
                    "reply" => {
                        let (what, content) = arg.split_once(' ').unwrap_or((arg, ""));
                        *replies.entry(what.to_string()).or_default() += 1;
                        if what == "xtversion" && content != "\"GitViber\"" {
                            self.fail(format!("pane {t}: XTVERSION answered {content}"));
                        }
                        // The answer to the last of the four questions.
                        if what == "cpr"
                            && let Some(q) = asked.pop_front()
                        {
                            slowest = slowest.max(ms.saturating_sub(q));
                        }
                    }
                    "text" => typed.push_str(arg.trim_matches('"')),
                    "paste" => pastes.push(arg.trim_matches('"').to_string()),
                    "other" => self.fail(format!("pane {t} got bytes nobody sent: {arg}")),
                    _ => {}
                }
            }
            let tr = &self.tracks[t];
            let mut bad = Vec::new();
            if typed != tr.typed {
                bad.push(format!("typed {typed:?}, meant {:?}", tr.typed));
            }
            if pastes != tr.pastes {
                bad.push(format!("pastes {pastes:?}, meant {:?}", tr.pastes));
            }
            for what in ["xtversion", "da", "osc11", "cpr"] {
                let n = replies.get(what).copied().unwrap_or(0);
                if n != queries {
                    bad.push(format!("{n} {what} answers to {queries} questions"));
                }
            }
            if replies.get("dsr").copied().unwrap_or(0) != offs {
                bad.push(format!("{:?} DSR answers to {offs}", replies.get("dsr")));
            }
            if slowest > 1000 {
                bad.push(format!("an answer took {slowest} ms"));
            }
            if !in_order(&sizes, &tr.sizes) {
                bad.push(format!(
                    "told sizes {sizes:?}, while it showed {:?}",
                    tr.sizes
                ));
            }
            let grid = self.cx.update(|cx| {
                let emu = tr.pane.read(cx).session.emu.lock();
                (emu.term.columns(), emu.term.screen_lines())
            });
            if sizes.last() != Some(&grid) {
                bad.push(format!(
                    "ends told {:?}, its grid is {grid:?}",
                    sizes.last()
                ));
            }
            // Each report is one of the changes: all of them, but for those while 1004 was off.
            let meant = &tr.focus[1..];
            if (!toggles && reports != meant) || !in_order(&reports, meant) {
                bad.push(format!(
                    "focus reports {}, its focus changed {}",
                    ios(&reports),
                    ios(meant)
                ));
            }
            eprintln!(
                "pane {t} (agent {pid}{}): {} typed, {} pastes, {} focus reports, sizes {sizes:?}, {queries}×4 questions, slowest answer {slowest} ms",
                if toggles { ", toggles 1004" } else { "" },
                typed.len(),
                pastes.len(),
                reports.len()
            );
            for b in bad {
                self.fail(format!("pane {t}: {b}"));
            }
        }
    }
}

/// Six fake agents in five tabs and a split, churned through tabs, panes, the panel, zoom,
/// selections, typing, pastes and window focus: each gets only its own input, a resize only when
/// its own size changes, and each pane shows what its agent drew, painted as from nothing.
#[test]
fn panes_get_only_their_input_and_paint_their_grid() {
    let secs = std::env::var("GITVIBER_STRESS_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SECS);
    let seed = std::env::var("GITVIBER_STRESS_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(now_ms() as u64 | 1);
    eprintln!("stress: {secs} s, GITVIBER_STRESS_SEED={seed}");
    let dir = std::env::temp_dir().join(format!("gitviber-stress-{}", std::process::id()));
    let (home, config, agents, work) = (
        dir.join("home"),
        dir.join("config"),
        dir.join("agents"),
        dir.join("work"),
    );
    for d in [&home, &config, &agents, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    // Every pane's shell: this binary again, on the fake agent alone; its tty is the pane.
    let exe = std::env::current_exe().unwrap();
    let script = dir.join("agent.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n{}='{}' exec '{}' --exact {} --ignored --nocapture --test-threads=1 >/dev/null 2>&1\n",
            agent::DIR_VAR,
            agents.display(),
            exe.display(),
            agent::TEST_NAME
        ),
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
    // The panes' reader threads wake the UI as the app's do; the test scheduler would take a
    // backtrace of each such wake (seconds, under its lock) unless parking was ever allowed.
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
    type Made = Option<(Entity<Host>, Entity<TerminalPanel>)>;
    let made: std::rc::Rc<std::cell::RefCell<Made>> = Default::default();
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
            *slot.borrow_mut() = Some((host.clone(), panel));
            cx.new(|cx| gpui_kit::base::Root::new(AnyView::from(host), window, cx))
        })
        .unwrap()
        .into();
    let other: AnyWindowHandle = cx
        .open_window(size(px(200.), px(100.)), |window, cx| {
            let view = cx.new(|_| Other);
            cx.new(|cx| gpui_kit::base::Root::new(AnyView::from(view), window, cx))
        })
        .unwrap()
        .into();
    let (host, panel) = made.borrow_mut().take().unwrap();
    let _ = cx.update_window(window, |_, w, _| w.activate_window());
    let scale = cx
        .update_window(window, |_, w, _| w.scale_factor())
        .unwrap();
    let mut run = Run {
        cx,
        window,
        scale,
        other,
        host,
        panel: panel.clone(),
        tracks: Vec::new(),
        model: Model {
            tabs: Vec::new(),
            focused: Vec::new(),
            active: 0,
            shown: true,
            window_active: true,
            zoomed: false,
        },
        rng: Rng(seed),
        dir: dir.clone(),
        agents: agents.clone(),
        failures: Vec::new(),
        trace: Vec::new(),
        t0: 0,
        typed_nowhere: 0,
        actions: 0,
        checkpoints: 0,
        clock: Instant::now(),
    };
    run.pump(50);

    // Five tabs, the second split: six terminals.
    let mut panes: Vec<Vec<Entity<TerminalPane>>> = Vec::new();
    let mut seen = Vec::new();
    // The sizes each pane shows from its first, as `Track::sizes`.
    type Seen = Vec<(Entity<TerminalPane>, Vec<(usize, usize)>)>;
    let record = |run: &mut Run, seen: &mut Seen, list: &[Entity<TerminalPane>]| {
        for p in list {
            let Some(m) = run.cx.update(|cx| p.read(cx).metrics) else {
                continue;
            };
            let s = (m.cols, m.rows);
            match seen.iter_mut().find(|(q, _)| q == p) {
                Some((_, v)) if v.last() != Some(&s) => v.push(s),
                Some(_) => {}
                None => seen.push((p.clone(), vec![s])),
            }
        }
    };
    for _ in 0..5 {
        let _ = run.cx.update_window(window, |_, w, cx| {
            panel.update(cx, |p, cx| p.open_terminal(work.clone(), None, w, cx))
        });
        run.pump(100);
        let pane = run.cx.update(|cx| panel.read(cx).active_pane()).unwrap();
        panes.push(vec![pane.clone()]);
        record(&mut run, &mut seen, &[pane]);
    }
    key(&mut run.cx, window, "cmd-2");
    run.pump(100);
    let _ = run.cx.update_window(window, |_, w, cx| {
        panel.update(cx, |p, cx| p.split(Dir::Row, None, w, cx))
    });
    run.pump(200);
    let split = run.cx.update(|cx| panel.read(cx).active_pane()).unwrap();
    panes[1].push(split);
    record(&mut run, &mut seen, &panes[1]);

    // Each agent names itself in its pane's title; a freshly built binary's first runs can wait
    // on macOS's scan of it.
    let deadline = Instant::now() + Duration::from_secs(60);
    let all: Vec<Entity<TerminalPane>> = panes.iter().flatten().cloned().collect();
    let pids = loop {
        let pids: Vec<Option<u32>> = all
            .iter()
            .map(|p| {
                run.cx.update(|cx| {
                    p.read(cx)
                        .title()
                        .and_then(|t| t.strip_prefix("agent "))
                        .and_then(|n| n.parse().ok())
                })
            })
            .collect();
        if pids.iter().all(Option::is_some) {
            break pids.into_iter().flatten().collect::<Vec<u32>>();
        }
        assert!(
            Instant::now() < deadline,
            "the agents didn't start: {pids:?}"
        );
        run.pump(50);
    };
    let mut index = 0;
    for tab in &panes {
        let mut ids = Vec::new();
        for p in tab {
            run.tracks.push(Track {
                pane: p.clone(),
                pid: pids[index],
                typed: String::new(),
                pastes: Vec::new(),
                focus: Vec::new(),
                sizes: seen
                    .iter()
                    .find(|(q, _)| q == p)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default(),
            });
            ids.push(index);
            index += 1;
        }
        run.model.focused.push(ids.len() - 1);
        run.model.tabs.push(ids);
    }
    // The tracks hold the panes from here.
    drop((panes, all, seen));
    run.model.active = 1;
    run.pump(1000);
    run.observe();
    run.t0 = now_ms();
    for t in 0..run.tracks.len() {
        let f = run.model.focused(t);
        run.tracks[t].focus = vec![f];
    }

    let started = Instant::now();
    let every = Duration::from_secs(if secs > 60 { 30 } else { 8 });
    let mut next_check = started + every;
    while started.elapsed() < Duration::from_secs(secs) {
        run.act();
        let wait = 100 + run.rng.below(201);
        run.pump(wait);
        run.observe();
        if Instant::now() >= next_check {
            run.checkpoint();
            next_check = Instant::now() + every;
        }
    }
    run.checkpoint();
    run.analyze();
    eprintln!(
        "stress: {} actions, {} checkpoints, {} chars typed with the panel hidden",
        run.actions, run.checkpoints, run.typed_nowhere
    );

    let failures = std::mem::take(&mut run.failures);
    run.cx
        .update(|cx| panel.update(cx, |p, cx| p.close_all(cx)));
    let _ = run.cx.update_window(window, |_, w, _| w.remove_window());
    let _ = run.cx.update_window(other, |_, w, _| w.remove_window());
    run.cx.run_until_parked();
    // The handles go before the app does: it counts any left as leaked.
    let Run {
        cx,
        tracks,
        host,
        panel: own,
        ..
    } = run;
    drop((tracks, host, own, panel));
    drop(cx);
    if failures.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(
        failures.is_empty(),
        "{} failures (seed {seed}, logs in {}):\n{}",
        failures.len(),
        dir.display(),
        failures.join("\n")
    );
}
