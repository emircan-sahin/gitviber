//! The one `gitviber_core::Core` of the app and its events on the UI thread. Core calls block:
//! run them with `cx.background_spawn`, holding `core(cx)`'s Arc, never on the UI thread.
//!
//! ```ignore
//! let core = gitviber_ui::core(cx);
//! self.load = Some(cx.spawn(async move |this, cx| {
//!     let status = cx.background_spawn(async move { api::changes::status(&core) }).await;
//!     this.update(cx, |this, cx| { this.status = status.ok(); cx.notify() }).ok();
//! }));
//! cx.subscribe(&CoreEvents::global(cx), |this, _, event: &Event, cx| ..)
//! ```

use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gitviber_core::{Core, Event, EventSink};
use gpui_kit::{App, AppContext as _, Entity, EventEmitter, Global, Task};

/// Every core `Event`, emitted on the UI thread: subscribe with
/// `cx.subscribe(&CoreEvents::global(cx), ..)`.
pub struct CoreEvents;

impl EventEmitter<Event> for CoreEvents {}

impl CoreEvents {
    pub fn global(cx: &App) -> Entity<CoreEvents> {
        cx.global::<CoreGlobal>().events.clone()
    }
}

struct CoreGlobal {
    core: Arc<Core>,
    events: Entity<CoreEvents>,
    /// Takes events off the channel; dropped with the app.
    _pump: Task<()>,
}

impl Global for CoreGlobal {}

/// The app's core.
pub fn core(cx: &App) -> Arc<Core> {
    cx.global::<CoreGlobal>().core.clone()
}

pub fn has_core(cx: &App) -> bool {
    cx.has_global::<CoreGlobal>()
}

/// Makes the app's core, starts it (`Core::start`: the shell's PATH, logs, askpass, notifications,
/// the launch's paths) and puts its events on the UI thread. Once, before the first window.
pub fn start_core(cx: &mut App) -> Arc<Core> {
    let core = install_core(cx);
    core.start();
    core
}

/// As `start_core`, without starting anything in the background: for tests.
pub fn install_core(cx: &mut App) -> Arc<Core> {
    let (tx, mut rx) = mpsc::unbounded::<Event>();
    let sink: EventSink = Arc::new(move |event| {
        let _ = tx.unbounded_send(event);
    });
    let core = Arc::new(Core::new(sink));
    let events = cx.new(|_| CoreEvents);
    let weak = events.downgrade();
    let pump = cx.spawn(async move |cx| {
        while let Some(event) = rx.next().await {
            if weak.update(cx, |_, cx| cx.emit(event)).is_err() {
                break;
            }
        }
    });
    cx.set_global(CoreGlobal {
        core: core.clone(),
        events,
        _pump: pump,
    });
    core
}
