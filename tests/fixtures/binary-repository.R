# Tiny, real source and installed archives. No compiler or public repository.
args <- commandArgs(TRUE)
root <- normalizePath(args[[1]], winslash = "/", mustWork = TRUE)
tooling <- file.path(root, "tooling")
dir.create(tooling)
for (pkg in c("pak", "renv", "secretbase")) {
  stopifnot(file.copy(system.file(package = pkg), tooling, recursive = TRUE))
}
repo <- file.path(root, "repo")
src <- file.path(repo, "src/contrib")
platform <- pak::system_r_platform()
minor <- paste(R.version$major, strsplit(R.version$minor, ".", fixed = TRUE)[[1]][1], sep = ".")
binary <- if (.Platform$pkgType == "source") {
  file.path(repo, "bin", platform, minor)
} else {
  sub("^file://", "", contrib.url(paste0("file://", repo), type = .Platform$pkgType))
}
dir.create(src, recursive = TRUE)
dir.create(binary, recursive = TRUE)
lib <- file.path(root, "build-library")
dir.create(lib)

package <- function(name, version, artifact, imports = NULL, sentinel = FALSE) {
  path <- file.path(root, "packages", paste0(name, "-", version, "-", artifact), name)
  dir.create(file.path(path, "R"), recursive = TRUE)
  writeLines(c(paste("Package:", name), paste("Version:", version),
               "Title: IR Binary Fixture", "Description: Tests artifact selection.",
               "License: MIT", if (!is.null(imports)) paste("Imports:", imports)),
             file.path(path, "DESCRIPTION"))
  writeLines("export(artifact)", file.path(path, "NAMESPACE"))
  writeLines(sprintf('artifact <- function() "%s"', artifact), file.path(path, "R/artifact.R"))
  if (sentinel) {
    writeLines('stop("IR_UNEXPECTED_SOURCE_ARTIFACT")', file.path(path, "R/artifact.R"))
  }
  path
}

source_archive <- function(name, version, imports = NULL, sentinel = FALSE) {
  path <- package(name, version, "source", imports, sentinel)
  old <- setwd(dirname(path))
  on.exit(setwd(old))
  utils::tar(file.path(src, paste0(name, "_", version, ".tar.gz")), name,
             compression = "gzip", tar = "internal")
}

binary_archive <- function(name, version, imports = NULL) {
  path <- package(name, version, "binary", imports)
  install.packages(path, repos = NULL, type = "source", lib = lib,
                   INSTALL_opts = c("--no-test-load", "--no-help", "--no-docs"))
  stopifnot(dir.exists(file.path(lib, name)))
  old <- setwd(lib)
  on.exit(setwd(old))
  if (.Platform$OS.type == "windows") {
    utils::zip(file.path(binary, paste0(name, "_", version, ".zip")), name)
  } else {
    ext <- if (.Platform$pkgType == "source") ".tar.gz" else ".tgz"
    utils::tar(file.path(binary, paste0(name, "_", version, ext)), name,
               compression = "gzip", tar = "internal")
  }
}

source_archive("irsourceonly", "1.0.0")
source_archive("irnewonly", "1.0.0")
install.packages(file.path(src, "irsourceonly_1.0.0.tar.gz"), repos = NULL, lib = lib)
source_archive("irlag", "1.0.0", "irsourceonly", sentinel = TRUE)
source_archive("irlag", "2.0.0", "irsourceonly, irnewonly")
binary_archive("irlag", "0.5.0", "irsourceonly")
binary_archive("irlag", "1.0.0", "irsourceonly")
source_archive("irparent", "1.0.0", "irlag")
source_archive("irminimum", "1.0.0", "irlag (>= 2.0.0)")
source_archive("irsame", "1.0.0", sentinel = TRUE)
binary_archive("irsame", "1.0.0")
source_archive("iridentity", "1.0.0")
binary_archive("iridentity", "1.0.0")
source_archive("irconflict", "1.0.0", "irsourceonly (>= 2.0.0)")
source_archive("irconflict", "2.0.0", "irsourceonly")
source_archive("irchoice", "2.0.0", "irsourceonly")
binary_archive("irchoice", "0.5.0", "irsourceonly")
# Build the incompatible candidate without invoking its dependency at load time.
file.copy(file.path(lib, "irsourceonly", "DESCRIPTION"), file.path(root, "dep-description"))
d <- read.dcf(file.path(lib, "irsourceonly", "DESCRIPTION"))
d[1, "Version"] <- "2.0.0"
write.dcf(d, file.path(lib, "irsourceonly", "DESCRIPTION"))
binary_archive("irconflict", "1.0.0", "irsourceonly (>= 2.0.0)")
binary_archive("irchoice", "1.0.0", "irsourceonly (>= 2.0.0)")
tools::write_PACKAGES(src, type = "source", latestOnly = FALSE)
binary_type <- if (.Platform$pkgType == "source") "source" else .Platform$pkgType
tools::write_PACKAGES(binary, type = binary_type, fields = "Built",
                      addFiles = TRUE, latestOnly = FALSE)
writeLines(sub(paste0(root, "/"), "", binary, fixed = TRUE), file.path(root, "binary-path"))
