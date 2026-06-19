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


def available_r() -> list[dict[str, Any]]:
    return json.loads(clean_rig_output(run_rig(["available", "--all", "--json"])))


def stable_version_parts(record: dict[str, Any]) -> tuple[int, int, int] | None:
    return version_parts(record.get("semver") or record.get("version", ""))


def available_release_parts() -> tuple[int, int, int]:
    available = available_r()
    release = next(
        (
            record
            for record in available
            if record.get("name") == "release"
        ),
        None,
    )
    if release is not None:
        parts = stable_version_parts(release)
        if parts is None:
            die("rig available reports release without a stable R version")
        return parts

    stable = [
        parts
        for record in available
        if record.get("name") not in {"devel", "next"}
        for parts in [stable_version_parts(record)]
        if parts is not None
    ]
    if not stable:
        die("rig available does not report a stable release R")

    return max(stable)


def release_parts(installed: list[dict[str, Any]]) -> tuple[int, int, int]:
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
        parts = version_parts(aliased.get("version", ""))
        if parts is None:
            die(f"installed release R has unsupported version {aliased.get('version')}")
        return parts

    return available_release_parts()


def resolve_install(spec: str) -> tuple[str, str]:
    offset = oldrel_offset(spec)
    installed = installed_r()
    baseline = release_parts(installed)
    if baseline[1] < offset:
        die(f"cannot resolve {spec} relative to release R {baseline[0]}.{baseline[1]}.{baseline[2]}")

    target = (baseline[0], baseline[1] - offset)
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
