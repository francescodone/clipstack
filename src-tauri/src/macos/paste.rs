//! Injecting a Cmd+V into whichever app the user was in.
//!
//! The order below matters and is the whole trick:
//!   1. hide our panel
//!   2. write the payload to the pasteboard
//!   3. reactivate the target app and wait for AppKit to confirm it is frontmost
//!   4. post a synthetic Cmd+V
//!
//! `AXUIElementPerformAction(_, "AXPaste")` is deliberately not used: it
//! reports success without pasting in Chromium-based apps (Chrome, Arc, Edge,
//! VS Code, Slack), which is a worse failure than not trying at all.

use std::ffi::c_void;
use std::time::Duration;

use core_graphics_ffi::*;

/// Virtual keycode for `v` on an ANSI layout.
const K_V: u16 = 9;
/// Virtual keycode for the left `command` key.
const K_COMMAND: u16 = 55;
/// `kCGEventFlagMaskCommand`.
const FLAG_COMMAND: u64 = 0x100_0000;
/// `kCGHIDEventTap` / `kCGSessionEventTap`.
const TAP_SESSION: u32 = 1;

/// Gap between key-down and key-up. Zero works on most apps but not all.
const KEY_HOLD: Duration = Duration::from_millis(12);
/// Gap around the synthetic Command presses, so the modifier is settled in
/// the HID state before `v` arrives and still held when it leaves.
const MODIFIER_HOLD: Duration = Duration::from_millis(10);

/// Synthesize Cmd+V.
///
/// The keystroke is the full hardware sequence — Command down, V down,
/// V up, Command up — not a bare V with modifier flags stamped onto it.
/// Apps that consult the live modifier state instead of the event's own
/// flags (Electron-family text views, and anything fed through
/// `CGEventPostToPid`, which is known to drop them) read the stamped-only
/// form as a plain `v` and type the letter. That is the "it pasted a v"
/// bug, and pressing the modifier as its own key event is what Clipy and
/// Maccy do to avoid it.
///
/// `pid` is the intended recipient. When it is already frontmost the event
/// goes through the session tap (so it behaves like real hardware input,
/// which Electron apps require); otherwise it is delivered straight to the
/// pid. Posting both ways pastes twice.
pub fn paste_keystroke(pid: Option<i32>) -> Result<(), String> {
    let already_frontmost = match pid {
        Some(pid) => super::frontmost::current().map(|t| t.pid) == Some(pid),
        None => false,
    };
    let direct_pid = pid.filter(|_| !already_frontmost);

    unsafe {
        let source = CGEventSourceCreate(SOURCE_HID);

        // (keycode, is-down, flags) in hardware order, with the Command
        // presses bracketing the V pair.
        let sequence = [
            (K_COMMAND, true, FLAG_COMMAND),
            (K_V, true, FLAG_COMMAND),
            (K_V, false, FLAG_COMMAND),
            (K_COMMAND, false, 0),
        ];

        for (step, &(virtual_key, key_down, flags)) in sequence.iter().enumerate() {
            let event = CGEventCreateKeyboardEvent(source, virtual_key, i32::from(key_down));
            if event.is_null() {
                if !source.is_null() {
                    CFRelease(source as *const c_void);
                }
                return Err("could not create a keyboard event".to_string());
            }
            CGEventSetFlags(event, flags);
            match direct_pid {
                Some(pid) => CGEventPostToPid(pid, event),
                None => CGEventPost(TAP_SESSION, event),
            }
            CFRelease(event as *const c_void);

            // Beat after the modifier lands, the V hold, and a beat after
            // V is released before the modifier lifts.
            match step {
                0 | 2 => std::thread::sleep(MODIFIER_HOLD),
                1 => std::thread::sleep(KEY_HOLD),
                _ => {}
            }
        }

        if !source.is_null() {
            CFRelease(source as *const c_void);
        }
    }
    Ok(())
}

/// Raw CoreGraphics bindings. `objc2-core-graphics` covers this too, but the
/// surface needed here is four functions and a release call.
mod core_graphics_ffi {
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        /// `kCGHIDEventSource` — events look like they came from hardware.
        pub fn CGEventSourceCreate(state_id: u32) -> *mut c_void;
        pub fn CGEventCreateKeyboardEvent(
            source: *mut c_void,
            virtual_key: u16,
            key_down: i32,
        ) -> *mut c_void;
        pub fn CGEventSetFlags(event: *mut c_void, flags: u64);
        pub fn CGEventPost(tap: u32, event: *mut c_void);
        pub fn CGEventPostToPid(pid: i32, event: *mut c_void);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFRelease(cf: *const c_void);
    }

    pub const SOURCE_HID: u32 = 1;
}
