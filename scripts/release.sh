#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/release.sh VERSION

Release a stable ir version directly from main, for example:

  scripts/release.sh 0.4.0

The command prepares and pushes the release commit, waits for its CI, creates
the annotated tag, waits for publication, tests the downloaded binary, restores
VERSION+dev, and waits for the final main CI run.

Re-run the same command after an interruption. It resumes only when the Git
commits, annotated tag, Cargo versions, and remote refs identify one exact
release state.
EOF
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

release_version=""
tag=""
release_commit=""
dev_commit=""
github_repo=""
host_target=""
phase="preflight"
rewrite_tmp=""
release_tmp=""

cleanup() {
  if [[ -n "$rewrite_tmp" && -d "$rewrite_tmp" ]]; then
    rm -rf -- "$rewrite_tmp"
  fi
  if [[ -n "$release_tmp" && -d "$release_tmp" ]]; then
    rm -rf -- "$release_tmp"
  fi
}

report_failure() {
  status=$?
  trap - EXIT
  cleanup

  if [[ "$status" -ne 0 ]]; then
    case "$phase" in
      preflight)
        printf 'Release stopped during preflight or state validation. Inspect the reported error before re-running.\n' >&2
        ;;
      release_changes)
        printf 'Release preparation stopped before a release commit was created. Exact partial Cargo version changes can be resumed by re-running the same command; inspect any other tracked changes manually.\n' >&2
        ;;
      release_committed)
        printf 'Release commit %s exists locally but was not pushed. Tag %s was not created. Re-run the same command.\n' \
          "$release_commit" "$tag" >&2
        ;;
      release_pushed)
        printf 'Release commit %s was pushed to main. Tag %s, publication, and the post-release development commit were not completed. Fix or rerun CI, then re-run the same command.\n' \
          "$release_commit" "$tag" >&2
        ;;
      tag_created)
        printf 'Annotated tag %s exists locally at %s but was not confirmed on origin. Re-run the same command.\n' \
          "$tag" "$release_commit" >&2
        ;;
      tag_pushed)
        printf 'Annotated tag %s was pushed at %s. Publication or downloaded-binary verification did not complete; the development commit was not created. Re-run the same command.\n' \
          "$tag" "$release_commit" >&2
        ;;
      release_verified | dev_changes)
        printf 'Release %s was published and verified at %s. The post-release development commit was not pushed. Exact partial Cargo version changes can be resumed by re-running the same command.\n' \
          "$tag" "$release_commit" >&2
        ;;
      dev_committed)
        printf 'Release %s was published. Post-release commit %s exists locally but was not pushed. Re-run the same command.\n' \
          "$tag" "$dev_commit" >&2
        ;;
      dev_pushed)
        printf 'Release %s and post-release commit %s were pushed, but final CI did not complete successfully. Fix or rerun CI, then re-run the same command.\n' \
          "$tag" "$dev_commit" >&2
        ;;
      final_verification)
        printf 'Release %s and post-release commit %s passed CI, but final Git state verification failed. Inspect origin/main and %s, then re-run the same command.\n' \
          "$tag" "$dev_commit" "$tag" >&2
        ;;
    esac
  fi

  exit "$status"
}

trap report_failure EXIT
trap 'exit 130' HUP INT TERM

