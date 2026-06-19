#!/usr/bin/env bash
#
# Set up the openai/codex-universal image with everything needed to build and
# test this package after network access is disabled.
#
# Run this as the Codex environment "Setup script":
#
#   ./scripts/setup_codex_universal.sh
#
# More info:
# https://platform.openai.com/docs/codex/overview#default-universal-image

set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  echo "scripts/setup_codex_universal.sh must run as root in the codex-universal setup phase" >&2
  exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."

export DEBIAN_FRONTEND=noninteractive
export NOT_CRAN=true

append_once() {
  local file="$1"
  local line="$2"
  touch "$file"
  if ! grep -Fqx "$line" "$file"; then
    printf '%s\n' "$line" >> "$file"
  fi
}

persist_export() {
  local name="$1"
  local value="$2"
  append_once "$HOME/.bashrc" "export ${name}=${value}"
  append_once "$HOME/.profile" "export ${name}=${value}"
  append_once "$HOME/.zshrc" "export ${name}=${value}"
  append_once /etc/profile.d/ir-codex.sh "export ${name}=${value}"
}

apt-get update -qq
apt-get install -y --no-install-recommends \
  build-essential \
  ca-certificates \
  cargo \
  curl \
  dirmngr \
  gfortran \
  git \
  jq \
  libcurl4-openssl-dev \
  libssl-dev \
  libxml2-dev \
  lsb-release \
  make \
  pkg-config \
  python3 \
  python3-apt \
  python3-dbus \
  python3-gi \
  python3-pip \
  python3-venv \
  rustc \
  software-properties-common \
  wget

codename="$(lsb_release -cs)"
arch="$(dpkg --print-architecture)"

install -d -m 0755 /usr/share/keyrings

curl -fsSL https://cloud.r-project.org/bin/linux/ubuntu/marutter_pubkey.asc \
  -o /usr/share/keyrings/cran_ubuntu_key.asc
cat > /etc/apt/sources.list.d/cran-r.sources <<EOF
Types: deb
URIs: https://cloud.r-project.org/bin/linux/ubuntu
Suites: ${codename}-cran40/
Components:
Arch: ${arch}
Signed-By: /usr/share/keyrings/cran_ubuntu_key.asc
EOF

curl -fsSL https://eddelbuettel.github.io/r2u/assets/dirk_eddelbuettel_key.asc \
  -o /usr/share/keyrings/r2u.asc
cat > /etc/apt/sources.list.d/r2u.sources <<EOF
Types: deb
URIs: https://r2u.stat.illinois.edu/ubuntu
Suites: ${codename}
Components: main
Arch: ${arch}
Signed-By: /usr/share/keyrings/r2u.asc
EOF

cat > /etc/apt/preferences.d/99cranapt <<'EOF'
Package: *
Pin: release o=CRAN-Apt Project
Pin: release l=CRAN-Apt Packages
Pin-Priority: 700
EOF

apt-get update -qq
apt-get install -y --no-install-recommends r-base-core r-base-dev

Rscript -e 'options(repos = c(CRAN = "https://cloud.r-project.org")); install.packages("bspm")'
append_once /etc/R/Rprofile.site 'suppressMessages(bspm::enable())'
append_once /etc/R/Rprofile.site 'options(bspm.version.check = FALSE)'

case "$(uname -m)" in
  x86_64 | amd64) quarto_arch="amd64" ;;
  aarch64 | arm64) quarto_arch="arm64" ;;
  *)
    echo "unsupported architecture for Quarto: $(uname -m)" >&2
    exit 1
    ;;
esac

curl -fsSL "https://quarto.org/download/latest/quarto-linux-${quarto_arch}.deb" \
  -o /tmp/quarto.deb
apt-get install -y --no-install-recommends /tmp/quarto.deb
rm -f /tmp/quarto.deb

curl -fsSL https://rig.r-pkg.org/deb/rig.gpg -o /etc/apt/trusted.gpg.d/rig.gpg
cat > /etc/apt/sources.list.d/rig.list <<'EOF'
deb http://rig.r-pkg.org/deb rig main
EOF
apt-get update -qq
apt-get install -y --no-install-recommends r-rig

