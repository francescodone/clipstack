//! The settings window.
//!
//! ClipStack normally runs as an accessory app with no Dock icon. Opening
//! settings temporarily promotes it to a regular app so the window behaves
//! like any other app window, and it is demoted again on close.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "settings";

pub fn open_settings(app: AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        promote(&app);
        return;
    }

    let builder = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App("settings.html".into()))
        .title("ClipStack Settings")
        .inner_size(560.0, 620.0)
        .min_inner_size(480.0, 420.0)
        // Transparent so the NSVisualEffectView installed below shows through,
        // which is what makes the window read as a native System Settings pane.
        .transparent(true)
        .resizable(true)
        .maximizable(false)
        .closable(true)
        .focused(true)
        .visible(true);

    match builder.build() {
        Ok(window) => {
            #[cfg(target_os = "macos")]
            {
                // `apply_vibrancy` must run on the main thread; this function
                // can be reached from tray-menu handlers, which do not run
                // there. Bounce it if needed.
                let w = window.clone();
                if let Err(err) = window.run_on_main_thread(move || {
                    if let Err(err) = window_vibrancy::apply_vibrancy(
                        &w,
                        window_vibrancy::NSVisualEffectMaterial::Sidebar,
                        Some(window_vibrancy::NSVisualEffectState::Active),
                        None,
                    ) {
                        eprintln!("[clipstack] settings vibrancy unavailable: {err}");
                    }
                }) {
                    eprintln!("[clipstack] could not schedule vibrancy: {err}");
                }
            }
            promote(&app)
        }
        Err(err) => eprintln!("[clipstack] could not open settings: {err}"),
    }
}

/// Show the Dock icon while a real window is on screen.
pub fn promote(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        let _ = app.show();
    }
}

/// Back to a menu-bar-only app, unconditionally.
///
/// Use this right after hiding or closing the settings window yourself:
/// `hide()` is dispatched to the event loop, so an `is_visible()` check made
/// in the same callback still sees the window on screen and would skip the
/// demote, leaving the Dock icon stuck.
pub fn demote(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
}

/// Back to a menu-bar-only app if nothing needs the Dock icon.
///
/// Checks whether the settings window is *visible*, not whether it exists:
/// closing it hides rather than destroys the window (so it reopens instantly).
pub fn demote_if_idle(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let busy = app
            .get_webview_window(LABEL)
            .map(|window| window.is_visible().unwrap_or(false))
            .unwrap_or(false);
        if !busy {
            demote(app);
        }
    }
}
