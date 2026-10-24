//! The few sequences alacritty's parser leaves unhandled that the pane acts on: OSC 133 (command
//! marks), OSC 7 (the shell's folder) and XTVERSION (CSI > q). Watched alongside the parser, with
//! where each one ends, so a mark lands where the cursor was at it. Plain text is skipped a chunk
//! at a time; OSCs of other numbers (an OSC 52 of megabytes) aren't kept.
//!
//! It also bounds what alacritty's parser holds: vte keeps an OSC's bytes in a buffer that only
//! grows, so one past `MAX_OSC` is cut (`CutOsc`) and the rest of it skipped, and after a big one
//! the pane starts a fresh parser.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// What follows `133;`.
    Mark(String),
    /// What follows `7;`: a file URL.
    Cwd(String),
    /// A notification escape: OSC 9, 777 or 99, and what follows its number.
    Notify(u32, String),
    XtVersion,
    /// An OSC past MAX_OSC: cancel it in the parser and skip what's left of it.
    CutOsc,
    /// The cut OSC ended here; `esc`: an ESC that ended it, to feed first.
    OscSkipped {
        esc: bool,
    },
    /// An OSC past BIG_OSC ended (whole): the parser's buffer stayed that big.
    BigOscEnded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
    Skip,
    SkipEsc,
}

/// Longer than any mark a shell sends (zsh's command line is cut at 200 characters, encoded).
const MAX_KEPT: usize = 4096;
const MAX_CSI: usize = 16;
/// An OSC 52 of the most the clipboard takes (about 1 MB decoded) fits.
pub const MAX_OSC: usize = 1_500_000;
/// Past this an OSC's buffer is worth giving back.
const BIG_OSC: usize = 64 * 1024;

pub struct Scanner {
    state: State,
    buf: Vec<u8>,
    /// Whether this OSC's payload is still kept: its number may be one we read.
    keep: bool,
    /// This OSC's length so far.
    osc_len: usize,
}

impl Default for Scanner {
    fn default() -> Self {
        Scanner {
            state: State::Ground,
            buf: Vec::new(),
            keep: true,
            osc_len: 0,
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
                    self.osc_len = 0;
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
                        self.osc_len += 1;
                        self.buf.push(b);
                        self.check_number();
                    }
                    _ => {
                        // A payload not kept, an OSC 52's: on to its end, or to the cap.
                        let room = MAX_OSC.saturating_sub(self.osc_len);
                        let rest = &bytes[i..bytes.len().min(i + room)];
                        match rest
                            .iter()
                            .position(|&b| matches!(b, 0x07 | 0x1b | 0x18 | 0x1a))
                        {
                            Some(at) => {
                                self.osc_len += at;
                                i += at;
                            }
                            None => {
                                self.osc_len += rest.len();
                                i += rest.len();
                                if self.osc_len >= MAX_OSC {
                                    out.push((i, Found::CutOsc));
                                    self.state = State::Skip;
                                }
                            }
                        }
                        continue;
                    }
                },
                State::Skip => match bytes[i..]
                    .iter()
                    .position(|&b| matches!(b, 0x07 | 0x1b | 0x18 | 0x1a))
                {
                    Some(at) => {
                        i += at;
                        if bytes[i] == 0x1b {
                            self.state = State::SkipEsc;
                        } else {
                            out.push((i + 1, Found::OscSkipped { esc: false }));
                            self.state = State::Ground;
                        }
                    }
                    None => return,
                },
                State::SkipEsc => {
                    if b == b'\\' {
                        out.push((i + 1, Found::OscSkipped { esc: false }));
                        self.state = State::Ground;
                    } else {
                        // An escape that starts something else: it goes to the parser again.
                        out.push((i, Found::OscSkipped { esc: true }));
                        self.state = State::Esc;
                        continue;
                    }
                }
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
            if !matches!(number, b"133" | b"7" | b"9" | b"777" | b"99") {
                self.keep = false;
            }
        }
    }

    fn end_osc(&mut self, end: usize, out: &mut Vec<(usize, Found)>) {
        self.state = State::Ground;
        if self.osc_len > BIG_OSC {
            out.push((end, Found::BigOscEnded));
        }
        if !self.keep {
            return;
        }
        let text = String::from_utf8_lossy(&self.buf);
        if let Some(mark) = text.strip_prefix("133;") {
            out.push((end, Found::Mark(mark.to_string())));
        } else if let Some(url) = text.strip_prefix("7;") {
            out.push((end, Found::Cwd(url.to_string())));
        } else {
            for code in [9, 777, 99] {
                if let Some(data) = text.strip_prefix(&format!("{code};")) {
                    out.push((end, Found::Notify(code, data.to_string())));
                }
            }
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

pub(crate) fn host_is_this_machine(host: &str) -> bool {
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
        assert_eq!(
            out,
            vec![
                (big.len() - 8, Found::BigOscEnded),
                (big.len(), Found::Mark("B".into()))
            ]
        );
        assert!(s.buf.capacity() < MAX_KEPT * 2);
        // DA2 and DECSCUSR aren't XTVERSION; an ESC inside an OSC ends it.
        assert!(scan(&[b"\x1b[>c\x1b[2 q\x1b[>1q"]).is_empty());
        assert_eq!(
            scan(&[b"\x1b]133;A\x1b[>q"]),
            vec![(0, 11, Found::XtVersion)]
        );
        assert!(scan(&[b"\x1b]1337;SetMark\x07"]).is_empty());
        assert_eq!(
            scan(&[b"\x1b]9;Claude needs you\x07\x1b]777;notify;T;B\x1b\\"]),
            vec![
                (0, 21, Found::Notify(9, "Claude needs you".into())),
                (0, 39, Found::Notify(777, "notify;T;B".into())),
            ]
        );
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

    #[test]
    fn an_osc_past_the_cap_is_cut_and_the_rest_skipped() {
        let mut s = Scanner::default();
        let mut out = vec![];
        s.feed(b"\x1b]52;c;", &mut out);
        let chunk = vec![b'A'; 1 << 20];
        s.feed(&chunk, &mut out);
        assert!(out.is_empty());
        s.feed(&chunk, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1, Found::CutOsc);
        out.clear();
        s.feed(&chunk, &mut out);
        s.feed(b"AAA\x07ok", &mut out);
        assert_eq!(out, vec![(4, Found::OscSkipped { esc: false })]);
        // Ended by another sequence's ESC: that ESC is the parser's again.
        let mut s = Scanner::default();
        let mut out = vec![];
        s.feed(b"\x1b]52;c;", &mut out);
        s.feed(&vec![b'A'; MAX_OSC + 10], &mut out);
        s.feed(b"\x1b", &mut out);
        s.feed(b"[>q", &mut out);
        assert_eq!(
            out[1..],
            [(0, Found::OscSkipped { esc: true }), (3, Found::XtVersion)]
        );
    }

    #[test]
    fn a_big_osc_that_ends_says_so() {
        let mut big = b"\x1b]52;c;".to_vec();
        big.extend(std::iter::repeat_n(b'A', 100_000));
        big.push(0x07);
        let found = scan(&[&big]);
        assert_eq!(found, vec![(0, big.len(), Found::BigOscEnded)]);
    }
}
