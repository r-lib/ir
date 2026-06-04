# ir resolve driver
#
# Run by the `ir` Rust binary in a private, throw-away R session.
#
#   IR_RESOLVE_RESULT_FILE=<result_file> Rscript resolve.R
#
# Responsibilities (steps 1-4 of the `ir` pipeline):
#   1. Consume package dependency specs from stdin, one dependency per line.
#   2. Resolve the declared dependencies into concrete versions with pak.
#   3. Hash the resolved set to derive a content-addressed library path
#      under <cache_dir>.
#   4. Materialise that path as a light-weight library of symlinks into
#      renv's package cache via renv::use().
#
# The resulting library path is written to the temp result file named by
# IR_RESOLVE_RESULT_FILE. stdout/stderr stay available for pak progress.
# This session then exits; the Rust process launches the user's script in a
# fresh, isolated R session pointed at the library.
#
# The helpers below are pure and side-effect free so they can be unit tested
# (see tests/test-resolve.R). The pipeline runs only when this file is executed
# as a script -- `sys.nframe() == 0L` is false when the file is sourced.

## --- resolver input ---------------------------------------------------------

ir_env_optional <- function(name) {
  value <- Sys.getenv(name, unset = NA_character_)
  if (is.na(value) || !nzchar(value)) NULL else value
}

# Optional date-bounded resolution. `exclude-newer` is a YAML mapping key whose
# value is an ISO date; resolution then uses that day's Posit Package Manager
# CRAN snapshot instead of the latest CRAN repository.
ir_exclude_newer <- function(value) {
  if (is.null(value)) return(NULL)

  value <- trimws(as.character(value)[[1L]])
  if (!grepl("^[0-9]{4}-[0-9]{2}-[0-9]{2}$", value))
    stop("`exclude-newer` must be a date string in YYYY-MM-DD format",
         call. = FALSE)

  value
}

## --- pak ref normalisation --------------------------------------------------

# Translate one dependency spec into a pak package reference:
#   `pkg`         -> `pkg`         (latest)
#   `pkg>=1.0`    -> `pkg@>=1.0`   (lower bound; solver picks)
#   `pkg==1.0`    -> `pkg@1.0`     (exact version)
# Native pak refs, GitHub refs, and URL refs are passed through untouched.
# Unsupported version operators such as `pkg<=1.2` are also passed to pak
# unchanged, so pak remains the source of truth for supported refs.
ir_to_ref <- function(d) {
  d <- trimws(d)
  m <- regmatches(d, regexec(
    "^([A-Za-z][A-Za-z0-9.]*[A-Za-z0-9])[[:space:]]*(>=|==)[[:space:]]*([0-9][0-9.-]*)$",
    d
  ))[[1L]]
  if (length(m) != 4L) return(d)
  if (m[[3L]] == ">=") sprintf("%s@>=%s", m[[2L]], m[[4L]])
  else sprintf("%s@%s", m[[2L]], m[[4L]])
}

## --- cache location ---------------------------------------------------------

# The cache root: the standard per-package user cache directory, overridable
# with IR_CACHE_DIR. Holds `libraries/` (materialised libraries) and
# `resolutions/` (the resolution request cache).
ir_cache_dir <- function() {
  env <- Sys.getenv("IR_CACHE_DIR")
  if (nzchar(env)) env else tools::R_user_dir("ir", "cache")
}

## --- repositories -----------------------------------------------------------

ir_ppm_snapshot_url <- function(exclude_newer) {
  sprintf("https://packagemanager.posit.co/cran/%s", exclude_newer)
}

ir_repos <- function(exclude_newer = NULL, repos = getOption("repos")) {
  if (!is.null(exclude_newer))
    return(c(CRAN = ir_ppm_snapshot_url(exclude_newer)))

  cran <- if (!is.null(repos)) repos[["CRAN"]] else NULL
  if (is.null(cran) || is.na(cran) || !nzchar(cran) || identical(cran, "@CRAN@"))
    c(CRAN = "https://cran.r-project.org")
  else
    repos
}

## --- resolution cache -------------------------------------------------------

# Key identifying a resolution request: the declared dependency specs (order
# independent), the resolution source, and the R version / platform. Latest
# resolution includes the current day so newly published versions are picked up
# at most once per day. Dated PPM snapshot resolution uses only the snapshot date
# because that repository state is immutable. Order independent so reordering
# deps doesn't bust the cache.
ir_input_key <- function(deps,
                         date          = Sys.Date(),
                         rversion      = getRversion(),
                         platform      = R.version$platform,
                         exclude_newer = NULL) {
  source_key <- if (is.null(exclude_newer))
    as.character(date)
  else
    sprintf("exclude-newer: %s", exclude_newer)

  secretbase::sha256(paste(c(sort(deps),
                             source_key,
                             as.character(rversion),
                             platform),
                           collapse = "\n"))
}

## --- pipeline ---------------------------------------------------------------

