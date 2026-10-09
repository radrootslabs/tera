"""Fresh owned native dispatch for Python test-tool bodies; no product runner."""

from __future__ import annotations

import hashlib
import json
import os
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import time
from pathlib import Path

if __package__:
    from .fixture_test_policy import limit, load_policy
else:
    from fixture_test_policy import limit, load_policy


class FixtureDispatchError(RuntimeError):
    """The owned fixture compiler or declared tool could not be established."""


class CompilerCommand:
    """One deadline, bounded pipes and cleanup before releasing the leader PID."""

    LIMITS = {"stdout": 1024 * 1024, "stderr": 64 * 1024}

    def __init__(
        self,
        argv: list[str],
        cwd: Path,
        timeout=None,
        *,
        env=None,
        budget="compiler",
        maximum_stdout=1024 * 1024,
    ):
        policy = load_policy()
        timeout = policy["limits"][budget] if timeout is None else timeout
        if not 0 < timeout <= policy["limits"][budget]:
            raise ValueError("fixture command deadline exceeds its fixed policy budget")
        if (
            type(maximum_stdout) is not int
            or not 0 < maximum_stdout <= self.LIMITS["stdout"]
        ):
            raise ValueError("command JSON output exceeds its byte bound")
        self.LIMITS = {**self.LIMITS, "stdout": maximum_stdout}
        self.argv, self.cwd, self.timeout = argv, cwd, timeout
        self.environment = env
        self.output = {name: bytearray() for name in self.LIMITS}
        self.selector = selectors.DefaultSelector()
        self.process = None
        self.started = time.monotonic()
        self.record = {
            "argv": argv,
            "cwd": str(cwd),
            "timeout": timeout,
            "policy_id": policy["id"],
            "policy_sha256": policy["source_sha256"],
            "budget": budget,
            "started_ns": time.time_ns(),
            "signals": [],
            "wait_reaped": False,
            "group_absent": False,
        }

    def exited(self) -> bool:
        state = os.waitid(
            os.P_PID, self.process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT
        )
        return state is not None and state.si_pid == self.process.pid

    def read_ready(self, timeout: float, *, reject_excess: bool) -> None:
        if not self.selector.get_map():
            time.sleep(timeout)
            return
        for key, _ in self.selector.select(timeout):
            name = key.data
            available = self.LIMITS[name] - len(self.output[name])
            size = min(65536, available + 1) if reject_excess else 65536
            data = os.read(key.fd, size)
            if not data:
                self.selector.unregister(key.fileobj)
                key.fileobj.close()
                continue
            self.output[name].extend(data[:available])
            if len(data) > available:
                self.record.setdefault("output_excess", name)
                if reject_excess:
                    raise FixtureDispatchError(
                        "fixture compiler capture limit exceeded"
                    )

    def capture(self) -> None:
        deadline = self.started + self.timeout
        while self.selector.get_map() or not self.exited():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired(self.argv, self.timeout)
            self.read_ready(min(remaining, 0.05), reject_excess=True)

    def group_present(self) -> bool:
        try:
            os.killpg(self.process.pid, 0)
        except ProcessLookupError:
            return False
        except PermissionError:
            return True
        return True

    def send(self, number: int) -> None:
        try:
            os.killpg(self.process.pid, number)
        except ProcessLookupError:
            return
        except PermissionError:
            self.record.setdefault("signal_errors", []).append(
                {"signal": number, "errno": 1}
            )
            if not self.exited():
                raise
            return
        self.record["signals"].append({"signal": number, "at_ns": time.time_ns()})

    def drain(self, deadline: float) -> None:
        while self.selector.get_map() and time.monotonic() < deadline:
            self.read_ready(
                min(0.01, max(0, deadline - time.monotonic())), reject_excess=False
            )

    def finish_cleanup(self, deadline: float) -> None:
        self.send(signal.SIGKILL)
        self.process.wait(timeout=max(0.001, deadline - time.monotonic()))
        self.record.update(wait_reaped=True, exit=self.process.returncode)
        self.drain(deadline)
        while self.group_present() and time.monotonic() < deadline:
            time.sleep(0.005)
        self.record["group_absent"] = not self.group_present()
        if not self.record["group_absent"] or self.selector.get_map():
            raise FixtureDispatchError("fixture compiler cleanup incomplete")

    def settle(self) -> None:
        deadline = time.monotonic() + limit("leaf_cleanup")
        try:
            self.send(signal.SIGTERM)
            grace = time.monotonic() + 0.2
            self.drain(grace)
            while self.group_present() and time.monotonic() < grace:
                time.sleep(0.005)
        finally:
            self.finish_cleanup(deadline)

    def execute(self) -> subprocess.CompletedProcess:
        try:
            self.process = subprocess.Popen(
                self.argv,
                cwd=self.cwd,
                env=self.environment,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                bufsize=0,
                start_new_session=True,
            )
            self.record.update(pid=self.process.pid, pgid=self.process.pid)
            for name in self.LIMITS:
                stream = getattr(self.process, name)
                os.set_blocking(stream.fileno(), False)
                self.selector.register(stream, selectors.EVENT_READ, name)
            self.capture()
        except BaseException as error:
            self.record["error"] = type(error).__name__
            raise
        finally:
            try:
                if self.process is not None:
                    self.settle()
            except BaseException as error:
                self.record["cleanup_error"] = type(error).__name__
                raise
            finally:
                self.selector.close()
                if self.process is not None:
                    for name in self.LIMITS:
                        getattr(self.process, name).close()
                self.record.update(
                    elapsed_seconds=time.monotonic() - self.started,
                    ended_ns=time.time_ns(),
                )
        return subprocess.CompletedProcess(
            self.argv,
            self.process.returncode,
            bytes(self.output["stdout"]),
            bytes(self.output["stderr"]),
        )


