//! Handing things to the OS: a web page to the browser, a file to the file manager.

use std::path::PathBuf;
use std::process::Command;

/// Links in GitHub text are written by anyone, so only http(s) passes, in the canonical
/// form a browser would use: scheme and host lowercased, spaces, quotes and non-ASCII
/// percent-encoded. The launchers get it as one argument, never through a shell.
fn openable(url: &str) -> Option<String> {
    let parsed = tauri::Url::parse(url).ok()?;
    let url = String::from(parsed);
    let plain = url
        .bytes()
        .all(|b| b.is_ascii_graphic() && !b"\"<>\\`".contains(&b));
    (url.starts_with("https://") || url.starts_with("http://"))
        .then_some(url)
        .filter(|_| plain)
}

/// A desktop tool with the session's environment, not ours: inside an AppImage ours points at
/// its bundled libraries and xdg-open, and the host's file manager crashed on them.
#[cfg(all(unix, not(target_os = "macos")))]
fn desktop_tool(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear().envs(crate::shell::clean_env());
    cmd
}

/// Opens a web page in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    let url = openable(url).ok_or("refusing to open this URL")?;
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    // Not `cmd /C start`: cmd.exe re-parses the argument.
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = desktop_tool("xdg-open");
    cmd.arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Selects `path` in the file manager.
pub fn reveal(path: PathBuf) -> Result<(), String> {
    // Waited on (it returns at once) so no zombie is left behind per click.
    #[cfg(target_os = "macos")]
    return match std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .status()
    {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("open -R failed ({s})")),
        Err(e) => Err(e.to_string()),
    };
    #[cfg(target_os = "linux")]
    {
        use std::process::Stdio;
        // The file manager's own interface selects the entry (Dolphin, Nautilus, Nemo, …).
        // gdbus comes with GLib, which the app needs anyway; dbus-send is a separate package on
        // some distros. The URI is percent-encoded, so no quote or comma breaks either syntax.
        let uri = format!("file://{}", crate::trash::encode(&path));
        let (items, array) = (format!("['{uri}']"), format!("array:string:{uri}"));
        let calls: [&[&str]; 2] = [
            &[
                "gdbus",
                "call",
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--object-path=/org/freedesktop/FileManager1",
                "--method=org.freedesktop.FileManager1.ShowItems",
                items.as_str(),
                "''",
            ],
            &[
                "dbus-send",
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.FileManager1",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                array.as_str(),
                "string:",
            ],
        ];
        for call in calls {
            let shown = desktop_tool(call[0])
                .args(&call[1..])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if shown.is_ok_and(|s| s.success()) {
                return Ok(());
            }
        }
        // Without one, open the folder it's in. xdg-open may stay until that window closes,
        // so it's reaped on a thread.
        let mut child = desktop_tool("xdg-open")
            .arg(path.parent().unwrap_or(&path))
            .spawn()
            .map_err(|e| format!("xdg-open: {e}"))?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        Err("Reveal is not supported on this platform yet".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openable_links() {
        let ok = |u: &str| openable(u).unwrap_or_else(|| panic!("{u}"));
        assert_eq!(
            ok("https://github.com/a/b/pull/1#issuecomment-2"),
            "https://github.com/a/b/pull/1#issuecomment-2"
        );
        assert_eq!(ok("HTTPS://Docs.RS/a?b=c&d"), "https://docs.rs/a?b=c&d");
        // Characters raw-HTML hrefs carry that a URL keeps as they are.
        assert_eq!(ok("https://x.com/it's/[1]|a"), "https://x.com/it's/[1]|a");
        assert_eq!(ok("https://x.com/ça va"), "https://x.com/%C3%A7a%20va");
        // Quotes and spaces can't survive into the argument.
        assert_eq!(
            ok("https://x.com/\"; rm -rf ~"),
            "https://x.com/%22;%20rm%20-rf%20~"
        );
        assert_eq!(openable("javascript:alert(1)"), None);
        assert_eq!(openable("file:///etc/passwd"), None);
        assert_eq!(openable("mailto:a@b.c"), None);
        assert_eq!(openable("not a url"), None);
    }
}
