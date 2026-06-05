#!/usr/bin/env -S ir run
#| dependencies:
#|   - dplyr>=1.0
#|   - tidyr
#|   - glue
#|   - jsonlite

pkgs <- c("dplyr", "tidyr", "glue", "jsonlite")
suppressPackageStartupMessages(invisible(lapply(pkgs, library, character.only = TRUE)))
lib <- strsplit(Sys.getenv("R_LIBS"), .Platform$path.sep, fixed = TRUE)[[1]][[1]]
expected <- normalizePath(file.path(lib, pkgs), mustWork = TRUE)
pkg_in_cache <- setNames(normalizePath(path.package(pkgs), mustWork = TRUE) == expected, pkgs)

data <- dplyr::tibble(group = c("a", "b", "a"), value = c(1L, 2L, 3L)) |>
  dplyr::group_by(group) |>
  dplyr::summarise(total = sum(value), .groups = "drop") |>
  tidyr::pivot_wider(names_from = group, values_from = total)

cat("ir.fixture=run-script\n")
cat("script.args=", paste(commandArgs(TRUE), collapse = "|"), "\n", sep = "")
cat("script.lib_in_cache=", tolower(all(pkg_in_cache)), "\n", sep = "")
cat("script.user_library=", Sys.getenv("R_LIBS_USER", unset = "<unset>"), "\n", sep = "")
cat("script.packages=", paste(names(pkg_in_cache), tolower(pkg_in_cache), sep = ":", collapse = ","), "\n", sep = "")
cat(glue::glue("script.result=a:{data$a},b:{data$b}"), "\n", sep = "")
cat("script.json=", jsonlite::toJSON(list(ok = TRUE, rows = nrow(data)), auto_unbox = TRUE), "\n", sep = "")