def selector_command(argv, *, maximum_stdout=1024 * 1024, timeout=None):
    capture = CompilerCommand(
        argv,
        Path.cwd(),
        timeout,
        budget="selector_command",
        maximum_stdout=maximum_stdout,
    )
    try:
        result = capture.execute()
    except FixtureDispatchError as error:
        if capture.record.get("output_excess") == "stdout":
            raise ValueError("command JSON output exceeds its byte bound") from error
        raise
    if result.returncode:
        raise subprocess.CalledProcessError(result.returncode, argv)
    return result


NATIVE_SOURCE = r"""
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#if defined(__APPLE__)
#include <mach-o/dyld.h>
#endif

static const char *roles[] = {ROLE_LITERALS};
static const char *python = PYTHON_LITERAL;

static int executing_image(char *resolved) {
    char image[PATH_MAX];
#if defined(__APPLE__)
    uint32_t size = sizeof(image);
    if (_NSGetExecutablePath(image, &size) != 0) return 0;
#elif defined(__linux__)
    ssize_t size = readlink("/proc/self/exe", image, sizeof(image) - 1);
    if (size < 0 || (size_t)size >= sizeof(image) - 1) return 0;
    image[size] = '\0';
#else
    return 0;
#endif
    return realpath(image, resolved) != NULL;
}

int main(int argc, char **argv) {
    if (argc < 1 || argc > 4096) return 64;
    const char *role = strrchr(argv[0], '/');
    role = role ? role + 1 : argv[0];
    size_t count = sizeof(roles) / sizeof(roles[0]);
    size_t index;
    for (index = 0; index < count; index++)
        if (strcmp(role, roles[index]) == 0) break;
    if (index == count) return 64;
    char image[PATH_MAX];
    if (!executing_image(image)) return 64;
    char *basename = strrchr(image, '/');
    if (!basename || strcmp(basename + 1, role) != 0) return 64;
    *basename = '\0';
    const char *path = getenv("PATH");
    if (!path || path[0] != '/') return 64;
    size_t length = strcspn(path, ":");
    if (!length || length >= PATH_MAX) return 64;
    if (strlen(image) != length || strncmp(path, image, length) != 0) return 64;
    char body[PATH_MAX];
    int size = snprintf(body, sizeof(body), "%.*s/%s.body.py", (int)length, path, role);
    if (size < 0 || (size_t)size >= sizeof(body)) return 64;
    if (access(body, R_OK) != 0) return 66;
    char **arguments = calloc((size_t)argc + 2, sizeof(char *));
    if (!arguments) return 70;
    arguments[0] = (char *)python;
    arguments[1] = body;
    for (int item = 1; item < argc; item++) arguments[item + 1] = argv[item];
    execv(python, arguments);
    perror("fixture-tool-dispatch: execv");
    free(arguments);
    return 71;
}
"""


