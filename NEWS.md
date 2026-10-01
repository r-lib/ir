# Development version

IR now prefers pre-built R packages (binaries) for requested packages and their dependencies, even when a newer source version is available. Version requirements still apply, and packages without a suitable binary can install from source.

Set `IR_PREFER_BINARIES=0` to prioritize newer versions. This does not force source installation.

After upgrading, IR checks your package choices again. Existing downloads are kept.
