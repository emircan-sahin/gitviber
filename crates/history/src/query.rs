//! History's search box: plain words must all be in the message; `author:`, `path:` and `code:`
//! (text a commit added or removed) narrow further. Quotes keep spaces in one term
//! (`author:"Ada Lovelace"`). Pure.

use gitviber_core::git;

/// What core's log is narrowed by.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Filter {
    pub grep: Vec<String>,
    pub author: Vec<String>,
    pub code: Option<String>,
    pub paths: Vec<String>,
    /// Follow a single file's `paths` entry through renames.
    pub follow: bool,
}

impl Filter {
    pub fn is_empty(&self) -> bool {
        self.grep.is_empty()
            && self.author.is_empty()
            && self.code.is_none()
            && self.paths.is_empty()
    }

    pub fn to_core(&self) -> git::LogFilter {
        git::LogFilter {
            grep: self.grep.clone(),
            author: self.author.clone(),
            code: self.code.clone(),
            paths: self.paths.clone(),
            follow: self.follow,
        }
    }
}

/// A word, or one with quoted parts that keep its spaces: `[^\s"]*"[^"]*"?` repeated, then the
/// rest of the word; else a run of non-space. An unclosed quote takes in the rest.
fn tokens(text: &str) -> Vec<&str> {
    let c: Vec<(usize, char)> = text.char_indices().collect();
    let n = c.len();
    let at = |i: usize| if i < n { c[i].0 } else { text.len() };
    let word = |i: usize| i < n && !c[i].1.is_whitespace() && c[i].1 != '"';
    let mut out = vec![];
    let mut i = 0;
    while i < n {
        if c[i].1.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i;
        let mut quoted = false;
        loop {
            let mut k = j;
            while word(k) {
                k += 1;
            }
            if k < n && c[k].1 == '"' {
                k += 1;
                while k < n && c[k].1 != '"' {
                    k += 1;
                }
                if k < n {
                    k += 1;
                }
                j = k;
                quoted = true;
            } else {
                break;
            }
        }
        if quoted {
            while word(j) {
                j += 1;
            }
        } else {
            j = i;
            while j < n && !c[j].1.is_whitespace() {
                j += 1;
            }
        }
        out.push(&text[at(start)..at(j)]);
        i = j;
    }
    out
}

/// "author", "path" or "code", any case, and what follows its colon.
fn prefixed(token: &str) -> Option<(String, &str)> {
    let (key, value) = token.split_once(':')?;
    let key = key.to_lowercase();
    matches!(key.as_str(), "author" | "path" | "code").then_some((key, value))
}

/// An abbreviated SHA is 7 characters or more; shorter hex is too often a word ("added", "face").
fn sha_like(word: &str) -> bool {
    (7..=40).contains(&word.len()) && word.chars().all(|c| c.is_ascii_hexdigit())
}

/// The filter, and the words that could be a commit id (looked up too).
pub fn parse_log_query(text: &str) -> (Filter, Vec<String>) {
    let mut filter = Filter::default();
    let mut shas = vec![];
    for raw in tokens(text) {
        let token = raw.replace('"', "");
        match prefixed(&token) {
            None => {
                if token.is_empty() {
                    continue;
                }
                if sha_like(&token) {
                    shas.push(token.clone());
                }
                filter.grep.push(token);
            }
            Some((_, "")) => {}
            Some((key, value)) => match key.as_str() {
                "author" => filter.author.push(value.to_string()),
                "code" => filter.code = Some(value.to_string()),
                _ => filter.paths.push(
                    value
                        .strip_prefix("./")
                        .unwrap_or(value)
                        .trim_end_matches('/')
                        .to_string(),
                ),
            },
        }
    }
    (filter, shas)
}

