SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c

FFI_ROOT := TeraFFI
FFI_TARGET ?= aarch64-apple-ios
SIMULATOR_NAME ?= iPhone 17 Pro
SIMULATOR_DESTINATION := platform=iOS Simulator,name=$(SIMULATOR_NAME)

.NOTPARALLEL:

.PHONY: all doctor bootstrap persona-verifier-bootstrap ffi-bootstrap artifact-check package-contract-check \
	ffi-source-write ffi-source-check \
	ffi-candidate-build ffi-candidate-check \
	swift-quality maintainability-check \
	linux-shared-rust kotlin-smoke kotlin-smoke-bootstrap \
	package-resolve package-build package-test project xcodegen xcode-resolve \
	xcode-build-debug xcode-build-release unit-test ui-test api-snapshot-write \
	api-snapshot-check release-evidence-write release-preflight verify clean distclean

all: verify

doctor:
	cargo extbuild doctor

ffi-bootstrap: doctor
	cargo extbuild run -- $(MAKE) -C $(FFI_ROOT) verify

persona-verifier-bootstrap: doctor
	cargo extbuild run -- uv sync --project scripts/persona-verifier --frozen

artifact-check: doctor
	cargo extbuild run -- $(FFI_ROOT)/scripts/verify-installed-artifacts.sh

ffi-source-write: doctor
	cargo extbuild run -- scripts/ffi-provenance.sh write --target '$(FFI_TARGET)'

ffi-source-check: doctor
	cargo extbuild run -- scripts/ffi-provenance.sh check --target '$(FFI_TARGET)'

ffi-candidate-build: doctor
	cargo extbuild run -- $(MAKE) -C $(FFI_ROOT) candidate-build

ffi-candidate-check: doctor
	cargo extbuild run -- $(MAKE) -C $(FFI_ROOT) candidate-check

package-contract-check: doctor
	cargo extbuild run -- scripts/verify-package-contract.sh

swift-quality: doctor
	cargo extbuild run -- scripts/swift-quality.sh

maintainability-check: doctor
	cargo extbuild run -- uv run --offline --project scripts/persona-verifier \
		python scripts/maintainability_ratchet.py verify

linux-shared-rust: doctor
	cargo extbuild run -- scripts/linux-shared-rust.sh

kotlin-smoke: doctor
	cargo extbuild run -- scripts/kotlin-smoke.sh verify

kotlin-smoke-bootstrap: doctor
	cargo extbuild run -- scripts/kotlin-smoke.sh bootstrap

package-resolve: artifact-check package-contract-check
	cargo extbuild run -- scripts/swift-package.sh resolve

project xcodegen: doctor
	cargo extbuild run -- scripts/generate-project.sh

xcode-resolve: artifact-check project
	cargo extbuild run -- scripts/xcode.sh resolve

bootstrap: persona-verifier-bootstrap ffi-bootstrap kotlin-smoke-bootstrap package-resolve xcode-resolve

package-build: artifact-check package-contract-check
	cargo extbuild run -- scripts/xcode.sh package-build

package-test: artifact-check package-contract-check
	cargo extbuild run -- scripts/xcode.sh package-test '$(SIMULATOR_DESTINATION)'

xcode-build-debug: artifact-check package-contract-check project
	cargo extbuild run -- scripts/xcode.sh project-build Debug

xcode-build-release: artifact-check package-contract-check project
	cargo extbuild run -- scripts/xcode.sh project-build Release

unit-test: artifact-check package-contract-check project
	cargo extbuild run -- scripts/xcode.sh project-test '$(SIMULATOR_DESTINATION)' TeraTests

ui-test: artifact-check package-contract-check project
	cargo extbuild run -- scripts/xcode.sh project-test '$(SIMULATOR_DESTINATION)' TeraUITests

api-snapshot-write: package-build
	cargo extbuild run -- scripts/app-api-snapshot.sh write

api-snapshot-check: package-build
	cargo extbuild run -- scripts/app-api-snapshot.sh check

release-evidence-write: doctor
	cargo extbuild run -- scripts/release-evidence.sh write

release-preflight: artifact-check package-contract-check
	cargo extbuild run -- scripts/release-preflight.sh

verify: swift-quality linux-shared-rust kotlin-smoke artifact-check package-contract-check package-build package-test \
	xcode-build-debug xcode-build-release unit-test ui-test api-snapshot-check

clean: doctor
	cargo extbuild run -- $(MAKE) -C $(FFI_ROOT) clean

distclean: doctor
	cargo extbuild run -- $(MAKE) -C $(FFI_ROOT) distclean
