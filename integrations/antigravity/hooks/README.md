# Hook boundary

The production bridge consumes structured `relintor-antigravity-hooks-v1`
messages. Relintor validates the mission, task packet, lease, worktree identity,
and exact action digest before returning a native permission decision. Post-tool
results are captured against the same action identity. A stop message creates a
safe incomplete boundary and can never mint completion.

No arbitrary hook script is installed or executed. The signed package installer
requires versioned, path-validated, exact-integrity files. The real Antigravity
runtime remains an external environment gate when no supported executable is
installed.
