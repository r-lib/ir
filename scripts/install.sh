#!/usr/bin/env sh
# Install a pre-built `ir` binary on Linux or macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/t-kalinowski/ir/main/scripts/install.sh | sh
#
# Downloads the archive for this machine's platform from the latest GitHub
# Release, verifies it runs, and installs `ir` and `rx` into $IR_INSTALL_DIR
# (default ~/.local/bin). Override the destination with IR_INSTALL_DIR=/some/dir.
# On macOS, the default install directory is added to ~/.zprofile when needed.
set -eu

OWNER="t-kalinowski"
REPO="ir"
APP="ir"

# Linux binaries are built against glibc 2.35 (Ubuntu 22.04). Refuse to install
# on older systems where the binary would fail to load, with a clear message.
require_supported_glibc() {
  case "$TARGET" in
    *-unknown-linux-gnu) ;;
    *) return 0 ;;
  esac

  glibc_version="$(getconf GNU_LIBC_VERSION 2>/dev/null || true)"
  case "$glibc_version" in
    glibc\ *) glibc_version="${glibc_version#glibc }" ;;
    *) return 0 ;; # Can't determine; let the runtime verification catch it.
  esac

  glibc_major="${glibc_version%%.*}"
  glibc_minor="${glibc_version#*.}"
  glibc_minor="${glibc_minor%%.*}"
  if [ "$glibc_major" -lt 2 ] || { [ "$glibc_major" -eq 2 ] && [ "$glibc_minor" -lt 35 ]; }; then
    echo "unsupported glibc ${glibc_version}; ${APP}-${TARGET} needs glibc 2.35+ (Ubuntu 22.04-compatible)" >&2
    echo "build from source instead: https://github.com/${OWNER}/${REPO}#install" >&2
    exit 1
  fi
}

path_has_dir() {
  case ":${PATH}:" in
    *":$1:"*) return 0 ;;
    *) return 1 ;;
  esac
}

show_path_hint() {
  echo "add ${INSTALL_DIR} to your PATH to run ${commands}"
}

zprofile_path() {
  if [ -n "${ZDOTDIR:-}" ]; then
    printf '%s/.zprofile\n' "$ZDOTDIR"
  else
    printf '%s/.zprofile\n' "$HOME"
  fi
}

zprofile_display() {
  profile="$1"
  if [ "$profile" = "${HOME}/.zprofile" ]; then
    printf '~/.zprofile\n'
  else
    printf '%s\n' "$profile"
  fi
}

profile_has_macos_path_lines() {
  [ -f "$1" ] && grep -F 'export PATH="$HOME/.local/bin:$PATH"' "$1" >/dev/null 2>&1
}

write_macos_path_lines() {
  profile="$1"
  {
    printf '\n'
    printf 'case ":$PATH:" in\n'
    printf '  *:"$HOME/.local/bin":*) ;;\n'
    printf '  *) export PATH="$HOME/.local/bin:$PATH" ;;\n'
    printf 'esac\n'
  } >>"$profile"
}

ensure_install_dir_on_path() {
  if path_has_dir "$INSTALL_DIR"; then
    return 0
  fi

  if [ -n "${IR_NO_MODIFY_PATH:-}" ]; then
    show_path_hint
    return 0
  fi

  case "$OS" in
    Darwin) ;;
    *)
      show_path_hint
      return 0
      ;;
  esac

  default_install_dir="${HOME}/.local/bin"
  if [ "$INSTALL_DIR" != "$default_install_dir" ]; then
    show_path_hint
    return 0
  fi

  profile="$(zprofile_path)"
  profile_display="$(zprofile_display "$profile")"
  if profile_has_macos_path_lines "$profile"; then
    echo "~/.local/bin PATH setup is already present in ${profile_display}, but ~/.local/bin is still not on PATH."
    echo "restart your shell, or run: source ${profile_display}"
  elif write_macos_path_lines "$profile"; then
    echo "Added ~/.local/bin to PATH in ${profile_display}."
    echo "restart your shell, or run: source ${profile_display}"
  else
    echo "could not add ~/.local/bin to PATH in ${profile_display}" >&2
    show_path_hint
    return 0
  fi

  PATH="${INSTALL_DIR}:${PATH}"
  export PATH
}

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux) os="unknown-linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *)
    echo "unsupported OS: $OS (no pre-built binary; build from source)" >&2
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *)
    echo "unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

TARGET="${arch}-${os}"
require_supported_glibc

URL="https://github.com/${OWNER}/${REPO}/releases/latest/download/${APP}-${TARGET}.tar.gz"
INSTALL_DIR="${IR_INSTALL_DIR:-${HOME}/.local/bin}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

archive="${TMPDIR}/${APP}.tar.gz"
extracted="${TMPDIR}/${APP}-${TARGET}/${APP}"
extracted_rx="${TMPDIR}/${APP}-${TARGET}/rx"

echo "downloading ${APP}-${TARGET} ..."
curl -fsSL "$URL" -o "$archive"
tar -xzf "$archive" -C "$TMPDIR"

# Verify the binary actually runs on this machine before installing it.
if ! "$extracted" --help >/dev/null 2>&1; then
  echo "downloaded ${APP}-${TARGET} does not run on this system" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
install "$extracted" "${INSTALL_DIR}/${APP}"
echo "installed ${APP} to ${INSTALL_DIR}/${APP}"

commands="${APP}"
if [ -f "$extracted_rx" ]; then
  if ! "$extracted_rx" --help >/dev/null 2>&1; then
    echo "downloaded rx from ${APP}-${TARGET} does not run on this system" >&2
    exit 1
  fi
  install "$extracted_rx" "${INSTALL_DIR}/rx"
  echo "installed rx to ${INSTALL_DIR}/rx"
  commands="${APP} and rx"
fi

ensure_install_dir_on_path