manifest_version() {
  awk '
    $0 == "[package]" { in_package = 1; next }
    in_package && /^\[/ { exit }
    in_package && /^version = "[^"]+"$/ {
      value = $0
      sub(/^version = "/, "", value)
      sub(/"$/, "", value)
      print value
      exit
    }
  ' Cargo.toml
}

lock_version() {
  awk '
    $0 == "[[package]]" { in_package = 1; is_ir = 0; next }
    in_package && $0 == "name = \"ir\"" { is_ir = 1; next }
    is_ir && /^version = "[^"]+"$/ {
      value = $0
      sub(/^version = "/, "", value)
      sub(/"$/, "", value)
      print value
      exit
    }
  ' Cargo.lock
}

manifest_version_at() {
  git show "$1:Cargo.toml" | awk '
    $0 == "[package]" { in_package = 1; next }
    in_package && /^\[/ { exit }
    in_package && /^version = "[^"]+"$/ {
      value = $0
      sub(/^version = "/, "", value)
      sub(/"$/, "", value)
      print value
      exit
    }
  '
}

lock_version_at() {
  git show "$1:Cargo.lock" | awk '
    $0 == "[[package]]" { in_package = 1; is_ir = 0; next }
    in_package && $0 == "name = \"ir\"" { is_ir = 1; next }
    is_ir && /^version = "[^"]+"$/ {
      value = $0
      sub(/^version = "/, "", value)
      sub(/"$/, "", value)
      print value
      exit
    }
  '
}

ensure_versions() {
  expected="$1"
  actual_manifest="$(manifest_version)"
  actual_lock="$(lock_version)"
  [[ "$actual_manifest" == "$expected" ]] ||
    die "Cargo.toml version is $actual_manifest; expected $expected"
  [[ "$actual_lock" == "$expected" ]] ||
    die "Cargo.lock ir version is $actual_lock; expected $expected"
}

ensure_local_binaries() {
  expected="$1"
  [[ -x target/debug/ir ]] || die "local build did not produce target/debug/ir"
  [[ -x target/debug/rx ]] || die "local build did not produce target/debug/rx"
  [[ "$(target/debug/ir --version)" == "ir $expected" ]] ||
    die "local ir binary does not report version $expected"
  [[ "$(target/debug/rx --version)" == "rx $expected" ]] ||
    die "local rx binary does not report version $expected"
}

ensure_commit_versions() {
  commit="$1"
  expected="$2"
  actual_manifest="$(manifest_version_at "$commit")"
  actual_lock="$(lock_version_at "$commit")"
  [[ "$actual_manifest" == "$expected" ]] ||
    die "$commit has Cargo.toml version $actual_manifest; expected $expected"
  [[ "$actual_lock" == "$expected" ]] ||
    die "$commit has Cargo.lock ir version $actual_lock; expected $expected"
}

rewrite_manifest_stream() {
  local new_version="$1"
  awk -v version="$new_version" '
    BEGIN { changed = 0 }
    $0 == "[package]" { in_package = 1; print; next }
    in_package && /^\[/ { in_package = 0 }
    in_package && /^version = "[^"]+"$/ {
      print "version = \"" version "\""
      changed++
      next
    }
    { print }
    END { if (changed != 1) exit 42 }
  '
}

rewrite_lock_stream() {
  local new_version="$1"
  awk -v version="$new_version" '
    BEGIN { changed = 0 }
    $0 == "[[package]]" { in_package = 1; is_ir = 0; print; next }
    in_package && $0 == "name = \"ir\"" { is_ir = 1; print; next }
    is_ir && /^version = "[^"]+"$/ {
      print "version = \"" version "\""
      changed++
      is_ir = 0
      next
    }
    { print }
    END { if (changed != 1) exit 42 }
  '
}

expected_manifest_blob() {
  local base="$1"
  local new_version="$2"
  git show "$base:Cargo.toml" |
    rewrite_manifest_stream "$new_version" |
    git hash-object --stdin
}

expected_lock_blob() {
  local base="$1"
  local new_version="$2"
  git show "$base:Cargo.lock" |
    rewrite_lock_stream "$new_version" |
    git hash-object --stdin
}

materialize_project_version() {
  local base="$1"
  local new_version="$2"
  rewrite_tmp="$(mktemp -d "${TMPDIR:-/tmp}/ir-release-version.XXXXXX")"

  if ! git show "$base:Cargo.toml" |
    rewrite_manifest_stream "$new_version" >"$rewrite_tmp/Cargo.toml"; then
    die "could not prepare Cargo.toml version $new_version"
  fi
  if ! git show "$base:Cargo.lock" |
    rewrite_lock_stream "$new_version" >"$rewrite_tmp/Cargo.lock"; then
    die "could not prepare Cargo.lock version $new_version"
  fi

  command cat "$rewrite_tmp/Cargo.toml" >Cargo.toml
  command cat "$rewrite_tmp/Cargo.lock" >Cargo.lock
  rm -rf -- "$rewrite_tmp"
  rewrite_tmp=""
  ensure_versions "$new_version"
}

