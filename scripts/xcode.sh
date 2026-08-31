#!/bin/bash
set -euo pipefail

for variable in XCODE_DERIVED_DATA XCODE_SOURCE_PACKAGES XCODE_PACKAGE_CACHE; do
    if [[ -z "${!variable:-}" ]]; then
        echo "error: $variable is required; run this command through cargo extbuild" >&2
        exit 1
    fi
done

output_args=(
    -quiet
    -derivedDataPath "$XCODE_DERIVED_DATA"
    -clonedSourcePackagesDirPath "$XCODE_SOURCE_PACKAGES"
    -packageCachePath "$XCODE_PACKAGE_CACHE"
)
offline_args=(
    -disableAutomaticPackageResolution
    -onlyUsePackageVersionsFromResolvedFile
    -skipPackagePluginValidation
)

operation=${1:-}
case "$operation" in
    resolve)
        exec xcodebuild \
            -resolvePackageDependencies \
            -project Radroots.xcodeproj \
            -scheme Radroots \
            "${output_args[@]}"
        ;;
    package-build)
        exec xcodebuild \
            -scheme RadrootsApp \
            -destination 'generic/platform=iOS Simulator' \
            "${output_args[@]}" \
            "${offline_args[@]}" \
            ARCHS=arm64 \
            build
        ;;
    package-test)
        destination=${2:?package-test requires a simulator destination}
        exec xcodebuild \
            -scheme RadrootsAppPublicAPITests \
            -destination "$destination" \
            "${output_args[@]}" \
            "${offline_args[@]}" \
            test
        ;;
    project-build)
        configuration=${2:?project-build requires a configuration}
        case "$configuration" in
            Debug|Release) ;;
            *) echo "error: unsupported configuration: $configuration" >&2; exit 1 ;;
        esac
        exec xcodebuild \
            -project Radroots.xcodeproj \
            -scheme Radroots \
            -configuration "$configuration" \
            -destination 'generic/platform=iOS Simulator' \
            "${output_args[@]}" \
            "${offline_args[@]}" \
            build
        ;;
    project-test)
        destination=${2:?project-test requires a simulator destination}
        test_target=${3:?project-test requires a test target}
        case "$test_target" in
            RadrootsTests|RadrootsUITests) ;;
            *) echo "error: unsupported test target: $test_target" >&2; exit 1 ;;
        esac
        exec xcodebuild \
            -project Radroots.xcodeproj \
            -scheme Radroots \
            -destination "$destination" \
            "${output_args[@]}" \
            "${offline_args[@]}" \
            "-only-testing:$test_target" \
            test
        ;;
    physical-app-build)
        destination=${2:?physical app build requires a device destination}
        xcconfig=${3:?physical app build requires an xcconfig}
        physical_automation=${RADROOTS_IOS_PHYSICAL_AUTOMATION:-}
        development_team=${RADROOTS_IOS_DEVELOPMENT_TEAM:?RADROOTS_IOS_DEVELOPMENT_TEAM is required}
        if [[ "$physical_automation" != "1" ]]; then
            echo "error: physical app build requires RADROOTS_IOS_PHYSICAL_AUTOMATION=1" >&2
            exit 64
        fi
        if [[ ! "$destination" =~ ^id=[A-Fa-f0-9-]+$ ]]; then
            echo "error: physical app build destination must be one exact device id" >&2
            exit 64
        fi
        if [[ ! "$development_team" =~ ^[A-Z0-9]{10}$ ]]; then
            echo "error: physical app build development team is invalid" >&2
            exit 64
        fi
        if [[ "$xcconfig" != "$XCODE_DERIVED_DATA"/* || ! -f "$xcconfig" ]]; then
            echo "error: physical app build xcconfig must be a regular file under XCODE_DERIVED_DATA" >&2
            exit 64
        fi
        device_id=${destination#id=}
        lock_state_file=$(mktemp "${TMPDIR:-/tmp}/radroots-ios-lock-state.XXXXXX")
        trap 'unlink "$lock_state_file"' EXIT
        if ! xcrun devicectl device info lockState \
            --device "$device_id" \
            --quiet \
            --timeout 10 \
            --json-output "$lock_state_file"
        then
            echo "error: physical app build device lock state is unavailable" >&2
            exit 1
        fi
        passcode_required=$(/usr/bin/plutil \
            -extract result.passcodeRequired raw -o - "$lock_state_file")
        unlocked_since_boot=$(/usr/bin/plutil \
            -extract result.unlockedSinceBoot raw -o - "$lock_state_file")
        if [[ "$passcode_required" != "false" || "$unlocked_since_boot" != "true" ]]; then
            echo "error: physical app build device is locked; refusing to invoke Xcode" >&2
            exit 1
        fi
        exec xcodebuild \
            -project Radroots.xcodeproj \
            -scheme Radroots \
            -configuration Debug \
            -destination "$destination" \
            -xcconfig "$xcconfig" \
            "${output_args[@]}" \
            "${offline_args[@]}" \
            "DEVELOPMENT_TEAM=$development_team" \
            CODE_SIGN_STYLE=Automatic \
            "CODE_SIGN_IDENTITY=Apple Development" \
            build
        ;;
    local-social-ui-test)
        destination=${2:?local-social-ui-test requires a simulator destination}
        result_name=${3:?local-social-ui-test requires a result name}
        scenario=${4:-five-flow}
        qualification_run_id=${RADROOTS_IOS_UI_TEST_RUN_ID:?RADROOTS_IOS_UI_TEST_RUN_ID is required}
        relay_port=${RADROOTS_IOS_LOCAL_SOCIAL_RELAY_PORT:-21000}
        blossom_port=${RADROOTS_IOS_LOCAL_SOCIAL_BLOSSOM_PORT:-21100}
        if [[ ! "$destination" =~ ^platform=iOS\ Simulator,id=[A-Fa-f0-9-]+$ ]]; then
            echo "error: local-social-ui-test destination must be one exact simulator id" >&2
            exit 64
        fi
        if [[ ! "$result_name" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]]; then
            echo "error: local-social-ui-test result name is invalid" >&2
            exit 64
        fi
        if [[ ! "$qualification_run_id" =~ ^[a-z0-9][a-z0-9-]{6,62}[a-z0-9]$ ]]; then
            echo "error: local-social-ui-test run id is invalid" >&2
            exit 64
        fi
        for port in "$relay_port" "$blossom_port"; do
            if [[ ! "$port" =~ ^[0-9]+$ ]] || ((port < 1024 || port > 65535)); then
                echo "error: local-social-ui-test port is invalid" >&2
                exit 64
            fi
        done
        if [[ "$relay_port" == "$blossom_port" ]]; then
            echo "error: local-social-ui-test ports must be distinct" >&2
            exit 64
        fi
        case "$scenario" in
            five-flow)
                test_selector=RadrootsUITests/RadrootsRemoteQualificationUITests/testLocalSocialFiveFlowScenario
                evidence_command=verify
                simulator_id=${destination##*id=}
                previous_content_size=
                requested_content_size=large
                ;;
            accessibility)
                test_selector=RadrootsUITests/RadrootsRemoteQualificationUITests/testLocalSocialAccessibilitySemantics
                evidence_command=verify-accessibility
                simulator_id=${destination##*id=}
                previous_content_size=
                requested_content_size=accessibility-extra-extra-extra-large
                persona_fixture=
                persona_result=
                ;;
            persona)
                test_selector=RadrootsUITests/RadrootsRemoteQualificationUITests/testLocalSocialDeterministicPersonas
                evidence_command=verify-persona
                simulator_id=${destination##*id=}
                previous_content_size=
                requested_content_size=accessibility-extra-extra-extra-large
                persona_fixture=test-fixtures/local-social-personas.v1.json
                persona_result="$XCODE_RESULTS/$result_name.persona-result.json"
                ;;
            *)
                echo "error: unsupported local-social-ui-test scenario: $scenario" >&2
                exit 64
                ;;
        esac
        : "${XCODE_RESULTS:?XCODE_RESULTS is required}"
        mkdir -p "$XCODE_RESULTS"
        result_bundle="$XCODE_RESULTS/$result_name.xcresult"
        evidence="$XCODE_RESULTS/$result_name.fixture.json"
        ready="$XCODE_RESULTS/$result_name.ready.json"
        control="$XCODE_RESULTS/$result_name.enable-uploads"
        if [[ -e "$result_bundle" || -e "$evidence" || -e "$ready" || -e "$control" || ( -n "${persona_result:-}" && -e "$persona_result" ) ]]; then
            echo "error: local-social-ui-test result already exists: $result_name" >&2
            exit 1
        fi
        if [[ "$scenario" == persona ]]; then
            if [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
                echo "error: persona qualification requires one clean exact source tree" >&2
                exit 1
            fi
            source_commit=$(git rev-parse HEAD)
            source_tree=$(git rev-parse 'HEAD^{tree}')
            upstream=$(git rev-parse '@{upstream}')
            xcode_identity=$(xcodebuild -version | tr '\n' ' ')
            simulator_sdk_build=$(xcrun --sdk iphonesimulator --show-sdk-build-version)
            app_build_sha256=$(
                printf 'radroots.ios.local-social.app-build.v1\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0' \
                    "$source_commit" "$source_tree" RadrootsUITests test Debug \
                    "$xcode_identity" "$simulator_sdk_build" \
                    | shasum -a 256 | awk '{print $1}'
            )
            persona_repair_commit=${RADROOTS_IOS_UI_TEST_FORWARD_REPAIR_COMMIT:-}
            if [[ "$source_commit" != "$upstream" ]]; then
                echo "error: persona qualification source is not equal to its configured upstream" >&2
                exit 1
            fi
            if [[ -n "$persona_repair_commit" ]]; then
                if [[ ! "$persona_repair_commit" =~ ^[0-9a-f]{40}$ ]] || \
                    ! git merge-base --is-ancestor "$persona_repair_commit" "$source_commit"
                then
                    echo "error: persona forward-repair commit is invalid" >&2
                    exit 1
                fi
            fi
            sh scripts/persona-verifier.sh verify-persona-fixture \
                --fixture "$persona_fixture" \
                --fixture-schema test-fixtures/local-social-personas.v1.schema.json \
                --result-schema test-fixtures/local-social-persona-results.v1.schema.json \
                --attempt-schema test-fixtures/local-social-persona-attempt-evidence.v1.schema.json \
                --result-v2-schema test-fixtures/local-social-persona-results.v2.schema.json
            sh scripts/persona-verifier.sh serve \
                --relay-port "$relay_port" \
                --blossom-port "$blossom_port" \
                --evidence "$evidence" \
                --ready "$ready" \
                --control "$control" \
                --persona-fixture "$persona_fixture" &
        else
            sh scripts/persona-verifier.sh serve \
                --relay-port "$relay_port" \
                --blossom-port "$blossom_port" \
                --evidence "$evidence" \
                --ready "$ready" \
                --control "$control" &
        fi
        fixture_pid=$!
        cleanup_fixture() {
            kill "$fixture_pid" 2>/dev/null || true
            wait "$fixture_pid" 2>/dev/null || true
            if [[ -n "$previous_content_size" ]]; then
                xcrun simctl ui "$simulator_id" content_size "$previous_content_size"
                previous_content_size=
            fi
        }
        trap cleanup_fixture EXIT INT TERM
        for _ in {1..100}; do
            [[ -f "$ready" ]] && break
            kill -0 "$fixture_pid" 2>/dev/null || {
                echo "error: local-social fixture exited before readiness" >&2
                exit 1
            }
            sleep 0.1
        done
        if [[ ! -f "$ready" ]]; then
            echo "error: local-social fixture readiness timed out" >&2
            exit 1
        fi
        xcrun simctl boot "$simulator_id" >/dev/null 2>&1 || true
        xcrun simctl bootstatus "$simulator_id" -b >/dev/null
        previous_content_size=$(xcrun simctl ui "$simulator_id" content_size)
        case "$previous_content_size" in
            extra-small|small|medium|large|extra-large|extra-extra-large|extra-extra-extra-large|accessibility-medium|accessibility-large|accessibility-extra-large|accessibility-extra-extra-large|accessibility-extra-extra-extra-large) ;;
            *)
                echo "error: simulator content-size state is unavailable" >&2
                exit 1
                ;;
        esac
        xcrun simctl ui "$simulator_id" content_size "$requested_content_size"
        set +e
        xcodebuild \
            -project Radroots.xcodeproj \
            -scheme Radroots \
            -configuration Debug \
            -destination "$destination" \
            "${output_args[@]}" \
            "${offline_args[@]}" \
            -resultBundlePath "$result_bundle" \
            "-only-testing:$test_selector" \
            "RADROOTS_IOS_UI_TEST_RUN_ID=$qualification_run_id" \
            "RADROOTS_IOS_UI_TEST_NOSTR_RELAY_URLS=ws://127.0.0.1:$relay_port" \
            "RADROOTS_IOS_UI_TEST_BLOSSOM_ORIGINS=http://127.0.0.1:$blossom_port" \
            "RADROOTS_IOS_UI_TEST_FIXTURE_CONTROL=$control" \
            "RADROOTS_IOS_UI_TEST_FIXTURE_EVIDENCE=$evidence" \
            "RADROOTS_IOS_UI_TEST_NETWORK_PROFILE=simulator" \
            "RADROOTS_IOS_UI_TEST_SOURCE_COMMIT=${source_commit:-}" \
            "RADROOTS_IOS_UI_TEST_SOURCE_TREE=${source_tree:-}" \
            "RADROOTS_IOS_UI_TEST_APP_BUILD_SHA256=${app_build_sha256:-}" \
            "RADROOTS_IOS_UI_TEST_SIMULATOR_ID=$simulator_id" \
            test
        test_status=$?
        set -e
        cleanup_fixture
        trap - EXIT INT TERM
        if ((test_status != 0)); then
            exit "$test_status"
        fi
        if [[ "$scenario" == persona ]]; then
            persona_result_command=(sh scripts/persona-verifier.sh "$evidence_command" \
                --fixture "$persona_fixture" \
                --fixture-schema test-fixtures/local-social-personas.v1.schema.json \
                --result-schema test-fixtures/local-social-persona-results.v1.schema.json \
                --attempt-schema test-fixtures/local-social-persona-attempt-evidence.v1.schema.json \
                --result-v2-schema test-fixtures/local-social-persona-results.v2.schema.json \
                --evidence "$evidence" \
                --result-bundle "$result_bundle" \
                --output "$persona_result" \
                --source-commit "$source_commit" \
                --source-tree "$source_tree" \
                --run-id "$qualification_run_id" \
                --simulator-id "$simulator_id")
            if [[ -n "$persona_repair_commit" ]]; then
                persona_result_command+=(--forward-repair-commit "$persona_repair_commit")
            fi
            "${persona_result_command[@]}"
        else
            sh scripts/persona-verifier.sh "$evidence_command" --evidence "$evidence"
        fi
        ;;
    remote-ui-test)
        destination=${2:?remote-ui-test requires a simulator destination}
        test_selector=${3:?remote-ui-test requires a RadrootsUITests selector}
        result_name=${4:?remote-ui-test requires a result name}
        qualification_run_id=${RADROOTS_IOS_UI_TEST_RUN_ID:?RADROOTS_IOS_UI_TEST_RUN_ID is required}
        blossom_origins=${RADROOTS_IOS_UI_TEST_BLOSSOM_ORIGINS:?RADROOTS_IOS_UI_TEST_BLOSSOM_ORIGINS is required}
        relay_urls=${RADROOTS_IOS_UI_TEST_NOSTR_RELAY_URLS:-}
        if [[ ! "$destination" =~ ^platform=iOS\ Simulator,id=[A-Fa-f0-9-]+$ ]]; then
            echo "error: remote-ui-test destination must be one exact simulator id" >&2
            exit 64
        fi
        if [[ ! "$test_selector" =~ ^RadrootsUITests(/[-A-Za-z0-9_]+){0,2}$ ]]; then
            echo "error: remote-ui-test selector is invalid" >&2
            exit 64
        fi
        if [[ ! "$result_name" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]]; then
            echo "error: remote-ui-test result name is invalid" >&2
            exit 64
        fi
        if [[ ! "$qualification_run_id" =~ ^[a-z0-9][a-z0-9-]{6,62}[a-z0-9]$ ]]; then
            echo "error: remote-ui-test run id is invalid" >&2
            exit 64
        fi
        result_bundle="$XCODE_RESULTS/$result_name.xcresult"
        if [[ -e "$result_bundle" ]]; then
            echo "error: remote-ui-test result already exists: $result_bundle" >&2
            exit 1
        fi
        mkdir -p "$XCODE_RESULTS"
        exec xcodebuild \
            -project Radroots.xcodeproj \
            -scheme Radroots \
            -configuration Debug \
            -destination "$destination" \
            "${output_args[@]}" \
            "${offline_args[@]}" \
            -resultBundlePath "$result_bundle" \
            "-only-testing:$test_selector" \
            "RADROOTS_IOS_UI_TEST_RUN_ID=$qualification_run_id" \
            "RADROOTS_IOS_UI_TEST_NOSTR_RELAY_URLS=$relay_urls" \
            "RADROOTS_IOS_UI_TEST_BLOSSOM_ORIGINS=$blossom_origins" \
            "RADROOTS_IOS_UI_TEST_NETWORK_PROFILE=public" \
            test
        ;;
    physical-ui-build|physical-ui-test)
        destination=${2:?physical UI qualification requires a device destination}
        test_selector=${3:?physical UI qualification requires a RadrootsUITests selector}
        result_name=${4:-}
        physical_automation=${RADROOTS_IOS_UI_TEST_PHYSICAL_AUTOMATION:-}
        development_team=${RADROOTS_IOS_DEVELOPMENT_TEAM:?RADROOTS_IOS_DEVELOPMENT_TEAM is required}
        qualification_run_id=${RADROOTS_IOS_UI_TEST_RUN_ID:?RADROOTS_IOS_UI_TEST_RUN_ID is required}
        blossom_origins=${RADROOTS_IOS_UI_TEST_BLOSSOM_ORIGINS:?RADROOTS_IOS_UI_TEST_BLOSSOM_ORIGINS is required}
        relay_urls=${RADROOTS_IOS_UI_TEST_NOSTR_RELAY_URLS:-}
        if [[ "$physical_automation" != "1" ]]; then
            echo "error: physical UI qualification requires RADROOTS_IOS_UI_TEST_PHYSICAL_AUTOMATION=1" >&2
            exit 64
        fi
        if [[ ! "$destination" =~ ^id=[A-Fa-f0-9-]+$ ]]; then
            echo "error: physical-ui-test destination must be one exact device id" >&2
            exit 64
        fi
        if [[ ! "$test_selector" =~ ^RadrootsUITests(/[-A-Za-z0-9_]+){0,2}$ ]]; then
            echo "error: physical-ui-test selector is invalid" >&2
            exit 64
        fi
        if [[ ! "$development_team" =~ ^[A-Z0-9]{10}$ ]]; then
            echo "error: physical-ui-test development team is invalid" >&2
            exit 64
        fi
        if [[ ! "$qualification_run_id" =~ ^[a-z0-9][a-z0-9-]{6,62}[a-z0-9]$ ]]; then
            echo "error: physical-ui-test run id is invalid" >&2
            exit 64
        fi
        device_id=${destination#id=}
        lock_state_file=$(mktemp "${TMPDIR:-/tmp}/radroots-ios-lock-state.XXXXXX")
        trap 'unlink "$lock_state_file"' EXIT
        if ! xcrun devicectl device info lockState \
            --device "$device_id" \
            --quiet \
            --timeout 10 \
            --json-output "$lock_state_file"
        then
            echo "error: physical UI qualification device lock state is unavailable" >&2
            exit 1
        fi
        passcode_required=$(/usr/bin/plutil \
            -extract result.passcodeRequired raw -o - "$lock_state_file")
        unlocked_since_boot=$(/usr/bin/plutil \
            -extract result.unlockedSinceBoot raw -o - "$lock_state_file")
        if [[ "$passcode_required" != "false" || "$unlocked_since_boot" != "true" ]]; then
            echo "error: physical UI qualification device is locked; refusing to invoke Xcode" >&2
            exit 1
        fi
        physical_args=(
            -project Radroots.xcodeproj \
            -scheme Radroots \
            -configuration Debug \
            -destination "$destination" \
            "${output_args[@]}" \
            "${offline_args[@]}" \
            "-only-testing:$test_selector" \
            "DEVELOPMENT_TEAM=$development_team" \
            CODE_SIGN_STYLE=Automatic \
            "CODE_SIGN_IDENTITY=Apple Development" \
            "RADROOTS_IOS_UI_TEST_RUN_ID=$qualification_run_id" \
            "RADROOTS_IOS_UI_TEST_NOSTR_RELAY_URLS=$relay_urls" \
            "RADROOTS_IOS_UI_TEST_BLOSSOM_ORIGINS=$blossom_origins" \
            "RADROOTS_IOS_UI_TEST_NETWORK_PROFILE=public"
        )
        if [[ "$operation" == "physical-ui-build" ]]; then
            exec xcodebuild "${physical_args[@]}" build-for-testing
        fi
        if [[ ! "$result_name" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]]; then
            echo "error: physical-ui-test result name is invalid" >&2
            exit 64
        fi
        result_bundle="$XCODE_RESULTS/$result_name.xcresult"
        if [[ -e "$result_bundle" ]]; then
            echo "error: physical-ui-test result already exists: $result_bundle" >&2
            exit 1
        fi
        mkdir -p "$XCODE_RESULTS"
        exec xcodebuild \
            "${physical_args[@]}" \
            -resultBundlePath "$result_bundle" \
            test-without-building
        ;;
    *)
        echo "usage: $0 {resolve|package-build|package-test|project-build|project-test|local-social-ui-test|remote-ui-test|physical-ui-build|physical-ui-test}" >&2
        exit 64
        ;;
esac
