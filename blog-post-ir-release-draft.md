---
title: 'ir 0.1.0: self-describing R scripts and Quarto documents'
description: >
  `ir` is a new command-line tool for running portable R scripts and
  rendering Quarto documents whose package requirements, and optional R
  selection, live inside the file itself.
topics: [Best Practices, Publishing]
tags/software: [R, Python, Quarto, Packages, Reproducibility, CLI]
---

Today we are announcing the first public release of `ir`, a small
command-line tool that runs R scripts and renders Quarto documents using
runtime requirements declared in the file itself.

`ir` is for one-file workflows that do not quite need a project but
still need to be easy to share and rerun later. Simply put the package
requirements, and optionally the R selection, next to the code. When you
render or execute the file, `ir` resolves the requirements, prepares
cached package libraries, and launches R or Quarto with a runtime ready
to use.

The interface is inspired by
[PEP 723](https://peps.python.org/pep-0723/) and
[`uv run --script`](https://docs.astral.sh/uv/). A script can carry
enough metadata to describe its runtime, and the runner can resolve that
runtime into a cached environment on demand. `ir` brings that pattern to
R scripts and Quarto documents, using `pak`, `renv`, and `rig` on the R
side, and `reticulate`'s uv-backed helper when Python is part of the
runtime.

`ir` focuses on two workflows:

- running or rendering self-describing scripts and documents \
  (`ir run`, `ir render`)
- running or installing command-line tools distributed through R
  packages (`rx`, `ir tool install`)

## Why `ir`?

R scripts often begin as small, local utilities: a report, a data pull,
a model-training or evaluation run, a quick diagnostic, or an example
shared with a colleague. Over time, the script can become important, but
the setup still lives somewhere else: in a README, in a shell history,
in a project library, or in the author's current R installation.

`ir` makes the runtime specification part of the source file. That means
a script can say, directly:

- which packages it needs
- which R should run it, when that needs to be explicit
- whether user libraries should be visible
- whether CRAN packages should be resolved as of a specific date

This should help you rerun the script reliably at a later date. Keeping
the metadata in the file makes it less likely to be lost or fall out of
sync with the code.

## How `ir` fits with existing tools

`ir` sits alongside the R tools people already use and builds on several
of them directly:

- [`rig`](https://github.com/r-lib/rig) installs, removes, and switches
  between R versions on macOS, Windows, and Linux. `ir` calls `rig` when
  a file requests a specific R version, or when date-only
  `exclude-newer` needs to select the latest R minor version available
  on that date. `rig` is optional for files that only declare packages.
- [`pak`](https://pak.r-lib.org/) is a fast package installer with a
  built-in solver. `ir` uses `pak` to resolve the dependency graph from
  a file's declared packages and fetch them from the appropriate
  repositories. `pak` is bootstrapped automatically on first use, so you
  do not need to install it separately.
- [`renv`](https://rstudio.github.io/renv/) gives R projects isolated
  package libraries and lockfiles. `ir` uses `renv`'s global package
  cache to assemble reusable libraries without creating a `renv` project
  or lockfile.

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

mtcars |> count(cyl, gear) |> pivot_wider(names_from = gear, values_from = n)
```

Run it with:

```console
$ ir run script.R
```

Or, on macOS and Linux, make it executable and run it directly:

```console
$ chmod +x script.R
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

Package authors can expose command-line entry points through standard
package subdirectories such as [`exec/` and `bin/`](
  https://cran.r-project.org/doc/manuals/r-release/R-exts.html#Package-subdirectories-1
). Files in `exec/` can be regular `Rscript` files,
[`Rapp`](https://github.com/r-lib/Rapp) apps, or direct executable
scripts. `rx` resolves the package, finds the requested executable, and
runs it in an isolated library. For tools you use regularly,
`ir tool install` writes persistent launchers.

For example, this resolves the `btw` CLI from the `btw` R package on
demand and runs `btw --help`:

```console
$ rx btw --help
```

To install `btw` so it is generally available:

```console
# run once
$ ir tool install btw

# now you only need
$ btw --help
```

This gives package-provided CLIs a shared command namespace: run them on
demand with `rx`, or promote the ones you use often to persistent
launchers with `ir tool install`.

## Cached by design

The first run of a new dependency set does the normal work of resolving
and installing packages. Later runs reuse cached resolutions and
content-addressed package libraries when the same requirements are seen
again.

That makes `ir` useful for both one-off and repeated command-line work.
You can run a script, run an inline expression, render a report, or
launch a package-provided executable without creating a project
directory just to hold the dependency state.

`ir` also bootstraps its own resolver tooling on first use, so you do
not need to pre-install `pak` or `renv`.

## Reproducibility without a project

For R scripts that need more explicit reproducibility, `exclude-newer`
is usually the first thing to reach for:

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
version or version range. This selection uses
[`rig`](https://github.com/r-lib/rig):

```yaml
r-version: "4.3"
```

Together, these options let a file carry the important parts of its
runtime requirements without needing a surrounding project. For
reproducibility, most files should need only a list of packages and a
date.

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
  exclude-newer: 2025-05-15
---
```

Then render with:

```console
$ ir render report.qmd
$ ir render report.qmd --to pdf
```

When `ir` selects an R executable using `--r-version`, frontmatter
`ir.r-version`, or date-only `exclude-newer`, it sets [`QUARTO_R`](
  https://quarto.org/docs/advanced/environment-vars.html#variables-quarto-inspects
) so Quarto renders with that R. `ir` also seeds `rmarkdown`
automatically for knitr-based renders unless you declare it yourself.

## Python environments too

Some R scripts and Quarto documents also need Python. In an R script,
declare the R packages and Python requirements in the same metadata
block:

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

`ir` creates the environment with `reticulate`'s uv-backed environment
helper, sets `RETICULATE_PYTHON`, and activates the environment for
subprocesses. Declare `reticulate` under `packages` when the R script
loads `reticulate`.

For Quarto, put Python metadata under the document's `ir:` key. A knitr
document that uses reticulate can mix R and Python requirements:

```yaml
---
title: My report
ir:
  packages:
    - reticulate
  python-packages:
    - pandas
  exclude-newer: 2025-01-01
---
```

For a Quarto document that uses the Jupyter engine, the `ir:` metadata
can contain only Python requirements:

```yaml
---
title: My notebook
jupyter: python3
ir:
  python-packages:
    - matplotlib
    - pandas
  python-version: "3.11"
---
```

For Quarto renders, `ir` injects `jupyter` into the Python environment,
passes the resolved interpreter to Quarto with `QUARTO_PYTHON`, and also
sets `RETICULATE_PYTHON` so documents that use reticulate see the same
interpreter. If the document uses the knitr engine and contains Python
chunks, `ir` automatically adds `reticulate` to the R package manifest.

When Python metadata is present, `exclude-newer` is also used for Python
environment resolution unless `python-exclude-newer` is set. Use
`python-exclude-newer` when Python packages should use a different
snapshot date from R packages.

## Install `ir`

Install a pre-built binary on Linux or macOS:

```console
$ curl -fsSL https://raw.githubusercontent.com/r-lib/ir/main/scripts/install.sh | sh
```

Install on Windows PowerShell:

```console
> irm https://raw.githubusercontent.com/r-lib/ir/main/scripts/install.ps1 | iex
```

The installers download the latest GitHub release and install both `ir`
and `rx`. You can also download release artifacts from the
[GitHub releases](https://github.com/r-lib/ir/releases) page or with
`gh release download`.

You will also need `R` / `Rscript`; `rig` is required when selecting R
by version or by date-only `exclude-newer`, and Quarto is required when
rendering Quarto sources.

## Learn more

The project is open source under the MIT license. To get started:

- Read the documentation: https://r-lib.github.io/ir/
- Browse the source: https://github.com/r-lib/ir
- Open an issue: https://github.com/r-lib/ir/issues

This is a first public release, and feedback is especially useful now.
If you try `ir` on your own scripts or Quarto documents, we would like
to hear which workflows feel natural, where the metadata model needs
more room, and which command-line edges still need smoothing.
