#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

readonly -a SOURCE_PATHS=(
  Package.swift
  Radroots
  RadrootsPublicAPITests
  RadrootsTests
  RadrootsUITests
)
readonly -a MAINTAINABILITY_RULES=(
  cyclomatic_complexity
  file_length
  function_body_length
  function_parameter_count
  large_tuple
  type_body_length
)
readonly -a PYTHON_QUALITY_PATHS=(
  scripts/maintainability_ratchet.py
  scripts/package_contract.py
  scripts/test_maintainability_ratchet.py
  scripts/test_package_contract.py
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

uv run --offline --project scripts/persona-verifier ruff format --check "${PYTHON_QUALITY_PATHS[@]}"
uv run --offline --project scripts/persona-verifier ruff check "${PYTHON_QUALITY_PATHS[@]}"
uv run --offline --project scripts/persona-verifier python scripts/maintainability_ratchet.py verify