def file_identity(path: Path) -> dict:
    information = path.stat()
    return {
        "path": str(path),
        "resolved": str(path.resolve()),
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "bytes": information.st_size,
        "mode": stat.S_IMODE(information.st_mode),
    }


def c_literal(value: str) -> str:
    return '"' + "".join(f"\\{byte:03o}" for byte in value.encode()) + '"'


class FixtureToolDispatcher:
    """Compile once per fixture, never execute the target or share tool inodes."""

    def __init__(self, tools: Path, roles: tuple[str, ...]):
        self.tools = tools.resolve()
        if not 0 < len(roles) <= 16 or len(set(roles)) != len(roles):
            raise FixtureDispatchError("fixture roles must be nonempty and unique")
        if any(re.fullmatch(r"[a-z][a-z0-9_]{0,31}", role) is None for role in roles):
            raise FixtureDispatchError("fixture role name rejected")
        self.roles = roles
        self.directory = self.tools / ".fixture-dispatch"
        self.directory.mkdir()
        self.binary = self.directory / "dispatch"
        self.record = {
            "interpreter": file_identity(Path(sys.executable)),
            "roles": list(roles),
            "tools": str(self.tools),
            "environment_policy": "compiler inherits existing selected developer environment",
            "body_policy": "actual interpreter argv is [interpreter, role.body.py, *original argument suffix]",
            "image_policy": "actual canonical image parent equals first PATH; actual basename equals argv0 role",
        }
        self.compile()

    def compile(self) -> None:
        compiler = shutil.which("cc")
        if compiler is None:
            self.record["error"] = "selected native compiler unavailable"
            self.save_record()
            raise FixtureDispatchError(
                "fixture requires an installed selected native compiler"
            )
        source = self.directory / "dispatch.c"
        source.write_text(
            NATIVE_SOURCE.replace(
                "ROLE_LITERALS", ",".join(map(c_literal, self.roles))
            ).replace("PYTHON_LITERAL", c_literal(sys.executable))
        )
        self.record.update(
            source=file_identity(source), compiler=file_identity(Path(compiler))
        )
        argv = [
            compiler,
            "-Wall",
            "-Wextra",
            "-Werror",
            "-o",
            str(self.binary),
            str(source),
        ]
        command = CompilerCommand(argv, self.directory)
        try:
            result = command.execute()
            if result.returncode != 0:
                raise FixtureDispatchError("fixture native compilation failed")
            if (
                not self.binary.is_file()
                or self.binary.stat().st_size > 2 * 1024 * 1024
            ):
                raise FixtureDispatchError("fixture compiler output rejected")
            self.record["binary"] = file_identity(self.binary)
        except BaseException as error:
            self.record["error"] = type(error).__name__
            raise
        finally:
            self.record["command"] = command.record
            for name, raw in command.output.items():
                (self.directory / f"compiler.{name}").write_bytes(raw)
            self.save_record()

    def save_record(self) -> None:
        (self.directory / "compilation.json").write_text(
            json.dumps(self.record, indent=2) + "\n"
        )

    def executable(self, name: str, body: str) -> None:
        if name not in self.roles or len(body.encode()) > 2 * 1024 * 1024:
            raise FixtureDispatchError("undeclared fixture tool or oversized body")
        path = self.tools / name
        if path.exists() or path.is_symlink():
            path.unlink()
        body_path = self.tools / f"{name}.body.py"
        body_path.write_text(body)
        shutil.copyfile(self.binary, path)
        path.chmod(0o700)
        record = {
            "tool": file_identity(path),
            "body": file_identity(body_path),
            "interpreter": self.record["interpreter"],
            "actual_exec_argv_prefix": [sys.executable, str(body_path)],
            "argument_suffix": "original native argv[1:] without transformation",
        }
        (self.directory / f"{name}.json").write_text(
            json.dumps(record, indent=2) + "\n"
        )
