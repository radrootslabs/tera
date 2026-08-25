# Radroots iOS App

Radroots is a public iOS 18 app for discovering and publishing local farm
updates, asks, in-person events, and food listings over Nostr. The product has
exactly two bottom tabs: Today for discovery and Add for authored operations.

The current public release is `0.1.0-alpha`.

## Requirements

- macOS with Xcode and an iOS 18-or-newer simulator
- XcodeGen
- Rust `1.97.1-aarch64-apple-darwin` with the iOS device and simulator targets
- `cargo-extbuild` configured for the checkout

Physical-device development additionally requires one exact paired, connected,
unlocked iPhone with Developer Mode enabled, an Apple development team, and a
generated xcconfig below the extbuild-owned DerivedData root. The governed
parent workspace supplies those machine inputs. The standalone script refuses
name-only destinations, unsigned builds, non-Debug physical builds, and
xcconfig files outside the managed output root:

```sh
RADROOTS_IOS_PHYSICAL_AUTOMATION=1 \
RADROOTS_IOS_DEVELOPMENT_TEAM=ABCDEFGHIJ \
cargo extbuild run -- scripts/xcode.sh physical-app-build \
  id=00000000-0000000000000000 \
  "$XCODE_DERIVED_DATA/radroots-ios-device/config/device.xcconfig"
```

The values above are placeholders. Device identities, teams, endpoints, and
certificate material are never checked into this public repository.

## Bootstrap and verify

The first bootstrap requires network access. It checks out the exact Rust
source revision, builds the deterministic UniFFI XCFramework and Swift
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

Use `SIMULATOR_NAME="Device Name" make verify` when the default simulator is
not installed. `make clean` removes only rebuildable external build output and
the ignored XCFramework; it preserves tracked generated bindings, locks,
snapshots, project sources, and user files.

## Package surface

`Package.swift` publishes the `RadrootsApp` library used by the generated Xcode
application wrapper. It pins AppleKit by exact HTTPS Git revision and consumes
the locally bootstrapped `RadrootsFFI.xcframework`. Ordinary Xcode compilation
never writes repository source: a read-only preflight rejects missing or stale
FFI artifacts and directs the developer to run `make bootstrap`.

The Rust source lock, generated bindings, XCFramework hashes, provenance, Swift
package locks, privacy manifests, and public API snapshots are checked as part
of the release lane.

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
