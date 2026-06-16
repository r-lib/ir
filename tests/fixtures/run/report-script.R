#' ---
#' title: "ir fixture quarto script"
#' format: html
#' ir:
#'   packages:
#'     - glue
#'   exclude-newer: 2026-06-01
#' ---

#' ## Rendered script

#| echo: false
suppressPackageStartupMessages(library(glue))
lib <- strsplit(Sys.getenv("R_LIBS"), .Platform$path.sep, fixed = TRUE)[[1]][[1]]
expected <- normalizePath(file.path(lib, "glue"), mustWork = TRUE)
loaded <- normalizePath(path.package("glue"), mustWork = TRUE)
cat("ir.fixture=render-script\n")
cat("render.script.glue_in_cache=", tolower(loaded == expected), "\n", sep = "")
cat("render.script.vanilla=", tolower("--vanilla" %in% commandArgs()), "\n", sep = "")
cat(glue::glue("render.script.result={2 + 2}"), "\n", sep = "")
