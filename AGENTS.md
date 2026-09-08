# Tera iOS app agent specification

This file applies to the complete standalone iOS app repository. A closer
`AGENTS.md` overrides it for its subtree.

## Authority and repository boundary

- This capsule owns the public iOS application, its Swift package, generated
  Xcode project, Apple host lifecycle, app state and views, FFI installation
  boundary, privacy manifest, public API snapshot, and standalone validation.
  It also owns application Rust policy, runtime transitions, durable authored
  operation orchestration and application FFI. Keep one root Rust workspace;
  application packages belong under `core/crates`. Native artifacts use the
  owned tera_ffi and tera_bindgen packages with exact shared foundation pins.
- `radroots.lib.source-lock.v1.toml`, `Cargo.toml`, and
  the foundation section of generated `TeraFFI/source.lock` must select the same
  exact remotely reachable public Lib revision and release version. The lock
  separately identifies the owned Tera source tree and installed manifest.
  `Package.swift`, both
  `Package.resolved` files, and `project.yml` own exact Apple package inputs.
- `TeraFFI/provenance.json`, `TeraFFI/api/**`, `api/**`, `release/**`,
  generated project/source inputs, and the package locks are machine evidence.
  Do not hand-edit generated bindings, XCFramework contents, provenance, SBOM,
  project output, or API snapshots.
- `TeraFFI/producer.toml` separately governs the owned Tera producer.
  Stage its declared source inputs before `make ffi-source-write`; check with
  `make ffi-source-check`. Source records identify a staged input tree and exact
  build tuple under extbuild output. They do not establish installed artifacts
  or remote release qualification; installed provenance remains separate.
  `make ffi-candidate-build ffi-candidate-check` builds and validates the exact
  owned native artifact bundle in external staging. `make ffi-bootstrap`
  installs its matching framework, bindings, API and source records with a
  separate nonempty installed manifest. Local evidence is not release qualification.
- Human specifications, decisions, migration history, runbooks, and
  qualification evidence are parent-owned under `docs/oss/ios_app/**`. They
  are absent from a standalone clone and must never become a build, test,
  package, generation, or release input for this capsule.
- `docs/**`, `.github/**`, and `.act/**` are forbidden tracked roots. Public
  commands remain forge agnostic; cross-repository workflow proof belongs only
  to the parent workspace's root `.act/**` surface.
- Do not depend on non-public parent paths, non-public contracts, implicit
  sibling checkouts, floating branches/tags, or unrecorded local artifacts.

## Product and security boundaries

- The product has exactly two bottom tabs, Today and Add, and retains its five
  current creation families. Cached Today and local Add must remain usable
  independently of network readiness. Do not activate farm, CRDT, commerce,
  additional transports or unrelated product surfaces through this refactor.
- The Swift host owns Apple presentation, lifecycle callbacks,
  user-presence prompts, Keychain integration, foreground/background
  scheduling, and translation between generated SDK DTOs and view state.
- Application validation, transitions, durable receipts and app-facing FFI
  models belong to this application's Rust packages. Shared domain types,
  signing protocols, transport and storage mechanics stay in their existing
  public producer packages at exact pins. Swift translates and presents those
  contracts; it must not fork canonical policy or own a second database.
- Branding changes must preserve installed bundle/Keychain identities,
  persisted schema and hash namespaces, and frozen signed operation identities.
- Keep identity secrets in the Apple credential boundary. Never log, snapshot,
  serialize, fixture, or expose secret material, raw private event content,
  credentials, tokens, private paths, or unsafe internal errors.
- Background work is host-owned, explicit, bounded, cancelable, and recoverable
  across app lifecycle changes. Do not add hidden workers, process-global
  runtime ownership, implicit relays, or direct database authority.
- Services-hardening generated changes must adopt the approved four coverage
  states and three outcomes across Swift and FFI together. Do not retain
  prototype evidence, receipt, outcome, or compatibility aliases.
- Physical-device development must use one exact UDID, an explicitly supplied
  development team, a Debug `iphoneos` build, and verified TLS endpoints.
  Never select the first device, disable signing or certificate verification,
  rewrite the checked-in Debug defaults, or treat local-device evidence as
  approved remote qualification.

## Generated and project files

- Change canonical producer contracts and generators before regenerating FFI
  or SDK output. Inspect every generated diff and run freshness/API checks.
