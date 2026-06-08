# ir resolve driver
#
# Run by the `ir` Rust binary in a private, throw-away R session.
#
#   IR_RESOLVE_RESULT_FILE=<result_file> Rscript resolve.R
#
# Responsibilities (steps 1-4 of the `ir` pipeline):
#   1. Consume pak package refs from stdin, one ref per line.
#   2. Resolve the declared dependencies into concrete versions with pak.
#   3. Hash the resolved set to derive a content-addressed library path
#      under <cache_dir>.
#   4. Materialise that path as a light-weight library of symlinks into
#      renv's package cache via renv::use().
#
# The resulting library path is written to the temp result file named by
# IR_RESOLVE_RESULT_FILE. stdout/stderr stay available for pak progress.
# This session then exits; the Rust process launches the user's script in a
# fresh R session with the resolved library prepended to `.libPaths()`.
#
# The helpers below are pure and side-effect free. The pipeline runs only when
# this file is executed as a script -- `sys.nframe() == 0L` is false when the
# file is sourced. End-to-end coverage lives in the Rust CLI tests
# (tests/cli.rs), which drive this resolver through real renders and package
# executions.

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

# Resolve dependency refs with pak, stopping if any ref fails to resolve.
ir_resolve_refs <- function(refs) {
  res <- pak::pkg_deps(refs, dependencies = NA, upgrade = TRUE)
  failed <- res[res$status != "OK", , drop = FALSE]
  if (nrow(failed))
    stop("pak could not resolve: ",
         paste(failed$ref, collapse = ", "), call. = FALSE)
  res
}

## --- cache location ---------------------------------------------------------

# The cache root: the standard per-package user cache directory, overridable
# with IR_CACHE_DIR. Holds `libraries/` (materialised libraries) and
# `resolutions/` (the resolution request cache).
ir_cache_dir <- function() {
  env <- Sys.getenv("IR_CACHE_DIR")
  if (nzchar(env)) env else tools::R_user_dir("ir", "cache")
}

## --- resolver tooling bootstrap ---------------------------------------------

# Packages the resolver itself needs. pak resolves dependencies, renv
# materialises the library, secretbase hashes the cache keys. They are
# installed into a dedicated tooling library so users need not pre-install them.
ir_tooling_packages <- function() c("pak", "renv", "secretbase")

# Repository for tooling installs: always the latest PPM snapshot, independent
# of the user's `exclude-newer`. ir's own tooling is not pinned to a user's
# reproducibility date. PPM serves binaries for Windows and macOS.
ir_tooling_repos <- function()
  c(CRAN = "https://packagemanager.posit.co/cran/latest")

# Tooling packages not loadable from the current library paths. Uses
# requireNamespace so a user who already has pak/renv/secretbase anywhere on
# their search path pays nothing.
ir_missing_tooling <- function(packages = ir_tooling_packages())
  Filter(function(p) !requireNamespace(p, quietly = TRUE), packages)

# Path to the tooling library, keyed by R version and platform so compiled
# packages match the running R, mirroring renv's cache layout.
ir_tooling_lib <- function(cache_dir = ir_cache_dir())
  file.path(cache_dir, "tooling",
            paste0(getRversion(), "-", R.version$platform))

