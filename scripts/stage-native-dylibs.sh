#!/bin/sh
set -eu

if [ "${TAURI_ENV_PLATFORM:-}" != "darwin" ]; then
  exit 0
fi

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
manifest="$project_dir/src-tauri/Cargo.toml"
target_dir=$(cargo metadata --manifest-path "$manifest" --format-version 1 --no-deps \
  | node -e 'let data=""; process.stdin.on("data", chunk => data += chunk); process.stdin.on("end", () => process.stdout.write(JSON.parse(data).target_directory));')

profile=release
if [ "${TAURI_ENV_DEBUG:-false}" = "true" ]; then
  profile=debug
fi

source_dir="$target_dir/$profile"
staging_dir="$project_dir/src-tauri/frameworks"
mkdir -p "$staging_dir"

for name in \
  libsherpa-onnx-c-api.dylib \
  libonnxruntime.1.17.1.dylib \
  libggml-base.0.dylib \
  libggml-cpu.0.dylib \
  libggml-metal.0.dylib \
  libggml.0.dylib \
  libllama-common.0.dylib \
  libllama.0.dylib
do
  source_file="$source_dir/$name"
  if [ ! -f "$source_file" ]; then
    echo "$source_file not found — a native dependency did not produce it." >&2
    exit 1
  fi
  cp "$source_file" "$staging_dir/$name"
done
