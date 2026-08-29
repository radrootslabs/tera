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
