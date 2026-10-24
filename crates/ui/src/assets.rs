//! Everything the windows load by path: the file icons (file-icons/<name>.svg), the app's logo
//! (app/logo.svg) and the whole Lucide catalog, so any `IconName` works without listing it first.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

use crate::widgets::file_icon;

pub struct Assets;

pub const LOGO: &str = "app/logo.svg";

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = file_icon::svg(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if path == LOGO {
            return Ok(Some(Cow::Borrowed(include_bytes!("../assets/logo.svg"))));
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out = file_icon::list(path);
        out.extend(gpui_kit::assets::AllAssets.list(path)?);
        Ok(out)
    }
}
