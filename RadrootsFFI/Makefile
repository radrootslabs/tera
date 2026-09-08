SHELL := /bin/bash
.DEFAULT_GOAL := all
.SHELLFLAGS := -eu -o pipefail -c

include source.lock

override IPHONEOS_DEPLOYMENT_TARGET := 18.0
override RUSTUP_TOOLCHAIN := 1.97.1-aarch64-apple-darwin

IOS_ROOT := $(abspath $(CURDIR)/..)
APP_ROOT := $(IOS_ROOT)/Radroots
ARTIFACTS_DIR := $(APP_ROOT)/Frameworks
GENERATED_SWIFT_DIR := $(APP_ROOT)/Generated

BUILD_ROOT := $(CURDIR)/.build
SOURCE_ROOT := $(CURDIR)/.radroots/source
TARGET_DIR ?= $(CARGO_TARGET_DIR)
ifeq ($(strip $(TARGET_DIR)),)
TARGET_DIR := $(BUILD_ROOT)/target
endif
override TARGET_DIR := $(abspath $(TARGET_DIR))
OUT_DIR := $(BUILD_ROOT)/out
GENERATED_DIR := $(OUT_DIR)/generated
HEADERS_DIR := $(OUT_DIR)/headers
API_WORK_DIR := $(OUT_DIR)/api
API_MODULE_DIR := $(API_WORK_DIR)/module
API_SYMBOLS_DIR := $(API_WORK_DIR)/symbols
API_SNAPSHOT_DIR := $(CURDIR)/api
API_SNAPSHOT := $(API_SNAPSHOT_DIR)/RadrootsKitBindings.symbols.json
API_RENDERED := $(API_WORK_DIR)/RadrootsKitBindings.symbols.json

FFI_OUTPUT_NAME := RadrootsFFI
LIB_STEM := libradroots_mobile_ffi
GIT_FIELD_LIB_DIR := $(SOURCE_ROOT)/lib
PROVENANCE_OUTPUT := RadrootsFFI/provenance.json
SOURCE_MODE ?= pinned
LOCAL_LIB_ROOT ?=
ifeq ($(SOURCE_MODE),local)
ifeq ($(strip $(LOCAL_LIB_ROOT)),)
$(error LOCAL_LIB_ROOT is required when SOURCE_MODE=local)
endif
SOURCE_WORKSPACE_ROOT := $(abspath $(LOCAL_LIB_ROOT))
else ifeq ($(SOURCE_MODE),pinned)
SOURCE_WORKSPACE_ROOT := $(GIT_FIELD_LIB_DIR)
else
$(error SOURCE_MODE must be pinned or local)
endif
FFI_MANIFEST := $(SOURCE_WORKSPACE_ROOT)/crates/mobile_ffi/Cargo.toml
UNIFFI_CONFIG := $(SOURCE_WORKSPACE_ROOT)/crates/mobile_ffi/uniffi.toml

BINDGEN_MANIFEST := $(SOURCE_WORKSPACE_ROOT)/Cargo.toml
RUSTC_BIN := $(shell rustup which --toolchain $(RUSTUP_TOOLCHAIN) rustc)
CARGO_BIN := rustup run $(RUSTUP_TOOLCHAIN) cargo
CARGO := IPHONEOS_DEPLOYMENT_TARGET=$(IPHONEOS_DEPLOYMENT_TARGET) \
	RUSTC=$(RUSTC_BIN) \
	CARGO_TARGET_DIR=$(TARGET_DIR) \
	RADROOTS_SOURCE_SHA=$(RADROOTS_FIELD_LIB_GIT_REV) \
	SOURCE_DATE_EPOCH=$(RADROOTS_FIELD_SOURCE_DATE_EPOCH) \
	$(CARGO_BIN)

