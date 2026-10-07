//! Watches the pasteboard for writes by other processes.
//!
//! We never intercept Cmd+C. Plain copy and paste stay entirely native: this
//! module only polls `NSPasteboard.changeCount` and records what appeared
//! afterwards. That is the same approach Maccy and Clipy take, and it is the
//! only one that does not steal the shortcut from every other app.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use tauri::{AppHandle, Emitter};

use crate::config;
use crate::macos::{frontmost, pasteboard};
use crate::store::Store;
use crate::types::{Captured, Settings};

/// How often to re-read the change counter. Clipy uses 500 ms; 250 ms still
/// costs effectively nothing and makes the stack feel immediate.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Set to the change count we are about to cause ourselves, so the poller can
/// tell our own paste writes apart from a real user copy.
const OWN_WRITE: i64 = -1;

static LAST_SEEN: AtomicI64 = AtomicI64::new(i64::MIN);
static IGNORE_UNTIL: AtomicI64 = AtomicI64::new(0);
static PAUSED: AtomicBool = AtomicBool::new(false);

pub fn set_paused(paused: bool) {
    PAUSED.store(paused, Ordering::SeqCst);
}

pub fn is_paused() -> bool {
    PAUSED.load(Ordering::SeqCst)
}

/// Call immediately before writing the pasteboard ourselves. The next observed
/// change is then attributed to us and skipped.
pub fn arm_own_write() {
    let next = pasteboard::change_count() + 4;
    IGNORE_UNTIL.store(next.max(OWN_WRITE + 1), Ordering::SeqCst);
}

/// Spawn the watcher. Returns as soon as the thread is running.
pub fn spawn(app: AppHandle, store: Arc<Store>, settings: Arc<Mutex<Settings>>) {
    // Seed the counter so the item already on the pasteboard at launch is not
    // treated as a fresh copy.
    LAST_SEEN.store(pasteboard::change_count(), Ordering::SeqCst);

    thread::Builder::new()
        .name("clipstack-poller".into())
        .spawn(move || {
            // A dedicated thread gets its own run loop turn, which AppKit
            // expects for pasteboard work.
            loop {
                poll_once(&app, &store, &settings);
                thread::sleep(POLL_INTERVAL);
            }
        })
        .expect("could not start the clipboard watcher");
}

fn poll_once(app: &AppHandle, store: &Arc<Store>, settings: &Arc<Mutex<Settings>>) {
    // Remember which app we were in before it changed. This is how the picker
    // knows where to paste, and it is cheaper and far less fragile than an
    // NSWorkspace notification observer on a thread with no run loop.
    frontmost::note_frontmost();

    if is_paused() {
        return;
    }

    let count = pasteboard::change_count();
    let last = LAST_SEEN.load(Ordering::SeqCst);
    if count == last {
        return;
    }
    LAST_SEEN.store(count, Ordering::SeqCst);

    if count <= IGNORE_UNTIL.load(Ordering::SeqCst) {
        return;
    }

    let snapshot = settings
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();

    if snapshot.excluded_apps.iter().any(|needle| {
        frontmost::current_bundle_id()
            .map(|id| id.eq_ignore_ascii_case(needle))
            .unwrap_or(false)
            || frontmost::current_name()
                .map(|n| n.eq_ignore_ascii_case(needle))
                .unwrap_or(false)
    }) {
        return;
    }

    let captured = pasteboard::read_current();
    let allowed = match &captured {
        Captured::Text { .. } => snapshot.capture_text,
        Captured::Image { .. } => snapshot.capture_images,
        Captured::Files { .. } => snapshot.capture_files,
        Captured::Skip => false,
    };
    if !allowed {
        return;
    }

    let source = frontmost::current_name();
    match store.record(captured, source.as_deref(), snapshot.max_items) {
        Ok(Some(id)) => {
            let _ = app.emit("stack://changed", id);
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!("[clipstack] could not stack an item: {err}");
        }
    }
}

/// Re-read settings from disk, used after the settings window edits them.
pub fn reload_settings(app: &AppHandle, settings: &Arc<Mutex<Settings>>) {
    let loaded = config::load(app);
    set_paused(loaded.paused);
    if let Ok(mut guard) = settings.lock() {
        *guard = loaded;
    }
}
