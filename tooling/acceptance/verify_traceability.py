#!/usr/bin/env python3
"""Validate the individual implementation record for every sealed feature."""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REQUIRED_FIELDS = {
    "feature_id",
    "feature_name",
    "primary_subsystem",
    "primary_phase",
    "dependencies",
    "planned_test_families",
    "required_evidence_families",
    "implementation_status",
    "verification_status",
}

IMPLEMENTATION_STATUSES = {
    "not_started",
    "foundation_only",
    "in_progress",
    "partially_implemented",
    "implemented",
}

VERIFICATION_STATUSES = {
    "not_started",
    "not_run",
    "blocked_environment",
    "failed",
    "passed",
    "verified",
}


def require_string_list(value, field: str, feature_id: str, allow_empty: bool) -> list[str]:
    if not isinstance(value, list) or any(not isinstance(item, str) or not item for item in value):
        raise RuntimeError(f"{feature_id} {field} must be a list of non-empty strings")
    if not allow_empty and not value:
        raise RuntimeError(f"{feature_id} {field} must not be empty")
    if len(value) != len(set(value)):
        raise RuntimeError(f"{feature_id} {field} contains duplicates")
    return value


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    args = parser.parse_args()
    root = args.root.resolve()

    try:
        sealed = json.loads(
            (root / "spec" / "locked" / "feature-register.json").read_text(
                encoding="utf-8"
            )
        )
        expected = {record["id"]: record for record in sealed["features"]}

        payload = json.loads(
            (
                root
                / "tooling"
                / "acceptance"
                / "implementation-traceability.json"
            ).read_text(encoding="utf-8")
        )

        if payload.get("schema_version") != 1:
            raise RuntimeError("traceability schema_version must be 1")

        records = payload.get("records")
        if not isinstance(records, list) or len(records) != 144:
            found = len(records) if isinstance(records, list) else "invalid"
            raise RuntimeError(
                f"traceability must contain exactly 144 records; found {found}"
            )

        actual: dict[str, dict] = {}

        for record in records:
            if not isinstance(record, dict):
                raise RuntimeError("traceability contains a non-object record")

            missing = REQUIRED_FIELDS - set(record)
            if missing:
                raise RuntimeError(
                    f"{record.get('feature_id', '<unknown>')} missing fields: "
                    f"{sorted(missing)}"
                )

            feature_id = record["feature_id"]
            if feature_id in actual:
                raise RuntimeError(f"duplicate traceability feature ID: {feature_id}")
            if feature_id not in expected:
                raise RuntimeError(f"orphan traceability feature ID: {feature_id}")

            actual[feature_id] = record
            sealed_record = expected[feature_id]

            if record["feature_name"] != sealed_record["feature"]:
                raise RuntimeError(f"feature name mismatch: {feature_id}")

            sealed_phases = sealed_record["phase"].split("/")
            if record["primary_phase"] not in sealed_phases:
                raise RuntimeError(
                    f"{feature_id} primary phase {record['primary_phase']} "
                    f"is not allowed by sealed phase {sealed_record['phase']}"
                )

            dependencies = require_string_list(
                record["dependencies"],
                "dependencies",
                feature_id,
                allow_empty=True,
            )
            for dependency in dependencies:
                if dependency == feature_id:
                    raise RuntimeError(f"{feature_id} depends on itself")
                if dependency not in expected:
                    raise RuntimeError(
                        f"{feature_id} has unknown dependency {dependency}"
                    )

            require_string_list(
                record["planned_test_families"],
                "planned_test_families",
                feature_id,
                allow_empty=False,
            )
            require_string_list(
                record["required_evidence_families"],
                "required_evidence_families",
                feature_id,
                allow_empty=False,
            )

            implementation_status = record["implementation_status"]
            verification_status = record["verification_status"]

            if implementation_status not in IMPLEMENTATION_STATUSES:
                raise RuntimeError(
                    f"{feature_id} has invalid implementation_status "
                    f"{implementation_status}"
                )
            if verification_status not in VERIFICATION_STATUSES:
                raise RuntimeError(
                    f"{feature_id} has invalid verification_status "
                    f"{verification_status}"
                )
            if (
                implementation_status == "not_started"
                and verification_status in {"passed", "verified"}
            ):
                raise RuntimeError(
                    f"{feature_id} cannot be verified while implementation is not_started"
                )

        if set(actual) != set(expected):
            raise RuntimeError(
                "traceability ID mismatch; "
                f"missing={sorted(set(expected) - set(actual))}, "
                f"extra={sorted(set(actual) - set(expected))}"
            )

        print(
            f"TRACEABILITY_VERIFY_PASS records={len(records)} "
            f"unique_ids={len(actual)}"
        )
        return 0

    except (OSError, json.JSONDecodeError, KeyError, TypeError, RuntimeError) as exc:
        print(f"TRACEABILITY_VERIFY_FAIL {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
