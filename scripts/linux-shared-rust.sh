#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${EXT_BUILD_RUN_ACTIVE:-}" || -z "${EXT_BUILD_PROJECT_DIR:-}" ]]; then
  echo "linux-shared-rust: run through cargo extbuild run --" >&2
  exit 1
fi

readonly SOURCE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly RUNNER_IMAGE="docker.io/library/rust:1.97.1-slim-trixie@sha256:fc0648ac2962539be80bd424729a20fd80f7b64bfba7e90bbd642aed6c697c5a"
readonly RUNNER_ROOT="$EXT_BUILD_PROJECT_DIR/linux-x86_64"
readonly RUNNER_CARGO_HOME="$RUNNER_ROOT/cargo-home"
readonly RUNNER_TARGET="$RUNNER_ROOT/target"

command -v docker >/dev/null || {
  echo "linux-shared-rust: docker is unavailable" >&2
  exit 1
}
mkdir -p "$RUNNER_CARGO_HOME" "$RUNNER_TARGET"

docker run --rm --init --platform linux/amd64 \
  --user "$(id -u):$(id -g)" \
  --volume "$SOURCE_ROOT:/workspace:ro" \
  --volume "$RUNNER_CARGO_HOME:/cargo-home" \
  --volume "$RUNNER_TARGET:/target" \
  --workdir /workspace \
  --env CARGO_HOME=/cargo-home \
  --env CARGO_TARGET_DIR=/target \
  "$RUNNER_IMAGE" \
  /bin/bash -c '
    set -euo pipefail
    [[ "$(uname -s)" == "Linux" ]]
    [[ "$(uname -m)" == "x86_64" ]]
    cargo check --workspace --all-targets --locked
    cargo test --workspace --all-targets --locked
  '
