//! File and folder icons from Material Icon Theme (MIT), looked up as VS Code does: the file's
//! name, then its extensions longest first, then its language. Tables and SVGs are static, so a
//! lookup allocates nothing and only icons on screen are ever rasterized.

use gpui_kit::{App, IntoElement, RenderOnce, SharedString, Styled, Window, img, px};

#[path = "file_icon_map.rs"]
mod map;

fn find(table: &[(&str, u16)], key: &str) -> Option<u16> {
    table
        .binary_search_by(|(k, _)| (*k).cmp(key))
        .ok()
        .map(|i| table[i].1)
}

fn path_of(icon: u16) -> &'static str {
    map::ICONS[icon as usize].0
}

/// The icon's SVG for an asset path this module handed out.
pub fn svg(asset_path: &str) -> Option<&'static [u8]> {
    let i = map::ICONS
        .binary_search_by(|(p, _)| (*p).cmp(asset_path))
        .ok()?;
    Some(map::ICONS[i].1)
}

pub(crate) fn list(prefix: &str) -> Vec<SharedString> {
    map::ICONS
        .iter()
        .filter(|(p, _)| p.starts_with(prefix))
        .map(|(p, _)| SharedString::new_static(p))
        .collect()
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The asset path of a file's icon. `language`: the file's language id when the caller knows it,
/// for names the tables don't cover (Dockerfile.dev, .ssh/config).
pub fn file_icon_path(path: &str, language: Option<&str>) -> &'static str {
    let name = basename(path);
    let lower = name.to_lowercase();
    let mut icon = find(map::FILE_NAMES, name).or_else(|| find(map::FILE_NAMES, &lower));
    // Longest compound extension first: "vault.controller.ts" → "controller.ts" → "ts".
    let mut from = 0;
    while icon.is_none() {
        let Some(dot) = lower[from..].find('.') else {
            break;
        };
        from += dot + 1;
        icon = find(map::FILE_EXTENSIONS, &lower[from..]);
    }
    let icon = icon
        .or_else(|| language.and_then(|l| find(map::LANGUAGE_IDS, l)))
        .unwrap_or(map::FILE);
    path_of(icon)
}

/// The asset path of a folder's icon, open or closed.
pub fn folder_icon_path(name: &str, open: bool) -> &'static str {
    let lower = name.to_lowercase();
    let icon = if open {
        find(map::FOLDER_NAMES_OPEN, &lower).unwrap_or(map::FOLDER_OPEN)
    } else {
        find(map::FOLDER_NAMES, &lower).unwrap_or(map::FOLDER)
    };
    path_of(icon)
}

/// A file's or folder's icon, 16px unless sized otherwise.
#[derive(IntoElement)]
pub struct FileIcon {
    asset: &'static str,
    size: gpui_kit::Pixels,
}

impl FileIcon {
    pub fn file(path: &str) -> Self {
        Self {
            asset: file_icon_path(path, None),
            size: px(16.),
        }
    }

    pub fn file_with_language(path: &str, language: &str) -> Self {
        Self {
            asset: file_icon_path(path, Some(language)),
            size: px(16.),
        }
    }

    pub fn folder(name: &str, open: bool) -> Self {
        Self {
            asset: folder_icon_path(name, open),
            size: px(16.),
        }
    }

    pub fn size(mut self, size: gpui_kit::Pixels) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for FileIcon {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        img(SharedString::new_static(self.asset))
            .size(self.size)
            .flex_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_for_binary_search() {
        for table in [
            map::FILE_NAMES,
            map::FILE_EXTENSIONS,
            map::FOLDER_NAMES,
            map::FOLDER_NAMES_OPEN,
            map::LANGUAGE_IDS,
        ] {
            assert!(table.windows(2).all(|w| w[0].0 < w[1].0));
        }
        assert!(map::ICONS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn every_icon_rasterizes() {
        let renderer = gpui_kit::SvgRenderer::new(std::sync::Arc::new(crate::assets::Assets));
        for (path, bytes) in map::ICONS {
            let image = renderer.render_single_frame(bytes, 1.0);
            assert!(image.is_ok(), "{path}: {:?}", image.err());
        }
    }

    #[test]
    fn lookups_follow_vs_code() {
        assert_eq!(
            file_icon_path("src/search/fuzzy.ts", None),
            "file-icons/typescript.svg"
        );
        assert_eq!(
            file_icon_path("package.json", None),
            "file-icons/nodejs.svg"
        );
        assert_eq!(
            file_icon_path("a/b/fuzzy.test.ts", None),
            "file-icons/test-ts.svg"
        );
        // The theme gives these an info and a check circle, which read as a status.
        assert_eq!(file_icon_path("README.md", None), "file-icons/markdown.svg");
        assert_eq!(
            file_icon_path("docs/todo.md", None),
            "file-icons/markdown.svg"
        );
        assert_eq!(file_icon_path("weird.nothing", None), "file-icons/file.svg");
        assert_eq!(folder_icon_path("src", false), "file-icons/folder-src.svg");
        assert_eq!(
            folder_icon_path("src", true),
            "file-icons/folder-src-open.svg"
        );
        assert_eq!(
            folder_icon_path("zzz-none", true),
            "file-icons/folder-open.svg"
        );
        assert!(
            svg("file-icons/typescript.svg")
                .unwrap()
                .starts_with(b"<svg")
        );
        assert!(svg("icons/search.svg").is_none());
    }
}
