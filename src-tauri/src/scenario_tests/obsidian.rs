//! Obsidian vaults as a user's real ones are: odd names, links in and out, big folders, odd files.
//! Every vault here is a temp folder; nothing reads or writes Obsidian's own config.

use super::*;
use crate::obsidian::{
    list_dir, list_files, parse_vaults, read_file, read_media, resolve, write_file,
};
use std::time::Instant;

fn vault(sb: &Sandbox) -> PathBuf {
    let v = sb.path("Vault");
    fs::create_dir_all(v.join(".obsidian/plugins/x")).unwrap();
    fs::write(v.join(".obsidian/plugins/x/main.js"), "code").unwrap();
    fs::create_dir_all(v.join("Daily")).unwrap();
    fs::write(v.join("Daily/Today.md"), "# Today\n").unwrap();
    v
}

#[test]
fn odd_note_names_list_read_and_save() {
    let sb = Sandbox::new("vault-names");
    let v = vault(&sb);
    let names = [
        "Note #1.md",
        "x^y.md",
        "[[weird]].md",
        "🚀 Launch.md",
        "with space/inner note.md",
        "Price $5 & 'quotes'.md",
        "Çalışma Notları.md",
    ];
    for n in names {
        let p = v.join(n);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, format!("# {n}\n")).unwrap();
        assert!(write_file(&v, n, "saved\n").is_ok(), "{n}");
        assert_eq!(read_file(&v, n).text, "saved\n", "{n}");
    }
    #[cfg(unix)]
    {
        fs::write(v.join("a|b.md"), "pipe").unwrap();
        assert_eq!(read_file(&v, "a|b.md").text, "pipe");
    }
    let files = list_files(&v).unwrap();
    for n in names {
        assert!(files.iter().any(|f| f == n), "{n} in {files:?}");
    }
    assert!(!files.iter().any(|f| f.contains(".obsidian")));
}

/// macOS keeps a name as it was typed (often decomposed); the page asks with the composed form.
#[cfg(target_os = "macos")]
#[test]
fn decomposed_names_are_found_by_their_composed_form() {
    let sb = Sandbox::new("vault-nfd");
    let v = vault(&sb);
    let nfd = "O\u{308}dev/Re\u{301}sume\u{301}.md";
    fs::create_dir_all(v.join("O\u{308}dev")).unwrap();
    fs::write(v.join(nfd), "nfd").unwrap();
    assert_eq!(read_file(&v, "\u{d6}dev/R\u{e9}sum\u{e9}.md").text, "nfd");
    // Listed as stored: links normalize both sides (lib/obsidian/links).
    assert!(list_files(&v).unwrap().iter().any(|f| f == nfd));
}

#[test]
fn traversal_and_hidden_paths_are_refused_for_reads_and_writes() {
    let sb = Sandbox::new("vault-escape");
    let v = vault(&sb);
    let outside = sb.path("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.md"), "secret").unwrap();
    for bad in [
        "../outside/secret.md",
        "Daily/../../outside/secret.md",
        "/etc/passwd",
        ".obsidian/plugins/x/main.js",
        ".obsidian/app.json",
        "Daily/.hidden.md",
        ".git/config",
    ] {
        assert!(resolve(&v, bad).is_err(), "{bad}");
        assert!(write_file(&v, bad, "pwned").is_err(), "{bad}");
        assert!(!read_file(&v, bad).exists, "{bad}");
        assert!(read_media(&v, bad).is_err(), "{bad}");
    }
    assert_eq!(
        fs::read_to_string(outside.join("secret.md")).unwrap(),
        "secret"
    );
    assert_eq!(
        fs::read_to_string(v.join(".obsidian/plugins/x/main.js")).unwrap(),
        "code"
    );
}

