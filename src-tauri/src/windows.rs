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
        .resizable(true)
        .maximizable(false)
        .closable(true)
        .focused(true)
        .visible(true);

    match builder.build() {
        Ok(_) => promote(&app),
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

/// Back to a menu-bar-only app.
///
/// Checks that a window *exists*, not that it is visible: a hidden-but-alive
/// settings window would otherwise leave the Dock icon stuck on.
pub fn demote_if_idle(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let busy = app.get_webview_window(LABEL).is_some();
        if !busy {
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        }
    }
}
