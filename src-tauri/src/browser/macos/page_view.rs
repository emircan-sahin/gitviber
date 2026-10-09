//! What a tab's tools do to its page besides loading it: find in it, zoom it (outside device
//! mode, which sets its own zoom to fit), and show it light or dark.

use super::{main_thread, view, GONE};
use crate::state::Res;
use block2::RcBlock;
use objc2::{sel, DefinedClass};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
};
use objc2_foundation::{NSObjectProtocol, NSString};
use objc2_web_kit::{WKFindConfiguration, WKFindResult};
use std::cell::Cell;
use std::ptr::NonNull;
use std::time::Duration;
use tauri::AppHandle;

/// As far as the tab's steps go (zoom.ts), Safari's.
const ZOOMS: std::ops::RangeInclusive<f64> = 0.5..=3.0;

/// The next match of `text` (or the one before), wrapping round: whether there was one. None
/// where this WebKit has no find. Waits on the main thread.
pub fn find(
    app: &AppHandle,
    id: String,
    text: String,
    backwards: bool,
    case_sensitive: bool,
) -> Res<Option<bool>> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let (Ok(mtm), Some(v)) = (main_thread(), view(&id)) else {
            return drop(tx.send(Err(GONE.to_string())));
        };
        if !v
            .web
            .respondsToSelector(sel!(findString:withConfiguration:completionHandler:))
        {
            return drop(tx.send(Ok(None)));
        }
        let config = unsafe { WKFindConfiguration::new(mtm) };
        unsafe {
            config.setBackwards(backwards);
            config.setCaseSensitive(case_sensitive);
            config.setWraps(true);
        }
        let tx = Cell::new(Some(tx));
        let block = RcBlock::new(move |result: NonNull<WKFindResult>| {
            if let Some(tx) = tx.take() {
                // SAFETY: WebKit's result, alive for this call.
                let _ = tx.send(Ok(Some(unsafe { result.as_ref().matchFound() })));
            }
        });
        unsafe {
            v.web.findString_withConfiguration_completionHandler(
                &NSString::from_str(&text),
                Some(&config),
                &block,
            )
        };
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|_| "The page didn't answer.".to_string())?
}

/// The page's own zoom, 1 for 100%; device mode's, while it's on, wins until it's off.
pub fn zoom(id: &str, zoom: f64) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let zoom = zoom.clamp(*ZOOMS.start(), *ZOOMS.end());
    v.delegate.ivars().zoom.set(zoom);
    if v.delegate.ivars().screen.get().is_none() {
        unsafe { v.web.setPageZoom(zoom) };
    }
    Ok(())
}

/// The page light or dark (`prefers-color-scheme` follows), or as the app's window (None).
pub fn appearance(id: &str, dark: Option<bool>) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let named = dark.and_then(|dark| {
        // SAFETY: AppKit's own names, there for the life of the app.
        let name = unsafe {
            if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        NSAppearance::appearanceNamed(name)
    });
    v.web.setAppearance(named.as_deref());
    Ok(())
}
