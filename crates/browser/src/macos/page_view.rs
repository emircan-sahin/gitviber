//! What a tab's tools do to its page besides loading it: find in it, zoom it (outside device
//! mode, which sets its own zoom to fit), show it light or dark, and its reader mode.

use super::page_tools::ours;
use super::{agent, ask, main_thread, view, Delegate, GONE};
use crate::reader::{self, Look};
use crate::{Handle, PageState};
use block2::RcBlock;
use gitviber_core::Res;
use objc2::runtime::AnyObject;
use objc2::{sel, DefinedClass, Message as _};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
};
use objc2_foundation::{NSError, NSNumber, NSObjectProtocol, NSString};
use objc2_web_kit::{WKFindConfiguration, WKFindResult, WKWebView};
use std::cell::Cell;
use std::ptr::NonNull;
use std::time::Duration;

/// As far as look.ts's ZOOMS go.
const ZOOMS: std::ops::RangeInclusive<f64> = 0.5..=3.0;

/// The next match of `text` (or the one before), wrapping round: whether there was one. None
/// where this WebKit has no find. Waits on the main thread.
pub fn find(
    app: &Handle,
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
    zoom_view(&v.web, &v.delegate, zoom);
    Ok(())
}

pub(super) fn zoom_view(web: &WKWebView, delegate: &Delegate, zoom: f64) {
    let zoom = zoom.clamp(*ZOOMS.start(), *ZOOMS.end());
    delegate.ivars().zoom.set(zoom);
    if delegate.ivars().screen.get().is_none() {
        unsafe { web.setPageZoom(zoom) };
    }
}

/// The page light or dark (`prefers-color-scheme` follows), or as the app's window (None).
pub fn appearance(id: &str, dark: Option<bool>) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    show_as(id, &v.web, dark);
    Ok(())
}

/// Light, dark or as the app, and kept for the tab whoever chose it, the user or an agent: a parked
/// tab made again by an agent's command shows as it did.
pub(super) fn show_as(id: &str, web: &WKWebView, dark: Option<bool>) {
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
    web.setAppearance(named.as_deref());
    ask(id, |a| a.dark = dark);
}

/// Reader mode on with `look`, or off (None). Its answer comes back as the page's state.
pub fn reader_mode(id: &str, look: Option<&Look>) -> Res<()> {
    let mtm = main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let script = look.map_or_else(|| reader::HIDE.to_string(), reader::show_script);
    let delegate = v.delegate.clone();
    let done = RcBlock::new(move |answer: *mut AnyObject, _error: *mut NSError| {
        // SAFETY: WebKit's answer, an object or nil, alive for this call.
        let answer = unsafe { answer.as_ref() }
            .and_then(|a| a.downcast_ref::<NSString>())
            .map(|a| a.to_string());
        let d = delegate.ivars();
        d.reader.set(answer.as_deref() == Some("shown"));
        // Readability found less than its check promised: the button goes too.
        if answer.as_deref() == Some("none") {
            d.readerable.set(false);
        }
        delegate.changed();
    });
    unsafe {
        v.web
            .evaluateJavaScript_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(&script),
                None,
                &ours(mtm),
                Some(&done),
            )
    };
    Ok(())
}

/// The reader's look changed (its text, the app's theme) while it shows.
pub fn reader_style(id: &str, look: &Look) -> Res<()> {
    let mtm = main_thread()?;
    let v = view(id).ok_or(GONE)?;
    unsafe {
        v.web
            .evaluateJavaScript_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(&reader::style_script(look)),
                None,
                &ours(mtm),
                None,
            )
    };
    Ok(())
}

/// Whether a loaded page has an article, once for each address it shows; not in an agent's tab,
/// whose agent reads the page its own way.
pub(super) fn check_reader(delegate: &Delegate, web: &WKWebView, state: &PageState) {
    let d = delegate.ivars();
    let web_page = state.url.starts_with("http:") || state.url.starts_with("https:");
    if state.loading || state.reader || !web_page || *d.reader_checked.borrow() == state.url {
        return;
    }
    if agent::routed(&d.id) {
        return;
    }
    let Ok(mtm) = main_thread() else {
        return;
    };
    *d.reader_checked.borrow_mut() = state.url.clone();
    let (this, url) = (delegate.retain(), state.url.clone());
    let done = RcBlock::new(move |answer: *mut AnyObject, _error: *mut NSError| {
        // SAFETY: as in `reader_mode`.
        let yes = unsafe { answer.as_ref() }
            .and_then(|a| a.downcast_ref::<NSNumber>())
            .is_some_and(|n| n.boolValue());
        let d = this.ivars();
        // Moved on meanwhile: the answer was the last page's.
        if *d.reader_checked.borrow() == url && d.readerable.replace(yes) != yes {
            this.changed();
        }
    });
    unsafe {
        web.evaluateJavaScript_inFrame_inContentWorld_completionHandler(
            &NSString::from_str(&reader::check_script()),
            None,
            &ours(mtm),
            Some(&done),
        )
    };
}
