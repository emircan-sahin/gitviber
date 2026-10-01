//! What a running process is: its command line, folder and start, read of the kernel (no `ps`),
//! for the terminal's panes (pty.rs) and the agents in them (agents.rs).

use std::path::PathBuf;

/// A running program, as agents.rs tells an agent from its command line.
#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// When it started, in seconds since the epoch; 0 when that can't be read.
    pub started: u64,
}

/// A process group's leader that is a command the shell runs: not the shell, and never a pid
/// (0, 1, ours, our group) that would stand for more than that program.
pub fn job_leader(leader: Option<i32>, shell: Option<u32>) -> Option<u32> {
    let pid = u32::try_from(leader?).ok().filter(|&p| p > 1)?;
    #[cfg(unix)]
    if pid == std::process::id() || pid as libc::pid_t == unsafe { libc::getpgrp() } {
        return None;
    }
    (Some(pid) != shell).then_some(pid)
}

#[cfg(target_os = "macos")]
pub fn cwd(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut libc::proc_vnodepathinfo).cast(),
            size,
        )
    };
    if got != size {
        return None;
    }
    // libc declares the path as 32 rows of 32 chars; it's one MAXPATHLEN buffer.
    let raw = &info.pvi_cdir.vip_path;
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(raw.as_ptr().cast(), std::mem::size_of_val(raw)) };
    let path = std::ffi::CStr::from_bytes_until_nul(bytes).ok()?;
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()));
    path.is_absolute().then(|| path.to_path_buf())
}

#[cfg(target_os = "linux")]
pub fn cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn cwd(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
pub fn process(pid: u32) -> Option<Process> {
    Some(Process {
        pid,
        argv: args(pid)?,
        cwd: cwd(pid),
        started: started(pid)?,
    })
}

/// When `pid` started: a cheap read, to tell a pane's same program from a new one.
#[cfg(target_os = "macos")]
pub fn started(pid: u32) -> Option<u64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (got == size).then_some(info.pbi_start_tvsec)
}

#[cfg(target_os = "macos")]
fn args(pid: u32) -> Option<Vec<String>> {
    let mut max: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>();
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let ok = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut max as *mut libc::c_int).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 || max <= 0 {
        return None;
    }
    let mut buf = vec![0u8; max as usize];
    let mut len = buf.len();
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let ok = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 {
        return None;
    }
    parse_procargs2(&buf[..len.min(buf.len())])
}

/// KERN_PROCARGS2: argc, the executable's path, NUL padding, then argv, each NUL-ended (then
/// the environment). An empty argv[0] can't be told from the padding: it's skipped with it, and
/// argv then ends one string into the environment, as `ps` reads it too.
#[cfg(any(target_os = "macos", test))]
fn parse_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    const ARGC: usize = std::mem::size_of::<i32>();
    let argc = usize::try_from(i32::from_ne_bytes(buf.get(..ARGC)?.try_into().ok()?)).ok()?;
    let rest = buf[ARGC..].splitn(2, |&b| b == 0).nth(1)?;
    let args = &rest[rest.iter().position(|&b| b != 0)?..];
    let argv: Vec<String> = args
        .split(|&b| b == 0)
        .take(argc)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    // Cut short: not a command line to go by.
    (argv.len() == argc).then_some(argv)
}

/// The processes in a process group: what a launcher (npx) started under it.
#[cfg(target_os = "macos")]
pub fn job(pgid: u32) -> Vec<u32> {
    /// sys/proc_info.h; libc doesn't name it.
    const PROC_PGRP_ONLY: u32 = 2;
    let mut pids = vec![0 as libc::c_int; 64];
    let size = std::mem::size_of_val(pids.as_slice()) as libc::c_int;
    let got = unsafe { libc::proc_listpids(PROC_PGRP_ONLY, pgid, pids.as_mut_ptr().cast(), size) };
    let n = usize::try_from(got).unwrap_or(0) / std::mem::size_of::<libc::c_int>();
    pids[..n.min(pids.len())]
        .iter()
        .filter_map(|&p| u32::try_from(p).ok().filter(|&p| p > 0))
        .collect()
}

