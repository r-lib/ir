# ir

`ir` runs standalone R scripts that declare their own runtime requirements in
the script itself. It resolves those requirements into cached package libraries
and runs the script against them.

```r
#!/usr/bin/env -S ir run
#| dependencies:
#|   - dplyr>=1.0
#|   - tidyr
#| r-version: ">= 4.0"
#| exclude-newer: "2024-01-15"

library(dplyr)
library(tidyr)

1 + 1
```

```console
$ ir run script.R
$ ir run --vanilla script.R
# or, if the script is executable:
$ ./script.R
```

It can also evaluate inline expressions, pull in extra dependencies from the
command line, and run package-shipped executables from `exec/`:

```console
$ ir run -e '1 + 1'                          # inline expression, empty resolved library
$ ir run --with cli -e 'cli::cli_alert_success("hi")'
$ ir run --with dplyr,tidyr script.R         # add to the script's own deps
$ ir run --r-version 4.5 script.R            # select R with rig
$ ir tool run --from btw btw --help          # run exec/btw from package btw
$ ir tool run btw --help                     # shorthand for --from btw btw
$ ir tool run --from github::r-lib/Rapp Rapp
$ ir tool install btw                        # install launchers for exec/* apps
$ ir tool install github::r-lib/Rapp
```

For `ir tool run --from pkg tool`, `ir` resolves `pkg`, finds `exec/tool` or
`exec/tool.R` in the installed package, and launches that file with the selected
Rscript. The package ref can be a pak package ref or supported version spec. Use
quotes when the shell would otherwise interpret characters such as `>`.

For `ir tool install pkg-ref`, `ir` resolves `pkg-ref`, scans that package's
`exec/` directory for files whose shebang names `Rscript` or `Rapp`, and writes
launchers into `IR_TOOL_BIN_DIR`, `RAPP_BIN_DIR`, `XDG_BIN_HOME`,
`XDG_DATA_HOME/../bin`, or `~/.local/bin` on Unix. Use `--bin-dir <dir>` for an
explicit destination and `--force` to overwrite existing launcher paths. On
Windows, the default launcher directory is
`%LOCALAPPDATA%\Programs\R\ir\bin`, falling back to
`%USERPROFILE%\AppData\Local\Programs\R\ir\bin`. `--with` and `--r-version`
are recorded in the launcher recovery command. The launchers pin `R_LIBS` to
the resolved `ir` cache library and set `R_LIBS_USER=NULL`; if
`ir cache clean` removes that library, rerun `ir tool install --force pkg-ref`.

## How it works

`ir run` and `ir tool run` run in two phases:

1. **Resolve + materialise** (a private, throw-away R session).
   - The YAML frontmatter is parsed by Rust with **saphyr**.
   - If the YAML frontmatter has `r-version: "VERSION-SPEC"` or `ir run` is
     called with `--r-version VERSION-SPEC`, the requested R version is selected
     from installed versions reported by `rig list --json`. If no installed R
     satisfies the request, `ir` selects the newest available matching R version
     to suggest a concrete `rig install` command. With `exclude-newer`, dates
     covered by the embedded available-version table do not parse JSON, consult
     the filesystem, or call `rig available`; newer dates use a cached
     `<cache>/rig/available.json`, fetching it with `rig available --json` only
     when the cache is absent.
   - A *resolution cache* short-circuits this whole phase: the declared
     dependencies plus the resolution source (and R version / platform) are
     hashed, and if that exact request was already resolved, its library is
     reused and **pak is not invoked at all**. Latest resolution folds the
     current date into the key, forcing a fresh resolution — picking up newly
     published versions — at most once a day. Dated Posit Package Manager
     snapshot resolution uses the snapshot date instead.
   - On a cache miss, the declared dependencies are resolved into concrete
     package versions with **pak** (`pak::pkg_deps`), including the full
     transitive closure.
   - If the YAML frontmatter has `exclude-newer: "YYYY-MM-DD"`, CRAN is
     resolved from the Posit Package Manager snapshot for that date:
     `https://packagemanager.posit.co/cran/YYYY-MM-DD`.
   - The resolved set is hashed (together with the R version and platform) into
     a content-addressed library path under the cache directory.
   - **renv** (`renv::use`) installs the packages into renv's package cache and
     materialises that path as a light-weight library of **symlinks** into the
     cache. The library lives in our cache, not R's temp dir, so it persists.