persist_export NOT_CRAN true

Rscript - <<'EOF'
options(
  repos = c(CRAN = "https://packagemanager.posit.co/cran/latest"),
  Ncpus = max(1L, parallel::detectCores(logical = TRUE) - 1L)
)
packages <- c(
  "pak", "renv", "secretbase", "cli", "glue", "jsonlite",
  "dplyr", "tidyr", "reticulate", "knitr", "rmarkdown", "quarto",
  "btw", "Rapp", "docopt", "pkgsearch", "prettyunits"
)
if (!requireNamespace("pak", quietly = TRUE)) {
  install.packages("pak")
}
pak::pkg_install(setdiff(packages, "pak"))
missing <- packages[!vapply(packages, requireNamespace, logical(1), quietly = TRUE)]
stopifnot(!length(missing))

prefetch_lib <- file.path(
  Sys.getenv("HOME"),
  ".cache", "ir-codex-renv-prefetch",
  paste0(getRversion(), "-", R.version$platform)
)
dir.create(prefetch_lib, recursive = TRUE, showWarnings = FALSE)
renv::install(
  setdiff(packages, "pak"),
  library = prefetch_lib,
  prompt = FALSE,
  rebuild = FALSE
)
prefetch_cli_3_6_6_lib <- file.path(prefetch_lib, "cli-3.6.6")
dir.create(prefetch_cli_3_6_6_lib, recursive = TRUE, showWarnings = FALSE)
renv::install(
  "cli@@3.6.6",
  library = prefetch_cli_3_6_6_lib,
  prompt = FALSE,
  rebuild = FALSE
)
EOF

rig add oldrel/2
mapfile -t test_r_metadata < <(python3 scripts/resolve-test-r.py oldrel/2)
rig_name="${test_r_metadata[0]}"
test_r_version="${test_r_metadata[1]}"
test_r_exclude_newer="${test_r_metadata[2]}"
test_rscript="${test_r_metadata[3]}"
cat > /tmp/ir-rig-setup.R <<'EOF'
options(repos = c(CRAN = "https://packagemanager.posit.co/cran/latest"))
if (!requireNamespace("pak", quietly = TRUE)) {
  install.packages("pak")
}
pak::pkg_install(c("renv", "secretbase", "jsonlite", "knitr", "rmarkdown"))
prefetch_lib <- file.path(
  Sys.getenv("HOME"),
  ".cache", "ir-codex-renv-prefetch",
  paste0(getRversion(), "-", R.version$platform)
)
dir.create(prefetch_lib, recursive = TRUE, showWarnings = FALSE)
renv::install(
  c("jsonlite", "knitr", "rmarkdown"),
  library = prefetch_lib,
  repos = c(CRAN = "https://packagemanager.posit.co/cran/2026-06-01"),
  prompt = FALSE,
  rebuild = FALSE
)
stopifnot(
  requireNamespace("pak", quietly = TRUE),
  requireNamespace("renv", quietly = TRUE),
  requireNamespace("secretbase", quietly = TRUE)
)
EOF
env -u R_LIBS_USER rig run -r "$rig_name" -f /tmp/ir-rig-setup.R
export IR_TEST_R_VERSION="$test_r_version"
export IR_TEST_R_EXCLUDE_NEWER="$test_r_exclude_newer"
export IR_TEST_RSCRIPT="$test_rscript"
persist_export IR_TEST_R_VERSION "$test_r_version"
persist_export IR_TEST_R_EXCLUDE_NEWER "$test_r_exclude_newer"
persist_export IR_TEST_RSCRIPT "$test_rscript"

cores="$(nproc)"
append_once "$HOME/.Renviron" 'NOT_CRAN=true'
append_once "$HOME/.Renviron" "TESTTHAT_CPUS=${cores}"
append_once "$HOME/.Rprofile" 'options(testthat.use_colours = FALSE)'
append_once "$HOME/.Rprofile" 'options(testthat.summary.omit_dots = TRUE)'

cargo fetch --locked
cargo test --locked --no-run
