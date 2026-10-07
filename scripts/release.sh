#!/usr/bin/env bash
# Build, sign, and publish a ClipStack release with updater metadata.
#
#   ./scripts/release.sh v0.3.0
#
# Prerequisites:
#   - The updater private key at ~/.tauri/clipstack.key (see README).
#   - The `gh` CLI authenticated for the repository.
#   - The version already bumped: ./scripts/bump_version.sh 0.3.0
#
# What it does:
#   1. `npm run tauri build` with the updater key in the environment, which
#      produces ClipStack.app.tar.gz + .sig + .dmg in
#      src-tauri/target/release/bundle.
#   2. Creates the GitHub release with the .dmg (what a new user downloads)
#      and the .app.tar.gz + .sig (what the in-app updater consumes).
#   3. Uploads a generated latest.json manifest pointing at those assets.
#      The in-app updater fetches
#      .../releases/latest/download/latest.json and compares versions.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

[[ $# -eq 1 ]] || { echo "usage: $(basename "$0") <vX.Y.Z>" >&2; exit 1; }
TAG="${1#v}"
[[ "$TAG" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || {
  echo "error: '$TAG' is not a version" >&2; exit 1;
}
TAG="v$TAG"

CURRENT="$(node -p "require('./package.json').version")"
[[ "$TAG" == "v$CURRENT" ]] || {
  echo "error: tag $TAG does not match package.json ($CURRENT). Run scripts/bump_version.sh first." >&2
  exit 1
}

KEY="${TAURI_SIGNING_PRIVATE_KEY:-$(cat "$HOME/.tauri/clipstack.key" 2>/dev/null || true)}"
[[ -n "$KEY" ]] || { echo "error: no updater key (TAURI_SIGNING_PRIVATE_KEY or ~/.tauri/clipstack.key)" >&2; exit 1; }

command -v gh >/dev/null || { echo "error: the gh CLI is required" >&2; exit 1; }

echo "==> building $TAG"
TAURI_SIGNING_PRIVATE_KEY="$KEY" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" \
  npm run tauri build

BUNDLE="src-tauri/target/release/bundle"
TARBALL="$BUNDLE/macos/ClipStack.app.tar.gz"
SIG="$TARBALL.sig"
DMG=$(ls "$BUNDLE"/dmg/ClipStack_*.dmg | head -1)
[[ -f "$TARBALL" && -f "$SIG" ]] || { echo "error: updater artifacts missing" >&2; exit 1; }

echo "==> writing latest.json"
SIG_CONTENT="$(cat "$SIG")"
MANIFEST="$(mktemp)"
cat > "$MANIFEST" <<JSON
{
  "version": "$CURRENT",
  "notes": "ClipStack $TAG",
  "pub_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "platforms": {
    "darwin-aarch64": {
      "signature": "$SIG_CONTENT",
      "url": "https://github.com/francescodone/clipstack/releases/download/$TAG/ClipStack.app.tar.gz"
    }
  }
}
JSON

echo "==> creating release $TAG"
gh release create "$TAG" "$DMG" "$TARBALL" "$SIG" "$MANIFEST#latest.json" \
  --title "ClipStack $TAG" --generate-notes

rm -f "$MANIFEST"
echo
echo "done: https://github.com/francescodone/clipstack/releases/tag/$TAG"
echo "existing installs will pick it up from Settings → Updates."
