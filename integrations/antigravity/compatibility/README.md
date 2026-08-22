# Compatibility evidence boundary

`registry.json` is the versioned fail-closed compatibility registry used by the
Rust Antigravity bridge. The official CLI name is `agy`/`agy.exe`; executable
presence alone is not compatibility evidence. Detection records the resolved
path, version output, CLI invocation capability, plugin/hook capability,
platform, environment, and timestamp. Unknown or unsupported versions cannot
start sealed execution.

The current registry intentionally contains only the supported `1.x` prefix.
This is a foundation registry, not a claim that every future Antigravity build
is supported.
