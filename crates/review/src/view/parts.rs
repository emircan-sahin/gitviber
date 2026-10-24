//! The guide's small pieces: a category's and an importance's tag, a notice, a count.

use gitviber_ui::AppTheme;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::{
    AnyElement, Hsla, IntoElement, ParentElement as _, SharedString, Styled as _, div,
    prelude::FluentBuilder as _, px, rems,
};

use crate::model::guide::{Category, Importance};
use crate::model::risks::Severity;

pub fn category_icon(c: Category) -> IconName {
    match c {
        Category::Ui => IconName::Palette,
        Category::Api => IconName::Plug,
        Category::Core => IconName::Cpu,
        Category::Data => IconName::Database,
        Category::Cli => IconName::SquareTerminal,
        Category::Security => IconName::ShieldAlert,
        Category::Tests => IconName::FlaskConical,
        Category::Docs => IconName::BookOpen,
        Category::Examples => IconName::Lightbulb,
        Category::Deps => IconName::Package,
        Category::Build => IconName::Hammer,
        Category::Scripts => IconName::ScrollText,
        Category::Config => IconName::Settings2,
        Category::I18n => IconName::Languages,
        Category::Assets => IconName::Image,
        Category::Other => IconName::Shapes,
    }
}

/// Always an icon and a name; the color only on top.
pub fn category_tone(c: Category, t: &AppTheme) -> Hsla {
    match c {
        Category::Ui | Category::Config | Category::I18n => t.info,
        Category::Api | Category::Cli | Category::Assets => t.renamed,
        Category::Core => t.primary,
        Category::Data | Category::Deps => t.modified,
        Category::Security => t.destructive,
        Category::Tests => t.added,
        Category::Docs | Category::Examples => t.muted_foreground,
        Category::Build | Category::Scripts => t.conflict,
        Category::Other => t.subtle,
    }
}

pub fn icon(name: IconName, color: Hsla) -> Icon {
    Icon::new(name).size(px(14.)).text_color(color)
}

pub fn category_tag(c: Category, t: &AppTheme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .flex_none()
        .text_size(rems(0.72))
        .text_color(t.muted_foreground)
        .child(icon(category_icon(c), category_tone(c, t)))
        .child(c.label())
}

pub fn importance_icon(i: Importance) -> IconName {
    match i {
        Importance::High => IconName::Signal,
        Importance::Medium => IconName::SignalMedium,
        Importance::Low => IconName::SignalLow,
    }
}

pub fn importance_tone(i: Importance, t: &AppTheme) -> Hsla {
    match i {
        Importance::High => t.destructive,
        Importance::Medium => t.muted_foreground,
        Importance::Low => t.subtle,
    }
}

/// Always its word, the color only on top.
pub fn importance_tag(i: Importance, t: &AppTheme) -> impl IntoElement {
    let tone = importance_tone(i, t);
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .flex_none()
        .text_size(rems(0.72))
        .text_color(tone)
        .when(i == Importance::High, |d| {
            d.font_weight(gpui_kit::FontWeight::MEDIUM)
        })
        .child(icon(importance_icon(i), tone))
        .child(i.label())
}

pub fn severity_color(s: Severity, t: &AppTheme) -> Hsla {
    match s {
        Severity::High => t.destructive,
        Severity::Medium => t.modified,
        Severity::Low => t.subtle,
    }
}

/// A line of news above the guide: running, stopped, failed, outdated.
pub fn notice(
    icon_name: IconName,
    color: Hsla,
    t: &AppTheme,
    body: impl IntoElement,
) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap(px(8.))
        .px(px(12.))
        .py(px(8.))
        .rounded(px(6.))
        .border_1()
        .border_color(t.border)
        .bg(t.panel)
        .text_size(rems(0.78))
        .text_color(color)
        .child(div().pt(px(2.)).child(icon(icon_name, color)))
        .child(div().min_w_0().flex_1().child(body))
        .into_any_element()
}

/// "3 / 5", the count green once it's all of them.
pub fn count(n: usize, of: usize, t: &AppTheme) -> impl IntoElement {
    div()
        .flex()
        .gap(px(3.))
        .child(
            div()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(if n == of { t.added } else { t.foreground })
                .child(SharedString::from(n.to_string())),
        )
        .child(SharedString::from(format!("/ {of}")))
}
