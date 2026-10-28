# Replace one artifact without changing its name, version, URL, or byte length.
args <- commandArgs(TRUE)
root <- args[[1L]]
value <- args[[2L]]
checksums <- args[[3L]] == "checksums"
package <- file.path(root, "packages/iridentity-1.0.0-source/iridentity")
writeLines(sprintf('artifact <- function() "%s"', value),
           file.path(package, "R/artifact.R"))
src <- file.path(root, "repo/src/contrib")
archive <- file.path(src, "iridentity_1.0.0.tar.gz")
setwd(dirname(package))
utils::tar(archive, "iridentity", compression = "gzip", tar = "internal")
size <- file.size(archive)
stopifnot(size < 65536L)
connection <- file(archive, "ab")
writeBin(raw(65536L - size), connection)
close(connection)
tools::write_PACKAGES(src, type = "source", latestOnly = FALSE)
if (!checksums) {
  index <- read.dcf(file.path(src, "PACKAGES"))
  index <- index[, colnames(index) != "MD5sum", drop = FALSE]
  write.dcf(index, file.path(src, "PACKAGES"))
  unlink(file.path(src, c("PACKAGES.gz", "PACKAGES.rds")))
}
