#!/usr/bin/env sh
#
# Install workstation dependencies for building and testing ir on macOS or
# Debian/Ubuntu Linux. This installs system tools only; the first test run still
# owns the ir/R package cache warm-up.

set -eu

TEST_R_SPEC="oldrel/2"
TEST_R_NAME=""
TEST_R_VERSION=""
TEST_R_EXCLUDE_NEWER=""
TEST_RSCRIPT=""
DRY_RUN=0
PLATFORM="auto"
SKIP_RUST=0
SKIP_PYTHON=0
SKIP_QUARTO=0
SKIP_R_RELEASE=0
SKIP_TEST_R=0

usage() {
  cat <<EOF
Usage: scripts/install-dev-deps.sh [--dry-run] [--platform macos|linux-deb] [--skip COMPONENT]

Installs Rust, Python, rig, R release, R ${TEST_R_SPEC} for tests, and Quarto.
Use scripts/install-dev-deps.ps1 on Windows.

Options:
  --dry-run           Print the commands without running them.
  --platform PLATFORM Print or run the plan for a supported platform.
  --skip COMPONENT    Skip installing rust, python, quarto, r-release, or test-r.
  -h, --help          Show this help.
EOF
}

die() {
  echo "$*" >&2
  exit 1
}

run() {
  echo "+ $*"
  if [ "$DRY_RUN" -eq 0 ]; then
    "$@"
  fi
}

run_root() {
  if [ "$DRY_RUN" -eq 1 ]; then
    run sudo "$@"
  elif [ "$(id -u)" -eq 0 ]; then
    run "$@"
  else
    require_command sudo
    run sudo "$@"
  fi
}

have_command() {
  if [ "$DRY_RUN" -eq 1 ]; then
    return 1
  fi

  command -v "$1" >/dev/null 2>&1
}

require_command() {
  if [ "$DRY_RUN" -eq 1 ]; then
    return 0
  fi

  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

skip_component() {
  case "$1" in
    rust)
      SKIP_RUST=1
      ;;
    python)
      SKIP_PYTHON=1
      ;;
    quarto)
      SKIP_QUARTO=1
      ;;
    r-release)
      SKIP_R_RELEASE=1
      ;;
    test-r)
      SKIP_TEST_R=1
      ;;
    *)
      die "unsupported skip component: $1"
      ;;
  esac
}

detect_platform() {
  case "$(uname -s)" in
    Darwin)
      echo "macos"
      ;;
    Linux)
      if [ -r /etc/os-release ]; then
        # shellcheck disable=SC1091
        . /etc/os-release
        case "${ID:-} ${ID_LIKE:-}" in
          *debian* | *ubuntu*)
            echo "linux-deb"
            return 0
            ;;
        esac
      fi
      die "unsupported Linux distribution: this script currently supports Debian/Ubuntu"
      ;;
    MINGW* | MSYS* | CYGWIN*)
      die "use scripts/install-dev-deps.ps1 on Windows"
      ;;
    *)
      die "unsupported OS: $(uname -s)"
      ;;
  esac
}

linux_quarto_arch() {
  case "$(uname -m)" in
    x86_64 | amd64)
      echo "amd64"
      ;;
    aarch64 | arm64)
      echo "arm64"
      ;;
    *)
      die "unsupported architecture for Quarto: $(uname -m)"
      ;;
  esac
}

install_rust() {
  if have_command cargo; then
    echo "cargo already installed"
  else
    require_command curl
    if [ "$DRY_RUN" -eq 1 ]; then
      rustup_tmp="/tmp/ir-rustup-init"
    else
      rustup_tmp="${TMPDIR:-/tmp}/ir-rustup-init.$$"
    fi
    run curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o "$rustup_tmp"
    run sh "$rustup_tmp" -y --default-toolchain stable
    if [ "$DRY_RUN" -eq 0 ]; then
      rm -f "$rustup_tmp"
    fi
    export PATH="${HOME}/.cargo/bin:${PATH}"
  fi

  if [ "$DRY_RUN" -eq 1 ] || have_command rustup; then
    run rustup toolchain install stable --component rustfmt --component clippy
    run rustup default stable
  elif have_command cargo; then
    echo "rustup not found; cargo is installed, skipping rustup-managed component install"
  fi
}

