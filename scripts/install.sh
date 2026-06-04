#!/usr/bin/env sh
# Install a pre-built `ir` binary on Linux or macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/t-kalinowski/ir/main/scripts/install.sh | sh
#
# Downloads the archive for this machine's platform from the latest GitHub
# Release, verifies it runs, and installs `ir` into $IR_INSTALL_DIR
# (default ~/.local/bin). Override the destination with IR_INSTALL_DIR=/some/dir.
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
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *) echo "add ${INSTALL_DIR} to your PATH to run ${APP}" ;;
esac
