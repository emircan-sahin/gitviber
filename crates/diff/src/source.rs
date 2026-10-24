//! What a diff compares, as plain data: tabs, History, pull requests and reviews build it from
//! their own state, and the view reads both sides through core.

/// The two sides of a file's diff.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Compare {
    /// The index against the working tree: what `git diff` shows.
    Unstaged,
    /// HEAD against the index: what's staged.
    Staged,
    /// HEAD against the working tree (the file view's change bars).
    Worktree,
    /// A commit against its first parent.
    Commit { sha: String },
    /// `base` against `head`: a pull request from its merge base, a comparison.
    Range { base: String, head: String },
    /// A commit (a branch's merge base) against the working tree: a branch review.
    Base { base: String },
    /// Two files of the working tree: `old_path` against `path`.
    Files,
}

impl Compare {
    /// core's name for it (git::diff_pair's `kind`).
    pub fn kind(&self) -> &'static str {
        match self {
            Compare::Unstaged => "unstaged",
            Compare::Staged => "staged",
            Compare::Worktree => "worktree",
            Compare::Commit { .. } => "commit",
            Compare::Range { .. } => "range",
            Compare::Base { .. } => "base",
            Compare::Files => "files",
        }
    }

    /// The `sha` core takes: a commit's, a range's head.
    pub fn sha(&self) -> Option<&str> {
        match self {
            Compare::Commit { sha } => Some(sha),
            Compare::Range { head, .. } => Some(head),
            _ => None,
        }
    }

    /// The `base` core takes.
    pub fn base(&self) -> Option<&str> {
        match self {
            Compare::Range { base, .. } | Compare::Base { base } => Some(base),
            _ => None,
        }
    }

    /// Commits and ranges never change; the rest follow the working tree or the index.
    pub fn is_fixed(&self) -> bool {
        matches!(self, Compare::Commit { .. } | Compare::Range { .. })
    }

    /// The new side is the file on disk, which can be edited in place.
    pub fn new_side_on_disk(&self) -> bool {
        matches!(
            self,
            Compare::Unstaged | Compare::Worktree | Compare::Base { .. } | Compare::Files
        )
    }
}

/// One file's diff: what to compare, the file, and how to read it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DiffSource {
    pub compare: Compare,
    /// Repo-relative, as git names it.
    pub path: String,
    /// Where the old side is read: a rename's old path, or the left file of `Compare::Files`.
    pub old_path: Option<String>,
    /// A highlighter language name ("rust", "tsx"…) instead of the one the path says.
    pub language: Option<String>,
    /// What git status says about the file, for the notes a text diff can't show.
    pub file: FileFacts,
}

/// What status knows about a changed file that its two texts don't say.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FileFacts {
    /// One of M A D R C T U ?; empty when unknown.
    pub status: String,
    /// "100644 → 100755" when the mode changed.
    pub mode: Option<String>,
    /// A submodule's porcelain v2 `S<c><m><u>`.
    pub submodule: Option<String>,
}

impl DiffSource {
    pub fn new(compare: Compare, path: impl Into<String>) -> Self {
        DiffSource {
            compare,
            path: path.into(),
            old_path: None,
            language: None,
            file: FileFacts::default(),
        }
    }

    pub fn old_path(mut self, old_path: Option<String>) -> Self {
        self.old_path = old_path;
        self
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub fn file(mut self, file: FileFacts) -> Self {
        self.file = file;
        self
    }

    /// From a status entry of an unstaged or staged list (or a commit's / range's files).
    pub fn from_change(compare: Compare, change: &gitviber_core::git::FileChange) -> Self {
        DiffSource {
            compare,
            path: change.path.clone(),
            old_path: change.old_path.clone(),
            language: None,
            file: FileFacts {
                status: change.status.clone(),
                mode: change.mode.clone(),
                submodule: change.submodule.clone(),
            },
        }
    }

    /// The path the old side is read at.
    pub fn original_path(&self) -> &str {
        self.old_path.as_deref().unwrap_or(&self.path)
    }

    /// Identity for what a view of it remembers (viewed marks, folds opened, scroll).
    pub fn key(&self) -> String {
        format!(
            "{}\0{}\0{}\0{}\0{}",
            self.compare.kind(),
            self.path,
            self.old_path.as_deref().unwrap_or(""),
            self.compare.sha().unwrap_or(""),
            self.compare.base().unwrap_or("")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_arguments() {
        let range = Compare::Range {
            base: "b".into(),
            head: "h".into(),
        };
        assert_eq!(
            (range.kind(), range.sha(), range.base()),
            ("range", Some("h"), Some("b"))
        );
        assert!(range.is_fixed() && !Compare::Unstaged.is_fixed());
        let s = DiffSource::new(Compare::Staged, "a.rs").old_path(Some("b.rs".into()));
        assert_eq!(s.original_path(), "b.rs");
        assert_ne!(s.key(), DiffSource::new(Compare::Unstaged, "a.rs").key());
    }
}
