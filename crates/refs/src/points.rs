//! A side of a comparison: HEAD, a branch, a remote branch, a tag or a commit, by full ref or id.
//! Pure, shared by the Compare screen and the pickers that open it.

use crate::names::{Branch, review_base, short_ref};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComparePoint {
    /// A full ref (refs/heads/…, refs/remotes/…, refs/tags/…), HEAD, or a commit id.
    pub r#ref: String,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointKind {
    Head,
    Branch,
    Remote,
    Tag,
    Commit,
}

/// HEAD itself: what a detached checkout compares as.
pub fn head_point() -> ComparePoint {
    ComparePoint {
        r#ref: "HEAD".into(),
        label: "HEAD".into(),
    }
}

pub fn branch_point(name: &str, remote: bool) -> ComparePoint {
    ComparePoint {
        r#ref: format!("refs/{}/{name}", if remote { "remotes" } else { "heads" }),
        label: name.into(),
    }
}

pub fn tag_point(name: &str) -> ComparePoint {
    ComparePoint {
        r#ref: format!("refs/tags/{name}"),
        label: name.into(),
    }
}

pub fn commit_point(sha: &str) -> ComparePoint {
    ComparePoint {
        r#ref: sha.into(),
        label: sha.chars().take(7).collect(),
    }
}

pub fn point_kind(r: &str) -> PointKind {
    if r == "HEAD" {
        PointKind::Head
    } else if r.starts_with("refs/heads/") {
        PointKind::Branch
    } else if r.starts_with("refs/remotes/") {
        PointKind::Remote
    } else if r.starts_with("refs/tags/") {
        PointKind::Tag
    } else {
        PointKind::Commit
    }
}

/// Where the Compare screen starts: origin's default branch (where a PR would go) against the
/// branch checked out.
pub fn default_points(branches: &[Branch], current: Option<&str>) -> (ComparePoint, ComparePoint) {
    let base = review_base(branches).map_or_else(head_point, |b| ComparePoint {
        label: short_ref(&b).to_string(),
        r#ref: b,
    });
    let head = current.map_or_else(head_point, |c| branch_point(c, false));
    (base, head)
}

fn encode(segment: &str) -> String {
    let mut out = String::new();
    for b in segment.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// GitHub's page for a comparison, as `base...head` (a PR's view) or `base..head`. Only what
/// GitHub knows by name: a branch, a tag or a commit, and a remote branch only of origin.
pub fn github_compare_url(
    web: Option<&str>,
    base: &ComparePoint,
    head: &ComparePoint,
    merge_base: bool,
) -> Option<String> {
    let name = |p: &ComparePoint| -> Option<String> {
        match point_kind(&p.r#ref) {
            PointKind::Head => None,
            PointKind::Remote => p
                .r#ref
                .strip_prefix("refs/remotes/origin/")
                .map(str::to_string),
            PointKind::Commit => Some(p.r#ref.clone()),
            _ => Some(short_ref(&p.r#ref).to_string()),
        }
    };
    let (a, b) = (name(base)?, name(head)?);
    let path = |n: &str| n.split('/').map(encode).collect::<Vec<_>>().join("/");
    Some(format!(
        "{}/compare/{}{}{}",
        web?,
        path(&a),
        if merge_base { "..." } else { ".." },
        path(&b)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn p(r: &str, label: &str) -> ComparePoint {
        ComparePoint {
            r#ref: r.into(),
            label: label.into(),
        }
    }

    #[test]
    fn points_name_branches_remote_branches_tags_and_commits() {
        assert_eq!(
            branch_point("feature/x", false),
            p("refs/heads/feature/x", "feature/x")
        );
        assert_eq!(
            branch_point("origin/main", true),
            p("refs/remotes/origin/main", "origin/main")
        );
        assert_eq!(tag_point("v1.0"), p("refs/tags/v1.0", "v1.0"));
        assert_eq!(commit_point(SHA), p(SHA, "0123456"));
        let kinds: Vec<PointKind> = [
            "HEAD",
            "refs/heads/a",
            "refs/remotes/o/a",
            "refs/tags/t",
            SHA,
        ]
        .iter()
        .map(|r| point_kind(r))
        .collect();
        assert_eq!(
            kinds,
            vec![
                PointKind::Head,
                PointKind::Branch,
                PointKind::Remote,
                PointKind::Tag,
                PointKind::Commit
            ]
        );
    }

    #[test]
    fn the_screen_starts_on_origins_default_branch_against_the_branch_checked_out() {
        let branches = vec![
            Branch {
                name: "main".into(),
                ..Default::default()
            },
            Branch {
                name: "origin/main".into(),
                remote: true,
                remote_default: true,
                ..Default::default()
            },
        ];
        assert_eq!(
            default_points(&branches, Some("fix")),
            (
                p("refs/remotes/origin/main", "origin/main"),
                p("refs/heads/fix", "fix")
            )
        );
        assert_eq!(default_points(&[], None), (head_point(), head_point()));
    }

    #[test]
    fn githubs_compare_page_needs_names_it_knows() {
        let web = Some("https://github.com/acme/app");
        let feature = branch_point("feature/x y", false);
        let main = branch_point("origin/main", true);
        assert_eq!(
            github_compare_url(web, &main, &feature, true).as_deref(),
            Some("https://github.com/acme/app/compare/main...feature/x%20y")
        );
        assert_eq!(
            github_compare_url(web, &main, &feature, false).as_deref(),
            Some("https://github.com/acme/app/compare/main..feature/x%20y")
        );
        assert_eq!(
            github_compare_url(web, &tag_point("v1"), &commit_point(SHA), true),
            Some(format!("https://github.com/acme/app/compare/v1...{SHA}"))
        );
        assert_eq!(
            github_compare_url(web, &branch_point("fork/main", true), &feature, true),
            None
        );
        assert_eq!(github_compare_url(web, &head_point(), &feature, true), None);
        assert_eq!(github_compare_url(None, &main, &feature, true), None);
    }
}
