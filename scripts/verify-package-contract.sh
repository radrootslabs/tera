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
grep -Fq '<key>RADROOTS_IOS_UI_TEST_FIXTURE_EVIDENCE</key>' \
    "$repo_root/RadrootsUITests/Info.plist"
grep -Fq '<key>RADROOTS_IOS_UI_TEST_NETWORK_PROFILE</key>' \
    "$repo_root/RadrootsUITests/Info.plist"
for evidence_key in \
    RADROOTS_IOS_UI_TEST_SOURCE_COMMIT \
    RADROOTS_IOS_UI_TEST_SOURCE_TREE \
    RADROOTS_IOS_UI_TEST_APP_BUILD_SHA256 \
    RADROOTS_IOS_UI_TEST_SIMULATOR_ID
do
    grep -Fq "<key>$evidence_key</key>" "$repo_root/RadrootsUITests/Info.plist"
done
grep -Fq 'local-social-ui-test)' "$repo_root/scripts/xcode.sh"
grep -Fq 'RadrootsUITests/RadrootsRemoteQualificationUITests/testLocalSocialFiveFlowScenario' \
    "$repo_root/scripts/xcode.sh"
grep -Fq 'RadrootsUITests/RadrootsRemoteQualificationUITests/testLocalSocialAccessibilitySemantics' \
    "$repo_root/scripts/xcode.sh"
grep -Fq 'RadrootsUITests/RadrootsRemoteQualificationUITests/testLocalSocialDeterministicPersonas' \
    "$repo_root/scripts/xcode.sh"
grep -Fq 'requested_content_size=large' "$repo_root/scripts/xcode.sh"
grep -Fq 'requested_content_size=accessibility-extra-extra-extra-large' \
    "$repo_root/scripts/xcode.sh"
grep -Fq 'xcrun simctl ui "$simulator_id" content_size "$requested_content_size"' \
    "$repo_root/scripts/xcode.sh"
grep -Fq 'xcrun simctl ui "$simulator_id" content_size "$previous_content_size"' \
    "$repo_root/scripts/xcode.sh"
grep -Fq 'verify-accessibility' "$repo_root/scripts/local-social-fixture.py"
grep -Fq 'verify-persona-fixture' "$repo_root/scripts/local-social-fixture.py"
grep -Fq 'verify-bud11-corpus' "$repo_root/scripts/local-social-fixture.py"
grep -Fq 'verify-persona' "$repo_root/scripts/local-social-fixture.py"
grep -Fq 'verify-persona-result' "$repo_root/scripts/local-social-fixture.py"
grep -Fq 'RADROOTS_IOS_UI_TEST_FIXTURE_EVIDENCE' "$repo_root/scripts/xcode.sh"
grep -Fq 'LoopbackConnectionFactory' "$repo_root/scripts/local-social-fixture.py"
grep -Fq 'attachments = extract_persona_attempt_attachments(' \
    "$repo_root/scripts/local-social-fixture.py"
grep -Fq 'result = reconstruct_persona_result_v2(' \
    "$repo_root/scripts/local-social-fixture.py"
grep -Fq -- '--attempt-schema test-fixtures/local-social-persona-attempt-evidence.v1.schema.json' \
    "$repo_root/scripts/xcode.sh"
grep -Fq -- '--result-v2-schema test-fixtures/local-social-persona-results.v2.schema.json' \
    "$repo_root/scripts/xcode.sh"
if rg -n 'pending_step_258' "$repo_root/RadrootsUITests" --glob '*.swift'; then
    echo "error: XCUITest must emit measured network evidence" >&2
    exit 1
fi
grep -Fq 'add.tap()' "$repo_root/RadrootsUITests/RadrootsRemoteQualificationUITests.swift"
if rg -n 'coordinate\(' "$repo_root/RadrootsUITests" --glob '*.swift'; then
    echo "error: UI qualification must not use coordinate taps" >&2
    exit 1
fi
for fixture in \
    bud11-upload-authorization-mutations.v1.json \
    bud11-upload-authorization-mutations.v1.schema.json \
    local-social-personas.v1.json \
    local-social-personas.v1.schema.json \
    local-social-persona-results.v1.schema.json \
    local-social-persona-attempt-evidence.v1.schema.json \
    local-social-persona-results.v2.schema.json
