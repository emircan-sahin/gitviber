//! The icon a command shows in the palette: its own where one says what it does, else its
//! category's.

use gitviber_ui::keymap::{Category, Command};
use gpui_kit::assets::IconName;

pub fn command_icon(c: &Command) -> IconName {
    match c.id {
        "workbench.quickOpen" => IconName::File,
        "workbench.openChange" | "view.changes" => IconName::FileDiff,
        "workbench.openSettings" => IconName::Settings2,
        "workbench.shortcutOverlay" | "help.shortcuts" => IconName::Keyboard,
        "file.openRepo" => IconName::FolderOpen,
        "file.cloneRepo" => IconName::FolderDown,
        "file.switchProject" => IconName::FolderGit2,
        "file.reveal" | "file.revealInExplorer" => IconName::FolderSearch,
        "file.openIn" | "editor.openOnGitHub" => IconName::SquareArrowOutUpRight,
        "file.save" => IconName::FileDown,
        "project.forget" => IconName::CircleMinus,
        "repo.refresh" | "browser.reload" => IconName::RotateCw,
        "app.quit" => IconName::CircleX,
        "app.hide" | "app.hideOthers" => IconName::EyeOff,
        "app.showAll" | "review.toggleViewed" | "editor.peekDefinition" => IconName::Eye,
        "window.minimize" => IconName::Minus,
        "window.zoom" | "window.toggleFullScreen" => IconName::Maximize,
        "editor.find" | "browser.find" | "history.find" => IconName::Search,
        "search.findInFiles" | "history.search" => IconName::SearchCode,

        "view.history" => IconName::GitGraph,
        "view.pulls" => IconName::GitPullRequest,
        "view.issues" => IconName::CircleDot,
        "view.toggleExplorer" | "view.showExplorer" => IconName::PanelLeft,
        "view.toggleGitPanel" | "view.focusGitPanel" => IconName::PanelRight,
        "view.focusCode" => IconName::Code,
        "changes.focusMessage" | "github.postComment" => IconName::MessageSquare,
        "view.zoomIn"
        | "editor.fontZoomIn"
        | "terminal.fontZoomIn"
        | "media.zoomIn"
        | "browser.zoomIn" => IconName::ZoomIn,
        "view.zoomOut"
        | "editor.fontZoomOut"
        | "terminal.fontZoomOut"
        | "media.zoomOut"
        | "browser.zoomOut" => IconName::ZoomOut,
        "view.zoomReset"
        | "editor.fontZoomReset"
        | "terminal.fontZoomReset"
        | "browser.zoomReset" => IconName::RotateCcw,
        "media.fit" => IconName::Maximize2,

        "tab.close" | "tab.closeOthers" | "tab.closeLeft" | "tab.closeRight" | "tab.closeAll"
        | "terminal.close" => IconName::Close,
        "tab.reopenClosed" => IconName::RotateCcwSquare,
        "tab.moveLeft"
        | "tab.prev"
        | "browser.back"
        | "view.focusPrevPanel"
        | "terminal.focusLeft"
        | "terminal.prevPane" => IconName::ArrowLeft,
        "tab.moveRight"
        | "tab.next"
        | "browser.forward"
        | "view.focusNextPanel"
        | "terminal.focusRight"
        | "terminal.nextPane" => IconName::ArrowRight,
        "review.prevFile"
        | "diff.prevChange"
        | "terminal.focusUp"
        | "terminal.prevCommand"
        | "browser.findPrev" => IconName::ArrowUp,
        "review.nextFile"
        | "diff.nextChange"
        | "terminal.focusDown"
        | "terminal.nextCommand"
        | "browser.findNext" => IconName::ArrowDown,

        "review.addNote" => IconName::MessageSquareText,
        "review.copyNotes" | "help.copyDiagnostics" => IconName::ClipboardCopy,
        "review.sendNotes" => IconName::Send,
        "review.openAll"
        | "review.openAllStaged"
        | "review.openAllBranch"
        | "review.openAllCommit"
        | "review.openAllComparison" => IconName::Files,
        "diff.stageChange" => IconName::Plus,
        "diff.unstageChange" => IconName::Minus,
        "diff.discardChange" | "git.undo" => IconName::Undo2,
        "git.redo" => IconName::Redo2,
        "diff.toggleSplit" | "terminal.split" => IconName::Columns2,
        "terminal.splitDown" => IconName::Rows2,
        "diff.toggleCollapse" => IconName::ChevronsDownUp,
        "diff.toggleWhitespace" => IconName::Space,

        "editor.toggleWrap" => IconName::TextWrap,
        "editor.toggleBlame" | "help.showLogs" => IconName::ScrollText,
        "editor.copyGitHubLink" | "terminal.hints" => IconName::Link,
        "editor.goToDefinition" => IconName::Crosshair,
        "editor.goToReferences" => IconName::ListTree,

        "terminal.showAgents" => IconName::Bot,
        "terminal.resumeConversation" => IconName::MessageCircleReply,
        "terminal.clear" => IconName::Eraser,
        "terminal.toggleMaximize" | "terminal.zoomPane" => IconName::Maximize2,
        "terminal.equalizePanes" | "help.license" => IconName::Scale,
        "terminal.renamePane" | "explorer.rename" | "git.renameBranch" | "worktree.rename" => {
            IconName::Pencil
        }

        "browser.inspect" | "help.reportBug" => IconName::Bug,
        "browser.pick" => IconName::SquareDashedMousePointer,
        "browser.toggleDevice" => IconName::TabletSmartphone,
        "browser.clearData" | "explorer.delete" | "git.discard" | "worktree.remove" => {
            IconName::Trash
        }

        "git.newBranch" => IconName::GitBranchPlus,
        "git.compareBranches" => IconName::GitCompare,
        "git.switchWorktree" => IconName::FolderGit2,
        "git.newWorktree" => IconName::FolderPlus,
        "git.fetch" => IconName::Download,
        "git.pull" => IconName::ArrowDownToLine,
        "git.push" => IconName::ArrowUpFromLine,
        "git.applyPatch" => IconName::ClipboardList,
        "git.toggleStage" | "git.selectAllChanges" => IconName::SquareCheck,
        "git.commit" => IconName::GitCommitHorizontal,
        "git.createTag" => IconName::Tag,
        "git.suggestMessage" => IconName::Sparkles,
        "worktree.openTerminal" | "app.installCli" => IconName::SquareTerminal,
        "worktree.merge" => IconName::GitMerge,

        "app.about" => IconName::Info,
        "app.checkForUpdates" => IconName::CircleArrowUp,
        "help.readme" => IconName::BookOpen,
        "help.releaseNotes" => IconName::Sparkles,
        _ => category_icon(c.category),
    }
}

