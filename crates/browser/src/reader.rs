//! Reader mode: a page's article shown over it as a page of its own, set for reading
//! (scripts/reader.js on scripts/vendor/Readability.js), in GitViber's content world. The
//! library goes into a page only when its reader is asked for; after a load, only its small
//! check of whether the page has an article does.

use serde::{Deserialize, Serialize};

const READABILITY: &str = include_str!("scripts/vendor/Readability.js");
const READERABLE: &str = include_str!("scripts/vendor/Readability-readerable.js");
const READER: &str = include_str!("scripts/reader.js");

/// The text sizes the reader steps through, in CSS px.
pub const SIZES: &[u32] = &[15, 17, 19, 21, 24, 27, 30];

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Font {
    #[default]
    Serif,
    Sans,
}

/// How the reader sets its text, the same in every tab (ui/store.rs keeps it).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct Text {
    pub size: u32,
    pub font: Font,
}

impl Default for Text {
    fn default() -> Self {
        Self {
            size: 19,
            font: Font::Serif,
        }
    }
}

impl Text {
    /// The size `by` steps up (or down) from the nearest of SIZES, held at their ends.
    pub fn step(self, by: i32) -> Self {
        let at = (0..SIZES.len())
            .min_by_key(|&i| SIZES[i].abs_diff(self.size))
            .unwrap_or(0);
        let to = (at as i32 + by).clamp(0, SIZES.len() as i32 - 1) as usize;
        Self {
            size: SIZES[to],
            ..self
        }
    }

    pub fn smallest(self) -> bool {
        self.size <= SIZES[0]
    }

    pub fn largest(self) -> bool {
        self.size >= SIZES[SIZES.len() - 1]
    }
}

/// A theme's colors as the reader uses them, CSS hex.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Colors {
    pub bg: String,
    pub fg: String,
    pub muted: String,
    pub link: String,
    pub rule: String,
    pub code: String,
}

/// The app's light and dark colors, the page picking by its own appearance, and the text.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub light: Colors,
    pub dark: Colors,
    #[serde(flatten)]
    pub text: Text,
}

fn json(look: &Look) -> String {
    serde_json::to_string(look).unwrap_or_else(|_| "{}".into())
}

/// Shows the article: "shown", or "none" when the page has none after all.
pub(crate) fn show_script(look: &Look) -> String {
    format!(
        "{READABILITY}\n{READER}\nwindow.__gvReader.show({});",
        json(look)
    )
}

/// The look changed while the reader shows.
pub(crate) fn style_script(look: &Look) -> String {
    format!("{READER}\nwindow.__gvReader.style({});", json(look))
}

/// Back to the page beneath, as it was.
pub(crate) const HIDE: &str = "window.__gvReader ? window.__gvReader.hide() : \"hidden\";";

/// Whether the page likely has an article: a few paragraphs' worth of text, not the whole parse.
pub(crate) fn check_script() -> String {
    format!("{READERABLE}\nisProbablyReaderable(document);")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_steps_through_its_sizes_and_stops_at_their_ends() {
        let text = Text::default();
        assert_eq!(text.size, 19);
        assert_eq!(text.step(1).size, 21);
        assert_eq!(text.step(-1).size, 17);
        assert_eq!(text.step(9).size, 30);
        assert!(text.step(9).largest() && !text.step(9).smallest());
        assert_eq!(text.step(-9).size, 15);
        assert!(text.step(-9).smallest());
        // A size kept by hand, or by an older list, steps from the nearest.
        assert_eq!(Text { size: 22, ..text }.step(1).size, 24);
        assert_eq!(Text { size: 3, ..text }.step(0).size, 15);
    }

    #[test]
    fn the_kept_text_reads_back_and_fills_in_what_it_lacks() {
        let text = Text {
            size: 24,
            font: Font::Sans,
        };
        let kept = serde_json::to_string(&text).unwrap();
        assert_eq!(kept, r#"{"size":24,"font":"sans"}"#);
        assert_eq!(serde_json::from_str::<Text>(&kept).unwrap(), text);
        let partial: Text = serde_json::from_str(r#"{"font":"sans"}"#).unwrap();
        assert_eq!(partial.size, 19);
    }

    #[test]
    fn the_scripts_call_the_reader_with_its_look() {
        let look = Look {
            light: Colors {
                bg: "#ffffff".into(),
                ..Colors::default()
            },
            ..Look::default()
        };
        let show = show_script(&look);
        assert!(show.starts_with(READABILITY));
        assert!(show.ends_with(
            r##"window.__gvReader.show({"light":{"bg":"#ffffff","fg":"","muted":"","link":"","rule":"","code":""},"dark":{"bg":"","fg":"","muted":"","link":"","rule":"","code":""},"size":19,"font":"serif"});"##
        ));
        // Restyling doesn't take the library in again.
        assert!(!style_script(&look).contains(READABILITY));
        assert!(check_script().ends_with("isProbablyReaderable(document);"));
        assert!(!check_script().contains(READER));
    }
}
