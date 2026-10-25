//! A terminal tab's split panes as a tree: sizes are percents, one per child.

pub type PaneId = u64;

#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    Pane(PaneId),
    Split(Split),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    pub dir: Dir,
    pub children: Vec<Layout>,
    pub sizes: Vec<f64>,
}

/// `Row` puts its children side by side, `Col` stacks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Row,
    Col,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const UNIT: Rect = Rect {
        x: 0.,
        y: 0.,
        width: 1.,
        height: 1.,
    };
}

/// The panes in tree order: the order a tab keeps them in.
pub fn leaves(l: &Layout) -> Vec<PaneId> {
    let mut out = Vec::new();
    fn walk(l: &Layout, out: &mut Vec<PaneId>) {
        match l {
            Layout::Pane(id) => out.push(*id),
            Layout::Split(s) => s.children.iter().for_each(|c| walk(c, out)),
        }
    }
    walk(l, &mut out);
    out
}

/// Each pane's box inside `bounds`, from its splits' directions and sizes, in tree order.
pub fn pane_rects(l: &Layout, bounds: Rect) -> Vec<(PaneId, Rect)> {
    let mut out = Vec::new();
    fn walk(l: &Layout, b: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match l {
            Layout::Pane(id) => out.push((*id, b)),
            Layout::Split(s) => {
                let total: f64 = s.sizes.iter().sum();
                let total = if total > 0. { total } else { 1. };
                let mut at = 0.;
                for (c, size) in s.children.iter().zip(&s.sizes) {
                    let share = size / total;
                    let part = match s.dir {
                        Dir::Row => Rect {
                            x: b.x + at * b.width,
                            width: share * b.width,
                            ..b
                        },
                        Dir::Col => Rect {
                            y: b.y + at * b.height,
                            height: share * b.height,
                            ..b
                        },
                    };
                    walk(c, part, out);
                    at += share;
                }
            }
        }
    }
    walk(l, bounds, &mut out);
    out
}

/// Tops this close (of the tab's height) are one row: a divider dragged a hair off its neighbour's.
const ROW: f64 = 0.01;

/// The panes as numbered, by where they're drawn rather than how the splits were made: row by row
/// from the top, each left to right. A 2×2 grid split right first or down first numbers the same.
pub fn pane_order(l: &Layout) -> Vec<PaneId> {
    let mut panes = pane_rects(l, Rect::UNIT);
    panes.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
    let mut rows: Vec<Vec<(PaneId, Rect)>> = Vec::new();
    for pane in panes {
        match rows.last_mut() {
            Some(last) if pane.1.y - last[0].1.y <= ROW => last.push(pane),
            _ => rows.push(vec![pane]),
        }
    }
    rows.into_iter()
        .flat_map(|mut r| {
            r.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
            r.into_iter().map(|(id, _)| id)
        })
        .collect()
}

/// The pane one step (`dir` ±1) from `from` in `order`, past either end to the other, as pages turn.
pub fn step_in(order: &[PaneId], from: PaneId, dir: i32) -> Option<PaneId> {
    let at = order.iter().position(|&id| id == from)?;
    if order.len() < 2 {
        return None;
    }
    let n = order.len() as i64;
    Some(order[((at as i64 + dir as i64).rem_euclid(n)) as usize])
}

/// Panes side by side at equal widths.
pub fn row(ids: &[PaneId]) -> Layout {
    if ids.len() == 1 {
        return Layout::Pane(ids[0]);
    }
    Layout::Split(Split {
        dir: Dir::Row,
        children: ids.iter().map(|&id| Layout::Pane(id)).collect(),
        sizes: vec![100. / ids.len() as f64; ids.len()],
    })
}

