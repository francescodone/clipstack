use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use image::GenericImageView;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::config;
use crate::types::{Captured, ClipDetail, ClipItem, ClipKind, ClipSummary};

/// Longest payload we are willing to keep. Anything bigger is dropped rather
/// than bloating the stack and the disk.
const MAX_PAYLOAD_BYTES: i64 = 24 * 1024 * 1024;
/// Images above this size are not inlined as a `data:` URL for the preview pane.
const MAX_PREVIEW_IMAGE_BYTES: usize = 4 * 1024 * 1024;
/// How many rows a search may score at once.
const SEARCH_SCAN_LIMIT: i64 = 500;

pub struct Store {
    conn: Mutex<Connection>,
    data_dir: PathBuf,
}

impl Store {
    pub fn open(app: &AppHandle) -> Result<Store, String> {
        let dir = config::data_dir(app)?;
        let path = dir.join("clipstack.db");
        let conn = Connection::open(&path)
            .map_err(|e| format!("could not open {}: {e}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| format!("could not enable WAL: {e}"))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| format!("could not set synchronous: {e}"))?;
        migrate(&conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
            data_dir: dir,
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.conn
            .lock()
            .map_err(|_| "the clipboard store is unavailable".to_string())
    }

    /// Record a freshly captured copy. Returns the row id that now represents it.
    ///
    /// Re-copying identical content does not create a duplicate row: the
    /// existing row is promoted to the top of the stack instead.
    pub fn record(
        &self,
        captured: Captured,
        source_app: Option<&str>,
        max_items: u32,
    ) -> Result<Option<i64>, String> {
        let now = now_ms();
        let conn = self.lock()?;

        let (kind, preview, text, html, rtf_bytes, blob_bytes, mime, file_urls, byte_len) =
            match captured {
                Captured::Skip => return Ok(None),
                Captured::Text { text, html, rtf } => {
                    let preview = one_line(&text, 240);
                    let byte_len = text.len() as i64;
                    (
                        ClipKind::Text,
                        preview,
                        Some(text),
                        html,
                        rtf,
                        None::<Vec<u8>>,
                        None,
                        Vec::new(),
                        byte_len,
                    )
                }
                Captured::Image { bytes, mime, ext: _ } => {
                    byte_len_check(bytes.len() as i64)?;
                    let (w, h) = image_dims(&bytes).unwrap_or((None, None));
                    let preview = match (w, h) {
                        (Some(w), Some(h)) => format!("{w} x {h}"),
                        _ => "Image".to_string(),
                    };
                    let byte_len = bytes.len() as i64;
                    (
                        ClipKind::Image,
                        preview,
                        None,
                        None,
                        None,
                        Some(bytes),
                        Some(mime),
                        Vec::new(),
                        byte_len,
                    )
                }                Captured::Files { paths } => {
                    if paths.is_empty() {
                        return Ok(None);
                    }
                    let preview = match paths.len() {
                        1 => Path::new(&paths[0])
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| paths[0].clone()),
                        n => format!("{n} files"),
                    };
                    let byte_len: i64 = paths
                        .iter()
                        .map(|p| fs::metadata(p).map(|m| m.len() as i64).unwrap_or(0))
                        .sum();
                    (
                        ClipKind::Files,
                        preview,
                        None,
                        None,
                        None,
                        None,
                        None,
                        paths,
                        byte_len,
                    )
                }
            };

        let file_json = serde_json::to_string(&file_urls).unwrap_or_else(|_| "[]".to_string());
        let sha = content_hash(kind, &text, &file_urls, blob_bytes.as_deref());

        // Same content already stacked -> promote instead of duplicating.
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM clip_items WHERE sha256 = ?1 ORDER BY last_used_at DESC LIMIT 1",
                params![sha],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            conn.execute(
                "UPDATE clip_items SET last_used_at = ?1 WHERE id = ?2",
                params![now, id],
            )
            .map_err(|e| format!("could not promote a stacked item: {e}"))?;
            return Ok(Some(id));
        }

        let mut rtf_path = None;
        if let Some(bytes) = &rtf_bytes {
            rtf_path = Some(self.write_blob(&sha, "rtf", bytes)?);
        }
        let mut blob_path = None;
        let mut img_wh = (None, None);
        if let Some(bytes) = &blob_bytes {
            img_wh = image_dims(bytes).unwrap_or((None, None));
            let ext = ext_for_mime(mime.unwrap_or("application/octet-stream"));
            blob_path = Some(self.write_blob(&sha, ext, bytes)?);
        }

        conn.execute(
            "INSERT INTO clip_items (
                kind, preview, text, html, rtf_path, blob_path, file_urls, mime,
                width, height, byte_len, sha256, source_app, created_at, last_used_at, pinned
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?14,0)",
            params![
                kind.as_str(),
                preview,
                text,
                html,
                rtf_path,
                blob_path,
                file_json,
                mime,
                img_wh.0.map(|v| v as i64),
                img_wh.1.map(|v| v as i64),
                byte_len,
                sha,
                source_app,
                now,
            ],
        )
        .map_err(|e| format!("could not stack an item: {e}"))?;

        let id = conn.last_insert_rowid();
        drop(conn);
        self.trim(max_items)?;
        Ok(Some(id))
    }

