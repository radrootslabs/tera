#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
source_root="$repo_root/RadrootsFFI/.radroots/source/lib"
expected_revision=$(awk '
    $2 == "RADROOTS_FIELD_LIB_GIT_REV" && $3 == ":=" { print $4 }
' "$repo_root/RadrootsFFI/source.lock")

"$repo_root/scripts/verify-package-contract.sh"
"$repo_root/RadrootsFFI/scripts/verify-installed-artifacts.sh"
"$repo_root/scripts/release-evidence.sh" check

if [ ! -d "$source_root/.git" ]; then
    echo "error: pinned Lib source is absent; run make bootstrap" >&2
    exit 1
fi
if [ "$(git -C "$source_root" rev-parse HEAD)" != "$expected_revision" ]; then
    echo "error: pinned Lib source revision does not match the FFI lock" >&2
    exit 1
fi
git -C "$source_root" diff --quiet --no-ext-diff
git -C "$source_root" diff --cached --quiet --no-ext-diff

cargo run --manifest-path "$source_root/Cargo.toml" -p xtask --locked -- \
    source-lock --consumer-root "$repo_root"

echo "unsigned release preflight succeeded at Lib revision $expected_revision"
