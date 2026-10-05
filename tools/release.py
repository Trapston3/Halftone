#!/usr/bin/env python3
"""Halftone release staging tool (stdlib only).

Stages built artifacts into dist/ under the ASSET NAMING CONTRACT that the
README installer (tools/install.ps1) and the app's OTA updater both depend
on — do not deviate from these names:

    Halftone.exe              Windows build (stable, unversioned name — the
                              OTA manifest points at this exact URL forever)
    Halftone_amd64.AppImage   Linux (optional)
    Halftone_amd64.deb        Linux (optional)
    latest.json               OTA manifest (tagged, immutable release URL)

Writes dist/latest.json with version, tagged download URL, sha256 and size.
Refuses to run when --version does not match src-tauri/tauri.conf.json and
src-tauri/Cargo.toml (the app compares its own version against latest.json;
a mismatched manifest would make every client see a bogus update).

The CI workflow calls this script, then creates the GitHub release with the
printed command. Locally it only PRINTS the gh command — it never uploads.

Usage:
    python3 tools/release.py --version 0.2.0 \
        --exe target/release/Halftone.exe \
        [--appimage target/release/Halftone_amd64.AppImage] \
        [--deb target/release/Halftone_amd64.deb] \
        [--notes "what changed"]
"""

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

REPO = "Trapston3/Halftone"
# The OTA URL contract: stable name on an immutable tagged release.
EXE_NAME = "Halftone.exe"
APPIMAGE_NAME = "Halftone_amd64.AppImage"
DEB_NAME = "Halftone_amd64.deb"


def die(msg: str) -> None:
    print(f"release.py: error: {msg}", file=sys.stderr)
    sys.exit(1)


def project_version() -> str:
    """Version from src-tauri/tauri.conf.json (the app ships this string)."""
    conf = Path("src-tauri/tauri.conf.json")
    if not conf.is_file():
        die(f"{conf} not found — run from the repo root")
    try:
        return json.loads(conf.read_text(encoding="utf-8"))["version"]
    except (json.JSONDecodeError, KeyError) as e:
        die(f"{conf}: {e}")


def cargo_version() -> str:
    """Version from src-tauri/Cargo.toml ([package] version, plain 'x.y.z')."""
    toml = Path("src-tauri/Cargo.toml")
    if not toml.is_file():
        die(f"{toml} not found — run from the repo root")
    in_pkg = False
    for line in toml.read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("[package]"):
            in_pkg = True
            continue
        if s.startswith("[") and s.endswith("]"):
            in_pkg = False
            continue
        if in_pkg and s.startswith("version"):
            _, _, val = s.partition("=")
            return val.strip().strip('"').strip("'")
    die(f"{toml}: no [package] version found")


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    ap = argparse.ArgumentParser(description="Stage Halftone release artifacts")
    ap.add_argument("--version", required=True, help="release version, e.g. 0.2.0 (no 'v')")
    ap.add_argument("--exe", help="path to the built Windows exe")
    ap.add_argument("--appimage", help="path to the built Linux AppImage")
    ap.add_argument("--deb", help="path to the built Linux .deb")
    ap.add_argument("--notes", default="", help="release notes for latest.json / gh --notes-file")
    ap.add_argument("--ci", action="store_true",
                    help="CI mode: stage only; the workflow publishes (no gh command hints)")
    args = ap.parse_args()

    version = args.version.strip().lstrip("vV")

    # --- version guard: manifest must describe the app being released ------
    conf_v = project_version()
    cargo_v = cargo_version()
    if version != conf_v:
        die(f"--version {version} != src-tauri/tauri.conf.json version {conf_v}")
    if version != cargo_v:
        die(f"--version {version} != src-tauri/Cargo.toml version {cargo_v}")

    if not args.exe and not args.appimage and not args.deb:
        die("nothing to stage: pass --exe and/or --appimage/--deb")

    dist = Path("dist")
    dist.mkdir(exist_ok=True)

    staged = []

    def stage(src: str, name: str) -> Path:
        s = Path(src)
        if not s.is_file():
            die(f"artifact not found: {s}")
        d = dist / name
        shutil.copyfile(s, d)  # contract name; overwrites a previous stage
        return d

    if args.exe:
        staged.append(stage(args.exe, EXE_NAME))
    if args.appimage:
        staged.append(stage(args.appimage, APPIMAGE_NAME))
    if args.deb:
        staged.append(stage(args.deb, DEB_NAME))

    # --- latest.json: OTA manifest (only exists for a Windows artifact) ----
    if args.exe:
        exe = dist / EXE_NAME
        manifest = {
            "version": version,
            "url": f"https://github.com/{REPO}/releases/download/v{version}/{EXE_NAME}",
            "sha256": sha256_file(exe),
            "size": exe.stat().st_size,
        }
        if args.notes:
            manifest["notes"] = args.notes
        (dist / "latest.json").write_text(
            json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
        )
        staged.append(dist / "latest.json")

    if args.ci:
        print(f"staged: {[p.name for p in staged]}")
        return

    # --- the release command (printed, never run here) ----------------------
    assets = " ".join(f"dist/{p.name}" for p in staged)
    notes_arg = "--notes-file <notes-file>" if args.notes else "--notes <notes>"
    print(f"gh release create v{version} {assets} "
          f"--repo {REPO} --title \"Halftone v{version}\" {notes_arg}")

    # --notes-file form the CI workflow can use verbatim:
    if args.notes:
        Path("dist/notes.txt").write_text(args.notes + "\n", encoding="utf-8")
        print(f"gh release create v{version} {assets} "
              f"--repo {REPO} --title \"Halftone v{version}\" --notes-file dist/notes.txt")

    print(f"\nstaged: {[p.name for p in staged]}")
    print("verify before creating the release:")
    if args.exe:
        exe = dist / EXE_NAME
        print(f"  sha256({EXE_NAME}) = {sha256_file(exe)}")
        print(f"  size   = {exe.stat().st_size} bytes")


if __name__ == "__main__":
    main()
