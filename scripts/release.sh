#!/usr/bin/env bash

set -euo pipefail

readonly repo="r-lib/ir"
release_dir=""

usage() {
  printf 'Usage: scripts/release.sh VERSION\n\nRelease a stable version such as 0.4.0.\n'
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$release_dir" && -d "$release_dir" ]]; then
    rm -rf -- "$release_dir"
  fi
}
trap cleanup EXIT

set_version() {
  local from="$1"
  local to="$2"
  local manifest_tmp lock_tmp

  manifest_tmp="$(mktemp "${TMPDIR:-/tmp}/ir-manifest.XXXXXX")"
  if ! awk -v from="$from" -v to="$to" '
      $0 == "[package]" { package = 1 }
      package && $0 == "version = \"" from "\"" {
        print "version = \"" to "\""
        changed++
        next
      }
      package && /^\[/ && $0 != "[package]" { package = 0 }
      { print }
      END { if (changed != 1) exit 42 }
    ' Cargo.toml >"$manifest_tmp"; then
    rm -f "$manifest_tmp"
    die "expected package version $from in Cargo.toml"
  fi

  lock_tmp="$(mktemp "${TMPDIR:-/tmp}/ir-lock.XXXXXX")"
  if ! awk -v from="$from" -v to="$to" '
      $0 == "[[package]]" { ir_package = 0 }
      $0 == "name = \"ir\"" { ir_package = 1 }
      ir_package && $0 == "version = \"" from "\"" {
        print "version = \"" to "\""
        changed++
        next
      }
      { print }
      END { if (changed != 1) exit 42 }
    ' Cargo.lock >"$lock_tmp"; then
    rm -f "$manifest_tmp" "$lock_tmp"
    die "expected ir version $from in Cargo.lock"
  fi

  mv "$manifest_tmp" Cargo.toml
  mv "$lock_tmp" Cargo.lock
}

wait_for_workflow() {
  local workflow="$1"
  local ref="$2"
  local commit="$3"
  local run_id=""
  local attempt

  for ((attempt = 0; attempt < 60; attempt++)); do
    run_id="$(
      gh run list \
        --repo "$repo" \
        --workflow "$workflow" \
        --event push \
        --branch "$ref" \
        --commit "$commit" \
        --limit 1 \
        --json databaseId \
        --jq '.[0].databaseId // empty'
    )"
    [[ -z "$run_id" ]] || break
    sleep 5
  done

  [[ -n "$run_id" ]] || die "timed out waiting for $workflow at $commit"
  gh run watch "$run_id" \
    --repo "$repo" \
    --compact \
    --exit-status \
    --interval 10
}

wait_for_pypi() {
  local version="$1"
  local tool_dir="$2"
  local bin_dir="$3"
  local cache_dir="$4"
  local attempt

  for ((attempt = 1; attempt <= 60; attempt++)); do
    if UV_NO_CONFIG=1 \
      UV_DEFAULT_INDEX=https://pypi.org/simple \
      UV_TOOL_DIR="$tool_dir" \
      UV_TOOL_BIN_DIR="$bin_dir" \
      UV_CACHE_DIR="$cache_dir" \
      uv tool install --no-cache "r-lib-ir==$version"; then
      return
    fi
    if ((attempt < 60)); then
      sleep 5
    fi
  done

  die "failed to install r-lib-ir==$version from PyPI"
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
esac

[[ "$#" == 1 ]] || die "provide one stable version like 0.4.0"
version="$1"
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] ||
  die "provide one stable version like 0.4.0"
tag="v$version"

for command in git gh awk grep mktemp tar Rscript uv; do
  command -v "$command" >/dev/null 2>&1 || die "required command not found: $command"
done

case "$(uname -s):$(uname -m)" in
  Darwin:arm64 | Darwin:aarch64) target="aarch64-apple-darwin"; checksum=shasum ;;
  Darwin:x86_64 | Darwin:amd64) target="x86_64-apple-darwin"; checksum=shasum ;;
  Linux:arm64 | Linux:aarch64) target="aarch64-unknown-linux-gnu"; checksum=sha256sum ;;
  Linux:x86_64 | Linux:amd64) target="x86_64-unknown-linux-gnu"; checksum=sha256sum ;;
  *) die "releases must run on a supported macOS or Linux host" ;;
esac
command -v "$checksum" >/dev/null 2>&1 || die "required command not found: $checksum"

