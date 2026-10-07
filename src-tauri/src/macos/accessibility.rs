//! The Accessibility permission, without which nothing can be pasted.

use objc2::runtime::AnyObject;
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

/// `kAXTrustedCheckOptionPrompt`.
const OPTION_PROMPT: &str = "AXTrustedCheckOptionPrompt";

/// Deep link to the Accessibility pane. The modern URL shape, with the legacy
/// one kept as a fallback for older macOS releases.
const SETTINGS_URL_MODERN: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility";
const SETTINGS_URL_LEGACY: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const AnyObject) -> bool;
}

/// Whether this process is trusted for Accessibility.
///
/// This is the canonical check. Probing with a live AX call is unreliable: it
/// can fail with `kAXErrorCannotComplete` on an app that is in fact trusted.
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Show the system prompt that offers to open System Settings.
///
/// macOS only offers this once per app version, so the settings window also
/// exposes a manual button.
pub fn prompt() {
    unsafe {
        let key = NSString::from_str(OPTION_PROMPT);
        let value = NSNumber::numberWithBool(true);
        let options = NSDictionary::from_slices(&[&*key], &[&*value]);
        let ptr = &*options as *const NSDictionary<NSString, NSNumber> as *const AnyObject;
        AXIsProcessTrustedWithOptions(ptr);
    }
}

/// Open the Accessibility settings pane.
pub fn open_settings() {
    let _ = open_url(SETTINGS_URL_MODERN).or_else(|_| open_url(SETTINGS_URL_LEGACY));
}

fn open_url(url: &str) -> Result<(), String> {
    let ns = NSString::from_str(url);
    let ns_url = NSURL::URLWithString(&ns).ok_or_else(|| format!("not a URL: {url}"))?;
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    let ok = workspace.openURL(&ns_url);
    if ok {
        Ok(())
    } else {
        Err(format!("could not open {url}"))
    }
}
