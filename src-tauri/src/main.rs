// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // git and ssh run this binary to ask for a password (askpass.rs); that never opens the app.
    if let Some(code) = gitviber_lib::askpass::helper() {
        std::process::exit(code);
    }
    #[cfg(target_os = "linux")]
    avoid_nvidia_dmabuf_crash();
    gitviber_lib::run()
}

/// WebKitGTK's DMA-BUF renderer dies on NVIDIA under Wayland ("Error 71 (Protocol error)
/// dispatching to Wayland display") before the window ever shows. Fall back to its other
/// renderer there, unless the user already chose one. Runs before any thread starts, which
/// `set_var` needs.
#[cfg(target_os = "linux")]
fn avoid_nvidia_dmabuf_crash() {
    const VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
    if std::env::var_os(VAR).is_none() && std::path::Path::new("/proc/driver/nvidia").exists() {
        std::env::set_var(VAR, "1");
    }
}