/// `pane` right after `at` along `dir`, halving its space. A split already going that way takes
/// it as one more child.
pub fn split_pane(l: &Layout, at: PaneId, pane: PaneId, dir: Dir) -> Layout {
    match l {
        Layout::Pane(id) if *id == at => Layout::Split(Split {
            dir,
            children: vec![Layout::Pane(at), Layout::Pane(pane)],
            sizes: vec![50., 50.],
        }),
        Layout::Pane(_) => l.clone(),
        Layout::Split(s) => {
            let i = s.children.iter().position(|c| *c == Layout::Pane(at));
            if let Some(i) = i.filter(|_| s.dir == dir) {
                let half = s.sizes[i] / 2.;
                let mut children = s.children.clone();
                children.insert(i + 1, Layout::Pane(pane));
                let mut sizes = s.sizes.clone();
                sizes[i] = half;
                sizes.insert(i + 1, half);
                return Layout::Split(Split {
                    dir,
                    children,
                    sizes,
                });
            }
            Layout::Split(Split {
                children: s
                    .children
                    .iter()
                    .map(|c| split_pane(c, at, pane, dir))
                    .collect(),
                ..s.clone()
            })
        }
    }
}

/// Without `pane`; a split left with one child gives way to it. None once nothing is left.
pub fn remove_pane(l: &Layout, pane: PaneId) -> Option<Layout> {
    let s = match l {
        Layout::Pane(id) => return (*id != pane).then(|| l.clone()),
        Layout::Split(s) => s,
    };
    let kept: Vec<(Layout, f64)> = s
        .children
        .iter()
        .zip(&s.sizes)
        .filter_map(|(c, &size)| remove_pane(c, pane).map(|c| (c, size)))
        .collect();
    if kept.len() <= 1 {
        return kept.into_iter().next().map(|(c, _)| c);
    }
    // The closed pane's space goes to the rest, in proportion.
    let total: f64 = kept.iter().map(|(_, s)| s).sum();
    let mut children = Vec::new();
    let mut sizes = Vec::new();
    for (c, size) in kept {
        let share = size / total * 100.;
        match c {
            // A child split going the same way (one left after its own close) joins this one.
            Layout::Split(inner) if inner.dir == s.dir => {
                sizes.extend(inner.sizes.iter().map(|x| x / 100. * share));
                children.extend(inner.children);
            }
            c => {
                children.push(c);
                sizes.push(share);
            }
        }
    }
    Some(Layout::Split(Split {
        dir: s.dir,
        children,
        sizes,
    }))
}

/// The split at `path` (child indexes from the top) with new sizes, as its divider was dragged.
pub fn resize(l: &Layout, path: &[usize], sizes: &[f64]) -> Layout {
    let Layout::Split(s) = l else {
        return l.clone();
    };
    match path.split_first() {
        None if sizes.len() == s.children.len() => Layout::Split(Split {
            sizes: sizes.to_vec(),
            ..s.clone()
        }),
        None => l.clone(),
        Some((&at, rest)) => Layout::Split(Split {
            children: s
                .children
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    if i == at {
                        resize(c, rest, sizes)
                    } else {
                        c.clone()
                    }
                })
                .collect(),
            ..s.clone()
        }),
    }
}

/// The split at `path` with its children at equal sizes: a double-click on its divider. `deep`:
/// every split under it too, Equalize Terminal Panes on a whole tab.
pub fn equalize(l: &Layout, path: &[usize], deep: bool) -> Layout {
    let Layout::Split(s) = l else {
        return l.clone();
    };
    if let Some((&at, rest)) = path.split_first() {
        return Layout::Split(Split {
            children: s
                .children
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    if i == at {
                        equalize(c, rest, deep)
                    } else {
                        c.clone()
                    }
                })
                .collect(),
            ..s.clone()
        });
    }
    let n = s.children.len();
    Layout::Split(Split {
        dir: s.dir,
        children: if deep {
            s.children.iter().map(|c| equalize(c, &[], true)).collect()
        } else {
            s.children.clone()
        },
        sizes: vec![100. / n as f64; n],
    })
}

/// The same tree with each pane's id mapped.
pub fn map_panes(l: &Layout, f: &impl Fn(PaneId) -> PaneId) -> Layout {
    match l {
        Layout::Pane(id) => Layout::Pane(f(*id)),
        Layout::Split(s) => Layout::Split(Split {
            children: s.children.iter().map(|c| map_panes(c, f)).collect(),
            ..s.clone()
        }),
    }
}

