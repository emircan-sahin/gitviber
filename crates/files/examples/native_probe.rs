//! Opens PDFKit's and AVKit's viewers for a file on the main thread and lets them go: whether the
//! frameworks load and the objects come up, without a window. macOS only.
//!
//! files-native-probe FILE.pdf [FILE.mp4]

fn main() {
    #[cfg(target_os = "macos")]
    {
        use gitviber_files::media::MediaKind;
        use gitviber_files::native_media::NativeMedia;
        let mut args = std::env::args().skip(1);
        let pdf = args.next().expect("a PDF");
        let video = args.next().unwrap_or_else(|| "/tmp/none.mp4".into());
        for (path, kind) in [(pdf, MediaKind::Pdf), (video, MediaKind::Video)] {
            let opened = NativeMedia::open(std::path::Path::new(&path), kind);
            println!(
                "{kind:?} {path}: {}",
                if opened.is_some() { "opened" } else { "none" }
            );
            drop(opened);
        }
    }
}
