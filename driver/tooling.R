## --- cache location ---------------------------------------------------------

# The cache root: the standard per-package user cache directory, overridable
# with IR_CACHE_DIR. Holds `libraries/` (materialised libraries),
# `resolutions/` (the resolution request cache), and resolver tooling.
ir_cache_dir <- function() {
  env <- Sys.getenv("IR_CACHE_DIR")
  if (nzchar(env)) env else tools::R_user_dir("ir", "cache")
}

ir_configure_child_tempdir <- function(tmp = tempdir()) {
  stopifnot(length(tmp) == 1L, nzchar(tmp), dir.exists(tmp))
  tmp <- normalizePath(tmp, winslash = "/", mustWork = TRUE)
  Sys.setenv(TMPDIR = tmp, TMP = tmp, TEMP = tmp)
  invisible(tmp)
}

ir_close_pak_remote <- function(grace = 5000) {
  if (!isNamespaceLoaded("pak")) return(invisible())
  ns <- asNamespace("pak")
  if (!exists("pkg_data", ns, inherits = FALSE)) return(invisible())

  pkg_data <- get("pkg_data", ns, inherits = FALSE)
  remote <- pkg_data[["remote"]]
  if (is.null(remote)) return(invisible())
  close <- remote[["close"]]
  if (!is.function(close)) return(invisible())

  tryCatch(
    close(grace),
    error = function(e) {
      warning("could not close pak subprocess: ", conditionMessage(e),
              call. = FALSE)
    }
  )
  pkg_data[["remote"]] <- NULL
  invisible()
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

# Path to the tooling library, keyed by R version and platform so compiled
# packages match the running R, mirroring renv's cache layout.
ir_tooling_lib <- function(cache_dir = ir_cache_dir())
  file.path(cache_dir, "tooling",
            paste0(getRversion(), "-", R.version$platform))

ir_tooling_version <- function(package, lib = ir_tooling_lib()) {
  tryCatch(utils::packageVersion(package, lib.loc = lib),
           error = function(e) NULL)
}

ir_tooling_version_ok <- function(package, lib = ir_tooling_lib(),
                                  min_version = NULL) {
  version <- ir_tooling_version(package, lib)
  if (is.null(version)) return(FALSE)
  is.null(min_version) || version >= min_version
}

ir_tooling_min_version <- function(package, min_versions = character()) {
  if (!(package %in% names(min_versions))) return(NULL)
  value <- min_versions[[package]]
  if (is.null(value)) NULL else value
}

ir_reset_tooling_namespace <- function(package) {
  if (!isNamespaceLoaded(package)) return(invisible())

  attached <- paste0("package:", package)
  tryCatch({
    if (attached %in% search())
      detach(attached, character.only = TRUE, unload = TRUE)
    else
      unloadNamespace(package)
  }, error = function(e) {
    stop("package `", package, "` was loaded before resolver tooling ",
         "could select its private copy: ", conditionMessage(e),
         call. = FALSE)
  })

  if (isNamespaceLoaded(package)) {
    stop("package `", package, "` was loaded before resolver tooling ",
         "could select its private copy", call. = FALSE)
  }

  invisible()
}

# Tooling packages not already usable by the resolver. Prefer the private
# tooling library, but accept ambient packages unless they come from R_LIBS_USER
# and were built under a different R minor version.
ir_missing_tooling <- function(packages = ir_tooling_packages(),
                               lib = ir_tooling_lib(),
                               min_versions = character()) {
  r_libs_user <- Sys.getenv("R_LIBS_USER")
  user_libs <- character()
  if (nzchar(r_libs_user)) {
    user_libs <- strsplit(r_libs_user, .Platform$path.sep, fixed = TRUE)[[1L]]
    user_libs <- user_libs[nzchar(user_libs)]
    user_libs <- normalizePath(user_libs, winslash = "/", mustWork = FALSE)
  }

  current_r <- strsplit(as.character(getRversion()), ".", fixed = TRUE)[[1L]][1:2]
  missing <- character()
  bad_user_libs <- character()
  package_r_minor <- function(path) {
    metadata <- file.path(path, "Meta", "package.rds")
    info <- if (file.exists(metadata)) {
      tryCatch(readRDS(metadata), error = function(e) NULL)
    } else {
      NULL
    }

    built_r <- if (is.null(info)) character() else as.character(info$Built$R)
    if (length(built_r))
      built_r <- strsplit(built_r[[1L]], ".", fixed = TRUE)[[1L]][1:2]
    built_r
  }

  if (length(user_libs)) {
    user_secretbase <- find.package("secretbase", lib.loc = user_libs,
                                    quiet = TRUE)
    if (length(user_secretbase)) {
      pkg_lib <- normalizePath(dirname(user_secretbase[[1L]]), winslash = "/",
                               mustWork = FALSE)
      if (!identical(package_r_minor(user_secretbase[[1L]]), current_r))
        bad_user_libs <- c(bad_user_libs, pkg_lib)
    }
  }

  for (pkg in packages) {
    min_version <- ir_tooling_min_version(pkg, min_versions)
    if (ir_tooling_version_ok(pkg, lib, min_version)) next

    path <- find.package(pkg, quiet = TRUE)
    if (!length(path)) {
      missing <- c(missing, pkg)
      next
    }

    pkg_lib <- normalizePath(dirname(path[[1L]]), winslash = "/",
                             mustWork = FALSE)
    if (pkg_lib %in% user_libs) {
      if (pkg_lib %in% bad_user_libs) {
        missing <- c(missing, pkg)
        next
      }

      if (!identical(package_r_minor(path[[1L]]), current_r)) {
        bad_user_libs <- c(bad_user_libs, pkg_lib)
        missing <- c(missing, pkg)
        next
      }
    }

    version <- tryCatch(utils::packageVersion(pkg), error = function(e) NULL)
    if (!is.null(min_version) &&
        (is.null(version) || version < min_version)) {
      missing <- c(missing, pkg)
      next
    }
  }

  if (length(bad_user_libs)) {
    bad_user_libs <- unique(bad_user_libs)
    current_libs <- .libPaths()
    current_libs_normalized <- normalizePath(current_libs, winslash = "/",
                                             mustWork = FALSE)
    .libPaths(current_libs[!current_libs_normalized %in% bad_user_libs])

    user_libs <- user_libs[!user_libs %in% bad_user_libs]
    if (length(user_libs))
      Sys.setenv(R_LIBS_USER = paste(user_libs, collapse = .Platform$path.sep))
    else
      Sys.setenv(R_LIBS_USER = "NULL")
  }

  missing
}

ir_install_tooling_with_pak <- function(missing, refs, lib) {
  missing <- setdiff(missing, "pak")
  if (!length(missing)) return(invisible())

  if (!requireNamespace("pak", quietly = TRUE))
    stop("package `pak` is required to install resolver tooling",
         call. = FALSE)

  install_refs <- vapply(missing, function(pkg) {
    if (pkg %in% names(refs)) refs[[pkg]] else pkg
  }, character(1), USE.NAMES = FALSE)

  pak::pkg_install(install_refs, lib = lib, upgrade = TRUE,
                   ask = FALSE, dependencies = NA)
  invisible()
}

ir_bootstrap_pak <- function(missing, lib, repos) {
  if ("pak" %in% missing)
    utils::install.packages("pak", lib = lib, repos = repos)
  invisible()
}

# Ensure resolver tooling is available. `pak` itself is bootstrapped with
# install.packages(); every other tooling package is installed with pak.
ir_ensure_tooling <- function(packages = ir_tooling_packages(),
                              refs = character(),
                              min_versions = character(),
                              cache_dir = ir_cache_dir(),
                              repos = ir_tooling_repos()) {
  lib <- ir_tooling_lib(cache_dir)
  dir.create(lib, recursive = TRUE, showWarnings = FALSE)
  .libPaths(c(lib, .libPaths()))
  old_repos <- options(repos = repos)
  on.exit(options(old_repos), add = TRUE)

  missing <- ir_missing_tooling(packages = packages, lib = lib,
                                min_versions = min_versions)
  if (!length(missing)) return(invisible())

  ir_bootstrap_pak(missing, lib, repos)

  missing <- ir_missing_tooling(packages = packages, lib = lib,
                                min_versions = min_versions)
  ir_bootstrap_pak(missing, lib, repos)

  missing <- ir_missing_tooling(packages = packages, lib = lib,
                                min_versions = min_versions)
  ir_install_tooling_with_pak(missing, refs, lib)

  still_missing <- ir_missing_tooling(packages = packages, lib = lib,
                                      min_versions = min_versions)
  if (length(still_missing))
    stop("could not install resolver tooling into ", lib, ": ",
         paste(still_missing, collapse = ", "), call. = FALSE)
  invisible()
}
