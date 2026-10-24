//! A web page in a center tab: its address bar and tools here, the page a native view laid over
//! the area below (placer.rs). Its view is made as the tab first shows and lives on hidden while
//! another does, until it parks (Settings → Browser); closing the tab closes it.

use super::devices::{self, DeviceChoice, Fitted};
use super::format::{self, AskAbout, Pick, ShownAs};
use super::hub::{Hub, HubEvent};
use super::placer::{self, Painted};
use super::url::{self, BLANK};
use crate::console::Entry;
use crate::ports::Port;
use crate::{Go, Look, Screen};
use gitviber_ui::actions::browser as act;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{page_label, Selection, TabIcon, TabItem, Toast, Workspace};
use gitviber_ui::{ActiveAppTheme as _, AppTheme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    canvas, div, img, px, relative, AnyElement, App, AppContext as _, Bounds, ClickEvent, Context,
    Entity, FocusHandle, Focusable, Hitbox, HitboxBehavior, Image, ImageFormat,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, RenderImage,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity,
    Window,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex, Disableable as _, Icon, Selectable as _, Sizable as _};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

/// The tab's key context: the browser's own commands (⌘L, ⌘R, ⌘F…) listen in it.
pub const CONTEXT: &str = "Browser";

/// Light or dark whatever the app is; None: as the app.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    fn parse(s: Option<&str>) -> Option<Self> {
        match s? {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// What the tab saves of itself: where its page is and how it shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub id: String,
    pub url: String,
    pub title: Option<String>,
    pub device: Option<DeviceChoice>,
    pub zoom: f64,
    pub scheme: Option<Scheme>,
}

impl Saved {
    pub fn of(selection: &Selection) -> Option<Self> {
        let Selection::Browser {
            id,
            url,
            title,
            device,
            zoom,
            scheme,
        } = selection
        else {
            return None;
        };
        Some(Self {
            id: id.clone(),
            url: if url::is_page_url(url) {
                url.clone()
            } else {
                BLANK.into()
            },
            title: title.clone(),
            // A name no longer in devices.json shows the page as itself.
            device: device
                .clone()
                .and_then(|d| serde_json::from_value::<DeviceChoice>(d).ok())
                .filter(|d| devices::device_of(d).is_some()),
            zoom: zoom
                .filter(|z| devices::ZOOMS.iter().any(|s| (s - z).abs() < 1e-9))
                .unwrap_or(1.0),
            scheme: Scheme::parse(scheme.as_deref()),
        })
    }

    pub fn selection(&self) -> Selection {
        Selection::Browser {
            id: self.id.clone(),
            url: self.url.clone(),
            title: self.title.clone().filter(|t| !t.is_empty()),
            device: self
                .device
                .as_ref()
                .and_then(|d| serde_json::to_value(d).ok()),
            zoom: (self.zoom != 1.0).then_some(self.zoom),
            scheme: self.scheme.map(|s| s.name().to_string()),
        }
    }
}

struct Find {
    input: Entity<InputState>,
    match_case: bool,
    /// Whether the last find matched; None before one, or where WebKit can't say.
    found: Option<bool>,
    /// The latest find: an answer for a query typed over since says nothing of this one.
    latest: u64,
    task: Task<()>,
    _events: Subscription,
}

struct Ask {
    about: AskAbout,
    input: Entity<TextareaState>,
    include_errors: bool,
    /// The models offered and the one it starts on (Claude Code only).
    model: Option<(Vec<String>, String)>,
    /// Where it's anchored in the page area, its px.
    at: (f32, f32),
    _events: Subscription,
}

/// Room around a device's body: Responsive's drag handles sit in it.
const PAD: f32 = 16.;

/// Responsive's right edge (`dx`), bottom edge (`dy`) or corner being dragged: where the drag
/// began, the scale then, and the size so far.
struct Drag {
    dx: f64,
    dy: f64,
    start: (f32, f32),
    scale: f64,
    from: (f64, f64),
    size: (f64, f64),
}

/// Responsive's Width × Height, typed: taken on ↵ or leaving the field, so "3" on the way to 320
/// isn't 200.
struct SizeFields {
    w: Entity<InputState>,
    h: Entity<InputState>,
    /// The size they last showed: a drag or a pick starts them over.
    shown: (f64, f64),
    _events: [Subscription; 2],
}

