//! Debug builds only: runs JavaScript sent to 127.0.0.1 in the main webview and answers with its
//! result, so a script (or an agent) can read the DOM and click by selector instead of driving the
//! window through screenshots. macOS has no WebDriver for WKWebView. The port is the dev server's
//! plus 100 (1520 by default); .claude/skills/drive-gitviber/ui.sh `js` is the client.
//!
//! A connection sends a function body (`return document.title`, awaits allowed) and closes its
//! write side; the reply is `{"ok":true,"value":…}` or `{"ok":false,"error":"…"}`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewWindow};

const TIMEOUT: Duration = Duration::from_secs(20);

pub fn start(app: AppHandle) {
    let dev = std::env::var("GITVIBER_DEV_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok());
    let port = dev.unwrap_or(1420) + 100;
    let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) else {
        eprintln!("dev bridge: port {port} is taken, not started");
        return;
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let app = app.clone();
            std::thread::spawn(move || serve(&app, stream));
        }
    });
}

fn serve(app: &AppHandle, mut stream: TcpStream) {
    let mut body = String::new();
    let reply = match (
        stream.read_to_string(&mut body),
        app.get_webview_window("main"),
    ) {
        (Err(e), _) => failure(&e.to_string()),
        (_, None) => failure("no main window"),
        (Ok(_), Some(webview)) => run(&webview, &body),
    };
    let _ = stream.write_all(reply.as_bytes());
}

fn failure(error: &str) -> String {
    serde_json::json!({ "ok": false, "error": error }).to_string()
}

/// WKWebView's evaluate returns before a promise settles, so the script parks its result on
/// `window.__devBridge` and this polls for it.
fn run(webview: &WebviewWindow, body: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let start = format!(
        "(() => {{ const b = (window.__devBridge ??= {{}}); \
         (async () => {{ {body}\n }})().then( \
           (value) => (b[{id}] = {{ ok: true, value: value === undefined ? null : value }}), \
           (e) => (b[{id}] = {{ ok: false, error: `${{e}}\n${{(e && e.stack) || ''}}` }})); \
         return true; }})()"
    );
    if let Err(e) = eval(webview, &start) {
        return failure(&e);
    }
    let poll = format!(
        "(() => {{ const b = window.__devBridge || {{}}; const r = b[{id}]; delete b[{id}]; \
         return r === undefined ? null : JSON.stringify(r); }})()"
    );
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        match eval(webview, &poll) {
            // The callback hands back the value as JSON: here a JSON string holding the reply.
            Ok(json) => {
                if let Ok(Some(reply)) = serde_json::from_str::<Option<String>>(&json) {
                    return reply;
                }
            }
            Err(e) => return failure(&e),
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    failure("timed out")
}

fn eval(webview: &WebviewWindow, js: &str) -> Result<String, String> {
    let (tx, rx) = mpsc::channel();
    webview
        .eval_with_callback(js, move |result| {
            let _ = tx.send(result);
        })
        .map_err(|e| e.to_string())?;
    rx.recv_timeout(TIMEOUT)
        .map_err(|_| "no answer from the page".to_string())
}
