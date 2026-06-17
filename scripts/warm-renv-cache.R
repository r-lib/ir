#!/usr/bin/env Rscript

args <- commandArgs(TRUE)
repos <- NULL

if (length(args) >= 2L && identical(args[[1L]], "--repos")) {
  repos <- args[[2L]]
  args <- args[-c(1L, 2L)]
}

stopifnot(length(args) > 0L)

tooling_repos <- c(CRAN = "https://packagemanager.posit.co/cran/latest")
if (is.null(repos)) {
  repos <- getOption("repos")
  cran <- repos[["CRAN"]]
  if (is.null(cran) || is.na(cran) || !nzchar(cran) || identical(cran, "@CRAN@"))
    repos <- tooling_repos
} else {
  repos <- c(CRAN = repos)
}

options(repos = repos, renv.consent = TRUE)

tooling <- c("pak", "renv", "secretbase")
missing <- tooling[!vapply(tooling, requireNamespace, logical(1), quietly = TRUE)]
if (length(missing))
  utils::install.packages(missing, repos = tooling_repos)

project <- tempfile("ir-renv-warm-project-")
library <- tempfile("ir-renv-warm-library-")
dir.create(project, recursive = TRUE, showWarnings = FALSE)
dir.create(library, recursive = TRUE, showWarnings = FALSE)
old_wd <- setwd(project)
on.exit(setwd(old_wd), add = TRUE)
on.exit(unlink(project, recursive = TRUE, force = TRUE), add = TRUE)
on.exit(unlink(library, recursive = TRUE, force = TRUE), add = TRUE)

do.call(renv::use, c(
  as.list(args),
  list(
    library = library,
    repos = repos,
    attach = FALSE,
    sandbox = FALSE,
    isolate = TRUE,
    verbose = TRUE
  )
))
