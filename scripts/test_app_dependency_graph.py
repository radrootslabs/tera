from __future__ import annotations

import copy
import json
import sys
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import app_dependency_graph as graph  # noqa: E402
import package_contract as contract  # noqa: E402


class AppDependencyGraphTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.root = SCRIPTS.parent
        cls.metadata = contract._cargo_workspace(cls.root, resolved=True)
        cls.foundation = contract._read_toml(
            cls.root / "radroots.lib.source-lock.v1.toml"
        )

    def test_actual_locked_application_graph(self) -> None:
        graph.validate(self.metadata, self.foundation)

    def test_actual_graph_rejects_retired_producer_and_duplicate_resolution(
        self,
    ) -> None:
        for name in graph.RETIRED:
            with self.subTest(name=name):
                changed = copy.deepcopy(self.metadata)
                changed["packages"].append({"id": "retired", "name": name})
                with self.assertRaisesRegex(graph.GraphError, "retired"):
                    graph.validate(changed, self.foundation)
        changed = copy.deepcopy(self.metadata)
        changed["packages"].append(changed["packages"][0])
        with self.assertRaisesRegex(graph.GraphError, "duplicated"):
            graph.validate(changed, self.foundation)

    def test_actual_shared_resolution_rejects_pin_version_and_local_override(
        self,
    ) -> None:
        for field, value in (
            ("source", None),
            ("source", "git+https://example.invalid/lib"),
            ("version", "9.9.9"),
        ):
            with self.subTest(field=field, value=value):
                changed = copy.deepcopy(self.metadata)
                package = next(
                    p for p in changed["packages"] if p["name"] == "radroots_sdk"
                )
                package[field] = value
                with self.assertRaisesRegex(graph.GraphError, "foundation"):
                    graph.validate(changed, self.foundation)

    def test_default_lane_rejects_wasm_or_omitted_native_package(self) -> None:
        wasm = next(
            p["id"] for p in self.metadata["packages"] if p["name"] == "tera_wasm"
        )
        for defaults in (
            [],
            self.metadata["workspace_default_members"][:-1],
            [*self.metadata["workspace_default_members"], wasm],
        ):
            with self.subTest(defaults=defaults):
                changed = {**self.metadata, "workspace_default_members": defaults}
                with self.assertRaisesRegex(graph.GraphError, "mobile defaults"):
                    graph.validate(changed, self.foundation)

    def test_mobile_profile_requires_executable_ffi_edge(self) -> None:
        changed = copy.deepcopy(self.metadata)
        ffi = next(p for p in changed["packages"] if p["name"] == "tera_ffi")
        edge = next(d for d in ffi["dependencies"] if d["name"] == "tera_core")
        edge["features"] = []
        with self.assertRaisesRegex(graph.GraphError, "mobile-social"):
            graph.validate(changed, self.foundation)

    def test_mobile_profile_requires_resolved_feature(self) -> None:
        changed = copy.deepcopy(self.metadata)
        core = next(p["id"] for p in changed["packages"] if p["name"] == "tera_core")
        node = next(n for n in changed["resolve"]["nodes"] if n["id"] == core)
        node["features"] = []
        with self.assertRaisesRegex(graph.GraphError, "mobile-social"):
            graph.validate(changed, self.foundation)

    def test_resolution_is_required_and_machine_readable(self) -> None:
        changed = json.loads(json.dumps(self.metadata))
        changed["resolve"] = None
        with self.assertRaisesRegex(graph.GraphError, "resolved dependency graph"):
            graph.validate(changed, self.foundation)


if __name__ == "__main__":
    unittest.main()
