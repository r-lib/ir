# A local GitHub API response and one archive with two same-version packages.
args <- commandArgs(TRUE)
root <- normalizePath(args[[1L]], winslash = "/", mustWork = TRUE)
sha <- paste(rep("a", 40L), collapse = "")
description <- paste(c("Package: irsubdir", "Version: 1.0.0",
                       "Title: IR Subdirectory Fixture",
                       "Description: Tests source subdirectory identity.",
                       "License: MIT"), collapse = "\n")
for (subdir in c("one", "two")) {
  package <- file.path(root, "snapshot", subdir)
  dir.create(file.path(package, "R"), recursive = TRUE)
  writeLines(description, file.path(package, "DESCRIPTION"))
  writeLines("export(artifact)", file.path(package, "NAMESPACE"))
  writeLines(sprintf('artifact <- function() "%s"', subdir),
             file.path(package, "R/artifact.R"))
  contents <- file.path(root, "repos/fixture/subdirs/contents", subdir)
  dir.create(contents, recursive = TRUE)
  writeLines(description, file.path(contents, "DESCRIPTION"))
}

writeLines(paste0('{"data":{"repository":{"desc1":{"isBinary":false,"text":',
                  encodeString(description, quote = '"'),
                  '},"sha":{"oid":"', sha, '"}}}}'),
           file.path(root, "graphql"))
archive <- file.path(root, "repos/fixture/subdirs/tarball", sha)
dir.create(dirname(archive), recursive = TRUE)
setwd(root)
utils::tar(archive, "snapshot", compression = "gzip", tar = "internal")

# renv prefixes GitHub hosts with HTTPS. The fixture API uses local HTTP.
writeLines(c(sprintf("source(%s)", deparse(file.path(root, "profile.R"))),
  'options(renv.download.override = function(url, destfile, ...) {',
  '  url <- sub("^https://127.0.0.1:", "http://127.0.0.1:", url)',
  '  utils::download.file(url, destfile, quiet = TRUE, mode = "wb")',
  '})'), file.path(root, "github-profile.R"))
