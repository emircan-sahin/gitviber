//! Counts, git status letters and chips, line counts, and the paths rows show.

use gpui_kit::{
    App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, prelude::*, px,
    rems,
};

use crate::theme::{ActiveAppTheme, AppTheme};

/// A count beside a tab's or a pane's title, as the Changes tab shows it; `active`: its tab is
/// selected or its pane open.
#[derive(IntoElement)]
pub struct CountBadge {
    count: SharedString,
    active: bool,
}

impl CountBadge {
    pub fn new(count: impl Into<SharedString>) -> Self {
        Self {
            count: count.into(),
            active: false,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

impl RenderOnce for CountBadge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.app_theme();
        let (bg, fg) = if self.active {
            (t.modified_fill, t.on_status)
        } else {
            (t.elevated, t.muted_foreground)
        };
        div()
            .flex_none()
            .px_1()
            .rounded(px(3.))
            .bg(bg)
            .text_color(fg)
            .font_family(t.fonts.code.clone())
            .text_size(rems(0.625))
            .line_height(rems(1.))
            .child(self.count)
    }
}

/// A file's change as git reports it, for the letter and chip that show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Modified,
    Added,
    Untracked,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Conflict,
}

impl ChangeKind {
    /// From git's status letter (M A ? D R C T U); anything else reads as modified.
    pub fn from_letter(letter: char) -> Self {
        match letter {
            'A' => Self::Added,
            '?' => Self::Untracked,
            'D' => Self::Deleted,
            'R' => Self::Renamed,
            'C' => Self::Copied,
            'T' => Self::TypeChanged,
            'U' => Self::Conflict,
            _ => Self::Modified,
        }
    }

    pub fn letter(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Untracked => "U",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Copied => "C",
            Self::TypeChanged => "T",
            Self::Conflict => "!",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Modified => "Modified",
            Self::Added => "Added",
            Self::Untracked => "Untracked",
            Self::Deleted => "Deleted",
            Self::Renamed => "Renamed",
            Self::Copied => "Copied",
            Self::TypeChanged => "Type changed",
            Self::Conflict => "Conflict",
        }
    }

    /// The text color, readable on every surface.
    pub fn color(self, t: &AppTheme) -> gpui_kit::Hsla {
        match self {
            Self::Added | Self::Untracked => t.added,
            Self::Deleted => t.removed,
            Self::Renamed | Self::Copied => t.renamed,
            Self::Conflict => t.conflict,
            Self::Modified | Self::TypeChanged => t.modified,
        }
    }

    /// A chip's bright fill, under `on_status` ink.
    pub fn fill(self, t: &AppTheme) -> gpui_kit::Hsla {
        match self {
            Self::Added | Self::Untracked => t.added_fill,
            Self::Deleted => t.removed_fill,
            Self::Renamed | Self::Copied => t.renamed_fill,
            Self::Conflict => t.conflict_fill,
            Self::Modified | Self::TypeChanged => t.modified_fill,
        }
    }
}

/// The status letter at a row's end.
#[derive(IntoElement)]
pub struct StatusLetter(pub ChangeKind);

impl RenderOnce for StatusLetter {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.app_theme();
        div()
            .flex_none()
            .w(px(12.))
            .text_center()
            .font_family(t.fonts.code.clone())
            .text_size(rems(0.6875))
            .font_weight(gpui_kit::FontWeight::BOLD)
            .text_color(self.0.color(t))
            .child(self.0.letter())
    }
}

/// A solid chip naming the change ("Added"), as a file header shows it.
#[derive(IntoElement)]
pub struct StatusPill(pub ChangeKind);

impl RenderOnce for StatusPill {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.app_theme();
        div()
            .flex_none()
            .px(px(6.))
            .py(px(1.))
            .rounded(px(3.))
            .bg(self.0.fill(t))
            .text_color(t.on_status)
            .text_size(rems(0.656))
            .font_weight(t.semibold())
            .child(self.0.label())
    }
}

/// "+16 −3" in the diff colors; nothing when neither is known.
#[derive(IntoElement)]
pub struct LineCounts {
    additions: Option<u32>,
    deletions: Option<u32>,
}

impl LineCounts {
    pub fn new(additions: Option<u32>, deletions: Option<u32>) -> Self {
        Self {
            additions,
            deletions,
        }
    }
}

impl RenderOnce for LineCounts {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.app_theme();
        let add = self.additions.filter(|n| *n > 0);
        let del = self.deletions.filter(|n| *n > 0);
        div()
            .flex()
            .flex_none()
            .gap_1()
            .font_family(t.fonts.code.clone())
            .text_size(rems(0.6875))
            .when_some(add, |d, n| {
                d.child(div().text_color(t.added).child(format!("+{n}")))
            })
            .when_some(del, |d, n| {
                d.child(div().text_color(t.removed).child(format!("−{n}")))
            })
    }
}

/// "dir/**name**": the directory gives way first; the file name keeps its width when it can.
#[derive(IntoElement)]
pub struct PathLabel(pub SharedString);

impl RenderOnce for PathLabel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.app_theme();
        let path = self.0;
        let (dir, name) = match path.rfind('/') {
            Some(i) => (
                Some(SharedString::from(path[..=i].to_string())),
                SharedString::from(path[i + 1..].to_string()),
            ),
            None => (None, path.clone()),
        };
        div()
            .flex()
            .min_w_0()
            .overflow_hidden()
            .when_some(dir, |d, dir| {
                d.child(div().min_w_0().truncate().text_color(t.subtle).child(dir))
            })
            .child(
                div()
                    .flex_none()
                    .max_w_full()
                    .truncate()
                    .text_color(t.foreground)
                    .child(name),
            )
    }
}

/// A terminal's or an agent's state, as a dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// Waits for the user: the loudest.
    Needs,
    Unread,
    Working,
    Done,
}

#[derive(IntoElement)]
pub struct StatusDot(pub Look);

impl RenderOnce for StatusDot {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.app_theme();
        let color = match self.0 {
            Look::Needs => t.destructive,
            Look::Unread => t.info,
            Look::Working => t.modified,
            Look::Done => t.muted_foreground,
        };
        let dot = div().flex_none().size(px(6.)).rounded_full();
        match self.0 {
            Look::Done => dot.border_1().border_color(color),
            Look::Needs => dot.bg(color).shadow(vec![gpui_kit::BoxShadow {
                color: color.opacity(0.3),
                offset: gpui_kit::point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(2.),
                inset: false,
            }]),
            _ => dot.bg(color),
        }
    }
}
