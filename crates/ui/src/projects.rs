//! How the saved projects are arranged: pinned ones on top, named folders, the rest by last
//! opened or in the user's own order, and each project's tile color. `UiState::recent_projects`
//! stays the list of projects, the latest opened first; this only arranges it, so a path it
//! names that isn't in that list shows nowhere.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::state::{lenient, lenient_list};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectSort {
    /// Last opened on top.
    #[default]
    Recent,
    /// Where the user dragged them; one not placed yet (just opened) shows on top.
    Manual,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProjectFolder {
    #[serde(deserialize_with = "lenient")]
    pub id: u64,
    #[serde(deserialize_with = "lenient")]
    pub name: String,
    #[serde(deserialize_with = "lenient")]
    pub collapsed: bool,
    #[serde(deserialize_with = "lenient_list")]
    pub projects: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProjectOrg {
    #[serde(deserialize_with = "lenient")]
    pub sort: ProjectSort,
    /// Every project in the user's order, kept while Recent shows so switching back restores it.
    #[serde(deserialize_with = "lenient_list")]
    pub manual: Vec<String>,
    #[serde(deserialize_with = "lenient_list")]
    pub pinned: Vec<String>,
    #[serde(deserialize_with = "lenient_list")]
    pub folders: Vec<ProjectFolder>,
    /// Path → swatch key. Picked by the user or given once when the project was first seen, so
    /// a project keeps its color as others come and go.
    #[serde(deserialize_with = "lenient")]
    pub colors: BTreeMap<String, String>,
}

/// Where a project sits in the list. A project is in one place: pinning takes it out of its
/// folder, and moving it into one unpins it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Pinned,
    Folder(u64),
    Loose,
}

/// What a dragged project was dropped on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropOn {
    /// Another project's row: it goes into that row's place, just above it.
    Before(String),
    /// A section's header or a folder's row: it goes to the end of that place.
    Into(Place),
}

/// A line of the arranged list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Header(Section),
    Folder {
        id: u64,
        name: String,
        open: bool,
        count: usize,
    },
    Project {
        path: String,
        place: Place,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Pinned,
    /// The projects in no folder, headed only when there are pins or folders above them.
    Projects,
}

impl Line {
    /// A row the keys move over and act on; headers aren't.
    pub fn is_row(&self) -> bool {
        !matches!(self, Line::Header(_))
    }
}

