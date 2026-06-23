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

# Path to the tooling library, keyed by R version, platform, and the tooling
# contract. A changed minimum version or package ref gets a fresh library, so
# loaded packages are never upgraded in place.
ir_tooling_key <- function(packages = ir_tooling_packages(),
                           refs = character(),
                           min_versions = character()) {
  named_lines <- function(prefix, values) {
    if (!length(values)) return(character())
    ord <- order(names(values), unname(values))
    paste0(prefix, ":", names(values)[ord], "=", unname(values)[ord])
  }

  lines <- c(
    "ir-tooling-v2",
    paste0("package:", sort(unique(packages))),
    named_lines("ref", refs),
    named_lines("min", min_versions)
  )

  file <- tempfile("ir-tooling-key-")
  on.exit(unlink(file), add = TRUE)
  writeBin(charToRaw(paste(lines, collapse = "\n")), file)
  unname(tools::md5sum(file))
}

ir_tooling_lib <- function(cache_dir = ir_cache_dir(),
                           packages = ir_tooling_packages(),
                           refs = character(),
                           min_versions = character())
  file.path(cache_dir, "tooling",
            paste0(getRversion(), "-", R.version$platform),
            ir_tooling_key(packages, refs, min_versions))

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

ir_tooling_namespace_path <- function(package) {
  if (!isNamespaceLoaded(package)) return(NULL)

  path <- getNamespaceInfo(package, "path")
  if (is.null(path) || !nzchar(path)) return(NULL)
  normalizePath(path, winslash = "/", mustWork = FALSE)
}

ir_reset_tooling_namespace_if_outside_lib <- function(package, lib) {
  actual <- ir_tooling_namespace_path(package)
  if (is.null(actual)) return(invisible())

  expected <- normalizePath(file.path(lib, package),
                            winslash = "/", mustWork = FALSE)
  if (identical(actual, expected)) return(invisible())

  if (identical(package, "pak")) ir_close_pak_remote()
  ir_reset_tooling_namespace(package)
}

ir_reset_tooling_namespaces_outside_lib <- function(packages, lib) {
  for (pkg in packages)
    ir_reset_tooling_namespace_if_outside_lib(pkg, lib)
  invisible()
}

ir_pak_private_cli_ok <- function() {
  if (!isNamespaceLoaded("pak")) return(FALSE)

  tryCatch({
    load_private_cli <- get("load_private_cli", asNamespace("pak"),
                            inherits = FALSE)
    load_private_cli()
    TRUE
  }, error = function(e) FALSE)
}

# Tooling packages not already usable by the resolver. The check intentionally
# tries to load the package: a package that is present but cannot load is not
# useful to the resolver.
ir_tooling_loaded_too_old <- function(package, min_version) {
  if (is.null(min_version) || !isNamespaceLoaded(package)) return(FALSE)

  version <- tryCatch(getNamespaceVersion(package), error = function(e) NULL)
  is.null(version) || version < package_version(min_version)
}

ir_tooling_safe_mode <- function()
  identical(Sys.getenv("IR_TOOLING_SAFE_MODE"), "1")

ir_tooling_lib_paths <- function(lib) {
  if (ir_tooling_safe_mode()) c(lib, .Library) else c(lib, .libPaths())
}

ir_configure_safe_tooling_env <- function(lib) {
  if (!ir_tooling_safe_mode()) return(invisible())

  no_user_file <- file.path(tempdir(), "ir-safe-mode-no-user-startup")
  Sys.setenv(
    R_LIBS = lib,
    R_LIBS_USER = "NULL",
    R_LIBS_SITE = "NULL",
    R_PROFILE_USER = no_user_file,
    R_ENVIRON_USER = no_user_file
  )
  invisible()
}

ir_tooling_available <- function(package, min_version = NULL) {
  if (ir_tooling_loaded_too_old(package, min_version)) return(FALSE)

  args <- list(package = package, quietly = TRUE)
  if (!is.null(min_version)) {
    args$versionCheck <- list(op = ">=",
                              version = package_version(min_version))
  }
  ok <- isTRUE(do.call(requireNamespace, args))
  if (ok && identical(package, "pak"))
    ok <- ir_pak_private_cli_ok()
  if (!ok && identical(package, "pak") && isNamespaceLoaded(package))
    try(unloadNamespace(package), silent = TRUE)

  ok
}

