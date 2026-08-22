from pathlib import Path
import sys

root = Path(__file__).resolve().parents[2]
rust = (root / "apps/desktop/src-tauri/src/lib.rs").read_text(encoding="utf-8")
app = (root / "apps/desktop/src/App.tsx").read_text(encoding="utf-8")

failures = []

if 'SELECT id, fingerprint FROM project_takeovers ORDER BY created_at DESC LIMIT 1' in rust:
    failures.append("P6-FR-05: takeover authority is selected globally instead of by project binding")

if 'params![project_id]' in rust and 'SELECT COALESCE(MAX(revision), 0) FROM mission_revisions WHERE mission_id = ?1' in rust:
    failures.append("P6-FR-04: mission revision lookup still uses project_id instead of canonical mission_id")

if '"desktop-rust-seal"' in rust:
    failures.append("P6-FR: desktop seal still uses a constant pseudo-timestamp")

if 'Review the applicable requirements and evidence obligations before sealing.' in rust:
    failures.append("P6-FR-03: authority_preview still carries the hard-coded review blocker")

if 'disabled={sealBusy || !projectId || preview.blockers.length > 0}' in app and 'projectId={investigation?.investigation.project_id ?? null}' in app:
    failures.append("P6-FR-03/05: Seal & Build remains unavailable to takeover-only flow and depends on always-populated preview blockers")

if failures:
    print("P6 final desktop source audit: FAIL")
    for failure in failures:
        print(" -", failure)
    sys.exit(1)

print("P6 final desktop source audit: static known-defect patterns cleared")