install_macos() {
  if [ "$SKIP_RUST" -eq 0 ]; then
    if [ "$DRY_RUN" -eq 1 ]; then
      run xcode-select --install
    elif ! xcrun -f cc >/dev/null 2>&1; then
      run xcode-select --install
      die "finish the Xcode Command Line Tools install, then rerun this script"
    fi
  fi

  if [ "$SKIP_RUST" -eq 0 ]; then
    install_rust
  fi

  if [ "$SKIP_PYTHON" -eq 0 ] && ! have_command python3; then
    require_command brew
    run brew install python
  fi

  if ! have_command rig; then
    require_command brew
    run brew tap r-lib/rig
    run brew install --cask rig
  fi

  if [ "$SKIP_QUARTO" -eq 0 ] && ! have_command quarto; then
    require_command brew
    run brew install --cask quarto
  fi
}

install_linux_deb() {
  require_command apt-get

  run_root apt-get update
  run_root apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    curl \
    gfortran \
    libcurl4-openssl-dev \
    libssl-dev \
    libxml2-dev \
    make \
    pkg-config

  if [ "$SKIP_PYTHON" -eq 0 ]; then
    run_root apt-get install -y --no-install-recommends python3 python3-venv
  fi

  if [ "$SKIP_RUST" -eq 0 ]; then
    install_rust
  fi

  if ! have_command rig; then
    if [ "$DRY_RUN" -eq 1 ]; then
      run curl -fsSL https://rig.r-pkg.org/deb/rig.gpg -o /tmp/ir-rig.gpg
      run sudo install -m 0644 /tmp/ir-rig.gpg /etc/apt/trusted.gpg.d/rig.gpg
      echo "+ write /tmp/ir-rig.list: deb http://rig.r-pkg.org/deb rig main"
      run sudo install -m 0644 /tmp/ir-rig.list /etc/apt/sources.list.d/rig.list
    else
      rig_key="${TMPDIR:-/tmp}/ir-rig.$$".gpg
      rig_list="${TMPDIR:-/tmp}/ir-rig.$$".list
      run curl -fsSL https://rig.r-pkg.org/deb/rig.gpg -o "$rig_key"
      printf '%s\n' "deb http://rig.r-pkg.org/deb rig main" >"$rig_list"
      run_root install -m 0644 "$rig_key" /etc/apt/trusted.gpg.d/rig.gpg
      run_root install -m 0644 "$rig_list" /etc/apt/sources.list.d/rig.list
      rm -f "$rig_key" "$rig_list"
    fi
    run_root apt-get update
    run_root apt-get install -y --no-install-recommends r-rig
  fi

  if [ "$SKIP_QUARTO" -eq 0 ] && ! have_command quarto; then
    if [ "$DRY_RUN" -eq 1 ]; then
      quarto_deb="/tmp/ir-quarto.deb"
    else
      quarto_deb="${TMPDIR:-/tmp}/ir-quarto.deb"
    fi
    run curl -fsSL "https://quarto.org/download/latest/quarto-linux-$(linux_quarto_arch).deb" -o "$quarto_deb"
    run_root apt-get install -y --no-install-recommends "$quarto_deb"
    if [ "$DRY_RUN" -eq 0 ]; then
      rm -f "$quarto_deb"
    fi
  fi
}

install_r_versions() {
  if [ "$DRY_RUN" -eq 0 ] && ! have_command rig; then
    die "rig is not on PATH after installation; restart the shell and rerun this script"
  fi

  if [ "$SKIP_R_RELEASE" -eq 0 ]; then
    run rig add release
  fi
  if [ "$SKIP_TEST_R" -eq 0 ]; then
    run rig add "$TEST_R_SPEC"
  fi
}

