//! Watching memory: the process's footprint as Activity Monitor counts it, and how many of the
//! app's views are alive. A view that should go away with its tab or panel carries a `Live`
//! field; a count that only grows over open-close cycles is a leak.

use std::collections::BTreeMap;
use std::sync::Mutex;

static LIVE: Mutex<BTreeMap<&'static str, usize>> = Mutex::new(BTreeMap::new());

/// Counts one live `kind` while it exists. Put it in a view's struct: `_live: Live::new("DiffView")`.
#[derive(Debug)]
pub struct Live(&'static str);

impl Live {
    pub fn new(kind: &'static str) -> Self {
        *LIVE.lock().unwrap().entry(kind).or_default() += 1;
        Self(kind)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let mut live = LIVE.lock().unwrap();
        if let Some(n) = live.get_mut(self.0) {
            *n = n.saturating_sub(1);
        }
    }
}

/// Each kind's live count.
pub fn live_counts() -> Vec<(&'static str, usize)> {
    LIVE.lock()
        .unwrap()
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(k, n)| (*k, *n))
        .collect()
}

pub fn live_total() -> usize {
    LIVE.lock().unwrap().values().sum()
}

/// Debug builds show the readout; GITVIBER_MEMORY=1 shows it in a release build too.
pub fn readout_enabled() -> bool {
    cfg!(debug_assertions) || std::env::var("GITVIBER_MEMORY").is_ok_and(|v| v == "1")
}

/// The process's physical footprint in bytes: what Activity Monitor's Memory column shows on
/// macOS (compressed and swapped pages included), the resident set elsewhere.
pub fn footprint() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        mac::phys_footprint()
    }
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
        let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kb * 1024)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::os::raw::c_int;

    /// The head of libproc's rusage_info_v2, up to the field read; the rest is room.
    #[repr(C)]
    struct RusageInfoV2 {
        uuid: [u8; 16],
        user_time: u64,
        system_time: u64,
        pkg_idle_wkups: u64,
        interrupt_wkups: u64,
        pageins: u64,
        wired_size: u64,
        resident_size: u64,
        phys_footprint: u64,
        rest: [u64; 16],
    }

    const RUSAGE_INFO_V2: c_int = 2;

    unsafe extern "C" {
        fn proc_pid_rusage(pid: c_int, flavor: c_int, buffer: *mut RusageInfoV2) -> c_int;
    }

    pub fn phys_footprint() -> Option<u64> {
        let mut info = std::mem::MaybeUninit::<RusageInfoV2>::zeroed();
        // SAFETY: the buffer is at least as large as rusage_info_v2, which the call fills.
        let ok = unsafe {
            proc_pid_rusage(
                std::process::id() as c_int,
                RUSAGE_INFO_V2,
                info.as_mut_ptr(),
            )
        };
        // SAFETY: zeroed, then filled by the call on success.
        (ok == 0).then(|| unsafe { info.assume_init() }.phys_footprint)
    }
}

/// "123 MB".
pub fn format_bytes(bytes: u64) -> String {
    format!("{} MB", bytes / (1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_counts_follow_drops() {
        let before = live_counts()
            .iter()
            .find(|(k, _)| *k == "TestView")
            .map_or(0, |(_, n)| *n);
        let a = Live::new("TestView");
        let b = Live::new("TestView");
        let now = |k| {
            live_counts()
                .iter()
                .find(|(n, _)| *n == k)
                .map_or(0, |(_, n)| *n)
        };
        assert_eq!(now("TestView"), before + 2);
        drop(a);
        drop(b);
        assert_eq!(now("TestView"), before);
    }

    #[test]
    fn footprint_is_read() {
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            assert!(footprint().unwrap() > 1024 * 1024);
        }
    }
}