expected_version_paths() {
  printf 'Cargo.lock\nCargo.toml\n'
}

ensure_single_parent() {
  local commit="$1"
  local parent_count
  parent_count="$(git rev-list --parents -n 1 "$commit" | awk '{ print NF - 1 }')"
  [[ "$parent_count" == "1" ]] || die "$commit must have exactly one parent"
}

file_mode_at() {
  local commit="$1"
  local path="$2"
  git ls-tree "$commit" -- "$path" | awk 'NR == 1 { print $1 }'
}

ensure_version_only_commit() {
  local commit="$1"
  local base="$2"
  local new_version="$3"
  local description="$4"
  local expected_manifest expected_lock

  if ! expected_manifest="$(expected_manifest_blob "$base" "$new_version")"; then
    die "could not derive the expected Cargo.toml for $new_version from $base"
  fi
  if ! expected_lock="$(expected_lock_blob "$base" "$new_version")"; then
    die "could not derive the expected Cargo.lock for $new_version from $base"
  fi

  [[ "$(git rev-parse "$commit:Cargo.toml")" == "$expected_manifest" ]] ||
    die "$description $commit changes Cargo.toml beyond the package version"
  [[ "$(git rev-parse "$commit:Cargo.lock")" == "$expected_lock" ]] ||
    die "$description $commit changes Cargo.lock beyond the ir package version"
  [[ "$(file_mode_at "$commit" Cargo.toml)" == "$(file_mode_at "$base" Cargo.toml)" ]] ||
    die "$description $commit changes the Cargo.toml file mode"
  [[ "$(file_mode_at "$commit" Cargo.lock)" == "$(file_mode_at "$base" Cargo.lock)" ]] ||
    die "$description $commit changes the Cargo.lock file mode"
}

ensure_release_commit() {
  local commit="$1"
  local expected_subject="Release $tag"
  local actual_subject actual_paths parent parent_manifest parent_lock

  ensure_single_parent "$commit"
  parent="$(git rev-parse "$commit^")"
  parent_manifest="$(manifest_version_at "$parent")"
  parent_lock="$(lock_version_at "$parent")"
  [[ "$parent_manifest" == "$parent_lock" ]] ||
    die "$parent has mismatched Cargo.toml and Cargo.lock versions"
  [[ "$parent_manifest" =~ ^[0-9]+\.[0-9]+\.[0-9]+\+dev$ ]] ||
    die "$commit is not based on a stable development version"

  actual_subject="$(git show -s --format=%s "$commit")"
  [[ "$actual_subject" == "$expected_subject" ]] ||
    die "$commit has subject '$actual_subject'; expected '$expected_subject'"
  actual_paths="$(git diff-tree --no-commit-id --name-only -r "$commit" | LC_ALL=C sort)"
  [[ "$actual_paths" == "$(expected_version_paths)" ]] ||
    die "$commit does not change exactly Cargo.toml and Cargo.lock"
  ensure_commit_versions "$commit" "$release_version"
  ensure_version_only_commit "$commit" "$parent" "$release_version" "release commit"
}

ensure_dev_commit() {
  local commit="$1"
  local expected_parent="$2"
  local actual_subject actual_paths

  ensure_single_parent "$commit"
  actual_subject="$(git show -s --format=%s "$commit")"
  [[ "$actual_subject" == "Mark post-release builds as development versions" ]] ||
    die "$commit is not the post-release development commit"
  [[ "$(git rev-parse "$commit^")" == "$expected_parent" ]] ||
    die "$commit is not directly based on release commit $expected_parent"
  actual_paths="$(git diff-tree --no-commit-id --name-only -r "$commit" | LC_ALL=C sort)"
  [[ "$actual_paths" == "$(expected_version_paths)" ]] ||
    die "$commit does not change exactly Cargo.toml and Cargo.lock"
  ensure_commit_versions "$commit" "${release_version}+dev"
  ensure_version_only_commit "$commit" "$expected_parent" "${release_version}+dev" \
    "post-release commit"
}