ir_resolve_main <- function() {

  deps        <- readLines(file("stdin"), warn = FALSE)
  result_file <- ir_env_optional("IR_RESOLVE_RESULT_FILE")
  package_result_file <- ir_env_optional("IR_RESOLVE_PACKAGE_RESULT_FILE")
  stopifnot(!is.null(result_file))
  cache_dir   <- ir_cache_dir()

  ## 1. Consume inputs parsed by Rust from script frontmatter
  exclude_newer <- ir_exclude_newer(ir_env_optional("IR_EXCLUDE_NEWER"))
  repos <- ir_repos(exclude_newer)
  options(repos = repos)

  ## 1b. Resolution cache: if this exact request was resolved already and its
  ## library still exists, reuse it and skip pak entirely. The marker is written
  ## only after a successful materialise (below), so its presence implies a
  ## complete library.
  primary_ref <- if (length(deps)) ir_to_ref(deps[[1L]]) else NULL
  marker <- file.path(cache_dir, "resolutions",
                      ir_input_key(deps, exclude_newer = exclude_newer))
  package_marker <- if (!is.null(primary_ref)) {
    file.path(cache_dir, "resolutions",
              paste0(basename(marker), "-primary-", secretbase::sha256(primary_ref)))
  } else {
    NULL
  }
  if (file.exists(marker)) {
    cached <- readLines(marker, n = 1L, warn = FALSE)
    if (length(cached) && nzchar(cached) && dir.exists(cached)) {
      if (!is.null(package_result_file) &&
          (is.null(package_marker) || !file.exists(package_marker))) {
        # The library is reusable, but this caller needs primary-package
        # metadata that older cache entries did not record.
      } else {
        writeLines(cached, result_file)
        if (!is.null(package_result_file)) {
          package <- readLines(package_marker, n = 1L, warn = FALSE)
          writeLines(package, package_result_file)
        }
        return(invisible())
      }
    }
  }

  ## 2. Resolve with pak
  # A script may legitimately declare no dependencies; it then gets an empty
  # but still isolated library (base R only), so undeclared library() calls
  # fail loudly instead of silently borrowing the user's packages.
  primary_package <- NULL
  if (length(deps)) {
    refs_in <- vapply(deps, ir_to_ref, character(1L), USE.NAMES = FALSE)
    res <- pak::pkg_deps(refs_in, dependencies = NA, upgrade = TRUE)

    failed <- res[res$status != "OK", , drop = FALSE]
    if (nrow(failed))
      stop("pak could not resolve: ",
           paste(failed$ref, collapse = ", "), call. = FALSE)

    if (!is.null(package_result_file)) {
      primary <- unique(res$package[res$direct & res$ref == refs_in[[1L]]])
      if (length(primary) != 1L)
        stop("package ref must resolve to exactly one R package: ",
             deps[[1L]], call. = FALSE)
      primary_package <- primary[[1L]]
    }

    # Drop base / recommended packages: those are supplied by R itself.
    keep <- is.na(res$priority) | !(res$priority %in% c("base", "recommended"))
    res <- res[keep, , drop = FALSE]

    pkgs     <- res$package
    resolved <- sort(unique(sprintf("%s@%s", res$package, res$version)))
  } else {
    pkgs     <- character()
    resolved <- character()
    if (!is.null(package_result_file))
      stop("cannot resolve a primary package without dependencies",
           call. = FALSE)
  }

  ## 3. Hash the resolved set -> content-addressed library path
  # Bind the hash to the R version and platform: the symlinks point into the
  # renv cache, whose layout is itself keyed by R version and platform.
  key <- paste(c(resolved,
                 as.character(getRversion()),
                 R.version$platform),
               collapse = "\n")
  library_path <- file.path(cache_dir, "libraries", secretbase::sha256(key))

  ## 4. Materialise the symlinked library via renv::use()
  # Skip when the library already holds every resolved package: repeat runs of
  # an unchanged script then cost nothing beyond resolution.
  dir.create(library_path, recursive = TRUE, showWarnings = FALSE)
  have <- list.files(library_path)
  if (length(pkgs) && !all(pkgs %in% have)) {
    # renv::use() installs into the renv cache and links the packages into
    # `library` as symlinks. Because `library` lives in our cache (not the R
    # temp dir), renv leaves it in place when the session ends.
    do.call(renv::use, c(
      as.list(resolved),
      list(
        library = library_path,
        repos   = repos,
        attach  = FALSE,
        sandbox = FALSE,
        isolate = TRUE,
        verbose = TRUE
      )
    ))
  }

  ## 4b. Record the resolution so an identical request skips pak.
  dir.create(dirname(marker), recursive = TRUE, showWarnings = FALSE)
  writeLines(library_path, marker)
  if (!is.null(primary_package)) {
    writeLines(primary_package, package_marker)
  }

  writeLines(library_path, result_file)
  if (!is.null(package_result_file)) {
    writeLines(primary_package, package_result_file)
  }
  invisible()
}

if (sys.nframe() == 0L) ir_resolve_main()
