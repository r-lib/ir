# Releasing ir

Run stable releases from a clean, up-to-date `main` checkout on macOS or
Linux:

```sh
scripts/release.sh 0.4.0
```

Pass the version without a `v` prefix or `+dev` suffix. The current version in
`Cargo.toml` and `Cargo.lock` must end in `+dev`.

Before the first PyPI release, create the GitHub Actions environment `pypi`.
Then create a pending Trusted Publisher for the PyPI project `r-lib-ir` with
owner `r-lib`, repository `ir`, workflow `release.yml`, and environment `pypi`.
A pending publisher does not reserve the project name, so configure it shortly
before the release.

The command requires authenticated `git` and `gh` access, plus the local tools
used by `scripts/check.sh`. It:

1. Changes both Cargo files to the stable version and runs the complete local
   check.
2. Commits and pushes the release version to `main`, then waits for that
   commit's CI run.
3. Creates and pushes an annotated tag, then waits for the tag-driven release
   workflow to publish the GitHub Release and PyPI wheels.
4. Confirms that the GitHub Release is public, downloads the archive for the
   current host, verifies its checksum, and smoke-tests `ir`, `rx`, and a short
   R execution.
5. Changes both Cargo files to `VERSION+dev`, commits and pushes that change,
   then waits for its CI run.

The release workflow builds and validates five platform archives and five
platform wheels. It publishes `SHA256SUMS.txt` with the GitHub Release, then
publishes the wheels to PyPI through Trusted Publishing. It does not publish a
source distribution. The local script tests the archive for its host and the
workflow verifies each wheel before publishing it.

The script follows one happy path and stops at the first failure. It does not
undo or resume partial releases. Inspect the Git commits, tag, Actions runs,
PyPI project, and Cargo files before continuing manually after a failure.

If PyPI publication fails after the GitHub Release is public, rerun the failed
Actions job while its original wheel artifacts are retained. Do not use a fresh
workflow dispatch to resume a partial PyPI upload: it rebuilds the wheels, while
PyPI filenames are immutable. If the artifacts have expired after any wheel was
uploaded, inspect the PyPI project and release a new version. Do not rerun
`scripts/release.sh` after its tag exists. After recovering a partial release,
complete the development-version commit manually.
