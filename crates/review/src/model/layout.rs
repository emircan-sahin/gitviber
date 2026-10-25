//! A flow's steps placed for drawing, top to bottom: each step a rank below the steps that lead
//! to it, the steps of a rank side by side in the order that crosses fewest edges, and each edge
//! a curve between the boxes (one back up the flow goes round the right side).

use super::guide::Flow;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    pub box_w: f32,
    pub box_h: f32,
    pub gap_x: f32,
    pub gap_y: f32,
}

/// A step's box, by its top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub step: usize,
    pub rank: usize,
    pub x: f32,
    pub y: f32,
}

/// A cubic curve from `start` to `end`, and where its label goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub edge: usize,
    pub start: (f32, f32),
    pub c1: (f32, f32),
    pub c2: (f32, f32),
    pub end: (f32, f32),
    pub label_at: (f32, f32),
    /// Back up the flow, or across a rank: drawn round the right side.
    pub around: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Laid {
    pub boxes: Vec<Placed>,
    pub curves: Vec<Curve>,
    pub width: f32,
    pub height: f32,
}

/// Each step's rank: the longest path to it from a step nothing leads to, with the edges that
/// close a loop (back to a step still on the walk) left out.
fn ranks(n: usize, edges: &[(usize, usize)]) -> (Vec<usize>, Vec<bool>) {
    let mut out_of: Vec<Vec<(usize, usize)>> = vec![vec![]; n];
    for (i, &(a, b)) in edges.iter().enumerate() {
        out_of[a].push((i, b));
    }
    // 0: not seen, 1: on the walk, 2: done.
    let mut state = vec![0u8; n];
    let mut back = vec![false; edges.len()];
    let mut order = Vec::with_capacity(n);
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if let Some(&(e, to)) = out_of[v].get(*next) {
                *next += 1;
                match state[to] {
                    0 => {
                        state[to] = 1;
                        stack.push((to, 0));
                    }
                    1 => back[e] = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                order.push(v);
                stack.pop();
            }
        }
    }
    // Finished last first is a topological order of what's left.
    order.reverse();
    let mut rank = vec![0usize; n];
    for &v in &order {
        for &(e, to) in &out_of[v] {
            if !back[e] && to != v {
                rank[to] = rank[to].max(rank[v] + 1);
            }
        }
    }
    (rank, back)
}

/// Lays `flow` out; edges between steps that aren't in it are skipped.
pub fn layout(flow: &Flow, s: Sizes) -> Laid {
    let n = flow.steps.len();
    if n == 0 {
        return Laid::default();
    }
    let index = |id: &str| flow.steps.iter().position(|st| st.id == id);
    let edges: Vec<(usize, usize, usize)> = flow
        .edges
        .iter()
        .enumerate()
        .filter_map(|(i, e)| Some((i, index(&e.from)?, index(&e.to)?)))
        .collect();
    let pairs: Vec<(usize, usize)> = edges.iter().map(|&(_, a, b)| (a, b)).collect();
    let (rank, _) = ranks(n, &pairs);
    let depth = rank.iter().copied().max().unwrap_or(0) + 1;
    let mut rows: Vec<Vec<usize>> = vec![vec![]; depth];
    for (v, &r) in rank.iter().enumerate() {
        rows[r].push(v);
    }
    // Each row in the order of the steps above that lead to it (their mean place), so few
    // edges cross; a step with none keeps its place.
    let mut place = vec![0f32; n];
    for row in &rows {
        for (i, &v) in row.iter().enumerate() {
            place[v] = i as f32;
        }
    }
    for (r, row) in rows.iter_mut().enumerate().skip(1) {
        let mut keyed: Vec<(f32, usize)> = row
            .iter()
            .map(|&v| {
                let from: Vec<f32> = pairs
                    .iter()
                    .filter(|&&(a, b)| b == v && rank[a] < r)
                    .map(|&(a, _)| place[a])
                    .collect();
                let key = if from.is_empty() {
                    place[v]
                } else {
                    from.iter().sum::<f32>() / from.len() as f32
                };
                (key, v)
            })
            .collect();
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        *row = keyed.into_iter().map(|(_, v)| v).collect();
        for (i, &v) in row.iter().enumerate() {
            place[v] = i as f32;
        }
    }
    let widest = rows.iter().map(Vec::len).max().unwrap_or(1) as f32;
    let row_w = |k: usize| k as f32 * s.box_w + (k.saturating_sub(1)) as f32 * s.gap_x;
    // Room on the right for the curves that go round.
    let around = edges.iter().any(|&(_, a, b)| rank[b] <= rank[a]);
    let pad = if around { s.gap_x * 1.5 } else { 0. };
    let width = row_w(widest as usize) + pad;
    let mut at = vec![(0f32, 0f32); n];
    let mut boxes = Vec::with_capacity(n);
    for (r, row) in rows.iter().enumerate() {
        let left = (width - pad - row_w(row.len())) / 2.;
        for (i, &v) in row.iter().enumerate() {
            let x = left + i as f32 * (s.box_w + s.gap_x);
            let y = r as f32 * (s.box_h + s.gap_y);
            at[v] = (x, y);
            boxes.push(Placed {
                step: v,
                rank: r,
                x,
                y,
            });
        }
    }
    boxes.sort_by_key(|b| b.step);
    let curves = edges
        .iter()
        .map(|&(e, a, b)| {
            let ((ax, ay), (bx, by)) = (at[a], at[b]);
            if rank[b] > rank[a] {
                let start = (ax + s.box_w / 2., ay + s.box_h);
                let end = (bx + s.box_w / 2., by);
                let dy = (end.1 - start.1) / 2.;
                let (c1, c2) = ((start.0, start.1 + dy), (end.0, end.1 - dy));
                Curve {
                    edge: e,
                    start,
                    c1,
                    c2,
                    end,
                    label_at: midpoint(start, c1, c2, end),
                    around: false,
                }
            } else {
                let start = (ax + s.box_w, ay + s.box_h / 2.);
                let end = (bx + s.box_w, by + s.box_h / 2.);
                let out = s.gap_x * 1.2;
                let (c1, c2) = ((start.0 + out, start.1), (end.0 + out, end.1));
                Curve {
                    edge: e,
                    start,
                    c1,
                    c2,
                    end,
                    label_at: midpoint(start, c1, c2, end),
                    around: true,
                }
            }
        })
        .collect();
    Laid {
        boxes,
        curves,
        width,
        height: depth as f32 * s.box_h + (depth - 1) as f32 * s.gap_y,
    }
}

