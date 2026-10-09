"""Standalone fixed fixture budgets; explicit short functional cases stay short."""

from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path

POLICY_PATH = Path(__file__).with_name("fixture_test_policy.v1.json")


def unique_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate fixture policy key")
        result[key] = value
    return result


def validate_identity(policy):
    if (
        policy.get("schema") != "radroots.tera.fixture-test-policy.v1"
        or policy.get("id") != "tera_fixture_busy_workstation_v1"
        or policy.get("phase") != "pre_mvp_20261008"
        or policy.get("functional_short_intervals_unchanged") is not True
        or policy.get("quiet_workstation_required") is not False
    ):
        raise ValueError("unknown fixture policy or altered functional controls")


def valid_budget(value):
    return type(value) in (int, float) and math.isfinite(value) and 0 < value <= 900


def load_policy():
    if POLICY_PATH.is_symlink():
        raise ValueError("fixture policy must be a regular source file")
    with POLICY_PATH.open("rb") as stream:
        raw = stream.read(16385)
    if len(raw) > 16384:
        raise ValueError("fixture policy exceeds its source bound")
    policy = json.loads(raw, object_pairs_hook=unique_keys)
    if not isinstance(policy, dict):
        raise ValueError("fixture policy must be an object")
    validate_identity(policy)
    limits = policy["limits"]
    if not isinstance(limits, dict) or not limits:
        raise ValueError("fixture budgets are missing")
    if not all(valid_budget(value) for value in limits.values()):
        raise ValueError("fixture budget is outside its finite bound")
    if policy.get("caps") != {"stdout": 1048576, "stderr": 65536}:
        raise ValueError("fixture output caps changed")
    policy["source_sha256"] = hashlib.sha256(raw).hexdigest()
    return policy


def limit(name):
    return load_policy()["limits"][name]
