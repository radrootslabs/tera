from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import maintainability_ratchet as ratchet  # noqa: E402


class MaintainabilityRatchetTests(unittest.TestCase):
    def _repository(self) -> tuple[tempfile.TemporaryDirectory[str], Path]:
        temporary = tempfile.TemporaryDirectory()
        root = Path(temporary.name)
        (root / "Radroots").mkdir()
        (root / "RadrootsTests").mkdir()
        (root / "RadrootsUITests").mkdir()
        (root / "scripts").mkdir()
        (root / "test-fixtures").mkdir()
        (root / "Radroots/App.swift").write_text("struct App {}\n", encoding="utf-8")
        (root / "scripts/check.py").write_text(
            "def check():\n    return True\n", encoding="utf-8"
        )
        self._write_baseline(root)
        return temporary, root

    def _write_baseline(self, root: Path, **overrides: object) -> None:
        baseline: dict[str, object] = {
            "schema": "radroots.ios.maintainability-baseline.v1",
            "schema_version": 1,
            "source_revision": "c63002bcc4d3f6656e93aabe4fca6bd771376629",
            "thresholds": {
                "swift_file_lines": 600,
                "python_file_lines": 800,
                "python_function_complexity": 10,
            },
            "swift_file_exception": [],
            "python_file_exception": [],
            "python_complexity_exception": [],
            "bounded_module": ["Radroots/App.swift", "scripts/check.py"],
        }
        baseline.update(overrides)
        (root / ratchet.BASELINE_PATH).write_text(
            json.dumps(baseline, sort_keys=True), encoding="utf-8"
        )

    def test_current_repository_satisfies_ratchet(self) -> None:
        ratchet.verify(SCRIPTS.parent)

    def test_new_oversized_swift_file_is_rejected(self) -> None:
        temporary, root = self._repository()
        with temporary:
            (root / "Radroots/Large.swift").write_text("x\n" * 601, encoding="utf-8")
            with self.assertRaisesRegex(ratchet.MaintainabilityError, "inventory"):
                ratchet.verify(root)

    def test_exception_cannot_grow_or_become_stale(self) -> None:
        temporary, root = self._repository()
        with temporary:
            path = root / "Radroots/App.swift"
            path.write_text("x\n" * 602, encoding="utf-8")
            exception = [{"path": "Radroots/App.swift", "maximum_lines": 601}]
            self._write_baseline(root, swift_file_exception=exception)
            with self.assertRaisesRegex(ratchet.MaintainabilityError, "regressed"):
                ratchet.verify(root)
            path.write_text("struct App {}\n", encoding="utf-8")
            with self.assertRaisesRegex(ratchet.MaintainabilityError, "inventory"):
                ratchet.verify(root)

    def test_new_oversized_python_file_is_rejected(self) -> None:
        temporary, root = self._repository()
        with temporary:
            (root / "scripts/large.py").write_text("x = 1\n" * 801, encoding="utf-8")
            with self.assertRaisesRegex(ratchet.MaintainabilityError, "inventory"):
                ratchet.verify(root)

    def test_new_complex_python_function_is_rejected(self) -> None:
        temporary, root = self._repository()
        with temporary:
            branches = "".join(
                f"    if value == {index}:\n        return {index}\n"
                for index in range(10)
            )
            (root / "scripts/complex.py").write_text(
                "def complex(value):\n" + branches + "    return -1\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ratchet.MaintainabilityError, "inventory"):
                ratchet.verify(root)

    def test_baseline_identity_threshold_and_order_are_closed(self) -> None:
        temporary, root = self._repository()
        with temporary:
            self._write_baseline(root, source_revision="f" * 40)
            with self.assertRaisesRegex(ratchet.MaintainabilityError, "identity"):
                ratchet.verify(root)
            self._write_baseline(
                root,
                thresholds={
                    "swift_file_lines": 601,
                    "python_file_lines": 800,
                    "python_function_complexity": 10,
                },
            )
            with self.assertRaisesRegex(ratchet.MaintainabilityError, "threshold"):
                ratchet.verify(root)
            self._write_baseline(
                root,
                bounded_module=["scripts/check.py", "Radroots/App.swift"],
            )
            with self.assertRaisesRegex(ratchet.MaintainabilityError, "inventory"):
                ratchet.verify(root)

    def test_bounded_module_must_exist_and_remain_small(self) -> None:
        temporary, root = self._repository()
        with temporary:
            self._write_baseline(root, bounded_module=["Radroots/Missing.swift"])
            with self.assertRaisesRegex(ratchet.MaintainabilityError, "absent"):
                ratchet.verify(root)
            (root / "Radroots/App.swift").write_text("x\n" * 601, encoding="utf-8")
            self._write_baseline(
                root,
                swift_file_exception=[
                    {"path": "Radroots/App.swift", "maximum_lines": 601}
                ],
            )
            with self.assertRaisesRegex(ratchet.MaintainabilityError, "bounded"):
                ratchet.verify(root)

    def test_exception_rows_are_exact_unique_and_ordered(self) -> None:
        invalid = [
            [
                {"path": "b", "maximum_lines": 601},
                {"path": "a", "maximum_lines": 601},
            ],
            [
                {"path": "a", "maximum_lines": 601},
                {"path": "a", "maximum_lines": 602},
            ],
            [{"path": "a", "maximum_lines": 601, "extra": True}],
        ]
        for rows in invalid:
            with self.subTest(rows=rows):
                with self.assertRaises(ratchet.MaintainabilityError):
                    ratchet._closed_exception_map(
                        rows, key_name="path", ceiling_name="maximum_lines"
                    )


if __name__ == "__main__":
    unittest.main()
