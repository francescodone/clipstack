import { listen } from "@tauri-apps/api/event";

import { api, formatAgo, formatBytes } from "./lib/ipc";
import type { ClipDetail, ClipSummary } from "./lib/types";

// The picker webview. One instance is created by Rust and only hidden, so this
// module must be able to reset itself every time it is shown again.

const queryEl = document.getElementById("query") as HTMLInputElement;
const resultsEl = document.getElementById("results") as HTMLUListElement;
const previewEl = document.getElementById("preview") as HTMLElement;
const countEl = document.getElementById("count") as HTMLElement;
const statusEl = document.getElementById("status") as HTMLElement;

/** How long to wait after the last keystroke before hitting SQLite. */
const SEARCH_DEBOUNCE_MS = 110;
/** Rows kept in the list; the search itself is capped server-side. */
const LIST_LIMIT = 200;

let items: ClipSummary[] = [];
let selected = 0;
let totalInStack = 0;
let searchTimer: number | undefined;
/** Guards against a slow preview response overwriting a newer one. */
let previewToken = 0;

// ---------------------------------------------------------------- rendering

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/** Small inline glyphs for the row badges; never built from user content. */
const KIND_GLYPH: Record<string, string> = {
  text: "A",
  image: "▣",
  files: "⧉",
};

function subtitleFor(item: ClipSummary): string {
  switch (item.kind) {
    case "image":
      return item.width && item.height
        ? `${item.width}×${item.height} · ${formatBytes(item.byteLen)}`
        : formatBytes(item.byteLen);
    case "files":
      return item.fileCount === 1 ? "1 file" : `${item.fileCount} files`;
    default:
      return item.lineCount > 1 ? `${item.lineCount} lines` : item.sourceApp ?? "Text";
  }
}

function renderList(): void {
  resultsEl.replaceChildren();

  if (items.length === 0) {
    const empty = el("li", "picker__empty");
    empty.textContent = queryEl.value.trim()
      ? "No matches in the stack"
      : "Nothing copied yet";
    resultsEl.append(empty);
    countEl.textContent = "";
    return;
  }

  items.forEach((item, index) => {
    const row = el("li", "row");
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", String(index === selected));
    row.dataset.index = String(index);

    const badge = el("span", `row__badge row__badge--${item.kind}`, KIND_GLYPH[item.kind]);
    row.append(badge);

    const text = el("span", "row__text");
    text.append(el("span", "row__title", item.preview));
    text.append(el("span", "row__sub", subtitleFor(item)));
    row.append(text);

    const meta = el("span", "row__meta");
    if (item.pinned) {
      const pin = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      pin.setAttribute("class", "row__pin");
      pin.setAttribute("viewBox", "0 0 12 12");
      pin.setAttribute("aria-hidden", "true");
      const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
      path.setAttribute("fill", "currentColor");
      path.setAttribute("d", "M6 1.2 7.4 4.6 11 4.9 8.3 7.3 9 10.8 6 9 3 10.8 3.7 7.3 1 4.9 4.6 4.6Z");
      pin.append(path);
      meta.append(pin);
    }
    meta.append(el("span", undefined, formatAgo(item.createdAt)));
    row.append(meta);

    resultsEl.append(row);
  });

  countEl.textContent = queryEl.value.trim()
    ? `${items.length} of ${totalInStack}`
    : String(totalInStack || items.length);
}

function scrollSelectedIntoView(): void {
  const row = resultsEl.querySelector<HTMLElement>(`[data-index="${selected}"]`);
  row?.scrollIntoView({ block: "nearest" });
}

function setSelected(index: number): void {
  if (items.length === 0) return;
  selected = Math.max(0, Math.min(items.length - 1, index));
  renderList();
  scrollSelectedIntoView();
  void showPreview(items[selected]);
}

// ------------------------------------------------------------------ preview

async function showPreview(item: ClipSummary | undefined): Promise<void> {
  const token = ++previewToken;
  previewEl.replaceChildren();

  if (!item) {
    previewEl.append(el("p", "picker__preview-empty", "Nothing selected"));
    return;
  }

  previewEl.append(el("h2", "preview__heading", item.sourceApp ?? labelForKind(item.kind)));

  let detail: ClipDetail | null;
  try {
    detail = await api.getClip(item.id);
  } catch {
    return;
  }
  // A newer selection won the race; drop this response.
  if (token !== previewToken) return;

  if (!detail) {
    previewEl.replaceChildren(el("p", "preview__note", "This item is no longer in the stack."));
    return;
  }

  if (detail.kind === "image" && detail.dataUrl) {
    const img = el("img", "preview__image");
    img.src = detail.dataUrl;
    img.alt = "Copied image preview";
    previewEl.append(img);
    if (detail.width && detail.height) {
      previewEl.append(
        el("p", "preview__note", `${detail.width} × ${detail.height} pixels`),
      );
    }
    return;
  }

  if (detail.kind === "files") {
    const list = el("ul", "preview__files");
    for (const url of detail.fileUrls) {
      const entry = el("li", "preview__file");
      entry.append(el("span", "row__badge row__badge--files", KIND_GLYPH.files));
      entry.append(el("span", "preview__file-name", basename(url)));
      list.append(entry);
    }
    previewEl.append(list);
    return;
  }

  const text = detail.text ?? item.preview;
  if (!text) {
    previewEl.append(el("p", "preview__note", "No text preview available."));
    return;
  }
  // Long payloads are truncated here rather than in Rust, so the list query
  // stays cheap and the pane still scrolls.
  const shown = text.length > 20000 ? `${text.slice(0, 20000)}\n…` : text;
  previewEl.append(el("pre", "preview__text", shown));
}

