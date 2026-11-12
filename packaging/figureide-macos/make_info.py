#!/usr/bin/env python3
"""Render the shared macOS bundle metadata as a FigureIDE-specific Info.plist."""

from __future__ import annotations

import plistlib
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 5:
        print("usage: make_info.py TEMPLATE OUTPUT VERSION BUILD_SHA", file=sys.stderr)
        return 2

    template, output, version, build_sha = sys.argv[1:]
    short_version = version.split("-", maxsplit=1)[0]
    with Path(template).open("rb") as stream:
        info = plistlib.load(stream)

    info["CFBundleDisplayName"] = "FigureIDE"
    info["CFBundleName"] = "FigureIDE"
    info["CFBundleExecutable"] = "FigureIDE"
    info["CFBundleIdentifier"] = "ai.storyteller.figureide"
    info["CFBundleIconFile"] = "FigureIDE"
    info["CFBundleShortVersionString"] = short_version
    info["CFBundleVersion"] = short_version
    info["FigureIDEVersion"] = version
    info["FigureIDEBuildCommit"] = build_sha or "unknown"
    info.pop("VectorCraftVersion", None)
    info.pop("VectorCraftBuildCommit", None)

    names = {"VectorCraft Document": "FigureIDE Document", "VectorCraft Template": "FigureIDE Template"}
    for document_type in info.get("CFBundleDocumentTypes", []):
        name = document_type.get("CFBundleTypeName")
        if name in names:
            document_type["CFBundleTypeName"] = names[name]

    with Path(output).open("wb") as stream:
        plistlib.dump(info, stream, fmt=plistlib.FMT_XML, sort_keys=False)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
