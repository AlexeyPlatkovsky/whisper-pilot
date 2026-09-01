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
"$root/scripts/bump-version.sh" "$mode"
git -C "$root" add package.json package-lock.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
exec git -C "$root" commit "$@"
