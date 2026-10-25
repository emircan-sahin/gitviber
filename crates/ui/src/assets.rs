//! Everything the windows load by path: the file icons (file-icons/<name>.svg), the app's logo
//! (app/logo.svg), gpui-kit's component icons and the Lucide icons the app names. Only those are
//! in the binary; a test fails when a source names an icon that isn't.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

use crate::widgets::file_icon;

pub struct Assets;

pub const LOGO: &str = "app/logo.svg";

// The Lucide icons the sources name beyond the component set: the whole catalog was 0.8 MB of
// SVGs, about 1,600 of them never shown. `every_named_icon_loads` keeps this list exact.
gpui_kit::assets::icon_assets!(
    AppIcons,
    [
        Archive,
        ArchiveRestore,
        ArrowDownToLine,
        ArrowLeftRight,
        ArrowLeftToLine,
        ArrowRightToLine,
        ArrowUpFromLine,
        ArrowUpToLine,
        Bug,
        Cherry,
        ChevronsDownUp,
        Circle,
        CircleArrowUp,
        CircleDashed,
        CircleDot,
        CircleMinus,
        CircleQuestionMark,
        CircleSlash,
        ClipboardCopy,
        ClipboardList,
        Cloud,
        CloudOff,
        CloudUpload,
        Code,
        Columns2,
        Combine,
        Contrast,
        Crosshair,
        Database,
        Download,
        Eraser,
        FileClock,
        FileCode,
        FileDiff,
        FileDown,
        Files,
        Flame,
        FlaskConical,
        FolderClock,
        FolderDown,
        FolderGit2,
        FolderSearch,
        FolderTree,
        FoldVertical,
        Gem,
        GitBranch,
        GitBranchPlus,
        GitCommitHorizontal,
        GitCommitVertical,
        GitCompare,
        GitCompareArrows,
        GitGraph,
        GitMerge,
        GitPullRequest,
        GitPullRequestArrow,
        GitPullRequestClosed,
        GitPullRequestDraft,
        Hammer,
        Image,
        Keyboard,
        Languages,
        Lightbulb,
        Link,
        List,
        ListFilter,
        ListTree,
        Lock,
        LockOpen,
        Maximize2,
        MessageCircleReply,
        MessageSquare,
        MessageSquareText,
        MessageSquareWarning,
        Minimize2,
        OctagonAlert,
        Package,
        PackageOpen,
        PanelTop,
        Pencil,
        Pin,
        PinOff,
        Plug,
        Quote,
        Regex,
        RotateCcw,
        RotateCcwClock,
        RotateCcwSquare,
        Rows2,
        Scale,
        ScanSearch,
        Scissors,
        ScrollText,
        SearchCode,
        Send,
        Shapes,
        ShieldAlert,
        ShieldCheck,
        ShieldX,
        Signal,
        SignalLow,
        SignalMedium,
        SlidersHorizontal,
        Space,
        Sparkles,
        SquareArrowOutUpRight,
        SquareCheck,
        SquareDashedMousePointer,
        SunMoon,
        TabletSmartphone,
        Tag,
        TextWrap,
        Trash,
        Unlink,
        Upload,
        UserPlus,
        WholeWord,
        Wifi,
        X,
        Zap,
        ZoomIn,
        ZoomOut,
    ]
);

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = file_icon::svg(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if path == LOGO {
            return Ok(Some(Cow::Borrowed(include_bytes!("../assets/logo.svg"))));
        }
        if let Some(bytes) = AppIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out = file_icon::list(path);
        out.extend(AppIcons.list(path)?);
        out.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::assets::IconName;
    use std::collections::{BTreeSet, HashMap};
    use std::path::Path;

    /// The variants after `IconName::` and `Lucide::` (crates/changes and diff's alias) in the
    /// .rs files under `dir`.
    fn named_icons(dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                named_icons(&path, out);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for prefix in ["IconName::", "Lucide::"] {
                for (at, _) in text.match_indices(prefix) {
                    let rest = &text[at + prefix.len()..];
                    let end = rest
                        .find(|c: char| !c.is_ascii_alphanumeric())
                        .unwrap_or(rest.len());
                    let name = &rest[..end];
                    if name.starts_with(|c: char| c.is_ascii_uppercase()) && name != "ALL" {
                        out.insert(name.to_owned());
                    }
                }
            }
        }
    }

    #[test]
    fn every_named_icon_loads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut named = BTreeSet::new();
        for krate in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
            let src = krate.path().join("src");
            if src.is_dir() {
                named_icons(&src, &mut named);
            }
        }
        for vendored in ["vendor/gpui-base/src", "vendor/gpui-component/src"] {
            named_icons(&root.join(vendored), &mut named);
        }
        let by_name: HashMap<String, IconName> = IconName::ALL
            .iter()
            .map(|icon| (format!("{icon:?}"), *icon))
            .collect();
        let missing: Vec<&String> = named
            .iter()
            .filter(|name| {
                by_name
                    .get(*name)
                    .is_none_or(|icon| !matches!(Assets.load(&icon.path()), Ok(Some(_))))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "named but not bundled, add to AppIcons: {missing:?}"
        );
        let unused: Vec<&String> = by_name
            .iter()
            .filter(|(name, icon)| {
                !named.contains(*name) && matches!(AppIcons.load(&icon.path()), Ok(Some(_)))
            })
            .map(|(name, _)| name)
            .collect();
        assert!(
            unused.is_empty(),
            "in AppIcons but named nowhere: {unused:?}"
        );
    }
}
