use crate::journal;
use crate::state::{Core, Res};

pub fn journal(core: &Core) -> Res<journal::View> {
    let journal = core.journal.clone();
    core.read_repo(move |r| Ok(journal.view(r)))
}

pub fn journal_last(core: &Core) -> Res<Option<u64>> {
    Ok(core.journal.last(&core.repo()?))
}

/// `id`: the entry the user means; refused if it is no longer the next one.
pub fn undo(core: &Core, id: Option<u64>) -> Res<journal::EntryView> {
    step(core, false, id)
}

pub fn redo(core: &Core, id: Option<u64>) -> Res<journal::EntryView> {
    step(core, true, id)
}

fn step(core: &Core, forward: bool, id: Option<u64>) -> Res<journal::EntryView> {
    let (journal, index) = (core.journal.clone(), core.index.clone());
    core.in_repo(move |r| journal.step(r, forward, id, &index))
}
