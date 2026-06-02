#!/usr/bin/env -S ir run
# dependencies:
#   - cli<99.0.0
# R: >= 4.0

library(cli)
cat(as.character(packageVersion("cli")))