LIB_DEVICE := $(TARGET_DIR)/aarch64-apple-ios/release/$(LIB_STEM).a
LIB_SIMULATOR := $(TARGET_DIR)/aarch64-apple-ios-sim/release/$(LIB_STEM).a
LIB_HOST := $(TARGET_DIR)/release/$(LIB_STEM).dylib
SIMULATOR_HEADERS := $(ARTIFACTS_DIR)/$(FFI_OUTPUT_NAME).xcframework/ios-arm64-simulator/Headers
SWIFT_SDK := $(shell xcrun --sdk iphonesimulator --show-sdk-path)
SWIFT_COMPILER := $(shell xcrun --sdk iphonesimulator --find swiftc)
SWIFT_SYMBOLGRAPH := $(shell xcrun --sdk iphonesimulator --find swift-symbolgraph-extract)
SWIFT_API_TARGET := arm64-apple-ios$(IPHONEOS_DEPLOYMENT_TARGET)-simulator

.PHONY: all clean distclean print-config sync-source ensure-toolchain build generate package install api-snapshot-render api-snapshot-write api-snapshot-check provenance verify
.PHONY: candidate-build candidate-check

candidate-build:
	../scripts/ffi-artifacts.sh build

candidate-check:
	../scripts/ffi-artifacts.sh check

all: verify
	@echo "done"
	@echo "  - source: $(RADROOTS_FIELD_LIB_GIT_URL)@$(RADROOTS_FIELD_LIB_GIT_REV)"
	@echo "  - xcframework: $(ARTIFACTS_DIR)/$(FFI_OUTPUT_NAME).xcframework"
	@echo "  - swift bindings: $(GENERATED_SWIFT_DIR)"

print-config:
	@echo "SOURCE_WORKSPACE_ROOT=$(SOURCE_WORKSPACE_ROOT)"
	@echo "FFI_MANIFEST=$(FFI_MANIFEST)"
	@echo "BINDGEN_MANIFEST=$(BINDGEN_MANIFEST)"
	@echo "UNIFFI_CONFIG=$(UNIFFI_CONFIG)"
	@echo "ARTIFACTS_DIR=$(ARTIFACTS_DIR)"
	@echo "GENERATED_SWIFT_DIR=$(GENERATED_SWIFT_DIR)"
	@echo "TARGET_DIR=$(TARGET_DIR)"
	@echo "IPHONEOS_DEPLOYMENT_TARGET=$(IPHONEOS_DEPLOYMENT_TARGET)"
	@echo "RUSTUP_TOOLCHAIN=$(RUSTUP_TOOLCHAIN)"
	@echo "RUSTC_BIN=$(RUSTC_BIN)"
	@echo "RADROOTS_FIELD_LIB_GIT_REV=$(RADROOTS_FIELD_LIB_GIT_REV)"
	@echo "RADROOTS_FIELD_SOURCE_DATE_EPOCH=$(RADROOTS_FIELD_SOURCE_DATE_EPOCH)"

clean:
	rm -rf $(TARGET_DIR) $(OUT_DIR) $(ARTIFACTS_DIR)/$(FFI_OUTPUT_NAME).xcframework

distclean: clean
	rm -rf $(SOURCE_ROOT)

sync-source:
	@if [ "$(SOURCE_MODE)" = "local" ]; then \
		test -d $(SOURCE_WORKSPACE_ROOT)/.git; \
		test "$$(git -C $(SOURCE_WORKSPACE_ROOT) rev-parse HEAD)" = "$(RADROOTS_FIELD_LIB_GIT_REV)"; \
		git -C $(SOURCE_WORKSPACE_ROOT) diff --quiet --no-ext-diff; \
		git -C $(SOURCE_WORKSPACE_ROOT) diff --cached --quiet --no-ext-diff; \
	else \
		mkdir -p $(SOURCE_ROOT); \
		if [ ! -d $(GIT_FIELD_LIB_DIR)/.git ]; then \
			git clone --filter=blob:none $(RADROOTS_FIELD_LIB_GIT_URL) $(GIT_FIELD_LIB_DIR); \
		fi; \
		git -C $(GIT_FIELD_LIB_DIR) fetch --tags --force origin; \
		git -C $(GIT_FIELD_LIB_DIR) checkout --detach $(RADROOTS_FIELD_LIB_GIT_REV); \
		test "$$(git -C $(GIT_FIELD_LIB_DIR) rev-parse HEAD)" = "$(RADROOTS_FIELD_LIB_GIT_REV)"; \
		git -C $(GIT_FIELD_LIB_DIR) diff --quiet --no-ext-diff; \
		git -C $(GIT_FIELD_LIB_DIR) diff --cached --quiet --no-ext-diff; \
	fi
	test -f $(FFI_MANIFEST)
	test -f $(BINDGEN_MANIFEST)
	test -f $(UNIFFI_CONFIG)
	test "$$($(CARGO_BIN) pkgid --manifest-path $(FFI_MANIFEST) | sed -E 's/.*@//')" = "$(RADROOTS_FIELD_FFI_CRATE_VERSION)"

