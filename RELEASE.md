# Releasing ir

Run stable releases from a clean, up-to-date `main` checkout on macOS or
Linux:

```sh
scripts/release.sh 0.4.0
```

Pass the version without a `v` prefix or `+dev` suffix. The current version in
`Cargo.toml` and `Cargo.lock` must end in `+dev`.

The command requires authenticated `git` and `gh` access, plus the local tools
used by `scripts/check.sh`. It:

1. Changes both Cargo files to the stable version and runs the complete local
   check.
2. Commits and pushes the release version to `main`, then waits for that
   commit's CI run.
3. Creates and pushes an annotated tag, then waits for the tag-driven release
   workflow.
4. Confirms that the GitHub Release is public, downloads the archive for the
   current host, verifies its checksum, and smoke-tests `ir`, `rx`, and a short
   R execution.
5. Changes both Cargo files to `VERSION+dev`, commits and pushes that change,
   then waits for its CI run.

The release workflow builds and validates all five platform archives and
publishes `SHA256SUMS.txt`. The local script tests only the archive for the
machine on which it runs.

The script follows one happy path and stops at the first failure. It does not
undo or resume partial releases. Inspect the Git commits, tag, Actions runs,
and Cargo files before continuing manually after a failure.
