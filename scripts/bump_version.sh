#!/usr/bin/env bash
# Bump the ClipStack version everywhere it is declared.
#
#   ./scripts/bump_version.sh 0.3.0
#   ./scripts/bump_version.sh v0.3.0        # leading "v" is stripped
#
# Tauri keeps the version in five places that have to agree, or the built app
# reports one version in the About panel and another in the updater metadata:
#
#   package.json             npm manifest
#   package-lock.json        root + packages[""] entries
#   src-tauri/tauri.conf.json  what Tauri bakes into the bundle
#   src-tauri/Cargo.toml       the Rust crate
#   src-tauri/Cargo.lock       derived; refreshed with cargo when available
#
# Edits are anchored on the surrounding keys rather than the version string, so
# dependencies that happen to share the old version are never touched.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
  echo "usage: $(basename "${BASH_SOURCE[0]}") <version>   e.g. 0.3.0" >&2
  exit 1
}

[[ $# -eq 1 ]] || usage
NEW="${1#v}"

if [[ ! "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "error: '$NEW' is not a semver version (major.minor.patch[-prerelease])" >&2
  exit 1
fi

cd "$ROOT"

# The old version is read from package.json, the single source we trust; every
# file that still carries it gets rewritten to the new one.
OLD="$(node -p "require('./package.json').version")"

if [[ "$OLD" == "$NEW" ]]; then
  echo "already at v$NEW — nothing to do"
  exit 0
fi

# --- npm -------------------------------------------------------------------
# package.json has exactly one version field, at the top level.
perl -0pi -e "s/\"version\": \"\Q$OLD\E\"/\"version\": \"$NEW\"/" package.json

# In the lockfile the version appears for the root package and again under
# packages[""]; both are the fields right after "name": "clipstack".
perl -0pi -e "s/(\"name\": \"clipstack\",\s*\n\s*\"version\": \")\Q$OLD\E\"/\${1}$NEW\"/g" \
  package-lock.json

# --- Tauri / Rust ----------------------------------------------------------
perl -0pi -e "s/\"version\": \"\Q$OLD\E\"/\"version\": \"$NEW\"/" \
  src-tauri/tauri.conf.json

perl -0pi -e "s/^version = \"\Q$OLD\E\"/version = \"$NEW\"/m" src-tauri/Cargo.toml

# Cargo.lock is generated, so let cargo regenerate the entry. Fall back to the
# same anchored edit when cargo is not on PATH.
if command -v cargo >/dev/null 2>&1 && \
   (cd src-tauri && cargo update -p clipstack --precise "$NEW" >/dev/null 2>&1); then
  LOCK_VIA="cargo"
else
  perl -0pi -e "s/(name = \"clipstack\"\nversion = \")\Q$OLD\E\"/\${1}\"$NEW\"/" \
    src-tauri/Cargo.lock
  LOCK_VIA="sed fallback"
fi

# --- verify ----------------------------------------------------------------
# Each check reads the actual field rather than grepping for the string, since
# unrelated dependencies in the lockfiles routinely share a version number.
fail=0

expect() { # label actual
  if [[ "$2" != "$NEW" ]]; then
    echo "  ! $label is '$2', expected '$NEW'" >&2
    fail=1
  fi
}

expect "package.json" "$(node -p "require('./package.json').version")"
expect "package-lock.json (root)" \
  "$(node -p "require('./package-lock.json').version")"
expect "package-lock.json (packages)" \
  "$(node -p "require('./package-lock.json').packages[''].version")"
expect "tauri.conf.json" \
  "$(node -p "require('./src-tauri/tauri.conf.json').version")"
expect "Cargo.toml" \
  "$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -1)"
expect "Cargo.lock" \
  "$(grep -A1 '^name = "clipstack"$' src-tauri/Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p')"

if [[ "$fail" == 0 ]] && grep -rqF -- "\"version\": \"$OLD\"" package.json \
    src-tauri/tauri.conf.json 2>/dev/null; then
  echo "  ! the old version $OLD is still present in a manifest" >&2
  fail=1
fi

(( fail == 0 )) || { echo "bump incomplete" >&2; exit 1; }

echo "v$OLD -> v$NEW"
echo "  package.json            $NEW"
echo "  package-lock.json       $NEW"
echo "  tauri.conf.json         $NEW"
echo "  Cargo.toml              $NEW"
echo "  Cargo.lock              $NEW ($LOCK_VIA)"
echo
echo "next: git add -A && git commit -m 'chore: release v$NEW' && git tag v$NEW"
