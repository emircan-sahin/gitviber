//! A commit message as GitHub shows it: @mentions and #123 link to the repository's pages.

use std::ops::Range;

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The ref at `chars[i]` ('@' or '#'), as (end, href): @login (GitHub's username rules) or #123,
/// not inside a word (in any script), path or email address; #0 and #007 are no issue.
fn ref_at(chars: &[char], i: usize, repo: &str) -> Option<(usize, String)> {
    if i > 0 {
        let p = chars[i - 1];
        if word(p) || "@/.-".contains(p) {
            return None;
        }
    }
    let alnum = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    match chars[i] {
        '@' => {
            let mut end = i + 1;
            if !alnum(chars.get(end)) {
                return None;
            }
            end += 1;
            let mut len = 1;
            while len < 39 {
                match chars.get(end) {
                    Some(c) if c.is_ascii_alphanumeric() => end += 1,
                    Some('-') if alnum(chars.get(end + 1)) => end += 1,
                    _ => break,
                }
                len += 1;
            }
            if chars
                .get(end)
                .is_some_and(|c| word(*c) || *c == '/' || *c == '-')
            {
                return None;
            }
            let login: String = chars[i + 1..end].iter().collect();
            Some((end, format!("https://github.com/{login}")))
        }
        '#' => {
            let first = chars.get(i + 1)?;
            if !('1'..='9').contains(first) {
                return None;
            }
            let mut end = i + 2;
            while chars.get(end).is_some_and(char::is_ascii_digit) {
                end += 1;
            }
            if chars.get(end).is_some_and(|c| word(*c)) {
                return None;
            }
            let number: String = chars[i + 1..end].iter().collect();
            Some((end, format!("{repo}/issues/{number}")))
        }
        _ => None,
    }
}

/// The refs in `text` by byte range, with the page each opens (GitHub redirects a PR's number);
/// `repo` is https://github.com/owner/name.
pub fn github_refs(text: &str, repo: &str) -> Vec<(Range<usize>, String)> {
    let (bytes, chars): (Vec<usize>, Vec<char>) = text.char_indices().unzip();
    let byte = |i: usize| bytes.get(i).copied().unwrap_or(text.len());
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        match ref_at(&chars, i, repo) {
            Some((end, href)) => {
                out.push((byte(i)..byte(end), href));
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

/// The repository a commit's GitHub page is in: https://github.com/owner/name.
pub fn repo_of(commit_url: &str) -> Option<&str> {
    commit_url.split_once("/commit/").map(|(repo, _)| repo)
}

fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// A message body for a selectable text view: its lines and indents as written, refs linked
/// when `repo` is known.
pub fn body_html(body: &str, repo: Option<&str>) -> String {
    let body = body.trim_end();
    let mut out = String::with_capacity(body.len() + 16);
    // A parser drops one line feed right after <pre>: a body that starts with one keeps it.
    out.push_str("<pre>\n");
    let mut at = 0;
    for (range, href) in repo.map(|r| github_refs(body, r)).unwrap_or_default() {
        escape(&body[at..range.start], &mut out);
        out.push_str("<a href=\"");
        escape(&href, &mut out);
        out.push_str("\">");
        escape(&body[range.clone()], &mut out);
        out.push_str("</a>");
        at = range.end;
    }
    escape(&body[at..], &mut out);
    out.push_str("</pre>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "https://github.com/o/r";

    fn linked(text: &str) -> Vec<(&str, String)> {
        github_refs(text, REPO)
            .into_iter()
            .map(|(r, href)| (&text[r], href))
            .collect()
    }

    #[test]
    fn mentions_and_issue_numbers_link_but_not_inside_words_paths_or_emails() {
        assert_eq!(
            linked("Fix login (#12), thanks @octo; not x#3"),
            vec![
                ("#12", format!("{REPO}/issues/12")),
                ("@octo", "https://github.com/octo".to_string()),
            ]
        );
        assert!(linked("a@b.com, src/#4, #0, #007, @-x, @a_b").is_empty());
        // Byte ranges past text in other scripts.
        assert_eq!(
            linked("düzelt #7 için"),
            vec![("#7", format!("{REPO}/issues/7"))]
        );
    }

    #[test]
    fn the_body_keeps_its_lines_and_escapes_what_isnt_a_link() {
        assert_eq!(
            body_html("Closes #3\n  <b> & \"q\"\n\n", Some(REPO)),
            "<pre>\nCloses <a href=\"https://github.com/o/r/issues/3\">#3</a>\n  &lt;b&gt; &amp; &quot;q&quot;</pre>"
        );
        assert_eq!(body_html("Closes #3", None), "<pre>\nCloses #3</pre>");
        assert_eq!(
            repo_of("https://github.com/o/r/commit/abc"),
            Some("https://github.com/o/r")
        );
    }
}
