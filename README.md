# ir

`ir` runs self-describing R scripts and renders or previews Quarto sources.

Put the packages and R version next to the code, then run the file.
`ir` resolves the requirements, prepares a cached package library, and starts R with that library ready to use.

```r
#!/usr/bin/env -S ir run
#| packages:
#|   - dplyr
#|   - tidyr==1.3.1
#| r-version: ">= 4.3"
#| isolated: true
#| exclude-newer: "2024-02-01"

airquality |> tidyr::drop_na(Ozone) |> dplyr::count(Month)
```

```sh
ir run script.R
./script.R
```

This records a last-known-good environment: tidyr is pinned to the version
tested with the script, while the snapshot date selects dplyr and the
transitive dependencies.

To convert an existing R script, initialize it in place:

```sh
ir init --script analysis.R
```

`ir` statically discovers direct package uses and inserts the shebang and
frontmatter. The generated R requirement uses the inspecting Rscript's current
major and minor as a lower bound, while packages supplied by that R are
omitted. It also records the current UTC date as `exclude-newer`, bounding
package resolution to the state available when the script was initialized. On
Unix, `ir` also makes the script executable. When the script is inside an renv
project, `ir` uses the nearest `renv.lock` to pin those direct requirements and
supported remote sources. Use `--no-project` to generate bare package
requirements instead.

Full documentation: <https://r-lib.github.io/ir/>

## Why use it?

- **The file explains itself.** R and Python package requirements live in the script or document, not in a separate setup note.
- **Fast by design.** `ir` keeps package setup direct and reuses cached resolutions and libraries when the same requirements are seen again.
- **Reproducibility is explicit.** Use frontmatter `r-version`, `--r-version`, or `IR_R_VERSION` to select R by version. Use `--rscript` or `IR_RSCRIPT` only when you need a machine-local Rscript override. Use `--exclude-newer`, `IR_EXCLUDE_NEWER`, or frontmatter `exclude-newer` to resolve the default CRAN and Bioconductor repositories from Posit Package Manager snapshots as of a specific date. Without another R selector, the date selects the latest R minor released by then. When `r-version` can match more than one R minor, the date limits selection to minor versions released by then.
- **It works with normal R habits.** Forward `Rscript` options, render or preview Quarto documents, evaluate inline expressions, or use `--with` for one-off packages.
- **Package tools are easy to try.** Run package executables with `rx`, or install persistent launchers backed by a durable tool store.

`ir` is designed to be small, fast, and predictable: resolve once, reuse cached libraries aggressively, and avoid making you manage a project directory for a one-file workflow.

## Common commands

```sh
ir init --script script.R
ir run script.R
ir run --vanilla script.R
ir render report.qmd --to html
ir preview report.qmd
ir run --with cli -e 'cli::cli_alert_success("works")'
ir run --with BiocGenerics -e 'library(BiocGenerics)'
ir run --r-version 4.3 script.R
ir run --exclude-newer 2024-02-01 script.R
rx btw --help
ir tool run --from btw btw --help
ir tool install btw
ir cache dir
```

Bioconductor packages use their bare package names.
pak selects the Bioconductor release compatible with the selected R.

## Install

Install from PyPI with `uv`:

```sh
uv tool install r-lib-ir
```

This installs both `ir` and `rx`. If the `uv` tool executable directory is
not already on `PATH`, run `uv tool update-shell`.

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
If `rig` is not on `PATH`, the installers print platform-specific rig install guidance.
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

The setup scripts install Rust, Python, rig, the current R release, R 4.3 for
the version-selection and documentation example tests, and Quarto. They do not
run tests or pre-warm package caches. Pass `--dry-run` on Unix or `-DryRun` on
Windows to inspect the plan.

## Requirements

- `Rscript` on `PATH`, or `IR_RSCRIPT`, for `ir init` dependency discovery.
- `R` / `Rscript` on `PATH`, or `--rscript`/`IR_RSCRIPT`, when R is not selected by version or date.
- `rig` on `PATH` when using `r-version`, `IR_R_VERSION`, `--r-version`, or date-only `exclude-newer` R selection.
- `quarto` on `PATH`, or `IR_QUARTO`, when rendering or previewing `.qmd`, `.Rmd`, or R script files.

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
