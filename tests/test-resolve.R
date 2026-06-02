# Comprehensive tests for the ir resolve driver (driver/resolve.R).
#
# Run with:
#   Rscript -e 'testthat::test_file("tests/test-resolve.R", stop_on_failure = TRUE)'
#
# The driver's logic is exercised through its pure helper functions. Version
# resolution is made deterministic and offline by injecting a fake `history`
# function in place of the real pak::pkg_history lookup.

library(testthat)

# Locate and source the driver. Sourcing only defines functions: the pipeline
# is guarded by `sys.nframe() == 0L`, which is false when sourced.
locate_driver <- function() {
  env <- Sys.getenv("IR_DRIVER")
  if (nzchar(env) && file.exists(env)) return(env)
  dir <- normalizePath(getwd(), mustWork = FALSE)
  repeat {
    cand <- file.path(dir, "driver", "resolve.R")
    if (file.exists(cand)) return(cand)
    parent <- dirname(dir)
    if (identical(parent, dir)) break
    dir <- parent
  }
  stop("could not locate driver/resolve.R (set IR_DRIVER or run from the repo)")
}
source(locate_driver())

# --- fixtures ---------------------------------------------------------------

# A fixed published-version set. Includes 10.0.0 so numeric (not lexical)
# ordering is exercised: lexically "10.0.0" < "2.0.0", numerically it is larger.
VERSIONS <- c("0.5.0", "1.0.0", "1.0.5", "1.1.0", "1.1.1",
              "1.2.0", "1.2.1", "1.2.2", "2.0.0", "10.0.0")
hist_mock    <- function(pkg) VERSIONS
hist_empty   <- function(pkg) character()
hist_boom    <- function(pkg) stop("history must not be consulted here")
hist_partial <- function(pkg) c("1.1.0", "1.2", "1.2.0", "1.3.0")

ref <- function(spec, history = hist_mock) ir_to_ref(spec, history = history)

# --- bare names and lower bounds (no history lookup) ------------------------

test_that("bare names pass through and never consult history", {
  expect_equal(ref("dplyr", hist_boom), "dplyr")
  expect_equal(ref("  dplyr  ", hist_boom), "dplyr")  # trimmed
  expect_equal(ref("data.table", hist_boom), "data.table")
})

test_that(">= lower bounds are native pak refs (no history lookup)", {
  expect_equal(ref("dplyr>=1.0", hist_boom), "dplyr@>=1.0")
  expect_equal(ref("dplyr>=1.1.0", hist_boom), "dplyr@>=1.1.0")
})

# --- exact pins -------------------------------------------------------------

test_that("exact pins (== / =) match versions numerically", {
  expect_equal(ref("pkg==1.0.0"), "pkg@1.0.0")
  expect_equal(ref("pkg==2.0.0"), "pkg@2.0.0")
  expect_equal(ref("pkg==1.2"),   "pkg@1.2.0")   # 1.2 == 1.2.0
  expect_equal(ref("pkg=1.0"),    "pkg@1.0.0")   # single '=' is exact
  expect_equal(ref("pkg==10.0.0"), "pkg@10.0.0")
})

test_that("exact pins prefer a verbatim published version string", {
  # both "1.2" and "1.2.0" are numerically equal and published; keep the
  # literal request so pak gets the exact tarball.
  expect_equal(ref("pkg==1.2", hist_partial), "pkg@1.2")
})

test_that("ir_constrained_ref treats empty / '=' op as exact", {
  expect_equal(ir_constrained_ref("pkg", "",  "1.0", history = hist_mock), "pkg@1.0.0")
  expect_equal(ir_constrained_ref("pkg", "=", "1.0", history = hist_mock), "pkg@1.0.0")
})

# --- upper bounds -----------------------------------------------------------

test_that("<= picks the newest version at or below the bound", {
  expect_equal(ref("pkg<=1.2"),    "pkg@1.2.0")  # 1.2.0 == 1.2 qualifies
  expect_equal(ref("pkg<=1.1.0"),  "pkg@1.1.0")
  expect_equal(ref("pkg<=2.0.0"),  "pkg@2.0.0")  # not 10.0.0
})

test_that("< picks the newest version strictly below the bound", {
  expect_equal(ref("pkg<1.2"),    "pkg@1.1.1")   # 1.2.0 excluded
  expect_equal(ref("pkg<1.1.0"),  "pkg@1.0.5")
  expect_equal(ref("pkg<10.0.0"), "pkg@2.0.0")
})

# --- lower-strict bound -----------------------------------------------------

test_that("> picks the newest version strictly above the bound", {
  expect_equal(ref("pkg>1.2"),    "pkg@10.0.0")  # newest overall above 1.2
  expect_equal(ref("pkg>2.0.0"),  "pkg@10.0.0")
})

# --- numeric (not lexical) version ordering ---------------------------------

test_that("version selection is numeric, not lexical", {
  hist <- function(pkg) c("1.2.0", "1.9.0", "1.10.0", "1.100.0")
  # lexical max would be "1.9.0"; numeric max under 2.0.0 is "1.100.0"
  expect_equal(ref("pkg<2.0.0", hist), "pkg@1.100.0")
  expect_equal(ref("pkg<=10.0.0"),     "pkg@10.0.0")
})