    fn write_blob(&self, sha: &str, ext: &str, bytes: &[u8]) -> Result<String, String> {
        let dir = self.data_dir.join("blobs");
        fs::create_dir_all(&dir).map_err(|e| format!("could not create blobs: {e}"))?;
        let path = dir.join(format!("{sha}.{ext}"));
        fs::write(&path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Enforce `max_items`, deleting the payload files of anything evicted.
    pub fn trim(&self, max_items: u32) -> Result<(), String> {
        let conn = self.lock()?;
        let keep = max_items.max(1) as i64;
        let mut stmt = conn
            .prepare(
                "SELECT id, blob_path, rtf_path FROM clip_items
                 ORDER BY pinned DESC, last_used_at DESC
                 LIMIT -1 OFFSET ?1",
            )
            .map_err(|e| format!("could not prepare the trim query: {e}"))?;
        let doomed: Vec<(i64, Option<String>, Option<String>)> = stmt
            .query_map(params![keep], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|e| format!("could not list items to trim: {e}"))?
            .filter_map(Result::ok)
            .collect();
        drop(stmt);

        for (id, blob, rtf) in doomed {
            if let Some(p) = blob {
                unlink(&p);
            }
            if let Some(p) = rtf {
                unlink(&p);
            }
            conn.execute("DELETE FROM clip_items WHERE id = ?1", params![id])
                .map_err(|e| format!("could not evict an item: {e}"))?;
        }
        Ok(())
    }

    /// The whole stack, newest first, as picker summaries.
    pub fn list(&self, limit: i64) -> Result<Vec<ClipSummary>, String> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(SELECT_ROWS)
            .map_err(|e| format!("could not prepare the list query: {e}"))?;
        let rows = stmt
            .query_map(params![limit], row_to_summary)
            .map_err(|e| format!("could not read the stack: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("could not read the stack: {e}"))
    }

    /// Newest-first stack filtered by a fuzzy query. Empty query returns all.
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<ClipSummary>, String> {
        let all = self.list(SEARCH_SCAN_LIMIT)?;
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Ok(all.into_iter().take(limit as usize).collect());
        }
        let mut scored: Vec<(u32, ClipSummary)> = all
            .into_iter()
            .filter_map(|item| {
                haystack(&item)
                    .iter()
                    .filter_map(|h| score(&q, h))
                    .max()
                    .map(|s| (s, item))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(scored.into_iter().map(|(_, item)| item).take(limit as usize).collect())
    }

    pub fn detail(&self, id: i64) -> Result<Option<ClipDetail>, String> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT kind, text, file_urls, blob_path, mime, width, height
                 FROM clip_items WHERE id = ?1",
            )
            .map_err(|e| format!("could not prepare the detail query: {e}"))?;
        let found = stmt
            .query_map(params![id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                ))
            })
            .map_err(|e| format!("could not read an item: {e}"))?
            .next()
            .and_then(Result::ok);
        drop(stmt);

        let Some((kind_s, text, files_json, blob_path, _mime, width, height)) = found else {
            return Ok(None);
        };
        let kind = ClipKind::from_str(&kind_s).unwrap_or(ClipKind::Text);
        let file_urls: Vec<String> =
            serde_json::from_str(&files_json.unwrap_or_else(|| "[]".to_string())).unwrap_or_default();

        let data_url = match (kind, &blob_path) {
            (ClipKind::Image, Some(path)) => fs::read(path)
                .ok()
                .filter(|b| b.len() <= MAX_PREVIEW_IMAGE_BYTES)
                .map(|b| {
                    use base64::Engine;
                    let mime = mime_for_path(path);
                    format!(
                        "data:{mime};base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(&b)
                    )
                }),
            _ => None,
        };

        Ok(Some(ClipDetail {
            id,
            kind,
            text,
            file_urls,
            data_url,
            width,
            height,
        }))
    }

