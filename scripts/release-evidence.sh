#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
mode=${1:-}
case "$mode" in
    write|check) ;;
    *) echo "usage: $0 {write|check}" >&2; exit 64 ;;
esac

for tool in cargo jq shasum
do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "error: required release-evidence tool is unavailable: $tool" >&2
        exit 1
    }
done

tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/tera-ios-release.XXXXXX")
trap 'rm -rf "$tmp_root"' EXIT HUP INT TERM
metadata="$tmp_root/cargo-metadata.json"
rendered_sbom="$tmp_root/sbom.cdx.json"
rendered_provenance="$tmp_root/provenance.json"

cargo metadata --locked --format-version 1 > "$metadata"

version=$(jq -er '
    [.packages[] | select(.name == "tera_ffi") | .version]
    | if length == 1 then .[0] else error("missing owned FFI package") end
' "$metadata")

jq -S --slurpfile swift "$repo_root/Package.resolved" \
    --arg version "$version" '
    def cargo_ref:
        "cargo:" + .name + "@" + .version + "?source=" + (.source // "workspace");
    def swift_ref:
        "swift:" + .identity + "@" + .state.revision + "?source=" + .location;
    def cargo_components:
        [.packages[] |
            {
                "bom-ref": cargo_ref,
                type: "library",
                name: .name,
                version: .version,
                licenses: (
                    if .license == null then [] else [{expression: .license}] end
                ),
                properties: [
                    {name: "radroots.package.manager", value: "cargo"},
                    {name: "radroots.package.source", value: (.source // "workspace")}
                ]
            }
        ];
    def swift_components:
        [$swift[0].pins[] |
            {
                "bom-ref": swift_ref,
                type: "library",
                name: .identity,
                version: .state.revision,
                properties: [
                    {name: "radroots.package.manager", value: "swiftpm"},
                    {name: "radroots.package.source", value: .location},
                    {name: "radroots.package.revision", value: .state.revision}
                ]
            }
        ];
    . as $cargo |
    ($cargo.packages |
        map({key: .id, value: cargo_ref}) |
        from_entries
    ) as $cargo_refs |
    (cargo_components + swift_components | sort_by(."bom-ref")) as $components |
    ("pkg:generic/tera@" + $version) as $application_ref |
    (
        [$cargo.resolve.nodes[] |
            {
                ref: $cargo_refs[.id],
                dependsOn: ([.dependencies[] as $dependency | $cargo_refs[$dependency]] | sort)
            }
        ] +
        [$swift[0].pins[] | {ref: swift_ref, dependsOn: []}] +
        [{
            ref: $application_ref,
            dependsOn: (
                ([$cargo.workspace_members[] as $member | $cargo_refs[$member]] +
                 [$swift[0].pins[] | swift_ref]) |
                sort
            )
        }] |
        sort_by(.ref)
    ) as $dependencies |
    {
        bomFormat: "CycloneDX",
        specVersion: "1.5",
        version: 1,
        metadata: {
            component: {
                "bom-ref": $application_ref,
                type: "application",
                name: "tera",
                version: $version,
                purl: ("pkg:generic/tera@" + $version),
                externalReferences: [{
                    type: "vcs",
                    url: "https://github.com/radrootslabs/tera"
                }]
            }
        },
        components: $components,
        dependencies: $dependencies
    }
' "$metadata" > "$rendered_sbom"

hash_file() {
    shasum -a 256 "$1" | awk '{print $1}'
}

lib_revision=$(jq -er '.foundation.revision' "$repo_root/TeraFFI/source/aarch64-apple-ios.json")
source_date_epoch=$(jq -er '.build.source_date_epoch' "$repo_root/TeraFFI/source/aarch64-apple-ios.json")
tera_ffi_source_tree=$(jq -er '.candidate.source.tree' "$repo_root/TeraFFI/provenance.json")
consumer_lock_sha256=$(hash_file "$repo_root/radroots.lib.source-lock.v1.toml")
cargo_lock_sha256=$(hash_file "$repo_root/Cargo.lock")
swift_lock_sha256=$(hash_file "$repo_root/Package.resolved")
xcode_swift_lock_sha256=$(hash_file \
    "$repo_root/Tera.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved")
ffi_provenance_sha256=$(hash_file "$repo_root/TeraFFI/provenance.json")
ffi_api_sha256=$(hash_file "$repo_root/TeraFFI/api/TeraKitBindings.symbols.json")
app_api_sha256=$(hash_file "$repo_root/api/TeraApp.symbols.json")
info_plist_sha256=$(hash_file "$repo_root/Tera/Info.plist")
privacy_manifest_sha256=$(hash_file "$repo_root/Tera/Resources/PrivacyInfo.xcprivacy")
xcode_project_sha256=$(hash_file "$repo_root/Tera.xcodeproj/project.pbxproj")
sbom_sha256=$(hash_file "$rendered_sbom")

jq -nS \
    --arg version "$version" \
    --arg lib_revision "$lib_revision" \
    --arg tera_ffi_source_tree "$tera_ffi_source_tree" \
    --argjson source_date_epoch "$source_date_epoch" \
    --arg consumer_lock_sha256 "$consumer_lock_sha256" \
    --arg cargo_lock_sha256 "$cargo_lock_sha256" \
    --arg swift_lock_sha256 "$swift_lock_sha256" \
    --arg xcode_swift_lock_sha256 "$xcode_swift_lock_sha256" \
    --arg ffi_provenance_sha256 "$ffi_provenance_sha256" \
    --arg ffi_api_sha256 "$ffi_api_sha256" \
    --arg app_api_sha256 "$app_api_sha256" \
    --arg info_plist_sha256 "$info_plist_sha256" \
    --arg privacy_manifest_sha256 "$privacy_manifest_sha256" \
    --arg xcode_project_sha256 "$xcode_project_sha256" \
    --arg sbom_sha256 "$sbom_sha256" '
    {
        schema: "tera.release-provenance.v1",
        product: "tera",
        version: $version,
        repository: "https://github.com/radrootslabs/tera",
        source: {
            lib_revision: $lib_revision,
            tera_ffi_source_tree: $tera_ffi_source_tree,
            source_date_epoch: $source_date_epoch,
            consumer_source_lock_sha256: $consumer_lock_sha256,
            cargo_lock_sha256: $cargo_lock_sha256,
            swift_package_lock_sha256: $swift_lock_sha256,
            xcode_package_lock_sha256: $xcode_swift_lock_sha256
        },
        artifacts: {
            ffi_provenance_sha256: $ffi_provenance_sha256,
            ffi_api_sha256: $ffi_api_sha256,
            app_api_sha256: $app_api_sha256,
            info_plist_sha256: $info_plist_sha256,
            privacy_manifest_sha256: $privacy_manifest_sha256,
            xcode_project_sha256: $xcode_project_sha256,
            sbom_sha256: $sbom_sha256
        },
        platforms: ["ios-arm64", "ios-arm64-simulator"],
        disposition: "unsigned"
    }
' > "$rendered_provenance"

jq -e '
    .bomFormat == "CycloneDX" and
    .specVersion == "1.5" and
    .version == 1 and
    .metadata.component.name == "tera" and
    (.components | length > 0) and
    (([.components[]."bom-ref"] | unique | length) == (.components | length)) and
    ([.components[]."bom-ref"] == ([.components[]."bom-ref"] | sort)) and
    ([.dependencies[].ref] == ([.dependencies[].ref] | sort)) and
    (.dependencies | all(.dependsOn == (.dependsOn | sort))) and
    (.components | all(
        (.name | type == "string" and length > 0) and
        (.version | type == "string" and length > 0)
    ))
' "$rendered_sbom" >/dev/null
jq -e \
    --arg lib_revision "$lib_revision" \
    --arg tera_ffi_source_tree "$tera_ffi_source_tree" \
    --arg sbom_sha256 "$sbom_sha256" '
    .schema == "tera.release-provenance.v1" and
    .repository == "https://github.com/radrootslabs/tera" and
    .source.lib_revision == $lib_revision and
    .source.tera_ffi_source_tree == $tera_ffi_source_tree and
    .artifacts.sbom_sha256 == $sbom_sha256 and
    .platforms == ["ios-arm64", "ios-arm64-simulator"] and
    .disposition == "unsigned"
' "$rendered_provenance" >/dev/null

if rg -n '/Users/|/Volumes/|BEGIN [A-Z ]*PRIVATE KEY' \
    "$rendered_sbom" "$rendered_provenance"
then
    echo "error: release evidence contains forbidden host or protected material" >&2
    exit 1
fi

output_root="$repo_root/release"
case "$mode" in
    write)
        mkdir -p "$output_root"
        install -m 0644 "$rendered_sbom" "$output_root/sbom.cdx.json"
        install -m 0644 "$rendered_provenance" "$output_root/provenance.json"
        ;;
    check)
        cmp "$rendered_sbom" "$output_root/sbom.cdx.json"
        cmp "$rendered_provenance" "$output_root/provenance.json"
        ;;
esac

echo "release evidence $mode succeeded for tera@$version"
