#!/usr/bin/env python3
"""
Render temporary Tauri build config with updater settings.

This script generates a JSON config file that overrides tauri.conf.json
settings for building with updater artifacts enabled.

Usage:
    python render-tauri-build-config.py \
        --output <output-file> \
        --updater-endpoint <endpoint> \
        --updater-pubkey <pubkey>
"""

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Render Tauri build config")
    parser.add_argument(
        "--output",
        required=True,
        help="Output file path for the config",
    )
    parser.add_argument(
        "--updater-endpoint",
        required=False,
        default="",
        help="Updater endpoint URL (optional, uses default if empty)",
    )
    parser.add_argument(
        "--updater-pubkey",
        required=False,
        default="",
        help="Updater public key (optional, uses config default if empty)",
    )
    parser.add_argument(
        "--disable-updater-artifacts",
        action="store_true",
        default=False,
        help="Disable updater artifact generation",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    # Default updater endpoint
    default_endpoint = "https://github.com/AstrBotDevs/AstrBot-desktop/releases/latest/download/latest.json"

    config: dict[str, Any] = {
        "bundle": {
            "createUpdaterArtifacts": not args.disable_updater_artifacts,
        },
        "plugins": {
            "updater": {
                "endpoints": [args.updater_endpoint or default_endpoint],
            },
        },
    }

    # Only include pubkey if provided
    if args.updater_pubkey:
        config["plugins"]["updater"]["pubkey"] = args.updater_pubkey

    # Write output
    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)

    with output_path.open("w", encoding="utf-8") as f:
        json.dump(config, f, indent=2)

    print(f"Generated Tauri build config: {output_path}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    sys.exit(main())
