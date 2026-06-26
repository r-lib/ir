# ir resolve driver
#
# Run by the `ir` Rust binary in a private, throw-away R session.
#
#   IR_RESOLVE_RESULT_FILE=<result_file> Rscript resolve.R
#
# Responsibilities (steps 1-4 of the `ir` pipeline):
#   1. Consume package refs from stdin, one ref per line.
#   2. Resolve dependencies with pak.
#   3. Hash the install refs to derive a content-addressed library path under
#      <cache_dir>.
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
# (tests/run.rs, tests/render.rs, and tests/tool.rs), which drive this resolver
# through real renders and package executions.

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

## --- repositories -----------------------------------------------------------

ir_named_value <- function(values, name) {
  if (is.null(values) || is.null(names(values)) || !(name %in% names(values)))
    return(NULL)
  unname(values[[name]])
}

ir_repo_resolve <- function(spec) {
  pak::repo_resolve(spec)
}

ir_linux_host <- function()
  identical(unname(Sys.info()[["sysname"]]), "Linux")

ir_public_ppm_latest_url <- function(repo)
  identical(sub("/+$", "", repo), "https://packagemanager.posit.co/cran/latest")

ir_ppm_snapshot_url <- function(exclude_newer) {
  if (!ir_linux_host())
    return(sprintf("https://packagemanager.posit.co/cran/%s", exclude_newer))

  unname(ir_repo_resolve(sprintf("PPM@%s", exclude_newer))[[1L]])
}

ir_ppm_latest_repos <- function() {
  c(CRAN = ir_ppm_snapshot_url("latest"))
}

ir_repos <- function(exclude_newer = NULL, repos = getOption("repos")) {
  if (!is.null(exclude_newer))
    return(c(CRAN = ir_ppm_snapshot_url(exclude_newer)))

  if (is.null(repos) || !length(repos))
    return(ir_ppm_latest_repos())

  if (is.null(names(repos))) {
    if (length(repos) == 1L) names(repos) <- "CRAN"
    else return(repos)
  }

  cran <- ir_named_value(repos, "CRAN")
  if (is.null(cran) || is.na(cran) || !nzchar(cran) ||
      identical(cran, "@CRAN@") || ir_public_ppm_latest_url(cran))
    repos[["CRAN"]] <- ir_ppm_snapshot_url("latest")

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
                         quarto        = FALSE,
                         library_root  = NULL) {
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
                             if (!is.null(library_root))
                               paste0("library-root: ", library_root) else NULL,
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

ir_is_standard_input_ref <- function(ref) {
  stopifnot(length(ref) == 1L)

  ref <- trimws(ref)
  grepl(paste0("^",
               "[[:alpha:]]([[:alnum:].]*[[:alnum:]])?",
               "(@(>=)?([0-9]+[-.][0-9]+([-.][0-9]+)*|current|last))?",
               "$"),
        ref)
}

ir_has_nonstandard_input_ref <- function(refs) {
  any(!vapply(refs, ir_is_standard_input_ref, logical(1)))
}

ir_is_standard_resolved_ref <- function(res) {
  stopifnot("type" %in% names(res))

  tolower(res$type) == "standard"
}

ir_install_spec <- function(res, i) {
  if (ir_is_standard_resolved_ref(res[i, , drop = FALSE]))
    return(sprintf("%s@%s", res$package[[i]], res$version[[i]]))

  res$ref[[i]]
}

ir_install_specs <- function(res) {
  sort(unique(vapply(seq_len(nrow(res)), function(i) ir_install_spec(res, i),
                     character(1))))
}

## --- pipeline ---------------------------------------------------------------

ir_resolve_main <- function() {
  cache_dir <- ir_cache_dir()
  library_root <- ir_env_optional("IR_LIBRARY_ROOT")
  ir_configure_child_tempdir()
  on.exit(ir_close_pak_remote(), add = TRUE)

  deps        <- readLines(file("stdin"), warn = FALSE)
  result_file <- ir_env_optional("IR_RESOLVE_RESULT_FILE")
  package_result_file <- ir_env_optional("IR_RESOLVE_PACKAGE_RESULT_FILE")
  python_result_file <- ir_env_optional("IR_PYTHON_RESULT_FILE")
  stopifnot(!is.null(result_file) || !is.null(python_result_file))

  ## 1. Consume inputs parsed by Rust from script frontmatter
  exclude_newer <- ir_exclude_newer(ir_env_optional("IR_EXCLUDE_NEWER"))

  if (!is.null(result_file)) {
    ## 0. Bootstrap pak before repository normalization. On Linux PPM URLs are
    ## resolved through pak::repo_resolve(), so pak must be available first.
    ir_ensure_tooling(packages = "pak", cache_dir = cache_dir)
    repos <- ir_repos(exclude_newer)
    options(repos = repos)

    ## Ensure the rest of the resolver's own tooling is available before any
    ## secretbase/pak/renv use below.
    ir_ensure_tooling(cache_dir = cache_dir)
  }

  if (!is.null(python_result_file)) {
    python_packages_file <- ir_env_optional("IR_PYTHON_PACKAGES_FILE")
    stopifnot(!is.null(python_packages_file))
    python_packages <- readLines(python_packages_file, warn = FALSE)
    python_version <- ir_env_optional("IR_PYTHON_VERSION")
    python_exclude_newer <- ir_env_optional("IR_PYTHON_EXCLUDE_NEWER")
    python <- ir_resolve_python_env(
      packages = python_packages,
      python_version = python_version,
      exclude_newer = python_exclude_newer
    )
    writeLines(python, python_result_file)
  }

  if (is.null(result_file)) return(invisible())

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
  cache_resolution <- !ir_has_nonstandard_input_ref(deps)
  marker <- ir_env_optional("IR_RESOLUTION_MARKER")
  if (is.null(marker) && cache_resolution) {
    marker <- file.path(cache_dir, "resolutions",
                        ir_input_key(deps, exclude_newer = exclude_newer,
                                     quarto = quarto,
                                     library_root = library_root))
  }
  package_marker <- ir_env_optional("IR_PRIMARY_PACKAGE_MARKER")
  if (!is.null(package_result_file) &&
      is.null(package_marker) &&
      !is.null(marker) &&
      !is.null(primary_ref)) {
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
  refs_in <- deps
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
    install_specs <- character()
    has_source_ref <- FALSE
  } else {
    # Drop base / recommended packages: those are supplied by R itself.
    keep <- is.na(res$priority) | !(res$priority %in% c("base", "recommended"))
    res <- res[keep, , drop = FALSE]
    pkgs     <- res$package
    install_specs <- ir_install_specs(res)
    has_source_ref <- any(!ir_is_standard_resolved_ref(res))
  }

  ## 3. Hash install specs -> content-addressed library path
  # Bind the hash to the R version and platform: the symlinks point into the
  # renv cache, whose layout is itself keyed by R version and platform.
  key <- paste(c(install_specs,
                 as.character(getRversion()),
                 R.version$platform),
               collapse = "\n")
  if (is.null(library_root)) library_root <- cache_dir
  library_path <- file.path(library_root, "libraries", secretbase::sha256(key))

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
      as.list(install_specs),
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
