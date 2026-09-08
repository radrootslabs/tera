"""Validate the real locked application graph after the proxy crate is retired."""

from __future__ import annotations

from typing import Any

OWNED = {"tera_core", "tera_ffi", "tera_bindgen", "tera_wasm"}
MOBILE = OWNED - {"tera_wasm"}
RETIRED = {
    "radroots_ios_source_lock",
    "radroots_mobile_core",
    "radroots_mobile_ffi",
    "radroots_mobile_bindgen",
    "radroots_mobile_wasm",
}


class GraphError(ValueError):
    """A stable rejection without dependency source paths or command output."""


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise GraphError(reason)


def indexed(rows: Any, label: str) -> dict[str, dict[str, Any]]:
    require(isinstance(rows, list) and bool(rows), f"{label} inventory is absent")
    result = {}
    for row in rows:
        require(isinstance(row, dict), f"{label} entry is malformed")
        identifier = row.get("id")
        require(
            isinstance(identifier, str) and bool(identifier),
            f"{label} identity is absent",
        )
        require(identifier not in result, f"{label} identity is duplicated")
        result[identifier] = row
    return result


def owned_packages(
    document: dict[str, Any], packages: dict[str, Any]
) -> dict[str, Any]:
    members = document.get("workspace_members")
    require(isinstance(members, list), "owned workspace members are absent")
    require(len(members) == len(OWNED), "owned workspace inventory differs")
    require(
        all(identifier in packages for identifier in members), "owned package is absent"
    )
    owned = {
        packages[identifier]["name"]: packages[identifier] for identifier in members
    }
    require(set(owned) == OWNED, "owned workspace inventory differs")
    defaults = document.get("workspace_default_members")
    require(isinstance(defaults, list), "mobile defaults are absent")
    require(
        sorted(defaults) == sorted(owned[name]["id"] for name in MOBILE),
        "mobile defaults must select the real native packages only",
    )
    return owned


def foundation_sources(packages: dict[str, Any], foundation: dict[str, Any]) -> None:
    revision = foundation["revision"]
    expected = f"git+{foundation['repository']}?rev={revision}#{revision}"
    shared = []
    for package in packages.values():
        name = package.get("name")
        require(isinstance(name, str), "resolved package name is absent")
        require(
            name not in RETIRED,
            "retired mobile producer or source-lock shim is present",
        )
        if name.startswith("radroots_"):
            shared.append(package)
            require(
                package.get("source") == expected, "resolved foundation source differs"
            )
            require(
                package.get("version") == foundation["version"],
                "resolved foundation version differs",
            )
        elif package.get("source") is None:
            require(
                name in OWNED, "unowned local package entered the application graph"
            )
    require(bool(shared), "resolved foundation inventory is empty")


def mobile_profile(owned: dict[str, Any], nodes: dict[str, Any]) -> None:
    ffi = owned["tera_ffi"]
    edges = ffi.get("dependencies")
    require(isinstance(edges, list), "owned FFI dependency inventory is absent")
    core_edges = [edge for edge in edges if edge.get("name") == "tera_core"]
    require(len(core_edges) == 1, "owned FFI must select one application core")
    edge = core_edges[0]
    require(
        edge.get("source") is None and edge.get("kind") is None,
        "FFI core must be an owned runtime dependency",
    )
    require(
        "mobile-social" in edge.get("features", []),
        "owned FFI mobile-social profile is absent",
    )
    core_node = nodes.get(owned["tera_core"]["id"], {})
    require(
        "mobile-social" in core_node.get("features", []),
        "resolved mobile-social feature is absent",
    )


def validate(document: dict[str, Any], foundation: dict[str, Any]) -> None:
    packages = indexed(document.get("packages"), "resolved package")
    owned = owned_packages(document, packages)
    foundation_sources(packages, foundation)
    resolution = document.get("resolve")
    require(isinstance(resolution, dict), "resolved dependency graph is absent")
    nodes = indexed(resolution.get("nodes"), "resolved node")
    require(set(nodes) == set(packages), "resolved node inventory differs")
    mobile_profile(owned, nodes)
