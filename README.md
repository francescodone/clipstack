# ClipStack

A menu-bar clipboard history for macOS. Copy things the normal way and they
stack up; press the picker shortcut to search the stack and paste any earlier
entry into whatever app you are in.

It runs entirely in the menu bar — no Dock icon, no window until you ask for
one.

Press **⌘⇧V** anywhere, search, hit **↵** — the entry is pasted into the app
you were just in.

![The picker: search the stack, preview an entry, paste with Enter](docs/screenshots/picker.png)

Everything is configurable from a small settings window:

![Settings window](docs/screenshots/settings.png)

## Install

Download the latest `.dmg` from
[GitHub Releases](https://github.com/francescodone/clipstack/releases/latest),
open it, and drag ClipStack into `/Applications`.

Because the app is distributed ad-hoc-signed (not notarised by Apple), the
first launch is blocked by Gatekeeper — see
[Troubleshooting](#could-not-verify-clipstack-is-free-of-malware-that-may-harm-your-mac-or-compromise-your-privacy)
for the one-time **Open Anyway** flow.

Want to build from source instead? See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## How it works

ClipStack **never intercepts Cmd+C**. A plain copy stays a plain copy, and a
plain Cmd+V stays a plain native paste. A background thread polls
`NSPasteboard.general.changeCount` every 250 ms and records whatever appeared
on the pasteboard, which is the same approach Maccy and Clipy take.

When you pick an entry from the stack:

1. the payload is written back to the pasteboard (rich text, HTML, image
   flavours, or file objects, whichever it was copied as),
2. the app you were in before the picker opened is reactivated,
3. a Cmd+V keystroke is posted to that app's process.

| What you copy | What is stacked | What is pasted |
| --- | --- | --- |
| Text | plain text + HTML + RTF when present | the same flavours, so formatting survives |
| Image | PNG, GIF (bytes kept, animation intact), or TIFF converted to PNG | `public.png` + `public.tiff` |
| Files | the file URLs (lazy — contents are never copied) | real file objects, not their paths |

**How an entry is classified.** One copy can put several flavours on the
pasteboard at once, so ClipStack picks a single kind by precedence: **files →
image → text**. That is why:

- copying a file from Finder stacks it as **Files**, not text — Finder also puts
  the path on the pasteboard, and pasting the file beats pasting its path;
- copying a picture stacks it as **Image** even when a caption travels with it;
- copying rich text that has an image *inside* it stacks as **Text**, because
  the picture is part of the HTML/RTF markup rather than a standalone image
  flavour. The row is labelled `formatted + image` so this is visible, and
  pasting it restores the markup, image and all.

For a text entry that captured formatting, the picker offers two copy
commands: copy as it was copied (⌘C, keeps formatting) or copy as plain text
(⌘⇧C, strips formatting and any inline image).

Entries are deduplicated by SHA-256: copying the same thing again promotes the
existing entry to the top instead of adding a second copy. Image bytes are
stored on disk under `~/Library/Application Support/com.fradone.clipstack/blobs/`
keyed by their hash; everything else lives in a SQLite database next to it.

Since file entries keep only the paths, pasting one after the file has been
moved or deleted pastes a stale reference — the target app has nothing to
copy.

## Shortcuts

| Keys | What happens |
| --- | --- |
| Cmd+C | copy, as always. ClipStack just notices |
| Cmd+V | paste the newest item, natively |
| **Cmd+Shift+V** | open the stack, search, pick what to paste |
| ↑ ↓ / ↵ | move and paste |
| Cmd+1…9 | paste the matching row directly |
| Cmd+C | copy the selected item to the clipboard without pasting |
| Cmd+Shift+C | copy the selected text as plain text, dropping its formatting |
| Cmd+P | pin an entry so it cannot be evicted |
| Cmd+Backspace | delete an entry from the stack |
| Esc | dismiss the picker |

The picker shortcut is rebindable from Settings. It must include ⌘, and if
another app already owns the combination ClipStack reports it and keeps the
previous binding rather than leaving you without one.

## Granting Accessibility access

Paste injection needs Accessibility, and ClipStack asks on first launch. If
the prompt did not appear, open Settings and use **Grant access** or **Open
System Settings**, then switch ClipStack on in
*System Settings → Privacy & Security → Accessibility*.

**Without it the app is still useful**: picking an entry puts the payload on
your clipboard so you can press Cmd+V yourself. The picker tells you this
instead of failing silently.

## Run at login

Settings → General → **Open at login** installs a Launch Agent. The switch
reflects what macOS actually has registered, not what was requested.

## Staying up to date

You never need to re-download and reinstall ClipStack. Settings → **Updates**
checks GitHub Releases for a newer version; when one exists, **Install &
relaunch** downloads it, verifies its minisign signature against the key baked
into the app, installs it in place, and restarts. The stack, settings, and
Accessibility grant all survive the update.

## Settings

| Setting | Default | Notes |
| --- | --- | --- |
| Show the stack | Cmd+Shift+V | rebinding takes effect immediately |
| Keep up to | 200 items | lowering this trims the stack at once |
| Capture | text, images, files | each type can be turned off |
| Never capture from | none | app name or bundle id; use it for password managers |
| Pause capture | off | also in the menu bar menu |
| Keep out of screenshots | on | sets `NSWindowSharingNone` on the picker |
| Remember the stack after quitting | on | turn off to wipe the stack on quit |
| Open at login | off | Launch Agent |
| Updates | GitHub Releases | check, verify, install & relaunch in place |

Concealed pasteboard content is never recorded: anything advertising
`org.nspasteboard.ConcealedType`, `TransientType`, or `AutoGeneratedType` is
skipped, which is how password managers and apps marking content as transient
keep their copies out of the stack.

## Troubleshooting

### "could not verify “ClipStack” is free of malware that may harm your Mac or compromise your privacy"

macOS Gatekeeper blocks apps that are not notarised, which includes every
build of ClipStack since it is distributed ad-hoc-signed from GitHub Releases.
The app is safe — you are downloading it from this repository — but Gatekeeper
cannot verify that, so it refuses the first launch. To open it anyway:

1. Open the app normally (the error appears) and click **Cancel / Annulla**.
2. Open **System Settings**.
3. Go to **Privacy & Security** and scroll down to the **Security** section.
4. You will see a note that ClipStack was blocked. Click **Open Anyway**.
5. Enter your Mac password or use Touch ID to confirm.

The app launches and the block is remembered, so this is a one-time step per
downloaded build. (The equivalent from a terminal is
`xattr -dr com.apple.quarantine /Applications/ClipStack.app`, but the System
Settings route above is the intended one.)

### Paste stops working after an update or rebuild

macOS keys the Accessibility permission to the app's code signature, and
ad-hoc-signed builds get a new signature on every build. Remove ClipStack from
the Accessibility list and add it again. The status row in Settings shows the
real state, so you can confirm rather than guess.

### The picker does not appear

Check the menu bar icon: if capture is paused the picker still opens, but if
the shortcut was taken by another app ClipStack reports it in Settings →
Shortcut and keeps the previous binding. Rebind from there if needed.

## Known limits

- Paste injection posts a keystroke to a process. Apps that block synthetic
  events, or that are fullscreen on another Space, can swallow it. The payload
  is still on your clipboard in those cases.
- Copying the same item twice within one poll interval looks like a single copy.
- Very large images are stacked but not previewed in the picker; the preview is
  skipped above 4 MB rather than base64-ing a huge blob into the webview.
- The stack is not encrypted at rest. Use *Never capture from* for anything you
  would not want on disk, and turn off *Remember the stack after quitting* if
  you want it gone on quit.

## Requirements

macOS only. The paste-injection and pasteboard layers are AppKit and Core
Graphics, and there is no other platform where "paste into the app the user was
just in" means the same thing.

## Contributing

Want to hack on ClipStack? Start with
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the toolchain, repo layout, and
release process, and [CONTRIBUTING.md](CONTRIBUTING.md) for how to send a
change.
