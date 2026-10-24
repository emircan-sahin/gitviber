//! Lanes for the history graph, built a page at a time: the builder keeps the lanes still waiting
//! for a parent, so the next page goes on where the last one stopped. Pure.

use std::collections::HashSet;

/// A lane's line in one row: its column, and which branch it is (0 is the list's own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lane {
    pub col: usize,
    pub id: usize,
}

/// One commit's row. Lanes are columns; each keeps its column until it ends, so lines run straight.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphRow {
    /// The commit's own lane.
    pub col: usize,
    /// The branch the commit is on, carried on by its first parent.
    pub id: usize,
    /// Lanes that pass the commit by.
    pub through: Vec<Lane>,
    /// Lanes that end at the commit: it's the parent they were waiting for.
    pub into: Vec<Lane>,
    /// Lanes the commit's parents continue in, below it.
    pub out: Vec<Lane>,
    /// Columns the row draws in.
    pub width: usize,
}

impl GraphRow {
    /// Without the graph (search results), one line joins each row to the next.
    pub fn straight(first: bool, last: bool) -> GraphRow {
        let lane = Lane { col: 0, id: 0 };
        GraphRow {
            col: 0,
            id: 0,
            through: vec![],
            into: if first { vec![] } else { vec![lane] },
            out: if last { vec![] } else { vec![lane] },
            width: 1,
        }
    }
}

#[derive(Clone, Debug)]
struct Waiting {
    sha: String,
    id: usize,
    /// HEAD's lane, kept free until something leads into it.
    held: bool,
}

/// Rows for commits listed children first, as `git log` lists them. A parent listed above its
/// child (git's date order under clock skew) gets no line: nothing below would ever close it.
/// `head`, when listed, is held the first lane and branch 0 even below newer branches' tips.
#[derive(Clone, Debug, Default)]
pub struct GraphBuilder {
    /// The commit each lane waits for; None is a free column.
    lanes: Vec<Option<Waiting>>,
    seen: HashSet<String>,
    next: usize,
}

impl GraphBuilder {
    pub fn new(head: Option<&str>) -> Self {
        match head {
            Some(h) => GraphBuilder {
                lanes: vec![Some(Waiting {
                    sha: h.to_string(),
                    id: 0,
                    held: true,
                })],
                seen: HashSet::new(),
                next: 1,
            },
            None => GraphBuilder::default(),
        }
    }