load_test_r_metadata() {
  if [ "$SKIP_TEST_R" -eq 1 ]; then
    return
  fi

  if [ "$DRY_RUN" -eq 1 ]; then
    TEST_R_NAME="<rig-name-for-${TEST_R_SPEC}>"
    TEST_R_VERSION="<resolved-${TEST_R_SPEC}-version>"
    TEST_R_EXCLUDE_NEWER="<release-date-for-${TEST_R_SPEC}>"
    TEST_RSCRIPT="<Rscript-for-${TEST_R_SPEC}>"
    return
  fi

  require_command python3
  metadata_file="${TMPDIR:-/tmp}/ir-test-r-metadata.$$"
  python3 scripts/resolve-test-r.py "$TEST_R_SPEC" >"$metadata_file"
  TEST_R_NAME="$(sed -n '1p' "$metadata_file")"
  TEST_R_VERSION="$(sed -n '2p' "$metadata_file")"
  TEST_R_EXCLUDE_NEWER="$(sed -n '3p' "$metadata_file")"
  TEST_RSCRIPT="$(sed -n '4p' "$metadata_file")"
  rm -f "$metadata_file"
  [ -n "$TEST_R_NAME" ] || die "scripts/resolve-test-r.py did not return a rig name"
  [ -n "$TEST_R_VERSION" ] || die "scripts/resolve-test-r.py did not return an R version"
  [ -n "$TEST_R_EXCLUDE_NEWER" ] || die "scripts/resolve-test-r.py did not return an R release date"
  [ -n "$TEST_RSCRIPT" ] || die "scripts/resolve-test-r.py did not return an Rscript path"
}

verify_install() {
  run cargo --version
  run rustc --version
  run python3 --version
  run rig --version
  run Rscript --version
  if [ "$SKIP_TEST_R" -eq 0 ] && [ "$DRY_RUN" -eq 1 ]; then
    run rig list --json
    test_r_name="$TEST_R_NAME"
  elif [ "$SKIP_TEST_R" -eq 0 ]; then
    test_r_name="$TEST_R_NAME"
    [ -n "$test_r_name" ] || die "test R metadata was not loaded"
  fi
  if [ "$SKIP_TEST_R" -eq 0 ]; then
    run rig run -r "$test_r_name" -e "stopifnot(as.character(getRversion()) == '${TEST_R_VERSION}')"
  fi
  run quarto --version
}

persist_github_env() {
  if [ "$DRY_RUN" -eq 1 ]; then
    return
  fi

  if [ -n "${GITHUB_ENV:-}" ] && [ "$SKIP_TEST_R" -eq 0 ]; then
    printf 'IR_TEST_R_VERSION=%s\n' "$TEST_R_VERSION" >>"$GITHUB_ENV"
    printf 'IR_TEST_R_EXCLUDE_NEWER=%s\n' "$TEST_R_EXCLUDE_NEWER" >>"$GITHUB_ENV"
    printf 'IR_TEST_RSCRIPT=%s\n' "$TEST_RSCRIPT" >>"$GITHUB_ENV"
  fi
}

print_next_steps() {
  cat <<EOF

Developer dependencies are installed.
EOF

  if [ "$SKIP_TEST_R" -eq 1 ]; then
    return
  fi

  cat <<EOF
To enable the version-selection tests in this shell, run:

  export IR_TEST_R_VERSION=${TEST_R_VERSION}
  export IR_TEST_R_EXCLUDE_NEWER=${TEST_R_EXCLUDE_NEWER}
  export IR_TEST_RSCRIPT=${TEST_RSCRIPT}

Then run:

  cargo test
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      ;;
    --platform)
      shift
      [ "$#" -gt 0 ] || die "--platform requires a value"
      PLATFORM="$1"
      ;;
    --skip)
      shift
      [ "$#" -gt 0 ] || die "--skip requires a value"
      skip_component "$1"
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
  shift
done

if [ "$PLATFORM" = "auto" ]; then
  PLATFORM="$(detect_platform)"
fi

case "$PLATFORM" in
  macos)
    install_macos
    ;;
  linux-deb)
    install_linux_deb
    ;;
  *)
    die "unsupported platform: $PLATFORM"
    ;;
esac

install_r_versions
load_test_r_metadata
verify_install
persist_github_env
print_next_steps
