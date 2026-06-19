#!/usr/bin/env python3
"""Resolve the concrete rig install used by version-selection tests."""

from __future__ import annotations

import re
import subprocess
import sys


def die(message: str) -> None:
    raise SystemExit(message)


def run_rig(args: list[str]) -> str:
    result = subprocess.run(
        ["rig", *args],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        die(f"`rig {' '.join(args)}` exited with code {result.returncode}")
    return result.stdout


def release_metadata(spec: str) -> tuple[str, str, str]:
    expression = (
        'rscript <- file.path(R.home("bin"), '
        'if (.Platform$OS.type == "windows") "Rscript.exe" else "Rscript"); '
        'cat(sprintf("IR_TEST_R_VERSION=%s\\nIR_TEST_R_DATE=%s-%s-%s\\nIR_TEST_RSCRIPT=%s\\n", '
        'as.character(getRversion()), R.version$year, R.version$month, R.version$day, '
        'normalizePath(rscript, winslash = "/", mustWork = TRUE)))'
    )
    output = run_rig(
        [
            "run",
            "-r",
            spec,
            "-e",
            expression,
        ]
    )
    return (
        output_field(output, "IR_TEST_R_VERSION", spec),
        output_field(output, "IR_TEST_R_DATE", spec),
        output_field(output, "IR_TEST_RSCRIPT", spec),
    )


def output_field(output: str, name: str, spec: str) -> str:
    value = re.search(rf"^{name}=(.+)$", output, re.MULTILINE)
    if not value:
        die(f"could not read {name} for {spec}")
    return value.group(1)


def main() -> None:
    if len(sys.argv) != 2:
        die("usage: scripts/resolve-test-r.py oldrel/N")

    spec = sys.argv[1]
    version, date, rscript = release_metadata(spec)
    print(spec)
    print(version)
    print(date)
    print(rscript)


if __name__ == "__main__":
    main()
