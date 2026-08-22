#!/usr/bin/env python3
"""Acceptance fixtures proving the spec verifier fails closed on tampering."""
from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VERIFIER = ROOT / "tooling" / "spec" / "verify_spec.py"


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fixture() -> Path:
    temp = Path(tempfile.mkdtemp(prefix="relintor-spec-fixture-"))
    locked = temp / "spec" / "locked"
    locked.mkdir(parents=True)

    for path in (ROOT / "spec" / "locked").iterdir():
        shutil.copy2(path, locked / path.name)

    shutil.copy2(
        ROOT / "spec" / "LOCKED_MANIFEST.sha256",
        temp / "spec" / "LOCKED_MANIFEST.sha256",
    )
    return temp


def refresh_feature_manifest(temp: Path) -> None:
    locked = temp / "spec" / "locked"
    manifest_path = locked / "MANIFEST.sha256.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))

    feature_path = locked / "feature-register.json"
    manifest["feature-register.json"] = {
        "sha256": digest(feature_path),
        "bytes": feature_path.stat().st_size,
    }
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    (temp / "spec" / "LOCKED_MANIFEST.sha256").write_text(
        digest(manifest_path) + "\n",
        encoding="utf-8",
    )


def run(temp: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(VERIFIER), "--root", str(temp)],
        capture_output=True,
        text=True,
        check=False,
    )


def expect_failure(name: str, mutate, refresh: bool = False) -> None:
    temp = fixture()
    try:
        mutate(temp)
        if refresh:
            refresh_feature_manifest(temp)
        result = run(temp)
        if result.returncode == 0:
            raise AssertionError(f"{name} unexpectedly passed")
        print(f"{name}=FAIL_AS_EXPECTED")
    finally:
        shutil.rmtree(temp, ignore_errors=True)


def main() -> int:
    baseline = fixture()
    try:
        result = run(baseline)
        if result.returncode != 0:
            raise AssertionError(result.stderr)
        print("baseline=PASS")
    finally:
        shutil.rmtree(baseline, ignore_errors=True)

    expect_failure(
        "locked_file_mutation",
        lambda temp: (temp / "spec" / "locked" / "README.md").write_text(
            (temp / "spec" / "locked" / "README.md").read_text(encoding="utf-8")
            + "\nMUTATION",
            encoding="utf-8",
        ),
    )

    expect_failure(
        "manifest_mutation",
        lambda temp: (temp / "spec" / "locked" / "MANIFEST.sha256.json").write_text(
            (temp / "spec" / "locked" / "MANIFEST.sha256.json").read_text(
                encoding="utf-8"
            )
            + "\n",
            encoding="utf-8",
        ),
    )

    expect_failure(
        "external_pin_mutation",
        lambda temp: (temp / "spec" / "LOCKED_MANIFEST.sha256").write_text(
            "0" * 64 + "\n",
            encoding="utf-8",
        ),
    )

    expect_failure(
        "unexpected_locked_file",
        lambda temp: (temp / "spec" / "locked" / "UNAUTHORIZED.txt").write_text(
            "not authority\n",
            encoding="utf-8",
        ),
    )

    def unknown(temp: Path) -> None:
        path = temp / "spec" / "locked" / "feature-register.json"
        data = json.loads(path.read_text(encoding="utf-8"))
        data["features"][0]["id"] = "Z-99"
        path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")

    def duplicate(temp: Path) -> None:
        path = temp / "spec" / "locked" / "feature-register.json"
        data = json.loads(path.read_text(encoding="utf-8"))
        data["features"][-1]["id"] = data["features"][0]["id"]
        path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")

    def missing(temp: Path) -> None:
        path = temp / "spec" / "locked" / "feature-register.json"
        data = json.loads(path.read_text(encoding="utf-8"))
        data["features"].pop()
        path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")

    expect_failure("unknown_id", unknown, True)
    expect_failure("duplicate_id", duplicate, True)
    expect_failure("missing_record", missing, True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
