#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $(basename "$0") <minor|major|release> <git commit arguments...>" >&2
  exit 1
}

mode="${1:-}"
shift || true
[[ "$mode" =~ ^(minor|major|release)$ ]] && [[ "$#" -gt 0 ]] || usage

root="$(git rev-parse --show-toplevel)"
if [[ "$mode" == "release" && "${WHISPERPILOT_RELEASE_AUTHORIZED:-}" != "1" ]]; then
  echo "FATAL: release requires explicit user authorization in WHISPERPILOT_RELEASE_AUTHORIZED=1." >&2
  exit 1
fi

"$root/scripts/bump-version.sh" verify
head_version="$(git -C "$root" show HEAD:src-tauri/Cargo.toml | sed -n 's/^version = "\(.*\)"/\1/p' | head -n 1)"
current_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/src-tauri/Cargo.toml" | head -n 1)"
amend=false
for argument in "$@"; do
  [[ "$argument" == "--amend" ]] && amend=true
done

if $amend; then
  [[ "$current_version" == "$head_version" ]] || {
    echo "FATAL: amend must reuse HEAD version $head_version, got $current_version." >&2
    exit 1
  }
  export WHISPERPILOT_VERSION_AMEND=1
else
  IFS='.' read -r head_major head_minor head_patch <<<"$head_version"
  case "$mode" in
    minor) expected_version="$head_major.$head_minor.$((head_patch + 1))" ;;
    major) expected_version="$head_major.$((head_minor + 1)).0" ;;
    release) expected_version="$((head_major + 1)).0.0" ;;
  esac
  if [[ "$current_version" == "$head_version" ]]; then
    "$root/scripts/bump-version.sh" "$mode"
  elif [[ "$current_version" == "$expected_version" ]]; then
    echo "reusing existing $head_version -> $current_version version bump"
  else
    echo "FATAL: $mode expects $expected_version from HEAD $head_version, got $current_version." >&2
    exit 1
  fi
fi

git -C "$root" add README.md package.json package-lock.json src-tauri/Cargo.lock \
  src-tauri/Cargo.toml src-tauri/tauri.conf.json
exec git -C "$root" commit "$@"
