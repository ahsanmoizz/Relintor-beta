#!/usr/bin/env python3
"""Dependency-free integrity and feature-register verifier for the locked authority."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

EXPECTED_FILES = {
    "README.md",
    "00_PRODUCT_CONSTITUTION.md",
    "01_UX_UI_SPEC.md",
    "02_SYSTEM_ARCHITECTURE.md",
    "03_ANTIGRAVITY_INTEGRATION_CONTRACT.md",
    "04_INVESTIGATION_AND_STANDARDS_ENGINE.md",
    "05_REQUIREMENT_EVIDENCE_GRAPH.md",
    "06_EXECUTION_WATCHDOG_RECOVERY.md",
    "07_SUBSCRIPTIONS_TEAMS_ADMIN.md",
    "08_FEATURE_REGISTER_144.md",
    "09_IMPLEMENTATION_PHASES.md",
    "10_VERIFICATION_AND_RELEASE_GATES.md",
    "11_COMPETITIVE_POSITIONING.md",
    "12_BRAND_AND_HERO.md",
    "RELINTOR_MASTER_SPEC.md",
    "feature-register.json",
    "MANIFEST.sha256.json",
    "hero-prototype.html",
}
MANIFEST_ENTRIES = EXPECTED_FILES - {"MANIFEST.sha256.json"}
EXPECTED_IDS = {
    f"{letter}-{number:02d}"
    for letter in "ABCDEFGHIJKL"
    for number in range(1, 13)
}
SHA256_RE = re.compile(r"^[0-9a-fA-F]{64}$")


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def fail(message: str) -> None:
    raise RuntimeError(message)


def verify(root: Path) -> list[str]:
    locked = root / "spec" / "locked"
    pin_path = root / "spec" / "LOCKED_MANIFEST.sha256"

    if not locked.is_dir():
        fail(f"locked directory missing: {locked}")
    if not pin_path.is_file():
        fail(f"external manifest pin missing: {pin_path}")

    pin_text = pin_path.read_text(encoding="utf-8").strip()
    if not SHA256_RE.fullmatch(pin_text):
        fail("external manifest pin must contain exactly one SHA-256 digest")
    pinned = pin_text.lower()

    entries = list(locked.iterdir())
    for path in entries:
        if path.is_symlink() or not path.is_file():
            fail(f"locked authority contains a non-regular file entry: {path.name}")

    actual_files = {path.name for path in entries}
    if actual_files != EXPECTED_FILES:
        fail(
            "sealed file set mismatch; "
            f"missing={sorted(EXPECTED_FILES - actual_files)}, "
            f"extra={sorted(actual_files - EXPECTED_FILES)}"
        )

    manifest_path = locked / "MANIFEST.sha256.json"
    actual_manifest_hash = digest(manifest_path)
    if pinned != actual_manifest_hash:
        fail("MANIFEST.sha256.json does not match the external pinned hash")

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or set(manifest) != MANIFEST_ENTRIES:
        fail("manifest entries do not match the expected sealed files excluding the manifest itself")

    for name, record in manifest.items():
        if not isinstance(record, dict):
            fail(f"manifest record is invalid: {name}")

        expected_bytes = record.get("bytes")
        expected_hash = record.get("sha256")

        if not isinstance(expected_bytes, int) or expected_bytes < 0:
            fail(f"manifest byte length is invalid: {name}")
        if not isinstance(expected_hash, str) or not SHA256_RE.fullmatch(expected_hash):
            fail(f"manifest SHA-256 is invalid: {name}")

        path = locked / name
        data = path.read_bytes()
        if len(data) != expected_bytes:
            fail(f"byte length mismatch: {name}")
        if digest(path) != expected_hash.lower():
            fail(f"SHA-256 mismatch: {name}")

    feature_json = json.loads(
        (locked / "feature-register.json").read_text(encoding="utf-8")
    )
    records = feature_json.get("features")
    if not isinstance(records, list) or len(records) != 144:
        found = len(records) if isinstance(records, list) else "invalid"
        fail(f"feature register must contain exactly 144 records; found {found}")

    json_by_id: dict[str, dict] = {}
    for record in records:
        if not isinstance(record, dict):
            fail("feature register contains a non-object record")
        feature_id = record.get("id")
        if not isinstance(feature_id, str):
            fail("feature record contains no string ID")
        if feature_id in json_by_id:
            fail(f"duplicate feature ID: {feature_id}")
        json_by_id[feature_id] = record

    ids = set(json_by_id)
    if ids != EXPECTED_IDS:
        fail(
            f"feature IDs mismatch; "
            f"missing={sorted(EXPECTED_IDS - ids)}, "
            f"extra={sorted(ids - EXPECTED_IDS)}"
        )

    markdown = (locked / "08_FEATURE_REGISTER_144.md").read_text(encoding="utf-8")
    markdown_rows = {
        match.group(1): (match.group(2).strip(), match.group(3).strip())
        for match in re.finditer(
            r"^\|\s*([A-L]-\d{2})\s*\|\s*(.*?)\s*\|\s*(.*?)\s*\|\s*$",
            markdown,
            re.MULTILINE,
        )
    }

    if set(markdown_rows) != EXPECTED_IDS:
        fail("Markdown feature register does not contain exactly the expected IDs")

    for feature_id, record in json_by_id.items():
        expected_row = (
            str(record.get("feature", "")).strip(),
            str(record.get("phase", "")).strip(),
        )
        if markdown_rows[feature_id] != expected_row:
            fail(f"Markdown/JSON feature mismatch: {feature_id}")

    return [
        (
            f"SPEC_VERIFY_PASS files={len(actual_files)} "
            f"manifest_entries={len(manifest)} "
            f"features={len(records)} unique_ids={len(ids)}"
        ),
        f"MANIFEST_SHA256={actual_manifest_hash}",
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    args = parser.parse_args()

    try:
        print("\n".join(verify(args.root.resolve())))
    except (OSError, json.JSONDecodeError, RuntimeError) as exc:
        print(f"SPEC_VERIFY_FAIL {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
