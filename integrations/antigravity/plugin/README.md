# Relintor Antigravity plugin boundary

`bridge-manifest.json` is the versioned identity contract for the production
plugin/bridge installation boundary. The Rust adapter validates the signed
package, pinned public-key identity, target triple, exact file set and SHA-256
digests, Antigravity plugin layout, canonical executable path, and atomic
installation target before a bridge can be used.

The checked-in `bridge-manifest.json` is intentionally a source placeholder
with `signing_status=external_required`; it can never make the adapter ready.
The beta release process builds the real `relintor-antigravity-bridge` binary,
creates an exact package under `beta-package`, and signs it with the external
private key referenced by `tooling/antigravity/build-signed-bridge-package.ps1`.
The private key is never stored in this repository. The desktop embeds only
the public trust root and refuses the package until signature, target, file
digests, installation, self-test, and Antigravity plugin discovery all pass.

Antigravity's official CLI plugin location is the per-user
`~/.gemini/antigravity-cli/plugins/relintor-authority` directory (expanded by
the runtime for the current Windows user). The installer uses an atomic
staging directory and never follows a link/reparse-point package entry.
