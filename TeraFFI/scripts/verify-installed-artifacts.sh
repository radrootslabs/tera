#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
export PYTHONDONTWRITEBYTECODE=1
exec uv run --offline --frozen --no-sync --project "$repo_root/scripts/persona-verifier" \
    python "$repo_root/scripts/ffi_installed.py"
