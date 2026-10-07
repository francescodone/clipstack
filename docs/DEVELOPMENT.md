# ClipStack — Development Guide

Everything about building, running, and shipping ClipStack. For what the app
does and how to use it, see the [user README](../README.md).

## Toolchain

- macOS (the app is AppKit/Core Graphics only — no other platform)
- Rust (stable) + the `aarch64-apple-darwin` / `x86_64-apple-darwin` targets
- Node 20+ (the webviews are plain TypeScript, no framework)
- Python 3 + Pillow (only for the icon generator scripts)

Built with Tauri 2, `objc2` for AppKit, and `rusqlite` (bundled SQLite).

## Day-to-day commands

```sh
npm install
npm run tauri dev      # run from the source tree (hot reload on src/ and src-tauri/src/)
npm run tauri build    # release build -> src-tauri/target/release/bundle
npx tsc --noEmit       # type-check the webviews
```

The dev build is ad-hoc-signed, which means the code signature changes on
every rebuild. macOS keys Accessibility permission to that signature, so
**after a rebuild you may need to remove ClipStack from the Accessibility list
and add it again** — that is why paste can stop working after `npm run
tauri dev`. To sign with a real Developer ID certificate instead, set
`APPLE_SIGNING_IDENTITY` and the permission survives rebuilds.

## Repo layout

```
src/                 two webviews, plain TypeScript
  picker.ts          the Spotlight-style panel
  settings.ts        the settings form
  lib/ipc.ts         typed wrappers over the Rust commands
  styles/            one stylesheet per window
src-tauri/src/
  lib.rs             wiring: plugins, state, window and exit events
  commands.rs        the invoke surface
  poller.rs          the changeCount watcher
  store.rs           SQLite stack, dedupe, trimming, search
  picker.rs          panel lifecycle and the paste sequence
  windows.rs         settings window and Dock-icon promotion
  tray.rs            menu bar icon and menu
  shortcuts.rs       shortcut registration with rollback
  config.rs          settings.json
  types.rs           the shared data model
  macos/             pasteboard, paste injection, frontmost app,
                     accessibility, NSWindow panel configuration
scripts/             release + icon tooling (see below)
docs/                app icon master, screenshots
.github/workflows/   release.yml — CI build & publish on tag push
```

## Scripts

| Script | Purpose |
| --- | --- |
| `scripts/bump_version.sh <X.Y.Z>` | Bumps the version in all five declarations (package.json, package-lock.json, tauri.conf.json, Cargo.toml, Cargo.lock) and verifies the result. |
| `scripts/release.sh <vX.Y.Z>` | Builds with the updater key, creates the GitHub release (`gh`) with DMG + `ClipStack.app.tar.gz` + `.sig` + generated `latest.json`. Requires the tag to match `package.json` — run `bump_version.sh` first. |
| `scripts/generate_app_icon.py` | Regenerates the 1024px app icon master (`docs/app-icon.png`). |
| `scripts/generate_tray_icon.py` | Regenerates the menu bar template icons (`tray.png`, `tray@2x.png`). |

After changing the app icon master, run `npx tauri icon docs/app-icon.png` to
regenerate all bundle formats.

## The updater key

Release builds must be signed with the project's minisign keypair so the
in-app updater can verify what it downloads:

- Private key: `~/.tauri/clipstack.key` (no password). **Never commit it.**
  If it is lost, existing installs can never receive updates again.
- Public key: baked into `src-tauri/tauri.conf.json` under
  `plugins.updater.pubkey`. The endpoint is the GitHub Releases
  `latest.json` of this repository.
- CI reads the same key from the `TAURI_SIGNING_PRIVATE_KEY` repository
  secret (Settings → Secrets and variables → Actions). Locally,
  `scripts/release.sh` picks it up from `~/.tauri/clipstack.key`
  automatically, or from the `TAURI_SIGNING_PRIVATE_KEY` env var.

Because a pubkey is configured with `createUpdaterArtifacts: true`, **every**
`tauri build` must have the private key available — otherwise the build fails
at the signing step after bundling.

## Cutting a release

```sh
./scripts/bump_version.sh 0.3.0     # update all version declarations
git commit -am "chore: release v0.3.0" && git push
./scripts/release.sh v0.3.0         # build + sign + gh release + latest.json
```

Or let CI do it: push the tag and `.github/workflows/release.yml` builds both
Apple-silicon and Intel targets via `tauri-apps/tauri-action` and creates a
**draft** release — publish it from the Releases page once both jobs are
green.

```sh
git tag v0.3.0
git push origin v0.3.0
```

A release must ship `ClipStack.app.tar.gz` plus its `.sig` and a `latest.json`
manifest describing them; the DMG is what new users download. `tauri-action`
generates the manifest automatically when the signing key is provided.

Note: the workflow file used by CI is the one from the *tagged commit* — if a
release fails due to workflow changes, re-point the tag (`git tag -f` +
force-push) rather than re-running the old job.

## CI

`.github/workflows/release.yml` triggers on `v*` tags (and manually via
workflow dispatch). It runs a two-target matrix on `macos-latest`, needs the
`TAURI_SIGNING_PRIVATE_KEY` secret, and uploads all artifacts to the draft
release. The DMG step (`bundle_dmg.sh`) drives Finder via AppleScript and can
flake locally without Automation permission — the `.app`/tarball/sig always
build.