- `scripts/generate-project.sh` owns `Tera.xcodeproj`; edit `project.yml`
  and canonical source inputs rather than hand-editing generated project data.
- `TeraFFI/scripts/verify-installed-artifacts.sh` must reject missing,
  stale, mismatched, or unproven FFI installations before Swift/Xcode work.
- Keep SwiftPM and Xcode workspace resolved revisions synchronized. Never allow
  automatic dependency updates to select release inputs.
- Repository scripts must keep Xcode derived data and source/package caches,
  SwiftPM scratch/cache output, and Cargo target output under extbuild-owned
  paths. `TeraFFI/.build/out/**` and `TeraFFI/.radroots/source/**` are
  ignored, rebuildable repo-local staging/source cache; they are never
  canonical source, tracked output, or independent release authority.

## Working and verification rules

- Inspect `git status --short`, relevant locks/contracts, package/project
  manifests, source, tests, scripts, generated artifacts, and snapshots before
  editing. Preserve unrelated work.
- Run `cargo extbuild doctor` before the first mutating build, test, check,
  dependency, package, generation, or snapshot command, then use the
  repository's Make/script surfaces, which route work through
  `cargo extbuild run -- ...`.
- `make package-contract-check` is the narrow standalone source-lock,
  package-lock, privacy, version, and forbidden-root guard.
- Persona qualification must use `scripts/persona-verifier.sh`, the exact
  Python and schema dependency lock under `scripts/persona-verifier/**`, and
  offline frozen resolution. Populate the exact lock only through
  `make persona-verifier-bootstrap`; do not bypass it with ambient Python.
- `make swift-quality` applies the checked-in SwiftFormat and SwiftLint policy
  to repository-owned package, app, unit-test, API-test, and UI-test sources;
  generated bindings and dependency/build output are excluded. It also runs
  the exact checked SwiftLint debt baseline, locked Ruff checks, and the
  closed size/complexity ratchet. Do not broaden a legacy exception or add a
  new exception to admit new code; decompose the new responsibility instead.
- `make maintainability-check` is the narrow fail-closed physical-line and
  Python-AST complexity gate. Its source revision records the pre-ratchet
  inventory, exception ceilings may only decrease or disappear, and newly
  bounded modules must remain below the fixed thresholds.
- `make linux-shared-rust` runs explicit locked native packages in the pinned
  Linux x86_64 Rust runner while keeping Cargo caches and output under the
  extbuild project root.
- `make bootstrap` performs networked source/artifact bootstrap. `make verify`
  is the complete package, Xcode build/test, UI, and API-snapshot lane. Use an
  explicitly installed simulator name when the default is unavailable.
- `make release-evidence-write` regenerates deterministic unsigned release
  evidence from exact locks and artifacts. `make release-preflight` checks its
  freshness and source authority without signing, tagging, or publication.
- Run the smallest credible target while iterating, followed by the complete
  affected standalone lane. Never claim a command passed unless it ran
  successfully; report missing Xcode, simulator, signing, or network
  prerequisites exactly.
- Prefer explicit typed state, deterministic transformations, bounded inputs,
  narrow side effects, safe errors, and fail-closed validation. Avoid `unsafe`
  and forced unwraps in production paths unless a local invariant is explicit
  and tested.

## Changes and external gates

- Make one coherent, reviewable target-state change at a time. Keep source,
  tests, machine contracts, generated outputs, locks, snapshots, and public
  README material aligned.
- Use focused commit subjects in the repository's established imperative
  style.
- Evidence that requires a normative product decision is recorded in the
  parent-owned services-hardening authority, with the corresponding standalone
  machine contract changed in the same ordered sequence. Do not create a local
  human deviation ledger.
- Do not push, tag, publish packages, deploy, change signing identities,
  profiles, entitlements, registry ownership, or credentials without separate
  explicit authority.

## Definition of done

- The requested behavior is complete at the correct Apple-host, generated FFI,
  package, project, or application boundary.
- Relevant package/Xcode/tool validation passed, generated output and locks are
  fresh, public API snapshots agree, and zero `docs/**`, `.github/**`, or
  `.act/**` roots exist.
- The final review finds no secret exposure, hidden runtime ownership, private
  dependency, unrelated change, or unreported skipped lane, and records whether
  the next sequence step is safe.
