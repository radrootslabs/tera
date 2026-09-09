#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
exec uv run --offline --frozen --project "$repo_root/scripts/persona-verifier" \
    python "$repo_root/scripts/kotlin_smoke.py" "$@"
