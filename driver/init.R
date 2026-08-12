# ir script initialization driver
#
# Run by the `ir` Rust binary in a private R session. It discovers direct
# dependencies in one script and, when supplied, borrows versions and source
# provenance from the nearest renv lockfile.

ir_init_required_env <- function(name) {
  value <- Sys.getenv(name, unset = NA_character_)
  if (is.na(value) || !nzchar(value))
    stop("environment variable `", name, "` is required", call. = FALSE)
  value
}

ir_init_minor_version <- function(version) {
  parts <- strsplit(as.character(version), ".", fixed = TRUE)[[1L]]
  if (length(parts) < 2L || any(!grepl("^[0-9]+$", parts[1:2])))
    stop("invalid R version `", version, "`", call. = FALSE)
  paste(parts[1:2], collapse = ".")
}

ir_init_package_version <- function(package, version) {
  if (is.null(version) || !grepl("^[0-9]+([.-][0-9]+)*$", version))
    stop("locked package `", package, "` has invalid version `",
         version, "`", call. = FALSE)
  version
}

ir_init_record_value <- function(record, field, default = NULL) {
  value <- record[[field]]
  if (is.null(value) || !length(value) || is.na(value[[1L]]) ||
      !nzchar(as.character(value[[1L]])))
    return(default)
  as.character(value[[1L]])
}

ir_init_remote_prefix <- function(package, repo) {
  if (identical(package, repo)) "" else paste0(package, "=")
}

ir_init_repository_url_supported <- function(repository, url) {
  if (!is.character(url) || length(url) != 1L || is.na(url) || !nzchar(url))
    return(FALSE)

  repository <- tolower(repository)
  pattern <- if (repository %in% c("cran", "rspm", "ppm", "p3m")) {
    paste0(
      "^https://(?:",
      "(?:cloud|cran)\\.r-project\\.org|",
      "cran\\.rstudio\\.com|",
      "(?:packagemanager\\.posit\\.co|",
      "packagemanager\\.rstudio\\.com|p3m\\.dev)/(?:cran|all)",
      ")(?:/|$)"
    )
  } else {
    paste0(
      "^https://(?:bioconductor\\.org/packages|",
      "packagemanager\\.posit\\.co/bioconductor)(?:/|$)"
    )
  }
  grepl(pattern, url, ignore.case = TRUE, perl = TRUE)
}

ir_init_validate_repository_urls <- function(package, repositories,
                                              lock_repositories) {
  lock_names <- names(lock_repositories)
  if (!length(lock_names))
    return(invisible())

  for (repository in repositories) {
    index <- match(tolower(repository), tolower(lock_names))
    if (is.na(index))
      next
    url <- lock_repositories[[index]]
    if (!ir_init_repository_url_supported(repository, url))
      stop("locked repository package `", package,
           "` uses unsupported repository URL for `", repository, "`",
           call. = FALSE)
  }
  invisible()
}

ir_init_hosted_ref <- function(package, record, type) {
  user <- ir_init_record_value(record, "RemoteUsername")
  repo <- ir_init_record_value(record, "RemoteRepo")
  ref <- ir_init_record_value(record, "RemoteSha")
  if (is.null(user) || is.null(repo) || is.null(ref))
    stop("locked ", type, " package `", package,
         "` is missing repository or revision metadata", call. = FALSE)

  host <- ir_init_record_value(record, "RemoteHost")
  subdir <- ir_init_record_value(record, "RemoteSubdir")
  prefix <- ir_init_remote_prefix(package, repo)

  if (identical(type, "github")) {
    if (!is.null(host) && !(host %in% c("github.com", "api.github.com")))
      stop("locked GitHub package `", package,
           "` uses unsupported host `", host, "`", call. = FALSE)
    path <- paste0(user, "/", repo,
                   if (is.null(subdir)) "" else paste0("/", subdir))
    return(paste0(prefix, "github::", path, "@", ref))
  }

  if (identical(type, "gitlab")) {
    origin <- if (is.null(host)) "gitlab.com" else host
    path <- paste0(user, "/", repo,
                   if (is.null(subdir)) "" else paste0("/-/", subdir))
    return(paste0(prefix, "gitlab::https://", origin, "/", path, "@", ref))
  }

  supported_hosts <- c("bitbucket.org", "api.bitbucket.org/2.0")
  if (!is.null(host) && !(host %in% supported_hosts))
    stop("locked Bitbucket package `", package,
         "` uses unsupported host `", host, "`", call. = FALSE)
  if (!is.null(subdir))
    stop("locked Bitbucket package `", package,
         "` uses a subdirectory that ir cannot represent", call. = FALSE)
  paste0(prefix, "git::https://bitbucket.org/", user, "/", repo,
         ".git@", ref)
}

