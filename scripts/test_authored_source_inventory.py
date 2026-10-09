"""Actual omitted roots, nested source and lane agreement regressions."""

import tempfile
import unittest
from pathlib import Path

from scripts import authored_source_inventory as inventory


class AuthoredSourceInventoryTests(unittest.TestCase):
    def repository(self, root):
        (root / "Package.swift").write_text("// package\n")
        for relative in inventory.SWIFT_ROOTS:
            (root / relative).mkdir(parents=True, exist_ok=True)
            (root / relative / "Included.swift").write_text("// authored\n")
        (root / "scripts/nested").mkdir()
        (root / "scripts/helper.py").write_text("# source\n")
        (root / "scripts/nested/test_added.py").write_text("# test\n")

    def test_all_authored_roots_and_nested_python_are_included(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.repository(root)
            swift, python = inventory.source_files(root)
            self.assertEqual(
                set(swift),
                {
                    root / "Package.swift",
                    *(root / path / "Included.swift" for path in inventory.SWIFT_ROOTS),
                },
            )
            self.assertEqual(
                python,
                [root / "scripts/helper.py", root / "scripts/nested/test_added.py"],
            )

    def test_only_generated_and_cache_outputs_are_excluded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.repository(root)
            generated = root / inventory.GENERATED_SWIFT
            generated.mkdir()
            (generated / "Bindings.swift").write_text("// generated\n")
            for name in inventory.IGNORED_DIRECTORIES:
                cache = root / "scripts" / name
                cache.mkdir()
                (cache / "test_external.py").write_text("# external\n")
            swift, python = inventory.source_files(root)
            self.assertNotIn(generated / "Bindings.swift", swift)
            self.assertEqual(len(python), 2)

    def test_authored_file_and_directory_links_are_rejected(self):
        for directory_link in [False, True]:
            with (
                self.subTest(directory_link=directory_link),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                self.repository(root)
                link = root / "scripts" / ("linked" if directory_link else "linked.py")
                target = root / "Tera" if directory_link else root / "scripts/helper.py"
                link.symlink_to(target, target_is_directory=directory_link)
                with self.assertRaises(inventory.InventoryError):
                    inventory.source_files(root)

    def test_missing_authored_root_cannot_silently_shrink_inventory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.repository(root)
            (root / "TeraPublicAPITests/Included.swift").unlink()
            (root / "TeraPublicAPITests").rmdir()
            with self.assertRaises(inventory.InventoryError):
                inventory.source_files(root)

    def test_current_quality_and_test_lanes_share_complete_inventory(self):
        root = Path(__file__).resolve().parent.parent
        _, python = inventory.source_files(root)
        paths = [path.relative_to(root).as_posix() for path in python]
        self.assertEqual(inventory.command(root, "format")[3:], paths)
        self.assertEqual(inventory.command(root, "lint")[2:], paths)
        self.assertEqual(
            inventory.command(root, "test")[3:],
            [path for path in paths if Path(path).name.startswith("test_")],
        )
        self.assertIn(
            "scripts/test_maintainability_ratchet.py", inventory.command(root, "test")
        )
        self.assertIn(
            "scripts/test_ffi_native_cache.py", inventory.command(root, "test")
        )
        self.assertIn(
            "scripts/local-social-fixture.py", inventory.command(root, "lint")
        )


if __name__ == "__main__":
    unittest.main()