/// Half a pixel: fractional layout's rounding, and panes meeting only at a divider's corner.
const EDGE: f64 = 0.5;

/// The pane next to `from` on that side, by where they're drawn rather than the tree: the nearest
/// one sharing an edge with it, then the one sharing most of it, then the first in reading order.
pub fn neighbor(rects: &[(PaneId, Rect)], from: PaneId, dir: Direction) -> Option<PaneId> {
    let r = rects.iter().find(|(id, _)| *id == from)?.1;
    let across = matches!(dir, Direction::Left | Direction::Right);
    let sign = if matches!(dir, Direction::Left | Direction::Up) {
        -1.
    } else {
        1.
    };
    let mid = |b: &Rect| {
        if across {
            b.x + b.width / 2.
        } else {
            b.y + b.height / 2.
        }
    };
    let mut best: Option<(PaneId, f64, f64)> = None;
    for &(id, c) in rects {
        let gap = match dir {
            Direction::Left => r.x - (c.x + c.width),
            Direction::Right => c.x - (r.x + r.width),
            Direction::Up => r.y - (c.y + c.height),
            Direction::Down => c.y - (r.y + r.height),
        };
        let overlap = if across {
            (r.y + r.height).min(c.y + c.height) - r.y.max(c.y)
        } else {
            (r.x + r.width).min(c.x + c.width) - r.x.max(c.x)
        };
        // Its middle past this one's: however narrow the panes, never one on the other side.
        if id == from || (mid(&c) - mid(&r)) * sign <= EDGE || gap < -EDGE || overlap <= EDGE {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, g, o)) => gap < g - EDGE || (gap <= g + EDGE && overlap > o + EDGE),
        };
        if better {
            best = Some((id, gap, overlap));
        }
    }
    best.map(|(id, ..)| id)
}

