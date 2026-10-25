//! GitViber's own updates, on the endpoints, manifest and signatures tauri-plugin-updater used:
//! a Tauri-built GitViber updates to this one, and this one reads the same releases.
//!
//! [`check`] reads latest.json, [`Update::download`] fetches the bundle and checks its minisign
//! signature against [`PUBLIC_KEY`], [`Download::install`] puts it in place of the running .app or
//! AppImage, and [`relaunch`] starts the new one once this process has quit. Everything blocks:
//! run it off the UI thread.
//!
//! A debug build is off unless `GITVIBER_UPDATER=1`; `GITVIBER_UPDATE_ENDPOINT` replaces the
//! endpoints (a local latest.json). Neither can change the key a download must be signed with.

mod install;

use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use install::relaunch;

/// This build's version, the workspace's.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The updater key's public half, as tauri.conf.json had it: base64 of the minisign .pub file.
/// Only bundles signed by its private half install (RELEASING.md, "The updater key").
pub const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEE5M0QxQkUzMkVERDMyOTgKUldTWU10MHU0eHM5cWRkSUNhRmp2dHZ3cUlhZGZCajVTQjdTT0pIZk8zT0JOL3JmSWVTdXJDWVQK";

/// The download mirror first (packaging/update-proxy), GitHub second.
pub const ENDPOINTS: [&str; 2] = [
    "https://gitviber-updates.gitviber.workers.dev/latest.json",
    "https://github.com/emircan-sahin/gitviber/releases/latest/download/latest.json",
];

/// Where a .deb or .rpm install sends the user: those belong to the package manager.
pub const RELEASES_URL: &str = "https://github.com/emircan-sahin/gitviber/releases/latest";

/// A file a mirror failed to serve is fetched from here, under its release's tag.
const RELEASE_DOWNLOADS: &str = "https://github.com/emircan-sahin/gitviber/releases/download";

const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
// GitHub's asset host has been measured at 0.1 MB/s: a whole download gets far longer.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// How this install updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The app replaces itself: a .app, or an AppImage, in a folder the user can write.
    Install,
    /// The user downloads it from [`RELEASES_URL`]: a .deb or .rpm, or a folder that needs an
    /// administrator (we never ask for a password).
    Download,
}

/// How this install updates; None when it doesn't (a development build).
pub fn mode() -> Option<Mode> {
    if !enabled() {
        return None;
    }
    let Some(bundle) = install::bundle() else {
        // GITVIBER_UPDATER=1 outside a bundle: checks work, installing says it can't.
        return std::env::var_os("GITVIBER_UPDATER").map(|_| Mode::Install);
    };
    match bundle {
        install::Bundle::Packaged => Some(Mode::Download),
        bundle if install::writable(bundle.path()?) => Some(Mode::Install),
        _ => Some(Mode::Download),
    }
}

fn enabled() -> bool {
    !cfg!(debug_assertions) || std::env::var_os("GITVIBER_UPDATER").is_some()
}

/// A newer release than this build.
#[derive(Clone, Debug)]
pub struct Update {
    pub version: String,
    /// The release's CHANGELOG section (markdown).
    pub notes: String,
    /// RFC 3339, when the release was made.
    pub date: Option<String>,
    url: String,
    signature: String,
    public_key: String,
}

/// Asks the endpoints in order for latest.json; the first that answers decides.
pub fn check() -> Result<Option<Update>, Error> {
    let override_endpoint = std::env::var("GITVIBER_UPDATE_ENDPOINT").ok();
    let endpoints: Vec<&str> = match &override_endpoint {
        Some(url) => vec![url.as_str()],
        None => ENDPOINTS.to_vec(),
    };
    let current = semver::Version::parse(VERSION).map_err(|e| Error::Manifest(e.to_string()))?;
    let targets = install::targets(install::bundle());
    check_endpoints(&endpoints, &current, &targets, PUBLIC_KEY)
}

