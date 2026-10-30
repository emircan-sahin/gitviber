//! Fuzzy matching for the command palette and quick open: the query's characters in order,
//! scored as fzy does, so matches at word starts and in a row rank first. Case and spaces in the
//! query are ignored ("open file", "gtp" → "Git: Push" both match).

use std::cmp::Ordering;
use std::sync::atomic::{AtomicBool, Ordering as Atomic};

use gpui_kit::SharedString;

#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub score: f64,
    /// Char indices into the text of the matched characters, for highlighting.
    pub hits: Vec<usize>,
}

const GAP_LEADING: f64 = -0.005;
const GAP_TRAILING: f64 = -0.005;
const GAP_INNER: f64 = -0.01;
const CONSECUTIVE: f64 = 1.0;

fn bonus(text: &[char], j: usize) -> f64 {
    if j == 0 {
        return 0.9;
    }
    let prev = text[j - 1];
    match prev {
        '/' | '\\' => 0.9,
        '-' | '_' | ' ' | ':' => 0.8,
        '.' => 0.6,
        _ if prev.is_lowercase() && text[j].is_uppercase() => 0.7,
        _ => 0.0,
    }
}

/// The query as matching wants it: lower case, without spaces.
pub fn prepare_query(query: &str) -> Vec<char> {
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A text's char as matching compares it: one char, so indices stay those of the text
/// ("İ" lower-cases to two).
fn lower_char(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// The chars a text holds, as a set of 64 buckets: a query with a char the text lacks can't
/// match, and checking that is one AND. Worked out once for texts matched on every keystroke.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CharBag(u64);

impl CharBag {
    fn bit(c: char) -> u64 {
        let ix = match c {
            'a'..='z' => c as u32 - 'a' as u32,
            '0'..='9' => 26 + (c as u32 - '0' as u32),
            _ => 36 + c as u32 % 28,
        };
        1 << ix
    }

    pub fn of(text: &str) -> Self {
        Self(
            text.chars()
                .fold(0, |bag, c| bag | Self::bit(lower_char(c))),
        )
    }

    /// A query from `prepare_query`.
    pub fn of_query(query: &[char]) -> Self {
        Self(query.iter().fold(0, |bag, c| bag | Self::bit(*c)))
    }

    pub fn contains(self, other: CharBag) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Texts matched again on every keystroke (quick open's files), each with its `CharBag`.
#[derive(Default)]
pub struct Candidates {
    texts: Vec<SharedString>,
    bags: Vec<CharBag>,
}

impl Candidates {
    pub fn new(texts: Vec<SharedString>) -> Self {
        let bags = texts.iter().map(|t| CharBag::of(t)).collect();
        Self { texts, bags }
    }

    pub fn len(&self) -> usize {
        self.texts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }

    pub fn texts(&self) -> &[SharedString] {
        &self.texts
    }

    pub fn bags(&self) -> &[CharBag] {
        &self.bags
    }
}

/// Scores `text` against a query from `prepare_query`; None when not every character is in it,
/// in order. `Scorer` repeats these sums: a change to the scoring changes both.
pub fn fuzzy_match(query: &[char], text: &str) -> Option<Match> {
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| lower_char(*c)).collect();
    let (m, n) = (query.len(), chars.len());
    if m == 0 {
        return Some(Match {
            score: 0.0,
            hits: vec![],
        });
    }
    if m > n {
        return None;
    }
    // Most texts fail here, before the O(m·n) scoring.
    let mut j = 0;
    for q in query {
        j += lower[j..].iter().position(|c| c == q)? + 1;
    }
    if m == n {
        return Some(Match {
            score: f64::INFINITY,
            hits: (0..n).collect(),
        });
    }

    // d: best score with query[i] matched at text[j]; best: best score for query[..i] within text[..j].
    let mut d = vec![vec![f64::NEG_INFINITY; n]; m];
    let mut best = vec![vec![f64::NEG_INFINITY; n]; m];
    let bonuses: Vec<f64> = (0..n).map(|j| bonus(&chars, j)).collect();
    for i in 0..m {
        let mut prev = f64::NEG_INFINITY;
        let gap = if i == m - 1 { GAP_TRAILING } else { GAP_INNER };
        for j in 0..n {
            if lower[j] == query[i] {
                let score = if i == 0 {
                    j as f64 * GAP_LEADING + bonuses[j]
                } else if j > 0 {
                    (best[i - 1][j - 1] + bonuses[j]).max(d[i - 1][j - 1] + CONSECUTIVE)
                } else {
                    f64::NEG_INFINITY
                };
                d[i][j] = score;
                prev = score.max(prev + gap);
            } else {
                prev += gap;
            }
            best[i][j] = prev;
        }
    }

    let mut hits = vec![0; m];
    let mut required = false;
    let mut j = n as isize - 1;
    for i in (0..m).rev() {
        while j >= 0 {
            let ju = j as usize;
            if d[i][ju] != f64::NEG_INFINITY && (required || d[i][ju] == best[i][ju]) {
                required = i > 0 && ju > 0 && best[i][ju] == d[i - 1][ju - 1] + CONSECUTIVE;
                hits[i] = ju;
                j -= 1;
                break;
            }
            j -= 1;
        }
    }
    Some(Match {
        score: best[m - 1][n - 1],
        hits,
    })
}

/// A path, matched on its file name first: "fuzzy" finds src/lib/fuzzy.ts before
/// src/fuzzy-old/index.ts. Falls back to the whole path, so "lib/fuz" works too.
pub fn match_path(query: &[char], path: &str) -> Option<Match> {
    let start = path.rfind('/').map(|i| i + 1).unwrap_or(0);
    let offset = path[..start].chars().count();
    if let Some(name) = fuzzy_match(query, &path[start..]) {
        return Some(Match {
            score: name.score + 10.0,
            hits: name.hits.iter().map(|h| h + offset).collect(),
        });
    }
    fuzzy_match(query, path)
}

/// `fuzzy_match`'s score alone, from two reused rows instead of two matrices: the same sums in
/// the same order, so the same score. Ranking calls it for every candidate; hits are traced for
/// the shown ones only.
#[derive(Default)]
pub struct Scorer {
    chars: Vec<char>,
    lower: Vec<char>,
    bonuses: Vec<f64>,
    d: (Vec<f64>, Vec<f64>),
    best: (Vec<f64>, Vec<f64>),
}

impl Scorer {
    pub fn score(&mut self, query: &[char], text: &str) -> Option<f64> {
        let m = query.len();
        if m == 0 {
            return Some(0.0);
        }
        // Most texts fail here, before anything is copied.
        let mut rest = query;
        let mut n = 0;
        for c in text.chars() {
            n += 1;
            if rest.first() == Some(&lower_char(c)) {
                rest = &rest[1..];
            }
        }
        if m > n || !rest.is_empty() {
            return None;
        }
        if m == n {
            return Some(f64::INFINITY);
        }

        self.chars.clear();
        self.chars.extend(text.chars());
        self.lower.clear();
        self.lower.extend(self.chars.iter().map(|c| lower_char(*c)));
        self.bonuses.clear();
        self.bonuses.extend((0..n).map(|j| bonus(&self.chars, j)));
        let (d_prev, d) = &mut self.d;
        let (best_prev, best) = &mut self.best;
        for row in [&mut *d_prev, &mut *d, &mut *best_prev, &mut *best] {
            row.clear();
            row.resize(n, f64::NEG_INFINITY);
        }
        for i in 0..m {
            let mut prev = f64::NEG_INFINITY;
            let gap = if i == m - 1 { GAP_TRAILING } else { GAP_INNER };
            for j in 0..n {
                if self.lower[j] == query[i] {
                    let score = if i == 0 {
                        j as f64 * GAP_LEADING + self.bonuses[j]
                    } else if j > 0 {
                        (best_prev[j - 1] + self.bonuses[j]).max(d_prev[j - 1] + CONSECUTIVE)
                    } else {
                        f64::NEG_INFINITY
                    };
                    d[j] = score;
                    prev = score.max(prev + gap);
                } else {
                    d[j] = f64::NEG_INFINITY;
                    prev += gap;
                }
                best[j] = prev;
            }
            std::mem::swap(d_prev, d);
            std::mem::swap(best_prev, best);
        }
        Some(best_prev[n - 1])
    }

    /// `match_path`'s score.
    pub fn score_path(&mut self, query: &[char], path: &str) -> Option<f64> {
        let start = path.rfind('/').map(|i| i + 1).unwrap_or(0);
        if let Some(name) = self.score(query, &path[start..]) {
            return Some(name + 10.0);
        }
        self.score(query, path)
    }
}

/// One text to rank.
pub struct Candidate<'a> {
    pub text: &'a str,
    /// Its `CharBag`, when worked out ahead.
    pub bag: Option<CharBag>,
    /// Lower first, after the score; None after every Some.
    pub recency: Option<usize>,
}

/// The `limit` best matches of `query` (from `prepare_query`) as (index, match), best first: by
/// score, then recency, then their order. Paths are matched with `match_path`. None when
/// `cancel` was set midway: a newer query took over.
pub fn rank<'a>(
    query: &[char],
    candidates: impl IntoIterator<Item = Candidate<'a>>,
    path: bool,
    limit: usize,
    cancel: &AtomicBool,
) -> Option<Vec<(usize, Match)>> {
    let query_bag = CharBag::of_query(query);
    let mut scorer = Scorer::default();
    let mut found: Vec<(usize, f64, usize, &str)> = Vec::new();
    for (at, c) in candidates.into_iter().enumerate() {
        if at % 1024 == 0 && cancel.load(Atomic::Relaxed) {
            return None;
        }
        if c.bag.is_some_and(|bag| !bag.contains(query_bag)) {
            continue;
        }
        let score = if path {
            scorer.score_path(query, c.text)
        } else {
            scorer.score(query, c.text)
        };
        if let Some(score) = score {
            found.push((at, score, c.recency.unwrap_or(usize::MAX), c.text));
        }
    }
    let order = |a: &(usize, f64, usize, &str), b: &(usize, f64, usize, &str)| {
        let by_score = if query.is_empty() {
            Ordering::Equal
        } else {
            b.1.total_cmp(&a.1)
        };
        by_score.then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0))
    };
    if found.len() > limit {
        found.select_nth_unstable_by(limit, order);
        found.truncate(limit);
    }
    found.sort_unstable_by(order);
    Some(
        found
            .into_iter()
            .filter_map(|(at, _, _, text)| {
                let m = if path {
                    match_path(query, text)
                } else {
                    fuzzy_match(query, text)
                };
                Some((at, m?))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(q: &str, t: &str) -> Option<f64> {
        fuzzy_match(&prepare_query(q), t).map(|m| m.score)
    }

    #[test]
    fn matches_in_order_only() {
        assert!(score("gtp", "Git: Push").is_some());
        assert!(score("open file", "Open File").is_some());
        assert!(score("xyz", "Git: Push").is_none());
        assert!(score("pg", "Git: Push").is_none());
        assert_eq!(score("", "anything"), Some(0.0));
    }

    #[test]
    fn word_starts_and_runs_rank_first() {
        assert!(score("tt", "Toggle Terminal").unwrap() > score("tt", "Settings").unwrap());
        assert!(score("push", "Push").unwrap() > score("push", "Git: Push").unwrap());
        assert!(
            score("term", "Toggle Terminal").unwrap() > score("term", "the error message").unwrap()
        );
    }

    #[test]
    fn hits_point_at_the_matched_chars() {
        let m = fuzzy_match(&prepare_query("gp"), "Git: Push").unwrap();
        assert_eq!(m.hits, vec![0, 5]);
    }

    #[test]
    fn paths_prefer_the_file_name() {
        let q = prepare_query("fuzzy");
        let a = match_path(&q, "src/lib/fuzzy.ts").unwrap();
        let b = match_path(&q, "src/fuzzy-old/index.ts").unwrap();
        assert!(a.score > b.score);
        assert_eq!(a.hits[0], 8);
        assert!(match_path(&prepare_query("lib/fuz"), "src/lib/fuzzy.ts").is_some());
    }

    /// Made-up paths and titles, with case, separators and a non-ASCII name.
    fn texts() -> Vec<String> {
        let words = [
            "Git",
            "push",
            "src",
            "lib",
            "Fuzzy",
            "term_view",
            "İndex",
            "a-b",
            "x.rs",
        ];
        let mut out = Vec::new();
        for (i, a) in words.iter().enumerate() {
            for (j, b) in words.iter().enumerate() {
                out.push(format!("{a}/{b}"));
                out.push(format!("{a}: {b}{}", i * j));
                out.push(format!("{b}{a}"));
            }
        }
        out
    }

    const QUERIES: [&str; 12] = [
        "",
        "g",
        "gp",
        "src",
        "lib/fu",
        "push",
        "tv",
        "i",
        "xrs",
        "ab",
        "zz",
        "fuzzyfuzzy",
    ];

    #[test]
    fn the_scorer_scores_as_the_full_match_does() {
        let mut scorer = Scorer::default();
        for q in QUERIES {
            let q = prepare_query(q);
            for t in texts() {
                let full = fuzzy_match(&q, &t).map(|m| m.score);
                assert_eq!(scorer.score(&q, &t), full, "{q:?} in {t}");
                let full = match_path(&q, &t).map(|m| m.score);
                assert_eq!(scorer.score_path(&q, &t), full, "{q:?} in path {t}");
                if full.is_some() {
                    assert!(CharBag::of(&t).contains(CharBag::of_query(&q)), "{q:?} {t}");
                }
            }
        }
    }

    /// The top few, found without sorting them all, rank exactly as a full sort of every match
    /// does: by score, then recency, then the list's order.
    #[test]
    fn ranking_the_top_few_keeps_the_full_sorts_order() {
        let texts = texts();
        let recency = |i: usize| (i % 7 == 0).then_some(i % 3);
        for path in [false, true] {
            for q in QUERIES {
                let q = prepare_query(q);
                let mut full: Vec<(usize, Match, usize)> = texts
                    .iter()
                    .enumerate()
                    .filter_map(|(at, t)| {
                        let m = if path {
                            match_path(&q, t)
                        } else {
                            fuzzy_match(&q, t)
                        }?;
                        Some((at, m, recency(at).unwrap_or(usize::MAX)))
                    })
                    .collect();
                full.sort_by(|a, b| {
                    let by_score = if q.is_empty() {
                        Ordering::Equal
                    } else {
                        b.1.score.total_cmp(&a.1.score)
                    };
                    by_score.then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0))
                });
                for limit in [1, 5, 40, 10_000] {
                    let candidates = texts.iter().enumerate().map(|(at, t)| Candidate {
                        text: t,
                        bag: (at % 2 == 0).then(|| CharBag::of(t)),
                        recency: recency(at),
                    });
                    let top = rank(&q, candidates, path, limit, &AtomicBool::new(false)).unwrap();
                    let expected: Vec<(usize, Match)> = full
                        .iter()
                        .take(limit)
                        .map(|(at, m, _)| (*at, m.clone()))
                        .collect();
                    assert_eq!(top, expected, "{q:?}, path {path}, limit {limit}");
                }
            }
        }
    }

    #[test]
    fn a_cancelled_ranking_gives_nothing() {
        let texts = texts();
        let candidates = texts.iter().map(|t| Candidate {
            text: t,
            bag: None,
            recency: None,
        });
        assert!(
            rank(
                &prepare_query("g"),
                candidates,
                false,
                10,
                &AtomicBool::new(true)
            )
            .is_none()
        );
    }
}