/// A split's structure without its sizes: what a tab re-lays out on (a split or close), not a drag.
pub fn shape(l: &Layout) -> String {
    match l {
        Layout::Pane(id) => id.to_string(),
        Layout::Split(s) => {
            let kids: Vec<String> = s.children.iter().map(shape).collect();
            format!("{:?}({})", s.dir, kids.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: PaneId) -> Layout {
        Layout::Pane(id)
    }
    fn split(dir: Dir, children: Vec<Layout>, sizes: Vec<f64>) -> Layout {
        Layout::Split(Split {
            dir,
            children,
            sizes,
        })
    }
    fn across(children: Vec<Layout>) -> Layout {
        let n = children.len();
        split(Dir::Row, children, vec![100. / n as f64; n])
    }
    fn col(children: Vec<Layout>) -> Layout {
        let n = children.len();
        split(Dir::Col, children, vec![100. / n as f64; n])
    }

    /// Where each pane is drawn in a 1200×800 tab with 1px dividers, as the panel lays them out.
    fn drawn(l: &Layout) -> Vec<(PaneId, Rect)> {
        fn walk(l: &Layout, b: Rect, out: &mut Vec<(PaneId, Rect)>) {
            match l {
                Layout::Pane(id) => out.push((*id, b)),
                Layout::Split(s) => {
                    let row = s.dir == Dir::Row;
                    let free =
                        (if row { b.width } else { b.height }) - (s.children.len() as f64 - 1.);
                    let mut at = if row { b.x } else { b.y };
                    for (c, size) in s.children.iter().zip(&s.sizes) {
                        let len = free * size / 100.;
                        let part = if row {
                            Rect {
                                x: at,
                                width: len,
                                ..b
                            }
                        } else {
                            Rect {
                                y: at,
                                height: len,
                                ..b
                            }
                        };
                        walk(c, part, out);
                        at += len + 1.;
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(
            l,
            Rect {
                x: 0.,
                y: 0.,
                width: 1200.,
                height: 800.,
            },
            &mut out,
        );
        out
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn sound(l: &Layout, parent: Option<Dir>) {
        let Layout::Split(s) = l else { return };
        assert!(s.children.len() >= 2 && s.sizes.len() == s.children.len());
        assert!(close(s.sizes.iter().sum::<f64>(), 100.));
        assert!(s.sizes.iter().all(|&x| x > 0.));
        assert_ne!(Some(s.dir), parent, "a split inside one going the same way");
        s.children.iter().for_each(|c| sound(c, Some(s.dir)));
    }

    #[test]
    fn splits_go_right_or_down_and_a_split_already_going_that_way_takes_one_more_pane() {
        let mut l = p(1);
        l = split_pane(&l, 1, 2, Dir::Row);
        assert_eq!(l, split(Dir::Row, vec![p(1), p(2)], vec![50., 50.]));
        l = split_pane(&l, 1, 3, Dir::Row);
        assert_eq!(
            l,
            split(Dir::Row, vec![p(1), p(3), p(2)], vec![25., 25., 50.])
        );
        l = split_pane(&l, 2, 4, Dir::Col);
        assert_eq!(
            l,
            split(
                Dir::Row,
                vec![
                    p(1),
                    p(3),
                    split(Dir::Col, vec![p(2), p(4)], vec![50., 50.])
                ],
                vec![25., 25., 50.]
            )
        );
        assert_eq!(leaves(&l), vec![1, 3, 2, 4]);
    }

    #[test]
    fn panes_are_numbered_row_by_row_from_the_top_each_left_to_right() {
        assert_eq!(
            pane_order(&across(vec![col(vec![p(1), p(3)]), col(vec![p(2), p(4)])])),
            [1, 2, 3, 4]
        );
        assert_eq!(
            pane_order(&col(vec![
                across(vec![p(1), p(2)]),
                across(vec![p(3), p(4)])
            ])),
            [1, 2, 3, 4]
        );
        assert_eq!(
            pane_order(&across(vec![
                p(1),
                col(vec![p(2), across(vec![p(3), p(4)])])
            ])),
            [1, 2, 3, 4]
        );
        let uneven = across(vec![
            split(Dir::Col, vec![p(1), p(4)], vec![70., 30.]),
            split(Dir::Col, vec![p(2), p(3)], vec![40., 60.]),
        ]);
        assert_eq!(pane_order(&uneven), [1, 2, 3, 4]);
        // A divider dragged a hair off its neighbour's still leaves one row.
        let hair = across(vec![
            split(Dir::Col, vec![p(1), p(3)], vec![50., 50.]),
            split(Dir::Col, vec![p(2), p(4)], vec![50.4, 49.6]),
        ]);
        assert_eq!(pane_order(&hair), [1, 2, 3, 4]);
        // Tree order isn't screen order.
        let l = across(vec![col(vec![p(7), p(4)]), p(9)]);
        assert_eq!(leaves(&l), [7, 4, 9]);
        assert_eq!(pane_order(&l), [7, 9, 4]);
        assert_eq!(pane_order(&p(5)), [5]);
        let r = pane_rects(
            &split(
                Dir::Row,
                vec![p(1), split(Dir::Col, vec![p(2), p(3)], vec![25., 75.])],
                vec![40., 60.],
            ),
            Rect::UNIT,
        );
        let three = r.iter().find(|(id, _)| *id == 3).unwrap().1;
        assert!(
            close(three.x, 0.4)
                && close(three.y, 0.25)
                && close(three.width, 0.6)
                && close(three.height, 0.75)
        );
    }

    #[test]
    fn the_numbered_panes_step_in_order_and_wrap_past_either_end() {
        let order = [7, 9, 4];
        let next: Vec<_> = order
            .iter()
            .map(|&id| step_in(&order, id, 1).unwrap())
            .collect();
        assert_eq!(next, [9, 4, 7]);
        let prev: Vec<_> = order
            .iter()
            .map(|&id| step_in(&order, id, -1).unwrap())
            .collect();
        assert_eq!(prev, [4, 7, 9]);
        assert_eq!(step_in(&[7], 7, 1), None);
        assert_eq!(step_in(&order, 5, 1), None);
        let mut at = 4;
        for _ in 0..order.len() {
            at = step_in(&order, at, -1).unwrap();
        }
        assert_eq!(at, 4);
    }

    #[test]
    fn closing_a_pane_gives_its_space_to_the_rest_in_proportion() {
        let grid = across(vec![col(vec![p(1), p(2)]), col(vec![p(3), p(4)])]);
        let l = remove_pane(&grid, 2).unwrap();
        assert_eq!(l, across(vec![p(1), col(vec![p(3), p(4)])]));
        assert_eq!(
            remove_pane(
                &split(Dir::Row, vec![p(1), p(2), p(3)], vec![20., 30., 50.]),
                1
            ),
            Some(split(Dir::Row, vec![p(2), p(3)], vec![37.5, 62.5]))
        );
        assert_eq!(remove_pane(&remove_pane(&l, 1).unwrap(), 3), Some(p(4)));
        assert_eq!(remove_pane(&p(1), 1), None);
        // A row inside a row (the column around it collapsed) joins its parent.
        let nested = split(
            Dir::Row,
            vec![
                split(
                    Dir::Col,
                    vec![split(Dir::Row, vec![p(2), p(3)], vec![25., 75.]), p(4)],
                    vec![50., 50.],
                ),
                p(1),
            ],
            vec![60., 40.],
        );
        let Some(Layout::Split(s)) = remove_pane(&nested, 4) else {
            panic!()
        };
        assert_eq!(s.children, vec![p(2), p(3), p(1)]);
        assert!(
            s.sizes
                .iter()
                .zip([15., 45., 40.])
                .all(|(a, b)| close(*a, b))
        );
    }

    #[test]
    fn two_dividers_dragged_at_once_both_keep_their_sizes() {
        let inner = split(Dir::Col, vec![p(2), p(3)], vec![50., 50.]);
        let left = split(Dir::Col, vec![p(4), p(5)], vec![50., 50.]);
        let l = split(
            Dir::Row,
            vec![left.clone(), p(1), inner],
            vec![30., 30., 40.],
        );
        let both = resize(&resize(&l, &[], &[20., 40., 40.]), &[2], &[30., 70.]);
        assert_eq!(
            both,
            split(
                Dir::Row,
                vec![
                    left,
                    p(1),
                    split(Dir::Col, vec![p(2), p(3)], vec![30., 70.])
                ],
                vec![20., 40., 40.]
            )
        );
        assert_eq!(resize(&l, &[1], &[50., 50.]), l, "a path to a pane");
        assert_eq!(
            resize(&l, &[2], &[10., 20., 70.]),
            l,
            "sizes for another split"
        );
    }

    #[test]
    fn any_run_of_splits_and_closes_keeps_the_tree_sound() {
        // mulberry32: the same run every time.
        let mut seed: u32 = 7;
        let mut random = || {
            seed = seed.wrapping_add(0x6d2b79f5);
            let mut t = (seed ^ (seed >> 15)).wrapping_mul(1 | seed);
            t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
            ((t ^ (t >> 14)) as f64) / 4294967296.
        };
        let mut l = p(1);
        let mut live = vec![1];
        let mut next = 2;
        for _ in 0..500 {
            let pick = live[(random() * live.len() as f64) as usize];
            if live.len() > 1 && random() < 0.4 {
                l = remove_pane(&l, pick).unwrap();
                live.retain(|&id| id != pick);
            } else {
                l = split_pane(
                    &l,
                    pick,
                    next,
                    if random() < 0.5 { Dir::Row } else { Dir::Col },
                );
                live.push(next);
                next += 1;
            }
            sound(&l, None);
            let mut got = leaves(&l);
            got.sort();
            let mut want = live.clone();
            want.sort();
            assert_eq!(got, want);
        }
    }

    #[test]
    fn focus_moves_to_the_pane_drawn_on_that_side() {
        use Direction::*;
        type Moves = [(Direction, Option<PaneId>); 4];
        let moves: [(PaneId, Moves); 4] = [
            (
                1,
                [(Left, None), (Right, Some(2)), (Up, None), (Down, Some(3))],
            ),
            (
                2,
                [(Left, Some(1)), (Right, None), (Up, None), (Down, Some(4))],
            ),
            (
                3,
                [(Left, None), (Right, Some(4)), (Up, Some(1)), (Down, None)],
            ),
            (
                4,
                [(Left, Some(3)), (Right, None), (Up, Some(2)), (Down, None)],
            ),
        ];
        let columns = across(vec![col(vec![p(1), p(3)]), col(vec![p(2), p(4)])]);
        let rows = col(vec![across(vec![p(1), p(2)]), across(vec![p(3), p(4)])]);
        for grid in [&columns, &rows] {
            for (from, ways) in &moves {
                for (dir, want) in ways {
                    assert_eq!(neighbor(&drawn(grid), *from, *dir), *want, "{from} {dir:?}");
                }
            }
        }
        for dir in [Left, Right, Up, Down] {
            assert_eq!(neighbor(&drawn(&p(1)), 1, dir), None);
        }
        assert_eq!(
            neighbor(&drawn(&columns), 9, Left),
            None,
            "a pane that's gone"
        );
        let over = col(vec![across(vec![p(1), p(2), p(3)]), p(4)]);
        assert_eq!(neighbor(&drawn(&over), 4, Up), Some(1));
        assert_eq!(neighbor(&drawn(&over), 2, Down), Some(4));
        assert_eq!(neighbor(&drawn(&over), 3, Left), Some(2));
        assert_eq!(neighbor(&drawn(&over), 1, Left), None);
    }

    #[test]
    fn with_panes_a_few_pixels_wide_focus_never_goes_the_other_way() {
        let b = |x: f64, width: f64| Rect {
            x,
            y: 0.,
            width,
            height: 400.,
        };
        let thin = vec![(3, b(6., 2.)), (1, b(0., 2.)), (2, b(3., 2.))];
        assert_eq!(neighbor(&thin, 2, Direction::Left), Some(1));
        assert_eq!(neighbor(&thin, 2, Direction::Right), Some(3));
        assert_eq!(neighbor(&thin, 1, Direction::Right), Some(2));
        assert_eq!(neighbor(&thin, 3, Direction::Left), Some(2));
        assert_eq!(neighbor(&thin, 1, Direction::Left), None);
        assert_eq!(neighbor(&thin, 3, Direction::Right), None);
        let hidden = vec![(1, b(0., 0.)), (2, b(0., 0.))];
        assert_eq!(neighbor(&hidden, 1, Direction::Right), None);
    }

    #[test]
    fn equalizing_gives_a_split_equal_sizes_and_deep_every_split_below() {
        let inner = split(Dir::Col, vec![p(2), p(3), p(4)], vec![10., 30., 60.]);
        let l = split(Dir::Row, vec![p(1), inner.clone()], vec![70., 30.]);
        assert_eq!(
            equalize(&l, &[], false),
            split(Dir::Row, vec![p(1), inner], vec![50., 50.])
        );
        let third = 100. / 3.;
        let even = split(Dir::Col, vec![p(2), p(3), p(4)], vec![third; 3]);
        assert_eq!(
            equalize(&l, &[1], false),
            split(Dir::Row, vec![p(1), even.clone()], vec![70., 30.])
        );
        assert_eq!(
            equalize(&l, &[], true),
            split(Dir::Row, vec![p(1), even], vec![50., 50.])
        );
        assert_eq!(equalize(&p(5), &[], true), p(5));
        assert_eq!(equalize(&l, &[0, 1], false), l);
    }

    #[test]
    fn mapping_ids_keeps_the_tree() {
        let l = split(
            Dir::Col,
            vec![p(7), split(Dir::Row, vec![p(9), p(8)], vec![40., 60.])],
            vec![65., 35.],
        );
        let mapped = map_panes(&l, &|id| id + 100);
        assert_eq!(leaves(&mapped), [107, 109, 108]);
        assert_eq!(shape(&l), "Col(7,Row(9,8))");
    }
}