fn check_endpoints(
    endpoints: &[&str],
    current: &semver::Version,
    targets: &[String],
    public_key: &str,
) -> Result<Option<Update>, Error> {
    let agent = agent(CHECK_TIMEOUT);
    let mut last = Error::Network("no update endpoint".into());
    for url in endpoints {
        match fetch_manifest(&agent, url) {
            // An endpoint may say "nothing newer" with no body, as Tauri's servers can.
            Ok(None) => return Ok(None),
            Ok(Some(json)) => match parse(&json, current, targets, public_key) {
                Ok(update) => return Ok(update),
                Err(err) => last = err,
            },
            Err(err) => last = err,
        }
    }
    Err(last)
}

fn fetch_manifest(agent: &ureq::Agent, url: &str) -> Result<Option<String>, Error> {
    let mut res = agent
        .get(url)
        .header("Accept", "application/json")
        .call()
        .map_err(network)?;
    if res.status() == 204 {
        return Ok(None);
    }
    res.body_mut().read_to_string().map(Some).map_err(network)
}

/// latest.json as release.yml writes it (`version`, `notes`, `pub_date`, `platforms`).
#[derive(serde::Deserialize)]
struct Manifest {
    #[serde(alias = "name")]
    version: String,
    notes: Option<String>,
    pub_date: Option<String>,
    platforms: HashMap<String, Platform>,
}

#[derive(serde::Deserialize)]
struct Platform {
    url: String,
    signature: String,
}

/// The update `json` offers over `current`, from the first of `targets` it has.
fn parse(
    json: &str,
    current: &semver::Version,
    targets: &[String],
    public_key: &str,
) -> Result<Option<Update>, Error> {
    let manifest: Manifest =
        serde_json::from_str(json).map_err(|e| Error::Manifest(e.to_string()))?;
    let version = semver::Version::parse(manifest.version.trim_start_matches('v'))
        .map_err(|e| Error::Manifest(format!("version {:?}: {e}", manifest.version)))?;
    if version <= *current {
        return Ok(None);
    }
    let platform = targets
        .iter()
        .find_map(|t| manifest.platforms.get(t))
        .ok_or_else(|| {
            Error::Manifest(format!(
                "{version} has no download for {}",
                targets.join(" or ")
            ))
        })?;
    Ok(Some(Update {
        version: version.to_string(),
        notes: manifest.notes.unwrap_or_default(),
        date: manifest.pub_date,
        url: platform.url.clone(),
        signature: platform.signature.clone(),
        public_key: public_key.to_string(),
    }))
}

impl Update {
    /// Downloads and verifies it, calling `progress(received, total)` as bytes arrive. A
    /// mirror that fails is retried from GitHub.
    pub fn download(&self, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Download, Error> {
        let mut last = None;
        for url in self.sources() {
            match self.download_from(&url, &mut progress) {
                Ok(download) => return Ok(download),
                Err(err) => last = Some(err),
            }
        }
        Err(last.unwrap_or_else(|| Error::Manifest("no download URL".into())))
    }

    /// The URL's last segment, when it's a plain release file name.
    fn file_name(&self) -> Option<&str> {
        let name = self.url.rsplit('/').next()?;
        let ok = !name.is_empty()
            && !self.url.contains(['?', '#'])
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        ok.then_some(name)
    }

    /// The manifest's URL, then the same file on GitHub when that's elsewhere.
    fn sources(&self) -> Vec<String> {
        let mut out = vec![self.url.clone()];
        if let Some(name) = self.file_name() {
            let github = format!("{RELEASE_DOWNLOADS}/v{}/{name}", self.version);
            if github != self.url {
                out.push(github);
            }
        }
        out
    }

    fn download_from(
        &self,
        url: &str,
        progress: &mut impl FnMut(u64, Option<u64>),
    ) -> Result<Download, Error> {
        let (key, signature) = decode(&self.public_key, &self.signature)?;
        let mut verifier = key
            .verify_stream(&signature)
            .map_err(|e| Error::Signature(e.to_string()))?;
        let name = self.file_name().unwrap_or("update");
        // Removed again when `download` is dropped, here on any error.
        let download = Download {
            path: std::env::temp_dir()
                .join(format!("gitviber-update-{}-{name}", std::process::id())),
        };
        let mut file = std::fs::File::create(&download.path)?;
        let mut res = agent(DOWNLOAD_TIMEOUT)
            .get(url)
            .header("Accept", "application/octet-stream")
            .call()
            .map_err(network)?;
        let total = res.body().content_length();
        let mut reader = res.body_mut().as_reader();
        let mut buf = vec![0; 256 * 1024];
        let mut received = 0u64;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| Error::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            verifier.update(&buf[..n]);
            file.write_all(&buf[..n])?;
            received += n as u64;
            progress(received, total);
        }
        file.sync_all()?;
        verifier
            .finalize()
            .map_err(|e| Error::Signature(e.to_string()))?;
        Ok(download)
    }
}