# Ensure pak/renv/secretbase are available. Any that are missing are installed
# into the tooling library, which is then put first on the search path.
ir_ensure_tooling <- function(cache_dir = ir_cache_dir(),
                              repos = ir_tooling_repos()) {
  lib <- ir_tooling_lib(cache_dir)
  dir.create(lib, recursive = TRUE, showWarnings = FALSE)
  .libPaths(c(lib, .libPaths()))

  missing <- ir_missing_tooling()
  if (!length(missing)) return(invisible())

  utils::install.packages(missing, lib = lib, repos = repos)

  still_missing <- ir_missing_tooling()
  if (length(still_missing))
    stop("could not install resolver tooling into ", lib, ": ",
         paste(still_missing, collapse = ", "), call. = FALSE)
  invisible()
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

# Legacy fallback key identifying a resolution request when Rust does not pass
# IR_RESOLUTION_MARKER. Normal CLI runs compute the marker path in Rust so warm
# caches can return before this R resolver is launched. Latest resolution keeps
# a stable key and stores the creation time in the marker value.
ir_input_key <- function(deps,
                         rversion      = getRversion(),
                         platform      = R.version$platform,
                         exclude_newer = NULL,
                         quarto        = FALSE) {
  source_key <- if (is.null(exclude_newer))
    "latest"
  else
    sprintf("exclude-newer: %s", exclude_newer)

  # `quarto` folds in only when TRUE: a Quarto render may inject rmarkdown, so
  # its resolved set differs from a plain run of the same deps. Omitting the
  # marker for non-Quarto runs keeps their existing keys (and cache) stable.
  secretbase::sha256(paste(c(sort(deps),
                             source_key,
                             if (quarto) "quarto" else NULL,
                             as.character(rversion),
                             platform),
                           collapse = "\n"))
}

ir_current_utc_seconds <- function()
  as.numeric(Sys.time())

ir_latest_resolution_max_age_seconds <- function() {
  value <- Sys.getenv("IR_LATEST_RESOLUTION_MAX_AGE_SECONDS", unset = NA_character_)
  if (is.na(value) || !nzchar(value)) return(24 * 60 * 60)

  if (!grepl("^[0-9]+$", value))
    stop("IR_LATEST_RESOLUTION_MAX_AGE_SECONDS must be an integer",
         call. = FALSE)
  as.numeric(value)
}

ir_marker_source <- function(exclude_newer,
                             created_at = ir_current_utc_seconds()) {
  if (is.null(exclude_newer))
    sprintf("latest: %.0f", floor(created_at))
  else
    sprintf("exclude-newer: %s", exclude_newer)
}

ir_marker_source_current <- function(source, exclude_newer) {
  if (!is.null(exclude_newer))
    return(identical(source, ir_marker_source(exclude_newer)))

  if (!startsWith(source, "latest: ")) return(FALSE)
  created_at <- suppressWarnings(as.numeric(sub("^latest: ", "", source)))
  if (is.na(created_at)) return(FALSE)

  now <- ir_current_utc_seconds()
  if (created_at > now) return(FALSE)
  now - created_at <= ir_latest_resolution_max_age_seconds()
}

ir_strip_package_prefix <- function(ref) {
  sub("^([[:alpha:]][[:alnum:].]*[[:alnum:]])=", "", ref)
}

ir_package_prefix <- function(ref) {
  match <- regexpr("^([[:alpha:]][[:alnum:].]*[[:alnum:]])=", ref)
  if (match[[1L]] == 1L) regmatches(ref, match) else ""
}

ir_is_bare_local_ref <- function(ref) {
  local_prefixes <- c("/", "~/", "./", ".\\", "../", "..\\")
  any(startsWith(ref, local_prefixes)) |
    ref %in% c("~", ".") |
    grepl("^[A-Za-z]:[\\\\/]", ref)
}

ir_is_local_input_ref <- function(ref) {
  stopifnot(length(ref) == 1L)

  ref <- trimws(ir_strip_package_prefix(ref))
  startsWith(ref, "local::") || ir_is_bare_local_ref(ref)
}

ir_has_local_input_ref <- function(refs) {
  any(vapply(refs, ir_is_local_input_ref, logical(1)))
}

ir_is_source_input_ref <- function(ref) {
  stopifnot(length(ref) == 1L)

  ref <- trimws(ir_strip_package_prefix(ref))
  source_prefixes <- c("github::", "gitlab::", "bitbucket::", "git::",
                       "url::", "http://", "https://", "ssh://", "git@")
  ir_is_local_input_ref(ref) ||
    any(startsWith(tolower(ref), source_prefixes)) ||
    grepl("/", ref, fixed = TRUE)
}

ir_has_source_input_ref <- function(refs) {
  any(vapply(refs, ir_is_source_input_ref, logical(1)))
}

ir_normalize_input_ref <- function(ref) {
  stopifnot(length(ref) == 1L)

  ref <- trimws(ref)
  package_prefix <- ir_package_prefix(ref)
  source <- ir_strip_package_prefix(ref)
  if (startsWith(source, "local::") || !ir_is_bare_local_ref(source))
    return(ref)

  paste0(package_prefix, "local::", source)
}

ir_normalize_input_refs <- function(refs) {
  vapply(refs, ir_normalize_input_ref, character(1), USE.NAMES = FALSE)
}

ir_is_source_ref <- function(res) {
  stopifnot("type" %in% names(res))

  source_types <- c("github", "gitlab", "bitbucket", "git", "local", "url")
  tolower(res$type) %in% source_types
}

ir_renv_source_refs <- function(ref, type) {
  stopifnot(length(ref) == length(type))

  package_prefix <- "(([[:alpha:]][[:alnum:].]*[[:alnum:]])=)?"
  is_github <- tolower(type) == "github"
  ref[is_github] <- sub(paste0("^", package_prefix,
                               "((github::)?[^/]+/[^/@#]+)/(.*)$"),
                        "\\1\\3:\\5",
                        ref[is_github])
  ref
}

ir_local_ref_paths <- function(ref) {
  sub("^local::", "", ir_strip_package_prefix(ref))
}

ir_local_ref_fingerprint <- function(path) {
  stopifnot(file.exists(path))

  root <- normalizePath(path, winslash = "/", mustWork = TRUE)
  if (!dir.exists(root)) {
    info <- file.info(root)
    return(paste(root, info$size, unname(tools::md5sum(root)), sep = ":"))
  }

  files <- sort(list.files(root, all.files = TRUE, recursive = TRUE,
                           full.names = TRUE, no.. = TRUE))
  files <- normalizePath(files, winslash = "/", mustWork = TRUE)
  info <- file.info(files)
  hash <- unname(tools::md5sum(files))
  rel <- substring(files, nchar(root) + 2L)
  paste(c(root, paste(rel, info$size, hash, sep = ":")),
        collapse = "\n")
}

ir_local_ref_fingerprints <- function(res) {
  stopifnot(c("type", "ref") %in% names(res))

  local <- tolower(res$type) == "local"
  if (!any(local)) return(character())
  vapply(ir_local_ref_paths(res$ref[local]), ir_local_ref_fingerprint,
         character(1))
}

ir_install_refs <- function(res) {
  stopifnot(c("package", "version", "ref") %in% names(res))

  refs <- sprintf("%s@%s", res$package, res$version)
  is_source_ref <- ir_is_source_ref(res)
  refs[is_source_ref] <- ir_renv_source_refs(res$ref[is_source_ref],
                                             res$type[is_source_ref])
  sort(unique(refs))
}

## --- pipeline ---------------------------------------------------------------

ir_resolve_main <- function() {

  ## 0. Ensure the resolver's own tooling (pak/renv/secretbase) is available
  ## before any secretbase/pak/renv use below.
  ir_ensure_tooling()

  deps        <- readLines(file("stdin"), warn = FALSE)
  result_file <- ir_env_optional("IR_RESOLVE_RESULT_FILE")
  package_result_file <- ir_env_optional("IR_RESOLVE_PACKAGE_RESULT_FILE")
  stopifnot(!is.null(result_file))
  cache_dir   <- ir_cache_dir()

  ## 1. Consume inputs parsed by Rust from script frontmatter
  exclude_newer <- ir_exclude_newer(ir_env_optional("IR_EXCLUDE_NEWER"))
  repos <- ir_repos(exclude_newer)
  options(repos = repos)

  # A Quarto render needs rmarkdown for the knitr engine; Rust sets
  # IR_QUARTO_RENDER so the resolver can inject it when the resolved set does not
  # already provide it. (Distinct from IR_QUARTO, the quarto executable path.)
  quarto <- !is.null(ir_env_optional("IR_QUARTO_RENDER"))

  ## 1b. Resolution cache: Rust checks this marker before launching this
  ## resolver. Keep the in-resolver check as the fallback for direct driver runs
  ## and races where another process warms the marker first. The marker is
  ## written only after a successful materialise (below), so its presence implies
  ## a complete library.
  primary_ref <- if (length(deps)) deps[[1L]] else NULL
  cache_resolution <- !ir_has_source_input_ref(deps)
  marker <- ir_env_optional("IR_RESOLUTION_MARKER")
  if (is.null(marker) && cache_resolution) {
    marker <- file.path(cache_dir, "resolutions",
                        ir_input_key(deps, exclude_newer = exclude_newer,
                                     quarto = quarto))
  }
  package_marker <- ir_env_optional("IR_PRIMARY_PACKAGE_MARKER")
  if (is.null(package_marker) && !is.null(marker) && !is.null(primary_ref)) {
    package_marker <- file.path(cache_dir, "resolutions",
                                paste0(basename(marker), "-primary-",
                                       secretbase::sha256(primary_ref)))
  }
  if (!is.null(marker) && file.exists(marker)) {
    cached <- readLines(marker, n = 2L, warn = FALSE)
    if (length(cached) >= 2L &&
        ir_marker_source_current(cached[[1L]], exclude_newer) &&
        nzchar(cached[[2L]]) &&
        dir.exists(cached[[2L]])) {
      if (!is.null(package_result_file) &&
          (is.null(package_marker) || !file.exists(package_marker))) {
        # The library is reusable, but this caller needs primary-package
        # metadata that older cache entries did not record.
      } else {
        writeLines(cached[[2L]], result_file)
        if (!is.null(package_result_file)) {
          package <- readLines(package_marker, n = 1L, warn = FALSE)
          writeLines(package, package_result_file)
        }
        return(invisible())
      }
    }
  }

  ## 2. Resolve with pak
  # A script may legitimately declare no dependencies; a non-Quarto run then gets
  # an empty resolved library. If the user requested `--isolated`, undeclared
  # library() calls fail loudly instead of borrowing from the user library. A
  # Quarto render still resolves rmarkdown (injected below).
  primary_package <- NULL
  refs_in <- ir_normalize_input_refs(deps)
  res <- if (length(refs_in)) ir_resolve_refs(refs_in) else NULL

  if (!is.null(package_result_file)) {
    if (is.null(res))
      stop("cannot resolve a primary package without dependencies",
           call. = FALSE)
    primary <- unique(res$package[res$direct & res$ref == refs_in[[1L]]])
    if (length(primary) != 1L)
      stop("package ref must resolve to exactly one R package: ",
           deps[[1L]], call. = FALSE)
    primary_package <- primary[[1L]]
  }

  ## 2b. Quarto's knitr engine needs rmarkdown. Inject it (latest) only when the
  ## resolved set does not already provide it -- whether the user declared it
  ## directly or it arrived as a transitive dependency of a declared package.
  if (quarto) {
    have_rmarkdown <- !is.null(res) && "rmarkdown" %in% res$package
    if (!have_rmarkdown) {
      refs_in <- c(refs_in, "rmarkdown")
      res <- ir_resolve_refs(refs_in)
    }
  }

  if (is.null(res)) {
    pkgs     <- character()
    install_refs <- character()
    local_ref_fingerprints <- character()
    has_source_ref <- FALSE
  } else {
    # Drop base / recommended packages: those are supplied by R itself.
    keep <- is.na(res$priority) | !(res$priority %in% c("base", "recommended"))
    res <- res[keep, , drop = FALSE]
    pkgs     <- res$package
    install_refs <- ir_install_refs(res)
    local_ref_fingerprints <- ir_local_ref_fingerprints(res)
    has_source_ref <- any(ir_is_source_ref(res))
  }

  ## 3. Hash the resolved set -> content-addressed library path
  # Bind the hash to the R version and platform: the symlinks point into the
  # renv cache, whose layout is itself keyed by R version and platform.
  key <- paste(c(install_refs,
                 local_ref_fingerprints,
                 as.character(getRversion()),
                 R.version$platform),
               collapse = "\n")
  library_path <- file.path(cache_dir, "libraries", secretbase::sha256(key))

  ## 4. Materialise the symlinked library via renv::use()
  # Skip when the library already holds every resolved package: repeat runs of
  # an unchanged script then cost nothing beyond resolution.
  dir.create(library_path, recursive = TRUE, showWarnings = FALSE)
  have <- list.files(library_path)
  if (length(pkgs) && (has_source_ref || !all(pkgs %in% have))) {
    # renv::use() installs into the renv cache and links the packages into
    # `library` as symlinks. Because `library` lives in our cache (not the R
    # temp dir), renv leaves it in place when the session ends.
    do.call(renv::use, c(
      as.list(install_refs),
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
  if (!is.null(marker)) {
    dir.create(dirname(marker), recursive = TRUE, showWarnings = FALSE)
    writeLines(c(ir_marker_source(exclude_newer), library_path), marker)
  }
  if (!is.null(primary_package) && !is.null(package_marker)) {
    writeLines(primary_package, package_marker)
  }
  writeLines(library_path, result_file)
  if (!is.null(package_result_file)) {
    writeLines(primary_package, package_result_file)
  }
  invisible()
}

if (sys.nframe() == 0L) ir_resolve_main()
