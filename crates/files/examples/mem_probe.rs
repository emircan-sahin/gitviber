//! Footprint of reading a big file and HEAD's version again and again, with no view: whether
//! memory a closed file tab gave back stays with the process.
//!
//! files-mem-probe REPO PATH [N], PROBE=read|pair|rows to try one step. With macOS's
//! malloc large cache on, freed multi-MB blocks stay in the footprint; MallocLargeCache=0 keeps
//! it flat.

use std::sync::Arc;

use gitviber_core::{Core, api};

fn main() {
    let mut args = std::env::args().skip(1);
    let repo = args.next().unwrap_or_else(|| ".".into());
    let path = args.next().unwrap_or_else(|| "src/big.rs".into());
    let n: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(15);
    let core = Arc::new(Core::new(Arc::new(|_| {})));
    api::repo::open_repo(&core, repo).expect("a repo");
    let fp = || {
        gitviber_ui::memory::footprint()
            .map(gitviber_ui::memory::format_bytes)
            .unwrap_or_default()
    };
    eprintln!("probe: before {}", fp());
    for i in 0..n {
        let path = path.clone();
        let mode = std::env::var("PROBE").unwrap_or_default();
        match mode.as_str() {
            "read" => drop(api::files::read_file(&core, path)),
            "pair" => drop(api::files::diff_pair(
                &core,
                "worktree".into(),
                path,
                None,
                None,
                None,
                None,
            )),
            "rows" => {
                let t = std::fs::read_to_string(
                    std::path::Path::new(&std::env::args().nth(1).unwrap()).join(&path),
                )
                .unwrap();
                let head = format!("// x\n{t}");
                drop(gitviber_core::diff::rows(
                    &head,
                    &t,
                    gitviber_core::diff::whitespace_mode(None),
                ));
            }
            _ => {
                let pair =
                    api::files::diff_pair(&core, "worktree".into(), path, None, None, None, None)
                        .unwrap();
                let rope = gpui_kit::component::input::Rope::from(pair.modified.text.as_str());
                drop(pair);
                drop(rope);
            }
        }
        eprintln!("probe: {i} {}", fp());
    }
}