pub fn project_name(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

/// `path` with the home folder written as ~, as a shell shows it.
pub fn home_relative(path: &str, home: Option<&str>) -> String {
    let home = home
        .map(|h| h.trim_end_matches('/'))
        .filter(|h| !h.is_empty());
    match home.and_then(|h| path.strip_prefix(h)) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

impl ProjectOrg {
    pub fn place_of(&self, path: &str) -> Place {
        if self.pinned.iter().any(|p| p == path) {
            return Place::Pinned;
        }
        self.folders
            .iter()
            .find(|f| f.projects.iter().any(|p| p == path))
            .map_or(Place::Loose, |f| Place::Folder(f.id))
    }

    /// Pinned or in a folder: the user put it there, so it stays in the list however many
    /// projects are opened after it.
    pub fn organized(&self, path: &str) -> bool {
        self.place_of(path) != Place::Loose
    }

    /// `recent` (latest opened first) in the order chosen.
    pub fn order(&self, recent: &[String]) -> Vec<String> {
        let mut out = recent.to_vec();
        if self.sort == ProjectSort::Manual {
            let at = |p: &String| self.manual.iter().position(|m| m == p);
            // Stable: ones not placed yet keep their recency among themselves, above the rest.
            out.sort_by_key(|p| at(p).map_or((0, 0), |i| (1, i)));
        }
        out
    }

    pub fn move_to(&mut self, path: &str, place: Place) {
        self.pinned.retain(|p| p != path);
        for f in &mut self.folders {
            f.projects.retain(|p| p != path);
        }
        match place {
            Place::Pinned => self.pinned.push(path.to_string()),
            Place::Folder(id) => {
                if let Some(f) = self.folders.iter_mut().find(|f| f.id == id) {
                    f.projects.push(path.to_string());
                }
            }
            Place::Loose => {}
        }
    }

    pub fn toggle_pin(&mut self, path: &str) {
        let to = match self.place_of(path) {
            Place::Pinned => Place::Loose,
            _ => Place::Pinned,
        };
        self.move_to(path, to);
    }

    /// Manual order, seeded with what showed: the user's earlier order where there was one.
    pub fn set_sort(&mut self, sort: ProjectSort, recent: &[String]) {
        if sort == ProjectSort::Manual {
            let was = self.sort;
            self.sort = ProjectSort::Manual;
            self.manual = self.order(recent);
            self.sort = was;
        }
        self.sort = sort;
    }

    pub fn drop_on(&mut self, path: &str, on: DropOn, recent: &[String]) {
        let before = match on {
            DropOn::Into(place) => return self.move_to(path, place),
            DropOn::Before(before) => before,
        };
        if before == path {
            return;
        }
        let (from, to) = (self.place_of(path), self.place_of(&before));
        if from != to {
            self.move_to(path, to);
            // Into another place is all a drop asks while the list sorts itself.
            if self.sort == ProjectSort::Recent {
                return;
            }
        }
        // A drop among its own: the user wants it there, which only manual order keeps.
        self.set_sort(ProjectSort::Manual, recent);
        self.manual.retain(|p| p != path);
        let at = self
            .manual
            .iter()
            .position(|p| *p == before)
            .unwrap_or(self.manual.len());
        self.manual.insert(at, path.to_string());
    }

    /// A new folder at the end, named `name`; its id.
    pub fn new_folder(&mut self, name: &str) -> u64 {
        let id = self.folders.iter().map(|f| f.id).max().map_or(1, |m| m + 1);
        self.folders.push(ProjectFolder {
            id,
            name: name.trim().to_string(),
            ..Default::default()
        });
        id
    }

    /// An empty name leaves it as it was.
    pub fn rename_folder(&mut self, id: u64, name: &str) {
        let name = name.trim();
        if let Some(f) = self.folders.iter_mut().find(|f| f.id == id)
            && !name.is_empty()
        {
            f.name = name.to_string();
        }
    }

    /// Its projects go back among the ungrouped ones.
    pub fn delete_folder(&mut self, id: u64) {
        self.folders.retain(|f| f.id != id);
    }

    pub fn toggle_folder(&mut self, id: u64) {
        if let Some(f) = self.folders.iter_mut().find(|f| f.id == id) {
            f.collapsed = !f.collapsed;
        }
    }

    pub fn set_color(&mut self, path: &str, key: Option<&str>) {
        match key {
            Some(k) => {
                self.colors.insert(path.to_string(), k.to_string());
            }
            None => {
                self.colors.remove(path);
            }
        }
    }

    /// Off the list: nothing of it stays.
    pub fn forget(&mut self, path: &str) {
        self.move_to(path, Place::Loose);
        self.manual.retain(|p| p != path);
        self.colors.remove(path);
    }

    /// A folder found elsewhere keeps its place, pin and color.
    pub fn relocate(&mut self, old: &str, new: &str) {
        if old == new {
            return;
        }
        self.forget(new);
        let swap = |p: &mut String| {
            if p == old {
                *p = new.to_string();
            }
        };
        self.manual.iter_mut().for_each(swap);
        self.pinned.iter_mut().for_each(swap);
        for f in &mut self.folders {
            f.projects.iter_mut().for_each(swap);
        }
        if let Some(c) = self.colors.remove(old) {
            self.colors.insert(new.to_string(), c);
        }
    }

    pub fn swatch_of(&self, path: &str) -> &'static Swatch {
        self.colors
            .get(path)
            .and_then(|k| swatch(k))
            .unwrap_or_else(|| auto_swatches().nth(preferred(path)).unwrap_or(&SWATCHES[0]))
    }

    /// Gives each project in `recent` without a color one, oldest first: its path's hash picks
    /// the swatch, or the next one the fewest others have, so neighbours differ while the
    /// palette lasts.
    pub fn assign_colors(&mut self, recent: &[String]) {
        let auto: Vec<&Swatch> = auto_swatches().collect();
        for path in recent.iter().rev() {
            if self.colors.get(path).and_then(|k| swatch(k)).is_some() {
                continue;
            }
            let uses = |s: &Swatch| {
                recent
                    .iter()
                    .filter(|p| self.colors.get(*p).map(String::as_str) == Some(s.key))
                    .count()
            };
            let start = preferred(path);
            let pick = (0..auto.len())
                .map(|i| auto[(start + i) % auto.len()])
                .min_by_key(|s| uses(s))
                .unwrap_or(auto[0]);
            self.colors.insert(path.clone(), pick.key.to_string());
        }
    }

    /// Every project as the arranged list has it, folders open: the sidebar's plain list.
    pub fn flat(&self, recent: &[String]) -> Vec<String> {
        self.layout(recent, "", true)
            .into_iter()
            .filter_map(|l| match l {
                Line::Project { path, .. } => Some(path),
                _ => None,
            })
            .collect()
    }

    /// The list as it shows: Pinned, the folders, then the rest; only what `query` matches by
    /// name or path (a folder's name matching shows all of it), searched folders open.
    /// `expand`: every folder open, as for a list without folders.
    pub fn layout(&self, recent: &[String], query: &str, expand: bool) -> Vec<Line> {
        let q = query.trim().to_lowercase();
        let hit = |s: &str| s.to_lowercase().contains(&q);
        let found = |p: &str| q.is_empty() || hit(project_name(p)) || hit(p);
        let order = self.order(recent);
        let mut lines = Vec::new();
        let pinned: Vec<&String> = order
            .iter()
            .filter(|p| self.place_of(p) == Place::Pinned && found(p))
            .collect();
        let organized = !pinned.is_empty() || !self.folders.is_empty();
        if !pinned.is_empty() {
            lines.push(Line::Header(Section::Pinned));
            lines.extend(pinned.into_iter().map(|p| Line::Project {
                path: p.clone(),
                place: Place::Pinned,
            }));
        }
        for f in &self.folders {
            let place = Place::Folder(f.id);
            let named = !q.is_empty() && hit(&f.name);
            let members: Vec<&String> = order
                .iter()
                .filter(|p| self.place_of(p) == place && (named || found(p)))
                .collect();
            if !q.is_empty() && members.is_empty() && !named {
                continue;
            }
            let open = expand || !q.is_empty() || !f.collapsed;
            lines.push(Line::Folder {
                id: f.id,
                name: f.name.clone(),
                open,
                count: members.len(),
            });
            if open {
                lines.extend(members.into_iter().map(|p| Line::Project {
                    path: p.clone(),
                    place,
                }));
            }
        }
        let loose: Vec<&String> = order
            .iter()
            .filter(|p| self.place_of(p) == Place::Loose && found(p))
            .collect();
        // Headed while unsearched even when empty: it's where a project is dragged out of a folder.
        if organized && (q.is_empty() || !loose.is_empty()) {
            lines.push(Line::Header(Section::Projects));
        }
        lines.extend(loose.into_iter().map(|p| Line::Project {
            path: p.clone(),
            place: Place::Loose,
        }));
        lines
    }
}

/// A tile color, in a shade for each theme; the letter on it is whichever of near-black and
/// white reads better.
#[derive(Debug, PartialEq, Eq)]
pub struct Swatch {
    pub key: &'static str,
    pub name: &'static str,
    dark: u32,
    light: u32,
    /// Given out unasked; gray is only picked by hand, since a dimmed tile means a missing folder.
    auto: bool,
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
        let fill = self.fill(dark);
        if contrast(fill, NEAR_BLACK) >= contrast(fill, WHITE) {
            NEAR_BLACK
        } else {
            WHITE
        }
    }
}

