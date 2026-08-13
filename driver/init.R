# ir script initialization driver
#
# Run by the `ir` Rust binary in a private R session. This driver scans one
# script for direct package uses. It deliberately does not inspect project
# lockfiles or infer package source provenance.

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

ir_init_r_requirement <- function() {
  status <- R.version$status
  if (identical(status, "Under development (unstable)"))
    return("devel")
  if (status %in% c("alpha", "beta", "RC"))
    return("next")
  if (status %in% c("", "Patched"))
    return(paste(">=", ir_init_minor_version(getRversion())))
  stop("unsupported R version status `", status, "`", call. = FALSE)
}

ir_init_main <- function() {
  script <- ir_init_required_env("IR_INIT_SCRIPT")
  result_file <- ir_init_required_env("IR_INIT_RESULT_FILE")

  ir_configure_child_tempdir()
  on.exit(ir_close_pak_remote(), add = TRUE)
  ir_ensure_tooling(min_versions = c(renv = "1.2.0"))

  old_options <- options(renv.dependencies.implied = list())
  on.exit(options(old_options), add = TRUE)
  dependencies <- renv::dependencies(
    path = script,
    root = dirname(script),
    progress = FALSE,
    errors = "fatal",
    dev = FALSE
  )
  packages <- sort(unique(dependencies[["Package"]]))
  r_supplied <- rownames(utils::installed.packages(
    lib.loc = .Library,
    priority = c("base", "recommended"),
    noCache = TRUE
  ))
  packages <- setdiff(packages, c("R", r_supplied))

  output <- c(paste0("r-version=", ir_init_r_requirement()), packages)
  if (any(grepl("[\r\n]", output)))
    stop("generated package metadata contains a newline", call. = FALSE)
  writeLines(output, result_file, useBytes = TRUE)
  invisible()
}

if (sys.nframe() == 0L) ir_init_main()
