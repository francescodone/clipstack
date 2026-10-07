//! The invoke surface exposed to the two webviews.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::picker;
use crate::store::Store;
use crate::types::{ClipDetail, Settings, StackStats};
use crate::AppState;

/// Owned handles to the shared state.
///
/// Returns owned values rather than a borrowed `State<'_, AppState>` guard,
/// which cannot outlive the helper that produced it.
fn handles(app: &AppHandle) -> Result<(Arc<Store>, Settings), String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "ClipStack is still starting up".to_string())?;
    let settings = state
        .settings
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "settings are unavailable".to_string())?;
    Ok((Arc::clone(&state.store), settings))
}

// ---------------------------------------------------------------- the stack

#[tauri::command]
pub fn list_clips(
    app: AppHandle,
    limit: Option<i64>,
) -> Result<Vec<crate::types::ClipSummary>, String> {
    let (store, _) = handles(&app)?;
    store.list(limit.unwrap_or(200).clamp(1, 1000))
}

#[tauri::command]
pub fn search_clips(
    app: AppHandle,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<crate::types::ClipSummary>, String> {
    let (store, _) = handles(&app)?;
    store.search(&query, limit.unwrap_or(50).clamp(1, 200))
}

#[tauri::command]
pub fn get_clip(app: AppHandle, id: i64) -> Result<Option<ClipDetail>, String> {
    let (store, _) = handles(&app)?;
    store.detail(id)
}

/// Paste a stacked entry into the app the user was in, and close the picker.
#[tauri::command]
pub fn paste_clip(app: AppHandle, id: i64) -> Result<(), String> {
    picker::paste_and_close(app, id);
    Ok(())
}

#[tauri::command]
pub fn delete_clip(app: AppHandle, id: i64) -> Result<(), String> {
    let (store, _) = handles(&app)?;
    store.remove(id)?;
    let _ = app.emit("stack://changed", id);
    Ok(())
}

#[tauri::command]
pub fn pin_clip(app: AppHandle, id: i64, pinned: bool) -> Result<(), String> {
    let (store, _) = handles(&app)?;
    store.set_pinned(id, pinned)?;
    let _ = app.emit("stack://changed", id);
    Ok(())
}

#[tauri::command]
pub fn clear_stack(app: AppHandle) -> Result<(), String> {
    let (store, _) = handles(&app)?;
    store.clear()?;
    let _ = app.emit("stack://changed", 0i64);
    Ok(())
}

#[tauri::command]
pub fn stack_stats(app: AppHandle) -> Result<StackStats, String> {
    let (store, _) = handles(&app)?;
    Ok(StackStats {
        count: store.count()?,
        storage_bytes: store.storage_bytes()?,
        paused: crate::poller::is_paused(),
    })
}

// --------------------------------------------------------------- settings

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    let (_, settings) = handles(&app)?;
    Ok(settings)
}

/// Persist settings and re-apply everything that has a runtime effect.
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    let (store, previous) = handles(&app)?;

    crate::poller::set_paused(settings.paused);
    crate::tray::set_pause_label(&app, settings.paused);

    if let Some(window) = app.get_webview_window(picker::LABEL) {
        let _ = window.set_content_protected(settings.content_protection);
    }

    if settings.picker_shortcut != previous.picker_shortcut {
        crate::shortcuts::rebind(&app, &settings.picker_shortcut)?;
    }

    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut guard) = state.settings.lock() {
            *guard = settings.clone();
        }
    }
    crate::config::save(&app, &settings)?;

    // Trim immediately in case the cap was lowered.
    store.trim(settings.max_items)?;
    let _ = app.emit("stack://changed", 0i64);

    Ok(settings)
}

/// Rebind the picker shortcut on its own, so the recorder can surface a
/// precise error and keep the old binding if the combination is taken.
#[tauri::command]
pub fn set_shortcut(app: AppHandle, accelerator: String) -> Result<Settings, String> {
    crate::shortcuts::rebind(&app, &accelerator)?;
    let (_, mut next) = handles(&app)?;
    next.picker_shortcut = accelerator.clone();

    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut guard) = state.settings.lock() {
            *guard = next.clone();
        }
    }
    crate::config::save(&app, &next)?;
    Ok(next)
}

// ------------------------------------------------------- system integration

#[tauri::command]
pub fn open_settings_window(app: AppHandle) {
    crate::windows::open_settings(app);
}

#[tauri::command]
pub fn close_settings_window(app: AppHandle) {
    if let Some(window) = app.get_webview_window(crate::windows::LABEL) {
        let _ = window.close();
    }
    crate::windows::demote_if_idle(&app);
}

#[tauri::command]
pub fn autostart_state(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("could not read the login-item state: {e}"))
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let result = if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    };
    result.map_err(|e| format!("could not change the login item: {e}"))?;

    // Mirror the real state rather than the requested one.
    let actual = app.autolaunch().is_enabled().unwrap_or(enabled);
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut guard) = state.settings.lock() {
            guard.run_at_startup = actual;
        }
        let snapshot = state
            .settings
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let _ = crate::config::save(&app, &snapshot);
    }
    Ok(actual)
}

#[tauri::command]
pub fn accessibility_state() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::macos::accessibility::is_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

#[tauri::command]
pub fn request_accessibility() {
    #[cfg(target_os = "macos")]
    crate::macos::accessibility::prompt();
}

#[tauri::command]
pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    crate::macos::accessibility::open_settings();
}

/// Called by the picker when the user presses Escape or clicks away.
#[tauri::command]
pub fn hide_picker(app: AppHandle) {
    picker::hide(&app);
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    // Exit code 0 signals a deliberate quit, which the run-event handler uses
    // to decide not to prevent exit.
    if let Some(state) = app.try_state::<AppState>() {
        let preserve = state
            .settings
            .lock()
            .map(|s| s.preserve_on_shutdown)
            .unwrap_or(true);
        if !preserve {
            let _ = state.store.clear();
        }
    }
    app.exit(0);
}