pub fn swatch(key: &str) -> Option<&'static Swatch> {
    SWATCHES.iter().find(|s| s.key == key)
}

fn auto_swatches() -> impl Iterator<Item = &'static Swatch> {
    SWATCHES.iter().filter(|s| s.auto)
}

/// The auto swatch `path` hashes to. FNV-1a: std's hasher may change between Rust releases,
/// and a project's color must not.
fn preferred(path: &str) -> usize {
    let mut h: u32 = 0x811c_9dc5;
    for b in path.as_bytes() {
        h = (h ^ u32::from(*b)).wrapping_mul(0x0100_0193);
    }
    h as usize % auto_swatches().count()
}

/// WCAG relative luminance of an 0xRRGGBB color.
fn luminance(rgb: u32) -> f32 {
    let channel = |shift: u32| {
        let c = ((rgb >> shift) & 0xff) as f32 / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

fn contrast(a: u32, b: u32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn paths(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| match l {
                Line::Header(Section::Pinned) => "#pinned".into(),
                Line::Header(Section::Projects) => "#projects".into(),
                Line::Folder { name, open, .. } => {
                    format!("{}{name}", if *open { "v " } else { "> " })
                }
                Line::Project { path, .. } => path.clone(),
            })
            .collect()
    }

    #[test]
    fn paths_under_home_read_from_tilde() {
        let home = Some("/Users/me");
        assert_eq!(home_relative("/Users/me/code/app", home), "~/code/app");
        assert_eq!(home_relative("/Users/me", Some("/Users/me/")), "~");
        // Another user whose name starts the same isn't home.
        assert_eq!(
            home_relative("/Users/mechanic/app", home),
            "/Users/mechanic/app"
        );
        assert_eq!(home_relative("/tmp/app", home), "/tmp/app");
        assert_eq!(home_relative("/tmp/app", None), "/tmp/app");
    }

    #[test]
    fn recent_by_default_and_manual_keeps_the_dragged_order() {
        let recent = list(&["/r/c", "/r/b", "/r/a"]);
        let mut org = ProjectOrg::default();
        assert_eq!(org.flat(&recent), recent);
        // Dropping a row on another switches to manual, seeded with what showed.
        org.drop_on("/r/a", DropOn::Before("/r/c".into()), &recent);
        assert_eq!(org.sort, ProjectSort::Manual);
        assert_eq!(org.flat(&recent), ["/r/a", "/r/c", "/r/b"]);
        // Opening another project doesn't move the placed ones; the new one shows on top.
        let recent = list(&["/r/d", "/r/b", "/r/c", "/r/a"]);
        assert_eq!(org.flat(&recent), ["/r/d", "/r/a", "/r/c", "/r/b"]);
        // Back to recent and to manual again: the order the user made returns.
        org.set_sort(ProjectSort::Recent, &recent);
        assert_eq!(org.flat(&recent), recent);
        org.set_sort(ProjectSort::Manual, &recent);
        assert_eq!(org.flat(&recent), ["/r/d", "/r/a", "/r/c", "/r/b"]);
        org.drop_on("/r/d", DropOn::Before("/r/d".into()), &recent);
        assert_eq!(org.flat(&recent), ["/r/d", "/r/a", "/r/c", "/r/b"]);
    }

    #[test]
    fn pins_and_folders_arrange_the_list() {
        let recent = list(&["/r/e", "/r/d", "/r/c", "/r/b", "/r/a"]);
        let mut org = ProjectOrg::default();
        org.toggle_pin("/r/a");
        let work = org.new_folder("  Work ");
        let empty = org.new_folder("Empty");
        assert_eq!((work, empty), (1, 2));
        org.move_to("/r/b", Place::Folder(work));
        org.move_to("/r/d", Place::Folder(work));
        assert_eq!(
            paths(&org.layout(&recent, "", false)),
            [
                "#pinned",
                "/r/a",
                "v Work",
                "/r/d",
                "/r/b",
                "v Empty",
                "#projects",
                "/r/e",
                "/r/c"
            ]
        );
        // Moving into a folder unpins; pinning takes it out of its folder.
        org.move_to("/r/a", Place::Folder(work));
        org.toggle_pin("/r/d");
        assert_eq!(org.pinned, ["/r/d"]);
        assert_eq!(org.place_of("/r/a"), Place::Folder(work));
        org.toggle_folder(work);
        org.rename_folder(work, "Client");
        org.rename_folder(work, "  ");
        assert_eq!(
            paths(&org.layout(&recent, "", false)),
            [
                "#pinned",
                "/r/d",
                "> Client",
                "v Empty",
                "#projects",
                "/r/e",
                "/r/c"
            ]
        );
        assert_eq!(org.flat(&recent), ["/r/d", "/r/b", "/r/a", "/r/e", "/r/c"]);
        // A deleted folder's projects go back among the rest.
        org.delete_folder(work);
        assert_eq!(org.place_of("/r/a"), Place::Loose);
        assert_eq!(org.new_folder("x"), 3);
        // A drop on a header or folder moves it there; on a row of another place, beside it.
        org.drop_on("/r/e", DropOn::Into(Place::Folder(3)), &recent);
        assert_eq!(org.place_of("/r/e"), Place::Folder(3));
        org.drop_on("/r/e", DropOn::Before("/r/d".into()), &recent);
        assert_eq!(org.place_of("/r/e"), Place::Pinned);
        assert_eq!(org.sort, ProjectSort::Recent);
    }

    #[test]
    fn a_search_filters_by_name_or_path_and_opens_folders() {
        let recent = list(&["/code/api", "/code/web", "/home/notes"]);
        let mut org = ProjectOrg::default();
        let f = org.new_folder("Clients");
        org.move_to("/code/web", Place::Folder(f));
        org.toggle_folder(f);
        assert_eq!(
            paths(&org.layout(&recent, "WEB", false)),
            ["v Clients", "/code/web"]
        );
        assert_eq!(
            paths(&org.layout(&recent, "code", false)),
            ["v Clients", "/code/web", "#projects", "/code/api"]
        );
        // The folder's name matching shows all of it.
        assert_eq!(
            paths(&org.layout(&recent, "client", false)),
            ["v Clients", "/code/web"]
        );
        assert!(org.layout(&recent, "zzz", false).is_empty());
        // Without pins or folders, no headers at all.
        assert_eq!(
            paths(&ProjectOrg::default().layout(&recent, "", false)),
            recent
        );
    }

    #[test]
    fn forgetting_and_locating_carry_the_arrangement() {
        let mut org = ProjectOrg::default();
        org.toggle_pin("/old");
        org.set_color("/old", Some("teal"));
        org.relocate("/old", "/new");
        assert_eq!(org.pinned, ["/new"]);
        assert_eq!(org.swatch_of("/new").key, "teal");
        org.forget("/new");
        assert!(org.pinned.is_empty() && org.colors.is_empty());
    }

    #[test]
    fn colors_are_stable_distinct_and_readable() {
        // The hash never changes between builds: a project keeps its color.
        assert_eq!(preferred("/r/app"), preferred("/r/app"));
        assert_eq!(preferred(""), 0x811c_9dc5usize % 12);
        let recent: Vec<String> = (0..12).map(|i| format!("/r/p{i}")).collect();
        let mut org = ProjectOrg::default();
        org.assign_colors(&recent);
        let mut keys: Vec<&String> = org.colors.values().collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 12, "twelve projects, twelve colors");
        assert!(!keys.iter().any(|k| *k == "gray"));
        // Given once: a new project doesn't recolor the others.
        let before = org.colors.clone();
        let mut more = recent.clone();
        more.insert(0, "/r/new".into());
        org.assign_colors(&more);
        assert!(before.iter().all(|(p, k)| org.colors[p] == *k));
        org.set_color("/r/new", Some("gray"));
        assert_eq!(org.swatch_of("/r/new").key, "gray");
        // An unknown key from a newer build falls back to the hashed one.
        org.set_color("/r/new", Some("chartreuse"));
        assert!(org.swatch_of("/r/new").auto);
        for s in &SWATCHES {
            for dark in [true, false] {
                let c = contrast(s.fill(dark), s.letter(dark));
                assert!(c >= 4.5, "{} ({dark}): {c}", s.key);
            }
        }
    }

    #[test]
    fn an_old_or_odd_file_reads() {
        let org: ProjectOrg = serde_json::from_str(
            r#"{"sort": "sideways", "pinned": ["/a", 3], "folders": [{"id": 4, "name": "W",
                "projects": ["/b"], "collapsed": "no"}], "colors": {"/a": "red"}}"#,
        )
        .unwrap();
        assert_eq!(org.sort, ProjectSort::Recent);
        assert_eq!(org.pinned, ["/a"]);
        assert_eq!(org.place_of("/b"), Place::Folder(4));
        assert!(!org.folders[0].collapsed);
        assert_eq!(org.swatch_of("/a").key, "red");
        let back: ProjectOrg = serde_json::from_str(&serde_json::to_string(&org).unwrap()).unwrap();
        assert_eq!(back, org);
    }
}
