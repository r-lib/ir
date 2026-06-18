#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

url="https://api.r-hub.io/rversions/r-versions"
tmp="${TMPDIR:-/tmp}/ir-r-versions-$$.json"
trap 'rm -f "$tmp"' EXIT INT HUP TERM

curl -fsSL "$url" -o "$tmp"
mv "$tmp" src/rig/r-versions.json
