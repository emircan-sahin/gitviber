//! Link hints: a letter label on every link on a pane's screen (⌃⇧Space). Typing a label opens its
//! link, with Shift copies it; Esc, a scroll, a resize or a click ends them.

/// Home-row letters: no digits, as Shift+label copies.
pub const HINT_ALPHABET: &str = "sadfjklewcmpgh";

/// Prefix-free labels: label i is the i-th in breadth-first order (s, a, …, ss, sa, …) after
/// skipping enough short ones that no label starts another, so a label is done the moment it's
/// typed. The first are the shortest: they go to the bottom links, the likeliest pick.
pub fn hint_labels(n: usize, alphabet: &str) -> Vec<String> {
    let letters: Vec<char> = alphabet.chars().collect();
    let k = letters.len() as i64;
    let skip = (n.max(2) as i64 - 2) / (k - 1);
    (0..n as i64)
        .map(|i| {
            let mut label = Vec::new();
            let mut num = skip + i;
            while num >= 0 {
                label.push(letters[(num % k) as usize]);
                num = num / k - 1;
            }
            label.iter().rev().collect()
        })
        .collect()
}

/// What a key does while hints show.
#[derive(Debug, PartialEq, Eq)]
pub enum HintKey {
    /// Typed toward a label: draw again.
    Narrowed,
    /// A label was typed whole: the link at this index, copied rather than opened when Shift was
    /// held on one of its letters.
    Chosen { index: usize, copy: bool },
    /// The hints end.
    End,
    /// Nothing happens (a letter no label goes on with).
    Ignored,
}

/// The labels shown and what's been typed toward them.
#[derive(Debug, Default)]
pub struct Hints {
    pub labels: Vec<String>,
    pub typed: String,
    copy: bool,
}

impl Hints {
    /// For `n` links, the bottom ones getting the shortest labels.
    pub fn new(n: usize) -> Self {
        let mut labels = hint_labels(n, HINT_ALPHABET);
        labels.reverse();
        Hints {
            labels,
            typed: String::new(),
            copy: false,
        }
    }

    /// `key`: GPUI's name for it ("a", "escape", "backspace"); a letter is lowercase, `shift` apart.
    pub fn key(&mut self, key: &str, shift: bool) -> HintKey {
        match key {
            "escape" => return HintKey::End,
            "backspace" => {
                self.typed.pop();
                return HintKey::Narrowed;
            }
            _ => {}
        }
        let mut chars = key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return HintKey::Ignored;
        };
        let c = c.to_ascii_lowercase();
        let next = format!("{}{c}", self.typed);
        if !HINT_ALPHABET.contains(c) || !self.labels.iter().any(|l| l.starts_with(&next)) {
            return HintKey::Ignored;
        }
        self.typed = next;
        self.copy |= shift;
        match self.labels.iter().position(|l| *l == self.typed) {
            Some(index) => HintKey::Chosen {
                index,
                copy: self.copy,
            },
            None => HintKey::Narrowed,
        }
    }

    /// Whether link `i`'s label still goes with what's typed.
    pub fn shown(&self, i: usize) -> bool {
        self.labels
            .get(i)
            .is_some_and(|l| l.starts_with(&self.typed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_letter_while_they_last_then_two_none_the_start_of_another() {
        assert!(hint_labels(0, HINT_ALPHABET).is_empty());
        assert_eq!(hint_labels(1, HINT_ALPHABET), ["s"]);
        assert_eq!(hint_labels(3, "abc"), ["a", "b", "c"]);
        assert_eq!(hint_labels(5, "abc"), ["b", "c", "aa", "ab", "ac"]);
        assert_eq!(
            hint_labels(HINT_ALPHABET.len(), HINT_ALPHABET),
            HINT_ALPHABET.chars().map(String::from).collect::<Vec<_>>()
        );
        for n in [2, 13, 14, 15, 27, 100, 196, 197, 500] {
            let labels = hint_labels(n, HINT_ALPHABET);
            let set: std::collections::HashSet<_> = labels.iter().collect();
            assert_eq!(set.len(), n);
            for a in &labels {
                for b in &labels {
                    assert!(a == b || !b.starts_with(a.as_str()), "{n}: {a} starts {b}");
                }
            }
            assert!(
                labels
                    .iter()
                    .all(|l| l.chars().all(|c| HINT_ALPHABET.contains(c)))
            );
            assert!(labels.windows(2).all(|w| w[1].len() >= w[0].len()));
        }
        assert_eq!(hint_labels(196, HINT_ALPHABET).last().unwrap().len(), 2);
    }

    #[test]
    fn every_count_up_to_600_links_distinct_prefix_free_few_keys_home_row_only() {
        let letters: std::collections::HashSet<char> = HINT_ALPHABET.chars().collect();
        assert_eq!(letters.len(), HINT_ALPHABET.len(), "no letter twice");
        // Lower case: Shift is the copy key.
        assert!(HINT_ALPHABET.chars().all(|c| c.is_ascii_lowercase()));
        for n in 0..=600 {
            let labels = hint_labels(n, HINT_ALPHABET);
            assert_eq!(labels.len(), n);
            let set: std::collections::HashSet<&str> = labels.iter().map(String::as_str).collect();
            assert_eq!(set.len(), n, "{n}: distinct");
            for l in &labels {
                assert!(l.chars().all(|c| letters.contains(&c)), "{n}: {l}");
                for k in 1..l.len() {
                    assert!(!set.contains(&l[..k]), "{n}: {} starts {l}", &l[..k]);
                }
            }
            // Two keys reach 196 links, three the rest of a screen.
            let longest = labels.iter().map(String::len).max().unwrap_or(0);
            let most = match n {
                0..=1 => n,
                2..=14 => 1,
                15..=196 => 2,
                _ => 3,
            };
            assert!(longest <= most, "{n}: {longest} keys");
        }
        // 26 links: half of them one key.
        let labels = hint_labels(26, HINT_ALPHABET);
        assert_eq!(labels.iter().filter(|l| l.len() == 1).count(), 13);
    }

    #[test]
    fn typing_a_label_picks_its_link_and_shift_copies() {
        let mut h = Hints::new(20);
        let target = h.labels[3].clone();
        let mut chars = target.chars();
        let first = chars.next().unwrap().to_string();
        assert_eq!(h.key(&first, true), HintKey::Narrowed);
        assert!(
            !h.shown(
                h.labels
                    .iter()
                    .position(|l| !l.starts_with(&first))
                    .unwrap()
            )
        );
        let last = chars.next().unwrap().to_string();
        assert_eq!(
            h.key(&last, false),
            HintKey::Chosen {
                index: 3,
                copy: true
            }
        );
        let mut h = Hints::new(3);
        assert_eq!(h.key("z", false), HintKey::Ignored);
        assert_eq!(h.key("escape", false), HintKey::End);
    }
}
