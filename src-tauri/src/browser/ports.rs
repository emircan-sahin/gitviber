//! The ports the programs in a terminal listen on (a dev server started in a pane), for the
//! browser's ports menu. Read from the kernel, as procinfo.rs reads processes: the shell's
//! process tree, then each process's listening TCP sockets. No lsof, and nothing polls: the page
//! asks when its menu opens and a little after a pane starts a command.

use serde::Serialize;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Port {
    pub port: u16,
    pub pid: u32,
    /// The program's name: node, python3, bun.
    pub process: String,
    /// On this machine's loopback address only, not every interface.
    pub loopback: bool,
}

/// What the process trees under `shells` listen on: one entry a port, the lowest first.
pub fn listening(shells: &[u32]) -> Vec<Port> {
    let mut ports: Vec<Port> = sockets(&tree(shells))
        .into_iter()
        .map(|(pid, port, loopback)| Port {
            port,
            pid,
            process: name(pid),
            loopback,
        })
        .collect();
    ports.sort_by_key(|p| p.port);
    // IPv4 and IPv6 both: loopback only if neither listens on every interface.
    ports.dedup_by(|b, a| {
        let same = a.port == b.port;
        if same {
            a.loopback &= b.loopback;
        }
        same
    });
    ports
}

fn name(pid: u32) -> String {
    let argv0 = crate::procinfo::process(pid).and_then(|p| p.argv.into_iter().next());
    let path = argv0.unwrap_or_default();
    path.rsplit('/').next().unwrap_or_default().to_string()
}

/// The shells and every process under them.
fn tree(shells: &[u32]) -> Vec<u32> {
    let children = children();
    let mut all: Vec<u32> = shells.to_vec();
    let mut at = 0;
    while let Some(&pid) = all.get(at) {
        for child in children(pid) {
            if !all.contains(&child) {
                all.push(child);
            }
        }
        at += 1;
    }
    all
}

