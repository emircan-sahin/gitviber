//! An agent tab's cookies, for its sessions (browser/sessions.rs): read, set and cleared only in
//! a store of its own, never in the user's.

use super::{asked, main_thread};
use crate::sessions::Cookie;
use block2::RcBlock;
use gitviber_core::Res;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{
    ns_string, NSArray, NSDate, NSDictionary, NSHTTPCookie, NSHTTPCookieDomain,
    NSHTTPCookieExpires, NSHTTPCookieName, NSHTTPCookiePath, NSHTTPCookieSameSiteLax,
    NSHTTPCookieSameSitePolicy, NSHTTPCookieSameSiteStrict, NSHTTPCookieSecure, NSHTTPCookieValue,
    NSString,
};
use objc2_web_kit::WKWebsiteDataStore;
use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

/// The user's own logins never go into a session, nor does one go over them.
const SHARED: &str = "This tab is in your own cookies and logins (--shared): sessions save, \
                      load and clear only in a tab of its own.";

/// The tab has a store of its own, which sessions may change; an error for a shared one.
pub fn agent_own_store(id: &str) -> Res<()> {
    main_thread()?;
    own(id).map(drop)
}

pub fn agent_cookies(id: &str, done: Box<dyn FnOnce(Vec<Cookie>)>) -> Res<()> {
    main_thread()?;
    let store = own(id)?;
    let done = Cell::new(Some(done));
    let block = RcBlock::new(move |cookies: NonNull<NSArray<NSHTTPCookie>>| {
        if let Some(done) = done.take() {
            // SAFETY: WebKit's array, alive for this call.
            let cookies = unsafe { cookies.as_ref() }.to_vec();
            done(cookies.iter().map(|c| read(c)).collect());
        }
    });
    unsafe { store.httpCookieStore().getAllCookies(&block) };
    Ok(())
}

/// Each of `cookies` set in the tab's own store; `done` once WebKit has them all.
pub fn agent_set_cookies(id: &str, cookies: Vec<Cookie>, done: Box<dyn FnOnce(())>) -> Res<()> {
    main_thread()?;
    let jar = unsafe { own(id)?.httpCookieStore() };
    let made: Vec<Retained<NSHTTPCookie>> = cookies.iter().filter_map(made).collect();
    if made.is_empty() {
        done(());
        return Ok(());
    }
    let left = Rc::new(Cell::new(made.len()));
    let done = Rc::new(Cell::new(Some(done)));
    for cookie in made {
        let (left, done) = (left.clone(), done.clone());
        let block = RcBlock::new(move || {
            left.set(left.get() - 1);
            if left.get() == 0 {
                if let Some(done) = done.take() {
                    done(());
                }
            }
        });
        unsafe { jar.setCookie_completionHandler(&cookie, Some(&block)) };
    }
    Ok(())
}

/// Everything the tab's own store keeps, gone: cookies, storage, caches.
pub fn agent_clear_store(id: &str, done: Box<dyn FnOnce(())>) -> Res<()> {
    let mtm = main_thread()?;
    let store = own(id)?;
    let done = Cell::new(Some(done));
    let block = RcBlock::new(move || {
        if let Some(done) = done.take() {
            done(());
        }
    });
    unsafe {
        store.removeDataOfTypes_modifiedSince_completionHandler(
            &WKWebsiteDataStore::allWebsiteDataTypes(mtm),
            &NSDate::distantPast(),
            &block,
        )
    };
    Ok(())
}

fn own(id: &str) -> Res<Retained<WKWebsiteDataStore>> {
    asked(id).store.ok_or_else(|| SHARED.into())
}

fn read(c: &NSHTTPCookie) -> Cookie {
    Cookie {
        name: c.name().to_string(),
        value: c.value().to_string(),
        domain: c.domain().to_string(),
        path: c.path().to_string(),
        expires: c.expiresDate().map(|d| d.timeIntervalSince1970()),
        secure: c.isSecure(),
        http_only: c.isHTTPOnly(),
        same_site: c
            .sameSitePolicy()
            .map(|s| s.to_string().to_ascii_lowercase()),
    }
}

/// As NSHTTPCookie takes one; None for one it won't.
fn made(c: &Cookie) -> Option<Retained<NSHTTPCookie>> {
    let text = |s: &str| Retained::into_super(Retained::into_super(NSString::from_str(s)));
    let yes = || text("TRUE");
    // SAFETY: Foundation's own constant keys and values.
    let mut pairs: Vec<(&NSString, Retained<AnyObject>)> = unsafe {
        vec![
            (NSHTTPCookieName, text(&c.name)),
            (NSHTTPCookieValue, text(&c.value)),
            (NSHTTPCookieDomain, text(&c.domain)),
            (NSHTTPCookiePath, text(&c.path)),
        ]
    };
    if let Some(at) = c.expires {
        let date = NSDate::dateWithTimeIntervalSince1970(at);
        pairs.push((
            unsafe { NSHTTPCookieExpires },
            Retained::into_super(Retained::into_super(date)),
        ));
    }
    if c.secure {
        pairs.push((unsafe { NSHTTPCookieSecure }, yes()));
    }
    // Not one of Foundation's named keys, though it reads it.
    if c.http_only {
        pairs.push((ns_string!("HttpOnly"), yes()));
    }
    let same_site = match c.same_site.as_deref() {
        Some("lax") => Some(unsafe { NSHTTPCookieSameSiteLax }),
        Some("strict") => Some(unsafe { NSHTTPCookieSameSiteStrict }),
        _ => None,
    };
    if let Some(policy) = same_site {
        pairs.push((
            unsafe { NSHTTPCookieSameSitePolicy },
            text(&policy.to_string()),
        ));
    }
    let keys: Vec<&NSString> = pairs.iter().map(|p| p.0).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|p| &*p.1).collect();
    let properties = NSDictionary::<NSString, AnyObject>::from_slices(&keys, &values);
    unsafe { NSHTTPCookie::cookieWithProperties(&properties) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cookie_comes_back_as_it_was_saved_httponly_and_samesite_too() {
        // Within the 400 days Foundation caps an expiry at.
        let tomorrow = NSDate::now().timeIntervalSince1970().floor() + 86_400.0;
        let saved = Cookie {
            name: "sid".into(),
            value: "s3cret".into(),
            domain: ".app.test".into(),
            path: "/".into(),
            expires: Some(tomorrow),
            secure: true,
            http_only: true,
            same_site: Some("strict".into()),
        };
        assert_eq!(read(&made(&saved).unwrap()), saved);
        let plain = Cookie {
            expires: None,
            secure: false,
            http_only: false,
            same_site: None,
            ..saved.clone()
        };
        assert_eq!(read(&made(&plain).unwrap()), plain);
    }
}
