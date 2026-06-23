---
title: 'ir 0.1.0: self-describing R scripts and Quarto documents'
description: >
  `ir` is a new command-line tool for running standalone R scripts and
  rendering Quarto documents whose package requirements, and optional R
  selection, live inside the file itself.
topics: [Best Practices, Publishing]
tags/software: [R, Python, Quarto, Packages, Reproducibility, CLI]
---

Today we are announcing the first public release of `ir`, a small
command-line tool for running standalone R scripts and rendering Quarto
documents that declare their runtime requirements in the file itself.

`ir` is for the one-file workflows that do not quite need a project, but
still need to be understandable, repeatable, and easy to run later. Put
the package requirements, and optionally the R selection, next to the
code, then run the file. `ir` resolves the requirements, prepares cached
package libraries, and launches R or Quarto with the runtime ready to
use.

`ir` focuses on two workflows:

- running or rendering self-describing scripts and documents (`ir run`,
  `ir render`)
- running or installing command-line tools distributed through R packages
  (`rx`, `ir tool install`)

## Why `ir`?

R scripts often begin as small, local utilities: a report refresh, a
data pull, a model check, a quick diagnostic, or an example shared with
a colleague. Over time, the script can become important, but the setup
still lives somewhere else: in a README, in a shell history, in a
project library, or in the author's current R installation.

`ir` makes the runtime part of the source file. That means a script can
say, directly:

- which packages it needs
- which R should run it, when that needs to be explicit
- whether user libraries should be visible
- whether CRAN packages should be resolved as of a specific date

## How `ir` fits with existing tools

`ir` sits alongside the R tools people already use. For machine-level R
installs, use `rig`. For package installation inside R, use `pak` as a
faster, more capable `install.packages()`. For project libraries, use
`renv`.

`ir` builds on that stack for non-project workflows: resolving the
runtime for a self-describing script or Quarto document, and running or
installing command-line entry points distributed by R packages.

## A self-describing R script

Here is a complete script:

```r
#!/usr/bin/env -S ir run
#| packages:
#|   - dplyr>=1.0
#|   - tidyr
#| isolated: true
#| exclude-newer: "2024-01-15"

library(dplyr)
library(tidyr)

1 + 1
```

Run it with:

```console
$ ir run script.R
```

Or, on macOS and Linux, make it executable and run it directly:

```console
$ ./script.R
```

The metadata block is YAML written in `#|` comments after an optional
shebang. `ir` reads that metadata, resolves the declared packages with
`pak`, materializes a package library with `renv`, and starts R with the
resolved library at the front of `.libPaths()`.

By default, user libraries remain visible as a fallback. Add
`isolated: true` in the file, or use `--isolated` at the command line,
to run without the user library.

## Running R package tools with `rx`

The `ir` release also includes `rx`, a short alias for `ir tool run`
that runs executables provided by R packages:

```console
$ rx btw

# same as:
$ ir tool run btw
```

Package authors can put executable scripts in a package's `exec/`
directory. Those scripts can be regular `Rscript` files or
[`Rapp`](https://github.com/r-lib/Rapp) apps. `rx` resolves the package,
finds the requested executable, and runs it in an isolated library. For
tools you use regularly, `ir tool install` writes persistent launchers:

```console
$ ir tool install btw
```

This gives package-provided CLIs a shared command namespace: run them on
demand with `rx`, or promote the ones you use often to persistent
launchers with `ir tool install`.

## Cached by design

The first run of a new dependency set does the normal work of resolving
and installing packages. Later runs reuse cached resolutions and
content-addressed package libraries when the same requirements are seen
again.

That makes `ir` useful for short-lived and repeated command-line work.
You can run a script, run an inline expression, render a report, or
launch a package-provided executable without creating a project
directory just to hold the dependency state.

```console
$ ir run --with cli -e 'cli::cli_alert_success("works")'
$ ir run --r-version 4.5 script.R
$ ir render report.qmd --to html
```

`ir` also bootstraps its own resolver tooling on first use, so you do
not need to pre-install `pak` or `renv`.

## Reproducibility controls for small files

For scripts and reports that need more explicit reproducibility,
`exclude-newer` is usually the first thing to reach for:

```yaml
exclude-newer: 2024-01-15
```

You can also provide the same date at the command line:

```console
$ ir run --exclude-newer 2024-01-15 script.R
```

This resolves packages from the Posit Package Manager snapshot for that
date. When no other R selector is set, the same date also tells `ir` to
select the latest R minor version available on that date. If you were
writing with the current release of R and current CRAN packages, the
date alone is usually enough.

Use `r-version` when the script really needs a specific installed R
version or version range, or `rscript` when it needs a specific Rscript
executable:

```yaml
r-version: "4.3"
```

```yaml
rscript: "/path/to/Rscript"
```

Together, these options let a file carry the important parts of its
runtime contract without requiring a surrounding project, but most files
should only need the date.

## Python environments too

Some R scripts and Quarto documents also need Python. `ir` can resolve a
Python environment from metadata before launching the script:

```r
#!/usr/bin/env -S ir run
#| packages:
#|   - reticulate
#| python-packages:
#|   - pandas
#|   - matplotlib
#| python-version: "3.11"
#| exclude-newer: "2026-06-01"

library(reticulate)
pd <- import("pandas")
```

`ir` creates the environment with reticulate's uv-backed environment
helper, sets `RETICULATE_PYTHON`, and activates the environment for
subprocesses. For Quarto, put Python metadata under the document's
`ir:` key; `ir` passes the resolved interpreter to Quarto with
`QUARTO_PYTHON`.

When Python metadata is present, `exclude-newer` is also used for Python
environment resolution unless `python-exclude-newer` is set. Use
`python-exclude-newer` when Python packages should use a different
snapshot date from R packages.

## Quarto documents too

`ir` uses the same metadata model for Quarto documents. Put package
metadata under an `ir:` key in the document YAML:

```yaml
---
title: My report
ir:
  packages:
    - dplyr>=1.0
    - gt@1.0
  isolated: true
  exclude-newer: 2024-01-15
---
```

Then render with:

```console
$ ir render report.qmd
$ ir render report.qmd --to pdf
```

When `ir` selects an R executable by `rscript`, `r-version`, or
date-only `exclude-newer`, it pins Quarto's knitr R to that selection.
`ir` also seeds `rmarkdown` automatically for knitr-based renders unless
you declare it yourself.

## Install `ir`

Install a pre-built binary on Linux or macOS:

```console
$ curl -fsSL https://raw.githubusercontent.com/t-kalinowski/ir/main/scripts/install.sh | sh
```

Install on Windows PowerShell:

```console
> irm https://raw.githubusercontent.com/t-kalinowski/ir/main/scripts/install.ps1 | iex
```

The installers download the latest release and install both `ir` and
`rx`. You will also need `R` / `Rscript`; `rig` is required when
selecting R by version or by date-only `exclude-newer`, and Quarto is
required when rendering Quarto sources.

## Learn more

The project is open source under the MIT license. To get started:

- Read the documentation: https://t-kalinowski.github.io/ir/
- Browse the source: https://github.com/t-kalinowski/ir
- Open an issue: https://github.com/t-kalinowski/ir/issues

This is a first public release, and feedback is especially useful now.
If you try `ir` on your own scripts or Quarto documents, we would like
to hear which workflows feel natural, where the metadata model needs
more room, and which command-line edges still need smoothing.
