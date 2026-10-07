import { invoke } from "@tauri-apps/api/core";
import type { ClipDetail, ClipSummary, Settings, StackStats } from "./types";

// Thin, typed wrappers over the Rust commands. Every call site goes through
// here so the command names live in exactly one place.

export const api = {
  listClips: (limit?: number) => invoke<ClipSummary[]>("list_clips", { limit }),
  searchClips: (query: string, limit?: number) =>
    invoke<ClipSummary[]>("search_clips", { query, limit }),
  getClip: (id: number) => invoke<ClipDetail | null>("get_clip", { id }),
  pasteClip: (id: number) => invoke<void>("paste_clip", { id }),
  copyClip: (id: number) => invoke<void>("copy_clip", { id }),
  deleteClip: (id: number) => invoke<void>("delete_clip", { id }),
  pinClip: (id: number, pinned: boolean) => invoke<void>("pin_clip", { id, pinned }),
  clearStack: () => invoke<void>("clear_stack"),
  stackStats: () => invoke<StackStats>("stack_stats"),

  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  setShortcut: (accelerator: string) =>
    invoke<Settings>("set_shortcut", { accelerator }),

  openSettingsWindow: () => invoke<void>("open_settings_window"),
  closeSettingsWindow: () => invoke<void>("close_settings_window"),

  autostartState: () => invoke<boolean>("autostart_state"),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),

  accessibilityState: () => invoke<boolean>("accessibility_state"),
  requestAccessibility: () => invoke<void>("request_accessibility"),
  openAccessibilitySettings: () => invoke<void>("open_accessibility_settings"),
  resetAccessibility: () => invoke<void>("reset_accessibility"),

  hidePicker: () => invoke<void>("hide_picker"),
  quit: () => invoke<void>("quit"),
};

/** `1.4 MB`, `12 KB`, … for the settings and footer readouts. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** Relative time, the way macOS lists files: "now", "4m", "3h", "2d". */
export function formatAgo(epochSeconds: number): string {
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - epochSeconds);
  if (seconds < 45) return "now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  const weeks = Math.floor(days / 7);
  if (weeks < 5) return `${weeks}w`;
  return new Date(epochSeconds * 1000).toLocaleDateString();
}

/**
 * `Cmd+Shift+V` -> `⌘⇧V`, for showing the binding inside the UI.
 * Accepts the accelerator grammar the Rust side parses.
 */
export function formatShortcut(accelerator: string): string {
  const glyphs: Record<string, string> = {
    cmd: "⌘",
    command: "⌘",
    super: "⌘",
    meta: "⌘",
    cmdorcontrol: "⌘/⌃",
    cmdorctrl: "⌘/⌃",
    ctrl: "⌃",
    control: "⌃",
    option: "⌥",
    alt: "⌥",
    shift: "⇧",
    shiftorplus: "+",
  };
  const keyGlyphs: Record<string, string> = {
    up: "↑",
    down: "↓",
    left: "←",
    right: "→",
    enter: "↵",
    return: "↵",
    space: "Space",
    tab: "⇥",
    esc: "Esc",
    escape: "Esc",
    backspace: "⌫",
    delete: "⌫",
    plus: "+",
  };

  return accelerator
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const lower = part.toLowerCase();
      if (glyphs[lower]) return glyphs[lower];
      if (keyGlyphs[lower]) return keyGlyphs[lower];
      return part.length === 1 ? part.toUpperCase() : part;
    })
    .join("");
}

/**
 * Translate a keydown into the accelerator string the Rust side expects, or
 * `null` when the combination is not registrable (a bare modifier, or a key
 * with no command modifier).
 */
export function keyEventToAccelerator(event: KeyboardEvent): string | null {
  // A shortcut must be owned by ⌘, otherwise it would swallow ordinary typing.
  if (!event.metaKey) return null;

  const key = event.key;
  if (
    key === "Meta" ||
    key === "Control" ||
    key === "Alt" ||
    key === "Shift" ||
    key === "CapsLock"
  ) {
    return null;
  }

  const parts: string[] = ["Cmd"];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");

  let name: string | null;
  if (key.length === 1) {
    name = key.toUpperCase();
  } else {
    name = namedKey(key);
  }
  if (!name) return null;

  parts.push(name);
  return parts.join("+");
}

/** Keys we are willing to bind that are not single characters. */
function namedKey(key: string): string | null {
  const allowed: Record<string, string> = {
    Up: "Up",
    Down: "Down",
    Left: "Left",
    Right: "Right",
    Enter: "Return",
    Tab: "Tab",
    Space: "Space",
    Escape: "Esc",
    Backspace: "Backspace",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    F1: "F1",
    F2: "F2",
    F3: "F3",
    F4: "F4",
    F5: "F5",
    F6: "F6",
    F7: "F7",
    F8: "F8",
    F9: "F9",
    F10: "F10",
    F11: "F11",
    F12: "F12",
  };
  return allowed[key] ?? null;
}
