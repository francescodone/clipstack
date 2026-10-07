//! Making the picker behave like a Spotlight panel.
//!
//! A plain Tauri window disappears behind another app's fullscreen Space and
//! hides itself the moment we lose focus, which is exactly the wrong behaviour
//! for a popup the user dismisses by clicking away.

use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSWindow, NSWindowCollectionBehavior, NSFloatingWindowLevel,
    NSMainMenuWindowLevel,
};
use tauri::WebviewWindow;

/// Floats above everything, including another app's fullscreen Space.
/// `NSWindowLevel` is an `NSInteger`, hence `isize`.
const LEVEL_VISIBLE: isize = NSMainMenuWindowLevel;
/// While hidden, sit below status-item menus so a closed panel cannot cover a
/// dropdown from the menu bar.
const LEVEL_HIDDEN: isize = NSFloatingWindowLevel;

fn ns_window(window: &WebviewWindow) -> Result<&'static NSWindow, String> {
    let raw = window
        .ns_window()
        .map_err(|e| format!("could not reach the native window: {e}"))?;
    if raw.is_null() {
        return Err("the native window is gone".to_string());
    }
    Ok(unsafe { &*(raw as *const NSWindow) })
}

/// Apply the panel behaviour once, right after the window is built.
pub fn configure_panel(window: &WebviewWindow) -> Result<(), String> {
    let ns = ns_window(window)?;
    // `CanJoinAllSpaces` and `MoveToActiveSpace` are mutually exclusive;
    // OR-ing both misbehaves, so only the former is used here.
    ns.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    // Without this the panel vanishes the instant we activate it.
    ns.setHidesOnDeactivate(false);
    ns.setLevel(LEVEL_HIDDEN);
    Ok(())
}

pub fn raise(window: &WebviewWindow) -> Result<(), String> {
    ns_window(window)?.setLevel(LEVEL_VISIBLE);
    Ok(())
}

/// Activate the app and make the panel key, so it actually receives keys.
///
/// An accessory (menu-bar-only) app is never activated by `show()` or
/// `set_focus()` alone, and a window that is not key never becomes first
/// responder — so the webview silently swallows nothing: it never sees a
/// single keystroke, and Enter in particular looks dead while mouse clicks
/// keep working (clicks do not require key status). This is the sequence
/// Spotlight-style panels use.
pub fn focus_panel(window: &WebviewWindow) -> Result<(), String> {
    // AppKit activation has to happen on the main thread; the global-shortcut
    // handler is not guaranteed to run there.
    let Some(mtm) = MainThreadMarker::new() else {
        let panel = window.clone();
        return window
            .run_on_main_thread(move || {
                let _ = focus_panel(&panel);
            })
            .map_err(|e| e.to_string());
    };

    let ns = ns_window(window)?;
    let app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    ns.makeKeyAndOrderFront(None);
    // Covers the case where activation lands a beat late and the window
    // would otherwise stay ordered out.
    ns.orderFrontRegardless();
    Ok(())
}

/// Resign active status after the panel is hidden.
///
/// `focus_panel` made ClipStack the active app; an accessory app with no
/// visible window must not keep the menu bar and key focus to itself, so we
/// step aside and let the system hand control back.
pub fn release_focus(window: &WebviewWindow) -> Result<(), String> {
    let Some(mtm) = MainThreadMarker::new() else {
        let panel = window.clone();
        return window
            .run_on_main_thread(move || {
                let _ = release_focus(&panel);
            })
            .map_err(|e| e.to_string());
    };
    NSApplication::sharedApplication(mtm).deactivate();
    Ok(())
}

pub fn lower(window: &WebviewWindow) -> Result<(), String> {
    ns_window(window)?.setLevel(LEVEL_HIDDEN);
    Ok(())
}
