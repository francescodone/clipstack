// Mirrors of the Rust serde types in `src-tauri/src/types.rs`.
// Renamed to camelCase by `#[serde(rename_all = "camelCase")]`.

export type ClipKind = "text" | "image" | "files";

export interface ClipSummary {
  id: number;
  kind: ClipKind;
  preview: string;
  lineCount: number;
  fileCount: number;
  byteLen: number;
  width: number | null;
  height: number | null;
  sourceApp: string | null;
  createdAt: number;
  lastUsedAt: number;
  pinned: boolean;
}

export interface ClipDetail {
  id: number;
  kind: ClipKind;
  text: string | null;
  fileUrls: string[];
  dataUrl: string | null;
  width: number | null;
  height: number | null;
}

export interface Settings {
  pickerShortcut: string;
  maxItems: number;
  preserveOnShutdown: boolean;
  runAtStartup: boolean;
  captureText: boolean;
  captureImages: boolean;
  captureFiles: boolean;
  paused: boolean;
  excludedApps: string[];
  contentProtection: boolean;
}

export interface StackStats {
  count: number;
  storageBytes: number;
  paused: boolean;
}
