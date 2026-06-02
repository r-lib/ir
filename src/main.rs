use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("ir: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<u8, String> {
    let mut args = env::args_os();
    let _program = args.next();

    let command = args.next().ok_or_else(usage)?;
    if command != "run" {
        return Err(usage());
    }

    let script = args.next().ok_or_else(usage).map(PathBuf::from)?;
    let script_args: Vec<OsString> = args.collect();

    let cache_dir = cache_dir()?;
    fs::create_dir_all(&cache_dir).map_err(|error| {
        format!(
            "failed to create cache directory {}: {error}",
            cache_dir.display()
        )
    })?;

    let resolver = write_resolver_script(&cache_dir)?;
    let library = resolve_library(&resolver, &script, &cache_dir)?;
    remove_file_if_exists(&resolver);

    run_script(&script, &script_args, &library)
}

fn usage() -> String {
    "usage: ir run <script.R> [args...]".to_string()
}

fn cache_dir() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("IR_CACHE_DIR") {
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("ir"));
    }

    let home = env::var_os("HOME").ok_or_else(|| {
        "IR_CACHE_DIR is unset and HOME is unavailable; cannot choose a cache directory".to_string()
    })?;
    Ok(PathBuf::from(home).join(".cache").join("ir"))
}

fn write_resolver_script(cache_dir: &Path) -> Result<PathBuf, String> {
    let tmp = cache_dir.join("tmp");
    fs::create_dir_all(&tmp).map_err(|error| {
        format!(
            "failed to create temporary directory {}: {error}",
            tmp.display()
        )
    })?;

    let path = tmp.join(format!(
        "ir-resolve-{}-{}.R",
        std::process::id(),
        timestamp_nanos()
    ));
    fs::write(&path, RESOLVER_R).map_err(|error| {
        format!(
            "failed to write resolver script {}: {error}",
            path.display()
        )
    })?;
    Ok(path)
}

