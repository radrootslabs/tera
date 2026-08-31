#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)

exec uv run \
    --project "$repo_root/scripts/persona-verifier" \
    --offline \
    --frozen \
    python "$repo_root/scripts/local-social-fixture.py" "$@"