/// What a command no other icon fits shows: its area's.
fn category_icon(category: Category) -> IconName {
    match category {
        Category::General => IconName::Zap,
        Category::View => IconName::PanelTop,
        Category::Tabs => IconName::PanelTop,
        Category::Review => IconName::ScanSearch,
        Category::Diff => IconName::FileDiff,
        Category::Editor => IconName::Code,
        Category::Terminal => IconName::SquareTerminal,
        Category::Browser => IconName::Globe,
        Category::Explorer => IconName::FolderTree,
        Category::Git => IconName::GitBranch,
        Category::GitHub => IconName::GitPullRequest,
        Category::Help => IconName::CircleQuestionMark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitviber_ui::keymap;

    fn icon(id: &str) -> String {
        format!("{:?}", command_icon(keymap::command(id).unwrap()))
    }

    #[test]
    fn commands_show_what_they_do() {
        assert_eq!(icon("terminal.split"), "Columns2");
        assert_eq!(icon("git.newWorktree"), "FolderPlus");
        assert_eq!(icon("browser.open"), "Globe");
        assert_eq!(icon("file.switchProject"), "FolderGit2");
        assert_eq!(icon("git.push"), "ArrowUpFromLine");
        assert_eq!(icon("workbench.openSettings"), "Settings2");
    }

    #[test]
    fn the_rest_show_their_category() {
        // No icon of their own: the category's.
        assert_eq!(icon("tab.goto3"), "PanelTop");
        assert_eq!(icon("terminal.new"), "SquareTerminal");
        assert_eq!(icon("git.switchBranch"), "GitBranch");
        assert_eq!(icon("help.readme"), "BookOpen");
        assert_eq!(icon("view.zoomIn"), "ZoomIn");
    }

    #[test]
    fn every_command_has_an_icon_that_loads() {
        use gpui_kit::AssetSource as _;
        for c in keymap::all_commands() {
            let path = command_icon(c).path();
            assert!(
                matches!(gitviber_ui::assets::Assets.load(&path), Ok(Some(_))),
                "{}: {path}",
                c.id
            );
        }
    }
}
