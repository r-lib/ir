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

# --- exclude-after snapshots -----------------------------------------------

test_that("ir_exclude_after reads an optional YYYY-MM-DD date", {
  expect_null(ir_exclude_after(NULL))
  expect_equal(ir_exclude_after("2024-01-15"), "2024-01-15")
  expect_equal(ir_exclude_after(" 2024-01-15 "), "2024-01-15")
})

test_that("ir_exclude_after rejects malformed dates", {
  expect_error(ir_exclude_after("2024-01"), "YYYY-MM-DD")
  expect_error(ir_exclude_after("2024-02-31"), "YYYY-MM-DD")
})

test_that("ir_repos uses a PPM snapshot when exclude after is present", {
  expect_equal(ir_repos("2024-01-15"),
               c(CRAN = "https://packagemanager.posit.co/cran/2024-01-15"))
})

test_that("ir_repos keeps the CRAN fallback when exclude after is absent", {
  withr::with_options(list(repos = c(CRAN = "@CRAN@")), {
    expect_equal(ir_repos(), c(CRAN = "https://cran.r-project.org"))
  })
})

# --- R version soft-check ---------------------------------------------------

test_that("ir_check_r_version warns only on a real mismatch", {
  r46 <- numeric_version("4.6.0")
  expect_warning(ir_check_r_version(">= 99.0", r46), "requests R")
  expect_warning(ir_check_r_version("== 4.0",  r46), "requests R")
  expect_silent(ir_check_r_version(">= 4.0", r46))
  expect_silent(ir_check_r_version("4.0",    r46))   # bare implies >=
  expect_silent(ir_check_r_version("<= 4.6", r46))
  expect_silent(ir_check_r_version(NULL, r46))        # no R key
  expect_silent(ir_check_r_version("not-a-version", r46))
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

test_that("ir_input_key separates dated PPM snapshots from daily latest resolution", {
  daily <- ir_input_key(c("dplyr"), as.Date("2026-06-02"), "4.6.0", "aarch64")
  snap1 <- ir_input_key(c("dplyr"), as.Date("2026-06-02"), "4.6.0", "aarch64",
                        exclude_after = "2024-01-15")
  snap2 <- ir_input_key(c("dplyr"), as.Date("2026-06-03"), "4.6.0", "aarch64",
                        exclude_after = "2024-01-15")
  expect_false(daily == snap1)
  expect_identical(snap1, snap2)
})

# --- dependency refs ---------------------------------------------------------

test_that("dependency specs are normalized to refs", {
  deps <- c("dplyr>=1.0", "secretbase==1.2")
  refs <- vapply(deps, ir_to_ref, character(1L), USE.NAMES = FALSE)
  expect_equal(refs, c("dplyr@>=1.0", "secretbase@1.2"))
})
