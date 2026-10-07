use serde::{Deserialize, Serialize};

/// What kind of payload a stack entry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipKind {
    Text,
    Image,
    Files,
}

impl ClipKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ClipKind::Text => "text",
            ClipKind::Image => "image",
            ClipKind::Files => "files",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "text" => Some(ClipKind::Text),
            "image" => Some(ClipKind::Image),
            "files" => Some(ClipKind::Files),
            _ => None,
        }
    }
}

/// A row in the clipboard stack.
///
/// Payload columns are mutually exclusive by `kind`:
/// - `Text`  -> `text`, optionally `html` and `rtf_path`
/// - `Image` -> `blob_path` (+ `mime`, `width`, `height`)
/// - `Files` -> `file_urls`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipItem {
    pub id: i64,
    pub kind: ClipKind,
    /// Short single-line label shown in the picker list.
    pub preview: String,
    pub text: Option<String>,
    pub html: Option<String>,
    pub rtf_path: Option<String>,
    pub blob_path: Option<String>,
    pub file_urls: Vec<String>,
    pub mime: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub byte_len: i64,
    pub sha256: String,
    pub source_app: Option<String>,
    pub created_at: i64,
    pub last_used_at: i64,
    pub pinned: bool,
}

/// A stack entry as sent to the picker, with the long payload truncated.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipSummary {
    pub id: i64,
    pub kind: ClipKind,
    pub preview: String,
    pub line_count: usize,
    pub file_count: usize,
    pub byte_len: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub source_app: Option<String>,
    pub created_at: i64,
    pub last_used_at: i64,
    pub pinned: bool,
    /// True when a text entry also carries HTML or RTF, i.e. it can be
    /// re-copied either as plain text or with its formatting intact.
    pub has_formatting: bool,
    /// True when a text entry's HTML embeds a picture, which is why a copy of
    /// rich text containing an image still files as text: the picture is part
    /// of the markup, not a separate clipboard image.
    pub has_inline_image: bool,
}

/// Which flavours of a stacked entry go back on the pasteboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyFormat {
    /// The plain-string flavour only; strips colour, fonts, and inline images.
    Plain,
    /// Plain text plus any HTML and RTF captured with it.
    Rich,
}

impl CopyFormat {
    /// The wire value the webviews send; anything unrecognised is rich, which
    /// is the behaviour every copy command had before this existed.
    pub fn from_opt(value: Option<&str>) -> Self {
        match value {
            Some("plain") => CopyFormat::Plain,
            _ => CopyFormat::Rich,
        }
    }
}

/// The full payload of one entry, for the preview pane.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipDetail {
    pub id: i64,
    pub kind: ClipKind,
    pub text: Option<String>,
    pub file_urls: Vec<String>,
    /// `data:` URL for image previews, `None` when unavailable or too large.
    pub data_url: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

/// User-facing configuration, persisted as JSON in the app config dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Accelerator for the "paste from stack" popup, e.g. `Cmd+Shift+V`.
    pub picker_shortcut: String,
    /// Maximum number of entries kept in the stack.
    pub max_items: u32,
    /// Keep the stack on disk when the app quits.
    pub preserve_on_shutdown: bool,
    /// Register the app as a login item. Mirrors the real autostart state.
    pub run_at_startup: bool,
    pub capture_text: bool,
    pub capture_images: bool,
    pub capture_files: bool,
    /// Temporarily stop recording new copies.
    pub paused: bool,
    /// Never record a copy whose frontmost app is in this list (bundle id or name).
    pub excluded_apps: Vec<String>,
    /// Ask AppKit to keep window contents out of screenshots and screen sharing.
    pub content_protection: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            picker_shortcut: default_shortcut().to_string(),
            max_items: 200,
            preserve_on_shutdown: true,
            run_at_startup: false,
            capture_text: true,
            capture_images: true,
            capture_files: true,
            paused: false,
            excluded_apps: Vec::new(),
            content_protection: true,
        }
    }
}

pub fn default_shortcut() -> &'static str {
    "Cmd+Shift+V"
}

/// Headline numbers for the settings window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackStats {
    pub count: i64,
    pub storage_bytes: i64,
    pub paused: bool,
}

/// What the poller recorded on the current tick, before it hits the database.
#[derive(Debug, Clone)]
pub enum Captured {
    Text {
        text: String,
        html: Option<String>,
        rtf: Option<Vec<u8>>,
    },
    Image {
        bytes: Vec<u8>,
        mime: &'static str,
        ext: &'static str,
    },
    Files {
        paths: Vec<String>,
    },
    /// Nothing worth stacking (empty, concealed, or excluded).
    Skip,
}