ensure_only_version_paths() {
  local paths="$1"
  local description="$2"
  local unexpected
  unexpected="$(printf '%s\n' "$paths" | awk 'NF && $0 != "Cargo.lock" && $0 != "Cargo.toml"')"
  [[ -z "$unexpected" ]] ||
    die "$description changes include unsupported paths: $unexpected"
}

ensure_version_transition() {
  local base="$1"
  local new_version="$2"
  local staged_paths unstaged_paths staged_summary unstaged_summary
  local path base_blob expected_blob index_blob worktree_blob

  staged_paths="$(git diff --cached --name-only)"
  unstaged_paths="$(git diff --name-only)"
  ensure_only_version_paths "$staged_paths" "staged"
  ensure_only_version_paths "$unstaged_paths" "unstaged"
  [[ -n "$staged_paths" || -n "$unstaged_paths" ]] ||
    die "no Cargo version transition is present"

  staged_summary="$(git diff --cached --summary -- Cargo.toml Cargo.lock)"
  unstaged_summary="$(git diff --summary -- Cargo.toml Cargo.lock)"
  [[ -z "$staged_summary" && -z "$unstaged_summary" ]] ||
    die "Cargo version transition changes a file mode"

  for path in Cargo.toml Cargo.lock; do
    base_blob="$(git rev-parse "$base:$path")"
    if [[ "$path" == "Cargo.toml" ]]; then
      expected_blob="$(expected_manifest_blob "$base" "$new_version")" ||
        die "could not derive the expected Cargo.toml for $new_version from $base"
    else
      expected_blob="$(expected_lock_blob "$base" "$new_version")" ||
        die "could not derive the expected Cargo.lock for $new_version from $base"
    fi
    index_blob="$(git rev-parse ":$path")"
    worktree_blob="$(git hash-object --path="$path" "$path")"

    [[ "$index_blob" == "$base_blob" || "$index_blob" == "$expected_blob" ]] ||
      die "staged $path contains changes beyond the version transition"
    [[ "$worktree_blob" == "$base_blob" || "$worktree_blob" == "$expected_blob" ]] ||
      die "unstaged $path contains changes beyond the version transition"
  done
}

commit_version_change() {
  local message="$1"
  local base="$2"
  local new_version="$3"
  local actual_paths

  ensure_version_transition "$base" "$new_version"
  git add Cargo.toml Cargo.lock
  git diff --cached --check
  actual_paths="$(git diff --cached --name-only | LC_ALL=C sort)"
  [[ "$actual_paths" == "$(expected_version_paths)" ]] ||
    die "staged changes are not limited to Cargo.toml and Cargo.lock"
  [[ -z "$(git diff --name-only)" ]] || die "unstaged tracked changes remain"
  git commit -m "$message"
}

wait_for_workflow() {
  workflow="$1"
  branch="$2"
  commit="$3"
  label="$4"
  run_id=""
  attempts=0

  while [[ -z "$run_id" && "$attempts" -lt 60 ]]; do
    run_id="$(gh run list \
      --repo "$github_repo" \
      --workflow "$workflow" \
      --event push \
      --branch "$branch" \
      --commit "$commit" \
      --limit 1 \
      --json databaseId \
      --jq '.[0].databaseId // empty')"
    if [[ -z "$run_id" ]]; then
      attempts=$((attempts + 1))
      sleep 5
    fi
  done

  [[ -n "$run_id" ]] || die "timed out waiting for $label workflow for $commit"
  printf 'Waiting for %s workflow run %s...\n' "$label" "$run_id"
  gh run watch "$run_id" --repo "$github_repo" --compact --exit-status --interval 10
}

remote_tag_commit() {
  git ls-remote --tags origin "refs/tags/$tag^{}" | awk 'NR == 1 { print $1 }'
}

first_mainline_commit_after() {
  local base="$1"
  local tip="$2"
  git rev-list --first-parent "$tip" "^$base" | awk 'END { print }'
}

