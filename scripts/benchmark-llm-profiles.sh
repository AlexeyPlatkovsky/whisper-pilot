#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 /path/to/Qwen3.5-4B-Q4_K_M.gguf /path/to/Qwen3.8-9B-Q6_K.gguf /path/to/gemma-4-12b-it-qat-q4_0.gguf" >&2
  exit 2
fi

project_dir=$(cd "$(dirname "$0")/.." && pwd)
model_names=(fast qwen38 gemma)
model_files=("$1" "$2" "$3")

for model_index in 0 1 2; do
  model_name=${model_names[$model_index]}
  model_file=${model_files[$model_index]}
  if [[ ! -f "$model_file" ]]; then
    echo "$model_name model not found: $model_file" >&2
    exit 2
  fi
  echo "== $model_name =="
  shasum -a 256 "$model_file"
  WHISPERPILOT_TEST_LLM_MODEL="$model_file" cargo test \
    --manifest-path "$project_dir/src-tauri/Cargo.toml" \
    --test llm_runtime_contract \
    pinned_model_passes_multilingual_translation_polish_and_mfu_smoke \
    -- --ignored --nocapture
done
