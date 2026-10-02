"""Admit closed historical owner output and derive explicit refusal inputs."""

from __future__ import annotations

from contextlib import closing, contextmanager
import hashlib
import json
import re
import shutil
import sqlite3
from pathlib import Path


def sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def load_object(path: Path) -> dict:
    value = json.loads(path.read_bytes())
    if not isinstance(value, dict):
        raise ValueError("Historical owner metadata is not an object")
    return value


@contextmanager
def closed_owner_reader(path: Path):
    if not path.is_file() or path.is_symlink():
        raise ValueError("Historical owner database is not a regular closed file")
    wal = path.with_name(path.name + "-wal")
    if wal.exists() and wal.stat().st_size:
        raise ValueError("Historical owner database has an uncheckpointed WAL")
    with closing(
        sqlite3.connect(path.as_uri() + "?mode=ro&immutable=1", uri=True)
    ) as connection:
        yield connection


def read_database(path: Path) -> dict:
    with closed_owner_reader(path) as connection:
        if connection.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
            raise ValueError("Historical owner database integrity differs")
        version = connection.execute("PRAGMA user_version").fetchone()[0]
        identity = connection.execute("PRAGMA application_id").fetchone()[0]
        catalog = connection.execute(
            "SELECT type,name,tbl_name,sql FROM sqlite_master "
            "WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name"
        ).fetchall()
    return {
        "user_version": version,
        "application_id": identity,
        "catalog_sha256": sha256(json.dumps(catalog).encode()),
    }


def actual_revisions(path: Path) -> tuple[list[dict], list[str]]:
    with closed_owner_reader(path) as connection:
        rows = connection.execute(
            "SELECT hex(draft_id),revision,hex(author),hex(operation_id),"
            "hex(payload_sha256),snapshot FROM radroots_runtime_authored_draft_revisions "
            "ORDER BY draft_id,revision"
        ).fetchall()
        signed = connection.execute(
            "SELECT signed_raw_json FROM radroots_runtime_authored_artifacts "
            "WHERE signed_raw_json IS NOT NULL ORDER BY artifact_id"
        ).fetchall()
    revisions = []
    for draft, revision, author, operation, payload_sha, snapshot in rows:
        value = json.loads(bytes(snapshot))
        payload = bytes(value["payload"])
        if sha256(payload) != payload_sha.lower():
            raise ValueError("Historical authored payload hash differs")
        if (
            value["revision"] != revision
            or bytes(value["draft_id"]).hex() != draft.lower()
        ):
            raise ValueError("Historical authored revision identity differs")
        revisions.append(
            {
                "draft_id": draft.lower(),
                "revision": revision,
                "author_public_key": author.lower(),
                "operation_id": operation.lower() or None,
                "payload_schema": value["payload_schema"],
                "payload_sha256": payload_sha.lower(),
                "snapshot_sha256": sha256(bytes(snapshot)),
            }
        )
    return revisions, [bytes(row[0]).decode() for row in signed]


def admit_case_logs(work: Path) -> dict:
    expected = {
        "historical-host": "testProduceHistoricalOwnerBytes",
        "historical-rust": "produce_pre_refactor_owner_state",
        "historical-transfer": "testProduceHistoricalOwnerBytes",
    }
    receipts = {}
    for name, method in expected.items():
        path = work / (name + ".txt")
        raw = path.read_text()
        pattern = (
            r"^test " + method + r" \.\.\. ok$"
            if name == "historical-rust"
            else r"^Test Case .*" + method + r".* passed \("
        )
        summary = (
            "test result: ok. 1 passed; 0 failed; 0 ignored;"
            if name == "historical-rust"
            else "Executed 1 test, with 0 failures"
        )
        if len(re.findall(pattern, raw, re.M)) != 1 or summary not in raw:
            raise ValueError(
                "Historical producer did not execute the required actual case: " + name
            )
        receipts[name] = {"method": method, "log_sha256": sha256(path.read_bytes())}
    return receipts


def admit_identities(host: dict, rust: dict, native: dict) -> str:
    public_key = rust["public_key"]
    if len(public_key) != 64 or bytes.fromhex(public_key).hex() != public_key:
        raise ValueError("Historical public identity is not canonical")
    if (
        native["public_key"] != public_key
        or native["source_generation"] != host["source_generation"]
    ):
        raise ValueError("Historical native and Rust owner identities differ")
    if (
        host["bundle_identifier"] != "dev.local.radroots"
        or host["keychain_service_prefix"] != "org.radroots.field_ios.local"
    ):
        raise ValueError("Historical installed identity differs")
    return public_key


