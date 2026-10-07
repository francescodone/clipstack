import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

import { api, formatBytes, formatShortcut, keyEventToAccelerator } from "./lib/ipc";
import type { Settings } from "./lib/types";

// The settings window. Everything is written through `save_settings`, which is
// the single place that re-applies a change to the running app, so this module
// only has to keep the form and the persisted `Settings` in step.

const checkbox = {
  captureText: must<HTMLInputElement>("capture-text"),
  captureImages: must<HTMLInputElement>("capture-images"),
  captureFiles: must<HTMLInputElement>("capture-files"),
  contentProtection: must<HTMLInputElement>("content-protection"),
  preserve: must<HTMLInputElement>("preserve"),
  autostart: must<HTMLInputElement>("autostart"),
  paused: must<HTMLInputElement>("paused"),
};

const maxItems = must<HTMLInputElement>("max-items");
const excludedInput = must<HTMLInputElement>("excluded");
const excludedList = must<HTMLUListElement>("excluded-list");
const shortcutButton = must<HTMLButtonElement>("shortcut");
const shortcutReset = must<HTMLButtonElement>("shortcut-reset");
const shortcutNote = must<HTMLElement>("shortcut-note");
const stackSummary = must<HTMLElement>("stack-summary");
const clearButton = must<HTMLButtonElement>("clear");
const quitButton = must<HTMLButtonElement>("quit");
const savedEl = must<HTMLElement>("saved");

const a11yDot = must<HTMLElement>("a11y-dot");
const a11yText = must<HTMLElement>("a11y-text");
const a11yRequest = must<HTMLButtonElement>("a11y-request");
const a11yOpen = must<HTMLButtonElement>("a11y-open");

const updateState = must<HTMLElement>("update-state");
const updateNote = must<HTMLElement>("update-note");
const updateCheck = must<HTMLButtonElement>("update-check");
const updateInstall = must<HTMLButtonElement>("update-install");

function must<T extends HTMLElement>(id: string): T {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing element #${id}`);
  return node as T;
}

let current: Settings | null = null;
/** True while the recorder is capturing the next key combination. */
let recording = false;
let saveToken = 0;

// ------------------------------------------------------------------ populate

function render(settings: Settings): void {
  checkbox.captureText.checked = settings.captureText;
  checkbox.captureImages.checked = settings.captureImages;
  checkbox.captureFiles.checked = settings.captureFiles;
  checkbox.contentProtection.checked = settings.contentProtection;
  checkbox.preserve.checked = settings.preserveOnShutdown;
  checkbox.paused.checked = settings.paused;
  maxItems.value = String(settings.maxItems);
  shortcutButton.textContent = formatShortcut(settings.pickerShortcut);
  renderChips(settings.excludedApps);
}

function renderChips(apps: string[]): void {
  excludedList.replaceChildren();
  for (const app of apps) {
    const chip = document.createElement("li");
    chip.className = "chip";
    chip.append(document.createTextNode(app));

    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "×";
    remove.setAttribute("aria-label", `Stop excluding ${app}`);
    remove.addEventListener("click", () => {
      const next = (current?.excludedApps ?? []).filter((entry) => entry !== app);
      void commit({ excludedApps: next });
    });
    chip.append(remove);
    excludedList.append(chip);
  }
}

// ------------------------------------------------------------------- saving

/**
 * Persist a partial change. Saves are serialised through a token so a slow
 * write cannot overwrite a newer one, and the shortcut is sent on its own path
 * so a rejected combination can be rolled back visibly.
 */
async function commit(patch: Partial<Settings>): Promise<void> {
  if (!current) return;
  const token = ++saveToken;
  const next: Settings = { ...current, ...patch };
  try {
    const saved = await api.saveSettings(next);
    if (token !== saveToken) return;
    current = saved;
    render(saved);
    flash("Saved");
    void refreshStats();
  } catch (error) {
    if (token !== saveToken) return;
    flash(describe(error), true);
    // Re-read rather than guessing: the Rust side may have partially applied
    // the change before failing.
    void load();
  }
}

async function flash(message: string, isError = false): Promise<void> {
  savedEl.textContent = message;
  savedEl.classList.toggle("settings__saved--error", isError);
  if (!isError) {
    window.setTimeout(() => {
      if (savedEl.textContent === message) savedEl.textContent = "";
    }, 1800);
  }
}

function describe(error: unknown): string {
  return typeof error === "string"
    ? error
    : error instanceof Error
      ? error.message
      : "That change could not be saved";
}

// ----------------------------------------------------------------- shortcut

const SHORTCUT_HINT = "Click the field, then press the combination you want. It must include ⌘.";

function setShortcutNote(message: string, isError = false): void {
  shortcutNote.textContent = message;
  shortcutNote.style.color = isError ? "var(--destructive)" : "";
}

