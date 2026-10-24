//! How Obsidian finds the file a link names. Paths are vault-relative with "/" between folders.

use std::collections::HashMap;

/// A name as links compare it: without regard to case or Unicode form ("Cafe\u{301}" is "Café").
/// Canonical decomposition of Latin, Greek, Cyrillic and Hangul, then lowercase: both sides of a
/// comparison go through it, so it needn't be NFC.
pub fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let push = |c: char, out: &mut String| out.extend(c.to_lowercase());
        let cp = c as u32;
        // Hangul syllables decompose by arithmetic (Unicode §3.12).
        if (0xAC00..0xAC00 + 11172).contains(&cp) {
            let s = cp - 0xAC00;
            let jamo = [0x1100 + s / 588, 0x1161 + (s % 588) / 28, 0x11A7 + s % 28];
            for (i, j) in jamo.into_iter().enumerate() {
                if i < 2 || j != 0x11A7 {
                    out.extend(char::from_u32(j));
                }
            }
            continue;
        }
        match crate::nfd::NFD.binary_search_by_key(&cp, |(k, _)| *k) {
            Ok(i) => crate::nfd::NFD[i].1.chars().for_each(|d| push(d, &mut out)),
            Err(_) => push(c, &mut out),
        }
    }
    out
}

fn dir(p: &str) -> &str {
    p.rfind('/').map_or("", |i| &p[..i])
}

fn base(p: &str) -> &str {
    p.rfind('/').map_or(p, |i| &p[i + 1..])
}

/// A vault's files, by lowercased name with and without ".md": a link's last part looks its
/// file up there.
#[derive(Debug, Default)]
pub struct LinkIndex {
    files: Vec<String>,
    by_name: HashMap<String, Vec<u32>>,
}

impl LinkIndex {
    pub fn new(files: Vec<String>) -> Self {
        let mut by_name: HashMap<String, Vec<u32>> = HashMap::with_capacity(files.len());
        for (i, file) in files.iter().enumerate() {
            let name = norm(base(file));
            if let Some(stem) = name.strip_suffix(".md") {
                by_name.entry(stem.to_string()).or_default().push(i as u32);
            }
            by_name.entry(name).or_default().push(i as u32);
        }
        LinkIndex { files, by_name }
    }

    pub fn files(&self) -> &[String] {
        &self.files
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// `path` is one of the vault's files, exactly as written.
    pub fn contains(&self, path: &str) -> bool {
        self.candidates(&norm(base(path))).any(|f| f == path)
    }

    fn candidates<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a str> + 'a {
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .map(|&i| self.files[i as usize].as_str())
    }

    /// The file a [[link]] or a markdown link's path names, written in the note at `from`, or
    /// None when the vault has none. As Obsidian: names match without regard to case (or
    /// Unicode form), ".md" may be left off, and a name alone finds the file anywhere in the
    /// vault. Of several that match, the one at that very path from the vault's top wins, then
    /// one in `from`'s folder, then the shortest path.
    pub fn resolve(&self, link: &str, from: &str) -> Option<String> {
        let mut want = link.trim().replace('\\', "/");
        if want.is_empty() {
            return Some(from.to_string());
        }
        if want.starts_with("./") || want.starts_with("../") {
            want = relative(&want, from);
        }
        let want = norm(want.trim_start_matches('/'));
        let suffix = format!("/{want}");
        let matches = |n: &str| n == want || n.ends_with(&suffix);
        let candidates: Vec<&str> = self
            .candidates(base(&want))
            .filter(|f| {
                let n = norm(f);
                matches(&n) || n.strip_suffix(".md").is_some_and(matches)
            })
            .collect();
        if candidates.is_empty() {
            return None;
        }
        // The note before a file without an extension of the same name: [[Plan]] is Plan.md.
        let note = format!("{want}.md");
        let exact = candidates
            .iter()
            .find(|f| norm(f) == note)
            .or_else(|| candidates.iter().find(|f| norm(f) == want));
        if let Some(f) = exact {
            return Some(f.to_string());
        }
        if let Some(f) = candidates.iter().find(|f| dir(f) == dir(from)) {
            return Some(f.to_string());
        }
        candidates
            .into_iter()
            .min_by(|a, b| {
                let depth = |p: &str| p.matches('/').count();
                depth(a)
                    .cmp(&depth(b))
                    .then(a.len().cmp(&b.len()))
                    .then(a.cmp(b))
            })
            .map(str::to_string)
    }
}

