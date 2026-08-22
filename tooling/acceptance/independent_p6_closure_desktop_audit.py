from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
rust = (root / "apps/desktop/src-tauri/src/lib.rs").read_text(encoding="utf-8")
app = (root / "apps/desktop/src/App.tsx").read_text(encoding="utf-8")
backend = (root / "apps/desktop/src/backend.ts").read_text(encoding="utf-8")

failures = []

m = re.search(r"const AUTHORITY_FACT_FIELDS:.*?= &\[(.*?)\];", rust, re.S)
if not m:
    failures.append("P6-FC-01: cannot locate Rust authority fact registry")
else:
    rust_facts = set(re.findall(r'"([a-z_]+)"', m.group(1)))
    options = re.search(r"const authorityFactOptions = \[(.*?)\] as const;", app, re.S)
    if options:
        ui_facts = set(re.findall(r'\["([a-z_]+)"', options.group(1)))
        missing = sorted(rust_facts - ui_facts)
        if missing:
            failures.append(
                "P6-FC-01: normal GUI cannot resolve authority facts: " + ", ".join(missing)
            )

if 'useState<string[]>(["desktop"])' in app:
    failures.append(
        "P6-FC-01: every target project still defaults to desktop because Relintor itself is desktop"
    )

if "sha256_hex(facts.join(\"\\n\").as_bytes())" in rust:
    failures.append(
        "P6-FC-03: reviewed fact revision is still input-order sensitive"
    )

# The exact UI review must be named at the sealing boundary.
if re.search(r"sealProjectMission\s*\(\s*projectId\s*:\s*string\s*\)", backend):
    failures.append(
        "P6-FC-02: frontend Seal & Build still does not send the displayed review digest"
    )

if re.search(
    r"fn seal_project_mission\s*\(\s*app:\s*AppHandle,\s*project_id:\s*String\s*\)",
    rust,
    re.S,
):
    failures.append(
        "P6-FC-02: Rust sealing boundary still does not require expected_review_digest"
    )

if "preview.review_digest" not in app:
    failures.append(
        "P6-FC-02: displayed AuthorityPreview.review_digest is not bound to the seal action"
    )

if failures:
    print("P6 closure desktop audit: FAIL")
    for failure in failures:
        print(" -", failure)
    sys.exit(1)

print("P6 closure desktop audit: PASS")
