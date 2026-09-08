#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
"$repo_root/scripts/verify-package-contract.sh"
"$repo_root/TeraFFI/scripts/verify-installed-artifacts.sh"
"$repo_root/scripts/release-evidence.sh" check

echo "local unsigned evidence matches the owned producer; remote release qualification is separate"
