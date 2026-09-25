//! Every error, in one file in the app's log folder (~/Library/Logs/app.gitviber.desktop on
//! macOS), and on the terminal running `pnpm tauri dev`: the page's (src/lib/app/errorLog.ts) and
//! Rust panics. The webview's console is rarely open, so an error there went unnoticed.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// Past this the file becomes errors.old.log and starts over, so it can't grow without bound.
const MAX_BYTES: u64 = 1 << 20;

pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    let _ = FILE.set(dir.join("errors.log"));
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write(
            "rust panic",
            &format!("{info}\n{}", std::backtrace::Backtrace::force_capture()),
        );
        default(info);
    }));
}

/// The log file, once `init` has run; it may not exist before the first error.
pub fn file() -> Option<&'static Path> {
    FILE.get().map(PathBuf::as_path)
}

pub fn write(source: &str, message: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let line = redact(&format!(
        "[{}] {source}: {}\n",
        utc(now),
        message.trim_end()
    ));
    eprint!("{line}");
    let Some(path) = FILE.get() else { return };
    // Checked on every write, not only at launch: every error toast lands here too.
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_file_name("errors.old.log"));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// GitHub's token prefixes. A real token has 30+ characters after one; asking for 20 leaves a
/// branch like `ghp_fix-login` alone.
const TOKENS: [&str; 6] = ["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"];

/// Credentials never reach the log: a URL's `user:token@` (git prints remote URLs as they are
/// configured) and GitHub tokens, wherever a message quotes them.
fn redact(text: &str) -> String {
    hide_tokens(&hide_logins(text))
}

/// `https://user:token@host/…` → `https://***@host/…`.
fn hide_logins(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        let host_end = tail
            .find(|c: char| c.is_whitespace() || "/?#'\"<>`".contains(c))
            .unwrap_or(tail.len());
        rest = match tail[..host_end].rfind('@') {
            Some(at) => {
                out.push_str("***");
                &tail[at..]
            }
            None => tail,
        };
    }
    out + rest
}

/// `ghp_abc…` → `ghp_***`. One pass: every prefix starts with a `g`, and a token's body is
/// skipped once read, so a 1 MB hook output doesn't stall the log.
fn hide_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut from) = (0, 0);
    while let Some(g) = text[from..].find('g').map(|i| from + i) {
        let Some(prefix) = TOKENS.iter().find(|p| text[g..].starts_with(*p)) else {
            from = g + 1;
            continue;
        };
        let start = g + prefix.len();
        let len = text[start..]
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(text.len() - start);
        if len >= 20 {
            out.push_str(&text[copied..start]);
            out.push_str("***");
            copied = start + len;
        }
        from = start + len;
    }
    out + &text[copied..]
}

/// `2026-09-24T14:03:07Z`, without a date crate: days since the epoch to a civil date (Hinnant).
fn utc(secs: u64) -> String {
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn utc_formats_civil_dates() {
        assert_eq!(super::utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(super::utc(1_790_000_000 + 59), "2026-09-21T14:14:19Z");
    }

    #[test]
    fn redacts_credentials() {
        let r = super::redact;
        assert_eq!(
            r("fatal: unable to access 'https://me:s3cret@github.com/a/b.git/': 403"),
            "fatal: unable to access 'https://***@github.com/a/b.git/': 403"
        );
        assert_eq!(
            r("https://x-access-token:abc@github.com and http://tok@host:8080/p"),
            "https://***@github.com and http://***@host:8080/p"
        );
        // An @ past the host is part of the path, not a login.
        assert_eq!(
            r("https://github.com/a/b@main ssh://git.example.com/x"),
            "https://github.com/a/b@main ssh://git.example.com/x"
        );
        let pat = format!("github_pat_{}", "A1b2_".repeat(16));
        assert_eq!(
            r(&format!("token ghp_{} and {pat}.", "x9".repeat(18))),
            "token ghp_*** and github_pat_***."
        );
        assert_eq!(r("branch ghp_fix-login"), "branch ghp_fix-login");
        assert_eq!(r("naïve → ok"), "naïve → ok");
        assert_eq!(r("gghp_x ghp_ ghg"), "gghp_x ghp_ ghg");
    }

    #[test]
    fn redacts_large_dense_text_in_one_pass() {
        // Took seconds at 120 KB and minutes at 1 MB when each prefix was searched per hit.
        let dense = "ghp_x ".repeat(200_000);
        assert_eq!(super::redact(&dense), dense);
        let tokens = format!("ghp_{} ", "a".repeat(30)).repeat(30_000);
        assert_eq!(super::redact(&tokens), "ghp_*** ".repeat(30_000));
        let urls = "https://u:p@h/ ".repeat(70_000);
        assert_eq!(super::redact(&urls), "https://***@h/ ".repeat(70_000));
    }
}
