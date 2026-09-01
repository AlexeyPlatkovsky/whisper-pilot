#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<EOF
Usage: $(basename "$0") <minor|major|release|sync|verify> [VERSION]

  minor    bump a fix version (1.9.0 -> 1.9.1)
  major    bump a feature version (1.9.0 -> 1.10.0)
  release  bump MAJOR (1.9.0 -> 2.0.0), or set an explicit later VERSION
  sync     make package.json and package-lock.json match Cargo/Tauri
  verify   fail unless every release-version source agrees

Cargo.toml is the canonical source. All modes validate the Tauri manifest,
package.json, and package-lock.json so release metadata cannot silently drift.
EOF
  exit 1
}

root="${VERSION_ROOT:-.}"
cargo_toml="$root/src-tauri/Cargo.toml"
tauri_conf="$root/src-tauri/tauri.conf.json"
package_json="$root/package.json"
package_lock="$root/package-lock.json"
mode="${1:-}"

[[ "$mode" =~ ^(minor|major|release|sync|verify)$ ]] || usage

for file in "$cargo_toml" "$tauri_conf" "$package_json" "$package_lock"; do
  [[ -f "$file" ]] || { echo "FATAL: missing release-version source: $file" >&2; exit 1; }
done

cargo_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$cargo_toml" | head -n 1)"
tauri_version="$(sed -n 's/^[[:space:]]*"version": "\([^"]*\)".*/\1/p' "$tauri_conf" | head -n 1)"
package_version="$(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).version ?? "")' "$package_json")"
lock_version="$(node -e 'const p=JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); console.log(p.version ?? "")' "$package_lock")"

is_semver() {
  [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

for version in "$cargo_version" "$tauri_version" "$package_version" "$lock_version"; do
  is_semver "$version" || { echo "FATAL: release version must be X.Y.Z, got: $version" >&2; exit 1; }
done

write_package_versions() {
  VERSION_VALUE="$1" PACKAGE_JSON="$package_json" PACKAGE_LOCK="$package_lock" node <<'NODE'
const fs = require("fs");
const version = process.env.VERSION_VALUE;
const packageJson = JSON.parse(fs.readFileSync(process.env.PACKAGE_JSON, "utf8"));
const packageLock = JSON.parse(fs.readFileSync(process.env.PACKAGE_LOCK, "utf8"));
if (!packageLock.packages || !packageLock.packages[""]) {
  throw new Error("package-lock.json has no root package record");
}
packageJson.version = version;
packageLock.version = version;
packageLock.packages[""].version = version;
fs.writeFileSync(process.env.PACKAGE_JSON, `${JSON.stringify(packageJson, null, 2)}\n`);
fs.writeFileSync(process.env.PACKAGE_LOCK, `${JSON.stringify(packageLock, null, 2)}\n`);
NODE
}

verify_sources() {
  if [[ "$cargo_version" != "$tauri_version" || "$cargo_version" != "$package_version" || "$cargo_version" != "$lock_version" ]]; then
    echo "FATAL: version sources disagree (Cargo=$cargo_version, Tauri=$tauri_version, package=$package_version, lock=$lock_version). Run scripts/bump-version.sh sync after confirming Cargo/Tauri are canonical." >&2
    exit 1
  fi
}

if [[ "$mode" == "sync" ]]; then
  if [[ "$cargo_version" != "$tauri_version" ]]; then
    echo "FATAL: Cargo ($cargo_version) and Tauri ($tauri_version) disagree; resolve the canonical app version before sync." >&2
    exit 1
  fi
  write_package_versions "$cargo_version"
  echo "synced package metadata to canonical version $cargo_version"
  exit 0
fi

verify_sources

if [[ "$mode" == "verify" ]]; then
  echo "version sources agree at $cargo_version"
  exit 0
fi

IFS='.' read -r current_major current_minor current_patch <<< "$cargo_version"
case "$mode" in
  minor)
    new_version="${current_major}.${current_minor}.$((current_patch + 1))"
    ;;
  major)
    new_version="${current_major}.$((current_minor + 1)).0"
    ;;
  release)
    new_version="${2:-$((current_major + 1)).0.0}"
    is_semver "$new_version" || { echo "FATAL: release version must be X.Y.Z, got: $new_version" >&2; exit 1; }
    ;;
esac

echo "$cargo_version -> $new_version"
if [[ "$(uname)" == "Darwin" ]]; then
  sed -i '' "s/^version = \"${cargo_version}\"/version = \"${new_version}\"/" "$cargo_toml"
  sed -i '' "s/\"version\": \"${tauri_version}\"/\"version\": \"${new_version}\"/" "$tauri_conf"
else
  sed -i "s/^version = \"${cargo_version}\"/version = \"${new_version}\"/" "$cargo_toml"
  sed -i "s/\"version\": \"${tauri_version}\"/\"version\": \"${new_version}\"/" "$tauri_conf"
fi
write_package_versions "$new_version"
echo "done — version bumped to $new_version in all release-version sources"
