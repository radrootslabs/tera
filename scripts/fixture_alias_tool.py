"""Native alias fixture: exercise argv0 without an unrelated Python child."""

from __future__ import annotations

import json
from pathlib import Path

from fixture_tool_dispatch import CompilerCommand, FixtureDispatchError


def compile_alias_dispatch(root: Path, compiler: str | None, environment: dict):
    if compiler is None:
        raise FixtureDispatchError("native alias fixture compiler unavailable")
    source = root / "alias-dispatch.c"
    binary = root / "alias-dispatch"
    source.write_text(
        "#include <stdio.h>\n#include <string.h>\n"
        "int main(int argc, char **argv) {\n"
        "    if (argc != 1 || argv[0] == NULL) return 64;\n"
        "    const char *name = strrchr(argv[0], '/');\n"
        "    return puts(name ? name + 1 : argv[0]) < 0 ? 74 : 0;\n"
        "}\n"
    )
    command = CompilerCommand(
        [compiler, "-Wall", "-Wextra", "-Werror", "-o", str(binary), str(source)],
        root,
        env=environment,
    )
    try:
        result = command.execute()
    finally:
        (root / "alias-compilation.json").write_text(json.dumps(command.record))
    if (
        result.returncode
        or not command.record["wait_reaped"]
        or not command.record["group_absent"]
    ):
        raise FixtureDispatchError("native alias fixture compilation failed")
    return binary, command.record
