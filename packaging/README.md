# Relintor distribution boundary

P11 exercises the Windows local distribution path through the current Tauri
`--no-bundle` build. The bundled installer, signing, update, and notarization
artifacts remain release-gate work.

- Windows local build: `pnpm --dir apps/desktop tauri build --no-bundle`
- macOS: `NOT_RUN_CROSS_PLATFORM_GATE_DEFERRED`
- Linux: `NOT_RUN_CROSS_PLATFORM_GATE_DEFERRED`
- Signing keys and updater metadata: no keys are stored in this repository;
  `P3_PLUGIN_SIGNING_PACKAGING=DEFERRED_TO_RELEASE_GATE`

The desktop UI reports unsupported or unavailable external distribution state
instead of presenting a mock installer or a false release certification.