do
    test -f "$repo_root/test-fixtures/$fixture"
done
python3 "$repo_root/scripts/local-social-fixture.py" verify-bud11-corpus \
    --corpus "$repo_root/test-fixtures/bud11-upload-authorization-mutations.v1.json" \
    --schema "$repo_root/test-fixtures/bud11-upload-authorization-mutations.v1.schema.json"
python3 "$repo_root/scripts/local-social-fixture.py" verify-persona-fixture \
    --fixture "$repo_root/test-fixtures/local-social-personas.v1.json" \
    --fixture-schema "$repo_root/test-fixtures/local-social-personas.v1.schema.json" \
    --result-schema "$repo_root/test-fixtures/local-social-persona-results.v1.schema.json" \
    --attempt-schema "$repo_root/test-fixtures/local-social-persona-attempt-evidence.v1.schema.json" \
    --result-v2-schema "$repo_root/test-fixtures/local-social-persona-results.v2.schema.json"
grep -Fq 'RadrootsUITests/testLocalSocialDeterministicPersonas' \
    "$repo_root/scripts/local-social-fixture.py"
grep -Fq 'radroots.ios.local-social.persona-attempt-evidence.v1' \
    "$repo_root/RadrootsUITests/RadrootsRemoteQualificationUITests.swift"
(
    cd "$repo_root"
    python3 -m unittest scripts/test_local_social_fixture.py
)
grep -Fq 'performAccessibilityAudit' \
    "$repo_root/RadrootsUITests/RadrootsRemoteQualificationUITests.swift"
grep -Fq 'element.identifier == "radroots.add.submit"' \
    "$repo_root/RadrootsUITests/RadrootsRemoteQualificationUITests.swift"
grep -Fq 'return issue.auditType == .textClipped' \
    "$repo_root/RadrootsUITests/RadrootsRemoteQualificationUITests.swift"
grep -Fq 'issue.auditType == .contrast || issue.auditType == .textClipped' \
    "$repo_root/RadrootsUITests/RadrootsRemoteQualificationUITests.swift"
grep -Fq -- '-UIAccessibilityReduceMotionEnabled' \
    "$repo_root/RadrootsUITests/RadrootsRemoteQualificationUITests.swift"
grep -Fq 'accessibility-extra-extra-extra-large' "$repo_root/scripts/xcode.sh"
grep -Fq '#if DEBUG' "$repo_root/Radroots/App/RadrootsRemoteQualification.swift"
grep -Fq 'case "simulator": self = .isolatedLoopback' \
    "$repo_root/Radroots/App/RadrootsRemoteQualification.swift"
grep -Fq 'case "public": self = .publicEndpoint' \
    "$repo_root/Radroots/App/RadrootsRemoteQualification.swift"
grep -Fq 'if let qualification, qualification.automatesIdentity' \
    "$repo_root/Radroots/State/RadrootsIdentityStore.swift"
grep -Fq 'qualification?.automatesIdentity == true' \
    "$repo_root/Radroots/State/RadrootsSessionStore.swift"
if rg -n 'if qualification != nil|automatesQualificationIdentity: qualification != nil' \
    "$repo_root/Radroots" --glob '*.swift'
then
    echo "error: qualification existence must not grant automated identity authority" >&2
    exit 1
fi
test -f "$repo_root/scripts/local-social-fixture.py"
test -f "$repo_root/.swiftformat"
test -f "$repo_root/.swiftlint.yml"
test -x "$repo_root/scripts/swift-quality.sh"
test -x "$repo_root/scripts/linux-shared-rust.sh"
grep -Fq -- '--no-cache' "$repo_root/scripts/swift-quality.sh"
grep -Fq 'swift-quality: doctor' "$repo_root/Makefile"
grep -Fq 'linux-shared-rust: doctor' "$repo_root/Makefile"
grep -Fq 'verify: swift-quality linux-shared-rust' "$repo_root/Makefile"

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
