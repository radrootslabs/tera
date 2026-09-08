# Tera iOS App

Tera is a public iOS 18 app for discovering and publishing local farm
updates, asks, in-person events, and food listings over Nostr. The product has
exactly two bottom tabs: Today for discovery and Add for authored operations.

The current public release is `0.1.0-alpha`.

Tera owns the application's Rust validation, runtime transitions, durable
authored operations and application FFI as well as the native iOS host. Shared
domain types, signing protocols, transport and storage mechanics remain in the
public foundation packages. The target behavior keeps cached Today and local
Add usable independently of network readiness; the native host presents the Rust contracts and supplies
Apple platform capabilities.

Application Rust packages belong under `core/crates` in the single root Cargo
workspace. Installed native artifacts use the owned `tera_ffi` and matching
`tera_bindgen` packages, with shared packages at their exact foundation pins.
The five creation families, Today/Add tabs, installed identity and persisted
operation formats remain compatible with the original app.

`TeraFFI/producer.toml` separately governs the owned Tera FFI producer.
After staging its source inputs, `make ffi-source-write ffi-source-check`
captures and checks the exact source tree, foundation lock, target, features and
toolchains under extbuild output. Select a supported target with `FFI_TARGET`.
This is local source evidence. `make ffi-bootstrap` installs the matching
native artifacts and records their exact source tree, foundation and hashes.

`make ffi-candidate-build ffi-candidate-check` builds the owned device,
simulator and host libraries, generates matching Swift and API outputs, and
verifies the staged XCFramework and provenance. Candidates remain under
extbuild output; this command does not install them into the native app.

## Requirements

- macOS with Xcode and an iOS 18-or-newer simulator
- XcodeGen
- Rust `1.97.1-aarch64-apple-darwin` with `llvm-tools` and the iOS device and simulator targets
- SwiftFormat and the locked Python verifier installed by bootstrap
- `cargo-extbuild` configured for the checkout

Physical-device development additionally requires one exact paired, connected,
unlocked iPhone with Developer Mode enabled, an Apple development team, and a
generated xcconfig below the extbuild-owned DerivedData root. The governed
parent workspace supplies those machine inputs. The standalone script refuses
name-only destinations, unsigned builds, non-Debug physical builds, and
xcconfig files outside the managed output root:

```sh
TERA_IOS_PHYSICAL_AUTOMATION=1 \
TERA_IOS_DEVELOPMENT_TEAM=ABCDEFGHIJ \
cargo extbuild run -- scripts/xcode.sh physical-app-build \
  id=00000000-0000000000000000 \
  "$XCODE_DERIVED_DATA/radroots-ios-device/config/device.xcconfig"
```

The values above are placeholders. Device identities, teams, endpoints, and
certificate material are never checked into this public repository.

## Bootstrap and verify

The first bootstrap requires network access. It resolves exact foundation
dependencies, builds the owned UniFFI XCFramework and Swift
bindings, resolves exact Swift package revisions, and generates the Xcode
project:

```sh
cargo extbuild doctor
make bootstrap
```

After bootstrap, the complete package and Xcode build/test lane uses the
resolved revisions without automatic dependency updates:

```sh
make verify
```

The complete lane includes the repository-owned Swift formatting and lint
policy and the pinned Linux x86_64 shared-Rust runner. Run those focused checks
independently with `make swift-quality` and `make linux-shared-rust`; both keep
their build output under extbuild.

`make swift-quality` also applies the exact checked SwiftLint complexity
baseline and the repository-owned Swift/Python maintainability ratchet. New
Swift files are capped at 600 physical lines, new Python files at 800, and new
Python functions at complexity 10. Existing larger files and functions are a
closed, non-growing inventory; the newly separated user-message classifier
and package verification modules must remain below the new-file limits. Run
`make maintainability-check` for the narrow size and Python-complexity gate.

Use `SIMULATOR_NAME="Device Name" make verify` when the default simulator is
not installed. `make clean` removes the external native candidate cache while
preserving Cargo incremental output, the current installation, source and locks.

The focused local-social scenario starts bounded loopback Nostr and Blossom
fixtures, then exercises the real app stores and installed Rust FFI on one
exact simulator. The test saves an unverified photo draft, relaunches, retries
from the durable draft against the enabled fixture, publishes all five product
types, and proves Today and the outbox survive another relaunch:

```sh
TERA_IOS_UI_TEST_RUN_ID=local-social-example \
cargo extbuild run -- scripts/xcode.sh local-social-ui-test \
  'platform=iOS Simulator,id=SIMULATOR-UDID' \
  local-social-example
```

This harness is Debug-only, binds only explicit loopback ports, records bounded
protocol evidence under the extbuild results root, and cannot become a
production endpoint fallback. Its typed isolated-loopback mode is the only
mode permitted to use automated user presence or test secret policy. Public
and physical qualification retain normal Apple user presence, remain optional,
and are not claimed by the deterministic simulator lane.

The app and XCUITest runner admit the simulator endpoints through closed typed
policies. The fixture creates both servers through one observable loopback
connection factory, rejects and counts every non-loopback peer, and records
accepted and rejected socket counters in its bounded evidence snapshot.

