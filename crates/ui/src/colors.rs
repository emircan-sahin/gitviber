//! The named colors projects and worktrees are told apart by, and how one is given unasked: a
//! hash of a path or name picks it, then neighbours are spread to the least used.

/// A named color, in a shade for each theme; a letter on it is whichever of near-black and
/// white reads better.
#[derive(Debug, PartialEq, Eq)]
pub struct Swatch {
    pub key: &'static str,
    pub name: &'static str,
    dark: u32,
    light: u32,
    /// Given out unasked; gray is only picked by hand, since a dimmed tile means a missing folder.
    pub(crate) auto: bool,
}

const fn sw(key: &'static str, name: &'static str, dark: u32, light: u32) -> Swatch {
    Swatch {
        key,
        name,
        dark,
        light,
        auto: true,
    }
}

/// Bright enough to stand out on the dark themes, deep enough on the light ones.
pub static SWATCHES: [Swatch; 13] = [
    sw("red", "Red", 0xF87171, 0xDC2626),
    sw("orange", "Orange", 0xFB923C, 0xEA580C),
    sw("amber", "Amber", 0xFBBF24, 0xD97706),
    sw("lime", "Lime", 0xA3E635, 0x65A30D),
    sw("green", "Green", 0x4ADE80, 0x16A34A),
    sw("teal", "Teal", 0x2DD4BF, 0x0D9488),
    sw("cyan", "Cyan", 0x22D3EE, 0x0891B2),
    sw("blue", "Blue", 0x60A5FA, 0x2563EB),
    sw("indigo", "Indigo", 0x818CF8, 0x4F46E5),
    sw("violet", "Violet", 0xA78BFA, 0x7C3AED),
    sw("fuchsia", "Fuchsia", 0xE879F9, 0xC026D3),
    sw("pink", "Pink", 0xF472B6, 0xDB2777),
    Swatch {
        auto: false,
        ..sw("gray", "Gray", 0x94A3B8, 0x64748B)
    },
];

const NEAR_BLACK: u32 = 0x111111;
const WHITE: u32 = 0xFFFFFF;

impl Swatch {
    pub fn fill(&self, dark: bool) -> u32 {
        if dark { self.dark } else { self.light }
    }

    pub fn letter(&self, dark: bool) -> u32 {
        let fill = rgb(self.fill(dark));
        if crate::theme::contrast(fill, rgb(NEAR_BLACK)) >= crate::theme::contrast(fill, rgb(WHITE))
        {
            NEAR_BLACK
        } else {
            WHITE
        }
    }
}

pub fn swatch(key: &str) -> Option<&'static Swatch> {
    SWATCHES.iter().find(|s| s.key == key)
}

/// 0xRRGGBB as sRGB channels in 0..=1.
fn rgb(c: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((c >> shift) & 0xff) as f32 / 255.)
}

/// FNV-1a over UTF-16 units, as the old app hashed names: std's hasher may change between Rust
/// releases, and a color given by a name must not.
pub fn stable_hash(s: &str) -> u32 {
    s.encode_utf16().fold(0x811c_9dc5, |h: u32, unit| {
        (h ^ u32::from(unit)).wrapping_mul(0x0100_0193)
    })
}

/// Of `n` colors, the least used (`uses`) going round from `start`; the first such on a tie, so
/// `start` itself while it's no more used than the rest.
pub fn spread(start: usize, n: usize, uses: impl Fn(usize) -> usize) -> usize {
    (0..n)
        .map(|i| (start + i) % n)
        .min_by_key(|&c| uses(c))
        .unwrap_or(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_never_changes_between_builds() {
        assert_eq!(stable_hash(""), 0x811c_9dc5);
        // FNV-1a of "a": a fixed vector, so a color given by a name stays.
        assert_eq!(stable_hash("a"), 0xe40c_292c);
        // Non-ASCII hashes by UTF-16 unit, not by byte.
        assert_eq!(
            stable_hash("é"),
            (0x811c_9dc5u32 ^ 0xe9).wrapping_mul(0x0100_0193)
        );
    }

    #[test]
    fn spreading_takes_the_least_used_from_the_start() {
        let used = [2, 0, 1, 0];
        assert_eq!(spread(1, 4, |c| used[c]), 1);
        assert_eq!(spread(2, 4, |c| used[c]), 3);
        assert_eq!(spread(0, 4, |c| used[c]), 1);
        assert_eq!(spread(0, 4, |_| 0), 0);
    }

    #[test]
    fn every_letter_reads_on_its_tile() {
        for s in &SWATCHES {
            for dark in [true, false] {
                let c = crate::theme::contrast(rgb(s.fill(dark)), rgb(s.letter(dark)));
                assert!(c >= 4.5, "{} ({dark}): {c}", s.key);
            }
        }
    }
}