def admit_media(output: Path, host: dict, native: dict) -> None:
    media = output / host["staged_relative_path"]
    if not media.resolve().is_relative_to(output.resolve()) or media.is_symlink():
        raise ValueError("Historical staged media escaped its fixture root")
    if (
        sha256(media.read_bytes()) != host["media_sha256"]
        or media.stat().st_size != host["media_bytes"]
    ):
        raise ValueError("Historical actual prepared media differs")
    if (
        native["media_sha256"] != host["media_sha256"]
        or native["state"] != "awaitingVerification"
    ):
        raise ValueError("Historical typed native receipt differs")


def admit_databases(
    output: Path, public_key: str
) -> tuple[dict, list[dict], list[str]]:
    owner = output / "data" / "radroots" / "users" / public_key
    runtime = owner / "runtime.sqlite"
    databases = {
        "runtime": read_database(runtime),
        "private": read_database(owner / "private.sqlite"),
    }
    if (
        databases["runtime"]["user_version"] != 13
        or databases["runtime"]["application_id"] != 1380209236
    ):
        raise ValueError(
            "Historical runtime schema is not the expected pre-refactor owner"
        )
    if databases["private"]["application_id"] != 1380208722:
        raise ValueError("Historical private store identity differs")
    revisions, signed = actual_revisions(runtime)
    if len(signed) != 1 or json.loads(signed[0])["kind"] != 1:
        raise ValueError(
            "Historical output must contain one signed synthetic Update and no authorization event"
        )
    return databases, revisions, signed


def admit_statuses(rust: dict, revisions: list[dict], public_key: str) -> None:
    heads = {row["draft_id"]: row for row in revisions}
    statuses = rust["drafts"]
    if (
        len(statuses) != 106
        or rust["actual_first_page_count"] != 100
        or not rust["pending_media_outside_first_page"]
    ):
        raise ValueError("Historical hidden pending media inventory was not exercised")
    for status in statuses:
        head = heads[status["draft_id"]]
        if (head["revision"], head["author_public_key"], head["operation_id"]) != (
            status["revision"],
            public_key,
            status["operation_id"],
        ):
            raise ValueError(
                "Historical returned status differs from closed owner bytes"
            )
    if not rust.get("secret_scan") or not rust.get("authorization_scan"):
        raise ValueError("Historical writer did not complete credential scans")


def admit_output(output: Path, work: Path) -> dict:
    host = load_object(output / "host-metadata.json")
    rust = load_object(output / "rust-metadata.json")
    native = load_object(output / "native-metadata.json")
    public_key = admit_identities(host, rust, native)
    admit_media(output, host, native)
    databases, revisions, signed = admit_databases(output, public_key)
    admit_statuses(rust, revisions, public_key)
    return {
        "state": "HISTORICAL_WRITER_ADMITTED_CURRENT_READERS_PENDING",
        "actual_case_receipts": admit_case_logs(work),
        "databases": databases,
        "revisions": revisions,
        "draft_count": 106,
        "signed_event": {
            "event_id": json.loads(signed[0])["id"],
            "raw_json": signed[0],
            "raw_sha256": sha256(signed[0].encode()),
        },
        "limits": "Actual historical synthetic owner bytes; no real account, credential, OS enqueue, device or external delivery.",
    }


def derive_refusal_inputs(output: Path, public_key: str) -> dict:
    """Only negative copies change; record both actual parent and derived hashes."""
    source = output / "data" / "radroots" / "users" / public_key / "runtime.sqlite"
    statements = {
        "unsupported_version": "PRAGMA user_version = 18",
        "damaged_catalog": "DROP TRIGGER radroots_runtime_authored_draft_revisions_update_guard",
    }
    derivatives = {}
    for name, statement in statements.items():
        destination = output / "refusal_inputs" / name / "runtime.sqlite"
        destination.parent.mkdir(parents=True, exist_ok=False)
        shutil.copyfile(source, destination)
        with closing(sqlite3.connect(destination)) as connection:
            with connection:
                connection.execute(statement)
        derivatives[name] = {
            "source_sha256": sha256(source.read_bytes()),
            "derived_sha256": sha256(destination.read_bytes()),
            "only_mutation": statement,
            "purpose": "Explicit negative startup-refusal input, never a positive historical write.",
        }
    return derivatives