The loopback Blossom fixture admits uploads only with the exact signed BUD-11
HTTP authorization produced by the installed Rust runtime: kind `24242`,
bounded non-empty human content, one upload action, one exact SHA-256, one
lowercase domain-only server scope, and one canonical expiration whose
lifetime is at most 300 seconds. Event ID and signature verification precede
admission, and the relay explicitly rejects authorization events. The shared
mutation corpus contains only field-mutation instructions; it persists no
private material or signed authorization event.

Passing `accessibility` as the final launcher argument runs Apple's
accessibility audit over every progressively disclosed Add composition at the
largest accessibility text size with Reduce Motion enabled. The corresponding
fixture verifier requires zero publication and upload effects:

```sh
TERA_IOS_UI_TEST_RUN_ID=local-social-accessibility \
cargo extbuild run -- scripts/xcode.sh local-social-ui-test \
  'platform=iOS Simulator,id=SIMULATOR-UDID' \
  local-social-accessibility \
  accessibility
```

Element-bound clipping findings remain fatal. The harness tolerates only
Xcode's elementless clipping diagnostics and narrowly identified contrast
false positives for disabled controls, system-chrome overlap, and the
black-on-white Submit button.

Passing `persona` runs the strict five-persona, 15-attempt deterministic
local-social matrix serially. Each persona receives a fresh run-scoped native
identity and isolated durable store while one bounded loopback Nostr relay and
Blossom service record exact event, media, retry, and subscription evidence:

```sh
TERA_IOS_UI_TEST_RUN_ID=local-social-persona-run-001 \
cargo extbuild run -- scripts/xcode.sh local-social-ui-test \
  'platform=iOS Simulator,id=SIMULATOR-UDID' \
  local-social-persona-run-001 \
  persona
```

The persona fixture and historical v1 result remain strict, deny-unknown
contracts. Each completed attempt now also emits one bounded canonical
`persona-attempt-evidence.v1` xcresult attachment bound to the exact XCUITest
target and identifier, test action and configuration, source commit and tree,
app-build digest, simulator, run, persona, attempt, endpoint policy, and visible
UI outcome. The attachment never retains a raw secret, signed authorization
event, or raw event content. Validation, retry, relaunch, retention, Today,
connection, subscription, event, upload, and retrieval fields come from the
executed UI path and monotonic fixture-snapshot deltas. The v2 result is
reconstructed only from the exact 15 measured attachments and is cross-checked
against the final fixture totals; a non-loopback attempt fails the run.

The standalone fixture and result verifier runs under the exact Python 3.14.7
and `jsonschema` 4.26.0 environment locked in
`scripts/persona-verifier/uv.lock`. `make bootstrap` installs that exact lock
through extbuild once; qualification and package checks then use only
`--offline --frozen` resolution. Every JSON input is read with a
maximum-plus-one bound before decoding, schema files pass Draft 2020-12
meta-validation, semantic fixtures must agree with their schemas, exported
attachment inventory is exact, and result-bundle hashing uses a
domain-separated length-framed preimage with bounded paths, entries, files,
and aggregate bytes.

The test uses ordinary visible controls, native-generated signing identities,
real app stores, and generated Rust FFI. This is deterministic non-human
conformance evidence. It does not claim human usability, demographic or
population validity, observed VoiceOver-user experience, release readiness,
or production qualification.

## Package surface

`Package.swift` publishes the `TeraApp` library used by the generated Xcode
application wrapper. It pins AppleKit by exact HTTPS Git revision and consumes
the locally bootstrapped `TeraFFI.xcframework`. Ordinary Xcode compilation
never writes repository source: a read-only preflight rejects missing or stale
FFI artifacts and directs the developer to run `make bootstrap`.

The Rust source lock, generated bindings, XCFramework hashes, provenance, Swift
package locks, privacy manifests, and public API snapshots are checked as part
of the release lane.

`make package-contract-check` evaluates the Swift package manifest and parses
the TOML, plist, JSON, xcconfig, project-package, and lock inputs as structured,
bounded data. It also runs the locked fixture and verifier unit suites.
The check evaluates Cargo's workspace graph and rejects members or local
dependencies outside this standalone repository, including implicit sibling
checkouts. The default Rust lane selects `tera_core`, `tera_ffi`, and
`tera_bindgen`; `tera_wasm` remains non-default. The resolved graph must use
the exact shared foundation lock, activate the FFI mobile-social profile, and
contain no retired source-lock shim or old mobile producer. Human specifications
and execution evidence remain outside the capsule and are never required by
these checks.
Comments, examples, unreachable source, and arbitrary matching text cannot
satisfy a behavior-bearing package assertion; application behavior is proven
by the compiled Swift and simulator test lanes.
The contract also binds the exact Ruff development tool, both maintainability
baselines, and their executable verifier, so local lint behavior cannot drift
with an ambient Python installation.

The unsigned release-evidence lane also regenerates a deterministic CycloneDX
SBOM from the locked Rust and Swift dependency graphs and binds it to the
checked-in locks, API snapshots, privacy inputs, Xcode project, XCFramework
provenance, and exact public repository identity:

```sh
make release-evidence-write
make release-preflight
```

Run the write target only when an owned release input changes. The preflight
is read-only and rejects stale generated evidence. Signing, tagging,
publication, and deployment remain separate operations.
