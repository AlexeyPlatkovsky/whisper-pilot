#!/usr/bin/env bash
set -euo pipefail

git rev-parse --git-dir >/dev/null 2>&1 || exit 0
git config core.hooksPath .githooks
echo "Git hooks enabled from .githooks"
