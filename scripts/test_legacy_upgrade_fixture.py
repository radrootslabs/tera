from __future__ import annotations

import copy
import io
import tempfile
import unittest
import sys
from pathlib import Path
from unittest.mock import patch
import tarfile

SCRIPTS = Path(__file__).resolve().parent
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import legacy_upgrade_fixture_producer as producer  # noqa: E402
import legacy_upgrade_fixture_admission as admission  # noqa: E402


class HistoricalProducerBoundaryTests(unittest.TestCase):
    def test_freeze_refuses_unadmitted_changed_or_escaped_bytes_before_creating_destination(
        self,
    ):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fixture_root = root / "test-fixtures"
            fixture_root.mkdir()
            work = root / "managed"
            (work / "output").mkdir(parents=True)
            content = b"synthetic guard unit bytes, not a historical owner fixture"
            (work / "output" / "guard.txt").write_bytes(content)
            (work / "output" / "link.txt").symlink_to(work / "output" / "guard.txt")
            (root / "outside").mkdir()
            (root / "outside" / "guard.txt").write_bytes(content)
            (work / "output" / "parent_link").symlink_to(
                root / "outside", target_is_directory=True
            )
            destination = fixture_root / "legacy_upgrade_v1"
            record = {
                "state": "HISTORICAL_WRITER_ADMITTED_CURRENT_READERS_PENDING",
                "files": {"guard.txt": producer.digest(content)},
            }
            with patch.object(
                producer, "__file__", str(root / "scripts" / "producer.py")
            ):
                for changed in [
                    {**record, "state": "UNADMITTED"},
                    {**record, "files": {"guard.txt": "0" * 64}},
                    {**record, "files": {"../outside": "0" * 64}},
                    {**record, "files": {"link.txt": producer.digest(content)}},
                    {
                        **record,
                        "files": {"parent_link/guard.txt": producer.digest(content)},
                    },
                ]:
                    with self.assertRaises(ValueError):
                        producer.freeze_output(work, changed, destination)
                    self.assertFalse(destination.exists())
                producer.freeze_output(work, record, destination)
                self.assertEqual((destination / "guard.txt").read_bytes(), content)
                with self.assertRaisesRegex(ValueError, "fresh canonical"):
                    producer.freeze_output(work, record, destination)

    def test_source_root_must_be_its_exact_public_git_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            replies = [str(root).encode(), b"https://github.com/unrelated/lib.git"]
            with patch.object(producer, "git", side_effect=replies):
                with self.assertRaisesRegex(ValueError, "identity"):
                    producer.validate_root(root, "lib")
            with patch.object(producer, "git", return_value=str(root.parent).encode()):
                with self.assertRaisesRegex(ValueError, "Git root"):
                    producer.validate_root(root, "lib")

    def test_historical_commit_must_be_exact_and_published(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            replies = [
                str(root).encode(),
                b"https://github.com/radrootslabs/lib.git",
                b"0" * 40,
            ]
            with patch.object(producer, "git", side_effect=replies):
                with self.assertRaisesRegex(ValueError, "unavailable"):
                    producer.validate_root(root, "lib")
            replies[-1] = producer.SOURCES["lib"][0].encode()
            with patch.object(producer, "git", side_effect=replies):
                with patch.object(
                    producer.subprocess,
                    "run",
                    side_effect=RuntimeError("not published"),
                ):
                    with self.assertRaisesRegex(RuntimeError, "published"):
                        producer.validate_root(root, "lib")
            with patch.object(
                producer.subprocess, "check_output", return_value=b"record"
            ) as command:
                producer.git(root, "rev-parse", "HEAD")
                command.assert_called_once_with(
                    [
                        "git",
                        "--no-replace-objects",
                        "-C",
                        str(root),
                        "rev-parse",
                        "HEAD",
                    ]
                )

    def test_historical_archive_refuses_traversal_links_and_devices(self):
        for name, kind in [
            ("../escaped", tarfile.REGTYPE),
            ("/absolute", tarfile.REGTYPE),
            ("link", tarfile.SYMTYPE),
            ("hard", tarfile.LNKTYPE),
            ("device", tarfile.CHRTYPE),
        ]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                buffer = io.BytesIO()
                with tarfile.open(fileobj=buffer, mode="w") as archive:
                    entry = tarfile.TarInfo(name)
                    entry.type = kind
                    archive.addfile(entry)
                with patch.object(producer, "git", return_value=buffer.getvalue()):
                    with self.assertRaisesRegex(ValueError, "unsupported"):
                        producer.archive(Path(temporary), "lib", Path(temporary))

    def test_work_must_be_a_fresh_child_of_managed_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            managed = Path(temporary).resolve()
            with patch.dict(
                producer.os.environ, {"CARGO_TARGET_DIR": str(managed / "target")}
            ):
                for destination in [managed, managed.parent / "outside"]:
                    with (
                        self.subTest(destination=destination),
                        self.assertRaisesRegex(ValueError, "fresh child"),
                    ):
                        producer.produce(
                            {}, destination, Path("unused.swift"), Path("unused.rs")
                        )
                used = managed / "already_used"
                used.mkdir()
                with self.assertRaises(FileExistsError):
                    producer.produce({}, used, Path("unused.swift"), Path("unused.rs"))


class HistoricalAdmissionBoundaryTests(unittest.TestCase):
    def test_pending_wal_and_nonregular_owner_refuse_before_sqlite_open(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            database = root / "runtime.sqlite"
            database.write_bytes(b"negative unopened guard input")
            database.with_name(database.name + "-wal").write_bytes(b"pending")
            with patch.object(admission.sqlite3, "connect") as connect:
                with self.assertRaisesRegex(ValueError, "uncheckpointed WAL"):
                    admission.read_database(database)
                link = root / "linked.sqlite"
                link.symlink_to(database)
                with self.assertRaisesRegex(ValueError, "regular closed file"):
                    admission.read_database(link)
                with self.assertRaisesRegex(ValueError, "regular closed file"):
                    admission.read_database(root / "missing.sqlite")
                connect.assert_not_called()

    def test_zero_duplicate_missing_and_skipped_case_logs_cannot_be_admitted(self):
        templates = {
            "historical-host": "Test Case '-[TeraHistoricalFixtureWriterTests testProduceHistoricalOwnerBytes]' passed (0.1 seconds).\nExecuted 1 test, with 0 failures\n",
            "historical-rust": "test produce_pre_refactor_owner_state ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n",
            "historical-transfer": "Test Case '-[TeraHistoricalFixtureWriterTests testProduceHistoricalOwnerBytes]' passed (0.1 seconds).\nExecuted 1 test, with 0 failures\n",
        }
        with tempfile.TemporaryDirectory() as temporary:
            work = Path(temporary)
            for name, raw in templates.items():
                (work / (name + ".txt")).write_text(raw)
            self.assertEqual(len(admission.admit_case_logs(work)), 3)
            for name, raw in templates.items():
                for malformed in [
                    "",
                    raw + raw,
                    raw.replace("passed", "skipped").replace("... ok", "... ignored"),
                    raw.replace("1 passed", "0 passed").replace(
                        "Executed 1", "Executed 0"
                    ),
                ]:
                    with self.subTest(name=name, malformed=malformed):
                        (work / (name + ".txt")).write_text(malformed)
                        with self.assertRaises(ValueError):
                            admission.admit_case_logs(work)
                (work / (name + ".txt")).write_text(raw)

    def test_changed_public_generation_and_installed_identity_refuse(self):
        host = {
            "source_generation": "fixture_generation",
            "bundle_identifier": "dev.local.radroots",
            "keychain_service_prefix": "org.radroots.field_ios.local",
        }
        rust = {"public_key": "ab" * 32}
        native = {
            "public_key": rust["public_key"],
            "source_generation": host["source_generation"],
        }
        self.assertEqual(
            admission.admit_identities(host, rust, native), rust["public_key"]
        )
        for field in ["source_generation", "public_key"]:
            value = copy.deepcopy(native)
            value[field] = "different"
            with self.subTest(field=field), self.assertRaises(ValueError):
                admission.admit_identities(host, rust, value)
        for field in ["bundle_identifier", "keychain_service_prefix"]:
            value = copy.deepcopy(host)
            value[field] = "different"
            with self.subTest(field=field), self.assertRaises(ValueError):
                admission.admit_identities(value, rust, native)
        for public in ["ab" * 31, "AB" * 32, "z" * 64]:
            with self.subTest(public=public), self.assertRaises(ValueError):
                admission.admit_identities(host, {"public_key": public}, native)

    def test_media_escape_hash_size_and_receipt_state_refuse(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "output"
            output.mkdir()
            content = (
                b"synthetic byte-boundary input, not a historical positive fixture"
            )
            (output / "blob").write_bytes(content)
            (root / "outside").write_bytes(content)
            host = {
                "staged_relative_path": "blob",
                "media_sha256": admission.sha256(content),
                "media_bytes": len(content),
            }
            native = {
                "media_sha256": host["media_sha256"],
                "state": "awaitingVerification",
            }
            admission.admit_media(output, host, native)
            for field, value in [
                ("staged_relative_path", "../outside"),
                ("media_sha256", "0" * 64),
                ("media_bytes", len(content) + 1),
            ]:
                changed = {**host, field: value}
                with self.subTest(field=field), self.assertRaises(ValueError):
                    admission.admit_media(output, changed, native)
            with self.assertRaises(ValueError):
                admission.admit_media(output, host, {**native, "state": "completed"})
            (output / "link").symlink_to(output / "blob")
            with self.assertRaises(ValueError):
                admission.admit_media(
                    output, {**host, "staged_relative_path": "link"}, native
                )

    def test_wrong_inventory_head_or_missing_credential_scan_refuses(self):
        rows = [
            {
                "draft_id": str(index),
                "revision": 1,
                "author_public_key": "public",
                "operation_id": None,
            }
            for index in range(106)
        ]
        rust = {
            "drafts": [
                {"draft_id": row["draft_id"], "revision": 1, "operation_id": None}
                for row in rows
            ],
            "actual_first_page_count": 100,
            "pending_media_outside_first_page": True,
            "secret_scan": True,
            "authorization_scan": True,
        }
        admission.admit_statuses(rust, rows, "public")
        for field, value in [
            ("actual_first_page_count", 99),
            ("pending_media_outside_first_page", False),
            ("secret_scan", False),
            ("authorization_scan", False),
        ]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                admission.admit_statuses({**rust, field: value}, rows, "public")
        damaged = copy.deepcopy(rows)
        damaged[0]["revision"] = 2
        with self.assertRaises(ValueError):
            admission.admit_statuses(rust, damaged, "public")


if __name__ == "__main__":
    unittest.main()