ir_init_locked_ref <- function(package, record, lock_repositories) {
  recorded_package <- ir_init_record_value(record, "Package")
  if (!identical(recorded_package, package))
    stop("lockfile record `", package, "` identifies package `",
         recorded_package, "`", call. = FALSE)

  source <- tolower(ir_init_record_value(record, "Source", "unknown"))
  type <- tolower(ir_init_record_value(record, "RemoteType", source))
  version <- ir_init_package_version(
    package,
    ir_init_record_value(record, "Version")
  )

  if (source == "bioconductor") {
    return(paste0("bioc::", package, "@", version))
  }

  if (source %in% c("repository", "cran", "bioconductor") ||
      type %in% c("standard", "cran", "repository")) {
    supported <- c("cran", "rspm", "ppm", "p3m", "bioc", "bioconductor")
    repository <- ir_init_record_value(record, "Repository")
    repositories <- if (is.null(repository)) {
      names(lock_repositories)
    } else {
      repository
    }
    ir_init_validate_repository_urls(package, repositories, lock_repositories)
    repositories <- tolower(repositories)
    if (!length(repositories) || any(!nzchar(repositories)))
      stop("locked repository package `", package,
           "` has no repository metadata", call. = FALSE)
    unsupported <- setdiff(repositories, supported)
    if (length(unsupported))
      stop("locked repository package `", package,
           "` uses unsupported repository `", unsupported[[1L]], "`",
           call. = FALSE)
    if (length(repositories) == 1L &&
        repositories %in% c("bioc", "bioconductor"))
      return(paste0("bioc::", package, "@", version))
    return(paste0(package, "==", version))
  }

  if (type %in% c("github", "gitlab", "bitbucket"))
    return(ir_init_hosted_ref(package, record, type))

  if (type == "git" || source == "git") {
    url <- ir_init_record_value(record, "RemoteUrl")
    ref <- ir_init_record_value(record, "RemoteSha")
    subdir <- ir_init_record_value(record, "RemoteSubdir")
    portable_url <- !is.null(url) && grepl(
      "^(https?|git|ssh)://[^/[:space:]]+/[^@[:space:]]+$",
      url
    )
    if (!portable_url || is.null(ref) || !is.null(subdir))
      stop("locked git package `", package,
           "` cannot be represented as a portable ir package ref",
           call. = FALSE)
    return(paste0(package, "=git::", url, "@", ref))
  }

  if (type == "url" || source == "url") {
    stop("locked URL package `", package,
         "` has no immutable revision that ir can preserve", call. = FALSE)
  }

  if (type == "local" || source == "local" || grepl("[/\\\\]", source))
    stop("locked package `", package,
         "` uses a local source that is not portable", call. = FALSE)

  stop("locked package `", package, "` uses unsupported source `",
       source, "`", call. = FALSE)
}

ir_init_main <- function() {
  script <- ir_init_required_env("IR_INIT_SCRIPT")
  result_file <- ir_init_required_env("IR_INIT_RESULT_FILE")
  lockfile <- Sys.getenv("IR_INIT_LOCKFILE", unset = NA_character_)

  ir_configure_child_tempdir()
  on.exit(ir_close_pak_remote(), add = TRUE)
  ir_ensure_tooling()

  old_options <- options(renv.dependencies.implied = list())
  on.exit(options(old_options), add = TRUE)
  deps <- renv::dependencies(
    path = script,
    root = dirname(script),
    progress = FALSE,
    errors = "fatal",
    dev = FALSE
  )
  packages <- sort(unique(deps[["Package"]]))
  r_supplied <- rownames(utils::installed.packages(
    lib.loc = .Library,
    priority = c("base", "recommended"),
    noCache = TRUE
  ))
  packages <- setdiff(
    packages,
    c("R", r_supplied)
  )

  r_version <- paste(">=", ir_init_minor_version(getRversion()))
  refs <- packages
  lockfile_used <- ""

  if (!is.na(lockfile)) {
    lock <- renv::lockfile_read(lockfile)
    records <- lock$Packages
    missing <- setdiff(packages, names(records))
    if (length(missing)) {
      stop(
        "dependencies ", paste(sprintf("`%s`", missing), collapse = ", "),
        " are not recorded in `", lockfile, "`; run renv::snapshot()",
        call. = FALSE
      )
    }
    refs <- vapply(packages, function(package) {
      ir_init_locked_ref(package, records[[package]], lock$R$Repositories)
    }, character(1), USE.NAMES = FALSE)
    lockfile_used <- lockfile
  }

  output <- c(
    paste0("r-version=", r_version),
    paste0("lockfile=", lockfile_used),
    refs
  )
  if (any(grepl("[\r\n]", output)))
    stop("generated package metadata contains a newline", call. = FALSE)
  writeLines(output, result_file, useBytes = TRUE)
  invisible()
}

if (sys.nframe() == 0L) ir_init_main()
