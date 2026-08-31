SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c

FFI_ROOT := RadrootsFFI
SIMULATOR_NAME ?= iPhone 17 Pro
SIMULATOR_DESTINATION := platform=iOS Simulator,name=$(SIMULATOR_NAME)

.NOTPARALLEL:

.PHONY: all doctor bootstrap persona-verifier-bootstrap ffi-bootstrap artifact-check package-contract-check \
	swift-quality \
	linux-shared-rust \
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

package-contract-check: doctor
	cargo extbuild run -- scripts/verify-package-contract.sh

swift-quality: doctor
	cargo extbuild run -- scripts/swift-quality.sh

linux-shared-rust: doctor
	cargo extbuild run -- scripts/linux-shared-rust.sh

package-resolve: artifact-check package-contract-check
	cargo extbuild run -- scripts/swift-package.sh resolve

project xcodegen: doctor
	cargo extbuild run -- scripts/generate-project.sh

xcode-resolve: artifact-check project
	cargo extbuild run -- scripts/xcode.sh resolve

bootstrap: persona-verifier-bootstrap ffi-bootstrap package-resolve xcode-resolve

package-build: artifact-check package-contract-check
	cargo extbuild run -- scripts/xcode.sh package-build

package-test: artifact-check package-contract-check
	cargo extbuild run -- scripts/xcode.sh package-test '$(SIMULATOR_DESTINATION)'

xcode-build-debug: artifact-check package-contract-check project
	cargo extbuild run -- scripts/xcode.sh project-build Debug

xcode-build-release: artifact-check package-contract-check project
	cargo extbuild run -- scripts/xcode.sh project-build Release

unit-test: artifact-check package-contract-check project
	cargo extbuild run -- scripts/xcode.sh project-test '$(SIMULATOR_DESTINATION)' RadrootsTests

ui-test: artifact-check package-contract-check project
	cargo extbuild run -- scripts/xcode.sh project-test '$(SIMULATOR_DESTINATION)' RadrootsUITests

api-snapshot-write: package-build
	cargo extbuild run -- scripts/app-api-snapshot.sh write

api-snapshot-check: package-build
	cargo extbuild run -- scripts/app-api-snapshot.sh check

release-evidence-write: doctor
	cargo extbuild run -- scripts/release-evidence.sh write

release-preflight: artifact-check package-contract-check
	cargo extbuild run -- scripts/release-preflight.sh

verify: swift-quality linux-shared-rust artifact-check package-contract-check package-build package-test \
	xcode-build-debug xcode-build-release unit-test ui-test api-snapshot-check

clean: doctor
	cargo extbuild run -- $(MAKE) -C $(FFI_ROOT) clean

distclean: doctor
	cargo extbuild run -- $(MAKE) -C $(FFI_ROOT) distclean
