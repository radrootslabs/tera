"""Actual release shell branches with synthetic dependency/artifact dispatch."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))


class ReleaseFixture(unittest.TestCase):
    def setUp(self) -> None:
        evidence = os.environ.get("TERA_C138_TEST_EVIDENCE")
        if evidence:
            self.root = Path(tempfile.mkdtemp(prefix="release-", dir=evidence))
        else:
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            self.root = Path(temporary.name).resolve()
        self.tools = self.root / "tools"
        self.tools.mkdir()
        (self.root / "scripts").mkdir()
        for name in (
            "release-evidence.sh",
            "app_source.py",
            "ffi_source.py",
            "package_contract.py",
            "app_dependency_graph.py",
            "legacy_identifiers.py",
            "package_privacy.py",
        ):
            path = SCRIPTS / name
            if path.exists():
                shutil.copyfile(path, self.root / "scripts" / name)
        for tool in (
            "jq",
            "shasum",
            "awk",
            "mktemp",
            "rm",
            "mkdir",
            "install",
            "cmp",
            "dirname",
            "git",
            "rg",
        ):
            resolved = shutil.which(tool)
            self.assertIsNotNone(resolved, f"fixture requires {tool}")
            (self.tools / tool).symlink_to(resolved)
        metadata = {
            "packages": [
                {
                    "id": "ffi",
                    "name": "tera_ffi",
                    "version": "0.1.0-alpha",
                    "source": None,
                    "license": "GPL-3.0-or-later",
                }
            ],
            "resolve": {"nodes": [{"id": "ffi", "dependencies": []}]},
            "workspace_members": ["ffi"],
        }
        self.write("metadata.json", json.dumps(metadata))
        self.executable(
            "cargo",
            "import os\nfrom pathlib import Path\n"
            "print(Path(os.environ['FIXTURE_METADATA']).read_text())\n",
        )
        self.executable(
            "uv",
            "import json, os, sys\nfrom pathlib import Path\n"
            "args = sys.argv[1:]\n"
            "expected = ['run', '--project', os.environ['FIXTURE_PROJECT'], "
            "'--offline', '--frozen', 'python']\n"
            "if args[:6] != expected: sys.exit(64)\n"
            "with Path(os.environ['FIXTURE_UV_LOG']).open('a') as out:\n"
            "    out.write(json.dumps(args) + '\\n')\n"
            "os.execv(sys.executable, [sys.executable, *args[6:]])\n",
        )
        installer = shutil.which("install")
        self.executable(
            "install",
            "import os, sys\nfrom pathlib import Path\n"
            "with Path(os.environ['FIXTURE_INSTALL_LOG']).open('a') as out:\n"
            "    out.write('install\\n')\n"
            f"os.execv({installer!r}, [{installer!r}, *sys.argv[1:]])\n",
        )
        self.write("Package.swift", "// fixture package\n")
        self.write("project.yml", "name: Tera\n")
        self.write(
            "Tera/Runtime/TeraGeneratedFixture.swift",
            "private func value() -> Int { 1 }\n",
        )
        self.write("Tera/Resources/message.txt", "fixture resource\n")
        self.write("Tera/Config/Debug.xcconfig", "FIXTURE = one\n")
        self.write("Tera/Info.plist", "fixture plist\n")
        self.write("Tera/Resources/PrivacyInfo.xcprivacy", "fixture privacy\n")
        self.write("Tera/Generated/Fixture.swift", "// synthetic generated binding\n")
        self.write("Tera/Frameworks/Fixture.a", "synthetic native bytes\n")
        self.write("Cargo.lock", "synthetic locked graph\n")
        shutil.copyfile(
            SCRIPTS.parent / "Package.resolved", self.root / "Package.resolved"
        )
        self.write(
            "Tera.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved",
            (self.root / "Package.resolved").read_text(),
        )
        self.write("Tera.xcodeproj/project.pbxproj", "synthetic generated project\n")
        self.write("TeraFFI/api/TeraKitBindings.symbols.json", "{}\n")
        self.write("api/TeraApp.symbols.json", "{}\n")
        self.write(
            "TeraFFI/provenance.json",
            json.dumps({"candidate": {"source": {"tree": "b" * 40}}}),
        )
        # Existing public producer paths are data, not newly invented identities.
        import ffi_source

        shutil.copyfile(
            SCRIPTS.parent / ffi_source.INPUTS[-1], self.root / ffi_source.INPUTS[-1]
        )
        self.epoch(1787871027)
        self.git("init", "--quiet")
        self.git("add", ".")
        self.environment = {
            **os.environ,
            "PATH": str(self.tools),
            "TMPDIR": str(self.root),
            "FIXTURE_METADATA": str(self.root / "metadata.json"),
            "FIXTURE_PROJECT": str(self.root / "scripts/persona-verifier"),
            "FIXTURE_UV_LOG": str(self.root / "uv.jsonl"),
            "FIXTURE_INSTALL_LOG": str(self.root / "install.log"),
        }
        self.commands = 0

    def write(self, relative: str, text: str) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def executable(self, name: str, body: str) -> None:
        path = self.tools / name
        if path.is_symlink():
            path.unlink()
        path.write_text(f"#!{sys.executable}\n" + body)
        path.chmod(0o700)

    def git(self, *arguments: str) -> str:
        return subprocess.check_output(
            ["git", *arguments], cwd=self.root, text=True, stderr=subprocess.DEVNULL
        ).strip()

    def epoch(self, value: int) -> None:
        self.write("TeraFFI/producer.toml", f"[build]\nsource_date_epoch = {value}\n")
        self.write(
            "TeraFFI/source/aarch64-apple-ios.json",
            json.dumps(
                {
                    "foundation": {"revision": "a" * 40},
                    "build": {"source_date_epoch": value},
                }
            ),
        )

    def run_release(self, mode: str) -> subprocess.CompletedProcess:
        argv = ["/bin/sh", str(self.root / "scripts/release-evidence.sh"), mode]
        result = subprocess.run(
            argv,
            cwd=self.root,
            env=self.environment,
            capture_output=True,
            check=False,
            timeout=30,
        )
        self.commands += 1
        self.write(
            f"command-{self.commands:02}.json",
            json.dumps(
                {
                    "argv": argv,
                    "cwd": str(self.root),
                    "exit": result.returncode,
                    "stdout": result.stdout.decode(),
                    "stderr": result.stderr.decode(),
                }
            ),
        )
        return result

    def prior(self) -> dict[str, bytes]:
        return {
            name: (self.root / "release" / name).read_bytes()
            for name in ("sbom.cdx.json", "provenance.json")
        }

    def baseline(self) -> dict[str, bytes]:
        result = self.run_release("write")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.installs = (self.root / "install.log").read_bytes()
        return self.prior()

    def assert_rejected_unchanged(self, mode: str, prior: dict[str, bytes]) -> None:
        result = self.run_release(mode)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(self.prior(), prior)
        self.assertEqual((self.root / "install.log").read_bytes(), self.installs)


class ReleaseEvidenceTests(ReleaseFixture):
    def assert_staged_change_stales(self, relative: str, text: str) -> None:
        prior = self.baseline()
        self.write(relative, text)
        self.git("add", relative)
        self.assert_rejected_unchanged("check", prior)

    def test_private_body_only_staged_change_invalidates_release_evidence(self) -> None:
        self.assert_staged_change_stales(
            "Tera/Runtime/TeraGeneratedFixture.swift",
            "private func value() -> Int { 2 }\n",
        )

    def test_resource_change_invalidates_release_evidence(self) -> None:
        self.assert_staged_change_stales("Tera/Resources/message.txt", "new resource\n")

    def test_configuration_change_invalidates_release_evidence(self) -> None:
        self.assert_staged_change_stales(
            "Tera/Config/Debug.xcconfig", "FIXTURE = two\n"
        )

    def test_staged_add_remove_invalidates_release_evidence(self) -> None:
        prior = self.baseline()
        self.write("Tera/Runtime/Added.swift", "private func added() {}\n")
        self.git("add", "Tera/Runtime/Added.swift")
        self.assert_rejected_unchanged("check", prior)
        prior = self.baseline()
        self.git("rm", "-f", "Tera/Runtime/Added.swift")
        self.assert_rejected_unchanged("check", prior)

    def test_package_and_project_manifest_edits_invalidate_release_evidence(
        self,
    ) -> None:
        for relative in ("Package.swift", "project.yml"):
            with self.subTest(relative=relative):
                self.assert_staged_change_stales(
                    relative, "changed authored manifest\n"
                )

    def test_app_or_epoch_drift_during_rendering_rejects_before_output_change(
        self,
    ) -> None:
        prior = self.baseline()
        control = self.root / "drift.json"
        self.environment["FIXTURE_DRIFT"] = str(control)
        self.executable(
            "cargo",
            "import json, os, subprocess\nfrom pathlib import Path\n"
            "control = Path(os.environ['FIXTURE_DRIFT'])\n"
            "if control.exists():\n"
            "    value = json.loads(control.read_text())\n"
            "    Path(value['path']).write_text(value['text'])\n"
            "    subprocess.run(['git', 'add', value['path']], check=True)\n"
            "print(Path(os.environ['FIXTURE_METADATA']).read_text())\n",
        )
        for relative, text in (
            (
                "Tera/Runtime/TeraGeneratedFixture.swift",
                "private func value() -> Int { 2 }\n",
            ),
            ("TeraFFI/producer.toml", "[build]\nsource_date_epoch = 12345\n"),
        ):
            original = (self.root / relative).read_bytes()
            for mode in ("write", "check"):
                with self.subTest(relative=relative, mode=mode):
                    self.write(
                        "drift.json",
                        json.dumps({"path": str(self.root / relative), "text": text}),
                    )
                    self.assert_rejected_unchanged(mode, prior)
                    (self.root / relative).write_bytes(original)
                    self.git("add", relative)
            control.unlink()


class ReleaseScannerTests(ReleaseFixture):
    def test_clean_deterministic_write_and_check_bind_canonical_epoch(self) -> None:
        self.epoch(12345)
        self.git("add", "TeraFFI")
        prior = self.baseline()
        self.assertEqual(self.run_release("check").returncode, 0)
        self.assertEqual(self.run_release("write").returncode, 0)
        self.assertEqual(self.prior(), prior)
        output = json.loads(prior["provenance.json"])
        self.assertEqual(output["source"]["source_date_epoch"], 12345)
        self.assertEqual(output["schema"], "tera.release-provenance.v1")
        self.assertEqual(output["source"]["authored_app"]["policy"], "staged_inputs")
        self.assertIn(
            "Tera/Runtime/TeraGeneratedFixture.swift",
            output["source"]["authored_app"]["files"],
        )

    def test_missing_scanner_rejects_write_and_check_preserving_prior_bytes(
        self,
    ) -> None:
        prior = self.baseline()
        (self.tools / "rg").unlink()
        for mode in ("write", "check"):
            with self.subTest(mode=mode):
                self.assert_rejected_unchanged(mode, prior)

    def test_scanner_status2_rejects_write_and_check_preserving_prior_bytes(
        self,
    ) -> None:
        prior = self.baseline()
        self.executable("rg", "import sys\nsys.exit(2)\n")
        for mode in ("write", "check"):
            with self.subTest(mode=mode):
                self.assert_rejected_unchanged(mode, prior)

    def test_forbidden_material_rejects_without_printing_it_or_changing_outputs(
        self,
    ) -> None:
        prior = self.baseline()
        metadata = json.loads((self.root / "metadata.json").read_text())
        sentinel = "BEGIN PRIVATE KEY C138_PROTECTED_SENTINEL"
        metadata["packages"][0]["source"] = sentinel
        self.write("metadata.json", json.dumps(metadata))
        for mode in ("write", "check"):
            with self.subTest(mode=mode):
                result = self.run_release(mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.prior(), prior)
                self.assertNotIn(sentinel.encode(), result.stdout + result.stderr)

    def test_unreadable_rendered_input_rejects_write_and_check_preserving_prior_bytes(
        self,
    ) -> None:
        prior = self.baseline()
        scanner = shutil.which("rg")
        self.executable(
            "rg",
            "import os, sys\n"
            "import subprocess\n"
            "os.chmod(sys.argv[-1], 0)\n"
            f"status = subprocess.run([{scanner!r}, *sys.argv[1:]], check=False).returncode\n"
            "os.chmod(sys.argv[-1], 0o644)\n"
            "sys.exit(status)\n",
        )
        for mode in ("write", "check"):
            with self.subTest(mode=mode):
                self.assert_rejected_unchanged(mode, prior)


if __name__ == "__main__":
    unittest.main()
