#!/usr/bin/env Rscript

args <- commandArgs(TRUE)
repos <- NULL
snapshot <- NULL
explicit_repos <- FALSE

if (length(args) >= 2L && identical(args[[1L]], "--repos")) {
  repos <- args[[2L]]
  explicit_repos <- TRUE
  args <- args[-c(1L, 2L)]
}
if (length(args) >= 2L && identical(args[[1L]], "--snapshot")) {
  snapshot <- args[[2L]]
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

named_value <- function(values, name) {
  if (is.null(values) || !(name %in% names(values))) return(NULL)
  unname(values[[name]])
}

linux_binary_distribution <- function() {
  if (!identical(unname(Sys.info()[["sysname"]]), "Linux")) return(NULL)

  os_release <- linux_os_release()
  id <- named_value(os_release, "ID")
  ubuntu_codename <- named_value(os_release, "UBUNTU_CODENAME")
  ubuntu_supported <- c("xenial", "bionic", "focal", "jammy", "noble",
                        "resolute")
  if (!is.null(ubuntu_codename) && ubuntu_codename %in% ubuntu_supported)
    return(ubuntu_codename)

  codename <- named_value(os_release, "VERSION_CODENAME")
  if (identical(id, "ubuntu")) {
    if (!is.null(codename) && codename %in% ubuntu_supported) return(codename)
  }
  if (identical(id, "debian")) {
    debian_supported <- c("buster", "bullseye", "bookworm", "trixie")
    if (!is.null(codename) && codename %in% debian_supported) return(codename)
  }
  if (is.null(id)) return(NULL)

  if (id %in% c("opensuse-leap", "sles")) {
    suse_supported <- c("15.6" = "opensuse156")
    if (identical(id, "sles"))
      suse_supported <- c(suse_supported, "15.7" = "opensuse156")
    version <- named_value(os_release, "VERSION_ID")
    distro <- if (!is.null(version)) suse_supported[[version]] else NULL
    if (!is.null(distro)) return(distro)
  }
  if (identical(id, "centos")) {
    centos_supported <- c("7" = "centos7", "8" = "centos8")
    version <- named_value(os_release, "VERSION_ID")
    major <- if (!is.null(version)) strsplit(version, ".", fixed = TRUE)[[1L]][[1L]] else NULL
    distro <- centos_supported[[major]]
    if (!is.null(distro)) return(distro)
  }
  if (id %in% c("rhel", "redhat", "rocky", "almalinux")) {
    rhel_supported <- c("7" = "centos7", "8" = "centos8", "9" = "rhel9",
                        "10" = "rhel10")
    version <- named_value(os_release, "VERSION_ID")
    major <- if (!is.null(version)) strsplit(version, ".", fixed = TRUE)[[1L]][[1L]] else NULL
    distro <- rhel_supported[[major]]
    if (!is.null(distro)) return(distro)
  }

  NULL
}

configure_ppm_user_agent <- function(repos) {
  cran <- named_value(repos, "CRAN")
  if (is.null(cran) || is.na(cran) || !grepl("/__linux__/", cran, fixed = TRUE))
    return(invisible())

  user_agent <- sprintf(
    "R/%s R (%s)",
    getRversion(),
    paste(getRversion(), R.version["platform"], R.version["arch"],
          R.version["os"])
  )
  options(HTTPUserAgent = user_agent)

  method <- getOption("download.file.method")
  if (identical(method, "curl") || identical(method, "wget")) {
    extra <- getOption("download.file.extra", "")
    if (is.null(extra)) extra <- ""
    if (!grepl(user_agent, extra, fixed = TRUE)) {
      extra <- trimws(paste(extra, "--user-agent", shQuote(user_agent)))
      options(download.file.extra = extra)
    }
  }

  invisible()
}

configure_renv_cache_prefix <- function() {
  distro <- linux_binary_distribution()
  if (is.null(distro) || nzchar(Sys.getenv("RENV_PATHS_PREFIX", unset = "")))
    return(invisible())

  Sys.setenv(RENV_PATHS_PREFIX = distro)
  invisible()
}

plain_ppm_snapshot <- function(url) {
  if (is.null(url) || is.na(url)) return(NULL)

  url <- sub("/+$", "", url)
  prefix <- "https://packagemanager.posit.co/cran/"
  if (!startsWith(url, prefix)) return(NULL)

  snapshot <- substring(url, nchar(prefix) + 1L)
  if (!nzchar(snapshot) || grepl("/", snapshot, fixed = TRUE)) return(NULL)
  snapshot
}

ppm_cran_url <- function(snapshot) {
  distro <- linux_binary_distribution()
  if (!is.null(distro))
    return(sprintf("https://packagemanager.posit.co/cran/__linux__/%s/%s",
                   distro, snapshot))

  sprintf("https://packagemanager.posit.co/cran/%s", snapshot)
}

default_repos <- function(repos) {
  cran <- named_value(repos, "CRAN")
  snapshot <- plain_ppm_snapshot(cran)
  if (is.null(cran) || is.na(cran) || !nzchar(cran) || identical(cran, "@CRAN@"))
    snapshot <- "latest"

  if (!is.null(snapshot)) {
    if (is.null(repos))
      return(c(CRAN = ppm_cran_url(snapshot)))
    repos[["CRAN"]] <- ppm_cran_url(snapshot)
  }
  repos
}

tooling_repos <- c(CRAN = ppm_cran_url("latest"))
repos <- if (!is.null(snapshot)) {
  c(CRAN = ppm_cran_url(snapshot))
} else if (is.null(repos)) {
  default_repos(getOption("repos"))
} else {
  default_repos(c(CRAN = repos))
}

Sys.unsetenv("RENV_CONFIG_REPOS_OVERRIDE")
options(repos = repos, renv.consent = TRUE)
configure_ppm_user_agent(repos)
configure_renv_cache_prefix()

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
if (length(missing)) {
  configure_ppm_user_agent(tooling_repos)
  utils::install.packages(missing, repos = tooling_repos)
}

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