fn loopback(v6: bool, address: [u8; 16]) -> bool {
    if v6 {
        Ipv6Addr::from(address).is_loopback()
            || Ipv6Addr::from(address)
                .to_ipv4_mapped()
                .is_some_and(|a| a.is_loopback())
    } else {
        Ipv4Addr::new(address[12], address[13], address[14], address[15]).is_loopback()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::mem::size_of;

    /// sys/proc_info.h's values, which libc doesn't name.
    const PROC_PPID_ONLY: u32 = 6;
    const PROC_PIDFDSOCKETINFO: i32 = 3;
    const SOCKINFO_TCP: i32 = 2;
    const TSI_S_LISTEN: i32 = 1;
    const INI_IPV6: u8 = 2;

    /// A process's children, asked of the kernel one parent at a time.
    pub fn children() -> impl Fn(u32) -> Vec<u32> {
        |ppid| {
            let mut pids = vec![0 as libc::c_int; 256];
            let size = i32::try_from(size_of::<libc::c_int>() * pids.len()).unwrap_or(0);
            let got = unsafe {
                libc::proc_listpids(PROC_PPID_ONLY, ppid, pids.as_mut_ptr().cast(), size)
            };
            let n = usize::try_from(got).unwrap_or(0) / size_of::<libc::c_int>();
            pids[..n.min(pids.len())]
                .iter()
                .filter_map(|&p| u32::try_from(p).ok().filter(|&p| p > 0))
                .collect()
        }
    }

    // sys/proc_info.h's socket_fdinfo, as far as a TCP socket's state and local address. The
    // kernel fills it whole or not at all: a reply of another size (a layout this mirror doesn't
    // know) is no answer.

    #[repr(C)]
    pub struct SocketFdInfo {
        pfi: ProcFileInfo,
        pub psi: SocketInfo,
    }

    #[repr(C)]
    struct ProcFileInfo {
        openflags: u32,
        status: u32,
        offset: i64,
        kind: i32,
        guardflags: u32,
    }

    #[repr(C)]
    pub struct SocketInfo {
        stat: libc::vinfo_stat,
        so: u64,
        pcb: u64,
        kind_of_socket: i32,
        protocol: i32,
        family: i32,
        options: i16,
        linger: i16,
        state: i16,
        qlen: i16,
        incqlen: i16,
        qlimit: i16,
        timeo: i16,
        error: u16,
        oobmark: u32,
        rcv: SockbufInfo,
        snd: SockbufInfo,
        pub kind: i32,
        rfu: u32,
        pub proto: SocketProto,
    }

    #[repr(C)]
    struct SockbufInfo {
        cc: u32,
        hiwat: u32,
        mbcnt: u32,
        mbmax: u32,
        lowat: u32,
        flags: i16,
        timeo: i16,
    }

    /// The union's largest member is a Unix socket's two addresses: 528 bytes.
    #[repr(C)]
    pub union SocketProto {
        pub tcp: TcpSockInfo,
        size: [u64; 66],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct TcpSockInfo {
        pub ini: InSockInfo,
        pub state: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct InSockInfo {
        fport: i32,
        /// In network order, in its low 16 bits.
        pub lport: i32,
        gencnt: u64,
        flags: u32,
        flow: u32,
        pub vflag: u8,
        ip_ttl: u8,
        rfu: u32,
        faddr: [u8; 16],
        /// IPv4 in the last four bytes.
        pub laddr: [u8; 16],
        v4: u8,
        v6: [u32; 3],
    }

    /// PROC_PIDFDSOCKETINFO_SIZE in the headers.
    pub const SOCKET_FDINFO_SIZE: usize = 792;
    const _: () = assert!(size_of::<SocketFdInfo>() == SOCKET_FDINFO_SIZE);

    /// The port a socket listens on over TCP, and whether on loopback only.
    pub fn listening_port(info: &SocketFdInfo) -> Option<(u16, bool)> {
        if info.psi.kind != SOCKINFO_TCP {
            return None;
        }
        // SAFETY: soi_kind says the union holds a TCP socket's info.
        let tcp = unsafe { info.psi.proto.tcp };
        if tcp.state != TSI_S_LISTEN {
            return None;
        }
        let port = u16::from_be(tcp.ini.lport as u16);
        let v6 = tcp.ini.vflag & INI_IPV6 != 0;
        (port != 0).then(|| (port, super::loopback(v6, tcp.ini.laddr)))
    }

    /// One socket's info, when the kernel gives all of it.
    fn socket(pid: i32, fd: i32) -> Option<SocketFdInfo> {
        let mut info = std::mem::MaybeUninit::<SocketFdInfo>::zeroed();
        let size = i32::try_from(SOCKET_FDINFO_SIZE).ok()?;
        let got = unsafe {
            libc::proc_pidfdinfo(
                pid,
                fd,
                PROC_PIDFDSOCKETINFO,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        // SAFETY: zeroed, then filled whole when the reply is its size.
        (got == size).then(|| unsafe { info.assume_init() })
    }

    /// Every (pid, port, loopback) the processes listen on.
    pub fn sockets(pids: &[u32]) -> Vec<(u32, u16, bool)> {
        let mut out = Vec::new();
        for &pid in pids {
            let Ok(cpid) = i32::try_from(pid) else {
                continue;
            };
            let mut fds = vec![
                libc::proc_fdinfo {
                    proc_fd: 0,
                    proc_fdtype: 0
                };
                512
            ];
            let size = i32::try_from(size_of::<libc::proc_fdinfo>() * fds.len()).unwrap_or(0);
            let got = unsafe {
                libc::proc_pidinfo(
                    cpid,
                    libc::PROC_PIDLISTFDS,
                    0,
                    fds.as_mut_ptr() as *mut c_void,
                    size,
                )
            };
            let n = usize::try_from(got).unwrap_or(0) / size_of::<libc::proc_fdinfo>();
            for fd in &fds[..n.min(fds.len())] {
                if fd.proc_fdtype != libc::PROX_FDTYPE_SOCKET as u32 {
                    continue;
                }
                if let Some((port, lo)) = socket(cpid, fd.proc_fd).as_ref().and_then(listening_port)
                {
                    out.push((pid, port, lo));
                }
            }
        }
        out
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::collections::HashMap;

    /// Every process's parent, read once from /proc.
    pub fn children() -> impl Fn(u32) -> Vec<u32> {
        let mut by_parent: HashMap<u32, Vec<u32>> = HashMap::new();
        for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse().ok()) else {
                continue;
            };
            let stat = std::fs::read_to_string(entry.path().join("stat")).unwrap_or_default();
            if let Some(ppid) = parent(&stat) {
                by_parent.entry(ppid).or_default().push(pid);
            }
        }
        move |pid| by_parent.get(&pid).cloned().unwrap_or_default()
    }

    /// /proc/<pid>/stat's parent: the second field after the command's ")".
    pub fn parent(stat: &str) -> Option<u32> {
        stat.rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }

    /// Every (pid, port, loopback) the processes listen on: their sockets' inodes, found in
    /// the kernel's TCP tables.
    pub fn sockets(pids: &[u32]) -> Vec<(u32, u16, bool)> {
        let mut listening = HashMap::new();
        for (file, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
            let table = std::fs::read_to_string(file).unwrap_or_default();
            for l in super::parse_proc_net_tcp(&table, v6) {
                listening.insert(l.inode, (l.port, l.loopback));
            }
        }
        let mut out = Vec::new();
        for &pid in pids {
            let fds = std::fs::read_dir(format!("/proc/{pid}/fd"));
            for fd in fds.into_iter().flatten().flatten() {
                let target = std::fs::read_link(fd.path()).unwrap_or_default();
                let inode = target
                    .to_str()
                    .and_then(|t| t.strip_prefix("socket:["))
                    .and_then(|t| t.strip_suffix(']'))
                    .and_then(|t| t.parse::<u64>().ok());
                if let Some(&(port, lo)) = inode.and_then(|i| listening.get(&i)) {
                    out.push((pid, port, lo));
                }
            }
        }
        out
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    pub fn children() -> impl Fn(u32) -> Vec<u32> {
        |_| Vec::new()
    }
    pub fn sockets(_: &[u32]) -> Vec<(u32, u16, bool)> {
        Vec::new()
    }
}

use platform::{children, sockets};

/// A listening socket in /proc/net/tcp{,6}.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, PartialEq)]
pub struct Listen {
    pub inode: u64,
    pub port: u16,
    pub loopback: bool,
}

/// The LISTEN rows (st 0A) of /proc/net/tcp, or tcp6 (`v6`). An address is hex, each 32-bit
/// word in the kernel's byte order (little-endian here), then ":" and the port in hex.
#[cfg(any(target_os = "linux", test))]
pub fn parse_proc_net_tcp(table: &str, v6: bool) -> Vec<Listen> {
    table
        .lines()
        .skip(1)
        .filter_map(|row| {
            let cols: Vec<&str> = row.split_whitespace().collect();
            if cols.get(3) != Some(&"0A") {
                return None;
            }
            let (address, port) = cols.get(1)?.split_once(':')?;
            let port = u16::from_str_radix(port, 16).ok()?;
            let inode = cols.get(9)?.parse().ok()?;
            let words = address.len() / 8;
            if words != if v6 { 4 } else { 1 } || address.len() % 8 != 0 {
                return None;
            }
            let mut bytes = [0u8; 16];
            for w in 0..words {
                let word = u32::from_str_radix(&address[w * 8..w * 8 + 8], 16).ok()?;
                let at = if v6 { w * 4 } else { 12 };
                bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
            }
            Some(Listen {
                inode,
                port,
                loopback: loopback(v6, bytes),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41001 1 0000000000000000 100 0 0 10 0
   1: 00000000:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41002 1 0000000000000000 100 0 0 10 0
   2: 0100007F:1435 0100007F:C350 01 00000000:00000000 00:00000000 00000000  1000        0 41003 1 0000000000000000 20 4 30 10 -1
   3: garbage
";

    const TCP6: &str = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:0FA0 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 52001 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 52002 1 0000000000000000 100 0 0 10 0
   2: 0000000000000000FFFF00000100007F:2328 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 52003 1 0000000000000000 100 0 0 10 0
   3: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 52004 1 0000000000000000 100 0 0 10 0
";

    #[test]
    fn listening_rows_of_the_tcp_tables() {
        assert_eq!(
            parse_proc_net_tcp(TCP, false),
            [
                Listen {
                    inode: 41001,
                    port: 5173,
                    loopback: true
                },
                Listen {
                    inode: 41002,
                    port: 3000,
                    loopback: false
                },
            ]
        );
        assert_eq!(
            parse_proc_net_tcp(TCP6, true),
            [
                Listen {
                    inode: 52001,
                    port: 4000,
                    loopback: true
                },
                Listen {
                    inode: 52002,
                    port: 8080,
                    loopback: false
                },
                // ::ffff:127.0.0.1, an IPv4 loopback listener on a dual-stack socket.
                Listen {
                    inode: 52003,
                    port: 9000,
                    loopback: true
                },
            ],
            "an IPv4 row in the IPv6 table is no row"
        );
        assert!(parse_proc_net_tcp("", false).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_stat_line_names_its_parent_past_the_command() {
        assert_eq!(platform::parent("41 (node) S 40 41 40 0"), Some(40));
        assert_eq!(platform::parent("42 (my (odd) name) R 7 42"), Some(7));
        assert_eq!(platform::parent("garbage"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn only_a_listening_tcp_socket_gives_a_port() {
        use platform::{listening_port, SocketFdInfo};
        // SAFETY: plain integers; all zeros is a valid (empty) reply.
        let mut info: SocketFdInfo = unsafe { std::mem::zeroed() };
        assert_eq!(listening_port(&info), None, "not TCP");
        info.psi.kind = 2;
        let mut tcp = unsafe { info.psi.proto.tcp };
        tcp.ini.lport = i32::from(5173u16.to_be());
        tcp.ini.vflag = 1;
        tcp.ini.laddr[12..].copy_from_slice(&[127, 0, 0, 1]);
        info.psi.proto.tcp = tcp;
        assert_eq!(listening_port(&info), None, "not listening");
        tcp.state = 1;
        info.psi.proto.tcp = tcp;
        assert_eq!(listening_port(&info), Some((5173, true)));
        tcp.ini.laddr = [0; 16];
        info.psi.proto.tcp = tcp;
        assert_eq!(listening_port(&info), Some((5173, false)));
    }

    /// The whole way through the kernel: this test's own listener, found under its own pid.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn finds_a_port_this_process_listens_on() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let found = listening(&[std::process::id()]);
        let mine = found.iter().find(|p| p.port == port);
        assert!(
            mine.is_some_and(|p| p.loopback && p.pid == std::process::id()),
            "{found:?}"
        );
    }

    fn v6(text: &str) -> [u8; 16] {
        text.parse::<Ipv6Addr>().unwrap().octets()
    }

    fn v4(a: [u8; 4]) -> [u8; 16] {
        let mut bytes = [0; 16];
        bytes[12..].copy_from_slice(&a);
        bytes
    }

    #[test]
    fn only_this_machine_counts_as_loopback_in_either_family() {
        assert!(loopback(false, v4([127, 0, 0, 1])));
        assert!(loopback(false, v4([127, 9, 8, 7])));
        for no in [
            [0, 0, 0, 0],
            [10, 0, 0, 1],
            [192, 168, 1, 2],
            [128, 0, 0, 1],
        ] {
            assert!(!loopback(false, v4(no)), "{no:?}");
        }
        assert!(loopback(true, v6("::1")));
        assert!(loopback(true, v6("::ffff:127.0.0.1")));
        assert!(loopback(true, v6("::ffff:127.1.2.3")));
        for no in [
            "::",
            "::ffff:0.0.0.0",
            "::ffff:10.0.0.1",
            "fe80::1",
            "::2",
            "::127.0.0.1",
        ] {
            assert!(!loopback(true, v6(no)), "{no}");
        }
    }

    #[test]
    fn rows_the_tables_never_list_as_listening_are_no_rows() {
        let header = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";
        for row in [
            // Not listening: established, time-wait, close.
            "   0: 0100007F:1435 0100007F:C350 01 00000000:00000000 00:00000000 00000000  1000 0 41003",
            "   0: 0100007F:1435 0100007F:C350 06 00000000:00000000 00:00000000 00000000  1000 0 41004",
            "   0: 0100007F:1435 00000000:0000 0a 00000000:00000000 00:00000000 00000000  1000 0 41005",
            // Malformed: no port, a bad port, no inode, a bad inode, an address of the other family.
            "   0: 0100007F 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 41006",
            "   0: 0100007F:XYZ 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 41007",
            "   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0",
            "   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 -1",
            "   0: 0100007F0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 41008",
            "   0: 0100007G:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 41009",
            "   0: :1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 41010",
            "   0: 0100007F:11435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000 0 41011",
            "",
            "0A",
        ] {
            assert!(parse_proc_net_tcp(&format!("{header}{row}\n"), false).is_empty(), "{row}");
        }
        // The header itself is never a row, even when it's all there is.
        assert!(parse_proc_net_tcp(header, false).is_empty());
        // A listener on the IPv6 table's every-interface address with lowercase hex.
        let any = format!("{header}   0: 00000000000000000000000000000000:1f90 00000000000000000000000000000000:0000 0A 0 0 0 1000 0 7\n");
        assert_eq!(
            parse_proc_net_tcp(&any, true),
            [Listen {
                inode: 7,
                port: 8080,
                loopback: false
            }]
        );
    }

    #[test]
    fn a_big_table_with_garbage_mixed_in_parses_without_panicking() {
        let mut table = String::from("header\n");
        let mut seed: u64 = 1;
        let mut expect = 0;
        for i in 0..5000u32 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let port = (seed >> 40) as u16;
            if seed.is_multiple_of(3) {
                table.push_str(&format!(
                    "{i}: 0100007F:{port:04X} 00000000:0000 0A 0 0 0 1 0 {i}\n"
                ));
                expect += 1;
            } else {
                let junk: String = (0..(seed % 40) as usize)
                    .map(|k| char::from(b"0A: x\t\xff"[(k + i as usize) % 7].min(0x7e)))
                    .collect();
                table.push_str(&junk);
                table.push('\n');
            }
        }
        let rows = parse_proc_net_tcp(&table, false);
        assert!(rows.len() >= expect, "{} < {expect}", rows.len());
        assert!(rows
            .iter()
            .all(|r| r.loopback || r.port == 0 || r.inode > 0));
    }

    /// A program a shell started, two levels down, listening: found through the tree. macOS
    /// only: its nc takes `-l host port`, which netcat on Linux spells differently.
    #[cfg(target_os = "macos")]
    #[test]
    fn finds_a_grandchilds_port_and_nothing_once_it_ends() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        // `; true` keeps the shell from exec'ing nc in its place.
        let Ok(mut shell) = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("nc -l 127.0.0.1 {port} >/dev/null 2>&1; true"))
            .stdin(std::process::Stdio::null())
            .spawn()
        else {
            return;
        };
        let found = (0..50).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            listening(&[shell.id()])
                .into_iter()
                .find(|p| p.port == port)
        });
        let _ = shell.kill();
        let _ = std::process::Command::new("pkill")
            .args(["-f", &format!("nc -l 127.0.0.1 {port}")])
            .status();
        let _ = shell.wait();
        let Some(found) = found else {
            panic!("nc's port {port} not found under the shell");
        };
        assert!(found.loopback && found.pid != shell.id(), "{found:?}");
        assert_eq!(found.process, "nc");
        // A process tree that isn't there any more lists nothing, and a pid of none either.
        assert!(listening(&[shell.id()]).iter().all(|p| p.port != port));
        assert!(listening(&[]).is_empty());
        assert!(listening(&[u32::MAX, 0]).iter().all(|p| p.pid != 0));
    }
}
