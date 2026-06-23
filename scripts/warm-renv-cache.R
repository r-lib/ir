#!/usr/bin/env Rscript

args <- commandArgs(TRUE)
repos <- NULL

if (length(args) >= 2L && identical(args[[1L]], "--repos")) {
  repos <- args[[2L]]
  args <- args[-c(1L, 2L)]
}

stopifnot(length(args) > 0L)

resolver_tooling_repos <- function() {
  c(CRAN = "https://packagemanager.posit.co/cran/latest")
}

ppm_latest_repos <- function() {
  c(CRAN = unname(pak::repo_resolve("PPM@latest")[[1L]]))
}

named_value <- function(values, name) {
  if (is.null(values) || is.null(names(values)) || !(name %in% names(values)))
    return(NULL)
  unname(values[[name]])
}

public_ppm_latest_url <- function(repo)
  identical(sub("/+$", "", repo), "https://packagemanager.posit.co/cran/latest")

default_repos <- function() {
  repos <- getOption("repos")
  if (is.null(repos) || !length(repos))
    return(ppm_latest_repos())

  if (is.null(names(repos))) {
    if (length(repos) == 1L) names(repos) <- "CRAN"
    else return(repos)
  }

  cran <- named_value(repos, "CRAN")
  if (is.null(cran) || is.na(cran) || !nzchar(cran) ||
      identical(cran, "@CRAN@") || public_ppm_latest_url(cran))
    repos[["CRAN"]] <- ppm_latest_repos()[["CRAN"]]

  repos
}

tooling_repos <- resolver_tooling_repos()
options(repos = tooling_repos, renv.consent = TRUE)

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

if (is.null(repos)) {
  Sys.unsetenv("RENV_CONFIG_REPOS_OVERRIDE")
  repos <- default_repos()
} else {
  Sys.setenv(RENV_CONFIG_REPOS_OVERRIDE = repos)
  repos <- c(CRAN = repos)
}

options(repos = repos, renv.consent = TRUE)

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
