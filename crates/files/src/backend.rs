//! The app's one Core: `gitviber_ui::core` once the app (or `dev::run`) made it; a test without
//! one gets a process-wide stand-in.

use std::sync::{Arc, OnceLock};

use gitviber_core::Core;
use gpui_kit::App;

pub fn core(cx: &App) -> Arc<Core> {
    if gitviber_ui::has_core(cx) {
        return gitviber_ui::core(cx);
    }
    static STANDIN: OnceLock<Arc<Core>> = OnceLock::new();
    STANDIN
        .get_or_init(|| Arc::new(Core::new(Arc::new(|_| {}))))
        .clone()
}
