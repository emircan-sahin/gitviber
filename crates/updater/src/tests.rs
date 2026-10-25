use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use base64::Engine as _;

use super::*;
use crate::install::{app_of, install_app, install_appimage, single_bundle, swap_in};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

// A throwaway key made with `tauri signer generate` (CLI 2.11.4), and its `tauri signer sign`
// of FIXTURE: what the Tauri-built releases carry.
const TAURI_TEST_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDhGNDk0OENBREM3NTBCM0UKUldRK0MzWGN5a2hKajEwdkFGcjFVelNUcTFwcXN0NERuNWRuZ3pWKzdxUStZd1BKUGVvbXA5aysK";
const TAURI_TEST_SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRK0MzWGN5a2hKancvZU5Nb3BOTGlIODNJNjhWN2VSVFl1a0tiTDlMU3RTbFQzeHk0TllMQ1oxT1ZHb2pzOTNKWUZtTEg1ZVFvNkJuZVN5RE9mN2h1RGxlR0FmWDViQXdVPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyODQ5MDUxCWZpbGU6Zml4dHVyZS50eHQKaEtXT1VoTmsvQkU1bHRTMitHUDZWRWFHaExqdWxscHY3a3pUV0Y1YUpMNXREaUphaHhmNTBIVW9ha3B4bjlnVUp4T09pWlZJdk5QSTRVNkxsWUs4Q1E9PQo=";
const FIXTURE: &[u8] = b"GitViber updater fixture\n";

/// A fresh key pair as `tauri signer generate` stores them (base64 of minisign's text files).
struct Key {
    public: String,
    secret: minisign::SecretKey,
}

fn key() -> Key {
    let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    Key {
        public: B64.encode(pair.pk.to_box().unwrap().to_string()),
        secret: pair.sk,
    }
}

/// `tauri signer sign`'s .sig for `data`.
fn sign(key: &Key, data: &[u8]) -> String {
    let sig = minisign::sign(
        None,
        &key.secret,
        data,
        Some("timestamp:1792849051\tfile:test"),
        Some("signature from tauri secret key"),
    )
    .unwrap();
    B64.encode(sig.to_string())
}

fn v(s: &str) -> semver::Version {
    semver::Version::parse(s).unwrap()
}

fn mac_targets() -> Vec<String> {
    vec!["darwin-aarch64-app".into(), "darwin-aarch64".into()]
}

fn manifest(version: &str) -> String {
    let entry = |file: &str| {
        serde_json::json!({
            "signature": "c2ln",
            "url": format!("https://github.com/emircan-sahin/gitviber/releases/download/v{version}/{file}"),
        })
    };
    serde_json::json!({
        "version": version,
        "notes": "### Fixed\n- **A fix**.",
        "pub_date": "2026-10-17T23:34:20.331Z",
        "platforms": {
            "darwin-aarch64": entry("GitViber_universal.app.tar.gz"),
            "darwin-aarch64-app": entry(&format!("GitViber_{version}_universal.app.tar.gz")),
            "linux-x86_64": entry(&format!("GitViber_{version}_amd64.AppImage")),
        },
    })
    .to_string()
}

#[test]
fn the_manifest_offers_only_a_newer_version_for_the_bundles_own_key_first() {
    let update = parse(&manifest("0.1.18"), &v("0.1.17"), &mac_targets(), "k")
        .unwrap()
        .unwrap();
    assert_eq!(update.version, "0.1.18");
    assert_eq!(update.notes, "### Fixed\n- **A fix**.");
    assert_eq!(update.date.as_deref(), Some("2026-10-17T23:34:20.331Z"));
    assert!(
        update
            .url
            .ends_with("/v0.1.18/GitViber_0.1.18_universal.app.tar.gz")
    );
    for same_or_older in ["0.1.17", "0.1.9", "0.1.17-rc.1"] {
        assert!(
            parse(&manifest(same_or_older), &v("0.1.17"), &mac_targets(), "k")
                .unwrap()
                .is_none(),
            "{same_or_older}"
        );
    }
    // A prerelease is older than its release, newer than the one before.
    assert!(
        parse(&manifest("v0.1.18"), &v("0.1.18-rc.1"), &mac_targets(), "k")
            .unwrap()
            .is_some()
    );
}

