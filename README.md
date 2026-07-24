# ir

`ir` runs self-describing R scripts and renders or previews Quarto sources.

Put the packages and R version next to the code, then run the file.
`ir` resolves the requirements, prepares a cached package library, and starts R with that library ready to use.

```r
#!/usr/bin/env -S ir run
#| packages:
#|   - dplyr>=1.0
#|   - tidyr
#| r-version: ">= 4.0"
#| isolated: true
#| exclude-newer: "2024-01-15"

library(dplyr)
library(tidyr)

1 + 1
```

```sh
ir run script.R
./script.R
```

Full documentation: <https://r-lib.github.io/ir/>

## Why use it?

- **The file explains itself.** R and Python package requirements live in the script or document, not in a separate setup note.
- **Fast by design.** `ir` keeps package setup direct and reuses cached resolutions and libraries when the same requirements are seen again.
- **Reproducibility is explicit.** Use frontmatter `r-version`, `--r-version`, or `IR_R_VERSION` to select R by version. If no matching R is installed, `ir` downloads it into its private cache with rig. Use `--rscript` or `IR_RSCRIPT` only when you need a machine-local Rscript override. Use `--exclude-newer`, `IR_EXCLUDE_NEWER`, or frontmatter `exclude-newer` to resolve packages as of a specific date. When `exclude-newer` is set without an R selection, `ir` selects the latest R minor version available on that date.
- **It works with normal R habits.** Forward `Rscript` options, render or preview Quarto documents, evaluate inline expressions, or use `--with` for one-off packages.
- **Package tools are easy to try.** Run package executables with `rx`, or install persistent launchers backed by a durable tool store.

`ir` is designed to be small, fast, and predictable: resolve once, reuse cached libraries aggressively, and avoid making you manage a project directory for a one-file workflow.

## Common commands

```sh
ir run script.R
ir run --vanilla script.R
ir render report.qmd --to html
ir preview report.qmd
ir run --with cli -e 'cli::cli_alert_success("works")'
ir run --r-version 4.5 script.R
ir run --exclude-newer 2024-01-15 script.R
rx btw --help
ir tool run --from btw btw --help
ir tool install btw
ir cache dir
```

## Install

Install a pre-built binary on Linux or macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/r-lib/ir/main/scripts/install.sh | sh
```

Install on Windows with Scoop using the `r-bucket` bucket:

```powershell
scoop bucket add r-bucket https://github.com/cderv/r-bucket.git
scoop install ir
```

Alternatively, install the Windows binary directly from PowerShell:

```powershell
irm https://raw.githubusercontent.com/r-lib/ir/main/scripts/install.ps1 | iex
```

The direct installers download the latest release and install `ir` and `rx` into `~/.local/bin` on Unix or `$HOME\bin` on Windows.
On macOS, the default `~/.local/bin` directory is added to `~/.zprofile` when needed.
On Windows, the install directory is added to the user `PATH`.
On Linux, the installer tells you if the install directory is not on `PATH`.
If a rig build with user-mode support is not on `PATH`, the installers print guidance for installing the development version from source.
Set `IR_NO_MODIFY_PATH=1` to skip PATH changes.
Set `IR_INSTALL_DIR` to choose another directory.

You can also build from source with Rust:

```sh
cargo build --release
```

This builds `target/release/ir` and `target/release/rx`.

## Development setup

To install the system dependencies needed to build the project and run tests on
a new machine, run:

```sh
scripts/install-dev-deps.sh
```

On Windows PowerShell, run:

```powershell
.\scripts\install-dev-deps.ps1
```

The setup scripts install Rust, Python, rig, the current R release, rig's
`oldrel/2` R for the version-selection tests, and Quarto. They do not run tests
or pre-warm package caches. Pass `--dry-run` on Unix or `-DryRun` on Windows to
inspect the plan.

## Requirements

- `R` / `Rscript` on `PATH`, or `--rscript`/`IR_RSCRIPT`, when R is not selected by version or date.
- The development version of `rig` on `PATH` when using `r-version`, `IR_R_VERSION`, `--r-version`, or date-only `exclude-newer` R selection. `ir` reuses matching rig-managed R installations and downloads missing versions into its cache in rig user mode, without administrator privileges.
- `quarto` on `PATH`, or `IR_QUARTO`, when rendering or previewing `.qmd`, `.Rmd`, or R script files.

Install the development rig with `cargo install --git https://github.com/r-lib/rig --locked --force rig`, then put Cargo's install `bin` directory before any older rig on `PATH`.
Symbolic selectors such as `release` and `oldrel/2` keep their resolved version in the `ir` cache, so later runs can reuse it without contacting rig's resolver.

On macOS and Windows, rig user mode may also update its normal per-user RStudio, `PATH`, or registry integration state. `ir cache clean` removes the cached R installation, but not that integration state.

On first use, `ir` prepares its resolver tooling in its cache, so you do not need to pre-install pak or renv.

## Learn more

For command details, configuration, and edge cases, see:

- [Scripts](https://r-lib.github.io/ir/run.html)
- [Quarto rendering and preview](https://r-lib.github.io/ir/quarto.html)
- [Package tools](https://r-lib.github.io/ir/tools.html)
- [Cache management](https://r-lib.github.io/ir/cache.html)
- [Install and configuration](https://r-lib.github.io/ir/config.html)
- [CLI reference](https://r-lib.github.io/ir/reference.html)

## License

MIT. See [LICENSE](LICENSE).
