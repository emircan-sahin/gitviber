//! Desktop notifications (src/lib/app/notify.ts). On macOS they go through
//! UNUserNotificationCenter, as cmux does: permission is asked for when the user turns them on
//! (which is what lists the app in System Settings → Notifications), the answer can be read
//! back, and a click comes back to us. tauri-plugin-notification's desktop side reports
//! "granted" without asking and posts through the NSUserNotificationCenter deprecated since
//! macOS 11. It stays for Windows and Linux, and for a dev build that couldn't become an app
//! bundle (dev_bundle.rs): UN needs one, and there the plugin posts as Terminal.

use serde::Serialize;
use tauri::AppHandle;

// Off macOS it's always Granted.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Granted,
    Denied,
    /// Not asked yet.
    Prompt,
    /// Allowed without banners (alert style None): they only go to Notification Center.
    Quiet,
    /// Not an app bundle (a dev build whose bundle failed): the plugin's notifications, unasked,
    /// shown as Terminal's.
    Unbundled,
}

/// Takes clicks from here on.
pub fn setup(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    mac::setup(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn permission() -> Result<Permission, String> {
    #[cfg(target_os = "macos")]
    return mac::permission();
    #[cfg(not(target_os = "macos"))]
    Ok(Permission::Granted)
}

/// The running app's bundle id, which a dev build's own bundle changes (dev_bundle.rs).
pub fn bundle_id() -> Option<String> {
    #[cfg(target_os = "macos")]
    return mac::bundle_id();
    #[cfg(not(target_os = "macos"))]
    None
}

/// Asks the OS, which asks the user once; after that it answers as they did.
pub fn request() -> Result<Permission, String> {
    #[cfg(target_os = "macos")]
    return mac::request();
    #[cfg(not(target_os = "macos"))]
    Ok(Permission::Granted)
}

/// `target`: what a click on it should show (the page reads it, "pane:<id>"), given back
/// with the "notification-click" event.
pub fn send(app: &AppHandle, title: &str, body: &str, target: Option<&str>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if let Some(sent) = mac::send(title, body, target) {
        return sent;
    }
    let _ = target;
    use tauri_plugin_notification::NotificationExt;
    let mut note = app.notification().builder().title(title);
    if !body.is_empty() {
        note = note.body(body);
    }
    note.show().map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Permission;
    use crate::objc::{c_string, ns_string, rust_string};
    use block2::{Block, RcBlock};
    use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject};
    use objc2::{define_class, msg_send, ClassType};
    use std::ffi::CStr;
    use std::sync::{mpsc, OnceLock};
    use std::time::Duration;
    use tauri::{AppHandle, Emitter};

    #[link(name = "UserNotifications", kind = "framework")]
    extern "C" {}

    static APP: OnceLock<AppHandle> = OnceLock::new();

    // cmux bounds every call the same way: one stuck in XPC to usernotificationsd would
    // otherwise hold a thread for good.
    const REPLY: Duration = Duration::from_secs(2);
    // The answer to the permission prompt comes once the user clicks, so it gets longer.
    const ANSWER: Duration = Duration::from_secs(120);
    const SOUND_AND_ALERT: usize = 1 << 1 | 1 << 2; // UNAuthorizationOptions
    const BANNER_LIST_SOUND: usize = 1 << 4 | 1 << 3 | 1 << 1; // UNNotificationPresentationOptions
    const TARGET: &CStr = c"target";

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and Delegate has no Drop.
        #[unsafe(super(NSObject))]
        #[name = "GitViberNotificationDelegate"]
        struct Delegate;

        impl Delegate {
            // Without it macOS keeps a notification out of sight while the app is in front:
            // the test one is sent from Settings.
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: *mut AnyObject,
                _note: *mut AnyObject,
                done: &Block<dyn Fn(usize)>,
            ) {
                done.call((BANNER_LIST_SOUND,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: *mut AnyObject,
                response: *mut AnyObject,
                done: &Block<dyn Fn()>,
            ) {
                let target = unsafe {
                    let note: *mut AnyObject = msg_send![response, notification];
                    target_of(msg_send![note, request])
                };
                if let Some(app) = APP.get() {
                    clicked(app, target);
                }
                done.call(());
            }
        }
    );

    /// A click on one of ours: the app comes to the front, and the page shows what it was about.
    fn clicked(app: &AppHandle, target: Option<String>) {
        crate::opened::raise(app);
        let _ = app.emit("notification-click", target);
    }

    /// The app's center, inside an app bundle only: elsewhere `currentNotificationCenter`
    /// throws, and the app aborts.
    fn center() -> Option<*mut AnyObject> {
        static BUNDLED: OnceLock<bool> = OnceLock::new();
        let bundled = *BUNDLED.get_or_init(|| {
            bundle_id().is_some()
                && objc2::rc::autoreleasepool(|_| unsafe {
                    let main: *mut AnyObject = msg_send![AnyClass::get(c"NSBundle")?, mainBundle];
                    rust_string(msg_send![main, bundlePath])
                })
                .is_some_and(|p| p.ends_with(".app"))
        });
        if !bundled {
            return None;
        }
        let center: *mut AnyObject = unsafe {
            msg_send![
                AnyClass::get(c"UNUserNotificationCenter")?,
                currentNotificationCenter
            ]
        };
        (!center.is_null()).then_some(center)
    }

    pub fn setup(app: &AppHandle) {
        let _ = APP.set(app.clone());
        let Some(center) = center() else { return };
        unsafe {
            // The center holds its delegate weakly: this one is never released.
            let delegate: *mut AnyObject = msg_send![Delegate::class(), new];
            let _: () = msg_send![center, setDelegate: delegate];
        }
    }

    /// Runs `call` with a sender for its completion handler's answer, and waits that long for it.
    fn ask<T: 'static>(wait: Duration, call: impl FnOnce(mpsc::Sender<T>)) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        call(tx);
        rx.recv_timeout(wait).ok()
    }

    unsafe fn error_text(error: *mut AnyObject) -> String {
        let text: *mut AnyObject = msg_send![error, localizedDescription];
        rust_string(text).unwrap_or_else(|| "macOS refused".into())
    }

    // Each runs in a pool of its own: the commands call them on worker threads, which have none.
    pub fn permission() -> Result<Permission, String> {
        objc2::rc::autoreleasepool(|_| {
            let Some(center) = center() else {
                return Ok(Permission::Unbundled);
            };
            let settings = ask(REPLY, |tx| {
                let done = RcBlock::new(move |settings: *mut AnyObject| unsafe {
                    let status: isize = msg_send![settings, authorizationStatus];
                    let alerts: isize = msg_send![settings, alertSetting];
                    let style: isize = msg_send![settings, alertStyle];
                    let listed: isize = msg_send![settings, notificationCenterSetting];
                    let _ = tx.send((status, alerts, style, listed));
                });
                unsafe {
                    let _: () =
                        msg_send![center, getNotificationSettingsWithCompletionHandler: &*done];
                }
            });
            settings
                .map(|(status, alerts, style, listed)| {
                    map_permission(status, alerts, style, listed)
                })
                .ok_or_else(|| "macOS didn't say whether notifications are allowed".into())
        })
    }

    /// UNAuthorizationStatus; UNNotificationSetting for alerts; UNAlertStyle; UNNotificationSetting
    /// for Notification Center. Authorized isn't shown: in System Settings alerts can be off, or
    /// their style None, with the app still authorized (MonoCode's map_permission). Then only
    /// "Show in Notification Center" still keeps them, quietly; without it macOS takes every
    /// notification and shows none.
    pub(super) fn map_permission(
        status: isize,
        alerts: isize,
        style: isize,
        listed: isize,
    ) -> Permission {
        const NOT_DETERMINED: isize = 0;
        const DENIED: isize = 1;
        const DISABLED: isize = 1;
        const ENABLED: isize = 2;
        const NONE: isize = 0;
        match status {
            NOT_DETERMINED => Permission::Prompt,
            DENIED => Permission::Denied,
            _ if alerts != DISABLED && style != NONE => Permission::Granted,
            _ if listed == ENABLED => Permission::Quiet,
            _ => Permission::Denied,
        }
    }

    /// The running app's own id: the dev bundle's isn't the config's.
    pub fn bundle_id() -> Option<String> {
        objc2::rc::autoreleasepool(|_| unsafe {
            let main: *mut AnyObject = msg_send![AnyClass::get(c"NSBundle")?, mainBundle];
            rust_string(msg_send![main, bundleIdentifier])
        })
    }

    pub fn request() -> Result<Permission, String> {
        objc2::rc::autoreleasepool(|_| {
            let Some(center) = center() else {
                return Ok(Permission::Unbundled);
            };
            let answer = ask(ANSWER, |tx| {
                let done = RcBlock::new(move |granted: Bool, error: *mut AnyObject| {
                    let _ = tx.send(if error.is_null() {
                        Ok(granted.as_bool())
                    } else {
                        Err(unsafe { error_text(error) })
                    });
                });
                unsafe {
                    let _: () = msg_send![center, requestAuthorizationWithOptions: SOUND_AND_ALERT, completionHandler: &*done];
                }
            });
            match answer {
                Some(Ok(true)) => Ok(Permission::Granted),
                Some(Ok(false)) => Ok(Permission::Denied),
                Some(Err(e)) => Err(e),
                // Still unanswered: the prompt waits in Notification Center.
                None => Ok(Permission::Prompt),
            }
        })
    }

    /// None outside an app bundle, where the plugin posts instead.
    pub fn send(title: &str, body: &str, target: Option<&str>) -> Option<Result<(), String>> {
        objc2::rc::autoreleasepool(|_| {
            let center = center()?;
            // Asked here, not trusted from the page: macOS takes a notification it won't show
            // (alerts turned off in System Settings) without an error.
            match permission() {
                Ok(Permission::Granted | Permission::Quiet) => {}
                Ok(_) => {
                    return Some(Err(
                        "GitViber isn't allowed to show notifications: see System Settings → Notifications".into(),
                    ))
                }
                Err(e) => return Some(Err(e)),
            }
            let sent = ask(REPLY, |tx| unsafe {
                let Some(request) = build(title, body, target) else {
                    let _ = tx.send(Err("Could not build the notification".to_string()));
                    return;
                };
                let done = RcBlock::new(move |error: *mut AnyObject| {
                    let _ = tx.send(if error.is_null() {
                        Ok(())
                    } else {
                        Err(error_text(error))
                    });
                });
                let _: () = msg_send![center, addNotificationRequest: request, withCompletionHandler: &*done];
            });
            // No answer in time isn't a failure: it's on its way, or macOS is slow to say.
            Some(sent.unwrap_or(Ok(())))
        })
    }

    /// An autoreleased UNNotificationRequest, shown at once, with the default sound.
    pub(super) unsafe fn build(
        title: &str,
        body: &str,
        target: Option<&str>,
    ) -> Option<*mut AnyObject> {
        let content: *mut AnyObject =
            msg_send![AnyClass::get(c"UNMutableNotificationContent")?, new];
        if content.is_null() {
            return None;
        }
        let filled = (|| {
            let _: () = msg_send![content, setTitle: ns_string(&c_string(title))];
            let _: () = msg_send![content, setBody: ns_string(&c_string(body))];
            let sound: *mut AnyObject =
                msg_send![AnyClass::get(c"UNNotificationSound")?, defaultSound];
            let _: () = msg_send![content, setSound: sound];
            if let Some(target) = target {
                let info: *mut AnyObject = msg_send![
                    AnyClass::get(c"NSDictionary")?,
                    dictionaryWithObject: ns_string(&c_string(target)),
                    forKey: ns_string(TARGET)
                ];
                let _: () = msg_send![content, setUserInfo: info];
            }
            let uuid: *mut AnyObject = msg_send![AnyClass::get(c"NSUUID")?, UUID];
            let id: *mut AnyObject = msg_send![uuid, UUIDString];
            let request: *mut AnyObject = msg_send![
                AnyClass::get(c"UNNotificationRequest")?,
                requestWithIdentifier: id,
                content: content,
                trigger: std::ptr::null_mut::<AnyObject>()
            ];
            (!request.is_null()).then_some(request)
        })();
        let _: () = msg_send![content, release];
        filled
    }

    /// What a click on `request` gives back.
    pub(super) unsafe fn target_of(request: *mut AnyObject) -> Option<String> {
        let content: *mut AnyObject = msg_send![request, content];
        let info: *mut AnyObject = msg_send![content, userInfo];
        let value: *mut AnyObject = msg_send![info, objectForKey: ns_string(TARGET)];
        rust_string(value)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::*;

    /// A test binary isn't an app bundle: UN is never touched (it would abort), and the
    /// framework is linked for the bundled app.
    #[cfg(target_os = "macos")]
    #[test]
    fn stays_off_outside_an_app_bundle() {
        assert_eq!(permission(), Ok(Permission::Unbundled));
        assert_eq!(request(), Ok(Permission::Unbundled));
        assert!(objc2::runtime::AnyClass::get(c"UNUserNotificationCenter").is_some());
    }

    /// Authorized isn't enough: alerts off, or their style None, shows nothing unless Notification
    /// Center keeps them.
    #[cfg(target_os = "macos")]
    #[test]
    fn allowed_means_they_show_somewhere() {
        use mac::map_permission;
        assert_eq!(map_permission(0, 0, 0, 0), Permission::Prompt);
        assert_eq!(map_permission(1, 2, 1, 2), Permission::Denied);
        assert_eq!(map_permission(2, 2, 1, 2), Permission::Granted);
        assert_eq!(
            map_permission(2, 2, 2, 1),
            Permission::Granted,
            "alerts, not listed"
        );
        assert_eq!(
            map_permission(3, 2, 1, 2),
            Permission::Granted,
            "provisional"
        );
        assert_eq!(
            map_permission(2, 1, 1, 1),
            Permission::Denied,
            "alerts off, not listed"
        );
        assert_eq!(
            map_permission(2, 2, 0, 1),
            Permission::Denied,
            "style None, not listed"
        );
        assert_eq!(
            map_permission(2, 2, 0, 2),
            Permission::Quiet,
            "style None, listed"
        );
        assert_eq!(
            map_permission(2, 1, 1, 2),
            Permission::Quiet,
            "alerts off, listed"
        );
    }

    /// What's sent carries the target a click gives back; a NUL from a terminal doesn't stop it.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_request_carries_its_target() {
        objc2::rc::autoreleasepool(|_| unsafe {
            let request = mac::build("Claude Code", "Needs\0 you", Some("pane:7")).unwrap();
            assert_eq!(mac::target_of(request), Some("pane:7".into()));
            let request = mac::build("Push finished", "", None).unwrap();
            assert_eq!(mac::target_of(request), None);
        });
    }
}