#[test]
fn the_manifest_without_this_platform_or_unreadable_is_an_error() {
    let linux_deb = vec!["linux-aarch64".to_string()];
    let err = parse(&manifest("0.2.0"), &v("0.1.17"), &linux_deb, "k").unwrap_err();
    assert!(
        matches!(err, Error::Manifest(ref m) if m.contains("linux-aarch64")),
        "{err}"
    );
    for bad in ["", "{}", r#"{"version":"x","platforms":{}}"#, "<html>"] {
        assert!(
            matches!(
                parse(bad, &v("0.1.17"), &mac_targets(), "k"),
                Err(Error::Manifest(_))
            ),
            "{bad}"
        );
    }
}

#[test]
fn the_real_0_1_17_manifest_parses() {
    // latest.json as release.yml published it for 0.1.17 (notes cut short).
    let json = r#"{"version":"0.1.17","notes":"x","pub_date":"2026-10-17T23:34:20.331Z","platforms":{"darwin-aarch64":{"signature":"s","url":"https://github.com/emircan-sahin/gitviber/releases/download/v0.1.17/GitViber_0.1.17_universal.app.tar.gz"},"darwin-aarch64-app":{"signature":"s","url":"u"},"darwin-x86_64":{"signature":"s","url":"u"},"darwin-x86_64-app":{"signature":"s","url":"u"},"linux-x86_64":{"signature":"s","url":"u"},"linux-x86_64-appimage":{"signature":"s","url":"u"},"linux-x86_64-deb":{"signature":"s","url":"u"},"linux-x86_64-rpm":{"signature":"s","url":"u"}}}"#;
    for targets in [
        mac_targets(),
        vec!["linux-x86_64-appimage".into(), "linux-x86_64".into()],
    ] {
        assert!(parse(json, &v("0.1.16"), &targets, "k").unwrap().is_some());
    }
}

#[test]
fn a_signature_by_the_tauri_cli_verifies_and_nothing_else_does() {
    verify(FIXTURE, TAURI_TEST_SIG, TAURI_TEST_KEY).unwrap();
    assert!(matches!(
        verify(
            b"GitViber updater fixture!\n",
            TAURI_TEST_SIG,
            TAURI_TEST_KEY
        ),
        Err(Error::Signature(_))
    ));
    assert!(matches!(
        verify(FIXTURE, TAURI_TEST_SIG, &key().public),
        Err(Error::Signature(_))
    ));
    assert!(matches!(
        verify(FIXTURE, "not base64", TAURI_TEST_KEY),
        Err(Error::Signature(_))
    ));
}

#[test]
fn our_signer_makes_what_the_tauri_updater_accepts() {
    // tauri-plugin-updater's check: minisign-verify with legacy allowed, over the whole file.
    let key = key();
    let data = b"an update".repeat(10_000);
    let sig = sign(&key, &data);
    verify(&data, &sig, &key.public).unwrap();
    let text = String::from_utf8(B64.decode(&sig).unwrap()).unwrap();
    assert!(text.starts_with("untrusted comment: signature from tauri secret key\n"));
    assert!(text.contains("\ntrusted comment: timestamp:"));
    // Prehashed ("ED"), which the stream check in `download` needs.
    let (pk, sig) = decode(&key.public, &sig).unwrap();
    let mut stream = pk.verify_stream(&sig).unwrap();
    for chunk in data.chunks(4096) {
        stream.update(chunk);
    }
    stream.finalize().unwrap();
}

#[test]
fn the_release_key_is_the_one_the_tauri_builds_have() {
    // 0.1.17 and earlier install only what this key signed (their tauri.conf.json): a new key here
    // alone would strand them, so it changes with them or not at all.
    const TAURI_BUILDS_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEE5M0QxQkUzMkVERDMyOTgKUldTWU10MHU0eHM5cWRkSUNhRmp2dHZ3cUlhZGZCajVTQjdTT0pIZk8zT0JOL3JmSWVTdXJDWVQK";
    assert_eq!(PUBLIC_KEY, TAURI_BUILDS_KEY);
    decode(PUBLIC_KEY, TAURI_TEST_SIG).unwrap();
}