fn timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn resolve_library(resolver: &Path, script: &Path, cache_dir: &Path) -> Result<PathBuf, String> {
    let output = Command::new("Rscript")
        .arg("--vanilla")
        .arg(resolver)
        .arg(script)
        .arg(cache_dir)
        .env("PKG_CACHE_DIR", cache_dir.join("pak-downloads"))
        .env("PKG_METADATA_CACHE_DIR", cache_dir.join("pak-metadata"))
        .env("PKG_PACKAGE_CACHE_DIR", cache_dir.join("pak-packages"))
        .env("PKG_USE_BIOCONDUCTOR", "false")
        .output()
        .map_err(|error| format!("failed to run Rscript resolver: {error}"))?;

    if !output.status.success() {
        write_output(&output.stdout, &output.stderr);
        return Err(match output.status.code() {
            Some(code) => format!("resolver R session failed with exit code {code}"),
            None => "resolver R session failed".to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let library = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("IR_LIBRARY_PATH="))
        .last()
        .ok_or_else(|| "resolver did not print IR_LIBRARY_PATH".to_string())?;

    Ok(PathBuf::from(library))
}

fn run_script(script: &Path, script_args: &[OsString], library: &Path) -> Result<u8, String> {
    let status = Command::new("Rscript")
        .arg("--vanilla")
        .arg(script)
        .args(script_args)
        .env("R_LIBS", library)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("failed to run Rscript: {error}"))?;

    Ok(status.code().unwrap_or(1).try_into().unwrap_or(1))
}

fn write_output(stdout: &[u8], stderr: &[u8]) {
    use std::io::Write;

    let _ = io::stdout().write_all(stdout);
    let _ = io::stderr().write_all(stderr);
}

fn remove_file_if_exists(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

const RESOLVER_R: &str = r#"
args <- commandArgs(trailingOnly = TRUE)
stopifnot(length(args) == 2L)

script <- args[[1L]]
cache_dir <- args[[2L]]
stopifnot(file.exists(script))

extract_frontmatter <- function(path) {
  lines <- readLines(path, warn = FALSE)
  if (!length(lines)) {
    return(character())
  }

  start <- if (grepl("^#!", lines[[1L]])) 2L else 1L
  frontmatter <- character()

  for (line in lines[start:length(lines)]) {
    if (!grepl("^\\s*#", line)) {
      break
    }

    frontmatter <- c(frontmatter, sub("^\\s*# ?", "", line))
  }

  frontmatter
}

normalize_frontmatter <- function(lines) {
  output <- character()

  for (line in lines) {
    if (grepl("^R\\s*:\\s*(>=|<=|==|>|<|=)\\s*[0-9]", line)) {
      line <- sub("^R\\s*:\\s*(.*)$", "R: \"\\1\"", line)
    }

    output <- c(output, line)
  }

  paste(output, collapse = "\n")
}

normalize_package_specs <- function(specs) {
  specs <- trimws(specs)
  vapply(specs, normalize_package_spec, character(1L), USE.NAMES = FALSE)
}

normalize_package_spec <- function(spec) {
  match <- regexec(
    "^([A-Za-z][A-Za-z0-9.]*[A-Za-z0-9])\\s*(>=|<=|==|>|<|=)\\s*([0-9][0-9.-]*)$",
    spec,
    perl = TRUE
  )
  parts <- regmatches(spec, match)[[1L]]
  if (!length(parts)) {
    return(spec)
  }

  package <- parts[[2L]]
  operator <- parts[[3L]]
  version <- parts[[4L]]

  switch(
    operator,
    ">=" = paste0(package, "@>=", version),
    "==" = paste0(package, "@", version),
    "=" = paste0(package, "@", version),
    paste0(package, "@", select_cran_version(package, operator, version))
  )
}

select_cran_version <- function(package, operator, required) {
  versions <- cran_package_versions(package)
  versions <- versions[vapply(versions, version_satisfies, logical(1L), operator, required)]

  if (!length(versions)) {
    stop(
      "no CRAN version of ",
      package,
      " satisfies ",
      operator,
      required,
      call. = FALSE
    )
  }

  versions[order(package_version(versions), decreasing = TRUE)][[1L]]
}

version_satisfies <- function(version, operator, required) {
  comparison <- utils::compareVersion(version, required)
  switch(
    operator,
    ">" = comparison > 0L,
    "<" = comparison < 0L,
    "<=" = comparison <= 0L,
    stop("unsupported package version operator: ", operator, call. = FALSE)
  )
}

cran_package_versions <- function(package) {
  versions <- character()

  available <- cran_available_packages()
  current <- available[available[, "Package"] == package, "Version", drop = TRUE]
  if (length(current)) {
    versions <- c(versions, current)
  }

  archive <- cran_archive_packages()
  archived <- archive[[package]]
  if (!is.null(archived)) {
    pattern <- paste0("^", package, "/", package, "_(.+)\\.tar\\.gz$")
    files <- rownames(archived)
    matches <- regexec(pattern, files, perl = TRUE)
    extracted <- regmatches(files, matches)
    archived_versions <- vapply(
      extracted,
      function(match) if (length(match)) match[[2L]] else NA_character_,
      character(1L)
    )
    versions <- c(versions, archived_versions[!is.na(archived_versions)])
  }

  versions <- unique(versions)
  valid <- !is.na(package_version(versions, strict = FALSE))
  versions[valid]
}

cran_available_packages <- function() {
  path <- file.path(cache_dir, "cran", "PACKAGES.rds")
  if (!file.exists(path)) {
    dir.create(dirname(path), recursive = TRUE, showWarnings = FALSE)
    url <- paste0(utils::contrib.url(cran_mirror(), type = "source"), "/PACKAGES.rds")
    utils::download.file(url, path, quiet = TRUE, mode = "wb")
  }

  readRDS(path)
}

cran_archive_packages <- function() {
  path <- file.path(cache_dir, "cran", "archive.rds")
  if (!file.exists(path)) {
    dir.create(dirname(path), recursive = TRUE, showWarnings = FALSE)
    url <- paste0(cran_mirror(), "/src/contrib/Meta/archive.rds")
    utils::download.file(url, path, quiet = TRUE, mode = "wb")
  }

  readRDS(path)
}

cran_mirror <- function() {
  repos <- getOption("repos")
  cran <- repos[["CRAN"]]
  if (is.null(cran) || !nzchar(cran) || identical(cran, "@CRAN@")) {
    cran <- "https://cran.rstudio.com"
  }

  sub("/+$", "", cran)
}

verify_installed_specs <- function(specs, library) {
  for (spec in specs) {
    match <- regexec(
      "^([A-Za-z][A-Za-z0-9.]*[A-Za-z0-9])@([0-9][0-9.-]*)$",
      spec,
      perl = TRUE
    )
    parts <- regmatches(spec, match)[[1L]]
    if (!length(parts)) {
      next
    }

    package <- parts[[2L]]
    version <- parts[[3L]]
    installed <- installed_package_version(package, library)
    if (!identical(installed, version)) {
      found <- if (is.na(installed)) "<missing>" else installed
      stop(
        "expected ",
        package,
        " ",
        version,
        " in ",
        library,
        ", found ",
        found,
        call. = FALSE
      )
    }
  }
}

library_satisfies_specs <- function(specs, library) {
  all(vapply(specs, spec_satisfied, logical(1L), library))
}

spec_satisfied <- function(spec, library) {
  exact <- regmatches(
    spec,
    regexec("^([A-Za-z][A-Za-z0-9.]*[A-Za-z0-9])@([0-9][0-9.-]*)$", spec, perl = TRUE)
  )[[1L]]
  if (length(exact)) {
    installed <- installed_package_version(exact[[2L]], library)
    return(identical(installed, exact[[3L]]))
  }

  lower <- regmatches(
    spec,
    regexec("^([A-Za-z][A-Za-z0-9.]*[A-Za-z0-9])@>=(.+)$", spec, perl = TRUE)
  )[[1L]]
  if (length(lower)) {
    installed <- installed_package_version(lower[[2L]], library)
    return(!is.na(installed) && utils::compareVersion(installed, lower[[3L]]) >= 0L)
  }

  plain <- regmatches(
    spec,
    regexec("^([A-Za-z][A-Za-z0-9.]*[A-Za-z0-9])$", spec, perl = TRUE)
  )[[1L]]
  if (length(plain)) {
    return(!is.na(installed_package_version(plain[[2L]], library)))
  }

  FALSE
}

installed_package_version <- function(package, library) {
  description <- packageDescription(package, lib.loc = library)
  if (!is.list(description)) {
    return(NA_character_)
  }

  version <- description$Version
  if (is.null(version) || is.na(version)) {
    return(NA_character_)
  }

  version
}

check_r_constraint <- function(constraint) {
  if (is.null(constraint)) {
    return(invisible())
  }

  constraint <- trimws(as.character(constraint[[1L]]))
  match <- regexec("^(>=|<=|==|>|<|=)\\s*([0-9][0-9.]*)$", constraint, perl = TRUE)
  parts <- regmatches(constraint, match)[[1L]]
  if (!length(parts)) {
    stop("unsupported R version constraint: ", constraint, call. = FALSE)
  }

  operator <- parts[[2L]]
  required <- parts[[3L]]
  comparison <- utils::compareVersion(as.character(getRversion()), required)
  ok <- switch(
    operator,
    ">=" = comparison >= 0L,
    ">" = comparison > 0L,
    "<=" = comparison <= 0L,
    "<" = comparison < 0L,
    "==" = comparison == 0L,
    "=" = comparison == 0L
  )

  if (!ok) {
    stop(
      "R ",
      as.character(getRversion()),
      " does not satisfy requested constraint ",
      constraint,
      call. = FALSE
    )
  }
}

hash_text <- function(text) {
  file <- tempfile("ir-hash-")
  on.exit(unlink(file), add = TRUE)
  writeLines(text, file, useBytes = TRUE)
  unname(tools::md5sum(file))
}

frontmatter <- extract_frontmatter(script)
yaml <- normalize_frontmatter(frontmatter)
metadata <- if (nzchar(trimws(yaml))) yaml12::parse_yaml(yaml, handlers = NULL) else list()

check_r_constraint(metadata$R)

dependencies <- metadata$dependencies
if (is.null(dependencies)) {
  dependencies <- character()
} else {
  dependencies <- as.character(unlist(dependencies, use.names = FALSE))
}

dependencies <- normalize_package_specs(dependencies)

if (length(dependencies)) {
  canonical <- paste(
    "R",
    as.character(getRversion()),
    R.version$platform,
    paste(sort(dependencies), collapse = "\n"),
    sep = "\n"
  )
} else {
  canonical <- paste("R", as.character(getRversion()), R.version$platform, sep = "\n")
}

hash <- hash_text(canonical)
platform <- gsub("[^A-Za-z0-9._-]+", "-", R.version$platform)
r_version <- gsub("[^A-Za-z0-9._-]+", "-", as.character(getRversion()))
library <- file.path(cache_dir, "libraries", paste0("R-", r_version, "-", platform), hash)
dir.create(library, recursive = TRUE, showWarnings = FALSE)

if (length(dependencies)) {
  if (!library_satisfies_specs(dependencies, library)) {
    pak::pkg_install(dependencies, lib = library, upgrade = FALSE, ask = FALSE, dependencies = NA)
  }
  verify_installed_specs(dependencies, library)
}

cat("IR_LIBRARY_PATH=", library, "\n", sep = "")
"#;