validate_tag() {
  [[ "$(git cat-file -t "refs/tags/$tag")" == "tag" ]] ||
    die "$tag must be an annotated tag"
  actual_commit="$(git rev-parse "$tag^{}")"
  [[ "$actual_commit" == "$release_commit" ]] ||
    die "$tag points to $actual_commit; expected $release_commit"
  ensure_release_commit "$release_commit"
}

verify_release() {
  release_state="$(gh release view "$tag" \
    --repo "$github_repo" \
    --json tagName,isDraft,isPrerelease,publishedAt \
    --jq '[.tagName, (.isDraft | tostring), (.isPrerelease | tostring), (.publishedAt // "")] | @tsv')"
  IFS=$'\t' read -r published_tag is_draft is_prerelease published_at <<<"$release_state"
  [[ "$published_tag" == "$tag" ]] || die "published release tag is $published_tag; expected $tag"
  [[ "$is_draft" == "false" ]] || die "$tag is still a draft release"
  [[ "$is_prerelease" == "false" ]] || die "$tag was published as a prerelease"
  [[ -n "$published_at" ]] || die "$tag has no publication timestamp"

  expected_assets="$(
    printf '%s\n' \
      SHA256SUMS.txt \
      ir-aarch64-apple-darwin.tar.gz \
      ir-aarch64-unknown-linux-gnu.tar.gz \
      ir-x86_64-apple-darwin.tar.gz \
      ir-x86_64-pc-windows-msvc.zip \
      ir-x86_64-unknown-linux-gnu.tar.gz |
      LC_ALL=C sort
  )"
  actual_assets="$(gh release view "$tag" --repo "$github_repo" --json assets --jq '.assets[].name' | LC_ALL=C sort)"
  [[ "$actual_assets" == "$expected_assets" ]] ||
    die "$tag does not contain the exact expected release asset set"
}

release_target() {
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64 | Darwin:aarch64) printf 'aarch64-apple-darwin\n' ;;
    Darwin:x86_64 | Darwin:amd64) printf 'x86_64-apple-darwin\n' ;;
    Linux:arm64 | Linux:aarch64) printf 'aarch64-unknown-linux-gnu\n' ;;
    Linux:x86_64 | Linux:amd64) printf 'x86_64-unknown-linux-gnu\n' ;;
    *) die "unsupported release host: $(uname -s) $(uname -m)" ;;
  esac
}

verify_downloaded_binary() {
  target="$host_target"
  archive="ir-${target}.tar.gz"
  package="ir-${target}"
  release_tmp="$(mktemp -d "${TMPDIR:-/tmp}/ir-release-${tag}.XXXXXX")"

  gh release download "$tag" \
    --repo "$github_repo" \
    --dir "$release_tmp" \
    --pattern "$archive" \
    --pattern SHA256SUMS.txt

  checksum_entry="$release_tmp/archive.sha256"
  if ! awk -v archive="$archive" '
    $2 == archive { print; found++ }
    END { if (found != 1) exit 42 }
  ' "$release_tmp/SHA256SUMS.txt" >"$checksum_entry"; then
    die "SHA256SUMS.txt does not contain exactly one checksum for $archive"
  fi

  if [[ "$(uname -s)" == "Darwin" ]]; then
    (cd "$release_tmp" && shasum -a 256 -c "${checksum_entry##*/}")
  else
    (cd "$release_tmp" && sha256sum -c "${checksum_entry##*/}")
  fi

  tar -xzf "$release_tmp/$archive" -C "$release_tmp"
  ir_bin="$release_tmp/$package/ir"
  rx_bin="$release_tmp/$package/rx"
  [[ -x "$ir_bin" ]] || die "$archive does not contain executable ir"
  [[ -x "$rx_bin" ]] || die "$archive does not contain executable rx"
  [[ "$("$ir_bin" --version)" == "ir $release_version" ]] ||
    die "downloaded ir does not report version $release_version"
  [[ "$("$rx_bin" --version)" == "rx $release_version" ]] ||
    die "downloaded rx does not report version $release_version"
  "$ir_bin" --help >/dev/null
  "$rx_bin" --help >/dev/null
  rscript="$(command -v Rscript)"
  IR_CACHE_DIR="$release_tmp/cache" "$ir_bin" run \
    --rscript "$rscript" \
    -e 'stopifnot(nzchar(as.character(getRversion())))'

  rm -rf -- "$release_tmp"
  release_tmp=""
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
esac

[[ "$#" -eq 1 ]] || {
  usage >&2
  die "expected exactly one stable version"
}

release_version="$1"
if [[ ! "$release_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  die "version must be stable semver without a v prefix, for example 0.4.0"
fi
tag="v${release_version}"

for command in git gh cargo awk mktemp sort tar Rscript; do
  require_command "$command"
done

case "$(uname -s)" in
  Darwin) require_command shasum ;;
  Linux) require_command sha256sum ;;
  *) die "scripts/release.sh supports macOS and Linux release hosts" ;;