ensure-toolchain:
	rustup target add --toolchain $(RUSTUP_TOOLCHAIN) aarch64-apple-ios aarch64-apple-ios-sim

build: sync-source ensure-toolchain
	mkdir -p $(TARGET_DIR) $(OUT_DIR)
	$(CARGO) build --manifest-path $(FFI_MANIFEST) --release --locked --target aarch64-apple-ios
	$(CARGO) build --manifest-path $(FFI_MANIFEST) --release --locked --target aarch64-apple-ios-sim
	$(CARGO) build --manifest-path $(FFI_MANIFEST) --release --locked

generate: build
	rm -rf $(GENERATED_DIR) $(HEADERS_DIR)
	mkdir -p $(GENERATED_DIR) $(HEADERS_DIR)
	$(CARGO) run --manifest-path $(BINDGEN_MANIFEST) -p radroots_mobile_bindgen --locked -- \
		generate $(LIB_HOST) --library \
		--language swift \
		--metadata-no-deps \
		--out-dir $(GENERATED_DIR) \
		--config $(UNIFFI_CONFIG)
	cp $(GENERATED_DIR)/$(FFI_OUTPUT_NAME).h $(HEADERS_DIR)/
	cp $(GENERATED_DIR)/$(FFI_OUTPUT_NAME).modulemap $(HEADERS_DIR)/module.modulemap

package: generate
	mkdir -p $(ARTIFACTS_DIR) $(OUT_DIR)
	@staging_root=$$(mktemp -d "$(OUT_DIR)/xcframework.XXXXXX"); \
	trap 'rm -rf "$$staging_root"' EXIT; \
	staging="$$staging_root/$(FFI_OUTPUT_NAME).xcframework"; \
	xcodebuild -create-xcframework \
		-library $(LIB_DEVICE) -headers $(HEADERS_DIR) \
		-library $(LIB_SIMULATOR) -headers $(HEADERS_DIR) \
		-output "$$staging"; \
	plutil -convert json -o "$$staging_root/info.json" "$$staging/Info.plist"; \
	jq -S '.AvailableLibraries |= sort_by(.LibraryIdentifier)' \
		"$$staging_root/info.json" > "$$staging_root/info.sorted.json"; \
	plutil -convert xml1 -o "$$staging/Info.plist" "$$staging_root/info.sorted.json"; \
	rm -rf $(ARTIFACTS_DIR)/$(FFI_OUTPUT_NAME).xcframework; \
	mv "$$staging" $(ARTIFACTS_DIR)/$(FFI_OUTPUT_NAME).xcframework

