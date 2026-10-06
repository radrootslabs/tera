"""Reset Bash background signal inheritance, then exec the exact owned child."""

from __future__ import annotations

import os
import signal
import sys


def main(arguments: list[str]) -> int:
    if not arguments:
        return 64
    signal.signal(signal.SIGINT, signal.SIG_DFL)
    signal.signal(signal.SIGTERM, signal.SIG_DFL)
    # The cleanup owner defers pending signals until both dispositions are reset.
    os.kill(os.getppid(), signal.SIGUSR1)
    try:
        os.execvp(arguments[0], arguments)
    except FileNotFoundError:
        print("error: owned Xcode child command is unavailable", file=sys.stderr)
        return 127
    except OSError:
        print("error: owned Xcode child command cannot be executed", file=sys.stderr)
        return 126


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
