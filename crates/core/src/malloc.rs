//! The allocator's settings that decide whether freed memory goes back to the system.

/// macOS malloc keeps freed multi-MB blocks dirty in its large cache: opening and closing a
/// 7.4 MB file tab grew the footprint about 15 MB a cycle (20 → 450 MB over 27) with nothing left
/// alive, and stayed near 30 MB with the cache off. libmalloc reads the setting once, when it
/// starts, so the process starts itself again with it; a bundle launched by LaunchServices gets
/// it from Info.plist's LSEnvironment and never needs that. Call first thing in `main`.
#[cfg(target_os = "macos")]
pub fn return_freed_blocks() {
    use std::os::unix::process::CommandExt;
    if std::env::var_os("MallocLargeCache").is_some() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut args = std::env::args_os();
    let mut command = std::process::Command::new(exe);
    if let Some(arg0) = args.next() {
        command.arg0(arg0);
    }
    let err = command.args(args).env("MallocLargeCache", "0").exec();
    // exec only returns when it failed: carry on with the cache.
    eprintln!("gitviber: couldn't restart without malloc's large cache: {err}");
}

/// glibc raises its mmap threshold each time a mapped block is freed, up to 32 MB; past that, big
/// buffers come from the heap, whose free space stays. A fixed threshold keeps them mapped.
#[cfg(target_os = "linux")]
pub fn return_freed_blocks() {
    // SAFETY: mallopt only changes a tunable; it's called before any thread starts.
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 256 * 1024);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn return_freed_blocks() {}