async function bindShortcut(accelerator: string): Promise<void> {
  shortcutButton.textContent = formatShortcut(accelerator);
  shortcutButton.classList.remove("recorder--armed");
  recording = false;
  try {
    current = await api.setShortcut(accelerator);
    flash("Shortcut saved");
    setShortcutNote(SHORTCUT_HINT);
  } catch (error) {
    shortcutButton.classList.add("recorder--invalid");
    const message = describe(error);
    flash(message, true);
    setShortcutNote(message, true);
    // Put the binding actually in force back on the button.
    void load();
  }
}

function armRecorder(): void {
  recording = true;
  shortcutButton.classList.add("recorder--armed");
  shortcutButton.classList.remove("recorder--invalid");
  shortcutButton.textContent = "Press a combination…";
  setShortcutNote("Press Escape to cancel.");
}

function disarmRecorder(): void {
  if (!recording) return;
  recording = false;
  shortcutButton.classList.remove("recorder--armed");
  setShortcutNote(SHORTCUT_HINT);
  if (current) shortcutButton.textContent = formatShortcut(current.pickerShortcut);
}

// ------------------------------------------------------------- accessibility

async function refreshAccessibility(): Promise<void> {
  let trusted = false;
  try {
    trusted = await api.accessibilityState();
  } catch {
    a11yDot.className = "dot dot--unknown";
    a11yText.textContent = "Could not determine the Accessibility status";
    return;
  }
  a11yDot.className = trusted ? "dot dot--ok" : "dot dot--missing";
  a11yText.textContent = trusted
    ? "Accessibility access is granted — ClipStack can paste for you"
    : "Accessibility access is not granted yet";
  a11yRequest.hidden = trusted;
}

// --------------------------------------------------------------------- stats

async function refreshStats(): Promise<void> {
  try {
    const stats = await api.stackStats();
    stackSummary.textContent =
      stats.count === 0
        ? "The stack is empty"
        : `${stats.count} ${stats.count === 1 ? "item" : "items"} · ${formatBytes(stats.storageBytes)}`;
  } catch {
    stackSummary.textContent = "—";
  }
}

// -------------------------------------------------------------------- load

async function load(): Promise<void> {
  try {
    current = await api.getSettings();
  } catch (error) {
    flash(describe(error), true);
    return;
  }
  render(current);
  // The login-item state is owned by macOS, so it is read from the source of
  // truth rather than trusted from the settings file.
  try {
    checkbox.autostart.checked = await api.autostartState();
  } catch {
    checkbox.autostart.checked = current.runAtStartup;
  }
  await Promise.all([refreshStats(), refreshAccessibility()]);
}

// ------------------------------------------------------------------- wiring

for (const [id, element] of Object.entries(checkbox)) {
  element.addEventListener("change", () => {
    const checked = element.checked;
    if (id === "autostart") {
      // Enable/disable through the plugin, which is what actually talks to
      // the login-item API, then mirror the result.
      void (async () => {
        try {
          checkbox.autostart.checked = await api.setAutostart(checked);
          if (current) current.runAtStartup = checkbox.autostart.checked;
          flash("Saved");
        } catch (error) {
          checkbox.autostart.checked = !checked;
          flash(describe(error), true);
        }
      })();
      return;
    }
    const key =
      id === "preserve" ? "preserveOnShutdown" : (id as keyof Settings);
    void commit({ [key]: checked } as Partial<Settings>);
  });
}

maxItems.addEventListener("change", () => {
  const value = Math.round(Number(maxItems.value));
  if (!Number.isFinite(value) || value < 10 || value > 1000) {
    flash("Choose between 10 and 1000 items", true);
    if (current) maxItems.value = String(current.maxItems);
    return;
  }
  void commit({ maxItems: value });
});

excludedInput.addEventListener("keydown", (event) => {
  if (event.key !== "Enter") return;
  event.preventDefault();
  const value = excludedInput.value.trim();
  if (!value || !current) return;
  if (current.excludedApps.some((entry) => entry.toLowerCase() === value.toLowerCase())) {
    excludedInput.value = "";
    return;
  }
  excludedInput.value = "";
  void commit({ excludedApps: [...current.excludedApps, value] });
});

shortcutButton.addEventListener("click", () => {
  if (recording) {
    disarmRecorder();
    return;
  }
  armRecorder();
});

shortcutReset.addEventListener("click", () => void bindShortcut("Cmd+Shift+V"));

