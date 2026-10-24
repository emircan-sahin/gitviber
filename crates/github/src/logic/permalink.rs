//! Links to code on GitHub that keep showing it: a file or its lines at a commit.

/// Percent-encodes a path segment as encodeURIComponent does.
fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A page on GitHub for `path` at commit `sha`: a file, its lines as `#L10-L24`, or a folder (`tree`).
pub fn permalink_url(
    web: &str,
    sha: &str,
    path: &str,
    tree: bool,
    lines: Option<(u32, u32)>,
) -> String {
    let at = if path.is_empty() {
        String::new()
    } else {
        format!(
            "/{}",
            path.split('/')
                .map(encode_segment)
                .collect::<Vec<_>>()
                .join("/")
        )
    };
    let anchor = match lines {
        Some((a, b)) if a == b => format!("#L{a}"),
        Some((a, b)) => format!("#L{a}-L{b}"),
        None => String::new(),
    };
    format!(
        "{web}/{}/{sha}{at}{anchor}",
        if tree { "tree" } else { "blob" }
    )
}

/// The repo's page from one of its commit pages (`…/commit/<sha>`).
pub fn repo_of_commit_url(url: &str) -> &str {
    url.rfind("/commit/").map_or(url, |at| &url[..at])
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEB: &str = "https://github.com/o/r";

    #[test]
    fn a_file_links_to_its_commit_with_the_lines_picked() {
        assert_eq!(
            permalink_url(WEB, "abc", "src/a.ts", false, None),
            format!("{WEB}/blob/abc/src/a.ts")
        );
        assert_eq!(
            permalink_url(WEB, "abc", "src/a.ts", false, Some((10, 24))),
            format!("{WEB}/blob/abc/src/a.ts#L10-L24")
        );
        assert_eq!(
            permalink_url(WEB, "abc", "src/a.ts", false, Some((7, 7))),
            format!("{WEB}/blob/abc/src/a.ts#L7")
        );
    }

    #[test]
    fn folders_are_trees_and_names_are_escaped_per_segment() {
        assert_eq!(
            permalink_url(WEB, "abc", "", true, None),
            format!("{WEB}/tree/abc")
        );
        assert_eq!(
            permalink_url(WEB, "abc", "app/[id] #1/p?.tsx", false, None),
            format!("{WEB}/blob/abc/app/%5Bid%5D%20%231/p%3F.tsx")
        );
        assert_eq!(
            permalink_url(WEB, "abc", "docs", true, None),
            format!("{WEB}/tree/abc/docs")
        );
    }

    #[test]
    fn a_commit_pages_repo() {
        assert_eq!(repo_of_commit_url(&format!("{WEB}/commit/abc")), WEB);
    }
}