    /// The full row, used by the paste pipeline.
    pub fn get(&self, id: i64) -> Result<Option<ClipItem>, String> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, preview, text, html, rtf_path, blob_path, file_urls,
                        mime, width, height, byte_len, sha256, source_app, created_at,
                        last_used_at, pinned
                 FROM clip_items WHERE id = ?1",
            )
            .map_err(|e| format!("could not prepare the item query: {e}"))?;
        let found = stmt
            .query_map(params![id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, i64>(14)?,
                    row.get::<_, i64>(15)?,
                    row.get::<_, i64>(16)?,
                ))
            })
            .map_err(|e| format!("could not read an item: {e}"))?
            .next()
            .and_then(Result::ok);

        Ok(found.map(|r| ClipItem {
            id: r.0,
            kind: ClipKind::from_str(&r.1).unwrap_or(ClipKind::Text),
            preview: r.2,
            text: r.3,
            html: r.4,
            rtf_path: r.5,
            blob_path: r.6,
            file_urls: serde_json::from_str(&r.7.unwrap_or_else(|| "[]".to_string()))
                .unwrap_or_default(),
            mime: r.8,
            width: r.9,
            height: r.10,
            byte_len: r.11,
            sha256: r.12,
            source_app: r.13,
            created_at: r.14,
            last_used_at: r.15,
            pinned: r.16 != 0,
        }))
    }

    pub fn mark_used(&self, id: i64) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE clip_items SET last_used_at = ?1 WHERE id = ?2",
            params![now_ms(), id],
        )
        .map_err(|e| format!("could not stamp an item: {e}"))?;
        Ok(())
    }

    pub fn remove(&self, id: i64) -> Result<(), String> {
        let conn = self.lock()?;
        // A single row, so `query_row` avoids the borrow a prepared statement
        // would hold over `conn`.
        let paths: Vec<String> = conn
            .query_row(
                "SELECT blob_path, rtf_path FROM clip_items WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .map(|(blob, rtf)| [blob, rtf].into_iter().flatten().collect())
            .unwrap_or_default();
        conn.execute("DELETE FROM clip_items WHERE id = ?1", params![id])
            .map_err(|e| format!("could not delete an item: {e}"))?;
        drop(conn);
        for p in paths {
            unlink(&p);
        }
        Ok(())
    }

    pub fn set_pinned(&self, id: i64, pinned: bool) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE clip_items SET pinned = ?1 WHERE id = ?2",
            params![pinned as i64, id],
        )
        .map_err(|e| format!("could not pin an item: {e}"))?;
        Ok(())
    }

    /// Empty the stack and delete every payload file.
    pub fn clear(&self) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM clip_items", [])
            .map_err(|e| format!("could not clear the stack: {e}"))?;
        drop(conn);
        let dir = self.data_dir.join("blobs");
        if dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }

    pub fn count(&self) -> Result<i64, String> {
        let conn = self.lock()?;
        conn.query_row("SELECT COUNT(*) FROM clip_items", [], |r| r.get(0))
            .map_err(|e| format!("could not count the stack: {e}"))
    }

    /// On-disk size of the database plus every payload file, in bytes.
    pub fn storage_bytes(&self) -> Result<i64, String> {
        let mut total = 0i64;
        for name in ["clipstack.db", "clipstack.db-wal"] {
            if let Ok(meta) = fs::metadata(self.data_dir.join(name)) {
                total += meta.len() as i64;
            }
        }
        let dir = self.data_dir.join("blobs");
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    total += meta.len() as i64;
                }
            }
        }
        Ok(total)
    }
}

/// `pinned` plus two computed flags: does this row hold rich flavours, and does
/// its HTML embed a picture? Both are wrapped so a row with no HTML yields 0
/// rather than NULL, which the row mapper could not read as a bool.
const SELECT_ROWS: &str = "SELECT id, kind, preview, text, file_urls, byte_len, width, height,
        source_app, created_at, last_used_at, pinned,
        IFNULL(html IS NOT NULL OR rtf_path IS NOT NULL, 0),
        IFNULL(html LIKE '%<img%', 0)
     FROM clip_items
     ORDER BY pinned DESC, last_used_at DESC
     LIMIT ?1";

fn row_to_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClipSummary> {
    let kind_s: String = row.get(1)?;
    let kind = ClipKind::from_str(&kind_s).unwrap_or(ClipKind::Text);
    let text: Option<String> = row.get(3)?;
    let files_json: Option<String> = row.get(4)?;
    let file_count = serde_json::from_str::<Vec<String>>(&files_json.unwrap_or_default())
        .map(|v| v.len())
        .unwrap_or(0);
    let line_count = text
        .as_deref()
        .map(|t| t.lines().count().max(1))
        .unwrap_or(0);
    Ok(ClipSummary {
        id: row.get(0)?,
        kind,
        preview: row.get(2)?,
        line_count,
        file_count,
        byte_len: row.get(5)?,
        width: row.get(6)?,
        height: row.get(7)?,
        source_app: row.get(8)?,
        created_at: row.get(9)?,
        last_used_at: row.get(10)?,
        pinned: row.get::<_, i64>(11)? != 0,
        has_formatting: row.get::<_, i64>(12)? != 0,
        has_inline_image: row.get::<_, i64>(13)? != 0,
    })
}

fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS clip_items (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            kind        TEXT    NOT NULL,
            preview     TEXT    NOT NULL,
            text        TEXT,
            html        TEXT,
            rtf_path    TEXT,
            blob_path   TEXT,
            file_urls   TEXT    NOT NULL DEFAULT '[]',
            mime        TEXT,
            width       INTEGER,
            height      INTEGER,
            byte_len    INTEGER NOT NULL DEFAULT 0,
            sha256      TEXT    NOT NULL,
            source_app  TEXT,
            created_at  INTEGER NOT NULL,
            last_used_at INTEGER NOT NULL,
            pinned      INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_clip_sha   ON clip_items(sha256);
        CREATE INDEX IF NOT EXISTS idx_clip_order ON clip_items(pinned DESC, last_used_at DESC);",
    )
    .map_err(|e| format!("could not migrate the clipboard store: {e}"))?;
    Ok(())
}

fn byte_len_check(len: i64) -> Result<(), String> {
    if len > MAX_PAYLOAD_BYTES {
        Err(format!(
            "that item is {} MB, above the {} MB limit",
            len / 1024 / 1024,
            MAX_PAYLOAD_BYTES / 1024 / 1024
        ))
    } else {
        Ok(())
    }
}

fn content_hash(
    kind: ClipKind,
    text: &Option<String>,
    files: &[String],
    blob: Option<&[u8]>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_str().as_bytes());
    hasher.update([0u8]);
    match kind {
        ClipKind::Text => hasher.update(text.as_deref().unwrap_or("").as_bytes()),
        ClipKind::Image => hasher.update(blob.unwrap_or(&[])),
        ClipKind::Files => {
            for path in files {
                hasher.update(path.as_bytes());
                hasher.update([0u8]);
            }
        }
    }
    hex::encode(hasher.finalize())
}

fn image_dims(bytes: &[u8]) -> Option<(Option<u32>, Option<u32>)> {
    if bytes.is_empty() {
        return None;
    }
    image::guess_format(bytes)
        .ok()
        .and_then(|fmt| image::load_from_memory_with_format(bytes, fmt).ok())
        .map(|img| {
            let (w, h) = img.dimensions();
            (Some(w), Some(h))
        })
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .collect::<String>();
    let flat = if flat.is_empty() {
        text.trim().replace('\n', " ")
    } else {
        flat
    };
    if flat.chars().count() > max {
        let cut: String = flat.chars().take(max).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/tiff" => "tiff",
        "image/bmp" => "bmp",
        "image/webp" => "webp",
        _ => "bin",
    }
}

fn mime_for_path(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("tiff" | "tif") => "image/tiff",
        Some("bmp") => "image/bmp",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

fn unlink(path: &str) {
    let _ = fs::remove_file(path);
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn haystack(item: &ClipSummary) -> Vec<String> {
    let mut out = vec![item.preview.clone()];
    if let Some(app) = &item.source_app {
        out.push(app.clone());
    }
    out
}

/// Subsequence score for one haystack: contiguous runs and word-start matches
/// are rewarded so "safari" beats a scattered letter match.
fn score(query: &str, hay: &str) -> Option<u32> {
    let hay = hay.to_lowercase();
    if hay.is_empty() {
        return None;
    }
    if let Some(pos) = hay.find(query) {
        let start_bonus = if pos == 0 { 60 } else { 20 };
        return Some(1000 + start_bonus - (pos as u32).min(50));
    }

    let mut hi = hay.chars().peekable();
    let mut total: u32 = 0;
    let mut matched = 0usize;
    let mut prev_match = false;
    let mut idx = 0usize;

    for qc in query.chars() {
        let mut found = false;
        while let Some(hc) = hi.next() {
            idx += 1;
            if hc == qc {
                let word_start = idx == 1 || matches!(hay.chars().nth(idx - 2), Some(' ') | Some('-') | Some('/'));
                let mut pts = 2;
                if prev_match {
                    pts += 6;
                }
                if word_start {
                    pts += 5;
                }
                total += pts;
                matched += 1;
                prev_match = true;
                found = true;
                break;
            }
            prev_match = false;
        }
        if !found {
            return None;
        }
    }

    if matched == query.chars().count() {
        Some(total)
    } else {
        None
    }
}
