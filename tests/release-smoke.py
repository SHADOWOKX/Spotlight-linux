#!/usr/bin/env python3
"""Verify and install the binary archive only into temporary XDG directories.

Run on tests/session-bus.conf and Xvfb; never on the user's session bus.
"""
import hashlib
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import time

repo = Path(__file__).resolve().parents[1]
archives = list((repo / "dist").glob("*.tar.gz"))
assert len(archives) == 1, archives
archive = archives[0]
expected, name = (repo / "dist/SHA256SUMS").read_text().split()
assert name == archive.name and hashlib.sha256(archive.read_bytes()).hexdigest() == expected
with tempfile.TemporaryDirectory(prefix="spotlight-release-test-") as directory:
    root = Path(directory)
    with tarfile.open(archive) as tar:
        for entry in tar.getmembers():
            assert entry.isfile() and not entry.issym() and not entry.islnk()
            assert entry.uid == entry.gid == 0
            assert not entry.mode & 0o6022
            assert not Path(entry.name).is_absolute() and ".." not in Path(entry.name).parts
        tar.extractall(root / "archive", filter="data")
    package = next((root / "archive").iterdir())
    env = os.environ.copy()
    for key, value in {"XDG_DATA_HOME": root / "data with spaces", "XDG_CONFIG_HOME": root / "config", "XDG_CACHE_HOME": root / "cache", "XDG_BIN_HOME": root / "bin with spaces", "XDG_DATA_DIRS": root / "empty-system-data", "XDG_RUNTIME_DIR": root / "run"}.items():
        value.mkdir(parents=True)
        env[key] = str(value)
    (root / "run").chmod(0o700)
    env["GSK_RENDERER"] = "cairo"
    def run(args, timeout=60):
        return subprocess.run(args, cwd=repo, env=env, check=True, text=True, capture_output=True, timeout=timeout).stdout
    # Refuse to disturb a resident instance even if invoked on the wrong bus.
    owner = run(["gdbus", "call", "--session", "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus", "--method", "org.freedesktop.DBus.NameHasOwner", "io.github.shadowokx.SpotlightLinux"])
    assert "false" in owner, "Use a private test bus with no Spotlight resident"
    run([str(package / "install.sh")])
    binary = Path(env["XDG_BIN_HOME"]) / "spotlight-linux"
    data = Path(env["XDG_DATA_HOME"])
    run(["desktop-file-validate", str(data / "applications/io.github.shadowokx.SpotlightLinux.desktop")])
    with (root / "resident.log").open("w") as log:
        proc = subprocess.Popen([str(binary), "--background"], env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            for _ in range(40):
                time.sleep(0.25)
                if "resident=true" in run([str(binary), "--diagnostics"]):
                    break
            else:
                raise AssertionError("resident did not initialize")
            print(run(["python3", str(repo / "tests/lifecycle-smoke.py"), str(binary)], 90))
            history = data / "spotlight-linux/history.sqlite3"
            assert history.exists() and history.stat().st_mode & 0o777 == 0o600
            marker = Path(env["XDG_CONFIG_HOME"]) / "synthetic-user-data"
            marker.write_text("keep")
            run([str(package / "uninstall.sh")])
            proc.wait(timeout=10)
            assert not binary.exists()
            assert not (data / "applications/io.github.shadowokx.SpotlightLinux.desktop").exists()
            assert history.exists() and marker.read_text() == "keep"
        finally:
            if proc.poll() is None:
                if binary.exists():
                    run([str(binary), "--quit"])
                else:
                    proc.terminate()
                proc.wait(timeout=10)
    print("PASS: archive checksum/permissions, binary install, lifecycle, uninstall, private history and user-data preservation")