/// Serves `routes` (path → status, body) over HTTP/1.1 on localhost, one request per connection.
fn serve(routes: Vec<(&'static str, u16, Vec<u8>)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut head = Vec::new();
            let mut byte = [0u8];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&head);
            let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
            let (status, body) = routes
                .iter()
                .find(|(p, ..)| *p == path)
                .map(|(_, s, b)| (*s, b.clone()))
                .unwrap_or((404, b"nope".to_vec()));
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    format!("http://{addr}")
}

/// The served file, with a signature over `signed`.
fn served_update(base: &str, key: &Key, signed: &[u8]) -> Update {
    Update {
        version: "9.9.9".into(),
        notes: String::new(),
        date: None,
        url: format!("{base}/GitViber_9.9.9_universal.app.tar.gz"),
        signature: sign(key, signed),
        public_key: key.public.clone(),
    }
}

#[test]
fn a_download_is_kept_only_when_its_signature_matches() {
    let key = key();
    let data = b"bundle bytes ".repeat(50_000);
    let base = serve(vec![(
        "/GitViber_9.9.9_universal.app.tar.gz",
        200,
        data.clone(),
    )]);
    let update = served_update(&base, &key, &data);
    let mut seen = Vec::new();
    let download = update
        .download(|got, total| seen.push((got, total)))
        .unwrap();
    assert_eq!(std::fs::read(download.path()).unwrap(), data);
    assert_eq!(
        seen.last(),
        Some(&(data.len() as u64, Some(data.len() as u64)))
    );
    let path = download.path().to_path_buf();
    drop(download);
    assert!(!path.exists(), "a dropped download is deleted");

    // Signed for other bytes: refused, from here and from the GitHub fallback alike.
    let bad = served_update(&base, &key, b"something else");
    let err = bad.download(|_, _| {}).unwrap_err();
    assert!(
        matches!(err, Error::Signature(_) | Error::Network(_)),
        "{err}"
    );
    assert!(
        !std::env::temp_dir()
            .join(format!(
                "gitviber-update-{}-GitViber_9.9.9_universal.app.tar.gz",
                std::process::id()
            ))
            .exists()
    );
}

#[test]
fn the_first_endpoint_that_answers_decides() {
    let base = serve(vec![
        ("/down", 500, b"oops".to_vec()),
        ("/latest.json", 200, manifest("0.3.0").into_bytes()),
        ("/none", 204, Vec::new()),
    ]);
    let down = format!("{base}/down");
    let ok = format!("{base}/latest.json");
    let none = format!("{base}/none");
    let found = check_endpoints(&[&down, &ok], &v("0.1.17"), &mac_targets(), "k").unwrap();
    assert_eq!(found.unwrap().version, "0.3.0");
    assert!(
        check_endpoints(&[&none, &ok], &v("0.1.17"), &mac_targets(), "k")
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        check_endpoints(&[&down], &v("0.1.17"), &mac_targets(), "k"),
        Err(Error::Network(_))
    ));
}