#[cfg(unix)]
#[test]
fn links_out_of_the_vault_or_into_its_config_are_refused() {
    use std::os::unix::fs::symlink;
    let sb = Sandbox::new("vault-links");
    let v = vault(&sb);
    let outside = sb.path("outside");
    fs::create_dir_all(outside.join(".ssh")).unwrap();
    fs::write(outside.join("secret.md"), "secret").unwrap();
    symlink(outside.join("secret.md"), v.join("Leak.md")).unwrap();
    symlink(&outside, v.join("Out")).unwrap();
    symlink(outside.join(".ssh"), v.join("Keys")).unwrap();
    symlink(v.join(".obsidian/plugins"), v.join("Plugins")).unwrap();
    symlink(v.join("nowhere.md"), v.join("Dangling.md")).unwrap();
    // A chain: inside → inside → out.
    symlink(v.join("Out"), v.join("Daily/Hop")).unwrap();
    for bad in [
        "Leak.md",
        "Out/secret.md",
        "Out/new.md",
        "Keys/id",
        "Plugins/x/main.js",
        "Dangling.md",
        "Daily/Hop/secret.md",
    ] {
        assert!(write_file(&v, bad, "pwned").is_err(), "{bad}");
        assert!(!read_file(&v, bad).exists, "{bad}");
    }
    assert!(!outside.join("new.md").exists());
    assert!(!v.join("nowhere.md").exists());
    assert_eq!(
        fs::read_to_string(outside.join("secret.md")).unwrap(),
        "secret"
    );
    let listed: Vec<String> = list_dir(&v, "")
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    for hidden in [
        "Leak.md",
        "Out",
        "Keys",
        "Plugins",
        "Dangling.md",
        ".obsidian",
    ] {
        assert!(
            !listed.iter().any(|n| n == hidden),
            "{hidden} in {listed:?}"
        );
    }
    let files = list_files(&v).unwrap();
    assert!(
        !files
            .iter()
            .any(|f| f.contains("secret") || f.contains("main.js")),
        "{files:?}"
    );
    // A folder linking back up isn't walked forever.
    symlink(&v, v.join("Daily/Loop")).unwrap();
    assert!(list_files(&v).unwrap().len() < 10);
}

#[cfg(unix)]
#[test]
fn a_vault_opened_through_a_symlink_or_inside_a_hidden_folder_works() {
    let sb = Sandbox::new("vault-root-link");
    let real = sb.path(".config/notes/Real Vault");
    fs::create_dir_all(real.join("Daily")).unwrap();
    fs::write(real.join("Daily/Today.md"), "# Today\n").unwrap();
    let link = sb.path("Vault Link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    for root in [&real, &link] {
        assert!(write_file(root, "Daily/Today.md", "# Saved\n").is_ok());
        assert_eq!(
            fs::read_to_string(real.join("Daily/Today.md")).unwrap(),
            "# Saved\n"
        );
        fs::write(real.join("Daily/Today.md"), "# Today\n").unwrap();
        assert_eq!(read_file(root, "Daily/Today.md").text, "# Today\n");
        assert_eq!(list_files(root).unwrap(), ["Daily/Today.md"]);
        assert_eq!(list_dir(root, "").unwrap().len(), 1);
    }
}

#[test]
fn saving_where_the_note_or_its_folder_went_away() {
    let sb = Sandbox::new("vault-save-gone");
    let v = vault(&sb);
    // Folder deleted (in Obsidian or by sync) while the tab was open: nothing is recreated.
    fs::remove_dir_all(v.join("Daily")).unwrap();
    assert!(write_file(&v, "Daily/Today.md", "lost?").is_err());
    assert!(!v.join("Daily").exists());
    // The name now a folder.
    fs::create_dir_all(v.join("Now a folder.md")).unwrap();
    assert!(write_file(&v, "Now a folder.md", "x").is_err());
    // The vault root itself isn't a file to write.
    assert!(write_file(&v, "", "x").is_err());
    assert!(list_dir(&v, "Missing").is_err());
}

