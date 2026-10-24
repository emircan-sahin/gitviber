//! The few sequences alacritty's parser leaves unhandled that the pane acts on: OSC 133 (command
//! marks), OSC 7 (the shell's folder) and XTVERSION (CSI > q). Watched alongside the parser, with
//! where each one ends, so a mark lands where the cursor was at it. Plain text is skipped a chunk
//! at a time; OSCs of other numbers (an OSC 52 of megabytes) aren't kept.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// What follows `133;`.
    Mark(String),
    /// What follows `7;`: a file URL.
    Cwd(String),
    XtVersion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
}

/// Longer than any mark a shell sends (zsh's command line is cut at 200 characters, encoded).
const MAX_KEPT: usize = 4096;
const MAX_CSI: usize = 16;

pub struct Scanner {
    state: State,
    buf: Vec<u8>,
    /// Whether this OSC's payload is still kept: its number may be one we read.
    keep: bool,
}

impl Default for Scanner {
    fn default() -> Self {
        Scanner {
            state: State::Ground,
            buf: Vec::new(),
            keep: true,
        }
    }
}

impl Scanner {
    /// Feeds `bytes`; each find comes with the offset just past its last byte.
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<(usize, Found)>) {
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            match self.state {
                State::Ground => match bytes[i..].iter().position(|&b| b == 0x1b) {
                    Some(at) => {
                        i += at;
                        self.state = State::Esc;
                    }
                    None => return,
                },
                State::Esc => {
                    self.state = match b {
                        b'[' => State::Csi,
                        b']' => State::Osc,
                        0x1b => State::Esc,
                        _ => State::Ground,
                    };
                    self.buf.clear();
                    self.keep = true;
                }
                State::Csi => match b {
                    0x40..=0x7e => {
                        if b == b'q' && matches!(&self.buf[..], b">" | b">0") {
                            out.push((i + 1, Found::XtVersion));
                        }
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::Esc,
                    0x18 | 0x1a => self.state = State::Ground,
                    0x20..=0x3f if self.buf.len() < MAX_CSI => self.buf.push(b),
                    _ => {}
                },
                State::Osc => match b {
                    0x07 => {
                        self.end_osc(i + 1, out);
                    }
                    0x1b => self.state = State::OscEsc,
                    0x18 | 0x1a => self.state = State::Ground,
                    _ if self.keep => {
                        self.buf.push(b);
                        self.check_number();
                    }
                    _ => {}
                },
                State::OscEsc => {
                    if b == b'\\' {
                        self.end_osc(i + 1, out);
                    } else {
                        // Another escape cut the OSC short: this byte follows that ESC.
                        self.state = State::Esc;
                        continue;
                    }
                }
            }
            i += 1;
        }
    }

    /// Stops keeping a payload once its number isn't one we read, or it's grown past any mark.
    fn check_number(&mut self) {
        if self.buf.len() > MAX_KEPT {
            self.keep = false;
            return;
        }
        let number_done = self.buf.last() == Some(&b';') || self.buf.len() > 4;
        if number_done {
            let number = self.buf.split(|&b| b == b';').next().unwrap_or(&[]);
            if number != b"133" && number != b"7" {
                self.keep = false;
            }
        }
    }

    fn end_osc(&mut self, end: usize, out: &mut Vec<(usize, Found)>) {
        self.state = State::Ground;
        if !self.keep {
            return;
        }
        let text = String::from_utf8_lossy(&self.buf);
        if let Some(mark) = text.strip_prefix("133;") {
            out.push((end, Found::Mark(mark.to_string())));
        } else if let Some(url) = text.strip_prefix("7;") {
            out.push((end, Found::Cwd(url.to_string())));
        }
        self.buf = Vec::new();
    }
}

/// The folder an OSC 7 names: `file://host/path`, percent-encoded. Only this machine's.
pub fn cwd_from_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let slash = rest.find('/')?;
    let host = &rest[..slash];
    if !host.is_empty() && host != "localhost" && !host_is_this_machine(host) {
        return None;
    }
    let path = &rest[slash..];
    let mut out = Vec::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = path
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

fn host_is_this_machine(host: &str) -> bool {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: the buffer is ours and its length is passed.
        let ok = unsafe { libc_gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
        if ok {
            let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let name = String::from_utf8_lossy(&buf[..len]);
            let short = |s: &str| s.split('.').next().unwrap_or(s).to_ascii_lowercase();
            return short(&name) == short(host);
        }
    }
    let _ = host;
    false
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "gethostname"]
    fn libc_gethostname(name: *mut std::ffi::c_char, len: usize) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(chunks: &[&[u8]]) -> Vec<(usize, usize, Found)> {
        let mut s = Scanner::default();
        let mut all = vec![];
        for (n, chunk) in chunks.iter().enumerate() {
            let mut out = vec![];
            s.feed(chunk, &mut out);
            all.extend(out.into_iter().map(|(at, f)| (n, at, f)));
        }
        all
    }

    #[test]
    fn marks_folders_and_xtversion_with_where_they_end() {
        let bytes = b"hi\x1b]133;A\x07$ \x1b]133;D;0\x1b\\\x1b[>q\x1b]7;file:///tmp\x07";
        let found = scan(&[bytes]);
        assert_eq!(
            found,
            vec![
                (0, 10, Found::Mark("A".into())),
                (0, 23, Found::Mark("D;0".into())),
                (0, 27, Found::XtVersion),
                (0, 43, Found::Cwd("file:///tmp".into())),
            ]
        );
    }

    #[test]
    fn a_sequence_split_across_reads_is_found_where_it_ends() {
        let found = scan(&[b"\x1b]13", b"3;C;cmdline=ls -l", b"\x07rest"]);
        assert_eq!(found, vec![(2, 1, Found::Mark("C;cmdline=ls -l".into()))]);
        assert_eq!(
            scan(&[b"\x1b", b"[>0", b"q"]),
            vec![(2, 1, Found::XtVersion)]
        );
    }

    #[test]
    fn other_sequences_are_passed_over_and_big_oscs_not_kept() {
        let mut big = b"\x1b]52;c;".to_vec();
        big.extend(std::iter::repeat_n(b'A', 100_000));
        big.extend(b"\x07\x1b]133;B\x07");
        let mut s = Scanner::default();
        let mut out = vec![];
        s.feed(&big, &mut out);
        assert_eq!(out, vec![(big.len(), Found::Mark("B".into()))]);
        assert!(s.buf.capacity() < MAX_KEPT * 2);
        // DA2 and DECSCUSR aren't XTVERSION; an ESC inside an OSC ends it.
        assert!(scan(&[b"\x1b[>c\x1b[2 q\x1b[>1q"]).is_empty());
        assert_eq!(
            scan(&[b"\x1b]133;A\x1b[>q"]),
            vec![(0, 11, Found::XtVersion)]
        );
        assert!(scan(&[b"\x1b]1337;SetMark\x07"]).is_empty());
    }

    #[test]
    fn an_osc_7_names_a_folder_on_this_machine() {
        assert_eq!(
            cwd_from_url("file:///Users/me/Ekran%20Resmi").as_deref(),
            Some("/Users/me/Ekran Resmi")
        );
        assert_eq!(
            cwd_from_url("file://localhost/tmp").as_deref(),
            Some("/tmp")
        );
        assert_eq!(cwd_from_url("file://some-other-host.example/tmp"), None);
        assert_eq!(cwd_from_url("http://x/tmp"), None);
    }
}
