#!/bin/sh
set -eu

project_file=Radroots.xcodeproj/project.pbxproj
localizations_id=A11CE10CA11A710A00000001
root_package_id=9F54FC4930051FC4611B37D3
root_package_name=ios_app

xcodegen generate --spec project.yml

root_package_reference=$(perl -ne '
    if (/^\s*([0-9A-F]{24}) \/\* ([^*]+) \*\/ = \{isa = PBXFileReference; lastKnownFileType = folder; name = (?:"[^"]+"|[^";]+); path = \.; sourceTree = SOURCE_ROOT; \};$/) {
        print "$1\t$2\n";
    }
' "$project_file")
root_package_count=$(printf '%s\n' "$root_package_reference" | grep -c . || true)
if [ "$root_package_count" -ne 1 ]; then
    echo "error: expected exactly one root Swift package file reference" >&2
    exit 1
fi
generated_root_package_id=$(printf '%s\n' "$root_package_reference" | cut -f1)
generated_root_package_name=$(printf '%s\n' "$root_package_reference" | cut -f2-)
GENERATED_ROOT_PACKAGE_ID=$generated_root_package_id \
GENERATED_ROOT_PACKAGE_NAME=$generated_root_package_name \
ROOT_PACKAGE_ID=$root_package_id \
ROOT_PACKAGE_NAME=$root_package_name \
    perl -0pi -e '
        s/\Q$ENV{GENERATED_ROOT_PACKAGE_ID}\E \/\* \Q$ENV{GENERATED_ROOT_PACKAGE_NAME}\E \*\//$ENV{ROOT_PACKAGE_ID} \/\* $ENV{ROOT_PACKAGE_NAME} \*\//g;
        s/name = "?\Q$ENV{GENERATED_ROOT_PACKAGE_NAME}\E"?; path = \.;/name = $ENV{ROOT_PACKAGE_NAME}; path = .;/g;
    ' "$project_file"

if ! grep -Fq "$root_package_id /* $root_package_name */" "$project_file"; then
    echo "error: deterministic root Swift package reference is absent" >&2
    exit 1
fi
if grep -Fq "$generated_root_package_id /* $generated_root_package_name */" "$project_file" && \
    { [ "$generated_root_package_id" != "$root_package_id" ] || \
      [ "$generated_root_package_name" != "$root_package_name" ]; }
then
    echo "error: generated worktree-specific Swift package reference remains" >&2
    exit 1
fi
perl -0pi -e '
    s{
        (/\*\ Begin\ PBXFileReference\ section\ \*/\n)
        (.*?)
        (/\*\ End\ PBXFileReference\ section\ \*/)
    }{
        my ($start, $body, $end) = ($1, $2, $3);
        my @lines = split(/(?<=\n)/, $body);
        $start . join("", sort @lines) . $end;
    }gsex;
' "$project_file"

temporary_count=$(grep -Ec '"TEMP_[0-9A-F-]+" /\* Localizations \*/' "$project_file" || true)
case "$temporary_count" in
    0) ;;
    1)
        perl -0pi -e \
            's/"TEMP_[0-9A-F-]+"( \/\* Localizations \*\/)/A11CE10CA11A710A00000001$1/g' \
            "$project_file"
        ;;
    *)
        echo "error: unexpected XcodeGen Localizations group inventory" >&2
        exit 1
        ;;
esac

if grep -Eq '"TEMP_[0-9A-F-]+" /\* Localizations \*/' "$project_file"; then
    echo "error: XcodeGen temporary Localizations identifier remains" >&2
    exit 1
fi
deterministic_count=$(grep -Ec "$localizations_id /\* Localizations \*/" "$project_file" || true)
if [ "$deterministic_count" -gt 1 ]; then
    echo "error: deterministic Localizations identifier is duplicated" >&2
    exit 1
fi
