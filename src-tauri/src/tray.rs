//! The menu bar icon and its menu.

use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

pub const ID_PICKER: &str = "menu-picker";
pub const ID_SETTINGS: &str = "menu-settings";
pub const ID_PAUSE: &str = "menu-pause";
pub const ID_CLEAR: &str = "menu-clear";

pub const TRAY_ID: &str = "clipstack-tray";

const LABEL_PAUSE: &str = "Pause Capture";
const LABEL_RESUME: &str = "Resume Capture";

/// A 16pt alpha-only PNG rendered as a template image, so the icon follows the
/// menu bar appearance in light and dark mode.
const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");

/// Held so the label can be flipped later. `TrayIcon` has no getter for its
/// menu, so the item we need to mutate is kept here rather than looked up.
static PAUSE_ITEM: Mutex<Option<MenuItem<tauri::Wry>>> = Mutex::new(None);

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let pause = MenuItem::with_id(app, ID_PAUSE, LABEL_PAUSE, true, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .text(ID_PICKER, "Show Stack")
        .separator()
        .item(&pause)
        .text(ID_SETTINGS, "Settings…")
        .separator()
        .text(ID_CLEAR, "Clear Stack")
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some("Quit ClipStack"))?)
        .build()?;

    if let Ok(mut guard) = PAUSE_ITEM.lock() {
        *guard = Some(pause);
    }

    let icon = Image::from_bytes(TRAY_PNG).unwrap_or_else(|_| placeholder_icon());

    TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        // Left click opens the menu; the shortcut is the fast path.
        .show_menu_on_left_click(true)
        .icon_as_template(true)
        .icon(icon)
        .tooltip("ClipStack")
        .on_menu_event(move |app, event| match event.id.as_ref() {
            ID_PICKER => crate::picker::toggle(app.clone()),
            ID_SETTINGS => crate::windows::open_settings(app.clone()),
            ID_PAUSE => toggle_pause(app.clone()),
            ID_CLEAR => clear_stack(app.clone()),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// Reflect the paused state in the menu label.
pub fn set_pause_label(_app: &AppHandle, paused: bool) {
    if let Ok(guard) = PAUSE_ITEM.lock() {
        if let Some(item) = guard.as_ref() {
            let _ = item.set_text(if paused { LABEL_RESUME } else { LABEL_PAUSE });
        }
    }
}

fn toggle_pause(app: AppHandle) {
    let next = !crate::poller::is_paused();
    crate::poller::set_paused(next);
    if let Some(state) = app.try_state::<crate::AppState>() {
        let snapshot = {
            if let Ok(mut guard) = state.settings.lock() {
                guard.paused = next;
            }
            state
                .settings
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default()
        };
        if let Err(err) = crate::config::save(&app, &snapshot) {
            eprintln!("[clipstack] could not save settings: {err}");
        }
    }
    set_pause_label(&app, next);
    let _ = app.emit("stack://paused", next);
}

fn clear_stack(app: AppHandle) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        if let Err(err) = state.store.clear() {
            eprintln!("[clipstack] could not clear the stack: {err}");
        }
    }
    let _ = app.emit("stack://changed", 0i64);
}

/// Used only if the bundled tray PNG fails to decode, so the app still starts.
fn placeholder_icon() -> Image<'static> {
    let size = 18u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let edge = x < 2 || y < 2 || x >= size - 2 || y >= size - 2;
            let i = ((y * size + x) as usize) * 4;
            if edge {
                rgba[i] = 0;
                rgba[i + 1] = 0;
                rgba[i + 2] = 0;
                rgba[i + 3] = 200;
            }
        }
    }
    Image::new_owned(rgba, size, size)
}

