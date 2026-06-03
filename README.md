# ir

`ir` runs standalone R scripts that declare their own runtime requirements in
the script itself. It resolves those requirements into cached, isolated package
libraries and runs the script against them.

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

## How it works

`ir run script.R` runs in two phases:

1. **Resolve + materialise** (a private, throw-away R session).
   - The YAML frontmatter is parsed by Rust with **saphyr**.
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
   - If the YAML frontmatter has `r-version: "VERSION-SPEC"`, the requested R
     version is resolved with `rig available --json` and matched against
     installed versions from `rig list --json`. If `exclude-newer` is present,
     R versions released after that date are ignored.
   - The resolved set is hashed (together with the R version and platform) into
     a content-addressed library path under the cache directory.
   - **renv** (`renv::use`) installs the packages into renv's package cache and
     materialises that path as a light-weight library of **symlinks** into the
     cache. The library lives in our cache, not R's temp dir, so it persists.
2. **Run** (an ordinary R session).
   - The script runs as `Rscript [Rscript-options...] script.R`, so it sees the
     user's normal R environment unless forwarded Rscript options such as
     `--vanilla` disable startup files.
   - The materialised library is injected via `R_LIBS`, which **prepends** it to
     `.libPaths()`: the resolved dependencies take precedence, while the user's
     other libraries remain available as a fallback.

Libraries are content-addressed: two scripts that resolve to the same set of
package versions share one materialised library, and the individual packages
are shared system-wide through renv's cache.

## Cache management

`ir` exposes cache management commands:

```console
$ ir cache dir
$ ir cache clean
```

`ir cache dir` prints the cache root. `ir cache clean` removes the whole `ir`
cache, including materialised libraries and resolution markers.

## Frontmatter format

The YAML frontmatter is the leading block of lines that start exactly with
`#| ` (after a single optional `#!` shebang line). Rust strips the `#| ` prefix,
parses the YAML, and passes the declared dependency specs to the R resolver on
stdin, one dependency per line. Because the block is parsed as real YAML, two
YAML rules apply:

- The `r-version:` constraint must be **quoted** — `r-version: ">= 4.0"` —
  because a bare value starting with `>` is not valid YAML.
- The `dependencies:` field is a YAML sequence, one package ref per item.

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

## Requirements

- A Rust toolchain (to build `ir`).
- `R` / `Rscript` on `PATH` when `r-version` is not set.
- `rig` on `PATH` when `r-version` is set.
- The R packages `pak`, `renv`, and `secretbase` installed in that R.

## Build & install

```console
$ cargo build --release
$ cp target/release/ir ~/.local/bin/   # or anywhere on PATH
```

## Testing

```console
$ cargo test
```

`cargo test` runs the Rust CLI tests (`tests/cli.rs`) and, when an R toolchain
with the required test packages is available, the R resolution suite
(`tests/test-resolve.R`) — which covers pak ref normalisation, unsupported
version-operator pass-through, exotic-ref pass-through, snapshot repository
selection, cache keys, and `exclude-newer` handling. The R suite can also be
run on its own:

```console
$ Rscript -e 'testthat::test_file("tests/test-resolve.R", stop_on_failure = TRUE)'
```

## Configuration

| Variable       | Default                                          |
| -------------- | ------------------------------------------------ |
| `IR_CACHE_DIR` | `tools::R_user_dir("ir", "cache")`               |
| `IR_RSCRIPT`   | `Rscript` (resolved via `PATH`)                  |

The default cache directory follows R's per-package convention (e.g.
`~/Library/Caches/org.R-project.R/R/ir` on macOS), and also honours R's own
`R_USER_CACHE_DIR`.

## Limitations (prototype)

- Dependency specs support bare names, `>=`, `==`, and pak package refs.
  Upper-bound syntax such as `pkg<=1.2` is not resolved by `ir`.
- Repositories default to CRAN (`https://cran.r-project.org`).
