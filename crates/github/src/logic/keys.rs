//! Where the sidebar lists' keys move, as the old app's lists did: ↑/↓, Home/End, PgUp/PgDn.

/// Where a navigation key moves from row `i` of `n`, or None when it isn't one; `page` rows per
/// PgUp/PgDn.
pub fn move_target(key: &str, i: usize, n: usize, page: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let (i, n, page) = (i as isize, n as isize, page.max(1) as isize);
    let to = match key {
        "down" => i + 1,
        "up" => i - 1,
        "home" => 0,
        "end" => n - 1,
        "pagedown" => i + page,
        "pageup" => i - page,
        _ => return None,
    };
    Some(to.clamp(0, n - 1) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_move_within_the_list() {
        assert_eq!(move_target("down", 0, 3, 5), Some(1));
        assert_eq!(move_target("down", 2, 3, 5), Some(2));
        assert_eq!(move_target("up", 0, 3, 5), Some(0));
        assert_eq!(move_target("end", 0, 3, 5), Some(2));
        assert_eq!(move_target("home", 2, 3, 5), Some(0));
        assert_eq!(move_target("pagedown", 1, 30, 10), Some(11));
        assert_eq!(move_target("pageup", 4, 30, 10), Some(0));
        assert_eq!(move_target("a", 0, 3, 5), None);
        assert_eq!(move_target("down", 0, 0, 5), None);
    }
}
