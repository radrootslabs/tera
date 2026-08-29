# Radroots iOS app agent specification

This file applies to the complete standalone iOS app repository. A closer
`AGENTS.md` overrides it for its subtree.

## Authority and repository boundary

- This capsule owns the public iOS application, its Swift package, generated
  Xcode project, Apple host lifecycle, app state and views, FFI installation
  boundary, privacy manifest, public API snapshot, and standalone validation.
- `radroots.lib.source-lock.v1.toml`, `Cargo.toml`, and
  `RadrootsFFI/source.lock` must select the same exact remotely reachable public
  lib revision and release version. `Package.swift`, both
  `Package.resolved` files, and `project.yml` own exact Apple package inputs.
- `RadrootsFFI/provenance.json`, `RadrootsFFI/api/**`, `api/**`, `release/**`,
  generated project/source inputs, and the package locks are machine evidence.
  Do not hand-edit generated bindings, XCFramework contents, provenance, SBOM,
  project output, or API snapshots.
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

- The app is an iOS client. It owns Apple presentation, lifecycle callbacks,
  user-presence prompts, Keychain integration, foreground/background
  scheduling, and translation between generated SDK DTOs and view state.
- Canonical domain policy, signing protocol, relay semantics, durable engine
  state, wire contracts, and generated FFI models remain owned by their public
  producer packages. Do not fork them into Swift application models.
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
- `scripts/generate-project.sh` owns `Radroots.xcodeproj`; edit `project.yml`
  and canonical source inputs rather than hand-editing generated project data.
- `RadrootsFFI/scripts/verify-installed-artifacts.sh` must reject missing,
  stale, mismatched, or unproven FFI installations before Swift/Xcode work.
- Keep SwiftPM and Xcode workspace resolved revisions synchronized. Never allow
  automatic dependency updates to select release inputs.
- Repository scripts must keep Xcode derived data and source/package caches,
  SwiftPM scratch/cache output, and Cargo target output under extbuild-owned
  paths. `RadrootsFFI/.build/out/**` and `RadrootsFFI/.radroots/source/**` are
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
- `make swift-quality` applies the checked-in SwiftFormat and SwiftLint policy
  to repository-owned package, app, unit-test, API-test, and UI-test sources;
  generated bindings and dependency/build output are excluded.
- `make linux-shared-rust` runs the locked source-lock workspace in the pinned
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
