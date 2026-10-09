#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

readonly -a SOURCE_PATHS=(
  Package.swift
  Tera
  TeraPublicAPITests
  TeraTests
  TeraUITests
  scripts/legacy_fixture_writers
)
readonly -a MAINTAINABILITY_RULES=(
  cyclomatic_complexity
  file_length
  function_body_length
  function_parameter_count
  large_tuple
  type_body_length
)

command -v swiftformat >/dev/null || {
  echo "swift-quality: swiftformat is unavailable" >&2
  exit 1
}
command -v swiftlint >/dev/null || {
  echo "swift-quality: swiftlint is unavailable" >&2
  exit 1
}

swiftformat --lint --strict --config .swiftformat "${SOURCE_PATHS[@]}"
swiftlint lint \
  --strict \
  --quiet \
  --no-cache \
  --silence-deprecation-warnings \
  --config .swiftlint.yml \
  "${SOURCE_PATHS[@]}"

metric_arguments=()
for rule in "${MAINTAINABILITY_RULES[@]}"; do
  metric_arguments+=(--only-rule "$rule")
done
swiftlint lint \
  --strict \
  --quiet \
  --no-cache \
  --silence-deprecation-warnings \
  --config .swiftlint.yml \
  --baseline test-fixtures/swiftlint-maintainability-baseline.v1.json \
  "${metric_arguments[@]}" \
  "${SOURCE_PATHS[@]}"

uv run --offline --frozen --project scripts/persona-verifier python scripts/authored_source_inventory.py format
uv run --offline --frozen --project scripts/persona-verifier python scripts/authored_source_inventory.py lint
uv run --offline --project scripts/persona-verifier python scripts/maintainability_ratchet.py verify
uv run --offline --frozen --project scripts/persona-verifier python scripts/authored_source_inventory.py test
