//! Every error, in one file in the app's log folder (~/Library/Logs/app.gitviber.desktop on
//! macOS), and on the terminal running `pnpm tauri dev`: the page's (src/lib/app/errorLog.ts) and
//! Rust panics. The webview's console is rarely open, so an error there went unnoticed.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// Past this the file becomes errors.old.log and starts over, so it can't grow without bound.
const MAX_BYTES: u64 = 1 << 20;

pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("errors.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, dir.join("errors.old.log"));
    }
    let _ = FILE.set(path);
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write(
            "rust panic",
            &format!("{info}\n{}", std::backtrace::Backtrace::force_capture()),
        );
        default(info);
    }));
}

pub fn write(source: &str, message: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let line = format!("[{}] {source}: {}\n", utc(now), message.trim_end());
    eprint!("{line}");
    let Some(path) = FILE.get() else { return };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
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
}
