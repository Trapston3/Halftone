#!/usr/bin/env python3
"""Halftone self-contained installer (developer machine, per-user):

- copies release exe to C:\\Users\\traps\\Halftone\\Halftone.exe (local app dir)
- adds the dir to the USER PATH
- registers App Paths\\Halftone.exe so Win+R "halftone" works
- creates Desktop + Start Menu shortcuts named "Halftone"
- sets HalftoneDir env var
Idempotent: re-running updates files, never duplicates.
"""
import os, shutil, subprocess, sys
from pathlib import Path

HOME = Path(os.environ["USERPROFILE"])          # C:\Users\traps
DEST_DIR = HOME / "Halftone"
SRC = Path(r"C:\Users\traps\Projects\halftone\src-tauri\target\release\halftone.exe")
DEST = DEST_DIR / "Halftone.exe"

def real_desktop():
    out = subprocess.run(
        ["reg", "query",
         r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders",
         "/v", "Desktop"], capture_output=True, text=True).stdout
    for line in out.splitlines():
        if "Desktop" in line:
            p = line.split("REG_EXPAND_SZ")[-1].strip()
            return Path(os.path.expandvars(p))
    return HOME / "Desktop"

DESKTOP = real_desktop()
STARTMENU = HOME / "AppData" / "Roaming" / "Microsoft" / "Windows" / "Start Menu" / "Programs"

def reg(path_, name, value, vtype="REG_SZ"):
    subprocess.run(["reg", "add", path_, "/v", name, "/t", vtype, "/d", value, "/f"],
                   check=True, capture_output=True)

def shortcut(lnk, target):
    ps = ("$ws = New-Object -ComObject WScript.Shell;"
          f"$s = $ws.CreateShortcut('{lnk}');"
          f"$s.TargetPath = '{target}';"
          "$s.WorkingDirectory = '" + str(Path(target).parent) + "';"
          "$s.IconLocation = '" + str(target) + ",0';"
          "$s.Save()")
    subprocess.run(["powershell", "-NoProfile", "-Command", ps], check=True, capture_output=True)

def main():
    DEST_DIR.mkdir(exist_ok=True)
    shutil.copy2(SRC, DEST)
    print("exe ->", DEST, f"({DEST.stat().st_size/1e6:.1f} MB)")

    # App Paths: Win+R / ShellExecute "halftone"
    reg(r"HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\Halftone.exe",
        "", str(DEST))
    print("App Paths registered")

    # USER PATH
    cur = subprocess.run(["reg", "query", r"HKCU\Environment", "/v", "Path"],
                         capture_output=True, text=True).stdout
    have = str(DEST_DIR) in cur
    if not have:
        # append without touching other entries (REG_EXPAND_SZ preserved)
        existing = cur.split("Path    REG_EXPAND_SZ    ")[-1].strip() if "REG_EXPAND_SZ" in cur else ""
        newpath = (existing.rstrip(";") + ";" if existing else "") + str(DEST_DIR)
        reg(r"HKCU\Environment", "Path", newpath, "REG_EXPAND_SZ")
        print("PATH updated (new terminal sessions will see it)")
    else:
        print("PATH already contains Halftone dir")

    # env var
    reg(r"HKCU\Environment", "HalftoneDir", str(DEST_DIR))

    # shortcuts
    shortcut(str(DESKTOP / "Halftone.lnk"), str(DEST))
    shortcut(str(STARTMENU / "Halftone.lnk"), str(DEST))
    print("shortcuts: Desktop + Start Menu")

    print("INSTALL OK")

if __name__ == "__main__":
    main()
