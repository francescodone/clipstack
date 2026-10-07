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
/// `kCGEventFlagMaskCommand`.
const FLAG_COMMAND: u64 = 0x100_0000;
/// `kCGHIDEventTap` / `kCGSessionEventTap`.
const TAP_SESSION: u32 = 1;

/// Gap between key-down and key-up. Zero works on most apps but not all.
const KEY_HOLD: Duration = Duration::from_millis(12);

/// Synthesize Cmd+V.
///
/// `pid` is the intended recipient. When it is already frontmost the event goes
/// through the session tap (so it behaves like real hardware input, which
/// Electron apps require); otherwise it is delivered straight to the pid.
/// Posting both ways pastes twice.
pub fn paste_keystroke(pid: Option<i32>) -> Result<(), String> {
    let already_frontmost = match pid {
        Some(pid) => super::frontmost::current().map(|t| t.pid) == Some(pid),
        None => false,
    };

    unsafe {
        let source = CGEventSourceCreate(SOURCE_HID);
        let down = CGEventCreateKeyboardEvent(source, K_V, 1);
        let up = CGEventCreateKeyboardEvent(source, K_V, 0);
        if down.is_null() || up.is_null() {
            return Err("could not create a keyboard event".to_string());
        }
        CGEventSetFlags(down, FLAG_COMMAND);
        CGEventSetFlags(up, FLAG_COMMAND);

        match pid.filter(|_| !already_frontmost) {
            Some(pid) => {
                CGEventPostToPid(pid, down);
                CGEventPostToPid(pid, up);
            }
            None => {
                CGEventPost(TAP_SESSION, down);
                std::thread::sleep(KEY_HOLD);
                CGEventPost(TAP_SESSION, up);
            }
        }

        CFRelease(down as *const c_void);
        CFRelease(up as *const c_void);
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