esac
host_target="$(release_target)"

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"
[[ -f Cargo.toml && -f Cargo.lock ]] || die "run this command from the ir repository"
[[ -x scripts/check.sh ]] || die "scripts/check.sh is not executable"

branch="$(git symbolic-ref --quiet --short HEAD || true)"
[[ -z "$branch" || "$branch" == "main" ]] ||
  die "release from main or a detached worktree, not branch $branch"

origin_fetch_url="$(git remote get-url origin)"
origin_push_url="$(git remote get-url --push origin)"
fetch_repo_url="$(gh repo view "$origin_fetch_url" --json url --jq '.url')"
push_repo_url="$(gh repo view "$origin_push_url" --json url --jq '.url')"
[[ -n "$fetch_repo_url" && "$fetch_repo_url" == "$push_repo_url" ]] ||
  die "origin fetch and push URLs must identify the same GitHub repository"
case "$fetch_repo_url" in
  https://*/*/*) github_repo="${fetch_repo_url#https://}" ;;
  *) die "GitHub returned an unsupported canonical repository URL: $fetch_repo_url" ;;
esac

tracked_dirty=false
if ! git diff --quiet || ! git diff --cached --quiet; then
  tracked_dirty=true
fi

git fetch origin main --tags
if [[ "$tracked_dirty" == "false" ]]; then
  git merge --ff-only origin/main
fi

head="$(git rev-parse HEAD)"
origin_main="$(git rev-parse origin/main)"

