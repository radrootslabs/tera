"""Select measured persona evidence from a bounded native audit export."""

import json
from pathlib import Path
import re
import tempfile
from typing import Any, Callable, NamedTuple

AUDIT_UUID = r"[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}"
AUDIT_NAME = re.compile(
    rf"^(?:Screen before audit_0_{AUDIT_UUID}\.png|"
    rf"Accessibility tree before audit_0_{AUDIT_UUID}\.txt)$"
)
REQUIRED_ROW_KEYS = {
    "exportedFileName",
    "suggestedHumanReadableName",
    "isAssociatedWithFailure",
    "configurationName",
    "deviceName",
    "deviceId",
}
ALLOWED_ROW_KEYS = REQUIRED_ROW_KEYS | {
    "timestamp",
    "repetitionNumber",
    "arguments",
}


class ExtractionTools(NamedTuple):
    run_json: Callable
    exact_node: Callable
    read_json: Callable
    inventory: Callable
    digest: Callable
    load_selected: Callable
    attempt_id: Callable
    node_identifier: str
    node_url: str
    names: tuple[str, ...]
    maximum_json_bytes: int
    maximum_attempt_bytes: int
    run_export: Callable


def filename(value: object) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError("xcresult exported filename is invalid")
    if len(value.encode("utf-8")) > 255 or Path(value).name != value:
        raise ValueError("xcresult exported filename is invalid")
    return value


def row_identity(value: Any) -> tuple[str, str]:
    if not isinstance(value, dict):
        raise ValueError("xcresult attachment row is invalid")
    if not REQUIRED_ROW_KEYS <= value.keys() <= ALLOWED_ROW_KEYS:
        raise ValueError("xcresult attachment row is invalid")
    if (
        value["isAssociatedWithFailure"] is not False
        or value["configurationName"] != "Test Scheme Action"
    ):
        raise ValueError("xcresult attachment context is invalid")
    if not all(
        isinstance(value[k], str) and value[k]
        for k in ["deviceName", "deviceId", "suggestedHumanReadableName"]
    ):
        raise ValueError("xcresult attachment context is invalid")
    return filename(value["exportedFileName"]), value["suggestedHumanReadableName"]


def group_value(value: Any, tools: ExtractionTools) -> dict:
    if not isinstance(value, list) or len(value) != 1:
        raise ValueError("xcresult attachment manifest is invalid")
    group = value[0]
    if not isinstance(group, dict) or set(group) != {
        "testIdentifier",
        "testIdentifierURL",
        "attachments",
    }:
        raise ValueError("xcresult attachment manifest is invalid")
    if (
        group["testIdentifier"] != tools.node_identifier
        or group["testIdentifierURL"] != tools.node_url
    ):
        raise ValueError("xcresult attachment test binding is invalid")
    if not isinstance(group["attachments"], list):
        raise ValueError("xcresult attachment inventory is invalid")
    return group


def selected_rows(
    group: dict, inventory: set[str], tools: ExtractionTools
) -> list[dict]:
    selected, seen = [], set()
    for row in group["attachments"]:
        exported, name = row_identity(row)
        if exported in seen:
            raise ValueError("xcresult attachment export identity is duplicated")
        seen.add(exported)
        if tools.attempt_id(name) is not None:
            selected.append(row)
        elif AUDIT_NAME.fullmatch(name) is None:
            raise ValueError("xcresult contains an unknown diagnostic attachment")
    if inventory != {"manifest.json", *seen}:
        raise ValueError("xcresult raw export inventory is invalid")
    if len(selected) != len(tools.names):
        raise ValueError("xcresult requires exactly fifteen measured attempts")
    devices = {(r["deviceName"], r["deviceId"]) for r in group["attachments"]}
    if len(devices) != 1:
        raise ValueError("xcresult diagnostics do not bind the attempt device")
    return selected


def flat_inventory(path: Path, tools: ExtractionTools) -> set[str]:
    values = tools.inventory(path)
    if any(kind != 1 or b"/" in name for kind, name, _ in values):
        raise ValueError("xcresult raw export inventory is invalid")
    return {name.decode("utf-8") for _, name, _ in values}


def project(raw: Path, selected: Path, tools: ExtractionTools) -> None:
    # The full raw export retains the existing result-bundle entry/file/total
    # byte, regular-file, no-follow and stable-read gates, without new limits.
    tools.digest(raw)
    _, value = tools.read_json(raw / "manifest.json", tools.maximum_json_bytes)
    group = group_value(value, tools)
    rows = selected_rows(group, flat_inventory(raw, tools), tools)
    for row in rows:
        name = row["exportedFileName"]
        payload, _ = tools.read_json(raw / name, tools.maximum_attempt_bytes)
        (selected / name).write_bytes(payload)
    (selected / "manifest.json").write_text(
        json.dumps([{**group, "attachments": rows}]), encoding="utf-8"
    )


def extract(
    result_bundle: Path,
    suite: dict,
    tools: ExtractionTools,
    *,
    require_measured_network: bool,
):
    tools.exact_node(
        tools.run_json(
            [
                "xcrun",
                "xcresulttool",
                "get",
                "test-results",
                "tests",
                "--path",
                str(result_bundle),
            ],
            tools.maximum_json_bytes,
        )
    )
    with (
        tempfile.TemporaryDirectory() as raw_directory,
        tempfile.TemporaryDirectory() as selected_directory,
    ):
        raw, selected = Path(raw_directory), Path(selected_directory)
        tools.run_export(
            [
                "xcrun",
                "xcresulttool",
                "export",
                "attachments",
                "--test-id",
                tools.node_url,
                "--path",
                str(result_bundle),
                "--output-path",
                str(raw),
            ],
        )
        project(raw, selected, tools)
        # This parser still requires exactly fifteen canonical, bounded,
        # measured attempts and a strict sixteen-entry selected inventory.
        return tools.load_selected(
            selected, suite, require_measured_network=require_measured_network
        )
