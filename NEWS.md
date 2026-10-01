# Development version

IR now prefers compatible pre-built binaries for requested packages and their dependencies, even when a newer source version is available. Explicit version requirements still apply, and packages without a suitable binary fall back to source.

Set `IR_PREFER_BINARIES=0` to prioritize newer versions. This does not force source installation.

Existing cached plans are refreshed for the new default; download caches are retained.
