# Development version

IR now defaults to fast package installation: it prefers compatible downloadable binaries, even when a newer source release is available. This applies to requested packages and their dependencies. Explicit requirements remain authoritative, and source-only packages can install alongside binaries.

Set `IR_PREFER_BINARIES=0` to prioritize version freshness over avoiding compilation. This does not force source-only installation. The default requires no caller configuration.

The selected package artifacts now survive the handoff to renv. Missing binaries trigger a bounded re-solve using the remaining repository candidates. Resolution and environment cache identities have changed so older cached plans cannot bypass the new default; existing downloads and renv package caches are retained.
