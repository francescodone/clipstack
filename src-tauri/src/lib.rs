mod commands;
mod config;
mod picker;
mod poller;
mod shortcuts;
mod store;
mod tray;
mod types;
mod windows;

#[cfg(target_os = "macos")]
mod macos;

use std::sync::{Arc, Mutex};

use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use crate::types::Settings;

/// Shared state, reachable from every command via `State<AppState>`.
pub struct AppState {
    pub store: Arc<store::Store>,
    pub settings: Arc<Mutex<Settings>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    // macOS reports both a press and a release; acting on both
                    // would open then immediately close the picker.
                    if matches!(
                        event.state,
                        tauri_plugin_global_shortcut::ShortcutState::Pressed
                    ) {
                        picker::toggle(app.clone());
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            setup(&handle)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_clips,
            commands::search_clips,
            commands::get_clip,
            commands::paste_clip,
            commands::copy_clip,
            commands::delete_clip,
            commands::pin_clip,
            commands::clear_stack,
            commands::stack_stats,
            commands::get_settings,
            commands::save_settings,
            commands::set_shortcut,
            commands::open_settings_window,
            commands::close_settings_window,
            commands::autostart_state,
            commands::set_autostart,
            commands::accessibility_state,
            commands::reset_accessibility,
            commands::request_accessibility,
            commands::open_accessibility_settings,
            commands::hide_picker,
            commands::quit,
        ])
        .build(tauri::generate_context!())
        .expect("could not start ClipStack")
        .run(|app, event| match event {
            // A menu-bar app must survive having no visible windows.
            RunEvent::ExitRequested { code, api, .. } => {
                if code != Some(0) {
                    api.prevent_exit();
                }
            }
            RunEvent::WindowEvent { label, event, .. } => match event {
                // Cmd+W on settings hides it rather than destroying it, so the
                // poller and tray keep running.
                WindowEvent::CloseRequested { api, .. } if label == windows::LABEL => {
                    api.prevent_close();
                    if let Some(window) = app.get_webview_window(&label) {
                        let _ = window.hide();
                    }
                    // Demote unconditionally: the hide above is only queued,
                    // so a visibility check here would still see the window.
                    windows::demote(app);
                }
                WindowEvent::Destroyed if label == windows::LABEL => {
                    windows::demote_if_idle(app);
                }
                // Clicking away from the picker dismisses it.
                WindowEvent::Focused(false) if label == picker::LABEL => {
                    picker::hide(app);
                }
                _ => {}
            },
            _ => {}
        });
}

fn setup(handle: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // Menu-bar-only: no Dock icon, no window at launch.
    #[cfg(target_os = "macos")]
    {
        let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }

    let store = Arc::new(store::Store::open(handle)?);
    let settings = Arc::new(Mutex::new(config::load(handle)));

    poller::set_paused(settings.lock().map(|s| s.paused).unwrap_or(false));

    handle.manage(AppState {
        store: Arc::clone(&store),
        settings: Arc::clone(&settings),
    });

    tray::build(handle)?;

    tray::set_pause_label(handle, poller::is_paused());

    let shortcut = settings
        .lock()
        .map(|s| s.picker_shortcut.clone())
        .unwrap_or_else(|_| crate::types::default_shortcut().to_string());
    if let Err(err) = shortcuts::rebind(handle, &shortcut) {
        eprintln!("[clipstack] {err}");
    }

    poller::spawn(handle.clone(), store, settings);

    // Nudge the user once on first run, since pasting does not work without it.
    #[cfg(target_os = "macos")]
    if !macos::accessibility::is_trusted() {
        macos::accessibility::prompt();
    }

    Ok(())
}
