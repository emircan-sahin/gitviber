//! The box at the end of an issue's or a PR's conversation that adds a comment, and the keys
//! that post from any comment box.

use gitviber_ui::actions::github::PostComment;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Enter;
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, FocusHandle, InteractiveElement as _,
    ParentElement as _, Styled as _, Window, div, px,
};

use crate::ui::common::{section, theme};
use crate::ui::markdown_input::MarkdownInput;

pub(crate) const POST_KEY: &str = if cfg!(target_os = "macos") {
    "⌘↵"
} else {
    "Ctrl+Enter"
};

/// ⌘↵ in `el` calls `post`: PostComment's binding, and the text box's own ⌘↵, taken before
/// it puts a newline in the comment.
pub(crate) fn posts_on_key<V: 'static>(
    el: Div,
    post: fn(&mut V, &mut Context<V>),
    cx: &mut Context<V>,
) -> Div {
    el.key_context("IssueComment")
        .on_action(cx.listener(move |this, _: &PostComment, _, cx| post(this, cx)))
        .capture_action(cx.listener(move |this, e: &Enter, _, cx| {
            if e.secondary {
                cx.stop_propagation();
                post(this, cx);
            }
        }))
}

pub(crate) struct CommentBox {
    pub input: Entity<MarkdownInput>,
    /// The page list's last item's: drawn out of view while focus is in it, so typing and ⌘↵
    /// go on working with the box scrolled away.
    pub focus: FocusHandle,
    /// The text a post sent once it went through: the box drops it at the next frame, which
    /// has the window, and keeps what was typed while it posted.
    posted: Option<String>,
}

impl CommentBox {
    /// For issue or PR `number` of `repo` (owner/name), whose page is `url`.
    pub fn new<V: 'static>(
        repo: &str,
        url: &str,
        number: u64,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Self {
        let input = cx.new(|cx| {
            MarkdownInput::new(
                format!("https://github.com/{repo}"),
                url.to_string(),
                window,
                cx,
                |t| {
                    t.rows(4)
                        .placeholder(format!("Leave a comment (markdown; {POST_KEY} to post)"))
                },
            )
            .small()
            .signed_by(Some(repo.to_string()), number)
        });
        CommentBox {
            input,
            focus: cx.focus_handle(),
            posted: None,
        }
    }

    pub fn text(&self, cx: &App) -> String {
        self.input.read(cx).value(cx).trim().to_string()
    }

    /// `text` went through.
    pub fn posted(&mut self, text: String) {
        self.posted = Some(text);
    }

    /// At a frame: the text posted leaves the box; what follows it stays.
    pub fn sync(&mut self, window: &mut Window, cx: &mut App) {
        let Some(posted) = self.posted.take() else {
            return;
        };
        let now = self.input.read(cx).value(cx).to_string();
        if let Some(rest) = now.trim_start().strip_prefix(posted.as_str()) {
            let rest = rest.trim_start().to_string();
            self.input
                .update(cx, |c, cx| c.set_value(&rest, window, cx));
        }
    }

    /// The page's last item; `posting`: a post of it is on its way.
    pub fn render<V: 'static>(
        &self,
        can_post: bool,
        posting: bool,
        post: fn(&mut V, &mut Context<V>),
        cx: &mut Context<V>,
    ) -> Div {
        let t = theme(cx);
        let keys = posts_on_key(div().p(px(8.)), post, cx);
        section("Add a comment", None, &t)
            .track_focus(&self.focus)
            .child(
                keys.child(self.input.clone()).child(
                    div().mt(px(8.)).flex().justify_end().child(
                        Button::new("post-comment")
                            .primary()
                            .small()
                            .label(if posting { "Posting…" } else { "Comment" })
                            .disabled(!can_post)
                            .on_click(cx.listener(move |this, _, _, cx| post(this, cx))),
                    ),
                ),
            )
    }
}
