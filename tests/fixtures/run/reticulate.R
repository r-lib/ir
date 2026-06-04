#!/usr/bin/env -S ir run
#| dependencies:
#|   - reticulate

library(reticulate)

managed <- identical(Sys.getenv("IR_TEST_RETICULATE_MANAGED"), "1")
if (managed) {
  python_version <- Sys.getenv("IR_TEST_PYTHON_VERSION")
  stopifnot(nzchar(python_version))
  py_require(character(), python_version = python_version, action = "set")
}

json <- import("json")
config <- py_config()

lib <- normalizePath(.libPaths()[[1]], winslash = "/", mustWork = TRUE)
expected <- normalizePath(Sys.getenv("IR_EXPECT_CACHE_DIR"), winslash = "/", mustWork = FALSE)

cat("ir.fixture=reticulate\n")
cat("reticulate.lib_in_cache=", tolower(startsWith(lib, file.path(expected, "libraries"))), "\n", sep = "")
cat("reticulate.ephemeral=", tolower(isTRUE(config$ephemeral)), "\n", sep = "")
cat("reticulate.json=", json$dumps(dict(ok = TRUE)), "\n", sep = "")
