//! GitHub's page of a file or folder at a commit: Copy GitHub Link / Open on GitHub, in a diff
//! and in the files crate's explorer and code view.

/// GitHub's page of `path` at `sha`: a file's blob (at `lines`, `#L10-L24`) or a folder's tree.
pub fn permalink_url(
    web: &str,
    sha: &str,
    path: &str,
    tree: bool,
    lines: Option<(u32, u32)>,
) -> String {
    // Each segment percent-encoded as JavaScript's encodeURIComponent does.
    let at: String = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|seg| {
            let mut out = String::from("/");
            for b in seg.bytes() {
                if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
                    out.push(b as char);
                } else {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
            out
        })
        .collect();
    let anchor = match lines {
        Some((a, b)) if a == b => format!("#L{a}"),
        Some((a, b)) => format!("#L{a}-L{b}"),
        None => String::new(),
    };
    let kind = if tree { "tree" } else { "blob" };
    format!("{web}/{kind}/{sha}{at}{anchor}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEB: &str = "https://github.com/o/r";

    #[test]
    fn links_to_a_file_its_lines_or_a_folder() {
        assert_eq!(
            permalink_url(WEB, "abc", "src/a b.rs", false, Some((3, 9))),
            "https://github.com/o/r/blob/abc/src/a%20b.rs#L3-L9"
        );
        assert_eq!(
            permalink_url(WEB, "abc", "x.rs", false, Some((4, 4))),
            "https://github.com/o/r/blob/abc/x.rs#L4"
        );
        assert_eq!(
            permalink_url(WEB, "abc", "d", true, None),
            "https://github.com/o/r/tree/abc/d"
        );
        // The repo's root folder.
        assert_eq!(
            permalink_url(WEB, "abc", "", true, None),
            "https://github.com/o/r/tree/abc"
        );
    }
}
