# Comprehensive tests for the ir resolve driver (driver/resolve.R).
#
# Run with:
#   Rscript -e 'testthat::test_file("tests/test-resolve.R", stop_on_failure = TRUE)'
#
# The driver's logic is exercised through its pure helper functions.

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

ref <- function(spec) ir_to_ref(spec)

# --- bare names and lower bounds (no history lookup) ------------------------

test_that("bare names pass through and never consult history", {
  expect_equal(ref("dplyr"), "dplyr")
  expect_equal(ref("  dplyr  "), "dplyr")  # trimmed
  expect_equal(ref("data.table"), "data.table")
})

test_that(">= lower bounds are native pak refs (no history lookup)", {
  expect_equal(ref("dplyr>=1.0"), "dplyr@>=1.0")
  expect_equal(ref("dplyr>=1.1.0"), "dplyr@>=1.1.0")
})

test_that("== exact pins are native pak refs", {
  expect_equal(ref("pkg==1.2"), "pkg@1.2")
  expect_equal(ref("pkg==1.2.0"), "pkg@1.2.0")
})

# --- native pak refs and unsupported version syntax -------------------------

test_that("native pak refs pass through untouched", {
  expect_equal(ref("pkg@1.2.3"), "pkg@1.2.3")
  expect_equal(ref("pkg@>=1.2.3"), "pkg@>=1.2.3")
})

test_that("non-pak version operators are not rewritten by ir", {
  expect_equal(ref("pkg<=1.2"), "pkg<=1.2")
  expect_equal(ref("pkg<1.2"), "pkg<1.2")
  expect_equal(ref("pkg>1.2"), "pkg>1.2")
  expect_equal(ref("pkg!=1.2"), "pkg!=1.2")
  expect_equal(ref("pkg=1.2"), "pkg=1.2")
})

# --- exotic refs pass through -----------------------------------------------

test_that("non-standard refs are passed through untouched", {
  expect_equal(ref("user/repo"),         "user/repo")
  expect_equal(ref("user/repo@main"),    "user/repo@main")
  expect_equal(ref("github::r-lib/cli"), "github::r-lib/cli")
  expect_equal(ref("bioc::Biobase"),     "bioc::Biobase")
  expect_equal(ref("url::https://x/y.tar.gz"), "url::https://x/y.tar.gz")
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

# --- cache location ---------------------------------------------------------

test_that("ir_cache_dir defaults to R_user_dir and honours IR_CACHE_DIR", {
  withr::with_envvar(c(IR_CACHE_DIR = NA), {
    expect_identical(ir_cache_dir(), tools::R_user_dir("ir", "cache"))
  })
  withr::with_envvar(c(IR_CACHE_DIR = "/tmp/ir-test-cache"), {
    expect_identical(ir_cache_dir(), "/tmp/ir-test-cache")
  })
})

# --- resolution cache key ---------------------------------------------------

test_that("ir_input_key is deterministic and order independent", {
  d <- as.Date("2026-06-02")
  k1 <- ir_input_key(c("dplyr>=1.0", "tidyr"), d, "4.6.0", "aarch64")
  k2 <- ir_input_key(c("tidyr", "dplyr>=1.0"), d, "4.6.0", "aarch64")  # reordered
  expect_identical(k1, k2)
  expect_match(k1, "^[0-9a-f]{64}$")  # sha256 hex
})

test_that("ir_input_key changes with date, deps, R version, and platform", {
  base <- ir_input_key(c("dplyr"), as.Date("2026-06-02"), "4.6.0", "aarch64")
  expect_false(base == ir_input_key(c("dplyr"), as.Date("2026-06-03"), "4.6.0", "aarch64"))
  expect_false(base == ir_input_key(c("dplyr>=1.0"), as.Date("2026-06-02"), "4.6.0", "aarch64"))
  expect_false(base == ir_input_key(c("dplyr"), as.Date("2026-06-02"), "4.5.0", "aarch64"))
  expect_false(base == ir_input_key(c("dplyr"), as.Date("2026-06-02"), "4.6.0", "x86_64"))
})

# --- end-to-end glue (frontmatter -> deps -> refs) --------------------------

test_that("the parse -> deps -> refs pipeline composes", {
  lines <- c(
    "#!/usr/bin/env -S ir run",
    "# dependencies:",
    "#   dplyr>=1.0",
    "#   secretbase==1.2",
    "# R: \">= 4.0\"",
    "",
    "library(dplyr)"
  )
  spec <- ir_read_spec(ir_frontmatter(lines))
  deps <- ir_deps(spec)
  expect_equal(deps, c("dplyr>=1.0", "secretbase==1.2"))

  refs <- vapply(deps, ir_to_ref, character(1L), USE.NAMES = FALSE)
  expect_equal(refs, c("dplyr@>=1.0", "secretbase@1.2"))
})
