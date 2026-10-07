//! Which application the user was actually in.
//!
//! Showing our own panel makes us frontmost, so the picker would otherwise
//! paste into itself. Everything here exists to remember the app from *before*
//! that happened.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

/// The app the user was in before ClipStack took focus.
static PREVIOUS: Mutex<Option<Target>> = Mutex::new(None);

#[derive(Debug, Clone)]
pub struct Target {
    pub pid: i32,
    pub name: Option<String>,
    pub bundle_id: Option<String>,
}

fn workspace() -> objc2::rc::Retained<NSWorkspace> {
    NSWorkspace::sharedWorkspace()
}

fn running(pid: i32) -> Option<objc2::rc::Retained<NSRunningApplication>> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
}

fn describe(app: &NSRunningApplication) -> Target {
    Target {
        pid: app.processIdentifier(),
        name: app.localizedName().map(|n| n.to_string()),
        bundle_id: app.bundleIdentifier().map(|b| b.to_string()),
    }
}

fn is_self(pid: i32) -> bool {
    pid == std::process::id() as i32
}

/// The app that is frontmost right now, if any.
pub fn current() -> Option<Target> {
    workspace()
        .frontmostApplication()
        .map(|app| describe(&app))
}

pub fn current_name() -> Option<String> {
    current().and_then(|t| t.name)
}

pub fn current_bundle_id() -> Option<String> {
    current().and_then(|t| t.bundle_id)
}

/// Called by the watcher on every tick. Remembers the most recent app that was
/// frontmost and was not us, which is exactly the app a paste is meant for.
pub fn note_frontmost() {
    let Some(target) = current() else {
        return;
    };
    if is_self(target.pid) {
        return;
    }
    if let Ok(mut guard) = PREVIOUS.lock() {
        if guard.as_ref().map(|t| t.pid) != Some(target.pid) {
            *guard = Some(target);
        }
    }
}

/// ClipStack is about to take focus, so pin the current app as the target.
///
/// Must be called *before* the panel is shown.
pub fn capture_previous() {
    note_frontmost();
}

/// The app to paste into, falling back to whatever is frontmost right now.
pub fn paste_target() -> Option<i32> {
    let remembered = PREVIOUS.lock().ok().and_then(|g| g.clone());
    match remembered {
        Some(target) if running(target.pid).is_some() && !is_self(target.pid) => Some(target.pid),
        _ => current().map(|t| t.pid).filter(|pid| !is_self(*pid)),
    }
}

/// Bring `pid` back to the front and wait until AppKit agrees it is there.
///
/// Without this the synthetic Cmd+V can land while ClipStack is still key and
/// the keystroke is silently lost. Returns false if the app never came forward,
/// in which case the caller should post the event straight to the pid.
pub fn activate_and_wait(pid: i32, timeout: Duration) -> bool {
    let Some(app) = running(pid) else {
        return false;
    };
    // Deprecated on macOS 14, where the system ignores it; still required to
    // force activation on macOS 13 and earlier, where it is a no-op error to
    // omit it.
    #[allow(deprecated)]
    app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);

    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(front) = current() {
            if front.pid == pid {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    false
}