# --- exotic refs pass through -----------------------------------------------

test_that("non-standard refs are passed through untouched", {
  expect_equal(ref("user/repo", hist_boom),         "user/repo")
  expect_equal(ref("user/repo@main", hist_boom),    "user/repo@main")
  expect_equal(ref("github::r-lib/cli", hist_boom), "github::r-lib/cli")
  expect_equal(ref("bioc::Biobase", hist_boom),     "bioc::Biobase")
  expect_equal(ref("url::https://x/y.tar.gz", hist_boom), "url::https://x/y.tar.gz")
})

# --- error cases ------------------------------------------------------------

test_that("unsatisfiable constraints error and report available versions", {
  expect_error(ref("pkg<0.1"),     "no version of 'pkg' satisfies '<0.1'")
  expect_error(ref("pkg==9.9"),    "satisfies '==9.9'")
  expect_error(ref("pkg>10.0.0"),  "satisfies '>10.0.0'")
  expect_error(ref("pkg<=0.0.1"),  "available:")
})

test_that("missing history falls back to a literal pin (pak validates)", {
  expect_equal(ref("pkg==1.2", hist_empty), "pkg@1.2")
  expect_equal(ref("pkg<=1.2", hist_empty), "pkg@1.2")
  expect_equal(ref("pkg<1.2",  hist_empty), "pkg@1.2")
})

# --- frontmatter extraction -------------------------------------------------

test_that("ir_frontmatter extracts the leading comment block", {
  lines <- c(
    "#!/usr/bin/env -S ir run",
    "# dependencies:",
    "#   dplyr>=1.0",
    "",
    "library(dplyr)"
  )
  expect_equal(ir_frontmatter(lines), "dependencies:\n  dplyr>=1.0")
})

test_that("ir_frontmatter drops the shebang and strips one space after #", {
  expect_equal(ir_frontmatter(c("#!sh", "#a", "#  b", "code")), "a\n b")
  expect_equal(ir_frontmatter(c("# only", "x <- 1")), "only")
  expect_equal(ir_frontmatter(c("x <- 1")), "")          # no comment block
  expect_equal(ir_frontmatter(character()), "")          # empty file
})

# --- spec parsing -----------------------------------------------------------

test_that("ir_read_spec parses YAML mappings", {
  spec <- ir_read_spec("dependencies:\n  - dplyr\n  - tidyr\nR: \">= 4.0\"")
  expect_equal(spec$dependencies, c("dplyr", "tidyr"))
  expect_equal(spec$R, ">= 4.0")
})

test_that("ir_read_spec treats non-mappings and empty input as no frontmatter", {
  expect_equal(ir_read_spec(""), list())
  expect_equal(ir_read_spec("just some prose, not a mapping"), list())
})

test_that("ir_read_spec errors on malformed YAML", {
  expect_error(ir_read_spec("a: [1, 2"), "could not parse script frontmatter as YAML")
})

# --- dependency extraction --------------------------------------------------

test_that("ir_deps handles list and folded-scalar forms", {
  expect_equal(ir_deps(list(dependencies = c("dplyr>=1.0", "tidyr"))),
               c("dplyr>=1.0", "tidyr"))
  expect_equal(ir_deps(list(dependencies = "dplyr>=1.0 tidyr secretbase==1.2")),
               c("dplyr>=1.0", "tidyr", "secretbase==1.2"))
})

test_that("ir_deps returns character(0) when there are no dependencies", {
  expect_equal(ir_deps(list()), character())
  expect_equal(ir_deps(list(dependencies = NULL)), character())
  expect_equal(ir_deps(list(dependencies = c("dplyr", "", "  ", "tidyr"))),
               c("dplyr", "tidyr"))
})

# --- R version soft-check ---------------------------------------------------

test_that("ir_check_r_version warns only on a real mismatch", {
  r46 <- numeric_version("4.6.0")
  expect_warning(ir_check_r_version(list(R = ">= 99.0"), r46), "requests R")
  expect_warning(ir_check_r_version(list(R = "== 4.0"),  r46), "requests R")
  expect_silent(ir_check_r_version(list(R = ">= 4.0"), r46))
  expect_silent(ir_check_r_version(list(R = "4.0"),    r46))   # bare implies >=
  expect_silent(ir_check_r_version(list(R = "<= 4.6"), r46))
  expect_silent(ir_check_r_version(list(), r46))               # no R key
  expect_silent(ir_check_r_version(list(R = "not-a-version"), r46))
})

# --- end-to-end glue (frontmatter -> deps -> refs), mocked history ----------

test_that("the parse -> deps -> refs pipeline composes", {
  lines <- c(
    "#!/usr/bin/env -S ir run",
    "# dependencies:",
    "#   dplyr>=1.0",
    "#   secretbase<=1.2",
    "# R: \">= 4.0\"",
    "",
    "library(dplyr)"
  )
  spec <- ir_read_spec(ir_frontmatter(lines))
  deps <- ir_deps(spec)
  expect_equal(deps, c("dplyr>=1.0", "secretbase<=1.2"))

  refs <- vapply(deps, ir_to_ref, character(1L),
                 history = hist_mock, USE.NAMES = FALSE)
  expect_equal(refs, c("dplyr@>=1.0", "secretbase@1.2.0"))
})