repository_state=""
if [[ "$tracked_dirty" == "true" ]]; then
  [[ "$head" == "$origin_main" ]] ||
    die "origin/main moved while an exact Cargo version transition was incomplete"

  if git show-ref --verify --quiet "refs/tags/$tag"; then
    release_commit="$(git rev-parse "$tag^{}")"
    validate_tag
    remote_release_commit="$(remote_tag_commit)"
    [[ -n "$remote_release_commit" && "$remote_release_commit" == "$release_commit" ]] ||
      die "the incomplete development transition requires $tag on origin at $release_commit"
    [[ "$head" == "$release_commit" ]] ||
      die "the incomplete development transition is not based on release commit $release_commit"
    ensure_commit_versions "$release_commit" "$release_version"
    ensure_version_transition "$release_commit" "${release_version}+dev"
    repository_state="dev_changes"
  else
    previous_dev_version="$(manifest_version_at "$head")"
    previous_lock_version="$(lock_version_at "$head")"
    [[ "$previous_dev_version" == "$previous_lock_version" ]] ||
      die "HEAD has mismatched Cargo.toml and Cargo.lock versions"
    [[ "$previous_dev_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+\+dev$ ]] ||
      die "the incomplete release transition is not based on a development version"
    ensure_version_transition "$head" "$release_version"
    repository_state="release_changes"
  fi
else
  current_manifest="$(manifest_version)"
  current_lock="$(lock_version)"
  [[ -n "$current_manifest" && "$current_manifest" == "$current_lock" ]] ||
    die "Cargo.toml and the Cargo.lock ir entry must have the same version"

  if git show-ref --verify --quiet "refs/tags/$tag"; then
    release_commit="$(git rev-parse "$tag^{}")"
    validate_tag
    remote_release_commit="$(remote_tag_commit)"
    if [[ -n "$remote_release_commit" && "$remote_release_commit" != "$release_commit" ]]; then
      die "origin tag $tag points to $remote_release_commit; expected $release_commit"
    fi

    if [[ "$head" == "$release_commit" && "$origin_main" == "$release_commit" && "$current_manifest" == "$release_version" ]]; then
      if [[ -n "$remote_release_commit" ]]; then
        repository_state="tag_pushed"
      else
        repository_state="tag_local"
      fi
    elif [[ "$current_manifest" == "${release_version}+dev" && "$(git rev-parse "$head^")" == "$release_commit" ]]; then
      ensure_dev_commit "$head" "$release_commit"
      dev_commit="$head"
      if [[ "$head" == "$origin_main" ]]; then
        repository_state="dev_pushed"
      elif [[ "$origin_main" == "$release_commit" ]]; then
        repository_state="dev_local"
      else
        die "post-release commit $head is not based on the current origin/main state"
      fi
    elif [[ -n "$remote_release_commit" ]] && git merge-base --is-ancestor "$release_commit" "$origin_main"; then
      dev_commit="$(first_mainline_commit_after "$release_commit" "$origin_main")"
      [[ -n "$dev_commit" ]] ||
        die "origin/main does not contain a post-release commit after $release_commit"
      ensure_dev_commit "$dev_commit" "$release_commit"
      git merge-base --is-ancestor "$dev_commit" "$head" ||
        die "HEAD does not contain post-release commit $dev_commit"
      repository_state="dev_pushed"
    else
      die "$tag exists, but HEAD and Cargo versions do not identify a supported release state"
    fi
  else
    if [[ "$current_manifest" =~ ^[0-9]+\.[0-9]+\.[0-9]+\+dev$ && "$head" == "$origin_main" ]]; then
      repository_state="start"
      previous_dev_version="$current_manifest"
    elif [[ "$current_manifest" == "$release_version" ]]; then
      release_commit="$head"
      ensure_release_commit "$release_commit"
      if [[ "$head" == "$origin_main" ]]; then
        repository_state="release_pushed"
      elif [[ "$(git rev-parse HEAD^)" == "$origin_main" ]]; then
        repository_state="release_local"
      else
        die "release commit $head is not the tip of or directly ahead of origin/main"
      fi
    else
      die "no $tag exists and project version $current_manifest is not a resumable release state"
    fi
  fi
fi

if [[ "$repository_state" != "start" ]]; then
  printf 'Resuming %s from state %s.\n' "$tag" "$repository_state"
fi

case "$repository_state" in
  release_changes) phase="release_changes" ;;
  release_local) phase="release_committed" ;;
  release_pushed) phase="release_pushed" ;;
  tag_local) phase="tag_created" ;;
  tag_pushed) phase="tag_pushed" ;;
  dev_changes) phase="dev_changes" ;;
  dev_local) phase="dev_committed" ;;
  dev_pushed) phase="dev_pushed" ;;
esac

if [[ "$repository_state" == "start" ]]; then
  phase="release_changes"
  printf 'Preparing %s from development version %s...\n' "$tag" "$previous_dev_version"
  repository_state="release_changes"
fi

if [[ "$repository_state" == "release_changes" ]]; then
  phase="release_changes"
  materialize_project_version "$head" "$release_version"
  ensure_version_transition "$head" "$release_version"
  scripts/check.sh
  ensure_versions "$release_version"
  ensure_local_binaries "$release_version"
  commit_version_change "Release $tag" "$head" "$release_version"
  release_commit="$(git rev-parse HEAD)"
  ensure_release_commit "$release_commit"
  phase="release_committed"
  repository_state="release_local"
fi

if [[ "$repository_state" == "release_local" ]]; then
  git fetch origin main
  [[ "$(git rev-parse HEAD^)" == "$(git rev-parse origin/main)" ]] ||
    die "origin/main changed before the release commit could be pushed"
  git push origin HEAD:main
  phase="release_pushed"
  repository_state="release_pushed"
fi

