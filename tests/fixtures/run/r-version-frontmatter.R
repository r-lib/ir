#!/usr/bin/env -S ir run
#| r-version: "4.4.3"
#| dependencies:
#|   - jsonlite
#| exclude-newer: "2026-06-01"

library(jsonlite)
lib <- strsplit(Sys.getenv("R_LIBS"), .Platform$path.sep, fixed = TRUE)[[1]][[1]]
expected <- normalizePath(file.path(lib, "jsonlite"), mustWork = TRUE)
jsonlite_in_cache <- normalizePath(path.package("jsonlite"), mustWork = TRUE) == expected

cat("ir.fixture=r-version-frontmatter\n")
cat("version.r_version=[", as.character(getRversion()), "]\n", sep = "")
cat("version.lib_in_cache=", tolower(jsonlite_in_cache), "\n", sep = "")
cat("version.jsonlite_in_cache=", tolower(jsonlite_in_cache), "\n", sep = "")
