//! What the commit button says and whether it can run, from what's staged and written.

/// Past this, `git log --oneline` and GitHub cut the summary off.
pub const SUMMARY_LIMIT: usize = 72;

#[derive(Clone, Copy, Debug, Default)]
pub struct Facts<'a> {
    pub staged: usize,
    /// Unstaged files `git add` takes (nested repos left out), and those left out.
    pub stageable: usize,
    pub nested: usize,
    pub conflicts: usize,
    pub amend: bool,
    /// The amend message was changed from HEAD's.
    pub edited: bool,
    pub summary_empty: bool,
    pub busy: bool,
    pub branch: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// The button's word.
    pub label: &'static str,
    /// Why it can't run yet, for its tooltip.
    pub blocked: Option<&'static str>,
    /// What it takes, for the tooltip when it can.
    pub target: String,
    /// Nothing staged: it stages everything first.
    pub stages_all: bool,
}

pub fn plan(f: Facts) -> Plan {
    let has_staged = f.staged > 0;
    let has_any = has_staged || f.stageable > 0;
    let blocked = if f.busy {
        Some("Committing…")
    } else if f.conflicts > 0 {
        Some("Resolve the conflicts first")
    } else if !f.amend && !has_any {
        Some("Nothing to commit")
    } else if f.summary_empty && (!f.amend || f.edited) {
        // An amend left unedited keeps its message.
        Some("Write a commit message first")
    } else {
        None
    };
    let label = if f.amend {
        "Amend"
    } else if has_staged {
        "Commit"
    } else {
        "Commit all"
    };
    let scope = if has_staged && !f.amend {
        format!("Commit {} staged", f.staged)
    } else {
        label.to_string()
    };
    let target = match f.branch {
        Some(b) => format!("{scope} to {b}"),
        None => scope,
    };
    Plan {
        label,
        blocked,
        target,
        stages_all: !has_staged && !f.amend,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_button_says_what_it_does() {
        let f = Facts {
            staged: 2,
            summary_empty: false,
            branch: Some("main"),
            ..Default::default()
        };
        assert_eq!(
            plan(f),
            Plan {
                label: "Commit",
                blocked: None,
                target: "Commit 2 staged to main".into(),
                stages_all: false
            }
        );
        let all = plan(Facts {
            stageable: 3,
            ..Default::default()
        });
        assert_eq!(
            (all.label, all.blocked, all.stages_all),
            ("Commit all", None, true)
        );
        assert_eq!(plan(Facts::default()).blocked, Some("Nothing to commit"));
        assert_eq!(
            plan(Facts {
                staged: 1,
                summary_empty: true,
                ..Default::default()
            })
            .blocked,
            Some("Write a commit message first")
        );
        assert_eq!(
            plan(Facts {
                staged: 1,
                conflicts: 1,
                ..Default::default()
            })
            .blocked,
            Some("Resolve the conflicts first")
        );
        // An untouched amend keeps HEAD's message.
        assert_eq!(
            plan(Facts {
                amend: true,
                summary_empty: true,
                ..Default::default()
            })
            .blocked,
            None
        );
        assert_eq!(
            plan(Facts {
                amend: true,
                summary_empty: true,
                edited: true,
                ..Default::default()
            })
            .blocked,
            Some("Write a commit message first")
        );
    }
}
