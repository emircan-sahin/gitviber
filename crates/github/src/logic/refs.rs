//! A name as git takes it for a branch.

/// Characters git refuses anywhere in a ref name: controls and space, DEL, ~ ^ : ? * [ \ | " < >.
fn bad(c: char) -> bool {
    (c as u32) <= 0x20 || c == '\x7f' || "~^:?*[\\|\"<>".contains(c)
}

/// A typed branch name as git will take it: "fix login bug" → "fix-login-bug". Each run git
/// refuses becomes one "-": bad characters, "@{", "..", a component starting with a dot, a
/// trailing dot or "/", a component ending in ".lock"; then "//", a leading - + . or /, and HEAD.
pub fn sanitized_ref_name(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let n = chars.len();
    let at = |i: usize| chars.get(i).copied();
    let mut out = String::with_capacity(name.len());
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if bad(c) {
            while i < n && bad(chars[i]) {
                i += 1;
            }
            out.push('-');
            continue;
        }
        if c == '@' && at(i + 1) == Some('{') {
            out.push('-');
            i += 2;
            continue;
        }
        if c == '.' {
            if at(i + 1) == Some('.') {
                while at(i) == Some('.') {
                    i += 1;
                }
                out.push('-');
                continue;
            }
            let lock = chars[i..].starts_with(&['.', 'l', 'o', 'c', 'k'])
                && matches!(at(i + 5), None | Some('/'));
            if i == 0 || chars[i - 1] == '/' || i == n - 1 {
                out.push('-');
                i += 1;
                continue;
            }
            if lock {
                out.push('-');
                i += 5;
                continue;
            }
        }
        if c == '/' && i == n - 1 {
            out.push('-');
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    let mut collapsed = String::with_capacity(out.len());
    for c in out.chars() {
        if !(c == '/' && collapsed.ends_with('/')) {
            collapsed.push(c);
        }
    }
    let s = collapsed
        .trim_start_matches(['-', '+', '.', '/'])
        .to_string();
    if s == "HEAD" || s == "@" {
        format!("{s}-")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_git_refuses_become_ones_it_takes() {
        assert_eq!(sanitized_ref_name("fix login bug"), "fix-login-bug");
        assert_eq!(sanitized_ref_name("a..b"), "a-b");
        assert_eq!(sanitized_ref_name("feat/.hidden"), "feat/-hidden");
        assert_eq!(sanitized_ref_name("x.lock"), "x-");
        assert_eq!(sanitized_ref_name("x.lock/y"), "x-/y");
        assert_eq!(sanitized_ref_name("a//b/"), "a/b-");
        assert_eq!(sanitized_ref_name("--+./name."), "name-");
        assert_eq!(sanitized_ref_name("HEAD"), "HEAD-");
        assert_eq!(sanitized_ref_name("a@{b"), "a-b");
        assert_eq!(sanitized_ref_name("why?*now"), "why-now");
    }
}
