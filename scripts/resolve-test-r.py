#!/usr/bin/env python3
"""Resolve the concrete rig install used by version-selection tests."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from email.parser import Parser

R_METADATA_SCRIPT = r"""
rscript <- file.path(
  R.home("bin"),
  if (.Platform$OS.type == "windows") "Rscript.exe" else "Rscript"
)

metadata <- data.frame(
  version = as.character(getRversion()),
  date = sprintf("%s-%s-%s", R.version$year, R.version$month, R.version$day),
  rscript = normalizePath(rscript, winslash = "/", mustWork = TRUE)
)

write.dcf(metadata, stdout())
"""


def die(message: str) -> None:
    raise SystemExit(message)


def run_rig(args: list[str], stdin: str | None = None) -> str:
    result = subprocess.run(
        ["rig", *args],
        check=False,
        input=stdin,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        die(f"`rig {' '.join(args)}` exited with code {result.returncode}")
    return result.stdout


def resolve_spec(spec: str) -> str:
    output = run_rig(["-q", "resolve", spec])
    version = output.strip().split(maxsplit=1)[0] if output.strip() else ""
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        die(f"could not resolve {spec} to a concrete R version")
    return version


def installed_name_for_version(version: str, spec: str) -> str:
    installs = json.loads(run_rig(["-q", "list", "--json"]))
    for install in installs:
        if install.get("version") == version:
            return install["name"]
    die(f"R {version} from {spec} is not installed by rig")


def release_metadata(name: str) -> tuple[str, str, str]:
    output = run_rig(
        [
            "run",
            "-r",
            name,
            "-f",
            "-",
        ],
        stdin=R_METADATA_SCRIPT,
    )
    metadata = parse_metadata(output, name)
    return (
        metadata["version"],
        metadata["date"],
        metadata["rscript"],
    )


def parse_metadata(output: str, spec: str) -> dict[str, str]:
    metadata = Parser().parsestr(output)
    fields = {name: metadata.get(name, "") for name in ("version", "date", "rscript")}
    missing = [name for name, value in fields.items() if not value]
    if missing:
        die(f"could not read {', '.join(missing)} for {spec}")
    return fields


def main() -> None:
    if len(sys.argv) != 2:
        die("usage: scripts/resolve-test-r.py oldrel/N")

    spec = sys.argv[1]
    version = resolve_spec(spec)
    name = installed_name_for_version(version, spec)
    reported_version, date, rscript = release_metadata(name)
    if reported_version != version:
        die(f"rig resolved {spec} to R {version}, but ran R {reported_version}")
    print(name)
    print(version)
    print(date)
    print(rscript)


if __name__ == "__main__":
    main()
