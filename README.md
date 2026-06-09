# ir

`ir` runs self-describing R scripts and renders Quarto sources.

Put the packages and R version next to the code, then run the file. `ir`
resolves the requirements, prepares a cached package library, and starts R with
that library ready to use.

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

```console
$ ir run script.R
$ ./script.R
```

Full documentation: <https://t-kalinowski.github.io/ir/>

## Why use it?

- **The file explains itself.** Package requirements live in the script or document, not
  in a separate setup note.
- **Fast by design.** `ir` keeps package setup direct and reuses cached
  resolutions and libraries when the same requirements are seen again.
- **Reproducibility is explicit.** Use `r-version` to select an installed R and
  `exclude-newer` to resolve packages as of a specific date.
- **It works with normal R habits.** Forward `Rscript` options, render Quarto
  documents, evaluate inline expressions, or use `--with` for one-off packages.
- **Package tools are easy to try.** Run package executables with `rx`, or
  install persistent launchers from `exec/` without setting up a project by hand.

`ir` is designed to be small, fast, and predictable: resolve once, reuse cached
libraries aggressively, and avoid making you manage a project directory for a
one-file workflow.

## Common commands

```console
$ ir run script.R
$ ir run --vanilla script.R
$ ir render report.qmd --to html
$ ir run --with cli -e 'cli::cli_alert_success("works")'
$ ir run --r-version 4.5 script.R
$ rx btw --help
$ ir tool run --from btw btw --help
$ ir tool install btw
$ ir cache dir
```

## Install

Install a pre-built binary on Linux or macOS:

```console
$ curl -fsSL https://raw.githubusercontent.com/t-kalinowski/ir/main/scripts/install.sh | sh
```

Install on Windows PowerShell:

```console
> irm https://raw.githubusercontent.com/t-kalinowski/ir/main/scripts/install.ps1 | iex
```

The installers download the latest release and install `ir` and `rx` into
`~/.local/bin` on Unix or `$HOME\bin` on Windows. They tell you if that
directory is not on `PATH`. Set `IR_INSTALL_DIR` to choose another directory.

You can also build from source with Rust:

```console
$ cargo build --release
```

This builds `target/release/ir` and `target/release/rx`.

## Requirements

- `R` / `Rscript` on `PATH`, a rig default R install, or `IR_RSCRIPT`.
- `rig` on `PATH` when using `r-version`.
- `quarto` on `PATH`, or `IR_QUARTO`, when rendering `.qmd`, `.Rmd`, or R script files.

On first use, `ir` prepares its resolver tooling in its cache, so you do not
need to pre-install pak or renv.

## Learn more

For command details, configuration, and edge cases, see:

- [Scripts](https://t-kalinowski.github.io/ir/run.html)
- [Quarto rendering](https://t-kalinowski.github.io/ir/quarto.html)
- [Package tools](https://t-kalinowski.github.io/ir/tools.html)
- [Cache management](https://t-kalinowski.github.io/ir/cache.html)
- [Install and configuration](https://t-kalinowski.github.io/ir/config.html)
- [CLI reference](https://t-kalinowski.github.io/ir/reference.html)
