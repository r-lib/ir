# ir 0.4.1

IR now prefers pre-built R packages (binaries) for requested packages and their dependencies, even when a newer source version is available. Version requirements still apply, and packages without a suitable binary can install from source.

Set `IR_PREFER_BINARIES=0` to prioritize newer versions. This does not force source installation.

After upgrading, IR checks your package choices again. Existing downloads are kept.

Without Python metadata, `ir run` now defaults an unset `RETICULATE_PYTHON` to `managed`, allowing reticulate to prepare Python from `py_require()` declarations. Explicit `RETICULATE_PYTHON` settings are preserved.

IR now requires renv 1.2.0 or later for resolver tooling and installs a compatible version when needed.