pub struct BrowserTab {
    saved: Saved,
    /// The worktree whose tab this is: its pages, sessions and terminals' ports.
    root: String,
    workspace: WeakEntity<Workspace>,
    hub: Option<Entity<Hub>>,
    focus: FocusHandle,
    /// Stands for the page in GPUI's focus: the browser's keys listen here while it has them.
    page_focus: FocusHandle,
    address: Entity<InputState>,
    editing: bool,
    invalid: bool,
    /// How often its view had parked when this one was made: another is due once it parks again.
    made: Option<u32>,
    error: Option<SharedString>,
    /// WebKit lets the page see a device's pixel ratio here.
    dpr: bool,
    /// A parked page's picture, standing in until it has loaded again.
    restoring: Option<Arc<RenderImage>>,
    /// The page's picture while something covers it.
    cover: Option<Arc<RenderImage>>,
    /// The page had the keys as it stepped aside: they go back to it after.
    refocus: bool,
    /// The page has the keys (its view is the window's first responder), as last heard.
    page_had_keys: bool,
    find: Option<Find>,
    console_open: bool,
    console: Vec<Entry>,
    picking: bool,
    gathered: Vec<Pick>,
    last_pick: Option<Pick>,
    picks: Option<Vec<Pick>>,
    ask: Option<Ask>,
    drag: Option<Drag>,
    size_fields: Option<SizeFields>,
    ports: Rc<std::cell::RefCell<Vec<Port>>>,
    /// The page area's size as last laid out, for device mode's fit.
    area: Rc<Cell<(f32, f32)>>,
    persist: Task<()>,
    picture_task: Task<()>,
    ports_task: Task<()>,
    _subscriptions: Vec<Subscription>,
    _live: Live,
}

fn handle() -> Option<&'static crate::Handle> {
    crate::installed()
}

/// A JPEG data URL (a page's picture) as an image to draw; None for anything else.
fn decode_picture(data_url: &str, svg: gpui::SvgRenderer) -> Option<Arc<RenderImage>> {
    let b64 = data_url.strip_prefix("data:image/jpeg;base64,")?;
    let bytes = base64(b64)?;
    Image::from_bytes(ImageFormat::Jpeg, bytes)
        .to_image_data(svg)
        .ok()
}

fn base64(text: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|b| **b == b'=').count();
        let mut n = 0u32;
        for (i, b) in chunk.iter().enumerate() {
            let v = if *b == b'=' { 0 } else { value(*b)? };
            n |= v << (18 - 6 * i as u32);
        }
        let take = chunk.len().saturating_sub(1).saturating_sub(pad);
        out.extend(n.to_be_bytes()[1..].iter().take(take));
    }
    Some(out)
}

