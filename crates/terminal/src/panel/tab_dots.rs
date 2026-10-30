//! A split tab's dots, one per pane in its agent's look, and what they say for the tooltip. Past
//! `MAX_DOTS` the row turns into a count per look, so a big split doesn't push the title out.

use super::*;
use crate::agent::Look;

/// Six dots still read one by one at a glance; more turn into counts.
const MAX_DOTS: usize = 6;

/// Panes by look, the most urgent first and the idle ones (no agent, no news) last; only looks
/// that some pane has.
fn tally(looks: &[Option<Look>]) -> Vec<(Option<Look>, usize)> {
    Look::ALL
        .iter()
        .map(|l| Some(*l))
        .chain([None])
        .map(|l| (l, looks.iter().filter(|x| **x == l).count()))
        .filter(|(_, n)| *n > 0)
        .collect()
}

/// The dots in words: "2 agents working, 1 waiting for you, 3 idle".
fn summary(looks: &[Option<Look>]) -> String {
    tally(looks)
        .into_iter()
        .map(|(look, n)| match look {
            Some(Look::Needs) => format!("{n} waiting for you"),
            Some(Look::Unread) => format!("{n} not viewed"),
            Some(Look::Working) if n == 1 => "1 agent working".into(),
            Some(Look::Working) => format!("{n} agents working"),
            Some(Look::Done) => format!("{n} finished"),
            None => format!("{n} idle"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// An idle pane's dot, bigger for the tab's focused pane.
fn idle_dot(big: bool, t: &AppTheme) -> AnyElement {
    div()
        .flex_none()
        .rounded_full()
        .bg(if big { t.muted_foreground } else { t.subtle })
        .size(px(if big { 6. } else { 4. }))
        .into_any_element()
}

/// `panes`: each pane's look in the split's order, and whether it's the tab's focused one.
pub(super) fn split_dots(gid: u64, panes: &[(Option<Look>, bool)], cx: &App) -> AnyElement {
    let t = cx.app_theme();
    let looks: Vec<Option<Look>> = panes.iter().map(|(l, _)| *l).collect();
    let tip = summary(&looks);
    let row = h_flex().gap_1().items_center();
    let row = if panes.len() <= MAX_DOTS {
        row.children(panes.iter().map(|(look, big)| match look {
            Some(l) => attention::look_dot(*l, cx),
            None => idle_dot(*big, t),
        }))
    } else {
        row.gap_1p5()
            .children(tally(&looks).into_iter().map(|(look, n)| {
                h_flex()
                    .gap(px(3.))
                    .items_center()
                    .child(match look {
                        Some(l) => attention::look_dot(l, cx),
                        None => idle_dot(false, t),
                    })
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(t.muted_foreground)
                            .child(n.to_string()),
                    )
            }))
    };
    row.id(("terminal-tab-dots", gid))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{Look, summary, tally};

    #[test]
    fn tally_puts_the_most_urgent_first_and_idle_last() {
        let looks = [
            None,
            Some(Look::Working),
            Some(Look::Needs),
            None,
            Some(Look::Working),
        ];
        assert_eq!(
            tally(&looks),
            vec![(Some(Look::Needs), 1), (Some(Look::Working), 2), (None, 2)]
        );
        assert!(tally(&[]).is_empty());
    }

    #[test]
    fn summary_says_each_look_with_its_count() {
        let looks = [Some(Look::Working), Some(Look::Needs), Some(Look::Working)];
        assert_eq!(summary(&looks), "1 waiting for you, 2 agents working");
        let looks = [
            Some(Look::Working),
            Some(Look::Done),
            Some(Look::Unread),
            None,
        ];
        assert_eq!(
            summary(&looks),
            "1 not viewed, 1 agent working, 1 finished, 1 idle"
        );
    }
}
