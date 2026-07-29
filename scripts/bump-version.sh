#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<EOF
Usage: $(basename "$0") <patch|major|release [VERSION]>

  patch   — bump PATCH  (0.1.0 → 0.1.1) for bug fixes
  major   — bump MINOR  (0.1.0 → 0.2.0) for new features/tasks/epics
  release — bump MAJOR to 1.0.0, or to the explicit VERSION if given

The version is updated in both src-tauri/Cargo.toml and src-tauri/tauri.conf.json.
EOF
  exit 1
}

CARGO_TOML="src-tauri/Cargo.toml"
TAURI_CONF="src-tauri/tauri.conf.json"

mode="${1:-}"
[[ "$mode" =~ ^(patch|major|release)$ ]] || usage

# ----- read current version from Cargo.toml -----
current="$(sed -n 's/^version = "\(.*\)"/\1/p' "$CARGO_TOML")"
[[ -n "$current" ]] || { echo "FATAL: cannot parse version from $CARGO_TOML"; exit 1; }

IFS='.' read -r major minor patch <<< "$current"

case "$mode" in
  patch)
    new_major="$major"
    new_minor="$minor"
    new_patch=$((patch + 1))
    ;;
  major)
    new_major="$major"
    new_minor=$((minor + 1))
    new_patch=0
    ;;
  release)
    if [[ -n "${2:-}" ]]; then
      # validate explicit version
      echo "$2" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$' || {
        echo "FATAL: release version must be semver (X.Y.Z), got: $2"; exit 1
      }
      new_version="$2"
    else
      new_version="1.0.0"
    fi
    ;;
esac

if [[ "$mode" != "release" ]]; then
  new_version="${new_major}.${new_minor}.${new_patch}"
fi

echo "$current -> $new_version"

# ----- update Cargo.toml -----
if [[ "$(uname)" == "Darwin" ]]; then
  sed -i '' "s/^version = \"${current}\"/version = \"${new_version}\"/" "$CARGO_TOML"
else
  sed -i "s/^version = \"${current}\"/version = \"${new_version}\"/" "$CARGO_TOML"
fi

# ----- update tauri.conf.json -----
if [[ "$(uname)" == "Darwin" ]]; then
  sed -i '' "s/\"version\": \"${current}\"/\"version\": \"${new_version}\"/" "$TAURI_CONF"
else
  sed -i "s/\"version\": \"${current}\"/\"version\": \"${new_version}\"/" "$TAURI_CONF"
fi

echo "done — version bumped to $new_version in $CARGO_TOML and $TAURI_CONF"
