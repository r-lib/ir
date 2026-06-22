#!/usr/bin/env python3
"""Resolve the concrete rig install used by version-selection tests."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from email.parser import Parser
from pathlib import Path


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


def run_rscript(
    binary: str,
    args: list[str],
    stdin: str,
    env: dict[str, str] | None = None,
) -> str:
    full_env = os.environ.copy()
    if env is not None:
        full_env.update(env)
    result = subprocess.run(
        [binary, *args],
        check=False,
        env=full_env,
        input=stdin,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        die(f"`{binary} {' '.join(args)}` exited with code {result.returncode}")
    return result.stdout


def resolve_spec(spec: str) -> str:
    output = run_rig(["-q", "resolve", spec])
    version = output.strip().split(maxsplit=1)[0] if output.strip() else ""
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        die(f"could not resolve {spec} to a concrete R version")
    return version


def installed_r_for_version(version: str, spec: str) -> tuple[str, str]:
    installs = json.loads(run_rig(["-q", "list", "--json"]))
    for install in installs:
        if install.get("version") == version:
            name = install.get("name", "")
            binary = install.get("binary", "")
            if not name or not binary:
                die(f"rig did not report a name and R binary for R {version} from {spec}")
            return name, rscript_for_binary(binary)
    die(f"R {version} from {spec} is not installed by rig")


def rscript_for_binary(binary: str) -> str:
    binary_path = Path(binary)
    if binary_path.name.lower() == "r.exe":
        rscript = binary_path.with_name("Rscript.exe")
    else:
        rscript = binary_path.with_name("Rscript")
    if not rscript.exists():
        die(f"rig reported R binary `{binary}`, but `{rscript}` does not exist")
    return str(rscript)


def release_metadata(rscript: str) -> tuple[str, str, str]:
    output = run_rscript(
        rscript,
        ["-"],
        # fmt: r
        stdin="""
            rscript <- normalizePath(
              Sys.getenv("IR_TEST_METADATA_RSCRIPT"),
              winslash = "/",
              mustWork = TRUE
            )

            metadata <- data.frame(
              version = as.character(getRversion()),
              date = sprintf("%s-%s-%s", R.version$year, R.version$month, R.version$day),
              rscript = rscript
            )

            write.dcf(metadata, stdout(), width = 100000)
        """,
        env={"IR_TEST_METADATA_RSCRIPT": rscript},
    )
    metadata = parse_metadata(output, rscript)
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
    name, rscript = installed_r_for_version(version, spec)
    reported_version, date, rscript = release_metadata(rscript)
    if reported_version != version:
        die(f"rig resolved {spec} to R {version}, but ran R {reported_version}")
    print(name)
    print(version)
    print(date)
    print(rscript)


if __name__ == "__main__":
    main()