/// The `author:` value that finds one author: git matches a fixed part of "Name <email>", so with
/// the email it is that person alone. A term can't hold a quote: then the longest stretch without
/// one, which is still a part of it.
pub fn author_term(name: &str, email: Option<&str>) -> String {
    let full = match email {
        Some(e) if !e.is_empty() => format!("{name} <{e}>"),
        _ => name.to_string(),
    };
    full.split('"')
        .map(str::trim)
        .fold("", |a, b| {
            if b.chars().count() > a.chars().count() {
                b
            } else {
                a
            }
        })
        .to_string()
}

/// `text` narrowed to one author, as a click on a commit's author asks: the `author:` terms there
/// were go, the rest stays.
pub fn with_author(text: &str, name: &str, email: Option<&str>) -> String {
    let mut kept: Vec<String> = tokens(text)
        .into_iter()
        .filter(|t| !t.replace('"', "").to_lowercase().starts_with("author:"))
        // An unclosed quote would take in the author term after it.
        .map(|t| {
            if t.matches('"').count() % 2 == 1 {
                format!("{t}\"")
            } else {
                t.to_string()
            }
        })
        .collect();
    let who = author_term(name, email);
    kept.push(if who.chars().any(char::is_whitespace) {
        format!("author:\"{who}\"")
    } else {
        format!("author:{who}")
    });
    kept.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_match_the_message_prefixes_narrow() {
        let (f, _) = parse_log_query("  fix auth  author:ada path:./src/lib/ code:useState ");
        assert_eq!(
            f,
            Filter {
                grep: vec!["fix".into(), "auth".into()],
                author: vec!["ada".into()],
                code: Some("useState".into()),
                paths: vec!["src/lib".into()],
                follow: false
            }
        );
    }

    #[test]
    fn quotes_keep_spaces_prefixes_ignore_case_empty_values_drop() {
        let (f, _) =
            parse_log_query("\"fix login\" Author:\"Ada Lovelace\" code:\"a b\" path: author:");
        assert_eq!(f.grep, vec!["fix login"]);
        assert_eq!(f.author, vec!["Ada Lovelace"]);
        assert_eq!(f.code.as_deref(), Some("a b"));
        assert!(f.paths.is_empty());
    }

    #[test]
    fn sha_looking_words_are_also_looked_up() {
        assert_eq!(
            parse_log_query("1a2b3c4 added deadbeefcafe path:abcdef0").1,
            vec!["1a2b3c4", "deadbeefcafe"]
        );
        assert_eq!(parse_log_query("1a2b3c4").0.grep, vec!["1a2b3c4"]);
    }

    #[test]
    fn an_empty_or_blank_search_is_no_filter() {
        assert!(parse_log_query("").0.is_empty());
        assert!(parse_log_query("  \"\" ").0.is_empty());
        assert!(!parse_log_query("x").0.is_empty());
    }

    #[test]
    fn a_clicked_author_replaces_the_author_terms_and_keeps_the_rest() {
        assert_eq!(with_author("", "ada", None), "author:ada");
        assert_eq!(
            with_author(
                "fix path:src author:bob Author:\"Bo B\"",
                "Ada Lovelace",
                None
            ),
            "fix path:src author:\"Ada Lovelace\""
        );
        assert_eq!(with_author("author:ada", "ada", None), "author:ada");
        assert_eq!(
            with_author(
                &with_author("\"fix login\"", "Ada Lovelace", None),
                "Ada Lovelace",
                None
            ),
            "\"fix login\" author:\"Ada Lovelace\""
        );
    }

    #[test]
    fn whatever_the_name_the_parser_reads_it_back_as_one_author() {
        for name in [
            "ada",
            "Ada Lovelace",
            "  Grace  Hopper ",
            "Jean-Luc O'Brien",
            "Zoë Çelik",
            "path:src",
            "dev@example.com",
        ] {
            let (f, _) = parse_log_query(&with_author("code:x", name, None));
            assert_eq!(f.author, vec![name.trim()], "{name}");
            assert_eq!(f.code.as_deref(), Some("x"));
            assert!(f.grep.is_empty());
        }
    }

    #[test]
    fn a_clicked_author_keeps_every_other_kind_of_term_in_order() {
        let before = "fix \"login flow\" path:\"src/a b\" code:useState abc1234 AUTHOR:old author:\"Old Name\"";
        let after = with_author(before, "Ada", None);
        assert_eq!(
            after,
            "fix \"login flow\" path:\"src/a b\" code:useState abc1234 author:Ada"
        );
        let (f, shas) = parse_log_query(&after);
        assert_eq!(f.grep, vec!["fix", "login flow", "abc1234"]);
        assert_eq!(f.paths, vec!["src/a b"]);
        assert_eq!(f.code.as_deref(), Some("useState"));
        assert_eq!(f.author, vec!["Ada"]);
        assert_eq!(shas, vec!["abc1234"]);
    }

    #[test]
    fn names_made_of_syntax_read_back_as_one_author() {
        let long = "x".repeat(10_000);
        let longer = "Ada Lovelace ".repeat(1_000).trim().to_string();
        let names = [
            "author:mallory",
            "path:src code:x",
            "-n",
            "--all",
            "a.b*",
            "Ada (bot)",
            "[^a]+$",
            "C:\\Users\\ada",
            "\\",
            "!@#$%^&*()[]{}|;:',.<>?/~`=+",
            "🦀 Ferris the Crab",
            "李 小龍",
            "Ada\tLovelace",
            long.as_str(),
            longer.as_str(),
        ];
        for name in names {
            let (f, shas) = parse_log_query(&with_author("fix", name, None));
            assert_eq!(f.author, vec![name], "{}", &name[..name.len().min(40)]);
            assert_eq!(f.grep, vec!["fix"]);
            assert!(f.paths.is_empty() && f.code.is_none() && shas.is_empty());
        }
    }

    #[test]
    fn clicking_authors_one_after_another_keeps_one_author_term() {
        let mut q = "fix".to_string();
        for i in 0..200 {
            q = with_author(
                &q,
                &if i % 2 == 1 {
                    format!("Dev {i}")
                } else {
                    format!("dev{i}")
                },
                None,
            );
        }
        assert_eq!(q, "fix author:\"Dev 199\"");
    }

    #[test]
    fn a_long_pathological_search_parses_quickly() {
        let start = std::time::Instant::now();
        for text in [
            "\"".repeat(20_000),
            "a\"".repeat(10_000),
            "a ".repeat(10_000),
            format!("author:\"{}", "x ".repeat(10_000)),
        ] {
            parse_log_query(&with_author(&text, "Ada", None));
        }
        assert!(start.elapsed().as_secs_f32() < 1.0);
    }

    #[test]
    fn an_unclosed_quote_before_a_clicked_author_doesnt_swallow_it() {
        assert_eq!(
            parse_log_query(&with_author("\"fix login", "Ada", None))
                .0
                .author,
            vec!["Ada"]
        );
    }

    #[test]
    fn a_name_holding_quotes_still_matches_its_own_commits() {
        for name in ["Kim \"KJ\" Lee", "O\"Neil"] {
            let author = parse_log_query(&with_author("", name, None))
                .0
                .author
                .remove(0);
            assert!(name.contains(&author), "{name} -> {author}");
        }
    }

    #[test]
    fn with_the_email_the_author_term_is_that_one_person() {
        assert_eq!(
            with_author("fix", "Ada Lovelace", Some("ada@example.com")),
            "fix author:\"Ada Lovelace <ada@example.com>\""
        );
        assert_eq!(
            parse_log_query(&with_author("", "Ada", Some("ada@example.com")))
                .0
                .author,
            vec!["Ada <ada@example.com>"]
        );
        assert_eq!(
            author_term("Kim \"KJ\" Lee", Some("kim@example.com")),
            "Lee <kim@example.com>"
        );
        assert_eq!(
            author_term("", Some("ada@example.com")),
            "<ada@example.com>"
        );
    }
}