#[test]
fn huge_binary_and_odd_files_are_not_read_as_notes() {
    let sb = Sandbox::new("vault-odd-files");
    let v = vault(&sb);
    // A 50 MB note, as an export or a pasted log can be: refused without reading it.
    let big = fs::File::create(v.join("Huge.md")).unwrap();
    big.set_len(50 * 1024 * 1024).unwrap();
    let started = Instant::now();
    let f = read_file(&v, "Huge.md");
    assert!(f.too_large && f.exists && f.text.is_empty());
    assert!(started.elapsed().as_secs() < 2);
    // An image named like a note, and a Bases file (YAML).
    fs::write(v.join("Pasted.md"), b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
    assert!(read_file(&v, "Pasted.md").binary);
    fs::write(
        v.join("Books.base"),
        "views:\n  - type: table\n    name: All\n",
    )
    .unwrap();
    let base = read_file(&v, "Books.base");
    assert!(!base.binary && base.text.starts_with("views:"));
    // UTF-16 shows, but is marked so the editor never writes it back as UTF-8.
    fs::write(v.join("Wide.md"), [0xFF, 0xFE, b'h', 0, b'i', 0]).unwrap();
    let wide = read_file(&v, "Wide.md");
    assert!(wide.lossy && wide.text == "hi");
    #[cfg(unix)]
    {
        // A FIFO would hang a read.
        let fifo = v.join("pipe.md");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o644) }, 0);
        assert!(!read_file(&v, "pipe.md").exists);
        assert!(read_media(&v, "pipe.md").is_err());
        assert!(!list_files(&v).unwrap().iter().any(|f| f == "pipe.md"));
    }
}

#[test]
fn a_twenty_thousand_note_vault_lists_quickly() {
    let sb = Sandbox::new("vault-big");
    let v = vault(&sb);
    for d in 0..200 {
        let dir = v.join(format!("Area {d}"));
        fs::create_dir_all(&dir).unwrap();
        for n in 0..100 {
            fs::write(dir.join(format!("Note {n}.md")), "").unwrap();
        }
    }
    // A plugin's cache and a git folder with as many files aren't walked.
    fs::create_dir_all(v.join(".git/objects")).unwrap();
    for n in 0..2000 {
        fs::write(v.join(format!(".git/objects/{n}")), "").unwrap();
    }
    let started = Instant::now();
    let files = list_files(&v).unwrap();
    let took = started.elapsed();
    assert_eq!(files.len(), 20_001);
    assert!(took.as_secs_f64() < 3.0, "{took:?}");
}

#[test]
fn obsidian_json_as_it_is_found_in_the_wild() {
    // Obsidian's own shape, plus what an older version, a hand edit or a sync tool leaves.
    let json = r#"{
        "vaults": {
            "a": {"path": "/notes/Work/", "ts": 1700000000000, "open": true},
            "b": {"path": "~/Notes", "ts": 1},
            "c": {"path": "/", "ts": 2},
            "d": {"path": "", "ts": 3},
            "e": {"path": 42},
            "f": {"path": "/notes/Float", "ts": 1.7e12, "open": "yes"},
            "g": "not an object"
        },
        "updateDisabled": true
    }"#;
    let vaults = parse_vaults(json);
    let by = |id: &str| vaults.iter().find(|v| v.id == id);
    assert_eq!(by("a").unwrap().name, "Work");
    assert!(by("c").is_none() && by("d").is_none() && by("e").is_none() && by("g").is_none());
    // Kept as written; vaults() drops what isn't a folder ("~" is never expanded).
    assert_eq!(by("b").unwrap().path, "~/Notes");
    let f = by("f").unwrap();
    assert!(!f.open && f.ts == 0);
    for broken in [
        "",
        "{",
        "null",
        "[]",
        r#"{"vaults": []}"#,
        r#"{"vaults": null}"#,
        "\u{feff}{}",
    ] {
        assert!(parse_vaults(broken).is_empty(), "{broken}");
    }
}
