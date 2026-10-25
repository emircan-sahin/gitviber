//! Starting work on an issue: the branch it proposes, the Run command's {issue}, and the closing
//! line a PR from that branch gets.

use super::refs::sanitized_ref_name;

const SLUG: usize = 40;

/// Letters NFKD leaves whole: Turkish's dotless ı among them, which would otherwise stay while
/// its neighbors lose their marks.
fn plain(c: char) -> Option<&'static str> {
    Some(match c {
        'ı' => "i",
        'ł' => "l",
        'ø' => "o",
        'đ' | 'ð' => "d",
        'þ' => "th",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        _ => return None,
    })
}

/// The combining marks NFKD splits off letters, the ones a title is likely to hold.
fn is_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x064B..=0x065F | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0x3099..=0x309A | 0xFE20..=0xFE2F)
}

/// `12-short-title`, as GitHub's own "Create a branch" names one: lowercase, cut at a word.
pub fn issue_branch_name(number: u64, title: &str) -> String {
    let decomposed = icu_normalizer::DecomposingNormalizerBorrowed::new_nfkd().normalize(title);
    let lower: String = decomposed
        .chars()
        .filter(|c| !is_mark(*c))
        .collect::<String>()
        .to_lowercase();
    let mut slug = String::new();
    let mut gap = false;
    for c in lower.chars() {
        if c == '\'' || c == '’' {
            continue;
        }
        let piece = plain(c)
            .map(str::to_string)
            .unwrap_or_else(|| c.to_string());
        if c.is_alphanumeric() {
            if gap && !slug.is_empty() {
                slug.push('-');
            }
            gap = false;
            slug.push_str(&piece);
        } else {
            gap = true;
        }
    }
    // By character: cutting bytes could leave half a letter, which no ref name may hold.
    let chars: Vec<char> = slug.chars().collect();
    if chars.len() > SLUG {
        let cut: String = chars[..=SLUG].iter().collect();
        slug = match cut.rfind('-') {
            Some(at) => cut[..at].to_string(),
            None => chars[..SLUG].iter().collect(),
        };
    }
    sanitized_ref_name(&if slug.is_empty() {
        format!("issue-{number}")
    } else {
        format!("{number}-{slug}")
    })
}

/// The Run command with `{issue}` as the issue's number: `claude "Fix #{issue}"`.
pub fn with_issue(command: &str, number: u64) -> String {
    command.replace("{issue}", &number.to_string())
}

/// owner/name and the number of an issue's page, https://github.com/owner/name/issues/12.
fn issue_ref(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/');
    let (owner, name, kind, number) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || kind != "issues" || owner.is_empty() || name.is_empty() {
        return None;
    }
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let repo = &rest[..owner.len() + 1 + name.len()];
    Some((repo, number))
}

/// `Closes #12` for an issue in the repository the PR goes to (owner/name), else `Closes owner/name#12`.
pub fn closing_line(issue_url: &str, pull_repo: &str) -> Option<String> {
    let (repo, number) = issue_ref(issue_url)?;
    let prefix = if repo.eq_ignore_ascii_case(pull_repo) {
        ""
    } else {
        repo
    };
    Some(format!("Closes {prefix}#{number}"))
}

/// Whether `body` closes the issue already: one of GitHub's keywords, an optional colon, then one
/// of `refs`, not followed by a word character or a slash.
fn closes(body: &str, refs: &[String]) -> bool {
    const KEYWORDS: [&str; 9] = [
        "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
    ];
    let lower = body.to_lowercase();
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let bytes = lower.as_bytes();
    let skip_space = |at: &mut usize| {
        let from = *at;
        while *at < bytes.len() && bytes[*at].is_ascii_whitespace() {
            *at += 1;
        }
        *at > from
    };
    for (start, _) in lower.char_indices() {
        if start > 0 && lower[..start].chars().next_back().is_some_and(word) {
            continue;
        }
        for kw in KEYWORDS {
            if !lower[start..].starts_with(kw) {
                continue;
            }
            let mut at = start + kw.len();
            if lower[at..].chars().next().is_some_and(word) {
                continue;
            }
            let before = skip_space(&mut at);
            let colon = bytes.get(at) == Some(&b':');
            if colon {
                at += 1;
            }
            let after = skip_space(&mut at);
            // `fix #12` and `fix: #12` are one; `fix:#12` isn't.
            if !(if colon { after } else { before }) {
                continue;
            }
            for r in refs {
                let r = r.to_lowercase();
                if lower[at..].starts_with(&r)
                    && !lower[at + r.len()..]
                        .chars()
                        .next()
                        .is_some_and(|c| word(c) || c == '/')
                {
                    return true;
                }
            }
        }
    }
    false
}