2. **Run**.
   - Script files run as `Rscript [Rscript-options...] script.R`, so they see
     the user's normal R environment unless forwarded Rscript options such as
     `--vanilla` disable startup files.
   - Inline expressions run as `Rscript -e <expr>`.
   - The materialised library is injected via `R_LIBS`, which **prepends** it to
     `.libPaths()`: the resolved dependencies take precedence, while the user's
     other libraries remain available as a fallback. With `--isolated`, the user
     library is dropped (`R_LIBS_USER=NULL`); the system library stays on the
     path. See [Isolated runs](#isolated-runs).
   - Package executables use their shebang to choose Rscript or Rapp execution.
     `R_LIBS` points to the resolved library, `R_LIBS_USER` is set to `NULL`,
     and `PATH` is prepended with the resolved package `exec/` directories plus
     the directory that contains the selected Rscript when it is path-like.
   - Installed tool launchers use the same Rscript/Rapp dispatch, but keep the
     resolved library path in the launcher so the tool can be run directly from
     `PATH`.

Libraries are content-addressed: two scripts that resolve to the same set of
package versions share one materialised library, and the individual packages
are shared system-wide through renv's cache.

## Quarto documents

`ir run` also renders Quarto documents (`.qmd`, `.Rmd`). Declare dependencies
under an `ir:` key in the document's YAML frontmatter:

```yaml
---
title: "My report"
ir:
  dependencies:
    - dplyr>=1.0
    - gt@1.0
  r-version: ">= 4.0"
  exclude-newer: "2024-01-15"
---
```

The `ir:` key accepts the same fields as a script's frontmatter: `dependencies`,
`r-version`, and `exclude-newer`. `r-version` selects an installed R through rig
(see above), and the document is rendered with that same R.

`ir run report.qmd` resolves those dependencies into the same cached, resolved
library used for scripts, then runs `quarto render report.qmd` with that library
and the selected R. Trailing arguments are passed to `quarto render`
(`ir run report.qmd --to pdf`); leading Rscript options are forwarded to the
knitr engine (`ir run --vanilla report.qmd`). Rscript options containing commas
cannot be forwarded to Quarto because `QUARTO_KNITR_RSCRIPT_ARGS` is
comma-separated with no escaping. Set `IR_QUARTO` to use a specific Quarto
executable.

Rendering a Quarto document needs the `rmarkdown` package (the knitr engine).
`ir` appends `rmarkdown` to the resolved dependencies automatically, pulling the
latest version (or the latest as of `exclude-newer` when set). To pin it, declare
`rmarkdown` yourself under `ir.dependencies` (e.g. `rmarkdown@2.29`); an explicit
ref overrides the injected one and silences the reproducibility advisory. When
the injected `rmarkdown` is used, `ir` prints a one-line note advising you to pin
a version.

## Cache management

`ir` exposes cache management commands:

```console
$ ir cache dir
$ ir cache clean
```

`ir cache dir` prints the cache root. `ir cache clean` removes the whole `ir`
cache, including materialised libraries and resolution markers. `ir cache clean
--force` is accepted and has the same behavior.

## Frontmatter format

The YAML frontmatter is the leading block of lines that start exactly with
`#| ` (after a single optional `#!` shebang line). Rust strips the `#| ` prefix,
parses the YAML, and passes the declared dependency specs to the R resolver on
stdin, one dependency per line. Because the block is parsed as real YAML, two
YAML rules apply:

- The `r-version:` constraint must be **quoted** — `r-version: ">= 4.0"` —
  because a bare value starting with `>` is not valid YAML.
- The `dependencies:` field is usually a YAML sequence, one package ref per
  item. A string value is also accepted and split on whitespace.

```r
#| dependencies:
#|   - dplyr>=1.0      # lower bound
#|   - tidyr           # latest
#|   - cli==3.6.6      # exact version
#| r-version: ">= 4.0" # optional; selected via rig
#| exclude-newer: "2024-01-15"  # optional; resolve from that PPM snapshot date
```

Supported dependency specs in this prototype:

| Spec         | Meaning                                          |
| ------------ | ------------------------------------------------ |
| `pkg`        | latest available                                 |
| `pkg>=1.0`   | at least version 1.0                             |
| `pkg==1.2`   | exactly version 1.2                              |

`pkg>=1.0` and `pkg==1.2` are translated to pak refs before resolution. Other
pak ref forms, such as GitHub refs and URL refs, are passed through unchanged.
Version operators that are not representable as pak refs, including `pkg<=1.2`
and `pkg!=1.2`, are not resolved by `ir`.

## Inline expressions and command-line requirements

`ir run` has flags that mirror and extend Rscript:

- **`-e <expr>`** evaluates an inline R expression *instead of* running a script
  file, just like `Rscript -e`. It can be repeated (`-e ... -e ...`), and any
  trailing arguments are passed to the program as `commandArgs(TRUE)`. An inline
  expression has no frontmatter, so it runs against an empty resolved library
  unless dependencies are supplied with `--with`. The user's libraries remain on
  `.libPaths()` unless `--isolated` is supplied.

- **`ir run -`** reads the R program from standard input and runs it as
  `Rscript -`. Standard-input programs do not have frontmatter; use `--with`,
  `--r-version`, and `--isolated` for runtime requirements.

- **`ir tool run --from <pkg-ref> <command>`** resolves a package ref and runs
  the command from that package's `exec/` directory. A bare self-named package
  ref such as `ir tool run btw` is treated as `ir tool run --from btw btw`.

- **`ir tool install <pkg-ref>`** resolves a package ref and installs launchers
  for every supported `Rscript` or `Rapp` executable in that package's `exec/`
  directory. Remote refs work directly, for example `ir tool install
  github::r-lib/Rapp`. `--with` and `--r-version` are resolved at install time
  and recorded in the launchers' recovery command.

- **`--with <pkg>`** adds a dependency for this run. It can be repeated and
  accepts a comma-separated list (`--with dplyr,tidyr`), and uses the same spec
  format as the `dependencies:` frontmatter (e.g. `cli`, `dplyr>=1.0`,
  `cli==3.6.6`). With a script file, `--with` packages are *merged* with the
  script's declared dependencies; with `-e`, they are the only dependencies.
  With `ir tool run` or `ir tool install`, they are resolved alongside the
  provider package.

- **`--r-version <spec>`** selects the R version for this run with rig. With a
  script file, it overrides `r-version:` in the frontmatter; with `-e` or
  `ir tool run`, it is the only R version requirement. With `ir tool install`,
  it selects the Rscript written into installed launchers.

```console
$ ir run --with cli -e 'cli::cli_alert_success("works")'
$ ir run --with 'dplyr>=1.1' --with tidyr -e 'library(dplyr); library(tidyr); 1'
$ ir tool run --with cli --from btw btw
$ ir tool run --from 'btw>=0.1.0' btw
$ ir tool install --with cli btw
$ ir tool install --r-version 4.5 btw
$ echo 'print(commandArgs(TRUE))' | ir run - stdin-arg
$ ir run --r-version 4.5 -e 'getRversion()'
$ ir run --vanilla --with cli script.R       # Rscript options still apply
```

`--with` packages, `ir tool run` provider packages, and `--r-version` join the
resolved set that is hashed into the content-addressed library, so a given
combination of frontmatter and command-line requirements resolves once and is
reused on later runs.

## Isolated runs

By default the resolved library is *prepended* to `.libPaths()`, so the user's
own libraries stay on the search path as a fallback. Pass `--isolated` to drop
the user library for the run:

```console
$ ir run --isolated script.R
$ ir run --isolated --with cli -e 'cli::cli_alert_success("hi")'
$ ir tool run btw
```

`--isolated` sets `R_LIBS_USER=NULL` — R's documented way to disable the user
library — so `.libPaths()` is the resolved library plus the site and base/system
libraries. Tool runs always set `R_LIBS_USER=NULL`. This stops a run from
silently borrowing an undeclared package from your personal library. The system
library stays on the path, so base and recommended packages keep working (and
anything else installed there is still visible).

## Requirements

- `R` / `Rscript` on `PATH`, a rig default R install, or `IR_RSCRIPT` when
  `r-version` is not set.
- `rig` on `PATH` when `r-version` is set.
- `quarto` on `PATH`, or `IR_QUARTO`, when rendering `.qmd` or `.Rmd` files.
- `ir` installs its own resolver tooling (`pak`, `renv`, `secretbase`) on first
  use into a dedicated library under the cache directory, so you do not need to
  pre-install them. If they are already on your R library path, `ir` uses those
  instead of installing a private copy.
- A Rust toolchain only if you build `ir` from source (not needed for the
  pre-built binaries below).

## Install

Install a pre-built binary — no Rust toolchain required.

On Linux or macOS:

```console
$ curl -fsSL https://raw.githubusercontent.com/t-kalinowski/ir/main/scripts/install.sh | sh
```

On Windows (PowerShell):

```console
> irm https://raw.githubusercontent.com/t-kalinowski/ir/main/scripts/install.ps1 | iex
```

The installer downloads the archive for your platform from the latest
[release](https://github.com/t-kalinowski/ir/releases) and installs `ir` into
`~/.local/bin` (`$HOME\bin` on Windows); set `IR_INSTALL_DIR` to choose another
directory. Pre-built binaries are provided for macOS (arm64, x86_64), Linux
(x86_64, arm64; glibc 2.35+), and Windows (x86_64). Each release also lists the
archives and a `SHA256SUMS.txt` for manual download and verification.

### Build from source

With a Rust toolchain installed:

```console
$ cargo build --release
$ cp target/release/ir ~/.local/bin/   # or anywhere on PATH
```

## Testing

```console
$ cargo test
```

`cargo test` runs the Rust CLI tests (`tests/cli.rs`). The end-to-end fixtures
under `tests/fixtures/run/` drive the R resolver (`driver/resolve.R`) through
real renders and package executions, covering pak ref normalisation,
`exclude-newer` snapshot selection, the resolution cache, and rmarkdown
injection for Quarto documents. They require an R toolchain with the fixture
packages installed (see `.github/workflows/ci.yml`) plus Quarto on `PATH`.

The end-to-end CLI tests use real R package resolution and the normal `ir`
cache. Repeat runs with a warm cache skip the download and materialisation path;
fresh CI jobs download and materialise packages when pak is called.

## Configuration

| Variable          | Used by           | Default                                          |
| ----------------- | ----------------- | ------------------------------------------------ |
| `IR_CACHE_DIR`    | all commands      | `tools::R_user_dir("ir", "cache")`               |
| `IR_RSCRIPT`      | runs without `r-version` | rig's default R install, then `Rscript` on `PATH` |
| `IR_QUARTO`       | Quarto rendering  | `quarto` on `PATH`                               |
| `IR_TOOL_BIN_DIR` | `ir tool install` | first launcher-directory override                |
| `RAPP_BIN_DIR`    | `ir tool install` | second launcher-directory override               |
| `XDG_BIN_HOME`    | `ir tool install` on Unix | third launcher-directory override          |
| `XDG_DATA_HOME`   | `ir tool install` on Unix | `<parent of XDG_DATA_HOME>/bin`             |
| `LOCALAPPDATA`    | `ir tool install` on Windows | `%LOCALAPPDATA%\Programs\R\ir\bin`     |
| `USERPROFILE`     | `ir tool install` on Windows | fallback under `%USERPROFILE%\AppData` |

The default cache directory follows R's per-package convention (e.g.
`~/Library/Caches/org.R-project.R/R/ir` on macOS), and also honours R's own
`R_USER_CACHE_DIR`.

`IR_RSCRIPT` is ignored when `r-version` is set, because version selection is
resolved through `rig`. `IR_TOOL_BIN_DIR`, `RAPP_BIN_DIR`, `XDG_BIN_HOME`,
`XDG_DATA_HOME`, `LOCALAPPDATA`, and `USERPROFILE` are only used to choose the
default destination for `ir tool install`; `--bin-dir <dir>` overrides them.

## Limitations (prototype)

- `r-version` selects installed rig-managed R versions. `ir` does not install
  missing R versions; it reports the `rig install` command to run.
- Dependency specs support bare names, `>=`, `==`, and pak package refs.
  Upper-bound syntax such as `pkg<=1.2` is not resolved by `ir`.
- Repositories default to CRAN (`https://cran.r-project.org`).
- The self-named package executable shortcut is for package refs whose package
  name can be inferred locally, such as `btw` or `btw>=0.1.0`. Use
  `ir tool run --from <pkg-ref> <command>` for remotes and other refs.
- The auto-injected `rmarkdown` for Quarto is added only when the resolved
  dependency set does not already contain it. Declaring `rmarkdown` yourself —
  directly, transitively (e.g. via `quarto`), or through any ref pak resolves to
  the `rmarkdown` package — suppresses the injected seed and its advisory.
