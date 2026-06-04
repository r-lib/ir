#!/usr/bin/env -S ir run
#| dependencies:
#|   - reticulate

library(reticulate)

python_version <- Sys.getenv("IR_TEST_PYTHON_VERSION")
stopifnot(nzchar(python_version))

py_require(character(), python_version = python_version, action = "set")
json <- import("json")
config <- py_config()

lib <- strsplit(Sys.getenv("R_LIBS"), .Platform$path.sep, fixed = TRUE)[[1]][[1]]
expected <- normalizePath(file.path(lib, "reticulate"), mustWork = TRUE)
pkg_in_cache <- path.package("reticulate") == expected

cat("ir.fixture=reticulate\n")
cat("reticulate.lib_in_cache=", tolower(pkg_in_cache), "\n", sep = "")
cat("reticulate.ephemeral=", tolower(isTRUE(config$ephemeral)), "\n", sep = "")
cat("reticulate.json=", json$dumps(dict(ok = TRUE)), "\n", sep = "")