#[test]
fn a_mirror_download_falls_back_to_the_same_file_on_github() {
    let update = Update {
        version: "0.1.18".into(),
        notes: String::new(),
        date: None,
        url: "https://gitviber-updates.gitviber.workers.dev/download/v0.1.18/GitViber_0.1.18_universal.app.tar.gz".into(),
        signature: String::new(),
        public_key: String::new(),
    };
    assert_eq!(
        update.sources()[1],
        "https://github.com/emircan-sahin/gitviber/releases/download/v0.1.18/GitViber_0.1.18_universal.app.tar.gz"
    );
    let github = Update {
        url: update.sources()[1].clone(),
        ..update.clone()
    };
    assert_eq!(github.sources().len(), 1);
    let odd = Update {
        url: "https://x/a?b=../c".into(),
        ..update
    };
    assert_eq!(odd.sources().len(), 1);
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitviber-updater-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fake_app(dir: &Path, marker: &str) -> PathBuf {
    let app = dir.join("GitViber.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(app.join("Contents/Info.plist"), marker).unwrap();
    std::fs::write(app.join("Contents/MacOS/gitviber"), marker).unwrap();
    app
}

fn tar_gz(from: &Path, entries: &[&str], out: &Path) {
    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(out)
        .arg("-C")
        .arg(from)
        .args(entries)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn an_app_update_replaces_the_bundle_and_leaves_nothing_behind() {
    let dir = scratch("app");
    let installed = dir.join("Applications");
    std::fs::create_dir_all(&installed).unwrap();
    let app = fake_app(&installed, "old");
    std::fs::write(app.join("Contents/stale"), "gone after").unwrap();
    let build = dir.join("build");
    fake_app(&build, "new");
    let archive = dir.join("GitViber_9.9.9_universal.app.tar.gz");
    tar_gz(&build, &["GitViber.app"], &archive);

    install_app(&archive, &app).unwrap();
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap(),
        "new"
    );
    assert!(!app.join("Contents/stale").exists());
    let left: Vec<_> = std::fs::read_dir(&installed)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["GitViber.app"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_archive_that_isnt_one_bundle_leaves_the_app_alone() {
    let dir = scratch("bad");
    let installed = dir.join("Applications");
    std::fs::create_dir_all(&installed).unwrap();
    let app = fake_app(&installed, "old");
    let build = dir.join("build");
    fake_app(&build, "new");
    std::fs::write(build.join("extra"), "x").unwrap();
    let archive = dir.join("two.tar.gz");
    tar_gz(&build, &["GitViber.app", "extra"], &archive);
    assert!(install_app(&archive, &app).is_err());
    assert!(single_bundle(&build).is_err());
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap(),
        "old"
    );
    assert_eq!(std::fs::read_dir(&installed).unwrap().count(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_swap_puts_the_old_bundle_back() {
    let dir = scratch("swap");
    let app = fake_app(&dir, "old");
    assert!(swap_in(&dir.join("missing.app"), &app).is_err());
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap(),
        "old"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_appimage_update_keeps_the_images_mode() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("appimage");
    let image = dir.join("GitViber_0.1.17_amd64.AppImage");
    std::fs::write(&image, "old").unwrap();
    std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o755)).unwrap();
    let download = dir.join("download");
    std::fs::write(&download, "new").unwrap();
    std::fs::set_permissions(&download, std::fs::Permissions::from_mode(0o600)).unwrap();
    install_appimage(&download, &image).unwrap();
    assert_eq!(std::fs::read_to_string(&image).unwrap(), "new");
    assert_eq!(
        std::fs::metadata(&image).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_bundle_is_found_from_its_executable_only() {
    let app = Path::new("/Applications/GitViber.app");
    assert_eq!(
        app_of(&app.join("Contents/MacOS/gitviber")).as_deref(),
        Some(app)
    );
    assert_eq!(app_of(Path::new("/repo/target/release/gitviber")), None);
    assert_eq!(
        app_of(Path::new("/x/GitViber/Contents/MacOS/gitviber")),
        None
    );
}

#[test]
fn targets_put_the_bundle_kind_first() {
    let arch = std::env::consts::ARCH;
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    if cfg!(target_os = "macos") {
        let app = install::Bundle::App("/Applications/GitViber.app".into());
        assert_eq!(
            install::targets(Some(app)),
            [format!("{os}-{arch}-app"), format!("{os}-{arch}")]
        );
    } else {
        let image = install::Bundle::AppImage("/x.AppImage".into());
        assert_eq!(
            install::targets(Some(image)),
            [format!("{os}-{arch}-appimage"), format!("{os}-{arch}")]
        );
        assert_eq!(
            install::targets(Some(install::Bundle::Packaged)),
            [format!("{os}-{arch}")]
        );
    }
}

/// A published bundle against the release key: GITVIBER_REAL_ARCHIVE=<file>, its .sig beside
/// it (GITVIBER_VERIFY_KEY, base64, checks another key's instead).
/// `cargo nextest run -p gitviber-updater --run-ignored only`.
#[test]
#[ignore = "needs a downloaded release"]
fn a_published_file_verifies_against_the_release_key() {
    let archive = std::env::var("GITVIBER_REAL_ARCHIVE").expect("GITVIBER_REAL_ARCHIVE");
    let key = std::env::var("GITVIBER_VERIFY_KEY").unwrap_or_else(|_| PUBLIC_KEY.into());
    let data = std::fs::read(&archive).unwrap();
    let sig = std::fs::read_to_string(format!("{archive}.sig")).unwrap();
    verify(&data, &sig, &key).unwrap();
    assert!(verify(&data[1..], &sig, &key).is_err());
}