    fn free(&self) -> usize {
        self.lanes
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.lanes.len())
    }

    fn set(&mut self, col: usize, w: Waiting) {
        if col == self.lanes.len() {
            self.lanes.push(Some(w));
        } else {
            self.lanes[col] = Some(w);
        }
    }

    /// The next commit's row.
    pub fn push(&mut self, sha: &str, parents: &[impl AsRef<str>]) -> GraphRow {
        self.seen.insert(sha.to_string());
        let waiting: Vec<(usize, usize, bool)> = self
            .lanes
            .iter()
            .enumerate()
            .filter_map(|(col, l)| {
                l.as_ref()
                    .filter(|l| l.sha == sha)
                    .map(|l| (col, l.id, l.held))
            })
            .collect();
        let col = waiting.first().map_or_else(|| self.free(), |w| w.0);
        let id = match waiting.first() {
            Some(w) => w.1,
            None => {
                self.next += 1;
                self.next - 1
            }
        };
        for w in &waiting {
            self.lanes[w.0] = None;
        }
        let into: Vec<Lane> = waiting
            .iter()
            .filter(|w| !w.2)
            .map(|w| Lane { col: w.0, id: w.1 })
            .collect();
        let through: Vec<Lane> = self
            .lanes
            .iter()
            .enumerate()
            .filter_map(|(c, l)| {
                l.as_ref()
                    .filter(|l| !l.held)
                    .map(|l| Lane { col: c, id: l.id })
            })
            .collect();
        let mut out = vec![];
        for (k, p) in parents.iter().enumerate() {
            let p = p.as_ref();
            if self.seen.contains(p) {
                continue;
            }
            // The first parent carries the commit's own lane on; another joins the lane already waiting for it.
            let at = self
                .lanes
                .iter()
                .position(|l| l.as_ref().is_some_and(|l| l.sha == p));
            let lane = if k == 0 {
                Lane { col, id }
            } else if let Some(at) = at {
                Lane {
                    col: at,
                    id: self.lanes[at].as_ref().map_or(0, |l| l.id),
                }
            } else {
                let col = self.free();
                self.next += 1;
                Lane {
                    col,
                    id: self.next - 1,
                }
            };
            self.set(
                lane.col,
                Waiting {
                    sha: p.to_string(),
                    id: lane.id,
                    held: false,
                },
            );
            out.push(lane);
        }
        while self.lanes.last().is_some_and(Option::is_none) {
            self.lanes.pop();
        }
        let width = through
            .iter()
            .chain(&into)
            .chain(&out)
            .map(|l| l.col)
            .fold(col, usize::max)
            + 1;
        GraphRow {
            col,
            id,
            through,
            into,
            out,
            width,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(commits: &[(&str, &[&str])], head: Option<&str>) -> Vec<GraphRow> {
        let mut b = GraphBuilder::new(head);
        commits
            .iter()
            .map(|(sha, parents)| b.push(sha, parents))
            .collect()
    }

    type Lanes = Vec<(usize, usize)>;

    /// col, id, through, into, out, width
    fn flat(r: &GraphRow) -> (usize, usize, Lanes, Lanes, Lanes, usize) {
        let f = |v: &[Lane]| v.iter().map(|l| (l.col, l.id)).collect::<Vec<_>>();
        (r.col, r.id, f(&r.through), f(&r.into), f(&r.out), r.width)
    }

    #[test]
    fn a_straight_history_stays_in_one_lane() {
        let r: Vec<_> = rows(&[("c", &["b"]), ("b", &["a"]), ("a", &[])], None)
            .iter()
            .map(flat)
            .collect();
        assert_eq!(
            r,
            vec![
                (0, 0, vec![], vec![], vec![(0, 0)], 1),
                (0, 0, vec![], vec![(0, 0)], vec![(0, 0)], 1),
                (0, 0, vec![], vec![(0, 0)], vec![], 1)
            ]
        );
    }

    #[test]
    fn a_merge_opens_a_lane_for_the_merged_branch_which_closes_where_it_forked() {
        let r: Vec<_> = rows(
            &[("m", &["b", "f"]), ("f", &["a"]), ("b", &["a"]), ("a", &[])],
            None,
        )
        .iter()
        .map(flat)
        .collect();
        assert_eq!(
            r,
            vec![
                (0, 0, vec![], vec![], vec![(0, 0), (1, 1)], 2),
                (1, 1, vec![(0, 0)], vec![(1, 1)], vec![(1, 1)], 2),
                (0, 0, vec![(1, 1)], vec![(0, 0)], vec![(0, 0)], 2),
                (0, 0, vec![], vec![(0, 0), (1, 1)], vec![], 2),
            ]
        );
    }

    const REUSED: &[(&str, &[&str])] = &[
        ("m2", &["m1", "g"]),
        ("g", &["m1"]),
        ("m1", &["b", "f"]),
        ("f", &["b"]),
        ("b", &[]),
    ];

    #[test]
    fn a_branch_keeps_its_own_id_in_a_reused_column() {
        let r = rows(REUSED, None);
        assert_eq!(r[1].col, r[3].col);
        assert_ne!(r[1].id, r[3].id);
    }

    #[test]
    fn a_freed_lane_is_reused_rather_than_widening_the_graph() {
        assert_eq!(rows(REUSED, None).iter().map(|r| r.width).max(), Some(2));
    }

    #[test]
    fn a_parent_listed_above_its_child_gets_no_line() {
        assert_eq!(
            flat(&rows(&[("a", &[]), ("b", &["a"])], None)[1]),
            (0, 1, vec![], vec![], vec![], 1)
        );
    }

    #[test]
    fn a_row_is_only_as_wide_as_the_lanes_it_draws() {
        let r = rows(
            &[("m", &["b", "f"]), ("f", &["b"]), ("b", &["a"]), ("a", &[])],
            None,
        );
        assert_eq!(
            r.iter().map(|r| r.width).collect::<Vec<_>>(),
            vec![2, 2, 2, 1]
        );
    }

    #[test]
    fn a_parent_past_the_loaded_page_keeps_its_lane_open_to_the_bottom() {
        assert_eq!(
            rows(&[("b", &["a"])], None)[0].out,
            vec![Lane { col: 0, id: 0 }]
        );
    }

    #[test]
    fn head_keeps_the_first_lane_below_a_newer_branchs_tip() {
        let r: Vec<_> = rows(&[("f", &["a"]), ("h", &["a"]), ("a", &[])], Some("h"))
            .iter()
            .map(flat)
            .collect();
        assert_eq!(r[0], (1, 1, vec![], vec![], vec![(1, 1)], 2));
        assert_eq!(r[1], (0, 0, vec![(1, 1)], vec![], vec![(0, 0)], 2));
        assert_eq!(r[2].3, vec![(0, 0), (1, 1)]);
    }

    #[test]
    fn a_branch_built_on_head_leads_into_heads_lane() {
        let r: Vec<_> = rows(&[("f", &["h"]), ("h", &["a"]), ("a", &[])], Some("h"))
            .iter()
            .map(flat)
            .collect();
        assert_eq!(r[1], (0, 0, vec![], vec![(1, 1)], vec![(0, 0)], 2));
    }

    #[test]
    fn pages_built_one_after_another_match_one_build() {
        let all = rows(REUSED, None);
        let mut b = GraphBuilder::new(None);
        let mut paged: Vec<GraphRow> = REUSED[..2].iter().map(|(s, p)| b.push(s, p)).collect();
        paged.extend(REUSED[2..].iter().map(|(s, p)| b.push(s, p)));
        assert_eq!(all, paged);
    }
}
