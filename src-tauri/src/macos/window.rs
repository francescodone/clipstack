//! Making the picker behave like a Spotlight panel.
//!
//! A plain Tauri window disappears behind another app's fullscreen Space and
//! hides itself the moment we lose focus, which is exactly the wrong behaviour
//! for a popup the user dismisses by clicking away.

use objc2_app_kit::{
    NSWindow, NSWindowCollectionBehavior, NSFloatingWindowLevel, NSMainMenuWindowLevel,
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
    unsafe {
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
    }
    Ok(())
}

pub fn raise(window: &WebviewWindow) -> Result<(), String> {
    unsafe { ns_window(window)?.setLevel(LEVEL_VISIBLE) }
    Ok(())
}

pub fn lower(window: &WebviewWindow) -> Result<(), String> {
    unsafe { ns_window(window)?.setLevel(LEVEL_HIDDEN) }
    Ok(())
}
