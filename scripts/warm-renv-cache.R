#!/usr/bin/env Rscript

args <- commandArgs(TRUE)
repos <- NULL

if (length(args) >= 2L && identical(args[[1L]], "--repos")) {
  repos <- args[[2L]]
  args <- args[-c(1L, 2L)]
}

stopifnot(length(args) > 0L)

default_repos <- function() {
  repos <- getOption("repos")
  cran <- repos[["CRAN"]]
  rspm <- Sys.getenv("RSPM", unset = "")

  if (nzchar(rspm) &&
      (is.null(cran) || is.na(cran) || !nzchar(cran) ||
       identical(cran, "@CRAN@"))) {
    return(c(CRAN = rspm))
  }

  repos
}

if (is.null(repos)) {
  repos <- default_repos()
} else {
  Sys.setenv(RENV_CONFIG_REPOS_OVERRIDE = repos)
  repos <- c(CRAN = repos)
}

tooling_repos <- default_repos()
options(repos = repos, renv.consent = TRUE)

r_libs_user <- Sys.getenv("R_LIBS_USER", unset = "")
if (nzchar(r_libs_user)) {
  user_libs <- strsplit(r_libs_user, .Platform$path.sep, fixed = TRUE)[[1L]]
  user_libs <- user_libs[nzchar(user_libs)]
  for (user_lib in user_libs)
    dir.create(user_lib, recursive = TRUE, showWarnings = FALSE)
  .libPaths(c(user_libs, .libPaths()))
}

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