/// `link` resolved against the note at `from`: a relative path (./, ../) as written. Climbing
/// past the vault's top stays at the top.
fn relative(link: &str, from: &str) -> String {
    let mut parts: Vec<&str> = dir(from).split('/').filter(|s| !s.is_empty()).collect();
    for seg in link.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "" | "." => {}
            seg => parts.push(seg),
        }
    }
    parts.join("/")
}

/// A file's name as Obsidian shows it: a note without its ".md".
pub fn note_name(path: &str) -> &str {
    let name = base(path);
    match name.len().checked_sub(3) {
        Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".md") => {
            &name[..at]
        }
        _ => name,
    }
}

/// The target of a [[link]] or a markdown link's href, as path and anchor (heading or ^block),
/// split at the first '#'.
pub fn split_target(target: &str) -> (&str, &str) {
    match target.find('#') {
        Some(i) => (&target[..i], &target[i + 1..]),
        None => (target, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn index(files: &[&str]) -> LinkIndex {
        LinkIndex::new(files.iter().map(|s| s.to_string()).collect())
    }

    const FILES: [&str; 10] = [
        "Home.md",
        "Projects/Alpha.md",
        "Projects/Beta/Plan.md",
        "Archive/Plan.md",
        "Plan.md",
        "Inbox/Plan.md",
        "assets/diagram.png",
        "Daily/2026-01-02.md",
        "Café.md",
        "Notes/Note.v2.md",
    ];

    fn r(i: &LinkIndex, link: &str, from: &str) -> Option<String> {
        i.resolve(link, from)
    }

    #[test]
    fn a_name_finds_its_note_anywhere_without_md_and_regardless_of_case() {
        let i = index(&FILES);
        assert_eq!(
            r(&i, "alpha", "Home.md").as_deref(),
            Some("Projects/Alpha.md")
        );
        assert_eq!(
            r(&i, "Projects/ALPHA.md", "Home.md").as_deref(),
            Some("Projects/Alpha.md")
        );
        assert_eq!(
            r(&i, "diagram.png", "Daily/2026-01-02.md").as_deref(),
            Some("assets/diagram.png")
        );
        assert_eq!(
            r(&i, "Note.v2", "Home.md").as_deref(),
            Some("Notes/Note.v2.md")
        );
        assert_eq!(r(&i, "Missing", "Home.md"), None);
        assert_eq!(
            r(&i, "", "Daily/2026-01-02.md").as_deref(),
            Some("Daily/2026-01-02.md")
        );
    }

    #[test]
    fn of_several_that_match_the_path_from_the_top_then_the_same_folder_then_the_shortest() {
        let i = index(&FILES);
        assert_eq!(
            r(&i, "Plan", "Projects/Alpha.md").as_deref(),
            Some("Plan.md")
        );
        assert_eq!(
            r(&i, "Archive/Plan", "Home.md").as_deref(),
            Some("Archive/Plan.md")
        );
        assert_eq!(
            r(&i, "Beta/Plan", "Home.md").as_deref(),
            Some("Projects/Beta/Plan.md")
        );
        let no_root: Vec<&str> = FILES.iter().copied().filter(|f| *f != "Plan.md").collect();
        let i = index(&no_root);
        assert_eq!(
            r(&i, "Plan", "Archive/Other.md").as_deref(),
            Some("Archive/Plan.md")
        );
        assert_eq!(r(&i, "Plan", "Home.md").as_deref(), Some("Inbox/Plan.md"));
    }

    #[test]
    fn a_note_wins_over_a_file_without_an_extension_of_the_same_name() {
        let i = index(&["Plan", "Plan.md"]);
        assert_eq!(r(&i, "Plan", "Home.md").as_deref(), Some("Plan.md"));
        assert_eq!(r(&i, "Plan.md", "Home.md").as_deref(), Some("Plan.md"));
    }

    #[test]
    fn relative_paths_and_unicode_forms() {
        let i = index(&FILES);
        assert_eq!(
            r(&i, "../Home", "Projects/Alpha.md").as_deref(),
            Some("Home.md")
        );
        assert_eq!(
            r(&i, "./Beta/Plan.md", "Projects/Alpha.md").as_deref(),
            Some("Projects/Beta/Plan.md")
        );
        assert_eq!(r(&i, "Cafe\u{301}", "Home.md").as_deref(), Some("Café.md"));
        // The other way round too: a name on disk in NFD, a link typed in NFC.
        let nfd = index(&["Cafe\u{301}.md", "한글.md"]);
        assert_eq!(
            r(&nfd, "Café", "Home.md").as_deref(),
            Some("Cafe\u{301}.md")
        );
        let hangul_nfd = "\u{1112}\u{1161}\u{11ab}\u{1100}\u{1173}\u{11af}";
        assert_eq!(r(&nfd, hangul_nfd, "Home.md").as_deref(), Some("한글.md"));
    }

    #[test]
    fn targets_split_at_the_first_hash() {
        assert_eq!(split_target("Note#Part#Sub"), ("Note", "Part#Sub"));
        assert_eq!(split_target("#^block"), ("", "^block"));
        assert_eq!(note_name("Daily/Today.MD"), "Today");
        assert_eq!(note_name("a/b.png"), "b.png");
        assert_eq!(note_name("é.md"), "é");
    }

    #[test]
    fn a_20000_note_vault_indexes_and_resolves_every_link_quickly() {
        let files: Vec<String> = (0..20000)
            .map(|i| format!("Area {}/Topic {}/Note {i}.md", i % 40, i % 400))
            .collect();
        let start = Instant::now();
        let i = LinkIndex::new(files.clone());
        for (n, file) in files.iter().enumerate() {
            assert_eq!(
                i.resolve(&format!("Note {n}"), "Home.md").as_ref(),
                Some(file)
            );
        }
        let took = start.elapsed();
        // A debug build; the old app's limit was 1s in node.
        assert!(took.as_millis() < 3000, "{took:?}");
    }

    #[test]
    fn odd_names() {
        let i = index(&[
            "Notes/C# basics.md",
            "Notes/x^y.md",
            "🚀 Launch.md",
            "My Note.md",
            "Résumé.md",
            "Öğrenci Notları/İstanbul.md",
            "It's $5.md",
        ]);
        assert_eq!(
            r(&i, "C# basics", "Home.md").as_deref(),
            Some("Notes/C# basics.md")
        );
        assert_eq!(r(&i, "x^y", "Home.md").as_deref(), Some("Notes/x^y.md"));
        assert_eq!(
            r(&i, "🚀 launch", "Home.md").as_deref(),
            Some("🚀 Launch.md")
        );
        assert_eq!(
            r(&i, "  My Note  ", "Home.md").as_deref(),
            Some("My Note.md")
        );
        assert_eq!(r(&i, "Résumé", "Home.md").as_deref(), Some("Résumé.md"));
        assert_eq!(
            r(&i, "Öğrenci Notları/İstanbul", "Home.md").as_deref(),
            Some("Öğrenci Notları/İstanbul.md")
        );
        assert_eq!(r(&i, "It's $5", "Home.md").as_deref(), Some("It's $5.md"));
        assert!(i.contains("Notes/C# basics.md") && !i.contains("notes/c# basics.md"));
    }

    #[test]
    fn a_link_cant_climb_out_of_the_vault_or_name_a_folder() {
        let i = index(&["Home.md", "a/b.md"]);
        assert_eq!(r(&i, "../../../etc/passwd", "a/b.md"), None);
        assert_eq!(r(&i, "../../Home", "a/b.md").as_deref(), Some("Home.md"));
        assert_eq!(r(&i, "/Home", "a/b.md").as_deref(), Some("Home.md"));
        assert_eq!(r(&i, "a", "Home.md"), None);
        assert_eq!(r(&i, "..", "a/b.md"), None);
    }

    #[test]
    fn the_same_name_in_many_folders() {
        let files: Vec<String> = (0..2000)
            .map(|i| format!("Docs/Section {i}/index.md"))
            .collect();
        let i = LinkIndex::new(files.clone());
        assert_eq!(
            r(&i, "index", "Docs/Section 7/Intro.md").as_deref(),
            Some("Docs/Section 7/index.md")
        );
        assert_eq!(
            r(&i, "index", "Elsewhere/x.md").as_deref(),
            Some("Docs/Section 0/index.md")
        );
        assert_eq!(
            r(&i, "Section 1999/index", "Elsewhere/x.md").as_deref(),
            Some("Docs/Section 1999/index.md")
        );
        let mut with_root = files;
        with_root.push("index.md".into());
        let i = LinkIndex::new(with_root);
        assert_eq!(
            r(&i, "index", "Docs/Section 7/Intro.md").as_deref(),
            Some("index.md")
        );
    }
}
