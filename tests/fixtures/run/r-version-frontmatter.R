#!/usr/bin/env -S ir run
#| r-version: "4.4.3"
#| dependencies:
#|   - jsonlite
#| exclude-newer: "2026-06-01"

stopifnot(requireNamespace("jsonlite", quietly = TRUE))

lib <- normalizePath(.libPaths()[[1]], winslash = "/", mustWork = TRUE)
expected <- normalizePath(Sys.getenv("IR_EXPECT_CACHE_DIR"), winslash = "/", mustWork = FALSE)
libraries <- file.path(expected, "libraries")
# jsonlite must be physically in the run library, not merely loadable from a
# system or site copy. Check the DESCRIPTION at the library path rather than
# resolving find.package(), since renv symlinks the package dir to its cache.
jsonlite_in_cache <- startsWith(lib, libraries) &&
  file.exists(file.path(lib, "jsonlite", "DESCRIPTION"))

cat("ir.fixture=r-version-frontmatter\n")
cat("version.r_version=[", as.character(getRversion()), "]\n", sep = "")
cat("version.lib_in_cache=", tolower(startsWith(lib, libraries)), "\n", sep = "")
cat("version.jsonlite_in_cache=", tolower(jsonlite_in_cache), "\n", sep = "")
