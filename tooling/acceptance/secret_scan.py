#!/usr/bin/env python3
"""Small dependency-free baseline scanner; CI also runs Gitleaks."""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

SKIP_DIRS = {"node_modules", "target", "dist", "build", ".git", "coverage", ".next"}
PATTERNS = {
    "private_key": re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
    "aws_access_key": re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    "google_api_key": re.compile(r"\bAIza[0-9A-Za-z_-]{30,}\b"),
    "openai_like_key": re.compile(r"\bsk-[A-Za-z0-9]{20,}\b"),
    "connection_secret": re.compile(r"(?:postgres|mysql|mongodb(?:\+srv)?):\/\/[^\s:/]+:[^\s@]+@", re.IGNORECASE),
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    root = args.root.resolve()
    findings: list[str] = []
    for path in root.rglob("*"):
        if not path.is_file() or any(part in SKIP_DIRS for part in path.relative_to(root).parts):
            continue
        try:
            content = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for name, pattern in PATTERNS.items():
            if pattern.search(content):
                findings.append(f"{name}:{path.relative_to(root)}")
    if findings:
        print("SECRET_SCAN_FAIL", file=sys.stderr)
        print("\n".join(findings), file=sys.stderr)
        return 1
    print("SECRET_SCAN_PASS no matching provider/private-key/connection-secret patterns")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