if [[ "$repository_state" == "release_pushed" ]]; then
  phase="release_pushed"
fi

wait_for_workflow ci.yml main "$release_commit" "release-commit CI"

if ! git show-ref --verify --quiet "refs/tags/$tag"; then
  git fetch origin main --tags
  [[ "$(git rev-parse origin/main)" == "$release_commit" ]] ||
    die "origin/main changed before $tag could be created"
  git tag -a "$tag" "$release_commit" -m "Release $tag"
  phase="tag_created"
fi
validate_tag

remote_release_commit="$(remote_tag_commit)"
if [[ -z "$remote_release_commit" ]]; then
  git push origin "$tag"
  phase="tag_pushed"
elif [[ "$remote_release_commit" != "$release_commit" ]]; then
  die "origin tag $tag points to $remote_release_commit; expected $release_commit"
else
  phase="tag_pushed"
fi

wait_for_workflow release.yml "$tag" "$release_commit" "release"
verify_release
verify_downloaded_binary
phase="release_verified"

head="$(git rev-parse HEAD)"
git fetch origin main --tags
origin_main="$(git rev-parse origin/main)"
if [[ "$repository_state" == "dev_changes" ]]; then
  [[ "$head" == "$release_commit" && "$origin_main" == "$release_commit" ]] ||
    die "origin/main changed before the incomplete development transition could resume"
  phase="dev_changes"
  materialize_project_version "$release_commit" "${release_version}+dev"
elif [[ "$head" == "$release_commit" && "$origin_main" == "$release_commit" ]]; then
  phase="dev_changes"
  materialize_project_version "$release_commit" "${release_version}+dev"
  repository_state="dev_changes"
elif [[ "$repository_state" != "dev_local" && "$repository_state" != "dev_pushed" ]]; then
  die "origin/main changed before the post-release development commit"
fi

if [[ "$repository_state" == "dev_changes" ]]; then
  ensure_version_transition "$release_commit" "${release_version}+dev"
  cargo build --locked --bins
  ensure_versions "${release_version}+dev"
  ensure_local_binaries "${release_version}+dev"
  commit_version_change "Mark post-release builds as development versions" \
    "$release_commit" "${release_version}+dev"
  dev_commit="$(git rev-parse HEAD)"
  ensure_dev_commit "$dev_commit" "$release_commit"
  phase="dev_committed"
  repository_state="dev_local"
fi

if [[ "$repository_state" == "dev_local" ]]; then
  git fetch origin main --tags
  [[ "$(git rev-parse origin/main)" == "$release_commit" ]] ||
    die "origin/main changed before the post-release commit could be pushed"
  [[ "$(git rev-parse "$tag^{}")" == "$release_commit" ]] ||
    die "$tag no longer points to $release_commit"
  git push origin HEAD:main
  phase="dev_pushed"
  repository_state="dev_pushed"
fi

phase="dev_pushed"
wait_for_workflow ci.yml main "$dev_commit" "post-release CI"

phase="final_verification"
git fetch origin main --tags
main_tip="$(git rev-parse origin/main)"
git merge-base --is-ancestor "$dev_commit" HEAD ||
  die "HEAD does not contain post-release commit $dev_commit"
git merge-base --is-ancestor "$dev_commit" "$main_tip" ||
  die "origin/main no longer contains post-release commit $dev_commit"
mainline_dev_commit="$(first_mainline_commit_after "$release_commit" "$main_tip")"
[[ "$mainline_dev_commit" == "$dev_commit" ]] ||
  die "origin/main first-parent history no longer starts with post-release commit $dev_commit"
validate_tag
[[ "$(remote_tag_commit)" == "$release_commit" ]] ||
  die "origin tag $tag no longer points to $release_commit"
ensure_dev_commit "$dev_commit" "$release_commit"
git diff --quiet || die "tracked worktree changes remain after release"
git diff --cached --quiet || die "staged changes remain after release"

phase="complete"
printf 'Released %s at %s; post-release version %s is commit %s and origin/main is %s.\n' \
  "$tag" "$release_commit" "${release_version}+dev" "$dev_commit" "$main_tip"
