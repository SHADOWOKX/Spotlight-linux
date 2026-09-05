#!/usr/bin/env python3
"""Create a user-local binary archive with neutral ownership and a checksum."""
import hashlib
import io
import os
from pathlib import Path
import platform
import re
import subprocess
import tarfile
import tomllib

root = Path(__file__).resolve().parents[1]
subprocess.run(["python3", str(root / "scripts/build-release.py")], check=True)
version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
name = f"spotlight-linux-{version}-linux-{platform.machine()}"
dist = root / "dist"
dist.mkdir(exist_ok=True)
binary = (root / "target/release/spotlight-linux").read_bytes()
# Reject leaked local usernames/paths before making a distributable artifact.
if re.search(rb"/(?:home|Users)/[^/\x00\s]+", binary):
    raise SystemExit("Release binary contains a home-directory path; do not publish it")
install = b'''#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
"$root/spotlight-linux" --install-user
printf '%s\\n' 'Installed. Open Spotlight Linux from GNOME Overview; configure your shortcut in Settings.'
'''
readme = f'''Spotlight Linux {version} — preview

Binary target: Ubuntu 26.04, {platform.machine()}. GTK/libadwaita and a graphical
session are required. Do not run as root. No Rust compiler is needed.

Install:   ./install.sh
Uninstall: ./uninstall.sh

Settings and launch history survive uninstall. Configure the portal shortcut in
Settings > Keyboard. Physical GNOME focus/login acceptance remains manual.
Source and documentation: https://github.com/SHADOWOKX/Spotlight-linux
License: GPL-3.0-or-later; see LICENSE.
'''.encode()
files = {"spotlight-linux": (binary, 0o755), "install.sh": (install, 0o755), "uninstall.sh": ((root / "scripts/uninstall-user.sh").read_bytes(), 0o755), "README.txt": (readme, 0o644), "LICENSE": ((root / "LICENSE").read_bytes(), 0o644)}
archive = dist / f"{name}.tar.gz"
epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "1788566400"))
with tarfile.open(archive, "w:gz") as tar:
    for path, (data, mode) in files.items():
        info = tarfile.TarInfo(f"{name}/{path}")
        info.size, info.mode, info.mtime = len(data), mode, epoch
        info.uid = info.gid = 0
        info.uname = info.gname = "root"
        tar.addfile(info, io.BytesIO(data))
(dist / "SHA256SUMS").write_text(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n")
print(archive)
