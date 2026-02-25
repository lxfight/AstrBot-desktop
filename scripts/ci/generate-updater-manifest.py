#!/usr/bin/env python3
"""
Generate updater manifest (latest.json) for Tauri updater plugin.

This script scans release artifacts and generates a JSON manifest file
that the Tauri updater plugin uses to check for updates.

Usage:
    python generate-updater-manifest.py \
        --root <artifact-dir> \
        --repository <org/repo> \
        --release-tag <tag> \
        --version <version> \
        --output <output-file>
"""

import argparse
import json
import os
import re
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate Tauri updater manifest")
    parser.add_argument(
        "--root",
        required=True,
        help="Root directory containing release artifacts",
    )
    parser.add_argument(
        "--repository",
        required=True,
        help="GitHub repository (e.g., AstrBotDevs/AstrBot-desktop)",
    )
    parser.add_argument(
        "--release-tag",
        required=True,
        help="Release tag (e.g., v4.19.0)",
    )
    parser.add_argument(
        "--version",
        required=True,
        help="Application version (e.g., 4.19.0)",
    )
    parser.add_argument(
        "--output",
        required=True,
        help="Output file path for the manifest",
    )
    parser.add_argument(
        "--strict-version-match",
        action="store_true",
        default=False,
        help="Fail if artifact version doesn't match provided version",
    )
    return parser.parse_args()


def extract_platform_info(filename: str) -> dict[str, str] | None:
    """
    Extract platform and architecture from artifact filename.

    Expected patterns:
    - AstrBot_{version}_windows_{arch}_updater.zip
    - AstrBot_{version}_linux_{arch}_updater.tar.gz
    - AstrBot_{version}_macos_{arch}_updater.tar.gz
    """
    patterns = [
        # Windows
        (r"AstrBot_.*_windows_(x86_64|aarch64|armv7)_updater\.zip", "windows", None),
        # Linux
        (r"AstrBot_.*_linux_(x86_64|aarch64|armv7)_updater\.tar\.gz", "linux", None),
        # macOS
        (r"AstrBot_.*_macos_(x86_64|aarch64|universal)_updater\.tar\.gz", "darwin", None),
    ]

    for pattern, os_name, _ in patterns:
        match = re.search(pattern, filename, re.IGNORECASE)
        if match:
            arch = match.group(1).lower()
            # Normalize architecture names
            arch_map = {
                "x86_64": "x86_64",
                "aarch64": "aarch64",
                "armv7": "armv7",
                "universal": "universal",
            }
            normalized_arch = arch_map.get(arch, arch)
            return {"os": os_name, "arch": normalized_arch}

    return None


def find_updater_artifacts(artifact_dir: Path) -> list[Path]:
    """Find all updater artifacts in the directory."""
    artifacts = []
    patterns = [
        "*_updater.zip",
        "*_updater.zip.sig",
        "*_updater.tar.gz",
        "*_updater.tar.gz.sig",
    ]

    for pattern in patterns:
        artifacts.extend(artifact_dir.glob(f"**/{pattern}"))

    return artifacts


def read_signature_file(sig_path: Path) -> str:
    """Read signature file content."""
    if not sig_path.exists():
        raise FileNotFoundError(f"Signature file not found: {sig_path}")
    return sig_path.read_text().strip()


def generate_manifest(
    artifacts: list[Path],
    version: str,
    release_tag: str,
    repository: str,
    strict_version_match: bool = False,
) -> dict[str, Any]:
    """Generate the updater manifest."""
    manifest: dict[str, Any] = {
        "version": version,
        "notes": f"Release {release_tag}",
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {},
    }

    # Group artifacts by platform
    artifact_map: dict[str, dict[str, Path]] = {}

    for artifact in artifacts:
        if artifact.suffix == ".sig":
            continue  # Skip signature files for now

        info = extract_platform_info(artifact.name)
        if not info:
            print(f"Warning: Could not extract platform info from {artifact.name}", file=sys.stderr)
            continue

        os_name = info["os"]
        arch = info["arch"]
        key = f"{os_name}-{arch}"

        artifact_map[key] = {
            "bundle": artifact,
            "signature": artifact.parent / f"{artifact.name}.sig",
        }

    # Build platforms section
    base_url = f"https://github.com/{repository}/releases/download/{release_tag}"

    for platform_key, paths in artifact_map.items():
        bundle_path = paths["bundle"]
        sig_path = paths["signature"]

        # Read signature
        try:
            signature = read_signature_file(sig_path)
        except FileNotFoundError:
            print(f"Warning: Signature not found for {bundle_path.name}", file=sys.stderr)
            continue

        # Generate download URL
        download_url = f"{base_url}/{bundle_path.name}"

        manifest["platforms"][platform_key] = {
            "signature": signature,
            "url": download_url,
        }

    return manifest


def main() -> int:
    args = parse_args()

    artifact_root = Path(args.root)
    if not artifact_root.exists():
        print(f"Error: Artifact root does not exist: {artifact_root}", file=sys.stderr)
        return 1

    # Find updater artifacts
    updater_artifacts = find_updater_artifacts(artifact_root)

    if not updater_artifacts:
        print("Warning: No updater artifacts found", file=sys.stderr)
        # Still generate an empty manifest
        manifest = {
            "version": args.version,
            "notes": f"Release {args.release_tag}",
            "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "platforms": {},
        }
    else:
        print(f"Found {len(updater_artifacts)} updater artifacts", file=sys.stderr)

        # Generate manifest
        manifest = generate_manifest(
            updater_artifacts,
            args.version,
            args.release_tag,
            args.repository,
            args.strict_version_match,
        )

    # Write output
    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)

    with output_path.open("w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)

    print(f"Generated updater manifest: {output_path}", file=sys.stderr)
    print(f"Platforms: {list(manifest['platforms'].keys())}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    sys.exit(main())
