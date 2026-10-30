//! The top bar chip: the open project's tile and name, which opens the picker.

use super::*;

/// Each window's project chip as last drawn, for Switch Project to open; a closed window's or an
/// old project's goes with it (weak).
#[derive(Default)]
struct ProjectChips(Vec<(AnyWindowHandle, WeakEntity<ProjectChip>)>);
impl Global for ProjectChips {}

pub fn project_chip(ctx: &PanelContext, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let project = ctx.project.clone()?;
    let id = project.entity_id().as_u64() as usize;
    let chip = window.use_keyed_state(("refs-project-chip", id), cx, |_, cx| {
        ProjectChip::new(&project, cx)
    });
    let handle = window.window_handle();
    let chips = cx.default_global::<ProjectChips>();
    chips
        .0
        .retain(|(w, c)| *w != handle && c.upgrade().is_some());
    chips.0.push((handle, chip.downgrade()));
    Some(chip.into_any_element())
}

/// File → Switch Project: the window's project chip opens its list.
pub fn switch_project(cx: &mut App) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    if let Some(chip) = ProjectChip::of_window(handle, cx) {
        chip.update(cx, |c, cx| {
            c.asked = true;
            cx.notify();
        });
    }
}

pub struct ProjectChip {
    project: Entity<Project>,
    picker: Option<Entity<ProjectPicker>>,
    /// Switch Project asked for the list: the next frame opens it, having a window to build it in.
    asked: bool,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl ProjectChip {
    fn new(project: &Entity<Project>, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(project, |_, _, cx| cx.notify()),
            // The tile's color.
            cx.observe_global::<UiState>(|_, cx| cx.notify()),
        ];
        ProjectChip {
            project: project.clone(),
            picker: None,
            asked: false,
            _subs: subs,
            _live: Live::new("ProjectChip"),
        }
    }

    /// The chip `window` drew last.
    pub fn of_window(window: AnyWindowHandle, cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<ProjectChips>()?
            .0
            .iter()
            .find(|(w, _)| *w == window)?
            .1
            .upgrade()
    }

    #[doc(hidden)]
    pub fn is_open(&self) -> bool {
        self.picker.is_some()
    }

    /// The row the keys are on, counting folders and projects but not headers, while the
    /// filter has the keys.
    #[doc(hidden)]
    pub fn key_row(&self, window: &Window, cx: &App) -> Option<usize> {
        let p = self.picker.as_ref()?.read(cx);
        if !p.query.focus_handle(cx).is_focused(window) {
            return None;
        }
        let lines = p.lines(cx);
        let at = p.hot_line(&lines)?;
        Some(lines[..at].iter().filter(|l| l.is_row()).count())
    }

    /// The project is its main worktree, even inside a linked one.
    fn main(&self, cx: &App) -> String {
        let p = self.project.read(cx);
        p.main_worktree()
            .map(|m| m.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.root().to_string_lossy().into_owned())
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open == self.picker.is_some() {
            return;
        }
        self.picker = open.then(|| {
            let (main, chip) = (self.main(cx), cx.weak_entity());
            cx.new(|cx| ProjectPicker::new(main, chip, window, cx))
        });
        cx.notify();
    }

    pub(super) fn close(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        cx.notify();
    }
}

impl Render for ProjectChip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.asked) {
            self.set_open(true, window, cx);
        }
        let t = cx.app_theme().clone();
        let main = self.main(cx);
        let name = project_name(&main).to_string();
        let trigger = Button::new("project-chip").ghost().small().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .max_w(px(256.))
                .child(project_tile(&main, 18., cx))
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(rems(0.78125))
                        .child(name),
                )
                .child(
                    Icon::new(IconName::ChevronsUpDown)
                        .xsmall()
                        .text_color(t.subtle),
                ),
        );
        let picker = self.picker.clone();
        let focus = picker.as_ref().map(|p| p.read(cx).query.focus_handle(cx));
        let mut popover = Popover::new("project-picker")
            .anchor(Anchor::TopLeft)
            .open(picker.is_some())
            .on_open_change(cx.listener(|this, open: &bool, window, cx| {
                this.set_open(*open, window, cx);
                // A click opened the popover, which took focus before the picker existed: the
                // filter takes it now, so the keys work at once.
                if let Some(p) = &this.picker {
                    p.read(cx).query.focus_handle(cx).focus(window, cx);
                }
            }))
            .trigger(trigger)
            .content(move |_, _, _| {
                picker
                    .clone()
                    .map(|p| p.into_any_element())
                    .unwrap_or_else(|| div().into_any_element())
            });
        if let Some(f) = &focus {
            popover = popover.track_focus(f);
        }
        popover
    }
}
