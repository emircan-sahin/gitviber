//! A file's diff as read from core, with what the view needs worked out off the main thread.

use crate::model::{self, Row, SideText, indent, notes};
use crate::source::DiffSource;
use crate::style::Whitespace;
use gitviber_core::Core;

pub struct DiffData {
    pub old: SideText,
    pub new: SideText,
    pub rows: Vec<Row>,
    pub whitespace_hidden: bool,
    pub eol_only: bool,
    /// Spaces per indentation level the drawing widens (model::indent).
    pub unit: usize,
    /// Columns of the widest line as drawn.
    pub widest: usize,
    pub additions: u32,
    pub deletions: u32,
    /// Why no lines show, when none do.
    pub placeholder: Option<String>,
    /// What the header says beside the path.
    pub note: Option<String>,
}

impl DiffData {
    /// Blocking: reads both sides and diffs them.
    pub fn load(
        core: &Core,
        source: &DiffSource,
        whitespace: Whitespace,
    ) -> Result<DiffData, String> {
        let c = &source.compare;
        let pair = gitviber_core::api::files::diff_pair(
            core,
            c.kind().to_string(),
            source.path.clone(),
            source.old_path.clone(),
            c.sha().map(str::to_string),
            c.base().map(str::to_string),
            whitespace.core_name(),
        )?;
        Ok(DiffData::from_pair(pair, source))
    }

    pub fn from_pair(pair: gitviber_core::git::DiffPair, source: &DiffSource) -> DiffData {
        let gitviber_core::git::DiffPair {
            original,
            modified,
            rows,
            whitespace_hidden,
            eol_only,
        } = pair;
        let unit = indent::indent_unit([original.text.as_str(), modified.text.as_str()]);
        let widest = indent::widest(&original.text, unit).max(indent::widest(&modified.text, unit));
        let (old, new) = (SideText::new(original), SideText::new(modified));
        let (additions, deletions) = model::counts(&rows);
        let placeholder = notes::placeholder(&old, &new, &rows, whitespace_hidden, &source.file);
        let note = notes::header_note(&old, &new, &rows, whitespace_hidden, eol_only, &source.file);
        DiffData {
            old,
            new,
            rows,
            whitespace_hidden,
            eol_only,
            unit,
            widest,
            additions,
            deletions,
            placeholder,
            note,
        }
    }

    /// Two texts diffed here, for tests and previews that have no repo.
    pub fn from_texts(old: &str, new: &str, source: &DiffSource) -> DiffData {
        let (rows, hidden) =
            gitviber_core::diff::rows(old, new, gitviber_core::diff::whitespace_mode(None));
        let text = |t: &str| gitviber_core::git::FileText {
            text: t.to_string(),
            exists: true,
            ..Default::default()
        };
        let eol_only = rows.iter().any(|r| r.k != 0) && gitviber_core::diff::eol_only(old, new);
        DiffData::from_pair(
            gitviber_core::git::DiffPair {
                original: text(old),
                modified: text(new),
                rows,
                whitespace_hidden: hidden,
                eol_only,
            },
            source,
        )
    }

    /// Same texts and rows: a refresh that found nothing new keeps what's drawn.
    pub fn same_as(&self, other: &DiffData) -> bool {
        self.rows == other.rows
            && self.old.exists == other.old.exists
            && self.new.exists == other.new.exists
            && self.old.len() == other.old.len()
            && self.new.len() == other.new.len()
            && self.old.rope == other.old.rope
            && self.new.rope == other.new.rope
    }

    /// The side a line of `side` is read from.
    pub fn side(&self, side: model::Side) -> &SideText {
        match side {
            model::Side::Old => &self.old,
            model::Side::New => &self.new,
        }
    }
}