/// A verified update on disk, deleted when dropped.
#[derive(Debug)]
pub struct Download {
    path: PathBuf,
}

impl Download {
    /// Puts the update in place of this install; [`relaunch`] then starts it. Leaves the
    /// download as it was when it fails, so it can be tried again.
    pub fn install(&self) -> Result<(), Error> {
        match install::bundle() {
            Some(install::Bundle::App(app)) => install::install_app(&self.path, &app),
            Some(install::Bundle::AppImage(image)) => install::install_appimage(&self.path, &image),
            _ => Err(Error::NotInstalled),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Download {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The key and a release's signature, both base64 of minisign's text files, as tauri-plugin-updater
/// read them.
fn decode(
    public_key: &str,
    signature: &str,
) -> Result<(minisign_verify::PublicKey, minisign_verify::Signature), Error> {
    let text = |b64: &str| {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| Error::Signature(e.to_string()))?;
        String::from_utf8(bytes).map_err(|e| Error::Signature(e.to_string()))
    };
    let key = minisign_verify::PublicKey::decode(&text(public_key)?)
        .map_err(|e| Error::Signature(format!("the update key: {e}")))?;
    let signature = minisign_verify::Signature::decode(&text(signature)?)
        .map_err(|e| Error::Signature(e.to_string()))?;
    Ok((key, signature))
}

/// Whether `data` carries `signature` by `public_key` (both base64, as in latest.json).
pub fn verify(data: &[u8], signature: &str, public_key: &str) -> Result<(), Error> {
    let (key, signature) = decode(public_key, signature)?;
    key.verify(data, &signature, true)
        .map_err(|e| Error::Signature(e.to_string()))
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(CHECK_TIMEOUT))
        .timeout_recv_response(Some(CHECK_TIMEOUT))
        .timeout_global(Some(timeout))
        .user_agent(format!("GitViber/{VERSION}"))
        .build()
        .into()
}

fn network(err: ureq::Error) -> Error {
    Error::Network(err.to_string())
}

#[derive(Debug)]
pub enum Error {
    /// Offline, a timeout, an error status: worth trying again later, not worth a log.
    Network(String),
    /// latest.json didn't parse, or has nothing for this platform.
    Manifest(String),
    /// The download isn't signed by the release key.
    Signature(String),
    /// The app's folder needs an administrator to change; the user downloads it instead.
    NotWritable(PathBuf),
    /// Not running from an installed .app or AppImage.
    NotInstalled,
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Network(e) => write!(f, "Couldn't reach the update server: {e}"),
            Error::Manifest(e) => write!(f, "The update server's answer is off: {e}"),
            Error::Signature(e) => write!(f, "The update isn't signed by GitViber's key: {e}"),
            Error::NotWritable(dir) => write!(
                f,
                "GitViber can't replace itself in {}: download the update instead",
                dir.display()
            ),
            Error::NotInstalled => write!(f, "Only an installed GitViber updates itself"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

#[cfg(test)]
mod tests;
