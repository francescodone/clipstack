//! The Spotlight-style picker window.
//!
//! Created once and reused: building a fresh webview on every keystroke of the
//! shortcut would add a visible delay, so the window is hidden instead of
//! closed and simply re-pointed at the current display when shown again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg(target_os = "macos")]
use crate::macos::window as ns;

pub const LABEL: &str = "picker";

/// Whether the picker is on screen. Guards against the shortcut re-entering
/// itself while the panel is already up.
static OPEN: AtomicBool = AtomicBool::new(false);

/// Width of the panel in logical points, matching the CSS.
const WIDTH: f64 = 680.0;
const HEIGHT: f64 = 460.0;
/// How far down the screen the panel sits, as a fraction of the work area.
const TOP_FRACTION: f64 = 0.22;

pub fn is_open() -> bool {
    OPEN.load(Ordering::SeqCst)
}

/// Open the picker, or close it if it is already up.
pub fn toggle(app: AppHandle) {
    if is_open() {
        hide(&app);
    } else {
        show(&app);
    }
}

/// Show the picker over whichever display the cursor is on.
pub fn show(app: &AppHandle) {
    // Remember who we are about to paste into, before we steal focus.
    #[cfg(target_os = "macos")]
    crate::macos::frontmost::capture_previous();

    let Some(window) = ensure_window(app) else {
        return;
    };

    position_on_cursor_monitor(app, &window);

    #[cfg(target_os = "macos")]
    if let Err(err) = ns::raise(&window) {
        eprintln!("[clipstack] could not raise the picker: {err}");
    }

    let _ = window.show();

    #[cfg(target_os = "macos")]
    // Must come after show(): activates the accessory app and makes the panel
    // key, without which the webview never receives any keystroke (Enter in
    // particular). `set_focus` alone is not enough for an LSUIElement app.
    if let Err(err) = ns::focus_panel(&window) {
        eprintln!("[clipstack] could not focus the picker: {err}");
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window.set_focus();

    OPEN.store(true, Ordering::SeqCst);

    // Tell the renderer to reset the query and re-fetch, because the window was
    // only hidden and still holds the previous search.
    let _ = app.emit_to(LABEL, "picker://open", ());

    if let Some(state) = app.try_state::<crate::AppState>() {
        if state.settings.lock().map(|s| s.content_protection).unwrap_or(true) {
            let _ = window.set_content_protected(true);
        }
    }
}

pub fn hide(app: &AppHandle) {
    OPEN.store(false, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
        #[cfg(target_os = "macos")]
        {
            if let Err(err) = ns::lower(&window) {
                eprintln!("[clipstack] could not lower the picker: {err}");
            }
            // We activated the app to show the panel; step aside so focus
            // returns to the app the user was actually working in.
            if let Err(err) = ns::release_focus(&window) {
                eprintln!("[clipstack] could not release focus: {err}");
            }
        }
    }
}

/// Called by the renderer once the user has picked an entry.
///
/// Hides the panel, restores the target app, writes the payload, and injects
/// Cmd+V. Runs on a worker thread because it deliberately sleeps while waiting
/// for AppKit to move apps around.
pub fn paste_and_close(app: AppHandle, id: i64) {
    hide(&app);
    let store = match app.try_state::<crate::AppState>() {
        Some(state) => Arc::clone(&state.store),
        None => return,
    };

    std::thread::spawn(move || {
        if let Err(err) = do_paste(&app, &store, id) {
            eprintln!("[clipstack] paste failed: {err}");
            let _ = app.emit("stack://paste-failed", err);
        }
    });
}

fn do_paste(app: &AppHandle, store: &crate::store::Store, id: i64) -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, store, id);
        return Err("ClipStack only supports pasting on macOS".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        // Without Accessibility we cannot synthesise a keystroke, but the
        // payload still goes on the pasteboard so a manual Cmd+V works.
        let needs_permission = !crate::macos::accessibility::is_trusted();

        // Capture the target before the pasteboard write, so a slow app cannot
        // shuffle the stack order out from under us.
        let target = crate::macos::frontmost::paste_target();

        let item = store
            .get(id)?
            .ok_or_else(|| "that item is no longer in the stack".to_string())?;

        crate::poller::arm_own_write();
        crate::macos::pasteboard::write_item(&item)?;
        store.mark_used(id)?;

        if needs_permission {
            return Err(
                "ClipStack needs Accessibility access to paste for you — the item is on your \
                 clipboard, press Cmd+V"
                    .to_string(),
            );
        }

        // Give the pasteboard a moment to settle before the keystroke arrives.
        std::thread::sleep(Duration::from_millis(20));

        // Bring the original app back to the front. If it refuses, posting the
        // event straight to its pid still works in most cases.
        if let Some(pid) = target {
            let _ = crate::macos::frontmost::activate_and_wait(pid, Duration::from_millis(400));
        }

        crate::macos::paste::paste_keystroke(target)?;

        // The picker was only hidden, so the shortcut was never unregistered.
        let _ = app.emit_to(LABEL, "picker://reset", ());
        let _ = app.emit("stack://changed", id);
        Ok(())
    }
}

/// Build the picker once, or return the existing window.
fn ensure_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Some(window);
    }

    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("ClipStack")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .shadow(true)
        .visible(false)
        .focused(false)
        .center();

    match builder.build() {
        Ok(window) => {
            #[cfg(target_os = "macos")]
            if let Err(err) = ns::configure_panel(&window) {
                eprintln!("[clipstack] could not configure the picker: {err}");
            }
            Some(window)
        }
        Err(err) => {
            eprintln!("[clipstack] could not create the picker window: {err}");
            None
        }
    }
}

/// Centre the panel horizontally on the display holding the cursor, near the
/// top — the position Spotlight and Raycast use.
fn position_on_cursor_monitor(app: &AppHandle, window: &tauri::WebviewWindow) {
    let cursor = match app.cursor_position() {
        Ok(pos) => pos,
        Err(_) => {
            let _ = window.center();
            return;
        }
    };

    let monitors = app.available_monitors().unwrap_or_default();
    // Bind the owned values first: `primary_monitor()` produces a temporary
    // that cannot be borrowed through the chain below.
    let primary = app.primary_monitor().ok().flatten();
    let cursor_monitor = monitors.iter().find(|m| contains(m, cursor)).cloned();
    let monitor = match cursor_monitor
        .or(primary)
        .or_else(|| monitors.first().cloned())
    {
        Some(monitor) => monitor,
        None => {
            let _ = window.center();
            return;
        }
    };

    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let x = work.position.x as f64 + (work.size.width as f64 / scale - WIDTH) / 2.0;
    let y = work.position.y as f64 + (work.size.height as f64 / scale) * TOP_FRACTION;

    let _ = window.set_position(LogicalPosition::new(x, y));
}

fn contains(monitor: &tauri::Monitor, cursor: tauri::PhysicalPosition<f64>) -> bool {
    let pos = monitor.position();
    let size = monitor.size();
    cursor.x >= pos.x as f64
        && cursor.x < pos.x as f64 + size.width as f64
        && cursor.y >= pos.y as f64
        && cursor.y < pos.y as f64 + size.height as f64
}