ir_unavailable_tooling <- function(packages = ir_tooling_packages(),
                                   min_versions = character()) {
  unavailable <- character()
  loaded_too_old <- character()

  for (pkg in packages) {
    min_version <- ir_tooling_min_version(pkg, min_versions)
    if (ir_tooling_available(pkg, min_version)) next

    unavailable <- c(unavailable, pkg)
    if (ir_tooling_loaded_too_old(pkg, min_version))
      loaded_too_old <- c(loaded_too_old, pkg)
  }

  attr(unavailable, "loaded_too_old") <- loaded_too_old
  unavailable
}

ir_signal_tooling_restart <- function(packages) {
  restart_file <- Sys.getenv("IR_TOOLING_RESTART_FILE", unset = NA_character_)
  if (is.na(restart_file) || !nzchar(restart_file)) {
    stop("resolver tooling was updated; rerun ir", call. = FALSE)
  }

  writeLines(packages, restart_file)
  quit(save = "no", status = 86L, runLast = FALSE)
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
  if ("pak" %in% missing) {
    unlink(file.path(lib, "pak"), recursive = TRUE, force = TRUE)
    utils::install.packages("pak", lib = lib, repos = repos)
  }
  invisible()
}

# Ensure resolver tooling is available. `pak` itself is bootstrapped with
# install.packages(); every other tooling package is installed with pak.
ir_ensure_tooling <- function(packages = ir_tooling_packages(),
                              refs = character(),
                              min_versions = character(),
                              cache_dir = ir_cache_dir(),
                              repos = ir_tooling_repos()) {
  lib <- ir_tooling_lib(cache_dir, packages = packages, refs = refs,
                        min_versions = min_versions)
  dir.create(lib, recursive = TRUE, showWarnings = FALSE)
  .libPaths(ir_tooling_lib_paths(lib))
  ir_configure_safe_tooling_env(lib)
  if (ir_tooling_safe_mode())
    ir_reset_tooling_namespaces_outside_lib(packages, lib)
  old_repos <- options(repos = repos)
  on.exit(options(old_repos), add = TRUE)

  unavailable <- ir_unavailable_tooling(packages = packages,
                                        min_versions = min_versions)
  if (!length(unavailable)) return(invisible())

  loaded_too_old <- attr(unavailable, "loaded_too_old")
  ir_bootstrap_pak(unavailable, lib, repos)
  if ("pak" %in% loaded_too_old)
    ir_signal_tooling_restart("pak")

  unavailable <- ir_unavailable_tooling(packages = packages,
                                        min_versions = min_versions)
  if (!length(unavailable)) return(invisible())

  loaded_too_old <- attr(unavailable, "loaded_too_old")
  ir_install_tooling_with_pak(unavailable, refs, lib)
  if (length(loaded_too_old))
    ir_signal_tooling_restart(loaded_too_old)

  still_unavailable <- ir_unavailable_tooling(packages = packages,
                                              min_versions = min_versions)
  if (length(still_unavailable))
    stop("could not install resolver tooling into ", lib, ": ",
         paste(still_unavailable, collapse = ", "), call. = FALSE)
  invisible()
}

ir_resolve_python_env <- function(packages,
                                  python_version = NULL,
                                  exclude_newer = NULL) {
  ir_reset_tooling_namespace("reticulate")
  ir_ensure_tooling(
    packages = c("pak", "reticulate"),
    refs = c(reticulate = "reticulate@>=1.41.0"),
    min_versions = c(reticulate = "1.41.0")
  )
  if (!exists("uv_get_or_create_env", asNamespace("reticulate"),
              inherits = FALSE)) {
    stop("package `reticulate` must provide `uv_get_or_create_env()",
         call. = FALSE)
  }

  python <- reticulate:::uv_get_or_create_env(
    packages = packages,
    python_version = python_version,
    exclude_newer = exclude_newer
  )
  stopifnot(length(python) == 1L, nzchar(python))
  python
}
