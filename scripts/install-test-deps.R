#!/usr/bin/env Rscript
# R packages used by the integration tests. Re-run after installing or upgrading R.

pkgs <- c(
  "pak",
  "renv",
  "secretbase",
  "cli",
  "glue",
  "jsonlite",
  "dplyr",
  "tidyr",
  "reticulate",
  "fansi",
  "htmltools",
  "knitr",
  "rmarkdown",
  "quarto",
  "btw",
  "Rapp",
  "docopt",
  "pkgsearch",
  "prettyunits"
)

missing <- pkgs[!vapply(pkgs, requireNamespace, logical(1), quietly = TRUE)]
if (length(missing)) install.packages(missing)
