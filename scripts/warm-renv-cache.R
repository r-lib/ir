#!/usr/bin/env Rscript

args <- commandArgs(TRUE)
repos <- NULL
explicit_repos <- FALSE

if (length(args) >= 2L && identical(args[[1L]], "--repos")) {
  repos <- args[[2L]]
  explicit_repos <- TRUE
  args <- args[-c(1L, 2L)]
}

stopifnot(length(args) > 0L)

linux_os_release <- function(path = "/etc/os-release") {
  if (!file.exists(path)) return(character())

  lines <- readLines(path, warn = FALSE)
  values <- character()
  for (line in lines) {
    parts <- strsplit(line, "=", fixed = TRUE)[[1L]]
    if (length(parts) < 2L) next
    key <- parts[[1L]]
    value <- paste(parts[-1L], collapse = "=")
    values[[key]] <- gsub('^"|"$', "", value)
  }
  values
}

linux_binary_distribution <- function() {
  if (!identical(unname(Sys.info()[["sysname"]]), "Linux")) return(NULL)

  os_release <- linux_os_release()
  id <- os_release[["ID"]]
  ubuntu_codename <- os_release[["UBUNTU_CODENAME"]]
  if (!is.null(ubuntu_codename) && nzchar(ubuntu_codename))
    return(ubuntu_codename)

  codename <- os_release[["VERSION_CODENAME"]]
  if (identical(id, "ubuntu") || identical(id, "debian")) {
    if (!is.null(codename) && nzchar(codename)) return(codename)
  }

  version <- os_release[["VERSION_ID"]]
  if (is.null(id) || is.null(version) || !nzchar(version)) return(NULL)

  major <- strsplit(version, ".", fixed = TRUE)[[1L]][[1L]]
  if (identical(id, "centos")) return(paste0("centos", major))
  if (id %in% c("rhel", "rocky", "almalinux")) return(paste0("rhel", major))
  if (identical(id, "opensuse-leap"))
    return(paste0("opensuse", gsub(".", "", version, fixed = TRUE)))

  NULL
}

ppm_cran_url <- function(snapshot) {
  distro <- linux_binary_distribution()
  if (!is.null(distro))
    return(sprintf("https://packagemanager.posit.co/cran/__linux__/%s/%s",
                   distro, snapshot))

  sprintf("https://packagemanager.posit.co/cran/%s", snapshot)
}

ppm_snapshot_id <- function(url) {
  prefix <- "https://packagemanager.posit.co/cran/"
  if (!startsWith(url, prefix)) return(NULL)

  snapshot <- sub("/+$", "", substring(url, nchar(prefix) + 1L))
  if (!nzchar(snapshot) || grepl("/", snapshot, fixed = TRUE)) return(NULL)
  snapshot
}

linux_binary_repos <- function(repos) {
  cran <- repos[["CRAN"]]
  if (is.null(cran) || is.na(cran) || !nzchar(cran)) return(repos)

  snapshot <- ppm_snapshot_id(cran)
  if (is.null(snapshot)) return(repos)

  repos[["CRAN"]] <- ppm_cran_url(snapshot)
  repos
}

tooling_repos <- linux_binary_repos(
  c(CRAN = "https://packagemanager.posit.co/cran/latest")
)
repos <- if (is.null(repos)) getOption("repos") else c(CRAN = repos)
repos <- linux_binary_repos(repos)

if (explicit_repos)
  Sys.unsetenv("RENV_CONFIG_REPOS_OVERRIDE")
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