install: package
	mkdir -p $(GENERATED_SWIFT_DIR)
	if [ -d $(GENERATED_SWIFT_DIR) ]; then \
		find $(GENERATED_SWIFT_DIR) -maxdepth 1 -type f -name '*.swift' -delete; \
	fi
	cp $(GENERATED_DIR)/*.swift $(GENERATED_SWIFT_DIR)/

api-snapshot-render: install
	rm -rf $(API_WORK_DIR)
	mkdir -p $(API_MODULE_DIR) $(API_SYMBOLS_DIR)
	$(SWIFT_COMPILER) \
		-emit-module \
		-parse-as-library \
		-module-name RadrootsKitBindings \
		-target $(SWIFT_API_TARGET) \
		-sdk $(SWIFT_SDK) \
		-I $(SIMULATOR_HEADERS) \
		-module-cache-path $(API_WORK_DIR)/module-cache \
		-emit-module-path $(API_MODULE_DIR)/RadrootsKitBindings.swiftmodule \
		$(GENERATED_SWIFT_DIR)/RadrootsKitBindings.swift
	$(SWIFT_SYMBOLGRAPH) \
		-module-name RadrootsKitBindings \
		-target $(SWIFT_API_TARGET) \
		-sdk $(SWIFT_SDK) \
		-I $(API_MODULE_DIR) \
		-I $(SIMULATOR_HEADERS) \
		-module-cache-path $(API_WORK_DIR)/module-cache \
		-minimum-access-level public \
		-skip-inherited-docs \
		-skip-synthesized-members \
		-output-dir $(API_SYMBOLS_DIR)
	jq -cS \
		'{schema:"radroots.swift-api-snapshot.v1",generator:.metadata.generator,module:.module,symbols:(.symbols | map({kind:.kind.identifier,precise:.identifier.precise,path:.pathComponents,access:.accessLevel,declaration:.declarationFragments}) | sort_by(.precise)),relationships:(.relationships | map({kind,source,target}) | sort_by(.kind,.source,.target))}' \
		$(API_SYMBOLS_DIR)/RadrootsKitBindings.symbols.json > $(API_RENDERED)

api-snapshot-write: api-snapshot-render
	mkdir -p $(API_SNAPSHOT_DIR)
	cp $(API_RENDERED) $(API_SNAPSHOT)

api-snapshot-check: api-snapshot-render
	cmp $(API_RENDERED) $(API_SNAPSHOT)

provenance: install
	$(CARGO) run --manifest-path $(BINDGEN_MANIFEST) -p xtask --locked -- \
		artifact \
		--product mobile \
		--target ios \
		--language swift \
		--mode write \
		--consumer-root $(IOS_ROOT) \
		--source-root $(SOURCE_WORKSPACE_ROOT) \
		--output $(PROVENANCE_OUTPUT) \
		--source-date-epoch $(RADROOTS_FIELD_SOURCE_DATE_EPOCH) \
		--builder-id radroots_ios_app
	$(CARGO) run --manifest-path $(BINDGEN_MANIFEST) -p xtask --locked -- \
		artifact \
		--product mobile \
		--target ios \
		--language swift \
		--mode check \
		--consumer-root $(IOS_ROOT) \
		--source-root $(SOURCE_WORKSPACE_ROOT) \
		--output $(PROVENANCE_OUTPUT) \
		--source-date-epoch $(RADROOTS_FIELD_SOURCE_DATE_EPOCH) \
		--builder-id radroots_ios_app

verify: provenance api-snapshot-check
	test "$$(shasum -a 256 $(LIB_DEVICE) | awk '{print $$1}')" = "$(RADROOTS_FIELD_FFI_DEVICE_SHA256)"
	test "$$(shasum -a 256 $(LIB_SIMULATOR) | awk '{print $$1}')" = "$(RADROOTS_FIELD_FFI_SIMULATOR_SHA256)"
	test "$$(shasum -a 256 $(GENERATED_SWIFT_DIR)/RadrootsKitBindings.swift | awk '{print $$1}')" = "$(RADROOTS_FIELD_FFI_SWIFT_SHA256)"
	test "$$(shasum -a 256 $(GENERATED_DIR)/RadrootsFFI.h | awk '{print $$1}')" = "$(RADROOTS_FIELD_FFI_HEADER_SHA256)"
	test "$$(shasum -a 256 $(GENERATED_DIR)/RadrootsFFI.modulemap | awk '{print $$1}')" = "$(RADROOTS_FIELD_FFI_MODULEMAP_SHA256)"
	test "$$(shasum -a 256 $(API_SNAPSHOT) | awk '{print $$1}')" = "$(RADROOTS_FIELD_FFI_API_SHA256)"
	test "$$(cd $(ARTIFACTS_DIR) && find $(FFI_OUTPUT_NAME).xcframework -type f -print | LC_ALL=C sort | while IFS= read -r file; do shasum -a 256 "$$file"; done | shasum -a 256 | awk '{print $$1}')" = "$(RADROOTS_FIELD_FFI_XCFRAMEWORK_SHA256)"
	! grep -E -i 'secret.?key|private.?key|file.?path|media.?bytes|raw.?media|unbounded' $(GENERATED_SWIFT_DIR)/RadrootsKitBindings.swift $(GENERATED_DIR)/RadrootsFFI.h