#[cfg(target_os = "linux")]
pub fn process(pid: u32) -> Option<Process> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    // Empty while an exec is still setting up the new program (spawn can return by then) and
    // for a zombie: nothing to read yet, and the caller asks again.
    if cmdline.is_empty() {
        return None;
    }
    let argv = cmdline
        .strip_suffix(b"\0")
        .unwrap_or(&cmdline[..])
        .split(|&b| b == 0)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    Some(Process {
        pid,
        argv,
        cwd: cwd(pid),
        started: started(pid)?,
    })
}

/// /proc/<pid>/stat's fields after the command's ")": its 20th is the start, in clock ticks
/// since boot; /proc/stat's btime is the boot.
#[cfg(target_os = "linux")]
pub fn started(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let ticks: u64 = stat
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()?;
    let boot: u64 = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    let hz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    Some(boot + ticks / hz.max(1))
}

#[cfg(target_os = "linux")]
pub fn job(pgid: u32) -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
                s.rsplit_once(')')
                    .and_then(|(_, rest)| rest.split_whitespace().nth(2)?.parse().ok())
                    == Some(pgid)
            })
        })
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn process(_pid: u32) -> Option<Process> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn started(_pid: u32) -> Option<u64> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn job(_pgid: u32) -> Vec<u32> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn procargs(argc: i32, rest: &[u8]) -> Vec<u8> {
        [&argc.to_ne_bytes()[..], rest].concat()
    }

    #[test]
    fn procargs2_is_read_past_the_executable_and_its_padding() {
        let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let cases: &[(Vec<u8>, Option<Vec<String>>)] = &[
            (
                procargs(2, b"/bin/sleep\0\0\0\0sleep\x0030\0PATH=/bin\0"),
                Some(words(&["sleep", "30"])),
            ),
            // An empty last argument is one, not the start of the environment.
            (
                procargs(3, b"/bin/sh\0\0sh\0-c\0\0HOME=/x\0"),
                Some(words(&["sh", "-c", ""])),
            ),
            // No padding.
            (procargs(1, b"/x\0x\0"), Some(words(&["x"]))),
            // Cut short.
            (procargs(3, b"/bin/sh\0\0sh\0-c"), None),
            (procargs(1, b"/bin/sh"), None),
            (vec![1, 0], None),
            // An empty argv[0] goes with the padding: argv slides one into the environment.
            (
                procargs(2, b"/bin/x\0\0\0a\0HOME=/x\0"),
                Some(words(&["a", "HOME=/x"])),
            ),
        ];
        for (buf, want) in cases {
            assert_eq!(
                &parse_procargs2(buf),
                want,
                "{:?}",
                String::from_utf8_lossy(buf)
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_job_leader_is_never_the_shell_nor_a_pid_that_stands_for_more() {
        let ours = std::process::id();
        let group = unsafe { libc::getpgrp() };
        assert_eq!(job_leader(Some(4242), Some(100)), Some(4242));
        assert_eq!(job_leader(Some(100), Some(100)), None);
        for pid in [-1, 0, 1, ours as i32, group] {
            assert_eq!(job_leader(Some(pid), Some(100)), None, "{pid}");
        }
        assert_eq!(job_leader(None, Some(100)), None);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_process_folder_is_read() {
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        let read = cwd(std::process::id()).unwrap();
        assert_eq!(read.canonicalize().unwrap(), here);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_program_in_its_own_process_group_is_read_with_its_job() {
        use std::os::unix::process::CommandExt;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut cmd = std::process::Command::new("sh");
        // `:` after it keeps sh from exec'ing sleep; the empty last argument is one too.
        cmd.args(["-c", "sleep 30; :", "x", ""]).process_group(0);
        let mut child = crate::process::spawn(&mut cmd).unwrap();
        // On Linux spawn can return before the exec has set up argv.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let read = loop {
            match process(child.id()) {
                Some(p) if p.argv.len() > 1 => break Some(p),
                _ if std::time::Instant::now() > deadline => break None,
                _ => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        };
        let job = job(child.id());
        // The group, sleep with it, while sh isn't reaped and its number still names it.
        crate::process::kill_group(&mut child, std::time::Duration::ZERO);
        let read = read.unwrap();
        assert_eq!(read.argv, ["sh", "-c", "sleep 30; :", "x", ""]);
        assert!(read.started.abs_diff(now) <= 5, "{} vs {now}", read.started);
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(read.cwd.unwrap().canonicalize().unwrap(), here);
        assert!(job.contains(&child.id()), "{job:?}");
    }
}
