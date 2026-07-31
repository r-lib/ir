# Releasing ir

Stable releases are made with `scripts/release.sh`. The script prepares the
release commit, waits for CI, publishes and verifies the GitHub Release, then
restores the development version on `main`.

For example, to release `0.4.0`:

```sh
scripts/release.sh 0.4.0
```

Do not include the `v` prefix or a `+dev` suffix. The script accepts stable
versions of the form `X.Y.Z`.

## Before running the script

Run the command from a clone or worktree on one of the release workflow's
macOS or Linux architectures: Apple Silicon, Intel macOS, Arm64 Linux, or
x86-64 Linux. The worktree may be on `main` or at a detached HEAD. It must
have:

- `origin` fetch and push URLs that resolve to the same GitHub repository;
- authenticated `git` and `gh` access with permission to push to `main`, push
  tags, and read Actions and Releases;
- `cargo`, `Rscript`, and the tools used by `scripts/check.sh`;
- for a new release, no staged or unstaged tracked changes.

Untracked files are allowed and are left untouched. When starting a release,
the current package version in `Cargo.toml` and `Cargo.lock` must be a matching
development version, such as `0.3.0+dev`. A resumed release may instead have
the exact partial Cargo version changes described below.

## What the script does

The command:

1. Resolves GitHub operations from `origin`, fetches `origin/main` and tags,
   then fast-forwards the worktree.
2. Changes `Cargo.toml` and `Cargo.lock` to the stable version.
3. Runs `scripts/check.sh` and verifies the locally built `ir` and `rx`
   versions.
4. Commits `Release vX.Y.Z`, pushes it directly to `main`, and waits for that
   exact commit's `CI` workflow.
5. Creates and pushes the annotated `vX.Y.Z` tag, then waits for that exact
   tag's `Release` workflow.
6. Confirms that the GitHub Release is public and has the expected six assets.
7. Downloads the archive for the current machine, verifies its checksum, and
   tests both binaries, including a short `ir run` invocation with `Rscript`.
8. Changes the package version to `X.Y.Z+dev`, builds both binaries locally,
   and verifies their versions.
9. Commits `Mark post-release builds as development versions`, pushes it to
   `main`, and waits for that exact commit's `CI` workflow.
10. Confirms the exact post-release commit and tag, its place on the
    first-parent `main` history, and the tracked worktree state.

The release workflow builds archives for Apple Silicon macOS, Intel macOS,
Arm64 Linux, x86-64 Linux, and x86-64 Windows. The script tests the archive for
the machine on which it runs.

## Resuming after a failure

Re-run the same command with the same version:

```sh
scripts/release.sh 0.4.0
```

The script derives its state from the local and remote commits, annotated tag,
and exact Cargo file contents. It resumes only when those values identify one
supported state. Exact partial version changes are safe to resume, including a
failure after one file was rewritten or after both files were staged. The
script canonicalizes both version files and reruns the relevant local checks
before committing. It does not keep a separate state file and does not roll
back commits, tags, or releases after a failure.

The failure message identifies the last durable state. Common cases are:

- If a GitHub Actions run failed, fix or re-run that workflow, then invoke the
  release script again.
- If a network operation or download was interrupted, invoke the script again.
- If `origin/main` moved during the release, inspect the new commits before
  deciding how to continue. The script stops instead of rebasing or tagging an
  ambiguous history.
- If `origin/main` advances after the post-release development commit was
  pushed, the script accepts it only when that exact commit remains on the
  first-parent history and its own CI passed.
- If the local or remote tag does not point to the validated release commit,
  resolve that conflict manually. The script never moves an existing tag.

The command is complete when it prints the release tag and commit, the exact
`X.Y.Z+dev` commit, and the current `origin/main` tip.
