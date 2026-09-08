#!/bin/bash
set -euo pipefail

operation=${1:-}
case "$operation" in
    write|check) ;;
    *) echo "usage: $0 {write|check}" >&2; exit 64 ;;
esac

for variable in XCODE_DERIVED_DATA XCODE_SOURCE_PACKAGES; do
    if [[ -z "${!variable:-}" ]]; then
        echo "error: $variable is required; run this command through cargo extbuild" >&2
        exit 1
    fi
done

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
work_dir="$XCODE_DERIVED_DATA/tera-app-api"
symbol_dir="$work_dir/symbols"
rendered="$work_dir/TeraApp.symbols.json"
snapshot="$repo_root/api/TeraApp.symbols.json"
products="$XCODE_DERIVED_DATA/Build/Products/Debug-iphonesimulator"
generated_module_maps="$XCODE_DERIVED_DATA/Build/Intermediates.noindex/GeneratedModuleMaps-iphonesimulator"
secp_headers="$XCODE_SOURCE_PACKAGES/checkouts/swift-secp256k1/Sources/libsecp256k1/include"
ffi_headers="$repo_root/Tera/Frameworks/TeraFFI.xcframework/ios-arm64-simulator/Headers"
sdk=$(xcrun --sdk iphonesimulator --show-sdk-path)

rm -rf "$work_dir"
mkdir -p "$symbol_dir"
xcrun swift-symbolgraph-extract \
    -module-name TeraApp \
    -target arm64-apple-ios18.0-simulator \
    -sdk "$sdk" \
    -I "$products" \
    -I "$ffi_headers" \
    -F "$products" \
    -Xcc "-fmodule-map-file=$generated_module_maps/libsecp256k1.modulemap" \
    -Xcc "-I$secp_headers" \
    -minimum-access-level public \
    -skip-inherited-docs \
    -skip-synthesized-members \
    -output-dir "$symbol_dir"

jq -cS \
    '{schema:"radroots.swift-api-snapshot.v1",module:.module,symbols:(.symbols | map({kind:.kind.identifier,precise:.identifier.precise,path:.pathComponents,access:.accessLevel,declaration:.declarationFragments}) | sort_by(.precise)),relationships:(.relationships | map({kind,source,target}) | sort_by(.kind,.source,.target))}' \
    "$symbol_dir/TeraApp.symbols.json" > "$rendered"

if [[ "$operation" == write ]]; then
    mkdir -p "$(dirname -- "$snapshot")"
    cp "$rendered" "$snapshot"
else
    if [[ ! -f "$snapshot" ]]; then
        echo "error: missing public API snapshot; run 'make api-snapshot-write'" >&2
        exit 1
    fi
    cmp "$rendered" "$snapshot"
fi

echo "TeraApp public API snapshot $operation succeeded"
