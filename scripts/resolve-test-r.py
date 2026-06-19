#!/usr/bin/env python3
"""Resolve the concrete rig install used by version-selection tests."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from typing import Any


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


def clean_rig_output(text: str) -> str:
    return "\n".join(line for line in text.splitlines() if not line.startswith("[INFO]"))


def version_parts(value: str) -> tuple[int, int, int] | None:
    if not re.fullmatch(r"\d+\.\d+\.\d+", value):
        return None
    return tuple(int(part) for part in value.split("."))


def oldrel_offset(spec: str) -> int:
    if spec == "oldrel":
        return 1
    if spec.startswith("oldrel/"):
        value = spec.split("/", 1)[1]
        if value and value.isdigit() and int(value) > 0:
            return int(value)
    die(f"unsupported test R spec: {spec}")


def installed_r() -> list[dict[str, Any]]:
    return json.loads(clean_rig_output(run_rig(["list", "--json"])))


def release_install(installed: list[dict[str, Any]]) -> dict[str, Any]:
    aliased = next(
        (
            install
            for install in installed
            if install.get("name") == "release"
            or "release" in install.get("aliases", [])
        ),
        None,
    )
    if aliased is not None:
        return aliased

    stable = [
        (parts, install)
        for install in installed
        for parts in [version_parts(install.get("version", ""))]
        if parts is not None
    ]
    if not stable:
        die("rig does not report an installed stable R")

    _, release = max(stable, key=lambda item: item[0])
    return release


def resolve_install(spec: str) -> tuple[str, str]:
    offset = oldrel_offset(spec)
    installed = installed_r()
    release = release_install(installed)

    release_parts = version_parts(release.get("version", ""))
    if release_parts is None or release_parts[1] < offset:
        die(f"cannot resolve {spec} relative to installed release R {release.get('version')}")

    target = (release_parts[0], release_parts[1] - offset)
    matches = [
        (parts, install)
        for install in installed
        for parts in [version_parts(install.get("version", ""))]
        if parts is not None and parts[:2] == target
    ]
    if not matches:
        die(f"R {target[0]}.{target[1]} from {spec} is not installed by rig")

    _, install = max(matches, key=lambda item: item[0])
    return install["name"], install["version"]


def release_metadata(name: str) -> tuple[str, str]:
    expression = (
        'rscript <- file.path(R.home("bin"), '
        'if (.Platform$OS.type == "windows") "Rscript.exe" else "Rscript"); '
        'cat(sprintf("IR_TEST_R_DATE=%s-%s-%s\\nIR_TEST_RSCRIPT=%s\\n", '
        'R.version$year, R.version$month, R.version$day, '
        'normalizePath(rscript, winslash = "/", mustWork = TRUE)))'
    )
    output = run_rig(
        [
            "run",
            "-r",
            name,
            "-e",
            expression,
        ]
    )
    date = re.search(r"^IR_TEST_R_DATE=(\d{4}-\d{2}-\d{2})$", output, re.MULTILINE)
    if not date:
        die(f"could not read R release date for {name}")
    rscript = re.search(r"^IR_TEST_RSCRIPT=(.+)$", output, re.MULTILINE)
    if not rscript:
        die(f"could not read Rscript path for {name}")
    return date.group(1), rscript.group(1)


def main() -> None:
    if len(sys.argv) != 2:
        die("usage: scripts/resolve-test-r.py oldrel/N")

    name, version = resolve_install(sys.argv[1])
    date, rscript = release_metadata(name)
    print(name)
    print(version)
    print(date)
    print(rscript)


if __name__ == "__main__":
    main()
