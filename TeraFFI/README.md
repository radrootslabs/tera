# TeraFFI

The native artifact boundary builds `tera_ffi` and runs `tera_bindgen` from the
single Tera Rust workspace. Shared foundation packages remain exact Git inputs
in `radroots.lib.source-lock.v1.toml`; no separate Lib checkout is a build input.

Run `make ffi-bootstrap` from the repository root to build, verify and install
the owned candidate. Stage declared producer inputs first. Rust 1.97.1 and its
`llvm-tools` component, Xcode, SwiftFormat and the locked Python verifier are
required. The producer contract declares device, simulator and host targets,
features, release profile, deployment target and path-remapping flags.

`make ffi-candidate-build ffi-candidate-check` builds and verifies only the
external candidate. `make artifact-check` verifies installed source records and
all installed artifact hashes without building, generating or fetching inputs.
Xcode runs the same read-only preflight. Missing, stale, mixed or unproven
installations fail closed.

The generated `source.lock` and `provenance.json` distinguish the Tera input tree
from the exact foundation revision. Installed bindings, framework and API
snapshot derive from the same candidate. This local evidence does not qualify
a release or establish remote reachability.
