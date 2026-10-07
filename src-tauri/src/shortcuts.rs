//! Registering the "paste from stack" shortcut and rebinding it at runtime.

use std::sync::Mutex;

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// The accelerator currently held by the system, so a failed rebind can restore it.
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

/// Register `accelerator`, replacing whatever was registered before.
///
/// Returns an error if another app already owns the combination, in which case
/// the previous binding is put back so the user is never left without one.
pub fn rebind(app: &AppHandle, accelerator: &str) -> Result<(), String> {
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|_| format!("\"{accelerator}\" is not a valid shortcut"))?;

    let previous = CURRENT.lock().ok().and_then(|g| g.clone());

    unregister(app);

    match app.global_shortcut().register(shortcut) {
        Ok(()) => {
            if let Ok(mut guard) = CURRENT.lock() {
                *guard = Some(accelerator.to_string());
            }
            Ok(())
        }
        Err(err) => {
            // Roll back so a rejected rebind does not disable the app.
            if let Some(prev) = previous {
                if let Ok(parsed) = prev.parse::<Shortcut>() {
                    let _ = app.global_shortcut().register(parsed);
                    if let Ok(mut guard) = CURRENT.lock() {
                        *guard = Some(prev);
                    }
                }
            }
            Err(format!("\"{accelerator}\" could not be registered ({err}). It is probably taken by another app."))
        }
    }
}

pub fn unregister(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
    if let Ok(mut guard) = CURRENT.lock() {
        *guard = None;
    }
}