impl BrowserTab {
    pub fn new(
        saved: Saved,
        root: String,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let hub = Hub::global(cx);
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("A port (5173) or an address")
                .default_value(if saved.url == BLANK {
                    String::new()
                } else {
                    saved.url.clone()
                })
        });
        let mut subscriptions = vec![
            cx.subscribe_in(&address, window, Self::on_address),
            cx.on_release(|this, cx| this.released(cx)),
            // Show agents' tabs switched: the strip asks `hidden` again.
            cx.observe_global::<gitviber_ui::Settings>(|_, cx| cx.notify()),
        ];
        if let Some(hub) = &hub {
            subscriptions.push(cx.subscribe_in(hub, window, Self::on_hub));
        }
        Self {
            saved,
            root: crate::root_key(&root),
            workspace,
            hub,
            focus: cx.focus_handle(),
            page_focus: cx.focus_handle(),
            address,
            editing: false,
            invalid: false,
            made: None,
            error: None,
            dpr: true,
            restoring: None,
            cover: None,
            refocus: false,
            page_had_keys: false,
            find: None,
            console_open: false,
            console: Vec::new(),
            picking: false,
            gathered: Vec::new(),
            last_pick: None,
            picks: None,
            ask: None,
            drag: None,
            size_fields: None,
            ports: Rc::default(),
            area: Rc::default(),
            persist: Task::ready(()),
            picture_task: Task::ready(()),
            ports_task: Task::ready(()),
            _subscriptions: subscriptions,
            _live: Live::new("BrowserTab"),
        }
    }

    pub fn id(&self) -> &str {
        &self.saved.id
    }

    fn key(&self) -> String {
        format!("browser::{}", self.saved.id)
    }

    fn state<'a>(&self, cx: &'a App) -> Option<&'a crate::PageState> {
        self.hub
            .as_ref()?
            .read(cx)
            .page(&self.saved.id)?
            .state
            .as_ref()
    }

    fn page_info<'a>(&self, cx: &'a App) -> Option<&'a super::hub::Page> {
        self.hub.as_ref()?.read(cx).page(&self.saved.id)
    }

    /// Where the page is now: as it said, else where the tab was left.
    fn current_url(&self, cx: &App) -> String {
        self.state(cx)
            .map(|s| s.url.clone())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| self.saved.url.clone())
    }

    /// Closed in its workspace, its page goes with it (an agent's too: the user closed it). Let
    /// go only for memory, or its worktree switched away, it hides and parks in time; an
    /// agent's lives on out of sight.
    fn released(&mut self, cx: &mut App) {
        let id = self.saved.id.clone();
        let key = self.key();
        let agents = self
            .page_info(cx)
            .is_some_and(|p| (p.agent.is_some() || p.shown_by.is_some()) && !p.released);
        let closed = match self.workspace.upgrade() {
            Some(ws) => {
                let ws = ws.read(cx);
                let here = ws.project().is_some_and(|p| {
                    crate::root_key(&p.read(cx).root().to_string_lossy()) == self.root
                });
                here && ws.tabs().get(&key).is_none()
            }
            // Its window closed: an agent's tab stays with its pane.
            None => !agents,
        };
        placer::forget(&id, cx);
        if !closed {
            crate::hide(&id, false);
            return;
        }
        crate::close(&id);
        if let Some(hub) = &self.hub {
            hub.update(cx, |hub, _| hub.forget(&id));
        }
    }

    // MARK: The page's view

    /// Makes the page's view now, as its first show would: for a window that isn't drawing (a
    /// demo's behind others, which macOS stops drawing).
    pub fn load_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_made(window, cx);
    }

    /// The page's view made, on the tab's first show and after each park.
    fn ensure_made(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parks = self.page_info(cx).map_or(0, |p| p.parks);
        if self.made == Some(parks) || self.error.is_some() {
            return;
        }
        let Some(handle) = handle() else {
            self.error = Some("The browser isn't set up in this window.".into());
            return;
        };
        if let Some(view) = super::native_view(window) {
            crate::attach_window(view);
        }
        let device = self.device();
        let look = Look {
            ua: device
                .as_ref()
                .map(|(d, _)| d.ua.clone())
                .filter(|u| !u.is_empty()),
            touch: device.as_ref().is_some_and(|(d, _)| d.touch()),
            zoom: Some(self.saved.zoom),
            dark: self.saved.scheme.map(|s| s == Scheme::Dark),
        };
        match crate::create(
            handle,
            &self.saved.id,
            &self.root,
            &self.saved.url,
            &look,
            false,
        ) {
            Ok(created) => {
                self.made = Some(parks);
                self.dpr = created.dpr;
                let id = self.saved.id.clone();
                let page = created.page;
                if let Some(hub) = &self.hub {
                    hub.update(cx, |hub, _| hub.page_mut(&id).state = Some(page));
                }
                if let Some(shot) = created.snapshot {
                    let svg = cx.svg_renderer();
                    self.picture_task = cx.spawn(async move |this, cx| {
                        let image = cx
                            .background_spawn(async move { decode_picture(&shot, svg) })
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            this.set_restoring(image, cx);
                            cx.notify();
                        });
                    });
                }
                // A new tab starts at its address bar.
                if self.saved.url == BLANK {
                    self.focus_address(window, cx);
                }
            }
            Err(e) => self.error = Some(e.into()),
        }
    }

    fn set_restoring(&mut self, image: Option<Arc<RenderImage>>, cx: &mut App) {
        if let Some(old) = std::mem::replace(&mut self.restoring, image) {
            cx.drop_image(old, None);
        }
    }

    fn set_cover(&mut self, image: Option<Arc<RenderImage>>, cx: &mut App) {
        if let Some(old) = std::mem::replace(&mut self.cover, image) {
            cx.drop_image(old, None);
        }
    }

    /// Something covers the page: its picture stands in (placer.rs).
    pub fn covered(&mut self, shot: Option<String>, cx: &mut Context<Self>) {
        self.refocus = self.refocus || self.page_keys();
        let svg = cx.svg_renderer();
        self.picture_task = cx.spawn(async move |this, cx| {
            let image = cx
                .background_spawn(async move { shot.and_then(|s| decode_picture(&s, svg)) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.set_cover(image, cx);
                cx.notify();
            });
        });
    }

    /// Back on show: the picture goes, and the keys go back to the page if it had them.
    pub fn uncovered(&mut self, cx: &mut Context<Self>) {
        self.set_cover(None, cx);
        if std::mem::take(&mut self.refocus) {
            crate::focus(&self.saved.id, true);
        }
        cx.notify();
    }

    fn page_keys(&self) -> bool {
        self.page_had_keys
    }

    fn device(&self) -> Option<(devices::Device, DeviceChoice)> {
        let choice = self.saved.device.clone()?;
        devices::device_of(&choice).map(|d| (d, choice))
    }

    /// The device as laid out in the page area, Responsive at its size as dragged so far.
    fn fitted(&self) -> Option<(devices::Device, DeviceChoice, Fitted)> {
        let mut choice = self.saved.device.clone()?;
        if let Some(d) = &self.drag {
            (choice.w, choice.h) = (Some(d.size.0), Some(d.size.1));
        }
        let device = devices::device_of(&choice)?;
        let area = self.area.get();
        let room = (
            f64::from((area.0 - 2. * PAD).max(0.)),
            f64::from((area.1 - 2. * PAD).max(0.)),
        );
        let mut f = devices::fit(&device, choice.rotated, room);
        let pad = f64::from(PAD);
        for b in [&mut f.frame, &mut f.screen, &mut f.page] {
            b.x += pad;
            b.y += pad;
        }
        Some((device, choice, f))
    }

    /// Responsive's size fields, made on its first show and set to `size` when it changed
    /// elsewhere (a drag), unless one is being typed in.
    fn size_fields(
        &mut self,
        size: (f64, f64),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(Entity<InputState>, Entity<InputState>)> {
        let text = |n: f64| format!("{}", n.round());
        if self.size_fields.is_none() {
            let mut field = |n: f64, cx: &mut Context<Self>| {
                cx.new(|cx| InputState::new(window, cx).default_value(text(n)))
            };
            let (w, h) = (field(size.0, cx), field(size.1, cx));
            let take = |side: usize| {
                move |this: &mut Self,
                      _: &Entity<InputState>,
                      e: &InputEvent,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
                    if matches!(e, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.take_size(side, window, cx);
                    }
                }
            };
            let events = [
                cx.subscribe_in(&w, window, take(0)),
                cx.subscribe_in(&h, window, take(1)),
            ];
            self.size_fields = Some(SizeFields {
                w,
                h,
                shown: size,
                _events: events,
            });
        }
        let f = self.size_fields.as_mut()?;
        let typing =
            f.w.focus_handle(cx).is_focused(window) || f.h.focus_handle(cx).is_focused(window);
        if f.shown != size && !typing {
            f.shown = size;
            let (w, h) = (f.w.clone(), f.h.clone());
            w.update(cx, |i, cx| i.set_value(text(size.0), window, cx));
            h.update(cx, |i, cx| i.set_value(text(size.1), window, cx));
        }
        Some((f.w.clone(), f.h.clone()))
    }

    /// A side typed in, kept within Responsive's bounds; anything else shows the size again.
    fn take_size(&mut self, side: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(f), Some((device, _))) = (self.size_fields.as_ref(), self.device()) else {
            return;
        };
        let field = if side == 0 { f.w.clone() } else { f.h.clone() };
        let typed = field.read(cx).value().trim().parse::<f64>().ok();
        match typed.filter(|n| n.is_finite()) {
            Some(n) => {
                let n = devices::clamp_side(n);
                let (w, h) = if side == 0 {
                    (n, device.h)
                } else {
                    (device.w, n)
                };
                if let Some(f) = self.size_fields.as_mut() {
                    // Shown as kept, clamped.
                    f.shown = (f64::NAN, f64::NAN);
                }
                if (w, h) != (device.w, device.h) {
                    let next = DeviceChoice {
                        name: devices::RESPONSIVE.into(),
                        w: Some(w),
                        h: Some(h),
                        ..DeviceChoice::default()
                    };
                    self.choose_device(Some(next), window, cx);
                }
            }
            None => {
                if let Some(f) = self.size_fields.as_mut() {
                    f.shown = (f64::NAN, f64::NAN);
                }
            }
        }
        cx.notify();
    }

    /// A drag of Responsive's edges begins.
    fn start_drag(&mut self, dx: f64, dy: f64, at: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some((device, _, f)) = self.fitted() else {
            return;
        };
        self.drag = Some(Drag {
            dx,
            dy,
            start: (f32::from(at.x), f32::from(at.y)),
            scale: f.scale.max(0.01),
            from: (device.w, device.h),
            size: (device.w, device.h),
        });
        cx.notify();
    }

    /// Twice the pointer's move: the screen stays centered, so each edge goes half the way.
    fn drag_to(&mut self, at: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(d) = self.drag.as_mut() else {
            return;
        };
        let moved = (
            f64::from(f32::from(at.x) - d.start.0),
            f64::from(f32::from(at.y) - d.start.1),
        );
        d.size = (
            devices::clamp_side(d.from.0 + d.dx * 2. * moved.0 / d.scale),
            devices::clamp_side(d.from.1 + d.dy * 2. * moved.1 / d.scale),
        );
        cx.notify();
    }

    /// Let go: the size is kept with the tab.
    fn end_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.drag.take() else {
            return;
        };
        if d.size != d.from {
            let next = DeviceChoice {
                name: devices::RESPONSIVE.into(),
                w: Some(d.size.0),
                h: Some(d.size.1),
                ..DeviceChoice::default()
            };
            self.choose_device(Some(next), window, cx);
        }
        cx.notify();
    }

    // MARK: Events

    fn on_hub(
        &mut self,
        _: &Entity<Hub>,
        event: &HubEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            HubEvent::Changed(id) if *id == self.saved.id => {
                self.page_changed(window, cx);
                cx.notify();
            }
            HubEvent::Focused(id) if *id == self.saved.id => {
                self.page_had_keys = true;
                self.page_focus.focus(window, cx);
                cx.notify();
            }
            HubEvent::Parked(id) if *id == self.saved.id => cx.notify(),
            _ => {}
        }
    }

    fn page_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (url, title, loaded) = match self.state(cx) {
            Some(s) => (
                s.url.clone(),
                s.title.clone(),
                s.committed || s.failed.is_some(),
            ),
            None => return,
        };
        if loaded && self.restoring.is_some() {
            self.set_restoring(None, cx);
        }
        if !self.editing && !url.is_empty() {
            let shown = if url == BLANK {
                String::new()
            } else {
                url.clone()
            };
            if self.address.read(cx).value() != shown.as_str() {
                self.address
                    .update(cx, |a, cx| a.set_value(shown, window, cx));
            }
        }
        self.take_picks(window, cx);
        if self.console_open {
            self.read_console(cx);
        }
        // Kept for the next launch: once it settles, not every redirect.
        let title = Some(title).filter(|t| !t.is_empty());
        if !url.is_empty() && (url != self.saved.url || title != self.saved.title) {
            self.persist = cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.saved.url = url;
                    this.saved.title = title;
                    this.save(window, cx);
                });
            });
        }
    }

    /// The tab's selection updated in its workspace, which saves it.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = self.workspace.upgrade() else {
            return;
        };
        // An agent's tab isn't kept with the workspace (agents.rs).
        if self
            .page_info(cx)
            .is_some_and(|p| p.agent.is_some() && !p.released)
        {
            cx.notify();
            return;
        }
        let selection = self.saved.selection();
        let key: SharedString = self.key().into();
        // After this update: the workspace reads the tab's view as it saves.
        window.defer(cx, move |_, cx| {
            ws.update(cx, |ws, cx| ws.set_selection(&key, selection, cx));
        });
        cx.notify();
    }

    fn on_address(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Focus => {
                self.editing = true;
                self.page_had_keys = false;
                self.address.update(cx, |a, cx| a.select_all(window, cx));
            }
            InputEvent::Blur => {
                self.editing = false;
                self.invalid = false;
                let shown = self.current_url(cx);
                let shown = if shown == BLANK { String::new() } else { shown };
                self.address
                    .update(cx, |a, cx| a.set_value(shown, window, cx));
            }
            InputEvent::Change => self.invalid = false,
            InputEvent::PressEnter { .. } => {
                let typed = self.address.read(cx).value().to_string();
                match url::normalize_url(&typed) {
                    Some(next) => self.open_url(&next, window, cx),
                    None => self.invalid = true,
                }
            }
        }
        cx.notify();
    }

    fn open_url(&mut self, next: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = false;
        self.invalid = false;
        if let Err(e) = crate::navigate(&self.saved.id, next) {
            self.toast(Toast::error(e).title("Could not open the page"), window, cx);
            return;
        }
        self.keys_to_page(window, cx);
    }

    /// The keys to the page, unless it never loaded (its message is shown instead).
    fn keys_to_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state(cx).is_some_and(|s| s.failed.is_some()) {
            return;
        }
        self.page_focus.focus(window, cx);
        self.page_had_keys = true;
        crate::focus(&self.saved.id, true);
    }

    fn focus_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The keys come back from the page first, or they'd still go to its view.
        crate::focus(&self.saved.id, false);
        self.page_had_keys = false;
        self.address.update(cx, |a, cx| {
            a.focus(window, cx);
            a.select_all(window, cx);
        });
    }

    fn go(&mut self, to: Go, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = crate::go(&self.saved.id, to) {
            self.toast(Toast::error(e).title("The page didn't respond"), window, cx);
        }
    }

    fn toast(&self, toast: Toast, window: &mut Window, cx: &mut App) {
        match self.workspace.upgrade() {
            Some(ws) => ws.update(cx, |ws, cx| ws.toast(toast, cx)),
            None => toast.show(window, cx),
        }
    }

    // MARK: Look

    fn zoom_by(&mut self, by: i32, window: &mut Window, cx: &mut Context<Self>) {
        // Device mode zooms the page to fit, so its own zoom waits until that's off.
        if self.saved.device.is_some() {
            return;
        }
        self.saved.zoom = devices::step_zoom(self.saved.zoom, by);
        let _ = crate::zoom(&self.saved.id, self.saved.zoom);
        self.save(window, cx);
    }

    fn cycle_scheme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.saved.scheme = match self.saved.scheme {
            None => Some(Scheme::Light),
            Some(Scheme::Light) => Some(Scheme::Dark),
            Some(Scheme::Dark) => None,
        };
        self.apply_scheme();
        self.save(window, cx);
    }

    fn apply_scheme(&self) {
        let _ = crate::appearance(&self.saved.id, self.saved.scheme.map(|s| s == Scheme::Dark));
    }

    pub fn set_scheme(&mut self, dark: Option<bool>, window: &mut Window, cx: &mut Context<Self>) {
        self.saved.scheme = dark.map(|d| if d { Scheme::Dark } else { Scheme::Light });
        self.save(window, cx);
    }

    fn choose_device(
        &mut self,
        next: Option<DeviceChoice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(next) = &next {
            super::store::set_last_device(next, cx);
        }
        self.saved.device = next.filter(|d| devices::device_of(d).is_some());
        self.apply_device();
        self.save(window, cx);
    }

    /// The device's user agent and touch: the page loads again under them.
    fn apply_device(&self) {
        if self.made.is_none() {
            return;
        }
        let device = self.device();
        let ua = device
            .as_ref()
            .map(|(d, _)| d.ua.clone())
            .filter(|u| !u.is_empty());
        let touch = device.as_ref().is_some_and(|(d, _)| d.touch());
        let _ = crate::set_agent(&self.saved.id, ua.as_deref(), touch);
        if device.is_none() {
            let _ = crate::zoom(&self.saved.id, self.saved.zoom);
        }
    }

    /// An agent chose the tab's device (None: as itself).
    pub fn set_device(
        &mut self,
        device: Option<DeviceChoice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saved.device = device.filter(|d| devices::device_of(d).is_some());
        self.save(window, cx);
    }

    fn toggle_device(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = match self.saved.device {
            Some(_) => None,
            None => {
                Some(super::store::last_device(cx).unwrap_or_else(DeviceChoice::default_device))
            }
        };
        self.choose_device(next, window, cx);
    }

    // MARK: Tools

    fn inspect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match crate::inspect(&self.saved.id) {
            Ok(true) => {}
            Ok(false) => self.toast(
                Toast::info("Right-click the page and choose Inspect Element.")
                    .title("Web Inspector"),
                window,
                cx,
            ),
            Err(e) => self.toast(
                Toast::error(e).title("Could not open Web Inspector"),
                window,
                cx,
            ),
        }
    }

    fn show_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::focus(&self.saved.id, false);
        self.page_had_keys = false;
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in page"));
            let events =
                cx.subscribe_in(&input, window, |this, _, e: &InputEvent, _, cx| match e {
                    InputEvent::Change => this.run_find(false, cx),
                    InputEvent::PressEnter { shift, .. } => this.run_find(*shift, cx),
                    _ => {}
                });
            self.find = Some(Find {
                input,
                match_case: false,
                found: None,
                latest: 0,
                task: Task::ready(()),
                _events: events,
            });
        }
        if let Some(f) = &self.find {
            f.input.update(cx, |i, cx| {
                i.focus(window, cx);
                i.select_all(window, cx);
            });
        }
        cx.notify();
    }

    /// The next match (⌘G), or the one before; with nothing to find yet, the box.
    fn find_step(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let has_query = self
            .find
            .as_ref()
            .is_some_and(|f| !f.input.read(cx).value().is_empty());
        if has_query {
            self.run_find(backwards, cx);
        } else {
            self.show_find(window, cx);
        }
    }

    fn run_find(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let Some(f) = self.find.as_mut() else {
            return;
        };
        let text = f.input.read(cx).value().to_string();
        f.latest += 1;
        let n = f.latest;
        if text.is_empty() {
            f.found = None;
            cx.notify();
            return;
        }
        let (Some(handle), id, case) = (handle(), self.saved.id.clone(), f.match_case) else {
            return;
        };
        f.task = cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { crate::find(handle, id, text, backwards, case) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(f) = this.find.as_mut().filter(|f| f.latest == n) {
                    f.found = found.ok().flatten();
                    cx.notify();
                }
            });
        });
    }

    fn toggle_match_case(&mut self, cx: &mut Context<Self>) {
        if let Some(f) = self.find.as_mut() {
            f.match_case = !f.match_case;
        }
        self.run_find(false, cx);
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        self.keys_to_page(window, cx);
        cx.notify();
    }

    fn toggle_console(&mut self, cx: &mut Context<Self>) {
        self.console_open = !self.console_open;
        if self.console_open {
            self.read_console(cx);
        } else {
            self.console = Vec::new();
        }
        cx.notify();
    }

    fn read_console(&mut self, _: &mut Context<Self>) {
        self.console = crate::console_entries(&self.saved.id).unwrap_or_default();
    }

    fn page_shown_as(&self) -> Option<ShownAs> {
        let (device, _, f) = self.fitted()?;
        Some(ShownAs {
            name: device.name,
            viewport: f.viewport,
        })
    }

    fn errors_text(&self, cx: &App) -> String {
        let entries = crate::console_entries(&self.saved.id).unwrap_or_default();
        let url = self.current_url(cx);
        let shown = self.page_shown_as();
        format::format_errors(&entries, Some((&url, shown.as_ref())))
    }

    fn set_picking(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.picking = on;
        self.gathered.clear();
        self.last_pick = None;
        if on {
            self.picks = None;
            self.ask = None;
        }
        match crate::pick(&self.saved.id, on) {
            // Into the page: Esc there ends the picker.
            Ok(()) if on => self.keys_to_page(window, cx),
            Ok(()) => {}
            Err(e) => self.toast(
                Toast::error(e).title("Could not pick an element"),
                window,
                cx,
            ),
        }
        cx.notify();
    }

    /// The picker's news: ⇧-clicks gather a few on the way; a plain click picks the last.
    fn take_picks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hub) = self.hub.clone() else {
            return;
        };
        let picked = hub.update(cx, |hub, _| hub.take_picked(&self.saved.id));
        if picked.is_empty() {
            return;
        }
        for p in picked {
            match p {
                Some(p) if p.more => self.gathered.push(p),
                p => {
                    self.picking = false;
                    if p.is_none() {
                        self.gathered.clear();
                    }
                    self.last_pick = p;
                }
            }
        }
        let Some(last) = self.last_pick.clone() else {
            return;
        };
        if (self.gathered.len() as u32) < last.number.max(1) - 1 {
            return;
        }
        let mut done = std::mem::take(&mut self.gathered);
        done.push(last);
        done.sort_by_key(|p| p.number);
        self.last_pick = None;
        let shown = self.page_shown_as();
        if let Some(about) = format::picks_about(&done, shown.as_ref()) {
            // In the area's px: below the last pick, past the device's frame in device mode.
            let origin = self
                .fitted()
                .map_or((0., 0.), |(_, _, f)| (f.page.x, f.page.y));
            let at = done.last().map_or((8., 8.), |p| {
                let factor = self.factor();
                (
                    (origin.0 + p.bounds.x * factor) as f32,
                    (origin.1 + (p.bounds.y + p.bounds.h) * factor) as f32 + 6.,
                )
            });
            self.picks = Some(done);
            self.open_ask(about, at, window, cx);
        }
    }

    /// A page px in the tab's px: the device's scale in device mode, else the page's zoom.
    fn factor(&self) -> f64 {
        match self.fitted() {
            Some((_, _, f)) => f.scale,
            None => self.saved.zoom,
        }
    }

    fn open_ask(
        &mut self,
        about: AskAbout,
        at: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The keys come back from the page first, or the question's field would get none.
        crate::focus(&self.saved.id, false);
        self.page_had_keys = false;
        // Three lines, growing with what's typed to about sixteen; ↵ asks, ⇧↵ is a new line.
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Your question (optional). ↵ asks, ⇧↵ is a new line.")
                .submit_on_enter(true)
                .auto_grow(3, 16)
        });
        let events = cx.subscribe_in(&input, window, |this, input, e: &InputEvent, window, cx| {
            match e {
                // ↵ asks a question; without one, Skip is the button.
                InputEvent::PressEnter { shift: false, .. }
                    if !input.read(cx).value().trim().is_empty() =>
                {
                    this.send_ask(true, window, cx)
                }
                InputEvent::Change => cx.notify(),
                _ => {}
            }
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        let last = super::store::ask_model(cx);
        let model = super::ask::model_choices(last.as_deref(), cx);
        self.ask = Some(Ask {
            about,
            input,
            include_errors: false,
            model,
            at,
            _events: events,
        });
        cx.notify();
    }

    /// The model picked to ask with, remembered for the next question of a page.
    fn pick_model(&mut self, model: String, cx: &mut Context<Self>) {
        super::store::set_ask_model(&model, cx);
        if let Some((_, m)) = self.ask.as_mut().and_then(|a| a.model.as_mut()) {
            *m = model;
        }
        cx.notify();
    }

    fn close_ask(&mut self, cx: &mut Context<Self>) {
        self.ask = None;
        self.picks = None;
        cx.notify();
    }

    /// Ask (↵) starts a session with it sent; Skip one with it waiting in the prompt box.
    fn send_ask(&mut self, send: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ask) = self.ask.take() else {
            return;
        };
        let question = if send {
            ask.input.read(cx).value().to_string()
        } else {
            String::new()
        };
        let mut content = ask.about.content.clone();
        if ask.include_errors {
            let errors = self.errors_text(cx);
            if !errors.is_empty() {
                content = format!("{content}\n\n{errors}");
            }
        }
        let model = ask.model.map(|(_, m)| m);
        super::ask::ask_agent(
            &self.root,
            &content,
            ask.about.name.clone(),
            &question,
            model,
            window,
            cx,
        );
        self.picks = None;
        cx.notify();
    }

    fn read_ports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ptys = super::ask::ptys_in(&self.root, window, cx);
        let core = gitviber_ui::has_core(cx).then(|| gitviber_ui::core(cx));
        let Some(core) = core else {
            return;
        };
        let shells: Vec<u32> = ptys.iter().filter_map(|&id| core.ptys.shell(id)).collect();
        let ports = self.ports.clone();
        self.ports_task = cx.spawn(async move |_, cx| {
            let found = if shells.is_empty() {
                Vec::new()
            } else {
                cx.background_spawn(async move { crate::ports::listening(&shells) })
                    .await
            };
            *ports.borrow_mut() = found;
        });
    }
}

mod render;
