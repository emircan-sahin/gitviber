//! Fuzzy matching for the command palette and quick open: the query's characters in order,
//! scored as fzy does, so matches at word starts and in a row rank first. Case and spaces in the
//! query are ignored ("open file", "gtp" → "Git: Push" both match).

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

/// Scores `text` against a query from `prepare_query`; None when not every character is in it,
/// in order.
pub fn fuzzy_match(query: &[char], text: &str) -> Option<Match> {
    let chars: Vec<char> = text.chars().collect();
    // One char each, so indices stay those of `text` ("İ" lower-cases to two).
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
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
}