function labelForKind(kind: string): string {
  return kind === "image" ? "Image" : kind === "files" ? "Files" : "Text";
}

function basename(fileUrl: string): string {
  const decoded = tryDecode(fileUrl);
  const trimmed = decoded.replace(/\/+$/, "");
  const slash = trimmed.lastIndexOf("/");
  return slash === -1 ? trimmed : trimmed.slice(slash + 1);
}

function tryDecode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

// ------------------------------------------------------------------- status

let statusClearTimer: number | undefined;

function setStatus(message: string, kind: "info" | "error" = "info"): void {
  statusEl.textContent = message;
  statusEl.classList.toggle("picker__status--error", kind === "error");
  if (statusClearTimer !== undefined) window.clearTimeout(statusClearTimer);
  if (message && kind === "info") {
    statusClearTimer = window.setTimeout(() => setStatus(""), 2600);
  }
}

// ------------------------------------------------------------------- loading

async function load(query: string): Promise<void> {
  const trimmed = query.trim();
  try {
    items = trimmed
      ? await api.searchClips(trimmed, LIST_LIMIT)
      : await api.listClips(LIST_LIMIT);
  } catch (error) {
    setStatus(describe(error), "error");
    return;
  }
  selected = 0;
  renderList();
  void showPreview(items[0]);
}

function scheduleLoad(): void {
  if (searchTimer !== undefined) window.clearTimeout(searchTimer);
  searchTimer = window.setTimeout(() => void load(queryEl.value), SEARCH_DEBOUNCE_MS);
}

async function refreshCount(): Promise<void> {
  try {
    totalInStack = (await api.stackStats()).count;
  } catch {
    totalInStack = items.length;
  }
}

function describe(error: unknown): string {
  return typeof error === "string" ? error : error instanceof Error ? error.message : "Something went wrong";
}

// -------------------------------------------------------------------- paste

let busy = false;

async function paste(index: number): Promise<void> {
  const item = items[index];
  if (!item || busy) return;
  busy = true;
  setStatus("Pasting…");
  try {
    await api.pasteClip(item.id);
  } catch (error) {
    setStatus(describe(error), "error");
  } finally {
    busy = false;
  }
}

async function removeSelected(): Promise<void> {
  const item = items[selected];
  if (!item) return;
  try {
    await api.deleteClip(item.id);
  } catch (error) {
    setStatus(describe(error), "error");
    return;
  }
  await load(queryEl.value);
  setStatus("Removed from the stack");
}

async function togglePin(): Promise<void> {
  const item = items[selected];
  if (!item) return;
  try {
    await api.pinClip(item.id, !item.pinned);
  } catch (error) {
    setStatus(describe(error), "error");
    return;
  }
  await load(queryEl.value);
  setStatus(item.pinned ? "Unpinned" : "Pinned to the top");
}

// ----------------------------------------------------------------- keyboard

function onKeyDown(event: KeyboardEvent): void {
  // Escape works even when the input has been blurred by a click.
  if (event.key === "Escape") {
    event.preventDefault();
    void api.hidePicker();
    return;
  }

  if (event.metaKey && !event.ctrlKey && !event.altKey) {
    // Cmd+1…9 pastes the matching row, the way a launcher picks a result.
    if (/^[1-9]$/.test(event.key)) {
      event.preventDefault();
      void paste(Number(event.key) - 1);
      return;
    }
    if (event.key === "Backspace") {
      event.preventDefault();
      void removeSelected();
      return;
    }
    if (event.key.toLowerCase() === "p") {
      event.preventDefault();
      void togglePin();
      return;
    }
    // Let the browser handle Cmd+A/C/V inside the search field.
    return;
  }

  switch (event.key) {
    case "ArrowDown":
      event.preventDefault();
      setSelected(selected + 1);
      break;
    case "ArrowUp":
      event.preventDefault();
      setSelected(selected - 1);
      break;
    case "PageDown":
      event.preventDefault();
      setSelected(selected + 8);
      break;
    case "PageUp":
      event.preventDefault();
      setSelected(selected - 8);
      break;
    case "Enter":
      event.preventDefault();
      void paste(selected);
      break;
    default:
      break;
  }
}

// ------------------------------------------------------------------ wiring

resultsEl.addEventListener("click", (event) => {
  const row = (event.target as HTMLElement).closest<HTMLElement>("[data-index]");
  if (!row) return;
  const index = Number(row.dataset.index);
  if (event.detail === 2) {
    void paste(index);
  } else {
    setSelected(index);
  }
});

queryEl.addEventListener("input", scheduleLoad);

window.addEventListener("keydown", onKeyDown);

/** Reset and refetch each time Rust shows the panel. */
async function reset(): Promise<void> {
  queryEl.value = "";
  busy = false;
  await Promise.all([refreshCount(), load("")]);
  queryEl.focus();
  queryEl.select();
}

async function main(): Promise<void> {
  await listen("picker://open", () => {
    void reset();
  });

  await listen("stack://paste-failed", (event) => {
    setStatus(String(event.payload ?? "The paste did not go through"), "error");
  });

  // The tray and the poller both announce stack changes; the picker is only
  // hidden between appearances, so keep the badge honest.
  await listen("stack://changed", () => {
    void refreshCount();
  });

  await Promise.all([refreshCount(), load("")]);
  queryEl.focus();
}

void main();
