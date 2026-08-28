#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
package="$repo_root/Package.swift"
source_lock="$repo_root/RadrootsFFI/source.lock"
consumer_lock="$repo_root/radroots.lib.source-lock.v1.toml"

grep -Fq 'repository = "https://github.com/radrootslabs/tera"' "$repo_root/Cargo.toml"
grep -Fq '9F54FC4930051FC4611B37D3 /* ios_app */' \
    "$repo_root/Radroots.xcodeproj/project.pbxproj"

for forbidden_root in docs .github .act
do
    if [ -e "$repo_root/$forbidden_root" ] || [ -L "$repo_root/$forbidden_root" ]; then
        echo "error: forbidden public repository root exists: $forbidden_root" >&2
        exit 1
    fi
done

make_value() {
    key=$1
    awk -v key="$key" '$2 == key && $3 == ":=" { print $4 }' "$source_lock"
}

release_version=$(make_value RADROOTS_FIELD_FFI_CRATE_VERSION)
lib_revision=$(make_value RADROOTS_FIELD_LIB_GIT_REV)
apple_revision=$(sed -n '/url: "https:\/\/github.com\/radrootslabs\/apple_kit.git"/{n;s/.*revision: "\([0-9a-f]*\)".*/\1/p;}' "$package")

if ! printf '%s\n' "$apple_revision" | grep -Eq '^[0-9a-f]{40}$'; then
    echo "error: apple_kit must use an exact 40-character Git revision" >&2
    exit 1
fi

grep -Fq "repository = \"https://github.com/radrootslabs/lib\"" "$consumer_lock"
grep -Fq "revision = \"$lib_revision\"" "$consumer_lock"
grep -Fq "version = \"$release_version\"" "$consumer_lock"
grep -Fq "public static let version = \"$release_version\"" "$repo_root/Radroots/App/AppEntry.swift"
grep -Fq "revision: $apple_revision" "$repo_root/project.yml"
grep -Fq "XCTAssertEqual(RadrootsAppRelease.version, \"$release_version\")" \
    "$repo_root/RadrootsPublicAPITests/RadrootsAppPublicAPITests.swift"
grep -Fq "NSPrivacyAccessedAPICategoryUserDefaults" \
    "$repo_root/Radroots/Resources/PrivacyInfo.xcprivacy"
grep -Fq "CA92.1" "$repo_root/Radroots/Resources/PrivacyInfo.xcprivacy"
plutil -lint "$repo_root/Radroots/Resources/PrivacyInfo.xcprivacy" >/dev/null
plutil -lint "$repo_root/Radroots/Info.plist" >/dev/null
plutil -lint "$repo_root/RadrootsUITests/Info.plist" >/dev/null

grep -Fq '<key>NSCameraUsageDescription</key>' "$repo_root/Radroots/Info.plist"
grep -Fq '<key>NSFaceIDUsageDescription</key>' "$repo_root/Radroots/Info.plist"
grep -Fq '<key>NSLocalNetworkUsageDescription</key>' "$repo_root/Radroots/Info.plist"
if ! plutil -extract NSAppTransportSecurity json -o - "$repo_root/Radroots/Info.plist" \
    | jq -e '
        type == "object" and
        keys == ["NSAllowsLocalNetworking"] and
        .NSAllowsLocalNetworking == true
    ' >/dev/null
then
    echo "error: ATS must allow only explicitly selected local-network development" >&2
    exit 1
fi
if grep -Fq '<key>NSBonjourServices</key>' "$repo_root/Radroots/Info.plist"; then
    echo "error: physical-device development must not enable Bonjour discovery" >&2
    exit 1
fi
if grep -Fq '<key>NSPhotoLibraryUsageDescription</key>' "$repo_root/Radroots/Info.plist"; then
    echo "error: PHPicker must not claim broad photo-library access" >&2
    exit 1
fi

grep -Fq 'RADROOTS_FIELD_IOS_NOSTR_RELAY_URLS = ws:$(SLASH)$(SLASH)127.0.0.1:21000' \
    "$repo_root/Radroots/Config/Debug.xcconfig"
grep -Fq 'RADROOTS_FIELD_IOS_BLOSSOM_ORIGINS = http:$(SLASH)$(SLASH)127.0.0.1:21100' \
    "$repo_root/Radroots/Config/Debug.xcconfig"
grep -Fq 'RADROOTS_FIELD_IOS_NOSTR_RELAY_URLS = wss:$(SLASH)$(SLASH)radroots.org$(SLASH)' \
    "$repo_root/Radroots/Config/Base.xcconfig"
grep -Fq 'RADROOTS_FIELD_IOS_BLOSSOM_ORIGINS = https:$(SLASH)$(SLASH)blossom.radroots.org' \
    "$repo_root/Radroots/Config/Base.xcconfig"
grep -Fq '<key>RADROOTS_IOS_UI_TEST_FIXTURE_CONTROL</key>' \
    "$repo_root/RadrootsUITests/Info.plist"
grep -Fq '<key>RADROOTS_IOS_UI_TEST_NETWORK_PROFILE</key>' \
    "$repo_root/RadrootsUITests/Info.plist"
grep -Fq 'local-social-ui-test)' "$repo_root/scripts/xcode.sh"
grep -Fq 'RadrootsUITests/RadrootsRemoteQualificationUITests/testLocalSocialFiveFlowScenario' \
    "$repo_root/scripts/xcode.sh"
grep -Fq '#if DEBUG' "$repo_root/Radroots/App/RadrootsRemoteQualification.swift"
grep -Fq 'case "simulator": runtimeMode = "simulator"' \
    "$repo_root/Radroots/App/RadrootsRemoteQualification.swift"
test -f "$repo_root/scripts/local-social-fixture.py"

if rg -n \
    'AsyncImage|URLSession\.shared|Data\(contentsOf:[[:space:]]*URL' \
    "$repo_root/Radroots" \
    --glob '*.swift'
then
    echo "error: production Swift source contains an ungoverned media/network access path" >&2
    exit 1
fi

for resolved in \
    "$repo_root/Package.resolved" \
    "$repo_root/Radroots.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved"
do
    if [ ! -f "$resolved" ]; then
        echo "error: missing Swift package lock: $resolved" >&2
        exit 1
    fi
    jq -e --arg apple_revision "$apple_revision" '
        (.version == 3) and
        (.pins | length == 2) and
        (all(.pins[];
            (.kind == "remoteSourceControl") and
            (.location | startswith("https://")) and
            (.state.revision | test("^[0-9a-f]{40}$")))) and
        (any(.pins[];
            .location == "https://github.com/radrootslabs/apple_kit.git" and
            .state.revision == $apple_revision)) and
        (any(.pins[];
            .location == "https://github.com/21-DOT-DEV/swift-secp256k1.git" and
            .state.revision == "e70a10e036a55fffea31568f0af92d69b6d449cd"))
    ' "$resolved" >/dev/null
done

package_pins=$(jq -cS '.pins | sort_by(.identity)' "$repo_root/Package.resolved")
project_pins=$(jq -cS '.pins | sort_by(.identity)' \
    "$repo_root/Radroots.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved")
if [ "$package_pins" != "$project_pins" ]; then
    echo "error: Swift package and Xcode project locks disagree" >&2
    exit 1
fi

echo "package contracts agree at $release_version; apple_kit@$apple_revision"