/// `body` with the closing line for `issue_url` after it, unless it already closes that issue:
/// any of GitHub's keywords before `#12` (in the PR's own repository), `owner/name#12` or the url.
pub fn with_closing(body: &str, issue_url: Option<&str>, pull_repo: &str) -> String {
    let Some((url, line)) = issue_url.and_then(|u| Some((u, closing_line(u, pull_repo)?))) else {
        return body.to_string();
    };
    let (repo, number) = issue_ref(url).unwrap_or_default();
    let mut refs = vec![format!("{repo}#{number}"), url.to_string()];
    if repo.eq_ignore_ascii_case(pull_repo) {
        refs.push(format!("#{number}"));
    }
    if closes(body, &refs) {
        return body.to_string();
    }
    if body.trim().is_empty() {
        line
    } else {
        format!("{}\n\n{line}", body.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issues_branch_is_its_number_and_title_cut_at_a_word() {
        assert_eq!(
            issue_branch_name(12, "Fix the login bug!"),
            "12-fix-the-login-bug"
        );
        assert_eq!(
            issue_branch_name(
                7,
                "Worktree picker: show each worktree's pull request and CI state"
            ),
            "7-worktree-picker-show-each-worktrees-pull"
        );
        assert_eq!(
            issue_branch_name(3, "Türkçe başlık, İstanbul'da"),
            "3-turkce-baslik-istanbulda"
        );
        // U+20000, a letter outside the BMP, kept whole.
        assert_eq!(
            issue_branch_name(6, &format!("{}\u{20000}\u{20000}", "a".repeat(39))),
            format!("6-{}\u{20000}", "a".repeat(39))
        );
        assert_eq!(issue_branch_name(4, "  ...  "), "issue-4");
        assert_eq!(
            issue_branch_name(5, &"a".repeat(60)),
            format!("5-{}", "a".repeat(40))
        );
    }

    #[test]
    fn issue_in_the_run_command_is_the_number() {
        assert_eq!(
            with_issue("claude \"Fix #{issue}\"", 12),
            "claude \"Fix #12\""
        );
        assert_eq!(with_issue("claude", 12), "claude");
    }

    #[test]
    fn the_closing_line_names_the_repository_only_when_the_pr_goes_elsewhere() {
        assert_eq!(
            closing_line("https://github.com/o/r/issues/12", "O/R").as_deref(),
            Some("Closes #12")
        );
        assert_eq!(
            closing_line("https://github.com/up/r/issues/12", "me/r").as_deref(),
            Some("Closes up/r#12")
        );
        assert_eq!(closing_line("https://github.com/o/r/pull/12", "o/r"), None);
    }

    #[test]
    fn the_closing_line_goes_after_the_body_once() {
        let issue = Some("https://github.com/o/r/issues/12");
        assert_eq!(with_closing("", issue, "o/r"), "Closes #12");
        assert_eq!(with_closing("Body\n", issue, "o/r"), "Body\n\nCloses #12");
        assert_eq!(with_closing("Fixes #12 too", issue, "o/r"), "Fixes #12 too");
        assert_eq!(
            with_closing("Fixes #123", issue, "o/r"),
            "Fixes #123\n\nCloses #12"
        );
        assert_eq!(with_closing("fixes O/R#12", issue, "o/r"), "fixes O/R#12");
        assert_eq!(
            with_closing("Resolves https://github.com/o/r/issues/12", issue, "o/r"),
            "Resolves https://github.com/o/r/issues/12"
        );
        assert_eq!(with_closing("Body", None, "o/r"), "Body");
    }

    #[test]
    fn into_another_repository_only_a_named_reference_closes_it() {
        let issue = Some("https://github.com/up/r/issues/12");
        assert_eq!(
            with_closing("resolves up/r#12", issue, "me/r"),
            "resolves up/r#12"
        );
        assert_eq!(
            with_closing("Fixes #12", issue, "me/r"),
            "Fixes #12\n\nCloses up/r#12"
        );
    }
}
