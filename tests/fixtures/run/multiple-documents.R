#!/usr/bin/env -S ir run
#| packages:
#|   - glue
#| ---
#| packages:
#|   - definitelynotapackageir

library(glue)

cat("ir.fixture=multi-doc\n")
cat(
  "multi.packages=glue:",
  tolower(requireNamespace("glue", quietly = TRUE)),
  "\n",
  sep = ""
)
cat(
  "multi.ignored_package=",
  tolower(requireNamespace("definitelynotapackageir", quietly = TRUE)),
  "\n",
  sep = ""
)
cat(glue::glue("multi.result={2 + 3}\n"))
