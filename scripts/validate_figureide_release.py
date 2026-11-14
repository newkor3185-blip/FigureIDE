#!/usr/bin/env python3
"""Validate the four FigureIDE beta archives and emit a checksum manifest."""

from __future__ import annotations

import argparse
import hashlib
import plistlib
import tarfile
import zipfile
from pathlib import Path


PROJECT_FILES = ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ASSETS.md", "THIRD-PARTY-LICENSES.md")


def build_info(text: str, *, version: str, commit: str, target: str) -> None:
    fields = {}
    for line in text.splitlines():
        key, separator, value = line.partition("=")
        if separator:
            fields[key] = value
    expected = {"version": version, "commit": commit, "target": target}
    for key, value in expected.items():
        if fields.get(key) != value:
            raise ValueError(f"build metadata {key!r} is {fields.get(key)!r}; expected {value!r}")


def zip_package(path: Path, *, version: str, commit: str, target: str, mac: bool = False) -> None:
    with zipfile.ZipFile(path) as archive:
        bad = archive.testzip()
        if bad:
            raise ValueError(f"{path.name}: corrupt ZIP member {bad}")
        names = set(archive.namelist())
        if mac:
            root = "FigureIDE.app/Contents/Resources/"
            build_path = root + "BUILD_INFO.txt"
            license_root = root
            plist_path = "FigureIDE.app/Contents/Info.plist"
            plist = plistlib.loads(archive.read(plist_path))
            if plist.get("FigureIDEVersion") != version:
                raise ValueError(f"{path.name}: Info.plist FigureIDEVersion mismatch")
            numeric = version.split("-", maxsplit=1)[0]
            if plist.get("CFBundleShortVersionString") != numeric or plist.get("CFBundleVersion") != numeric:
                raise ValueError(f"{path.name}: numeric macOS bundle versions do not match {numeric}")
        else:
            root = next((name.rsplit("/", 1)[0] + "/" for name in names if name.endswith("/FigureIDE.exe")), "")
            if not root:
                raise ValueError(f"{path.name}: FigureIDE.exe is missing")
            build_path = root + "BUILD_INFO.txt"
            license_root = root
        if build_path not in names:
            raise ValueError(f"{path.name}: BUILD_INFO.txt is missing")
        build_info(archive.read(build_path).decode("utf-8-sig"), version=version, commit=commit, target=target)
        for filename in PROJECT_FILES:
            member = license_root + filename
            if member not in names or not archive.read(member).strip():
                raise ValueError(f"{path.name}: missing or empty {filename}")
        if not any(name.startswith(license_root + "asset-licenses/") and not name.endswith("/") for name in names):
            raise ValueError(f"{path.name}: bundled asset license texts are missing")


def linux_package(path: Path, *, version: str, commit: str) -> None:
    target = "x86_64-unknown-linux-gnu"
    with tarfile.open(path, "r:gz") as archive:
        members = {member.name: member for member in archive.getmembers()}
        root = "FigureIDE-Linux-x86_64/"
        for name in (root + "FigureIDE", root + "BUILD_INFO.txt"):
            member = members.get(name)
            if member is None or not member.isfile():
                raise ValueError(f"{path.name}: missing package member {name}")
        executable = members[root + "FigureIDE"]
        if executable.mode & 0o111 == 0:
            raise ValueError(f"{path.name}: FigureIDE is not executable")
        stream = archive.extractfile(members[root + "BUILD_INFO.txt"])
        if stream is None:
            raise ValueError(f"{path.name}: BUILD_INFO.txt cannot be read")
        build_info(stream.read().decode("utf-8"), version=version, commit=commit, target=target)
        for filename in PROJECT_FILES:
            member = members.get(root + filename)
            if member is None or not member.isfile() or member.size == 0:
                raise ValueError(f"{path.name}: missing or empty {filename}")
        if not any(name.startswith(root + "asset-licenses/") and members[name].isfile() for name in members):
            raise ValueError(f"{path.name}: bundled asset license texts are missing")


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    version = args.version.removeprefix("v")
    root = args.directory
    files = [
        (root / "windows" / f"FigureIDE-v{version}-Windows-x64.zip", "x86_64-pc-windows-msvc", "zip"),
        (root / "linux" / f"FigureIDE-v{version}-Linux-x86_64.tar.gz", "x86_64-unknown-linux-gnu", "linux"),
        (root / "macos-apple-silicon" / f"FigureIDE-v{version}-macOS-AppleSilicon.zip", "aarch64-apple-darwin", "mac"),
        (root / "macos-intel" / f"FigureIDE-v{version}-macOS-Intel.zip", "x86_64-apple-darwin", "mac"),
    ]
    validated = []
    for path, target, kind in files:
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"required distribution archive is missing or empty: {path}")
        if kind == "linux":
            linux_package(path, version=version, commit=args.commit)
        else:
            zip_package(path, version=version, commit=args.commit, target=target, mac=kind == "mac")
        validated.append(path)
        print(f"validated {path.name}")
    manifest = root / "SHA256SUMS"
    manifest.write_text("".join(f"{digest(path)}  {path.name}\n" for path in validated), encoding="utf-8")
    print(f"wrote {manifest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
