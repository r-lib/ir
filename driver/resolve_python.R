ir_python_env_main <- function() {
  result_file <- Sys.getenv("IR_PYTHON_RESULT_FILE", unset = NA_character_)
  stopifnot(!is.na(result_file), nzchar(result_file))

  packages <- readLines("stdin", warn = FALSE)
  python_version <- Sys.getenv("IR_UV_PYTHON_VERSION", unset = NA_character_)
  exclude_newer <- Sys.getenv("IR_UV_EXCLUDE_NEWER", unset = NA_character_)
  if (is.na(python_version) || !nzchar(python_version)) python_version <- NULL
  if (is.na(exclude_newer) || !nzchar(exclude_newer)) exclude_newer <- NULL

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
  writeLines(python, result_file)
  invisible()
}

if (sys.nframe() == 0L) ir_python_env_main()