// Captured at the window level so the recorder works without moving focus.
window.addEventListener(
  "keydown",
  (event) => {
    if (!recording) return;
    if (event.key === "Escape") {
      event.preventDefault();
      disarmRecorder();
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    const accelerator = keyEventToAccelerator(event);
    if (!accelerator) {
      shortcutButton.textContent = "Include ⌘ …";
      return;
    }
    void bindShortcut(accelerator);
  },
  // Capture phase, ahead of anything else on the page.
  true,
);

document.addEventListener("click", (event) => {
  if (!recording) return;
  if (event.target === shortcutButton) return;
  disarmRecorder();
});

clearButton.addEventListener("click", () => {
  const label = clearButton.textContent;
  if (clearButton.dataset.armed !== "1") {
    // Two-step confirm instead of a modal, which would need a second window.
    clearButton.dataset.armed = "1";
    clearButton.textContent = "Really clear?";
    window.setTimeout(() => {
      clearButton.dataset.armed = "0";
      clearButton.textContent = label;
    }, 3000);
    return;
  }
  clearButton.dataset.armed = "0";
  clearButton.textContent = label;
  void (async () => {
    try {
      await api.clearStack();
      flash("Stack cleared");
      await refreshStats();
    } catch (error) {
      flash(describe(error), true);
    }
  })();
});

a11yRequest.addEventListener("click", () => {
  void api.requestAccessibility();
  // The prompt opens System Settings; poll so the indicator updates as soon as
  // the user flips the switch, without them touching this window.
  pollAccessibility();
});

a11yOpen.addEventListener("click", () => {
  void api.openAccessibilitySettings();
  pollAccessibility();
});

let a11yTimer: number | undefined;

function pollAccessibility(): void {
  if (a11yTimer !== undefined) return;
  let attempts = 0;
  a11yTimer = window.setInterval(() => {
    void refreshAccessibility();
    attempts += 1;
    if (attempts > 60) {
      window.clearInterval(a11yTimer);
      a11yTimer = undefined;
    }
  }, 1000);
}

quitButton.addEventListener("click", () => void api.quit());

// ------------------------------------------------------------------ updates

const UPDATE_HINT =
  "ClipStack looks for new versions on GitHub Releases. Updates are verified " +
  "with a signing key before they are installed.";

/** The pending update, kept so the install button does not re-check. */
let pendingUpdate: Awaited<ReturnType<typeof check>> = null;
let checking = false;

function setUpdateNote(message: string, isError = false): void {
  updateNote.textContent = message;
  updateNote.style.color = isError ? "var(--destructive)" : "";
}

async function checkForUpdates(auto: boolean): Promise<void> {
  if (checking) return;
  checking = true;
  updateCheck.disabled = true;
  updateState.textContent = "Checking for updates…";
  try {
    const update = await check();
    pendingUpdate = update;
    if (update) {
      updateState.textContent = `Update available: v${update.version} (you have v${await getVersion()})`;
      setUpdateNote(update.body?.trim() || "Open the notes on GitHub Releases for what changed.");
      updateInstall.hidden = false;
    } else if (!auto) {
      updateState.textContent = `You are up to date (v${await getVersion()})`;
      setUpdateNote(UPDATE_HINT);
      updateInstall.hidden = true;
    } else {
      // Stay quiet on a silent startup check that found nothing.
      updateState.textContent = `Up to date (v${await getVersion()})`;
    }
  } catch (error) {
    // A dev build has no bundled updater metadata, and a first run has no
    // network guarantee; neither is worth alarming the user over.
    updateState.textContent = "Updates are unavailable in this build";
    setUpdateNote(describe(error));
    updateInstall.hidden = true;
  } finally {
    checking = false;
    updateCheck.disabled = false;
  }
}

updateCheck.addEventListener("click", () => void checkForUpdates(false));

updateInstall.addEventListener("click", () => {
  const update = pendingUpdate;
  if (!update) return;
  updateInstall.disabled = true;
  updateCheck.disabled = true;
  let downloaded = 0;
  let contentLength = 0;
  void (async () => {
    try {
      updateState.textContent = "Downloading update…";
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          contentLength = event.data.contentLength ?? 0;
        } else if (event.event === "Progress" && contentLength > 0) {
          downloaded += event.data.chunkLength;
          const pct = Math.min(99, Math.round((downloaded / contentLength) * 100));
          updateState.textContent = `Downloading update… ${pct}%`;
        } else if (event.event === "Finished") {
          updateState.textContent = "Installing and relaunching…";
        }
      });
      updateState.textContent = "Relaunching…";
      await relaunch();
    } catch (error) {
      updateState.textContent = "The update could not be installed";
      setUpdateNote(describe(error), true);
      updateInstall.disabled = false;
      updateCheck.disabled = false;
    }
  })();
});

// A silent check shortly after the window opens, so the section is never
// showing a stale "Checking…" by the time the user scrolls to it.
window.setTimeout(() => void checkForUpdates(true), 800);

// The poller keeps running while this window is hidden, so refresh on return.
window.addEventListener("focus", () => {
  void Promise.all([refreshStats(), refreshAccessibility()]);
});

void load();
