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

/** Namespace helper for the inline icon set; never built from user content. */
function svgNode(tag: string, attrs: Record<string, string>): SVGElement {
  const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  return node;
}

/**
 * Build one icon from path data. One stroke weight, round joins, no fills
 * across the whole set, so the kind badges and the row actions read as a
 * family rather than three unrelated glyphs.
 */
function icon(paths: string[], viewBox = "0 0 14 14"): SVGSVGElement {
  const svg = svgNode("svg", { viewBox, "aria-hidden": "true", focusable: "false" }) as SVGSVGElement;
  for (const d of paths) {
    svg.append(
      svgNode("path", {
        d,
        fill: "none",
        stroke: "currentColor",
        "stroke-width": "1.3",
        "stroke-linecap": "round",
        "stroke-linejoin": "round",
      }),
    );
  }
  return svg;
}

/**
 * The kind badges: text is a run of lines, an image is a framed picture, a
 * file is a folder. All three share the 14-unit box and stroke weight.
 */
const KIND_ICON: Record<string, () => SVGSVGElement> = {
  text: () => icon(["M2.4 3.9h9.2", "M2.4 7h9.2", "M2.4 10.1h5.6"]),
  image: () => {
    const svg = icon(["M1.9 4.1a1.4 1.4 0 0 1 1.4-1.4h7.4a1.4 1.4 0 0 1 1.4 1.4v5.8a1.4 1.4 0 0 1-1.4 1.4H3.3a1.4 1.4 0 0 1-1.4-1.4z", "M2.4 9.1l2.9-2.7 2.2 2 1.7-1.5 2.4 2.2"]);
    svg.append(
      svgNode("circle", {
        cx: "5.1",
        cy: "5.6",
        r: "1",
        fill: "none",
        stroke: "currentColor",
        "stroke-width": "1.3",
      }),
    );
    return svg;
  },
  files: () => icon(["M1.9 4a1.2 1.2 0 0 1 1.2-1.2h2.5l1.3 1.6h4a1.2 1.2 0 0 1 1.2 1.2v4.6a1.2 1.2 0 0 1-1.2 1.2H3.1A1.2 1.2 0 0 1 1.9 10.2z"]),
};

/**
 * Two offset sheets: the copy glyph shared by both row actions. Rounded
 * corners keep it reading as documents at 13px; the sheets stay empty so the
 * sparkle pair is the only decoration competing with them.
 */
function copyIcon(withSparkle: boolean): SVGSVGElement {
  const svg = icon([
    // Back sheet, drawn as the visible L around the front one.
    "M4.9 4.7V3.4a1.2 1.2 0 0 1 1.2-1.2h3.2a1.2 1.2 0 0 1 1.2 1.2v3.2a1.2 1.2 0 0 1-1.2 1.2H8",
    // Front sheet.
    "M3.1 4.9h3.4a1.2 1.2 0 0 1 1.2 1.2v4.2a1.2 1.2 0 0 1-1.2 1.2H3.1a1.2 1.2 0 0 1-1.2-1.2V6.1a1.2 1.2 0 0 1 1.2-1.2z",
  ]);
  if (withSparkle) {
    // Sparkles say "this one keeps what the copy carried" — formatting, and
    // any inline image — while the unadorned glyph pastes the plain string.
    // Four-point stars pinched toward the centre: a large one with a small
    // companion, so the pair still reads at 13px.
    for (const d of [
      "M10.9 7.5Q10.9 10.2 13.6 10.2Q10.9 10.2 10.9 12.9Q10.9 10.2 8.2 10.2Q10.9 10.2 10.9 7.5Z",
      "M8.1 11.3Q8.1 12.5 9.3 12.5Q8.1 12.5 8.1 13.7Q8.1 12.5 6.9 12.5Q8.1 12.5 8.1 11.3Z",
    ]) {
      svg.append(svgNode("path", { d, fill: "currentColor", stroke: "none" }));
    }
  }
  return svg;
}

function subtitleFor(item: ClipSummary): string {
  switch (item.kind) {
    case "image":
      return item.width && item.height
        ? `${item.width}×${item.height} · ${formatBytes(item.byteLen)}`
        : formatBytes(item.byteLen);
    case "files":
      return item.fileCount === 1 ? "1 file" : `${item.fileCount} files`;
    default: {
      const head = item.lineCount > 1 ? `${item.lineCount} lines` : item.sourceApp ?? "Text";
      // Say what the capture actually holds: rich text that embeds a picture
      // files as text (the image lives inside the markup), which otherwise
      // reads like a misclassification.
      if (item.hasInlineImage) return `${head} · formatted + image`;
      if (item.hasFormatting) return `${head} · formatted`;
      return head;
    }
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

    const badge = el("span", `row__badge row__badge--${item.kind}`);
    badge.append((KIND_ICON[item.kind] ?? KIND_ICON.text)());
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

    // Copy-only action: puts the item on the clipboard without pasting. On a
    // formatted text row this is the rich variant, marked with sparkles.
    const rich = item.kind === "text" && item.hasFormatting;
    const copy = el("button", "row__copy");
    copy.type = "button";
    copy.title = rich
      ? "Copy as it was copied, formatting included (⌘C)"
      : "Copy to clipboard (⌘C)";
    copy.setAttribute("aria-label", "Copy to clipboard");
    copy.append(copyIcon(rich));
    copy.addEventListener("click", (event) => {
      event.stopPropagation();
      void copyToClipboard(index, "rich");
    });
    row.append(copy);

    // Second command for text that captured formatting: the same payload
    // stripped to the plain string (drops colour, fonts, inline images). Same
    // copy glyph, undecorated, so the pair reads as one action with two
    // outcomes rather than two unrelated buttons.
    if (rich) {
      const plain = el("button", "row__copy row__copy--plain");
      plain.type = "button";
      plain.title = item.hasInlineImage
        ? "Copy as plain text — drops the formatting and inline image (⌘⇧C)"
        : "Copy as plain text — drops the formatting (⌘⇧C)";
      plain.setAttribute("aria-label", "Copy as plain text");
      plain.append(copyIcon(false));
      plain.addEventListener("click", (event) => {
        event.stopPropagation();
        void copyToClipboard(index, "plain");
      });
      row.append(plain);
    }

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
      const mark = el("span", "row__badge row__badge--files");
      mark.append(KIND_ICON.files());
      entry.append(mark);
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

async function copyToClipboard(index: number, format: "plain" | "rich" = "rich"): Promise<void> {
  const item = items[index];
  if (!item) return;
  try {
    await api.copyClip(item.id, format);
  } catch (error) {
    setStatus(describe(error), "error");
    return;
  }
  setStatus(format === "plain" ? "Copied as plain text" : "Copied to the clipboard");
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
    if (event.key.toLowerCase() === "c") {
      // Cmd+C with a selection inside the search field copies that text;
      // anywhere else it copies the selected stack item to the clipboard.
      if (document.activeElement === queryEl && queryEl.selectionStart !== queryEl.selectionEnd) {
        return;
      }
      event.preventDefault();
      // Cmd+Shift+C strips whatever formatting the text entry captured.
      void copyToClipboard(selected, event.shiftKey ? "plain" : "rich");
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
  // Focus without selecting: an empty field keeps Cmd+C aimed at the stack
  // item rather than at selected placeholder text.
  queryEl.focus();
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
