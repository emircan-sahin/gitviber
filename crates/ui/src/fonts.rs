//! The fonts the app ships: Geist, Geist Mono and JetBrains Mono (SIL OFL 1.1, the licenses beside
//! them in assets/fonts), in the weights the settings reach: interface text 400–500 with labels a
//! step up and bold, code 300–600 with bold two steps up. GPUI's text system reads no variable-font
//! axes, so these are the static faces. A family reaches the text system the first time a setting
//! names it; the ones nobody picks never load.

use std::borrow::Cow;

use gpui_kit::{App, Global};

macro_rules! faces {
    ($dir:literal: $($face:literal),+ $(,)?) => {
        &[$(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/", $dir, "/", $face, ".ttf")) as &[u8]),+]
    };
}

const FAMILIES: &[(&str, &[&[u8]])] = &[
    (
        "Geist",
        faces!("geist": "Geist-Regular", "Geist-Italic", "Geist-Medium", "Geist-SemiBold",
            "Geist-Bold", "Geist-BoldItalic"),
    ),
    (
        "Geist Mono",
        faces!("geist-mono": "GeistMono-Light", "GeistMono-LightItalic", "GeistMono-Regular",
            "GeistMono-Italic", "GeistMono-Medium", "GeistMono-MediumItalic", "GeistMono-SemiBold",
            "GeistMono-SemiBoldItalic", "GeistMono-Bold", "GeistMono-BoldItalic",
            "GeistMono-ExtraBold", "GeistMono-ExtraBoldItalic"),
    ),
    (
        "JetBrains Mono",
        faces!("jetbrains-mono": "JetBrainsMono-Light", "JetBrainsMono-LightItalic",
            "JetBrainsMono-Regular", "JetBrainsMono-Italic", "JetBrainsMono-Medium",
            "JetBrainsMono-MediumItalic", "JetBrainsMono-SemiBold", "JetBrainsMono-SemiBoldItalic",
            "JetBrainsMono-Bold", "JetBrainsMono-BoldItalic", "JetBrainsMono-ExtraBold",
            "JetBrainsMono-ExtraBoldItalic"),
    ),
];

#[derive(Default)]
struct Loaded(Vec<&'static str>);
impl Global for Loaded {}

/// Hands `family`'s faces to the text system if the app ships it and they aren't there yet.
pub fn ensure(family: &str, cx: &mut App) {
    let Some((name, faces)) = FAMILIES.iter().find(|(name, _)| *name == family) else {
        return;
    };
    if cx.default_global::<Loaded>().0.contains(name) {
        return;
    }
    let fonts = faces.iter().map(|face| Cow::Borrowed(*face)).collect();
    match cx.text_system().add_fonts(fonts) {
        Ok(()) => cx.global_mut::<Loaded>().0.push(name),
        Err(err) => eprintln!("gitviber: couldn't load the bundled {name}: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_is_a_truetype_font() {
        for (name, faces) in FAMILIES {
            for face in *faces {
                assert_eq!(&face[..4], &[0, 1, 0, 0], "{name}");
            }
        }
    }
}
