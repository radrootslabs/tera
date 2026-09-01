#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
python_project="$repo_root/scripts/persona-verifier"

uv run --project "$python_project" --offline --frozen \
	python "$repo_root/scripts/package_contract.py" --repo-root "$repo_root"

(
	cd "$repo_root"
	uv run --project "$python_project" --offline --frozen \
		python -m unittest \
		scripts/test_package_contract.py \
		scripts/test_local_social_fixture.py
)

sh "$repo_root/scripts/persona-verifier.sh" verify-bud11-corpus \
	--corpus "$repo_root/test-fixtures/bud11-upload-authorization-mutations.v1.json" \
	--schema "$repo_root/test-fixtures/bud11-upload-authorization-mutations.v1.schema.json"

sh "$repo_root/scripts/persona-verifier.sh" verify-persona-fixture \
	--fixture "$repo_root/test-fixtures/local-social-personas.v1.json" \
	--fixture-schema "$repo_root/test-fixtures/local-social-personas.v1.schema.json" \
	--result-schema "$repo_root/test-fixtures/local-social-persona-results.v1.schema.json" \
	--attempt-schema "$repo_root/test-fixtures/local-social-persona-attempt-evidence.v1.schema.json" \
	--result-v2-schema "$repo_root/test-fixtures/local-social-persona-results.v2.schema.json"