/// A cubic curve's point halfway.
fn midpoint(a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32)) -> (f32, f32) {
    (
        (a.0 + 3. * b.0 + 3. * c.0 + d.0) / 8.,
        (a.1 + 3. * b.1 + 3. * c.1 + d.1) / 8.,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::guide::{FlowEdge, FlowStep, PartStatus};

    const S: Sizes = Sizes {
        box_w: 100.,
        box_h: 40.,
        gap_x: 20.,
        gap_y: 30.,
    };

    fn flow(steps: &[&str], edges: &[(&str, &str)]) -> Flow {
        Flow {
            title: String::new(),
            steps: steps
                .iter()
                .map(|id| FlowStep {
                    id: id.to_string(),
                    label: id.to_string(),
                    file: String::new(),
                    status: PartStatus::Same,
                    decision: false,
                    section: None,
                })
                .collect(),
            edges: edges
                .iter()
                .map(|(a, b)| FlowEdge {
                    from: a.to_string(),
                    to: b.to_string(),
                    label: String::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_chain_is_one_column() {
        let l = layout(&flow(&["a", "b", "c"], &[("a", "b"), ("b", "c")]), S);
        assert_eq!(
            l.boxes.iter().map(|b| b.rank).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert!(l.boxes.iter().all(|b| b.x == 0.));
        assert_eq!((l.width, l.height), (100., 3. * 40. + 2. * 30.));
        assert!(l.curves.iter().all(|c| !c.around && c.end.1 > c.start.1));
    }

    #[test]
    fn a_decision_branches_side_by_side_and_joins_below() {
        let l = layout(
            &flow(
                &["ask", "yes", "no", "end"],
                &[("ask", "yes"), ("ask", "no"), ("yes", "end"), ("no", "end")],
            ),
            S,
        );
        let rank: Vec<usize> = l.boxes.iter().map(|b| b.rank).collect();
        assert_eq!(rank, [0, 1, 1, 2]);
        assert_eq!(l.width, 220.);
        // The single steps are centered over the two.
        assert_eq!(l.boxes[0].x, 60.);
        assert!(l.boxes[1].x < l.boxes[2].x);
    }

    #[test]
    fn a_loop_back_goes_round_and_ranks_still_end() {
        let l = layout(
            &flow(&["a", "b", "c"], &[("a", "b"), ("b", "c"), ("c", "a")]),
            S,
        );
        assert_eq!(
            l.boxes.iter().map(|b| b.rank).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(l.curves.iter().filter(|c| c.around).count(), 1);
        assert!(l.width > 100.);
        // A step's edge to itself, and steps nothing joins, lay out too.
        let lone = layout(&flow(&["a", "b"], &[("a", "a")]), S);
        assert_eq!(lone.boxes.len(), 2);
        assert_eq!(lone.curves.len(), 1);
    }
}