cd "$(git rev-parse --show-toplevel)"
[[ "$(git branch --show-current)" == main ]] || die "release from main"
[[ -z "$(git status --porcelain)" ]] || die "release from a clean worktree"
fetch_repo="$(
  gh repo view "$(git remote get-url origin)" \
    --json nameWithOwner \
    --jq .nameWithOwner
)"
push_repo="$(
  gh repo view "$(git remote get-url --push origin)" \
    --json nameWithOwner \
    --jq .nameWithOwner
)"
[[ "$fetch_repo" == "$repo" && "$push_repo" == "$repo" ]] ||
  die "origin must fetch from and push to $repo"

git fetch origin main --tags
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] ||
  die "main must match origin/main"
if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
  die "tag $tag already exists"
fi

current_version="$(awk '
  $0 == "[package]" { package = 1; next }
  package && /^version = "/ { gsub(/^version = "|"$/, ""); print; exit }
' Cargo.toml)"
[[ "$current_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+\+dev$ ]] ||
  die "current package version must end in +dev"

set_version "$current_version" "$version"
scripts/check.sh
git add Cargo.toml Cargo.lock
git diff --cached --check
git commit -m "Release $tag"
release_commit="$(git rev-parse HEAD)"

git fetch origin main
[[ "$(git rev-parse HEAD^)" == "$(git rev-parse origin/main)" ]] ||
  die "origin/main changed during local checks"
git push origin HEAD:main
wait_for_workflow ci.yml main "$release_commit"

git tag -a "$tag" -m "Release $tag"
git push origin "$tag"
wait_for_workflow release.yml "$tag" "$release_commit"

release_url="$(
  gh release view "$tag" \
    --repo "$repo" \
    --json isDraft,isPrerelease,url \
    --jq 'select(.isDraft == false and .isPrerelease == false) | .url'
)"
[[ -n "$release_url" ]] || die "$tag is not a published stable release"

archive="ir-$target.tar.gz"
release_dir="$(mktemp -d "${TMPDIR:-/tmp}/ir-release.XXXXXX")"
gh release download "$tag" \
  --repo "$repo" \
  --dir "$release_dir" \
  --pattern "$archive" \
  --pattern SHA256SUMS.txt

awk -v archive="$archive" '$2 == archive { print }' \
  "$release_dir/SHA256SUMS.txt" >"$release_dir/archive.sha256"
[[ "$(awk 'END { print NR }' "$release_dir/archive.sha256")" == 1 ]] ||
  die "missing checksum for $archive"
if [[ "$(uname -s)" == Darwin ]]; then
  (cd "$release_dir" && shasum -a 256 -c archive.sha256)
else
  (cd "$release_dir" && sha256sum -c archive.sha256)
fi

tar -xzf "$release_dir/$archive" -C "$release_dir"
ir_bin="$release_dir/ir-$target/ir"
rx_bin="$release_dir/ir-$target/rx"
[[ "$("$ir_bin" --version)" == "ir $version" ]] || die "downloaded ir has the wrong version"
[[ "$("$rx_bin" --version)" == "rx $version" ]] || die "downloaded rx has the wrong version"
"$ir_bin" --help >/dev/null
"$rx_bin" --help >/dev/null
IR_CACHE_DIR="$release_dir/cache" "$ir_bin" run \
  --rscript "$(command -v Rscript)" \
  -e 'stopifnot(nzchar(as.character(getRversion())))'

pypi_tool_dir="$release_dir/pypi-tool"
pypi_bin_dir="$release_dir/pypi-bin"
wait_for_pypi \
  "$version" \
  "$pypi_tool_dir" \
  "$pypi_bin_dir" \
  "$release_dir/pypi-uv-cache"
pypi_ir="$pypi_bin_dir/ir"
pypi_rx="$pypi_bin_dir/rx"
[[ "$("$pypi_ir" --version)" == "ir $version" ]] || die "PyPI ir has the wrong version"
[[ "$("$pypi_rx" --version)" == "rx $version" ]] || die "PyPI rx has the wrong version"
"$pypi_ir" --help >/dev/null
"$pypi_rx" --help >/dev/null
IR_CACHE_DIR="$release_dir/pypi-cache" "$pypi_ir" run \
  --rscript "$(command -v Rscript)" \
  -e 'stopifnot(nzchar(as.character(getRversion())))'

git fetch origin main
[[ "$(git rev-parse origin/main)" == "$release_commit" ]] ||
  die "origin/main changed before the development-version commit"
set_version "$version" "$version+dev"
git add Cargo.toml Cargo.lock
git diff --cached --check
git commit -m "Mark post-release builds as development versions"
development_commit="$(git rev-parse HEAD)"
git push origin HEAD:main
wait_for_workflow ci.yml main "$development_commit"

printf 'Released %s: %s\n' "$tag" "$release_url"
